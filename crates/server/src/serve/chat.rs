//! #386: the `[chat]` tab (docs/design/conductor-chat/): per repo its conductor threads, and each thread as #138's chat
//! blocks with choice rows, images and files; the owner's writes (`POST /thread`) and the files (`GET /thread/<t>/<name>`).
use super::ago;
use crate::api::{Answered, ChatRepo, ChatTab, FileRef, Link, Msg, ThreadPage, ThreadRow, ThreadView};
use crate::conductor;
use crate::config::Config;
use crate::events::{closed, Event};
use crate::runner::Runner;
use crate::Ctx;
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// `a` if `c`, else `b`.
fn or<'a>(c: bool, a: &'a str, b: &'a str) -> &'a str {
    [b, a][usize::from(c)]
}

/// The files a message carries.
pub fn files_of(e: &Event) -> impl Iterator<Item = &Value> {
    e.data.get("files").and_then(Value::as_array).into_iter().flatten()
}

/// A file name's extension, lower case.
pub fn ext(name: &str) -> String {
    name.rsplit('.').next().unwrap_or_default().to_ascii_lowercase()
}

/// The thread's events, in order.
fn of<'a>(events: &'a [Event], id: &'a str) -> impl DoubleEndedIterator<Item = &'a Event> {
    events.iter().filter(move |e| e.str("thread") == Some(id))
}

/// Its open asks (`ask --thread`): they wait in the thread, never in the needs-you stack.
fn open_ask<'a>(events: &'a [Event], id: &'a str) -> Option<&'a Event> {
    of(events, id).find(|e| e.kind == "needs-decision" && closed(events, e).is_none())
}

/// `choose` (an open ask), `working` (mid-turn), `asleep`, `awake` or `new` (no conductor yet).
fn state(events: &[Event], id: &str, agents: &[Value]) -> &'static str {
    let lane = of(events, id).rfind(|e| e.kind.starts_with("conductor-"));
    let a = lane.and_then(|l| agents.iter().find(|a| l.data.get("agent") == Some(&a["name"])));
    match lane {
        _ if open_ask(events, id).is_some() => "choose",
        Some(l) if l.kind == "conductor-sleep" => "asleep",
        Some(_) if a.is_some_and(|a| a["agent_status"] == "working") => "working",
        Some(_) => "awake",
        None => "new",
    }
}

/// `[chat·N]`: the threads where a choice waits for the owner.
pub fn choosing(events: &[Event]) -> usize {
    let ids = events.iter().filter(|e| e.kind == "thread-new");
    ids.filter(|t| open_ask(events, t.str("thread").unwrap_or_default()).is_some())
        .count()
}

/// #413: a linked event's chip (`from t5`, `continued in t6`); a tap opens that thread (serve.js).
fn link(e: &Event) -> Option<Link> {
    conductor::link(e).map(|(w, t)| Link {
        w: w.into(),
        t: t.into(),
    })
}

/// An ask as its thread shows it: decided, the answer; else open (no `closed` event) for the choice rows.
fn ask(events: &[Event], a: &Event, m: Msg) -> Msg {
    let is = |e: &&Event| e.kind == "decision" && e.fleet == a.fleet && e.str("id") == a.str("id");
    let answer = events.iter().rfind(is).map(|d| {
        let c = d.str("choice").unwrap_or_default();
        let opts = a.data["options"].as_array().into_iter().flatten();
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

/// #408: the thread's last `n` messages before seq `before` (web/src/chat.ts draws each block, tagged `data-n`), under
/// `thread started …` or, older ones left, the marker serve.js loads them from (API v1 `thread/<t>?before=`).
fn page(ctx: &Ctx, cfg: &Config, events: &[Event], t: &Event, (before, n): (u64, usize)) -> ThreadPage {
    let id = t.str("thread").unwrap_or_default();
    let mut msgs = vec![];
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
        msgs.push(match (e.kind.as_str(), e.str("from").unwrap_or_default()) {
            // #387: a signed-out line's [sign in]: the harness's login in a setup terminal on the main host (#303).
            ("thread-msg", "system") => Msg {
                retry: e.str("line").is_some(),
                signin: e.str("signin").map(Into::into),
                host: main.map_or("local", |h| h.0.as_str()).into(),
                ..m("note", s("text"))
            },
            ("thread-msg", _) if s("text").starts_with("DECISION ") => continue,
            ("thread-handoff", _) => m("handoff", s("title")),
            ("thread-msg", f) => {
                let file = |f: &Value| FileRef {
                    name: f["name"].as_str().unwrap_or_default().into(),
                    size: f["size"].as_u64().unwrap_or(0),
                };
                Msg {
                    from: f.into(),
                    files: files_of(e).map(file).collect(),
                    ..m("msg", s("text"))
                }
            }
            ("thread-cmd", _) => m("cmd", s("cmd")),
            ("needs-decision", _) => ask(events, e, m("ask", s("question"))),
            ("config-propose", _) if crate::propose::settled(events, &s("id")).is_none() => Msg {
                from: s("id"),
                ..m("config", s("diff"))
            },
            ("secret-request", _)
                if crate::secret::open(events, id, &s("name")).is_some_and(|o| std::ptr::eq(o, e)) =>
            {
                Msg {
                    from: s("name"),
                    ..m("secret", s("why"))
                }
            }
            ("conductor-start", _) if e.data.get("resumed") == Some(&json!(true)) => m("resumed", s("session")),
            ("conductor-sleep", _) => m("sleep", String::new()),
            _ => continue,
        });
    }
    let k = msgs.len().saturating_sub(n);
    let dir = conductor::dir(ctx, cfg, id).display().to_string();
    let home = crate::config::home().display().to_string();
    ThreadPage {
        older: (k > 0).then(|| msgs[k].seq),
        started: t.ts,
        dir: dir.strip_prefix(&home).map_or(dir.clone(), |d| format!("~{d}")),
        msgs: msgs.split_off(k),
    }
}

/// #408: API v1 `thread/<t>?before=<seq>&n=30`: the page of messages before `before`.
pub fn older(ctx: &Ctx, events: &[Event], id: &str, q: &BTreeMap<String, String>) -> Option<ThreadPage> {
    let (cfg, t) = (Config::load(&ctx.config).ok()?, conductor::find(events, id).ok()?);
    let before = q.get("before").and_then(|v| v.parse().ok()).unwrap_or(u64::MAX);
    let n = q.get("n").and_then(|v| v.parse().ok()).unwrap_or(30);
    Some(page(ctx, &cfg, events, t, (before, n)))
}

/// One thread: the bar (fleet · harness · model · state), its last 30 messages and the live line's state.
fn thread(ctx: &Ctx, cfg: &Config, events: &[Event], t: &Event, agents: &[Value]) -> ThreadView {
    let id = t.str("thread").unwrap_or_default();
    let st = state(events, id, agents);
    let role = conductor::role(cfg, &t.fleet).ok().map(|r| r.1);
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
        page: page(ctx, cfg, events, t, (u64::MAX, 30)),
    }
}

/// A thread's row: unread, title, age; its state and the last message; the threads it links to (not its flow-back
/// lines), once each.
fn row(ctx: &Ctx, events: &[Event], t: &Event, agents: &[Value]) -> (u64, ThreadRow) {
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

/// #269: API v1's `chat`, the tab (web/src/chat.ts draws it): "all repos", then each fleet by name, a card each with
/// its threads newest first (the archived ones apart); then every thread's view (serve.js shows the open one).
pub fn tab(ctx: &Ctx, cfg: Option<&Config>, events: &[Event], agents: &[Value], rw: bool) -> ChatTab {
    let mut tab = ChatTab {
        now: ctx.now,
        rw,
        ..ChatTab::default()
    };
    let Some(cfg) = cfg else {
        tab.error = Some("cannot read fleet.toml".into());
        return tab;
    };
    let threads: Vec<&Event> = events.iter().filter(|e| e.kind == "thread-new").collect();
    let repos = std::iter::once(("all", "x")).chain(cfg.fleet.iter().map(|(n, f)| (n.as_str(), f.color(n))));
    for (repo, k) in repos {
        let (mut rows, mut arch) = (vec![], vec![]);
        for t in threads.iter().filter(|t| t.fleet == repo) {
            let on = conductor::archived(events, t.str("thread").unwrap_or_default());
            [&mut rows, &mut arch][usize::from(on)].push(row(ctx, events, t, agents));
        }
        rows.sort_by_key(|r| std::cmp::Reverse(r.0));
        arch.sort_by_key(|r| std::cmp::Reverse(r.0));
        tab.repos.push(ChatRepo {
            repo: repo.into(),
            k: k.into(),
            rows: rows.into_iter().map(|r| r.1).collect(),
            archived: arch.into_iter().map(|r| r.1).collect(),
        });
    }
    tab.threads = threads.iter().map(|t| thread(ctx, cfg, events, t, agents)).collect();
    tab
}

impl super::Server<'_> {
    /// #269: the [chat] tab's data for a device that writes (`rw`) or only reads.
    pub(super) fn chat_tab(&self, ctx: &Ctx, rw: bool) -> ChatTab {
        let events = crate::events::read(&ctx.state).unwrap_or_default();
        let agents = self.herdr_json(ctx.now, "agent");
        let agents = agents
            .and_then(|j| j["result"]["agents"].as_array().cloned())
            .unwrap_or_default();
        tab(ctx, Config::load(&ctx.config).ok().as_ref(), &events, &agents, rw)
    }
}

/// #405: a new thread's title: its first message's first line, cut to one row.
fn first(text: &str) -> String {
    let l = text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim();
    let cut: String = l.chars().take(60).collect();
    cut.replace(char::is_control, " ") + or(l.chars().count() > 60, "…", "")
}

/// The newest file named `name` a message in thread `id` carries (the owner's upload or the conductor's reply), if
/// it lies in the thread's conductor folder; `demo` (the demo export) also takes the repo's relative fixtures.
pub fn file(ctx: &Ctx, events: &[Event], id: &str, name: &str, demo: bool) -> Option<std::path::PathBuf> {
    let f = of(events, id).rev().flat_map(files_of).find(|f| f["name"] == name);
    let p = std::path::Path::new(f.and_then(|f| f["path"].as_str())?);
    let fixture = p.components().all(|c| matches!(c, std::path::Component::Normal(_)));
    if demo && fixture {
        return Some(p.into());
    }
    conductor::inside(ctx, &Config::load(&ctx.config).ok()?, id, p)
}

/// `POST /thread` from a paired owner device: `new` (#405: a first message starts a thread and its conductor), `say`
/// (#405: with files, `name`s `/`-separated, base64 `data` `,`-separated), #405's `archive` and `restore`, `answer`
/// (a choice row or the "other" box), #387's `config` ([apply]/[discard]) and `secret` ([save] with `value`, or
/// [dismiss]), `read`, or #403's `retry` (a red system line's [retry], its event `n`).
/// #403: `new`, `say` and `retry` only log; `later` gets the line `deliver` types once the reply is out.
pub fn post(
    ctx: &Ctx,
    r: &dyn Runner,
    f: &BTreeMap<String, String>,
    later: &mut Vec<(Event, String)>,
) -> Result<String> {
    let cfg = Config::load(&ctx.config)?;
    let events = crate::events::read(&ctx.state)?;
    let g = |k: &str| f.get(k).map_or("", String::as_str);
    let (id, mut out) = (g("t"), vec![]);
    match g("op") {
        // #405: `new` is a first message (in `repo`): the thread, titled with its first line, then the message.
        "new" | "say" => {
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
                conductor::new(ctx, &cfg, g("repo"), &first(text))?
            } else {
                conductor::find(&events, id)?;
                id.to_string()
            };
            let files = conductor::upload(ctx, &cfg, &id, &files)?;
            later.push(conductor::post(ctx, &cfg, (&id, text, &files), ("owner", false))?);
            Ok(if new { format!("{id} started") } else { "sent".into() })
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
        "answer" => {
            let (choice, by) = (
                or(g("choice").is_empty(), g("text"), g("choice")),
                ("owner", Some("web")),
            );
            crate::answer(ctx, &cfg, r, &mut out, (g("fleet"), g("id")), choice, by)?;
            Ok(format!("answered {}", g("id")))
        }
        "config" => crate::propose::settle(ctx, &cfg, r, g("id"), g("act") == "apply"),
        "secret" => crate::secret::settle(ctx, &cfg, r, (id, g("name")), (g("act") == "save").then(|| g("value"))),
        "read" => {
            conductor::mark_read(ctx, &cfg, &events, conductor::find(&events, id)?)?;
            Ok(String::new())
        }
        op => bail!("unknown thread op `{op}`"),
    }
}

/// #403: type a logged message into its conductor (after the reply went out); a failure is a red system line whose
/// [retry] types it again.
pub fn deliver(ctx: &Ctx, r: &dyn Runner, (t, line): &(Event, String)) {
    let Ok(cfg) = Config::load(&ctx.config) else { return };
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
