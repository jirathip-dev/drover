use super::ago;
use crate::api::{
    Answered, ChatRepo, ChatTab, Chip, FileRef, Handoff, Link, Msg, QueuedMsg, RunCard, ThreadPage, ThreadRow,
    ThreadView,
};
use crate::conductor::{self, of};
use crate::config::Config;
use crate::events::{ask_event, closed, Event, Json};
use crate::runner::Runner;
use crate::session::{Session, Status};
use crate::Ctx;
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn or<'a>(c: bool, a: &'a str, b: &'a str) -> &'a str {
    if c {
        a
    } else {
        b
    }
}

/// The files a message carries.
pub fn files_of(e: &Event) -> impl Iterator<Item = &Value> {
    e.data.get("files").and_then(Value::as_array).into_iter().flatten()
}

/// A file name's extension, lower case.
pub fn ext(name: &str) -> String {
    name.rsplit('.').next().unwrap_or_default().to_ascii_lowercase()
}

fn open_ask<'a>(events: &'a [Event], id: &'a str) -> Option<&'a Event> {
    let lane = |e: &Event| (e.kind == "ask-owner").then(|| conductor::lane_of(events, e)).flatten();
    of(events, id).find(|e| {
        let a = if e.kind == "needs-decision" { Some(*e) } else { lane(e) };
        a.is_some_and(|a| closed(events, a).is_none())
    })
}

fn change<'a>(ctx: &Ctx, events: &'a [Event], e: &Event) -> Option<&'a Event> {
    let id = e.str("change")?;
    let d = ask_event(events, "decision", &e.fleet, id)?;
    let by = d.str("by").unwrap_or_default();
    (by.starts_with("orchestrator-") && ctx.now < d.ts + crate::CHANGE).then_some(())?;
    ask_event(events, "needs-decision", &e.fleet, id)
}

fn qid(e: &Event) -> u64 {
    e.data.get("q").and_then(Value::as_u64).unwrap_or(e.seq)
}
fn qfiles(e: &Event) -> Vec<Value> {
    files_of(e).cloned().collect()
}

fn queued<'a>(events: &'a [Event], id: &'a str) -> BTreeMap<u64, &'a Event> {
    let mut q = BTreeMap::new();
    for e in of(events, id) {
        let n = qid(e);
        match e.kind.as_str() {
            "thread-queue" => drop(q.insert(n, e)),
            "thread-unqueue" => drop(q.remove(&n)),
            _ => {}
        }
    }
    q
}

fn state(events: &[Event], id: &str, agents: &[Session]) -> &'static str {
    let lane = of(events, id).rfind(|e| e.kind.starts_with("orchestrator-"));
    let a = lane.and_then(|l| agents.iter().find(|a| l.str("agent") == Some(a.name.as_str())));
    match lane {
        _ if open_ask(events, id).is_some() => "choose",
        Some(l) if l.kind == "orchestrator-sleep" => "asleep",
        Some(_) if a.is_some_and(Session::busy) => "working",
        Some(_) => "awake",
        None => "new",
    }
}

/// `[chat·N]`: the threads where a choice waits for the owner.
pub fn choosing(events: &[Event]) -> usize {
    let ids = conductor::threads(events).into_iter();
    ids.filter(|t| open_ask(events, t.str("thread").unwrap_or_default()).is_some())
        .count()
}

fn link(e: &Event) -> Option<Link> {
    conductor::link(e).map(|(w, t)| Link {
        w: w.into(),
        t: t.into(),
    })
}

fn ask(events: &[Event], a: &Event, m: Msg) -> Msg {
    let answer = ask_event(events, "decision", &a.fleet, a.str("id").unwrap_or_default()).map(|d| {
        let c = d.str("choice").unwrap_or_default();
        let opts = a.val("options").as_array().into_iter().flatten();
        Answered {
            n: (opts.filter_map(Value::as_str).zip(1..))
                .find(|o| o.0 == c)
                .map(|o| o.1),
            choice: c.into(),
            by: d.str("by").map(Into::into),
            ts: d.ts,
        }
    });
    Msg {
        open: answer.is_none() && closed(events, a).is_none(),
        routine: a.str("kind") == Some("routine"),
        ask: Some(super::v1::ask(a)),
        answer,
        ..m
    }
}

fn run_card(ctx: &Ctx, events: &[Event], e: &Event) -> RunCard {
    let s = |e: &Event, k: &str| e.str(k).unwrap_or_default().to_string();
    let id = s(e, "id");
    let set = crate::card::settled(events, &id);
    let done = events.iter().find(|d| d.kind == "run-done" && d.str("id") == Some(&id));
    let exp = e.ts + crate::card::TTL;
    let state = match set {
        Some(x) if x.val("approved") != true => "denied",
        Some(_) if done.is_none() => "running",
        Some(_) => "approved",
        None if ctx.now > exp => "expired",
        None => "open",
    };
    RunCard {
        id,
        cmd: s(e, "cmd"),
        why: s(e, "why"),
        blast: s(e, "blast"),
        host: s(e, "host"),
        dir: s(e, "dir"),
        sha: s(e, "sha"),
        exp,
        state: state.into(),
        by: set.and_then(|x| x.str("by")).map(Into::into),
        code: done.and_then(|d| d.val("code").as_i64()),
        tail: done.map(|d| s(d, "tail")).unwrap_or_default(),
    }
}

fn page(
    ctx: &Ctx,
    cfg: &Config,
    events: &[Event],
    t: &Event,
    (before, at, n): (u64, u64, usize),
    open: u64,
) -> ThreadPage {
    let id = t.str("thread").unwrap_or_default();
    let (mut msgs, mut run, sent) = (vec![], vec![], conductor::forwarded(events));
    let fold = |msgs: &mut Vec<Msg>, run: &mut Vec<Msg>| {
        let Some((seq, ts)) = run.first().map(|f| (f.seq, f.ts)) else {
            return;
        };
        if seq > open {
            return msgs.append(run);
        }
        let cmds = run.drain(..).map(|c| c.text).collect();
        msgs.push(Msg {
            seq,
            ts,
            kind: "fold".into(),
            cmds,
            ..Msg::default()
        });
    };
    for e in of(events, id).filter(|e| e.seq < before) {
        let s = |k| e.str(k).unwrap_or_default().to_string();
        let m = |kind: &str, text: String| Msg {
            seq: e.seq,
            ts: e.ts,
            kind: kind.into(),
            text,
            link: link(e),
            ..Msg::default()
        };
        let main = cfg.hosts.iter().find(|h| h.1.machine.is_none());
        let m = match (e.kind.as_str(), e.str("from").unwrap_or_default()) {
            ("thread-msg", _) if e.data.get("lane_ask").is_some() => continue,
            ("thread-msg", "system") => Msg {
                ask: change(ctx, events, e).map(super::v1::ask),
                open: change(ctx, events, e).is_some(),
                retry: e.str("line").is_some(),
                signin: e.str("signin").map(Into::into),
                host: main.map_or("local", |h| h.0.as_str()).into(),
                ..m("note", s("text"))
            },
            ("thread-msg", _) if s("text").starts_with("DECISION ") => continue,
            ("thread-handoff", _) => m("handoff", conductor::handoff_text(e)),
            ("thread-msg", f) => {
                let file = |f: &Value| FileRef {
                    name: f["name"].as_str().unwrap_or_default().into(),
                    size: f["size"].as_u64().unwrap_or(0),
                };
                let fwd = if sent.contains(&e.seq) { " (forwarded)" } else { "" };
                Msg {
                    from: f.into(),
                    files: files_of(e).map(file).collect(),
                    ..m("msg", s("text") + fwd)
                }
            }
            ("thread-cmd", _) => m("cmd", s("cmd")),
            ("needs-decision", _) => ask(events, e, m("ask", s("question"))),
            ("ask-owner", _) => {
                let Some(mut a) = conductor::lane_of(events, e).cloned() else {
                    continue;
                };
                a.data.insert("recommend".into(), e.val("recommend").clone());
                ask(events, &a, m("ask", s("question")))
            }
            ("config-propose" | "secret-request", _) => {
                let Some(m) = owner_card(ctx, events, e) else {
                    continue;
                };
                m
            }
            ("run-card", _) => {
                let run = run_card(ctx, events, e);
                Msg {
                    from: s("id"),
                    open: run.state == "open",
                    run: Some(run),
                    ..m("run", s("cmd"))
                }
            }
            ("orchestrator-start", _) if e.data.get("resumed") == Some(&json!(true)) => m("resumed", s("session")),
            ("thread-model", _) => m("note", conductor::picked(e)),
            ("orchestrator-sleep", _) if s("why") != "model" => m("sleep", String::new()),
            _ => continue,
        };
        if m.kind == "cmd" {
            run.push(m);
        } else {
            fold(&mut msgs, &mut run);
            msgs.push(m);
        }
    }
    fold(&mut msgs, &mut run);
    let pins: Vec<u64> = crate::propose::waiting(ctx, events, id).map(|e| e.seq).collect();
    if before != u64::MAX {
        msgs.retain(|m| !pins.contains(&m.seq));
    }
    let k = msgs.partition_point(|m| m.seq < at).saturating_sub(n);
    let dir = conductor::dir(ctx, cfg, id).display().to_string();
    let home = crate::config::home().display().to_string();
    ThreadPage {
        id: id.into(),
        archived: conductor::archived(events, id),
        older: msgs.get(k).filter(|_| k > 0).map(|m| m.seq),
        started: t.ts,
        dir: dir.strip_prefix(&home).map_or(dir.clone(), |d| format!("~{d}")),
        msgs: {
            let w = msgs.split_off(k);
            msgs.retain(|m| pins.contains(&m.seq));
            msgs.into_iter().chain(w).collect()
        },
    }
}

/// API v1 `thread/<t>?before=<seq>&n=30`: the page of messages before `before`.
pub fn older(ctx: &Ctx, events: &[Event], id: &str, q: &BTreeMap<String, String>) -> Option<ThreadPage> {
    let t = conductor::find(events, id)
        .ok()
        .or_else(|| conductor::attached(events, id))?;
    let cfg = Config::kept(ctx).ok()?;
    let before = q.get("before").and_then(|v| v.parse().ok()).unwrap_or(u64::MAX);
    let n = q.get("n").and_then(|v| v.parse().ok()).unwrap_or(30);
    Some(page(ctx, &cfg, events, t, (before, u64::MAX, n), u64::MAX))
}

fn thread(ctx: &Ctx, cfg: &Config, events: &[Event], t: &Event, (agents, at): (&[Session], u64)) -> ThreadView {
    let id = t.str("thread").unwrap_or_default();
    let st = state(events, id, agents);
    let me = |e: &&Event| e.kind == "thread-msg" && e.str("from") == Some("owner");
    let turn = of(events, id).rfind(me).map_or((0, t.ts), |e| (e.seq, e.ts));
    let open = if st == "working" { turn.0 } else { u64::MAX };
    let q = |(q, e): (&u64, &&Event)| QueuedMsg {
        q: *q,
        text: e.str("text").unwrap_or_default().into(),
    };
    let role = conductor::thread_role(cfg, events, t).ok().map(|r| r.1);
    let role = role.as_ref();
    let (h, model) = (
        role.map_or("", |r| r.harness.as_str()),
        role.and_then(|r| r.model.as_deref()),
    );
    let shown = or(st == "choose", "awake", or(st == "new", "", st));
    ThreadView {
        id: id.into(),
        title: conductor::title(events, id).into(),
        ds: super::join(&[t.fleet.as_str(), h, model.unwrap_or_default(), shown]),
        state: st.into(),
        waiting: waiting(ctx, events, id),
        since: (st == "working").then_some(turn.1),
        queued: queued(events, id).iter().map(q).collect(),
        chips: role.map(|r| chips(cfg, r)).unwrap_or_default(),
        page: page(ctx, cfg, events, t, (u64::MAX, at, 30), open),
    }
}

fn chip(name: &str, cur: Option<&str>, all: &[&str]) -> Chip {
    Chip {
        name: name.into(),
        cur: cur.map(Into::into),
        all: all.iter().map(ToString::to_string).collect(),
    }
}

fn chips(cfg: &Config, r: &crate::config::Role) -> Vec<Chip> {
    let mut models = conductor::harness_models(cfg, &r.harness);
    if !models.is_empty() {
        models.extend(r.model.as_deref().filter(|m| !models.contains(m)));
    }
    let effort = r.effort_or(&cfg.limits);
    let all = [
        chip("model", r.model.as_deref(), &models),
        chip("effort", effort.as_deref(), crate::providers::get(&r.harness).efforts()),
    ];
    all.into_iter()
        .filter(|c| c.all.len() > 1 || c.cur.is_none() && !c.all.is_empty())
        .collect()
}

fn waiting(ctx: &Ctx, events: &[Event], id: &str) -> Vec<String> {
    let w = crate::propose::waiting(ctx, events, id);
    w.filter_map(|e| e.str("id").map(Into::into)).collect()
}

fn row(ctx: &Ctx, events: &[Event], t: &Event, agents: &[Session]) -> (u64, ThreadRow) {
    let id = t.str("thread").unwrap_or_default();
    let mut msgs = of(events, id).filter(|e| matches!(e.kind.as_str(), "thread-msg" | "needs-decision"));
    let last = msgs.rfind(|e| e.str("from") != Some("system"));
    let at = last.map_or(t.ts, |e| e.ts);
    let line = last.map_or(String::new(), |e| {
        let who = or(e.str("from") == Some("owner"), "you", "conductor");
        format!("{who}: {}", e.str("text").or(e.str("question")).unwrap_or_default())
    });
    let lk: std::collections::BTreeSet<_> = of(events, id)
        .filter_map(|e| conductor::link(e).filter(|_| e.str("from") != Some("system")))
        .collect();
    let row = ThreadRow {
        id: id.into(),
        title: conductor::title(events, id).into(),
        age: ago(ctx.now, at),
        unread: conductor::unread(events, id),
        state: state(events, id, agents).into(),
        waiting: waiting(ctx, events, id),
        links: lk
            .into_iter()
            .map(|(w, t)| Link {
                w: w.into(),
                t: t.into(),
            })
            .collect(),
        line: line.lines().next().unwrap_or_default().into(),
    };
    (at, row)
}

pub fn tab(ctx: &Ctx, cfg: Result<&Config, &str>, events: &[Event], agents: &[Session], rw: bool) -> ChatTab {
    let mut tab = ChatTab {
        now: ctx.now,
        rw,
        ..ChatTab::default()
    };
    let cfg = match cfg {
        Ok(c) => c,
        Err(e) => {
            tab.error = Some(e.into());
            return tab;
        }
    };
    let threads = conductor::threads(events);
    let repos = std::iter::once(("all", "x")).chain(cfg.fleet.iter().map(|(n, f)| (n.as_str(), f.color(n))));
    for (repo, k) in repos {
        let (mut rows, mut arch) = (vec![], vec![]);
        for t in threads.iter().filter(|t| t.fleet == repo) {
            let on = conductor::archived(events, t.str("thread").unwrap_or_default());
            if on { &mut arch } else { &mut rows }.push(row(ctx, events, t, agents));
        }
        rows.sort_by_key(|r| std::cmp::Reverse(r.0));
        arch.sort_by_key(|r| std::cmp::Reverse(r.0));
        tab.repos.push(ChatRepo {
            repo: repo.into(),
            k: k.into(),
            rows: rows.into_iter().map(|r| r.1).collect(),
            archived: arch.into_iter().map(|r| r.1).collect(),
            harness: chip(
                "harness",
                conductor::defaults(cfg, repo).ok().map(|d| d.1.harness).as_deref(),
                &crate::providers::lane_names(),
            ),
        });
    }
    tab.threads = threads
        .iter()
        .map(|t| thread(ctx, cfg, events, t, (agents, u64::MAX)))
        .collect();
    tab
}

impl super::Server<'_> {
    pub(super) fn chat_tab(&self, ctx: &Ctx, rw: bool) -> Value {
        let agents = self.sessions(ctx.now).map(|s| s.running()).unwrap_or_default();
        let seen: Vec<_> = agents
            .iter()
            .map(|a| [a.name.as_str(), a.status().map_or("", Status::word)])
            .collect();
        let key = json!([crate::db::last_seq(&ctx.state), super::mtime(ctx), seen, ctx.now / 60]);
        let mut all = self.chat.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let [ro, wr] = &mut *all;
        let c = if rw { wr } else { ro };
        if c.0 != key {
            let (cfg, events) = match (Config::kept(ctx), crate::events::working(&ctx.state)) {
                (Ok(c), Ok(e)) => (Ok(c), e),
                (Err(e), _) | (_, Err(e)) => (Err(format!("{e:#}")), std::sync::Arc::default()),
            };
            *c = (
                key,
                json!(tab(ctx, cfg.as_ref().map_err(String::as_str), &events, &agents, rw)),
            );
        }
        c.1.clone()
    }
}

fn first(text: &str) -> String {
    let l = text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim();
    let cut: String = l.chars().take(60).collect();
    cut.replace(char::is_control, " ") + or(l.chars().count() > 60, "…", "")
}

pub fn file(ctx: &Ctx, events: &[Event], id: &str, name: &str, demo: bool) -> Option<std::path::PathBuf> {
    let f = of(events, id).rev().flat_map(files_of).find(|f| f["name"] == name);
    let p = std::path::Path::new(f.and_then(|f| f["path"].as_str())?);
    let fixture = p.components().all(|c| matches!(c, std::path::Component::Normal(_)));
    if demo && fixture {
        return Some(p.into());
    }
    conductor::inside(ctx, &Config::kept(ctx).ok()?, id, p)
}

pub fn post(
    ctx: &Ctx,
    r: &dyn Runner,
    f: &BTreeMap<String, String>,
    later: &mut Vec<(Event, String)>,
) -> Result<String> {
    let cfg = Config::kept(ctx)?;
    let events = crate::events::working(&ctx.state)?;
    let g = |k: &str| f.get(k).map_or("", String::as_str);
    let id = g("t");
    match g("op") {
        "new" | "say" | "queue" | "steer" => {
            let mut names = g("name").split('/').map(|n| n.replace(char::is_control, ""));
            let data = g("data").split(',').filter(|d| !d.is_empty());
            let decode = |d| {
                Ok((
                    names.next().unwrap_or_default(),
                    B64.decode(d).context("a file is not base64")?,
                ))
            };
            let files = data.map(decode).collect::<Result<Vec<_>>>()?;
            let all: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
            let all = all.join(", ");
            let text = or(g("text").trim().is_empty(), &all, g("text"));
            let new = g("op") == "new";
            let id = if new {
                conductor::role(&cfg, g("repo"))?;
                conductor::text_ok(text)?;
                conductor::files_ok(&files)?;
                conductor::new(
                    ctx,
                    &cfg,
                    g("repo"),
                    &first(text),
                    Some(g("harness")).filter(|h| !h.is_empty()),
                )?
            } else {
                conductor::find(&events, id)?;
                id.to_string()
            };
            let mut files = conductor::upload(ctx, &cfg, &id, &files)?;
            let q = g("q").parse::<u64>().ok();
            let old = q
                .and_then(|q| queued(&events, &id).remove(&q))
                .context("no such queued message");
            let mut text = text.to_string();
            if g("op") == "queue" {
                conductor::text_ok(&text)?;
                let mut d = json!({"thread": id, "text": text, "files": files});
                if let Ok(o) = old {
                    d.put("q", json!(q));
                    (d.at("files") == &json!([])).then(|| d.put("files", json!(qfiles(o))));
                }
                let t = conductor::find(&events, &id)?;
                crate::events::append(&ctx.state, &Event::new(ctx.now, &t.fleet, None, "thread-queue", d))?;
                return Ok("queued".into());
            }
            if g("op") == "steer" {
                if q.is_some() {
                    let o = old?;
                    text = o.str("text").unwrap_or_default().into();
                    files = qfiles(o);
                    unqueue(ctx, o, "sent")?;
                }
                stop(r, &events, &id);
            }
            later.push(conductor::post(ctx, &cfg, (&id, &text, &files), ("owner", false))?);
            Ok(if new { format!("{id} started") } else { "sent".into() })
        }
        "unqueue" => {
            let q = g("q").parse::<u64>().unwrap_or_default();
            unqueue(
                ctx,
                queued(&events, id).remove(&q).context("no such queued message")?,
                "removed",
            )?;
            Ok("removed".into())
        }
        "model" => {
            conductor::pick(ctx, &cfg, id, g("model"), g("effort"))?;
            Ok("picked".into())
        }
        "stop" => {
            conductor::find(&events, id)?;
            stop(r, &events, id);
            Ok("stopping".into())
        }
        "archive" | "restore" => {
            conductor::archive(ctx, &cfg, r, id, g("op") == "archive")?;
            Ok(format!("{id} {}d", g("op")))
        }
        "retry" => {
            let e = of(&events, id).find(|e| e.seq.to_string() == g("n"));
            let line = e.and_then(|e| e.str("line")).context("nothing to retry")?;
            later.push((conductor::find(&events, id)?.clone(), line.into()));
            Ok("retrying".into())
        }
        "secret" => crate::secret::settle(ctx, &cfg, r, (id, g("name")), (g("act") == "save").then(|| g("value"))),
        "read" => {
            conductor::mark_read(ctx, &cfg, &events, conductor::find(&events, id)?)?;
            Ok(String::new())
        }
        op => bail!("unknown thread op `{op}`"),
    }
}

fn unqueue(ctx: &Ctx, e: &Event, why: &str) -> Result<()> {
    let d = json!({"thread": e.str("thread"), "q": qid(e), "why": why});
    crate::events::append(&ctx.state, &Event::new(ctx.now, &e.fleet, None, "thread-unqueue", d))
}

fn stop(r: &dyn Runner, events: &[Event], id: &str) {
    let last = of(events, id).rfind(|e| e.kind.starts_with("orchestrator-"));
    if let Some(p) = last
        .filter(|e| e.kind == "orchestrator-start")
        .and_then(|e| e.str("pane"))
    {
        drop(crate::term::keys(r, p, "Escape"));
    }
}

pub fn flush(ctx: &Ctx, agents: &[Session]) -> Vec<(Event, String)> {
    let (Ok(cfg), Ok(events)) = (Config::kept(ctx), crate::events::working(&ctx.state)) else {
        return vec![];
    };
    let mut out = vec![];
    for t in conductor::threads(&events) {
        let id = t.str("thread").unwrap_or_default();
        for e in queued(&events, id)
            .values()
            .take_while(|_| state(&events, id, agents) != "working")
        {
            let files = qfiles(e);
            let sent = unqueue(ctx, e, "sent");
            let p = sent.and_then(|()| {
                conductor::post(
                    ctx,
                    &cfg,
                    (id, e.str("text").unwrap_or_default(), &files),
                    ("owner", false),
                )
            });
            out.extend(p.ok());
        }
    }
    out
}

pub fn deliver(ctx: &Ctx, r: &dyn Runner, (t, line): &(Event, String)) {
    let Ok(cfg) = Config::kept(ctx) else { return };
    if let Err(e) = conductor::deliver(ctx, &cfg, r, t, line) {
        let text = format!("✗ not delivered: {e:#}");
        let d = json!({"thread": t.str("thread"), "from": "system", "text": text, "line": line});
        drop(crate::events::emit(
            &ctx.state,
            &cfg,
            Event::new(ctx.now, &t.fleet, None, "thread-msg", d),
        ));
    }
}

fn figure(state: &str) -> Value {
    json!({"state": match state { "choose" => "needs", "working" => "working", _ => "idle" }})
}

fn row_block(r: &ThreadRow, repo: Option<&str>) -> Value {
    let (word, tone) = match r.state.as_str() {
        "choose" => ("! choose · ", "warn"),
        "working" => ("✻ working · ", ""),
        "asleep" => ("☾ asleep · ", ""),
        _ => ("", ""),
    };
    let sub = repo.map_or(format!("{word}{}", r.line), |f| format!("{f} · {}", r.line));
    let act = or(repo.is_some(), "restore", "archive");
    let mut v = json!({"type": "row", "id": r.id, "figure": figure(&r.state), "title": r.title,
        "subtitle": {"text": sub, "tone": tone}, "value": r.age, "tap": {"open": format!("/screens/threads/{}", r.id)},
        "swipe": [{"label": act, "action": format!("thread-{act}"), "values": {"thread": r.id}}]});
    if r.unread {
        v.put("badge", json!({"text": "new", "tone": "done"}));
    }
    v
}

pub fn list_screen(ctx: &Ctx, cfg: &Config, events: &[Event], agents: &[Session]) -> Value {
    let threads = conductor::threads(events);
    let (mut blocks, mut arch) = (vec![], vec![]);
    for repo in std::iter::once("all").chain(cfg.fleet.keys().map(String::as_str)) {
        let mut rows = vec![];
        for t in threads.iter().filter(|t| t.fleet == repo) {
            let on = conductor::archived(events, t.str("thread").unwrap_or_default());
            let r = row(ctx, events, t, agents);
            if on {
                arch.push((r.0, row_block(&r.1, Some(or(repo == "all", "all repos", repo)))));
            } else {
                rows.push((r.0, row_block(&r.1, None)));
            }
        }
        rows.sort_by_key(|r| std::cmp::Reverse(r.0));
        let mut rows: Vec<Value> = rows.into_iter().map(|r| r.1).collect();
        let older = rows.split_off(rows.len().min(3));
        let n = rows.len() + older.len();
        let name = or(repo == "all", "all repos", repo);
        let count = match n {
            0 => "no threads".into(),
            1 => "1 thread".into(),
            n => format!("{n} threads"),
        };
        let mut kids = vec![];
        if !rows.is_empty() {
            kids.push(json!({"type": "list", "id": format!("l-{repo}"), "rows": rows}));
        }
        if !older.is_empty() {
            kids.push(json!({"type": "card", "id": format!("older-{repo}"), "fold": {"line": format!("▸ {} older threads", older.len())},
                "blocks": [{"type": "list", "id": format!("l-older-{repo}"), "rows": older}]}));
        }
        kids.push(json!({"type": "button", "id": format!("new-{repo}"), "label": "+ thread", "tap": {"open": format!("/screens/threads/new/{repo}")}}));
        blocks.push(json!({"type": "section", "id": format!("r-{repo}"), "header": format!("{name} · {count}"), "blocks": kids}));
    }
    if !arch.is_empty() {
        arch.sort_by_key(|r| std::cmp::Reverse(r.0));
        let rows: Vec<Value> = arch.into_iter().map(|r| r.1).collect();
        blocks.push(
            json!({"type": "card", "id": "archived", "fold": {"line": format!("▸ {} archived", rows.len())},
            "blocks": [{"type": "list", "id": "l-archived", "rows": rows}]}),
        );
    }
    json!({"type": "screen", "id": "threads", "title": "Threads", "badge": choosing(events),
        "stream": "/screens/threads/stream", "blocks": blocks})
}

fn composer(label: &str, action: &str, values: &Value, more: Value) -> Value {
    let mut c = json!({"type": "composer", "id": "say", "label": label, "action": action, "values": values,
        "placeholder": "message the orchestrator"});
    if let (Some(o), Value::Object(m)) = (c.as_object_mut(), more) {
        o.extend(m.into_iter().filter(|(_, v)| !v.is_null()));
    }
    c
}

fn owner_card(ctx: &Ctx, events: &[Event], e: &Event) -> Option<Msg> {
    let s = |k| e.str(k).unwrap_or_default().to_string();
    let m = |kind: &str, from: String, text: String, open: bool| Msg {
        seq: e.seq,
        ts: e.ts,
        kind: kind.into(),
        from,
        text,
        open,
        ..Msg::default()
    };
    match e.kind.as_str() {
        "config-propose" if crate::propose::settled(events, &s("id")).is_none() => {
            Some(m("config", s("id"), s("diff"), !crate::propose::stale(ctx, e)))
        }
        "secret-request" => {
            let open = crate::secret::open(events, e.str("thread")?, &s("name"));
            open.is_some_and(|o| std::ptr::eq(o, e))
                .then(|| m("secret", s("name"), s("why"), false))
        }
        _ => None,
    }
}

pub(super) fn owner_cards(ctx: &Ctx, events: &[Event], paired: bool) -> Vec<Value> {
    let all = crate::events::of_kind(events, &["config-propose", "secret-request"]);
    let card = |e: &Event| {
        Some(msg_body(
            ctx.now,
            e.str("thread")?,
            &owner_card(ctx, events, e)?,
            paired,
            None,
        ))
    };
    all.into_iter().filter_map(card).collect()
}

fn msg_card(now: u64, id: &str, m: &Msg, paired: bool, h: Option<&Handoff>) -> Value {
    let (mut v, key) = (msg_body(now, id, m, paired, h), format!("m-{}", m.seq));
    let src = |f: &FileRef| format!("/thread/{id}/{}", f.name);
    let (imgs, docs): (Vec<&FileRef>, Vec<&FileRef>) = m
        .files
        .iter()
        .partition(|f| ["png", "jpg", "jpeg", "gif", "webp"].contains(&ext(&f.name).as_str()));
    let items: Vec<Value> = imgs.iter().map(|f| json!({"src": src(f), "alt": f.name})).collect();
    let rows: Vec<Value> = docs
        .iter()
        .enumerate()
        .map(|(i, f)| {
            json!({"type": "row", "id": format!("{key}-f{i}"), "title": f.name,
        "value": format!("{} KB", f.size.div_ceil(1024)), "tap": {"url": src(f)}})
        })
        .collect();
    let mut add = |k: &str, b: Value| match v.get_mut(k).and_then(Value::as_array_mut) {
        Some(a) => a.push(b),
        None => {
            v.put(k, json!([b]));
        }
    };
    if !items.is_empty() {
        add(
            "blocks",
            json!({"type": "images", "id": format!("{key}-i"), "items": items}),
        );
    }
    if !rows.is_empty() {
        add(
            "blocks",
            json!({"type": "list", "id": format!("{key}-f"), "rows": rows}),
        );
    }
    if let Some(l) = &m.link {
        add(
            "buttons",
            json!({"type": "button", "id": format!("{key}-link"), "label": format!("{} {}", l.w, l.t), "tap": {"thread": l.t}}),
        );
    }
    v
}

fn run_body(now: u64, key: &str, r: &RunCard) -> (Vec<Value>, Vec<Value>, &'static str) {
    let text = |k: &str, t: &str| json!({"type": "text", "id": format!("{key}-{k}"), "markdown": t});
    let host = or(r.host.is_empty(), "local", &r.host);
    if r.state == "open" {
        let sha = r.sha.get(..12).unwrap_or(&r.sha);
        let left = r.exp.saturating_sub(now).div_ceil(60);
        let run = json!({"type": "run", "id": r.id, "action": "run-card", "command": r.cmd, "sha": sha, "dir": r.dir, "touches": r.blast});
        return (
            vec![run, text("exp", &format!("on {host} · expires in {left}m"))],
            vec![],
            "warn",
        );
    }
    let by = r.by.as_deref().map_or(String::new(), |b| format!(" by {b}"));
    let code = r.code.map_or(String::new(), |c| format!(" · exit {c}"));
    let mut blocks = vec![
        text("cmd", &format!("```\n{}\n```", r.cmd)),
        text(
            "s",
            &format!(
                "on {host} · {} · touches {}  \n**{}**{by}{code}",
                r.dir, r.blast, r.state
            ),
        ),
    ];
    let mut buttons = vec![];
    if !r.tail.is_empty() {
        blocks.push(text("tail", &format!("```\n{}\n```", r.tail)));
        buttons.push(json!({"type": "button", "id": format!("{key}-copy"), "label": "copy", "tap": {"copy": r.tail}}));
    }
    (blocks, buttons, or(r.code.is_some_and(|c| c != 0), "bad", ""))
}

fn msg_body(now: u64, id: &str, m: &Msg, paired: bool, h: Option<&Handoff>) -> Value {
    let (key, age) = (format!("m-{}", m.seq), crate::events::age(now.saturating_sub(m.ts)));
    let text = |k: &str, t: &str| json!({"type": "text", "id": format!("{key}-{k}"), "markdown": t});
    let sys = |t: &str| json!({"type": "card", "id": key, "kind": "sys", "blocks": [text("t", t)]});
    let button = |label: &str, style: &str, action: &str, values: Value| json!({"type": "button", "id": format!("{key}-{}", label.replace(' ', "-")), "label": label, "style": style, "action": action, "values": values});
    match m.kind.as_str() {
        "msg" if m.from == "owner" => {
            json!({"type": "card", "id": key, "kind": "you", "title": "you", "meta": format!("✓ {age}"), "blocks": [text("t", &m.text)]})
        }
        "msg" => {
            let who = m
                .link
                .as_ref()
                .map_or("orchestrator".into(), |l| format!("{} {}", l.w, l.t));
            json!({"type": "card", "id": key, "kind": "orch", "title": who, "meta": age, "blocks": [text("t", &m.text)]})
        }
        "note" if m.retry => {
            json!({"type": "card", "id": key, "kind": "you", "tone": "bad", "title": "you", "meta": age,
            "blocks": [text("t", &m.text)],
            "buttons": [{"type": "button", "id": format!("{key}-retry"), "label": "retry", "style": "bad", "action": "thread-retry", "values": {"thread": id, "msg": m.seq}}]})
        }
        "cmd" => json!({"type": "card", "id": key, "kind": "tool", "fold": {"line": format!("$ {}", m.text)}}),
        "fold" => {
            let more = or(m.cmds.len() > 1, " …", "");
            let line = format!("$ {}{more}", m.cmds.first().map_or("", String::as_str));
            let all = format!("```\n{}\n```", m.cmds.join("\n"));
            json!({"type": "card", "id": key, "kind": "tool", "fold": {"line": line}, "blocks": [text("t", &all)]})
        }
        "ask" => {
            let Some(a) = &m.ask else { return sys(&m.text) };
            let mut blocks = vec![text("t", &a.question)];
            if let Some(d) = &m.answer {
                let by = d.by.as_deref().unwrap_or("you");
                let when = crate::events::age(now.saturating_sub(d.ts));
                blocks.push(text("a", &format!("✓ **{}** · {by} · {when}", d.choice)));
            } else if m.open {
                let opts: Vec<Value> = a.options.iter().map(|o| json!({"label": o, "value": o})).collect();
                let mut ch = json!({"type": "choice", "id": a.id, "action": "answer", "values": {"thread": id}, "options": opts,
                    "other": {"placeholder": "other: type an answer"}});
                let rec = a
                    .recommend
                    .and_then(|n| a.options.get(usize::try_from(n).ok()?.checked_sub(1)?));
                if let Some(r) = rec {
                    ch.put("recommended", json!(r));
                }
                if h.is_some() {
                    ch.put("values", json!({"thread": id, "fleet": a.fleet}));
                }
                blocks.push(ch);
            }
            let mut tone = or(m.open && m.answer.is_none(), "warn", "");
            let mut buttons = vec![];
            match h.map(|h| (h.state.as_str(), h.to.as_str(), h.error.as_deref())) {
                Some(("delivered", Some(to), _)) => buttons.push(json!({"type": "button", "id": format!("{key}-to"),
                    "label": format!("continued in {to}"), "tap": {"thread": to}})),
                Some(("failed", _, e)) => {
                    tone = "bad";
                    blocks.push(text("e", &format!("✗ not delivered: {}", e.unwrap_or_default())));
                }
                _ => {}
            }
            let mut c = json!({"type": "card", "id": key, "kind": "ask", "tone": tone,
                "title": format!("{}{}", a.id, or(m.routine, " · routine", "")), "meta": age, "blocks": blocks});
            if !buttons.is_empty() {
                c.put("buttons", json!(buttons));
            }
            c
        }
        "note" if m.open && m.ask.is_some() => {
            let Some(a) = &m.ask else { return sys(&m.text) };
            let opts: Vec<Value> = a.options.iter().map(|o| json!({"label": o, "value": o})).collect();
            let mins = crate::CHANGE / 60;
            json!({"type": "card", "id": key, "kind": "ask", "title": format!("{} · answered by the orchestrator", a.id), "meta": age,
                "blocks": [text("t", &m.text), text("c", &format!("change it within {mins} min of the answer:")),
                    {"type": "choice", "id": a.id, "action": "answer", "values": {"thread": id, "fleet": a.fleet}, "options": opts}]})
        }
        "note" if m.signin.is_some() => {
            let name = m.signin.as_deref().unwrap_or_default();
            let b = button(
                "sign in",
                "primary",
                "thread-signin",
                json!({"thread": id, "host": m.host, "name": name}),
            );
            json!({"type": "card", "id": key, "kind": "sys", "blocks": [text("t", &m.text)], "buttons": [b]})
        }
        "config" => {
            let v = |act: &str| json!({"thread": id, "id": m.from, "act": act});
            let mut buttons = vec![button("discard", "", "config-card", v("discard"))];
            if m.open {
                buttons.insert(0, button("apply", "primary", "config-card", v("apply")));
            }
            let sub = or(
                m.open,
                "a fleet.toml change for you to apply",
                "stale: fleet.toml changed since, it can no longer apply",
            );
            json!({"type": "card", "id": key, "kind": "config", "tone": or(m.open, "warn", "bad"), "title": format!("config proposal {}", m.from),
                "subtitle": sub, "meta": age, "blocks": [text("d", &format!("```diff\n{}\n```", m.text.trim_end()))], "buttons": buttons})
        }
        "secret" => {
            let mut blocks = vec![text("t", &m.text)];
            let mut buttons = vec![];
            if paired {
                let field = json!({"type": "secret", "name": "text", "label": "", "value": "", "placeholder": format!("value of {}", m.from)});
                blocks.push(json!({"type": "form", "id": format!("{key}-f"), "fields": [field],
                    "submit": {"label": "save", "action": "secret-card", "values": {"thread": id, "name": m.from, "act": "save"}}}));
                buttons.push(button(
                    "dismiss",
                    "",
                    "secret-card",
                    json!({"thread": id, "name": m.from, "act": "dismiss"}),
                ));
            } else {
                blocks.push(text("p", "pair this device to enter it"));
            }
            json!({"type": "card", "id": key, "kind": "secret", "tone": "warn", "title": format!("secret {}", m.from), "meta": age,
                "blocks": blocks, "buttons": buttons})
        }
        "run" => {
            let Some(r) = &m.run else { return sys(&m.text) };
            let (blocks, buttons, tone) = run_body(now, &key, r);
            json!({"type": "card", "id": key, "kind": "run", "tone": tone, "title": r.why, "meta": age, "blocks": blocks, "buttons": buttons})
        }
        "resumed" => sys(&format!("resumed session {}", m.text)),
        "sleep" => sys("the orchestrator went to sleep"),
        _ => sys(&m.text),
    }
}

pub fn thread_screen(
    ctx: &Ctx,
    cfg: &Config,
    events: &[Event],
    agents: &[Session],
    id: &str,
    (before, paired): (u64, bool),
) -> Option<Value> {
    let t = conductor::find(events, id).ok()?;
    let v = thread(ctx, cfg, events, t, (agents, before));
    let arch = v.page.archived;
    let act = or(arch, "restore", "archive");
    let (word, tone) = match v.state.as_str() {
        "choose" => ("! choose · ", "warn"),
        "working" => ("✻ working · ", ""),
        "new" => ("no orchestrator yet · ", ""),
        _ => ("", ""),
    };
    let sub = if v.waiting.is_empty() {
        json!({"text": format!("{word}{}", v.ds), "tone": tone})
    } else {
        json!({"text": format!("waiting: config {} · {word}{}", v.waiting.join(", "), v.ds), "tone": "warn"})
    };
    let mut blocks = vec![
        json!({"type": "row", "id": "who", "sticky": true, "figure": figure(&v.state), "title": format!("{} · {id}", v.title), "subtitle": sub}),
    ];
    blocks.extend(v.page.older.map(|n| {
        json!({"type": "button", "id": "older", "label": "↑ older messages",
        "tap": {"open": format!("/screens/threads/{id}?before={n}"), "keep": true}})
    }));
    let at = |m: &Msg| events.get(events.binary_search_by_key(&m.seq, |e| e.seq).ok()?);
    let handoff = |m: &Msg| {
        let e = at(m).filter(|e| m.kind == "ask" && e.data.get("handoff").is_some())?;
        Some(super::v1::handoff(events, e, true))
    };
    blocks.extend(
        v.page
            .msgs
            .iter()
            .map(|m| msg_card(ctx.now, id, m, paired, handoff(m).as_ref())),
    );
    if !arch {
        let working = v.since.map(|t| json!({ "since": super::screens::iso(t) }));
        blocks.push(composer(
            "send",
            "thread-say",
            &json!({ "thread": id }),
            json!({
            "acts": {"queue": "thread-queue", "steer": "thread-steer", "unqueue": "thread-unqueue",
                "stop": "thread-stop", "model": "thread-model"},
            "queued": v.queued, "chips": v.chips, "working": working}),
        ));
    }
    let q = if before == u64::MAX {
        String::new()
    } else {
        format!("?before={before}")
    };
    Some(
        json!({"type": "screen", "id": format!("thread-{id}"), "title": v.title, "focus": true,
        "stream": format!("/screens/threads/{id}/stream{q}"),
        "actions": [{"label": act, "action": format!("thread-{act}"), "values": {"thread": id}}], "blocks": blocks}),
    )
}

/// A new thread in `repo` (`/screens/threads/new/<repo>`): its first message starts it.
pub fn new_screen(cfg: &Config, repo: &str) -> Option<Value> {
    let role = conductor::defaults(cfg, repo).ok()?.1;
    let who = super::join(&[repo, role.harness.as_str(), role.model.as_deref().unwrap_or_default()]);
    let chips = [chip("harness", Some(&role.harness), &crate::providers::lane_names())];
    let c = composer(
        "start",
        "thread-new",
        &json!({ "repo": repo }),
        json!({ "chips": chips }),
    );
    let hint = "Your first message starts the thread and its orchestrator, and titles it with its first line. Nothing is logged until you send.";
    Some(
        json!({"type": "screen", "id": format!("thread-new-{repo}"), "title": "New thread", "focus": true, "blocks": [
        {"type": "row", "id": "who", "figure": {"state": "idle"}, "title": who, "subtitle": "no orchestrator yet"},
        {"type": "text", "id": "hint", "markdown": hint},
        c]}),
    )
}

impl super::Server<'_> {
    pub(super) fn threads_screen(&self, ctx: &Ctx, sub: &str, before: Option<&String>, paired: bool) -> Option<Value> {
        let (cfg, events) = (Config::kept(ctx).ok()?, crate::events::working(&ctx.state).ok()?);
        let agents = self.sessions(ctx.now).map(|s| s.running()).unwrap_or_default();
        match sub {
            "" => Some(list_screen(ctx, &cfg, &events, &agents)),
            s if s.starts_with("/new/") => new_screen(&cfg, &s[5..]),
            s => {
                let id = s.strip_prefix('/')?;
                if paired && self.addr.is_some() && self.export.is_none() {
                    drop(conductor::mark_read(
                        ctx,
                        &cfg,
                        &events,
                        conductor::find(&events, id).ok()?,
                    ));
                }
                let before = before.and_then(|b| b.parse().ok()).unwrap_or(u64::MAX);
                thread_screen(ctx, &cfg, &events, &agents, id, (before, paired))
            }
        }
    }
}
