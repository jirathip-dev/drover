//! #386: the `[chat]` tab (docs/design/conductor-chat/): per repo its conductor threads, and each thread as #138's chat
//! blocks with choice rows, images and files; the owner's writes (`POST /thread`) and the files (`GET /thread/<t>/<name>`).
use super::{ago, enc, esc};
use crate::conductor;
use crate::config::Config;
use crate::events::{closed, Event};
use crate::runner::Runner;
use crate::Ctx;
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{json, Value};
use std::collections::BTreeMap;

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
    let ids = events
        .iter()
        .filter(|e| e.kind == "thread-new")
        .filter_map(|t| t.str("thread"));
    ids.filter(|id| open_ask(events, id).is_some()).count()
}

/// Escaped prose: paragraphs at blank lines, `code`, and plain links.
fn prose(text: &str) -> String {
    let link = |w: &str| {
        let url = w.trim_end_matches(['.', ',', ')', ';', ':']);
        if url.starts_with("https://") || url.starts_with("http://") {
            format!("<a href=\"{url}\">{url}</a>{}", &w[url.len()..])
        } else {
            w.into()
        }
    };
    let para = |p: &str| {
        let p = esc(p.trim());
        let code = p.split('`').enumerate().map(|(i, s)| {
            if i % 2 == 1 {
                format!("<code>{s}</code>")
            } else {
                s.into()
            }
        });
        let p: String = code
            .collect::<String>()
            .split(' ')
            .map(link)
            .collect::<Vec<_>>()
            .join(" ");
        format!("<p>{}</p>", p.replace('\n', "<br>"))
    };
    text.split("\n\n").filter(|p| !p.trim().is_empty()).map(para).collect()
}

fn size(n: u64) -> String {
    match n {
        n if n < 1024 => format!("{n} B"),
        n if n < 1 << 20 => format!("{} KB", n >> 10),
        n => format!("{}.{} MB", n >> 20, ((n % (1 << 20)) * 10) >> 20),
    }
}

/// A message's files: an image inline at the bubble's width, any other file a row; both open `thread/<t>/<name>`.
fn files(id: &str, e: &Event) -> String {
    let each = |f: &Value| {
        let name = f["name"].as_str().unwrap_or_default();
        let (url, cap) = (
            format!("thread/{id}/{}", enc(name)),
            format!("{} · {}", esc(name), size(f["size"].as_u64().unwrap_or(0))),
        );
        let ext = name.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
        if ["png", "jpg", "jpeg", "gif", "webp"].contains(&ext.as_str()) {
            format!("<figure class=im><a href=\"{url}\"><img src=\"{url}\" alt=\"{}\" loading=lazy></a><figcaption>{cap}</figcaption></figure>", esc(name))
        } else {
            let s = size(f["size"].as_u64().unwrap_or(0));
            format!(
                "<a class=tf href=\"{url}\"><span>▤ {}</span><span class=m>{s}</span></a>",
                esc(name)
            )
        }
    };
    e.data
        .get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(each)
        .collect()
}

/// A block's time: the age, which serve.js turns into the local clock time.
fn tm(now: u64, ts: u64) -> String {
    format!("<span class=ct data-ts={ts}>{}</span>", ago(now, ts))
}

/// A message block (#138's: `you` in the accent colour, the conductor purple), open.
fn block(now: u64, k: &str, who: &str, pv: &str, body: &str, ts: u64) -> String {
    let pv = esc(pv.lines().next().unwrap_or_default());
    format!("<details class=\"blk {k}\" open><summary><span class=k>{who}</span><span class=pv>{pv}</span>{}<i class=cv></i></summary>{body}</details>", tm(now, ts))
}

/// An ask: its question, then the choice rows (recommended first) and an "other" box; answered, the ✓ line.
fn ask(ctx: &Ctx, events: &[Event], a: &Event, dis: &str) -> String {
    let s = |k| a.str(k).unwrap_or_default();
    let kind = if s("kind") == "routine" { " · routine" } else { "" };
    let mut body = format!(
        "<div class=md><p class=qid>{}{kind}</p>{}</div>",
        esc(s("id")),
        prose(s("question"))
    );
    let opts: Vec<&str> = a.data["options"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let d = events
        .iter()
        .rfind(|e| e.kind == "decision" && e.fleet == a.fleet && e.str("id") == a.str("id"));
    if let Some(d) = d {
        let c = d.str("choice").unwrap_or_default();
        let n = opts.iter().position(|o| *o == c);
        let what = n.map_or_else(
            || format!("other: \"{}\"", esc(c)),
            |n| format!("{}. {}", n + 1, esc(c)),
        );
        let who = match d.str("by") {
            Some("default") => "by: default".to_string(),
            Some("owner") | None => "you".into(),
            Some(b) => esc(b),
        };
        body += &format!(
            "<p class=ans>✓ {what} <span class=m>· {who} · {}</span></p>",
            tm(ctx.now, d.ts)
        );
    } else if closed(events, a).is_none() {
        let rec = a.data["recommend"].as_u64();
        let mut rows: Vec<_> = opts.iter().zip(1u64..).collect();
        rows.sort_by_key(|o| Some(o.1) != rec);
        let btn = |(o, n): (&&str, u64)| {
            let r = if Some(n) == rec {
                " <small>(recommended)</small>"
            } else {
                ""
            };
            format!(
                "<button type=button name=choice value={n}{dis}><span>{n}. {}{r}</span></button>",
                esc(o)
            )
        };
        let rows: String = rows.into_iter().map(btn).collect();
        body += &format!(
            "<form class=chq data-id=\"{}\" data-fleet=\"{}\">{rows}<div class=oth><input name=text aria-label=\"other answer\" placeholder=\"other: type an answer\"{dis}><button{dis}>send</button></div></form>",
            esc(s("id")),
            esc(&a.fleet)
        );
    }
    block(ctx.now, "ai", "conductor", s("question"), &body, a.ts)
}

/// One thread: the bar, its blocks, the live line and the composer (inert on a device that is not a paired owner's).
fn thread(ctx: &Ctx, cfg: &Config, events: &[Event], t: &Event, (agents, rw): (&[Value], bool)) -> String {
    let id = t.str("thread").unwrap_or_default();
    let dis = if rw { "" } else { " disabled" };
    let dir = conductor::dir(ctx, cfg, id).display().to_string();
    let home = crate::config::home().display().to_string();
    let dir = dir.strip_prefix(&home).map_or(dir.clone(), |d| format!("~{d}"));
    let mut h = format!(
        "<p class=qs>thread started · {} · conductor in {}</p>",
        tm(ctx.now, t.ts),
        esc(&dir)
    );
    for e in of(events, id) {
        let s = |k| e.str(k).unwrap_or_default();
        h += &match (e.kind.as_str(), s("from")) {
            ("thread-msg", "system") => format!("<p class=\"qs note\">{}</p>", esc(s("text"))),
            ("thread-msg", _) if s("text").starts_with("DECISION ") => String::new(),
            ("thread-msg", f) => {
                let (k, who) = if f == "conductor" { ("ai", "conductor") } else { ("me", "you") };
                let body = format!("<div class=md>{}</div>{}", prose(s("text")), files(id, e));
                block(ctx.now, k, who, s("text"), &body, e.ts)
            }
            ("thread-cmd", _) => format!("<details class=\"blk run\"><summary><span class=k>$</span><span class=cmd>{}</span><i class=cv></i></summary></details>", esc(s("cmd"))),
            ("needs-decision", _) => ask(ctx, events, e, dis),
            ("conductor-start", _) if e.data.get("resumed") == Some(&json!(true)) => {
                format!("<p class=qs>↻ resumed · same session ({:.4}…)</p>", esc(s("session")))
            }
            ("conductor-sleep", _) => "<p class=qs>☾ asleep after 30 min idle · writes wake it</p>".into(),
            _ => String::new(),
        };
    }
    let st = state(events, id, agents);
    let live = match st {
        "choose" => "<b>●</b> waiting for your answer · sleeps after 30 min idle",
        "working" => "<b>✻</b> working",
        "asleep" => "☾ asleep · a message wakes it with this conversation",
        "new" => "a message starts the conductor",
        _ => "<b class=g>●</b> awake · sleeps after 30 min idle",
    };
    let role = conductor::role(cfg, &t.fleet).ok().map(|r| r.1);
    let ds = [
        t.fleet.as_str(),
        role.map_or("", |r| r.harness.as_str()),
        role.and_then(|r| r.model.as_deref()).unwrap_or_default(),
        match st {
            "new" => "",
            "choose" => "awake",
            s => s,
        },
    ];
    let ds = super::join(&ds.map(str::trim));
    let dot = if matches!(st, "asleep" | "new") {
        ""
    } else {
        "<i class=dt></i>"
    };
    let guest = if rw {
        ""
    } else {
        "<p class=\"ns m\">pair this phone to answer or write</p>"
    };
    format!(
        "<div class=cvp data-t=\"{id}\"><div class=th><button type=button class=bk aria-label=\"back to threads\">‹</button><div><b>{}</b><span class=ds>{dot}{}</span></div></div>\
         <div class=tc>{h}</div><p class=tlv>{live}</p>{guest}<form class=tw data-t=\"{id}\"><input type=file hidden{dis}><button type=button class=add aria-label=\"attach an image or file\"{dis}>+</button>\
         <textarea name=text rows=1 aria-label=message placeholder=\"message the conductor\"{dis}></textarea><button{dis}>send</button></form></div>",
        esc(t.str("title").unwrap_or_default()),
        esc(&ds)
    )
}

/// A thread's row: unread dot, title, age; then its state (unless plain awake) and the last message.
fn row(ctx: &Ctx, events: &[Event], t: &Event, agents: &[Value]) -> (u64, String) {
    let id = t.str("thread").unwrap_or_default();
    let mut msgs = of(events, id).filter(|e| matches!(e.kind.as_str(), "thread-msg" | "needs-decision"));
    let last = msgs.rfind(|e| e.str("from") != Some("system"));
    let at = last.map_or(t.ts, |e| e.ts);
    let line = last.map_or(String::new(), |e| {
        let who = if e.str("from") == Some("owner") {
            "you"
        } else {
            "conductor"
        };
        format!("{who}: {}", e.str("text").or(e.str("question")).unwrap_or_default())
    });
    let st = match state(events, id, agents) {
        "choose" => "<b class=r>! choose</b>",
        "working" => "<b class=a>✻ working</b>",
        "asleep" => "<b class=m>☾ asleep</b>",
        _ => "",
    };
    let (rd, dot) = if conductor::unread(events, id) {
        ("", "●")
    } else {
        (" rd", "")
    };
    let html = format!(
        "<a class=\"tr{rd}\" href=#chat data-t=\"{id}\"><i class=ud>{dot}</i><span class=tt>{}</span><span class=ta>{}</span><span class=tp>{st}{}</span></a>",
        esc(t.str("title").unwrap_or_default()),
        ago(ctx.now, at),
        esc(line.lines().next().unwrap_or_default())
    );
    (at, html)
}

/// The tab: "all repos", then each fleet by name, a card each with its 3 newest threads (the rest fold) and
/// [+ thread]; then every thread's view (serve.js shows the open one).
pub fn tab(ctx: &Ctx, cfg: Option<&Config>, events: &[Event], agents: &[Value], rw: bool) -> String {
    let Some(cfg) = cfg else {
        return "<p class=r>cannot read fleet.toml</p>".into();
    };
    let threads: Vec<&Event> = events.iter().filter(|e| e.kind == "thread-new").collect();
    let repos = std::iter::once(("all", "x")).chain(cfg.fleet.iter().map(|(n, f)| (n.as_str(), f.color(n))));
    let mut list = String::new();
    for (repo, k) in repos {
        let mut rows: Vec<_> = threads
            .iter()
            .filter(|t| t.fleet == repo)
            .map(|t| row(ctx, events, t, agents))
            .collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.0));
        let n = rows.len();
        let count = match n {
            0 => "no threads".to_string(),
            1 => "1 thread".into(),
            n => format!("{n} threads"),
        };
        let name = if repo == "all" { "all repos" } else { repo };
        let older: String = rows.iter().skip(3).map(|r| r.1.as_str()).collect();
        let older = if n > 3 {
            format!("<details class=done><summary><span>{} older thread{}</span><i class=cv></i></summary>{older}</details>", n - 3, if n == 4 { "" } else { "s" })
        } else {
            String::new()
        };
        let new = if rw {
            format!(
                "<form class=nt data-repo=\"{}\" hidden><input name=title aria-label=\"thread title\" placeholder=\"title, e.g. dark-mode contrast pass\"><button>start</button><p class=ns>a conductor starts for {} with your [conductor] profile; it sleeps after 30 min idle</p></form>",
                esc(repo),
                esc(name)
            )
        } else {
            String::new()
        };
        let add = if rw {
            "<span class=btns><button type=button class=ntb>+ thread</button></span>"
        } else {
            ""
        };
        list += &format!(
            "<div class=\"fc k-{k}\"><div class=fh><span class=fn><b>{}</b><span class=ds>{count}</span></span>{add}</div>{new}{}{older}</div>",
            esc(name),
            rows.iter().take(3).map(|r| r.1.as_str()).collect::<String>()
        );
    }
    let views: String = threads
        .iter()
        .map(|t| thread(ctx, cfg, events, t, (agents, rw)))
        .collect();
    format!("<div class=clp>{list}</div>{views}")
}

/// The newest file named `name` a message in thread `id` carries (the owner's upload or the conductor's reply).
pub fn file(events: &[Event], id: &str, name: &str) -> Option<std::path::PathBuf> {
    let all = of(events, id)
        .rev()
        .flat_map(|e| e.data.get("files").and_then(Value::as_array).into_iter().flatten());
    all.into_iter()
        .find(|f| f["name"] == name)
        .and_then(|f| f["path"].as_str())
        .map(Into::into)
}

/// `POST /thread` from a paired owner device: `new` (start a thread and its conductor), `say` (with an attached
/// file, base64 `data`), `answer` (a choice row or the "other" box) or `read`.
pub fn post(ctx: &Ctx, r: &dyn Runner, f: &BTreeMap<String, String>) -> Result<String> {
    let cfg = Config::load(&ctx.config)?;
    let events = crate::events::read(&ctx.state)?;
    let g = |k: &str| f.get(k).map_or("", String::as_str);
    let (id, mut out) = (g("t"), vec![]);
    match g("op") {
        "new" => {
            conductor::role(&cfg, g("repo"))?;
            conductor::new(ctx, &cfg, &mut out, g("repo"), g("title"))?;
            let id = String::from_utf8_lossy(&out).trim().to_string();
            conductor::say(ctx, &cfg, r, (&id, g("title"), &[]), ("owner", false))?;
            Ok(format!("{id} started"))
        }
        "say" => {
            conductor::find(&events, id)?;
            let mut files = vec![];
            if !g("data").is_empty() {
                let name: String = g("name")
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect();
                let bytes = B64.decode(g("data")).context("the file is not base64")?;
                if name.trim_start_matches('.').is_empty() || bytes.len() > 10 << 20 {
                    return crate::gate::refused("BAD_VALUE", "a file has a name and is at most 10 MB");
                }
                let dir = conductor::dir(ctx, &cfg, id).join("uploads");
                std::fs::create_dir_all(&dir)?;
                let path = dir.join(format!("{}-{name}", ctx.now));
                std::fs::write(&path, &bytes)?;
                files.push(json!({"name": name, "path": path, "size": bytes.len()}));
            }
            let text = if g("text").trim().is_empty() {
                g("name")
            } else {
                g("text")
            };
            let agent = conductor::say(ctx, &cfg, r, (id, text, &files), ("owner", false))?;
            Ok(format!("sent to {agent}"))
        }
        "answer" => {
            let choice = if g("choice").is_empty() { g("text") } else { g("choice") };
            crate::answer(
                ctx,
                &cfg,
                r,
                &mut out,
                (g("fleet"), g("id")),
                choice,
                ("owner", Some("web")),
            )?;
            Ok(format!("answered {}", g("id")))
        }
        "read" => {
            conductor::mark_read(ctx, &cfg, &events, conductor::find(&events, id)?)?;
            Ok(String::new())
        }
        op => bail!("unknown thread op `{op}`"),
    }
}
