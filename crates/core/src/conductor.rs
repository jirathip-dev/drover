//! #355: conductor chat. A thread belongs to a fleet (or `all`); one conductor lane (role `conductor`, any harness)
//! talks it through in a working folder without a worktree, sleeps after `IDLE` and wakes with its saved session.
use crate::config::{expand, Config, Role};
use crate::events::{self, emit, read, Event};
use crate::gate::refused;
use crate::runner::Runner;
use crate::spawn::Plan;
use crate::{harness, herdr, Ctx};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::Write;

/// A conductor whose thread is quiet this long (and that is not working) closes on the next `check`.
pub const IDLE: u64 = 30 * 60;

/// The thread's events: its messages, asks, starts and sleeps.
fn of<'a>(events: &'a [Event], id: &'a str) -> impl DoubleEndedIterator<Item = &'a Event> {
    events.iter().filter(move |e| e.str("thread") == Some(id))
}

/// The thread's `thread-new` (`NO_SUCH_THREAD`).
pub fn find<'a>(events: &'a [Event], id: &str) -> Result<&'a Event> {
    match events
        .iter()
        .find(|e| e.kind == "thread-new" && e.str("thread") == Some(id))
    {
        Some(t) => Ok(t),
        None => refused(
            "NO_SUCH_THREAD",
            format!("no thread {id}; `herdsman thread new` starts one"),
        ),
    }
}

/// `herdsman thread new <fleet|all> <title>`: its id.
pub fn new(ctx: &Ctx, cfg: &Config, repo: &str, title: &str) -> Result<String> {
    if repo != "all" {
        cfg.fleet(repo)?;
    }
    if title.trim().is_empty() || title.chars().any(char::is_control) {
        return refused("BAD_VALUE", "a thread starts with a one-line title");
    }
    let events = read(&ctx.state)?;
    let id = format!("t{}", events.iter().filter(|e| e.kind == "thread-new").count() + 1);
    let d = json!({"thread": id, "title": title});
    emit(&ctx.state, cfg, Event::new(ctx.now, repo, None, "thread-new", d))?;
    Ok(id)
}

/// #413: `thread handoff`: a new thread in `to` (a repo or `all`) or thread `to`, linked; wakes its conductor as `from`.
pub fn handoff(ctx: &Ctx, cfg: &Config, r: &dyn Runner, [from, to, title, text]: [&str; 4]) -> Result<String> {
    text_ok(text)?;
    let events = read(&ctx.state)?;
    let t = find(&events, from)?;
    // The target's conductor is checked first: no thread or link is left behind without delivery.
    let to = if is_id(to) {
        role(cfg, &find(&events, to)?.fleet)?;
        to.to_string()
    } else {
        role(cfg, to)?;
        new(ctx, cfg, to, title)?
    };
    if to == from {
        return refused("BAD_VALUE", "a thread hands off to another thread");
    }
    let d = json!({"thread": from, "from": from, "to": to, "title": title, "text": text});
    let e = Event::new(ctx.now, &t.fleet, None, "thread-handoff", d);
    emit(&ctx.state, cfg, e)?;
    say(ctx, cfg, r, (&to, text, &[]), (from, false))?;
    Ok(to)
}

/// #413: a one-line row from thread `id` into each thread that handed off to it (an issue filed, an ask answered).
pub fn flow(ctx: &Ctx, cfg: &Config, id: &str, text: &str) -> Result<()> {
    let events = read(&ctx.state)?;
    let to_id = |e: &&Event| e.kind == "thread-handoff" && e.str("to") == Some(id);
    let srcs: std::collections::BTreeSet<_> = events.iter().filter(to_id).filter_map(|e| e.str("from")).collect();
    for s in srcs {
        let d = json!({"thread": s, "from": "system", "via": id, "text": text});
        let e = Event::new(ctx.now, &find(&events, s)?.fleet, None, "thread-msg", d);
        emit(&ctx.state, cfg, e)?;
    }
    Ok(())
}

/// A thread id: `t` and a number.
fn is_id(s: &str) -> bool {
    s.len() > 1 && s.starts_with('t') && s[1..].bytes().all(|b| b.is_ascii_digit())
}

/// #413: a linked row's chip: `from t5` (a message or flow-back row from t5) or `continued in t6` (a handoff).
pub fn link(e: &Event) -> Option<(&'static str, &str)> {
    match (e.kind.as_str(), e.str("from")) {
        ("thread-handoff", _) => Some(("continued in", e.str("to")?)),
        ("thread-msg", Some("system")) => Some(("from", e.str("via")?)),
        ("thread-msg", Some(f)) if is_id(f) => Some(("from", f)),
        _ => None,
    }
}

fn text_ok(text: &str) -> Result<()> {
    if text.trim().is_empty() || text.chars().count() > 20_000 {
        return refused("BAD_VALUE", "a message is non-empty text, at most 20,000 chars");
    }
    Ok(())
}

fn log(ctx: &Ctx, cfg: &Config, t: &Event, from: &str, (text, files): (&str, &[Value])) -> Result<()> {
    let mut d = json!({"thread": t.str("thread"), "from": from, "text": text});
    if !files.is_empty() {
        d["files"] = json!(files);
    }
    emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, "thread-msg", d))
}

/// The thread's conductor's working folder, one per thread: `<state dir>/conductor/<thread>/`, or (#386)
/// `<[conductor] dir>/<thread>/`.
pub fn dir(ctx: &Ctx, cfg: &Config, id: &str) -> std::path::PathBuf {
    let root = cfg.conductor.dir.as_deref().map(expand);
    root.unwrap_or_else(|| ctx.state.with_file_name("conductor")).join(id)
}

/// `p` resolved, if it lies in thread `id`'s conductor folder: the only files a thread shows or serves (#386).
pub fn inside(ctx: &Ctx, cfg: &Config, id: &str, p: &std::path::Path) -> Option<std::path::PathBuf> {
    let root = dir(ctx, cfg, id).canonicalize().ok()?;
    p.canonicalize().ok().filter(|p| p.starts_with(root))
}

/// A file a message carries, as the thread logs it: its name, path in the conductor's folder and size.
pub fn file(ctx: &Ctx, cfg: &Config, id: &str, p: &std::path::Path) -> Result<Value> {
    let Some(p) = inside(ctx, cfg, id, p) else {
        let d = dir(ctx, cfg, id).display().to_string();
        bail!("{}: not in the thread's folder {d}; copy it there first", p.display());
    };
    let size = std::fs::metadata(&p).with_context(|| p.display().to_string())?.len();
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
    Ok(json!({"name": name, "path": p, "size": size}))
}

/// `herdsman thread reply [--file <path>]…`: the conductor's message to the owner, its files shown inline.
pub fn reply(ctx: &Ctx, cfg: &Config, id: &str, text: &str, files: &[std::path::PathBuf]) -> Result<()> {
    text_ok(text)?;
    let files = files
        .iter()
        .map(|p| file(ctx, cfg, id, p))
        .collect::<Result<Vec<_>>>()?;
    log(ctx, cfg, find(&read(&ctx.state)?, id)?, "conductor", (text, &files))
}

/// `herdsman thread say` (and a thread ask's answer, `raw`: typed as is; #386: the board's, with `files`): log
/// `from`'s message, then type it into the thread's conductor, starting or resuming it first; the agent it went to.
pub fn say(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    (id, text, files): (&str, &str, &[Value]),
    (from, raw): (&str, bool),
) -> Result<String> {
    let (t, line) = post(ctx, cfg, (id, text, files), (from, raw))?;
    deliver(ctx, cfg, r, &t, &line)
}

/// #403: `say`'s first half: log the message; the thread and the line `deliver` types.
pub fn post(
    ctx: &Ctx,
    cfg: &Config,
    (id, text, files): (&str, &str, &[Value]),
    (from, raw): (&str, bool),
) -> Result<(Event, String)> {
    text_ok(text)?;
    let t = find(&read(&ctx.state)?, id)?.clone();
    log(ctx, cfg, &t, from, (text, files))?;
    let path = |f: &Value| format!(" (attached: {})", f["path"].as_str().unwrap_or_default());
    let paths: String = files.iter().map(path).collect();
    let line = if raw {
        text.into()
    } else {
        format!("[{id}] {from}: {text}{paths}")
    };
    Ok((t, line))
}

/// #403: `say`'s second half: type `line` into thread `t`'s conductor, starting or resuming it first.
pub fn deliver(ctx: &Ctx, cfg: &Config, r: &dyn Runner, t: &Event, line: &str) -> Result<String> {
    let id = t.str("thread").unwrap_or_default();
    let events = read(&ctx.state)?;
    let last = of(&events, id).rfind(|e| e.kind.starts_with("conductor-"));
    let agents = herdr::agents(r)?;
    let awake = last.filter(|s| s.kind == "conductor-start" && agents.iter().any(|a| a["name"] == s.data["agent"]));
    let Some(s) = awake else {
        return start(ctx, cfg, r, t, &events, line);
    };
    let (agent, pane) = (s.str("agent").unwrap_or_default(), s.str("pane").unwrap_or_default());
    herdr::reply(
        r,
        s.str("harness") == Some("dsh"),
        agent,
        pane,
        &line.replace('\n', " "),
    )?;
    Ok(agent.into())
}

/// The conductor role: the thread fleet's `[fleet.<n>.role.conductor]` (#401: else its impl role on profile
/// `lane-conductor`, `Config::parse`); for `all`, the first fleet's that has one.
pub fn role<'a>(cfg: &'a Config, repo: &str) -> Result<(&'a str, &'a Role)> {
    let has = |(n, f): (&'a String, &'a crate::config::Fleet)| Some((n.as_str(), f.role.get("conductor")?));
    let found = match repo {
        "all" => cfg.fleet.iter().find_map(has),
        _ => cfg.fleet.get_key_value(repo).and_then(has),
    };
    match found {
        Some(x) => Ok(x),
        None => refused(
            "NO_CONDUCTOR",
            format!("{repo} has no conductor or impl role; add [fleet.<n>.role.conductor]"),
        ),
    }
}

/// #401: the models a fleet's roles use, the [config] sheet's conductor model choices.
pub fn models(f: &crate::config::Fleet) -> Vec<&str> {
    let all: std::collections::BTreeSet<_> = f.role.values().filter_map(|r| r.model.as_deref()).collect();
    all.into_iter().collect()
}

/// #401: the [config] sheet's conductor choices (harness, model, effort) into `[fleet.<n>.role.conductor]` in place (a
/// fleet on the default gets the table: its impl role's keys); refused `DOCTOR` if `doctor` with the new file shows a ✗
/// it did not before. The old file goes to `fleet.toml.bak` first.
pub fn set(ctx: &Ctx, r: &dyn Runner, fleet: &str, wanted: &[(&str, String)]) -> Result<String> {
    let text = std::fs::read_to_string(&ctx.config)?;
    let cfg = Config::parse(&text)?;
    let cur = role(&cfg, fleet)?.1;
    let models = models(cfg.fleet(fleet)?);
    let bad = |(k, v): &&(&str, String)| {
        *k == "model" && !models.contains(&v.as_str()) || v.chars().any(|c| c == '"' || c == '\\' || c.is_control())
    };
    if let Some((k, v)) = wanted.iter().find(bad) {
        return refused("BAD_VALUE", format!("conductor {k} {v:?}: not one of the choices"));
    }
    let mut new = text.clone();
    if cur.default {
        let raw: toml::Table = toml::from_str(&text)?;
        let imp = raw
            .get("fleet")
            .and_then(|f| f.get(fleet)?.get("role")?.get("impl")?.as_table());
        let mut t = imp.cloned().unwrap_or_default();
        t.insert("profile".into(), "lane-conductor".into());
        let nl = if text.ends_with('\n') { "" } else { "\n" };
        new += &format!("{nl}\n[fleet.{fleet}.role.conductor]\n{}", toml::to_string(&t)?);
    }
    for (k, v) in wanted {
        new = crate::config::set_value(&new, fleet, "conductor", k, v)?;
    }
    let new_cfg = Config::parse(&new)?;
    let fails = |c: &Ctx| -> Result<Vec<String>> {
        let mut out = vec![];
        crate::doctor::doctor(c, r, &mut out, true)?;
        Ok(serde_json::from_slice::<Vec<String>>(&out)?
            .into_iter()
            .filter(|l| l.starts_with('✗'))
            .collect())
    };
    let (before, tmp) = (fails(ctx)?, crate::devices::tmp_name(&ctx.config));
    std::fs::write(&tmp, &new)?;
    let after = fails(&Ctx {
        config: tmp.clone(),
        ..ctx.clone()
    });
    std::fs::remove_file(&tmp)?;
    let bad: Vec<String> = after?.into_iter().filter(|l| !before.contains(l)).collect();
    if !bad.is_empty() {
        return refused("DOCTOR", format!("doctor fails with it: {}", bad.join("; ")));
    }
    std::fs::copy(&ctx.config, ctx.config.with_extension("toml.bak"))?;
    crate::config::write_atomic(&ctx.config, &new)?;
    for (k, v) in wanted {
        let d = json!({"role": "conductor", "key": k, "new": v, "via": "web"});
        emit(&ctx.state, &new_cfg, Event::new(ctx.now, fleet, None, "set", d))?;
    }
    let line: Vec<String> = wanted.iter().map(|(k, v)| format!("{k} {v}")).collect();
    Ok(format!("{fleet} conductor: {}", line.join(", ")))
}

/// A random v4 UUID, a new conductor's session id.
fn uuid() -> Result<String> {
    let mut b = crate::devices::random(16)?;
    (b[6], b[8]) = (b[6] & 0x0f | 0x40, b[8] & 0x3f | 0x80);
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    ))
}

/// Start the thread's conductor with `line`: its first start, or a resume of its session (a harness that can't
/// resume is pointed at the conversation so far instead).
fn start(ctx: &Ctx, cfg: &Config, r: &dyn Runner, t: &Event, events: &[Event], line: &str) -> Result<String> {
    let id = t.str("thread").unwrap_or_default();
    let (fleet, base) = role(cfg, &t.fleet)?;
    let f = cfg.fleet(fleet)?;
    let (role, resting) = crate::access::fallback(cfg, events, base, ctx.now)?;
    let h = harness::get(&role.harness);
    // Only a session this harness started resumes (a fallback to another harness starts its own).
    let first = of(events, id).find(|e| e.kind == "conductor-start" && e.str("harness") == Some(&role.harness));
    let session = first
        .and_then(|e| e.str("session"))
        .map_or_else(uuid, |s| Ok(s.into()))?;
    let resume = first.is_some() && h.resumes();
    let dir = dir(ctx, cfg, id);
    let brief = dir.join(".brief.md");
    // #397: the kick is one line (herdr refuses a multi-line `agent start` argument); the thread is in a file.
    let on = crate::hosts::On::new(r, None);
    let thread = dir.join(".thread.md");
    on.write(&thread, &render(&items(events, id)))?;
    let kick = if resume {
        line.replace(char::is_control, " ")
    } else {
        format!(
            "Read {} and act on it. The thread so far is in {}, the last message new.",
            brief.display(),
            thread.display()
        )
    };
    let agent = format!("conductor-{id}");
    let head = format!("{} · conductor {id}", t.fleet);
    let mut p = Plan {
        agent: agent.clone(),
        role: "conductor".into(),
        stage: "conductor".into(),
        family: "conductor",
        harness: role.harness.clone(),
        profile: role.profile_or("conductor"),
        model: role.model.clone(),
        effort: role.effort_or(&cfg.limits),
        branch: String::new(),
        base: String::new(),
        worktree: dir.clone(),
        brief: brief.clone(),
        kick,
        lane: format!("{fleet}:-:conductor"),
        account: crate::access::pick(ctx, cfg, events, &role)?,
        env: f.env.clone(),
        port: crate::spawn::lane_port(f.port_range.as_deref(), &format!("{fleet}/{agent}"), &[]),
        mcp: crate::access::mcp_servers(cfg, f, &role)?,
        mcp_file: dir.join(".mcp.lane.json"),
        title: crate::spawn::lane_title(&head, t.str("title").unwrap_or_default()),
        head,
        resting,
        session: Some((session.clone(), resume)),
    };
    p.env.insert("HERDSMAN_THREAD".into(), id.into());
    h.ready(ctx, r, &p)?;
    on.write(&brief, &profile(cfg, t, fleet))?;
    h.prepare(ctx, &on, &p)?;
    let pane = &herdr::workspace_create(r, &dir.to_string_lossy(), &agent)?;
    let wsid = pane.split(':').next().unwrap_or(pane);
    herdr::ok(r, &p.lane_cmd(pane))?;
    let o = herdr::exec(r, &p.start_cmd(ctx, pane))?;
    if o.code != 0 {
        let _ = herdr::workspace_close(r, wsid);
        bail!("herdr agent start failed: {}", o.stderr.trim());
    }
    let kicked = h.kick(r, &p, pane).unwrap_or(false);
    let d = json!({"thread": id, "agent": agent, "pane": pane, "workspace": wsid, "harness": p.harness, "model": p.model,
        "account": p.account.as_ref().map(|a| &a.0), "session": session, "resumed": resume, "confirmed": kicked});
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, &t.fleet, None, "conductor-start", d),
    )?;
    if let Some((on, until)) = &p.resting {
        log(
            ctx,
            cfg,
            t,
            "system",
            (
                &format!("switched to {} {})", &on[3..], crate::usage::date(r, *until, "+%H:%M")),
                &[],
            ),
        )?;
    }
    Ok(agent)
}

/// The conductor's `.brief.md`: its thread, then `[conductor]`'s instructions, memory notes and skills index.
fn profile(cfg: &Config, t: &Event, fleet: &str) -> String {
    let (id, title) = (t.str("thread").unwrap_or_default(), t.str("title").unwrap_or_default());
    let repo = cfg.fleet.get(&t.fleet).map_or("all repos".into(), |f| {
        format!("{} (fleet {}, checkout {})", f.repo, t.fleet, f.local)
    });
    let mut s = format!(
        "# Conductor brief: thread {id}\n\n{}\n\nThread: {id}, {title}\nRepo: {repo}\nFleet for asks: {fleet}\n",
        crate::spawn::role_prompt("conductor").trim_end()
    );
    let c = &cfg.conductor;
    if let Some(p) = &c.instructions {
        let text = std::fs::read_to_string(expand(p)).unwrap_or_else(|e| format!("({p} unreadable: {e})"));
        s += &format!("\n## Instructions ({p})\n\n{}\n", text.trim_end());
    }
    let files = |d: &Option<String>| {
        let all = d.as_deref().and_then(|d| std::fs::read_dir(expand(d)).ok());
        let mut v: Vec<_> = all.into_iter().flatten().flatten().map(|e| e.path()).collect();
        v.sort();
        v
    };
    for m in files(&c.memory)
        .iter()
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
    {
        let text = std::fs::read_to_string(m).unwrap_or_default();
        s += &format!("\n## Memory: {}\n\n{}\n", m.display(), text.trim_end());
    }
    let skills = files(&c.skills);
    if !skills.is_empty() {
        s += "\n## Skills (read one when it applies)\n\n";
        s += &skills
            .iter()
            .map(|p| format!("- {}\n", p.display()))
            .collect::<String>();
    }
    s
}

/// The thread as items: messages (`from`, `text`) and asks (`ask`, `question`, numbered `options` with the
/// recommended one first, `open`).
pub fn items(events: &[Event], id: &str) -> Vec<Value> {
    let item = |e: &Event| match e.kind.as_str() {
        "thread-msg" => {
            let mut m = json!({"ts": e.ts, "from": e.str("from"), "text": e.str("text")});
            if let Some(f) = e.data.get("files") {
                m["files"] = f.clone();
            }
            if let Some((w, t)) = link(e) {
                m["link"] = json!(format!("{w} {t}"));
            }
            Some(m)
        }
        "thread-handoff" => Some(
            json!({"ts": e.ts, "from": "system", "text": e.str("title"), "link": format!("continued in {}", e.str("to").unwrap_or_default())}),
        ),
        "needs-decision" => {
            let all = e.data["options"].as_array().cloned().unwrap_or_default();
            let mut opts: Vec<Value> = all
                .into_iter()
                .zip(1..)
                .map(|(t, n)| json!({"n": n, "text": t}))
                .collect();
            let rec = e.data["recommend"].as_u64().unwrap_or(0);
            opts.sort_by_key(|o| o["n"] != rec);
            let open = events::closed(events, e).is_none();
            Some(
                json!({"ts": e.ts, "from": "conductor", "ask": e.str("id"), "question": e.str("question"),
                "options": opts, "recommend": e.data["recommend"], "open": open}),
            )
        }
        _ => None,
    };
    of(events, id).filter_map(item).collect()
}

/// Items as lines: `<from>: <text>`, an ask as `<from> asks <id>: <question>` and its `[n]` options.
fn render(items: &[Value]) -> String {
    let s = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_string();
    let line = |i: &Value| match i["ask"].as_str() {
        None => {
            let files = i["files"].as_array().into_iter().flatten();
            let files: String = files.map(|f| format!(" [file {}]", s(f, "path"))).collect();
            let who = i["link"].as_str().unwrap_or(i["from"].as_str().unwrap_or_default());
            format!("{who}: {}{files}\n", s(i, "text"))
        }
        Some(q) => {
            let rec = |o: &Value| if o["n"] == i["recommend"] { " (recommended)" } else { "" };
            let opts = i["options"].as_array().into_iter().flatten();
            let opts: String = opts
                .map(|o| format!("  [{}] {}{}\n", o["n"], s(o, "text"), rec(o)))
                .collect();
            format!("conductor asks {q}: {}\n{opts}", s(i, "question"))
        }
    };
    items.iter().map(line).collect()
}

/// The conductor spoke (a message or an ask) since the owner last read or wrote.
pub fn unread(events: &[Event], id: &str) -> bool {
    let seen = |e: &&Event| matches!(e.kind.as_str(), "thread-msg" | "thread-read" | "needs-decision");
    let last = of(events, id).rfind(seen);
    last.is_some_and(|e| e.kind == "needs-decision" || e.str("from") == Some("conductor"))
}

/// Log `thread-read` for thread `t` if the conductor spoke since the owner last read or wrote (`thread show`, the board).
pub fn mark_read(ctx: &Ctx, cfg: &Config, events: &[Event], t: &Event) -> Result<()> {
    let id = t.str("thread").unwrap_or_default();
    let read = Event::new(ctx.now, &t.fleet, None, "thread-read", json!({"thread": id}));
    if unread(events, id) {
        emit(&ctx.state, cfg, read)?;
    }
    Ok(())
}

/// `herdsman thread show <id>`: the conversation; marks it read.
pub fn show(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, id: &str, as_json: bool) -> Result<()> {
    let events = read(&ctx.state)?;
    let t = find(&events, id)?;
    let items = items(&events, id);
    mark_read(ctx, cfg, &events, t)?;
    if as_json {
        let v = json!({"thread": id, "repo": t.fleet, "title": t.str("title"), "items": items});
        return Ok(writeln!(out, "{}", serde_json::to_string_pretty(&v)?)?);
    }
    writeln!(out, "{id} {}: {}", t.fleet, t.str("title").unwrap_or_default())?;
    Ok(write!(out, "{}", render(&items))?)
}

/// `herdsman thread ls [fleet]`: per repo, its threads with title, last message, unread dot and conductor state.
pub fn ls(ctx: &Ctx, out: &mut dyn Write, fleet: Option<&str>, as_json: bool) -> Result<()> {
    let events = read(&ctx.state)?;
    let mut rows: Vec<Value> = vec![];
    for t in events
        .iter()
        .filter(|e| e.kind == "thread-new" && fleet.is_none_or(|f| f == e.fleet))
    {
        let id = t.str("thread").unwrap_or_default();
        let items = items(&events, id);
        let last = items.last().map(|i| render(std::slice::from_ref(i)));
        let last = last.as_deref().and_then(|l| l.lines().next()).unwrap_or_default();
        let lane = of(&events, id).rfind(|e| e.kind.starts_with("conductor-"));
        let state = lane.map_or("none", |e| if e.kind == "conductor-start" { "awake" } else { "asleep" });
        rows.push(
            json!({"thread": id, "repo": t.fleet, "title": t.str("title"), "last": last,
            "at": items.last().map_or(t.ts, |i| i["ts"].as_u64().unwrap_or(t.ts)), "unread": unread(&events, id),
            "conductor": state}),
        );
    }
    rows.sort_by(|a, b| a["repo"].as_str().cmp(&b["repo"].as_str()));
    if as_json {
        return Ok(writeln!(out, "{}", serde_json::to_string_pretty(&rows)?)?);
    }
    let mut repo = "";
    for r in &rows {
        let s = |k: &str| r[k].as_str().unwrap_or_default();
        if s("repo") != repo {
            repo = s("repo");
            writeln!(out, "{repo}")?;
        }
        let dot = if r["unread"] == true { "•" } else { " " };
        let age = events::age(ctx.now.saturating_sub(r["at"].as_u64().unwrap_or(0)));
        writeln!(
            out,
            "  {} {dot} {}: {} ({age}, {})",
            s("thread"),
            s("title"),
            s("last"),
            s("conductor")
        )?;
    }
    Ok(())
}

/// `check`: each awake conductor whose thread has been quiet `IDLE` and that is not working is closed
/// (`conductor-sleep`); the next message resumes it.
pub fn sleep_idle(ctx: &Ctx, cfg: &Config, r: &dyn Runner, events: &[Event], agents: &[Value]) -> Result<()> {
    for t in events.iter().filter(|e| e.kind == "thread-new") {
        let id = t.str("thread").unwrap_or_default();
        let last = of(events, id).rfind(|e| e.kind.starts_with("conductor-"));
        let Some(s) = last.filter(|e| e.kind == "conductor-start") else {
            continue;
        };
        let quiet = of(events, id).map(|e| e.ts).max().unwrap_or(0) + IDLE <= ctx.now;
        let a = agents.iter().find(|a| a["name"] == s.data["agent"]);
        if a.is_some() && switch(ctx, cfg, r, t, s)? {
            continue;
        }
        if !quiet || a.is_some_and(|a| a["agent_status"] == "working") {
            continue;
        }
        if a.is_some() {
            herdr::workspace_close(r, s.str("workspace").unwrap_or_default())?;
        }
        let d = json!({"thread": id, "agent": s.str("agent"), "why": if a.is_some() { "idle" } else { "gone" }});
        emit(
            &ctx.state,
            cfg,
            Event::new(ctx.now, &t.fleet, None, "conductor-sleep", d),
        )?;
    }
    Ok(())
}

/// #387: an awake conductor showing a usage limit (or signed out) rests its account (else its harness) and resumes
/// the same session on the role's next account or `fallback` entry; signed out, the thread gets a [sign in] line.
fn switch(ctx: &Ctx, cfg: &Config, r: &dyn Runner, t: &Event, s: &Event) -> Result<bool> {
    let (id, account, hn) = (
        t.str("thread").unwrap_or_default(),
        s.str("account"),
        s.str("harness").unwrap_or_default(),
    );
    // No account and no `fallback`: nothing to switch to, but a sign-out still gets its [sign in] line.
    let chain = account.is_some() || role(cfg, &t.fleet).is_ok_and(|r| !r.1.fallback.is_empty());
    let (screen, h) = (
        herdr::read(r, s.str("pane").unwrap_or_default()).unwrap_or_default(),
        harness::get(hn),
    );
    let out = h.signed_out(&screen);
    let rest = out.then(|| ctx.now + cfg.limits.fallback_rest_minutes * 60);
    let limit = chain.then(|| h.usage_limited(cfg, r, &screen, ctx.now)).flatten();
    let Some(until) = limit.or(rest) else {
        return Ok(false);
    };
    if chain {
        let kind = ["agent-unavailable", "account-capped"][usize::from(account.is_some())];
        let d = json!({"agent": s.str("agent"), "harness": hn, "account": account, "until": until});
        emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, kind, d))?;
    }
    herdr::workspace_close(r, s.str("workspace").unwrap_or_default())?;
    let why = if out { "signed out" } else { "usage limit" };
    let d = json!({"thread": id, "agent": s.str("agent"), "why": why});
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, &t.fleet, None, "conductor-sleep", d),
    )?;
    if out {
        let text = format!(
            "{hn} is signed out{}: sign in",
            account.map_or(String::new(), |a| format!(" (account {a})"))
        );
        // #303 names an account's login `claude B` ([[setup]]); without one configured, the harness's own login.
        let named = account.map(|a| format!("{hn} {a}"));
        let su = named.filter(|n| crate::setup::all(cfg).iter().any(|s| &s.0 == n));
        let d = json!({"thread": id, "from": "system", "text": text, "signin": su.as_deref().unwrap_or(hn)});
        emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, "thread-msg", d))?;
    }
    if !chain {
        return Ok(true);
    }
    let line = format!("[{id}] system: {why}; you resumed on the next account. Carry on where you left off.");
    if let Err(e) = start(ctx, cfg, r, t, &read(&ctx.state)?, &line) {
        log(
            ctx,
            cfg,
            t,
            "system",
            (&format!("the conductor stays asleep: {e:#}"), &[]),
        )?;
    }
    Ok(true)
}
