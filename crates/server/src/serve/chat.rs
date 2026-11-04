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

/// #409: prose; between fence lines (to the end if unclosed) a box with [copy] of the block's exact text.
fn prose(text: &str) -> String {
    let mut seg = vec![String::new()];
    for l in text.split('\n') {
        if l.trim_start().starts_with("```") {
            seg.push(String::new());
        } else if let Some(s) = seg.last_mut() {
            *s += &format!("{l}\n");
        }
    }
    let cb = |c: &str| format!("<div class=cb><pre>{c}</pre><button type=button class=cpy>copy</button></div>");
    let html = |(i, s): (usize, &String)| match i % 2 {
        0 => paras(s),
        _ => cb(&esc(s.strip_suffix('\n').unwrap_or(s))),
    };
    seg.iter().enumerate().map(html).collect()
}

/// Escaped prose: paragraphs at blank lines, `code`, and plain links.
fn paras(text: &str) -> String {
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
        let code = |(i, s): (usize, &str)| or(i % 2 == 1, &format!("<code>{s}</code>"), s).to_string();
        let p: String = p.split('`').enumerate().map(code).collect();
        let p: Vec<String> = p.split(' ').map(link).collect();
        let p = p.join(" ");
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

/// A message's files: an image inline at the bubble's width (#406: a tap opens serve.js's viewer, never a page),
/// any other file a row that opens `thread/<t>/<name>`.
fn files(id: &str, e: &Event) -> String {
    let each = |f: &Value| {
        let name = f["name"].as_str().unwrap_or_default();
        let (url, n) = (
            format!("thread/{id}/{}", enc(name)),
            size(f["size"].as_u64().unwrap_or(0)),
        );
        let cap = format!("{} · {n}", esc(name));
        if ["png", "jpg", "jpeg", "gif", "webp"].contains(&ext(name).as_str()) {
            format!(
                "<figure class=im><img src=\"{url}\" alt=\"{}\" loading=lazy><figcaption>{cap}</figcaption></figure>",
                esc(name)
            )
        } else {
            format!(
                "<a class=tf href=\"{url}\"><span>▤ {}</span><span class=m>{n}</span></a>",
                esc(name)
            )
        }
    };
    files_of(e).map(each).collect()
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
    let (kind, q) = (or(s("kind") == "routine", " · routine", ""), prose(s("question")));
    let mut body = format!("<div class=md><p class=qid>{}{kind}</p>{q}</div>", esc(s("id")));
    let opts = a.data["options"].as_array().into_iter().flatten();
    let opts: Vec<&str> = opts.filter_map(Value::as_str).collect();
    let is = |e: &&Event| e.kind == "decision" && e.fleet == a.fleet && e.str("id") == a.str("id");
    let d = events.iter().rfind(is);
    if let Some(d) = d {
        let c = d.str("choice").unwrap_or_default();
        let n = opts.iter().position(|o| *o == c);
        let other = || format!("other: \"{}\"", esc(c));
        let what = n.map_or_else(other, |n| format!("{}. {}", n + 1, esc(c)));
        let who = match d.str("by") {
            Some("default") => "by: default".to_string(),
            Some("owner") | None => "you".into(),
            Some(b) => esc(b),
        };
        let at = tm(ctx.now, d.ts);
        body += &format!("<p class=ans>✓ {what} <span class=m>· {who} · {at}</span></p>");
    } else if closed(events, a).is_none() {
        let rec = a.data["recommend"].as_u64();
        let mut rows: Vec<_> = opts.iter().zip(1u64..).collect();
        rows.sort_by_key(|o| Some(o.1) != rec);
        let btn = |(o, n): (&&str, u64)| {
            let (r, o) = (or(Some(n) == rec, " <small>(recommended)</small>", ""), esc(o));
            format!("<button type=button name=choice value={n}{dis}><span>{n}. {o}{r}</span></button>")
        };
        let rows: String = rows.into_iter().map(btn).collect();
        let (q, f) = (esc(s("id")), esc(&a.fleet));
        body += &format!(
            "<form class=chq data-id=\"{q}\" data-fleet=\"{f}\">{rows}<div class=oth><input name=text aria-label=\"other answer\" placeholder=\"other: type an answer\"{dis}><button{dis}>send</button></div></form>"
        );
    }
    block(ctx.now, "ai", "conductor", s("question"), &body, a.ts)
}

/// #408: the thread's last `n` messages before seq `before`, each block tagged `data-n` (its event's seq), under
/// `thread started …` or, older ones left, the marker serve.js loads them from (`thread/<t>?before=`).
fn page(ctx: &Ctx, cfg: &Config, events: &[Event], t: &Event, rw: bool, (before, n): (u64, usize)) -> String {
    let id = t.str("thread").unwrap_or_default();
    let dis = or(rw, "", " disabled");
    let mut h = vec![];
    for e in of(events, id).filter(|e| e.seq < before) {
        let s = |k| e.str(k).unwrap_or_default();
        let lk = conductor::link(e).map_or(String::new(), |l| chip(l) + " ");
        let b = match (e.kind.as_str(), s("from")) {
            ("thread-msg", "system") => {
                let rt = format!(" <button type=button class=rty data-n={}{dis}>retry</button>", e.seq);
                let (r, rt) = if e.str("line").is_some() { (" r", rt) } else { ("", String::new()) };
                format!("<p class=\"qs note{r}\">{lk}{}{}{rt}</p>", esc(s("text")), signin(cfg, e, dis))
            }
            ("thread-msg", _) if s("text").starts_with("DECISION ") => String::new(),
            ("thread-handoff", _) => format!("<p class=qs>{lk}· {}</p>", esc(s("title"))),
            ("thread-msg", f) => {
                let (k, who) = match f {
                    "conductor" => ("ai", "conductor"),
                    _ if !lk.is_empty() => ("ai", lk.trim_end()),
                    _ => ("me", "you"),
                };
                let body = format!("<div class=md>{}</div>{}", prose(s("text")), files(id, e));
                block(ctx.now, k, who, s("text"), &body, e.ts)
            }
            ("thread-cmd", _) => format!("<details class=\"blk run\"><summary><span class=k>$</span><span class=cmd>{}</span><i class=cv></i></summary></details>", esc(s("cmd"))),
            ("needs-decision", _) => ask(ctx, events, e, dis),
            ("config-propose", _) if crate::propose::settled(events, s("id")).is_none() => {
                let (q, d) = (esc(s("id")), esc(s("diff")));
                let b = |v: &str| format!("<button name=act value={v}{dis}>{v}</button>");
                let body = format!("<div class=md><p class=qid>config {q}</p><pre class=df>{d}</pre></div><form class=chc data-id=\"{q}\">{}{}</form>", b("apply"), b("discard"));
                block(ctx.now, "ai", "conductor", &format!("proposes config {}", s("id")), &body, e.ts)
            }
            ("secret-request", _) if crate::secret::open(events, id, s("name")).is_some_and(|o| std::ptr::eq(o, e)) => {
                let (n, why) = (esc(s("name")), prose(s("why")));
                let form = format!("<form class=chs data-t=\"{id}\" data-name=\"{n}\"><input type=password name=value autocomplete=off aria-label=\"value of {n}\" placeholder=\"paste the value\"><button name=act value=save>save</button><button name=act value=dismiss>dismiss</button></form>");
                let form = if rw { form } else { "<p class=m>only a paired owner device can enter it</p>".into() };
                let body = format!("<div class=md><p class=qid>secret {n} · stored in the keychain, never in the chat</p>{why}</div>{form}");
                block(ctx.now, "ai", "conductor", &format!("asks for secret {}", s("name")), &body, e.ts)
            }
            ("conductor-start", _) if e.data.get("resumed") == Some(&json!(true)) => {
                format!("<p class=qs>↻ resumed · same session ({:.4}…)</p>", esc(s("session")))
            }
            ("conductor-sleep", _) => "<p class=qs>☾ asleep after 30 min idle · writes wake it</p>".into(),
            _ => String::new(),
        };
        if let Some(i) = b.find(' ') {
            h.push((e.seq, format!("{} data-n={}{}", &b[..i], e.seq, &b[i..])));
        }
    }
    let k = h.len().saturating_sub(n);
    let top = match h.get(k) {
        Some((s, _)) if k > 0 => format!("<p class=\"qs tmo\" data-b={s}>↑ older messages</p>"),
        _ => {
            let dir = conductor::dir(ctx, cfg, id).display().to_string();
            let home = crate::config::home().display().to_string();
            let dir = dir.strip_prefix(&home).map_or(dir.clone(), |d| format!("~{d}"));
            let (at, dir) = (tm(ctx.now, t.ts), esc(&dir));
            format!("<p class=qs>thread started by owner · {at} · conductor in {dir}</p>")
        }
    };
    h[k..].iter().fold(top, |a, b| a + &b.1)
}

/// #408: `thread/<t>?before=<seq>&n=30`: the page of messages before `before`.
pub fn older(ctx: &Ctx, events: &[Event], id: &str, q: &BTreeMap<String, String>, rw: bool) -> Option<String> {
    let (cfg, t) = (Config::load(&ctx.config).ok()?, conductor::find(events, id).ok()?);
    let before = q.get("before").and_then(|v| v.parse().ok()).unwrap_or(u64::MAX);
    let n = q.get("n").and_then(|v| v.parse().ok()).unwrap_or(30);
    Some(page(ctx, &cfg, events, t, rw, (before, n)))
}

/// One thread: the bar, its last 30 messages, the live line and the composer (inert off a paired owner device).
fn thread(ctx: &Ctx, cfg: &Config, events: &[Event], t: &Event, (agents, rw): (&[Value], bool)) -> String {
    let id = t.str("thread").unwrap_or_default();
    let dis = or(rw, "", " disabled");
    let h = page(ctx, cfg, events, t, rw, (u64::MAX, 30));
    let st = state(events, id, agents);
    let live = match st {
        "choose" => "<b>●</b> waiting for your answer · sleeps after 30 min idle",
        "working" => "<i class=tld aria-hidden=true></i> working", // #403: serve.js puts the theme's loading art in
        "asleep" => "☾ asleep · a message wakes it with this conversation",
        "new" => "a message starts the conductor",
        _ => "<b class=g>●</b> awake · sleeps after 30 min idle",
    };
    let role = conductor::role(cfg, &t.fleet).ok().map(|r| r.1);
    let (h2, model) = (
        role.map_or("", |r| r.harness.as_str()),
        role.and_then(|r| r.model.as_deref()),
    );
    let shown = or(st == "choose", "awake", or(st == "new", "", st));
    let ds = esc(&super::join(&[t.fleet.as_str(), h2, model.unwrap_or_default(), shown]));
    let title = esc(conductor::title(events, id));
    let dot = or(matches!(st, "asleep" | "new"), "", "<i class=dt></i>");
    let guest = or(rw, "", "<p class=\"ns m\">pair this phone to answer or write</p>");
    format!(
        "<div class=cvp data-t=\"{id}\"><div class=th><button type=button class=bk aria-label=\"back to threads\">‹</button><div><b>{title}</b><span class=ds>{dot}{ds}</span></div></div>\
         <div class=tc>{h}</div><p class=tlv>{live}</p>{guest}{}</div>",
        composer(id, "", dis)
    )
}

/// The composer: [+] attaches files (#405: several, up to 10), the box, [send].
fn composer(id: &str, repo: &str, dis: &str) -> String {
    format!(
        "<form class=tw data-t=\"{id}\"{repo}><input type=file multiple hidden{dis}><button type=button class=add aria-label=\"attach images or files\"{dis}>+</button>\
         <textarea name=text rows=1 aria-label=message placeholder=\"message the conductor\"{dis}></textarea><button{dis}>send</button></form>"
    )
}

/// #405: [+ thread]'s empty thread `new-<repo>`: nothing is logged until its first message starts it (`op=new`).
fn draft(repo: &str, dis: &str) -> String {
    let r = esc(repo);
    format!(
        "<div class=cvp data-t=\"new-{r}\"><div class=th><button type=button class=bk aria-label=\"back to threads\">‹</button><div><b>new thread</b><span class=ds>{}</span></div></div>\
         <div class=tc></div><p class=tlv>your first message starts it; its first line is the title</p>{}</div>",
        or(repo == "all", "all repos", &r),
        composer(&format!("new-{r}"), &format!(" data-repo=\"{r}\""), dis)
    )
}

/// #413: a linked row's chip (`from t5`, `continued in t6`); a tap opens that thread (serve.js).
fn chip((w, t): (&str, &str)) -> String {
    format!("<span class=lc data-go=\"{}\">{w} {}</span>", esc(t), esc(t))
}

/// #387: a signed-out line's [sign in]: the harness's login in a setup terminal on the main host (#303).
fn signin(cfg: &Config, e: &Event, dis: &str) -> String {
    let Some(name) = e.str("signin") else {
        return String::new();
    };
    let main = cfg
        .hosts
        .iter()
        .find(|h| h.1.machine.is_none())
        .map_or("local", |h| h.0.as_str());
    format!(
        " <button type=button data-su=\"{}\" data-host=\"{}\"{dis}>sign in</button>",
        esc(name),
        esc(main)
    )
}

/// A thread's row: unread dot, title, age; then its state (unless plain awake) and the last message.
fn row(ctx: &Ctx, events: &[Event], t: &Event, agents: &[Value]) -> (u64, String) {
    let id = t.str("thread").unwrap_or_default();
    let mut msgs = of(events, id).filter(|e| matches!(e.kind.as_str(), "thread-msg" | "needs-decision"));
    let last = msgs.rfind(|e| e.str("from") != Some("system"));
    let at = last.map_or(t.ts, |e| e.ts);
    let line = last.map_or(String::new(), |e| {
        let who = or(e.str("from") == Some("owner"), "you", "conductor");
        format!("{who}: {}", e.str("text").or(e.str("question")).unwrap_or_default())
    });
    let st = match state(events, id, agents) {
        "choose" => "<b class=r>! choose</b>",
        "working" => "<b class=a>✻ working</b>",
        "asleep" => "<b class=m>☾ asleep</b>",
        _ => "",
    };
    let un = conductor::unread(events, id);
    let (rd, dot) = (or(un, "", " rd"), or(un, "●", ""));
    let (title, age, line) = (
        esc(conductor::title(events, id)),
        ago(ctx.now, at),
        esc(line.lines().next().unwrap_or_default()),
    );
    // #413: the threads it is linked to (not its flow-back lines), once each.
    let lk: std::collections::BTreeSet<_> = of(events, id)
        .filter_map(|e| conductor::link(e).filter(|_| e.str("from") != Some("system")))
        .collect();
    let lk: String = lk.into_iter().map(|l| chip(l) + " ").collect();
    let html = format!(
        "<a class=\"tr{rd}\" href=#chat data-t=\"{id}\"><i class=ud>{dot}</i><span class=tt>{title}</span><span class=ta>{age}</span><span class=tp>{lk}{st}{line}</span></a>"
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
    let (mut list, mut views) = (String::new(), String::new());
    for (repo, k) in repos {
        let mine = threads.iter().filter(|t| t.fleet == repo);
        // #405: a row swiped left shows [archive]; the archived fold's rows each have [restore].
        let (dis, mut arch) = (or(rw, "", " disabled"), vec![]);
        let mut rows = vec![];
        for t in mine {
            let (at, a) = row(ctx, events, t, agents);
            let id = t.str("thread").unwrap_or_default();
            let (on, b) = (conductor::archived(events, id), ["archive", "restore"]);
            let b = b[usize::from(on)];
            let html = format!(
                "<div class=sw>{a}<button type=button class=sb name={b} data-t=\"{id}\"{dis}>{b}</button></div>"
            );
            [&mut rows, &mut arch][usize::from(on)].push((at, html));
        }
        rows.sort_by_key(|r| std::cmp::Reverse(r.0));
        arch.sort_by_key(|r| std::cmp::Reverse(r.0));
        let n = rows.len();
        let count = or(n == 0, "no threads", &format!("{n} thread{}", or(n == 1, "", "s"))).to_string();
        let name = or(repo == "all", "all repos", repo);
        let older: String = rows.iter().skip(3).map(|r| r.1.as_str()).collect();
        let older = if n > 3 {
            format!("<details class=done><summary><span>{} older thread{}</span><i class=cv></i></summary>{older}</details>", n - 3, or(n == 4, "", "s"))
        } else {
            String::new()
        };
        let a = arch.len();
        let arch: String = arch.iter().map(|r| r.1.as_str()).collect();
        let arch = if a > 0 {
            format!("<details class=\"done arc\"><summary><span>{a} archived</span><i class=cv></i></summary>{arch}</details>")
        } else {
            String::new()
        };
        let (r, nm) = (esc(repo), esc(name));
        let add = if rw {
            format!("<span class=btns><button type=button class=ntb data-repo=\"{r}\">+ thread</button></span>")
        } else {
            String::new()
        };
        let top: String = rows.iter().take(3).map(|r| r.1.as_str()).collect();
        list += &format!(
            "<div class=\"fc k-{k}\"><div class=fh><span class=fn><b>{nm}</b><span class=ds>{count}</span></span>{add}</div>{top}{older}{arch}</div>"
        );
        views += &draft(repo, dis);
    }
    let view = |t: &&Event| thread(ctx, cfg, events, t, (agents, rw));
    views += &threads.iter().map(view).collect::<String>();
    format!("<div class=clp>{list}</div>{views}")
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
