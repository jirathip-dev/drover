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

/// `herdsman thread new <fleet|all> <title>`: print its id.
pub fn new(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, repo: &str, title: &str) -> Result<()> {
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
    Ok(writeln!(out, "{id}")?)
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

/// A file a message carries, as the thread logs it: its name, absolute path and size.
pub fn file(p: &std::path::Path) -> Result<Value> {
    let p = std::path::absolute(p)?;
    let size = std::fs::metadata(&p).with_context(|| p.display().to_string())?.len();
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
    Ok(json!({"name": name, "path": p, "size": size}))
}

/// `herdsman thread reply [--file <path>]…`: the conductor's message to the owner, its files shown inline.
pub fn reply(ctx: &Ctx, cfg: &Config, id: &str, text: &str, files: &[std::path::PathBuf]) -> Result<()> {
    text_ok(text)?;
    let files = files.iter().map(|p| file(p)).collect::<Result<Vec<_>>>()?;
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
    let events = read(&ctx.state)?;
    let last = of(&events, id).rfind(|e| e.kind.starts_with("conductor-"));
    let agents = herdr::agents(r)?;
    let awake = last.filter(|s| s.kind == "conductor-start" && agents.iter().any(|a| a["name"] == s.data["agent"]));
    let Some(s) = awake else {
        return start(ctx, cfg, r, &t, &events, &line);
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

/// The conductor role: the thread fleet's `[fleet.<n>.role.conductor]`; for `all`, the first fleet's that has one.
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
            format!("{repo} has no conductor; add [fleet.<n>.role.conductor]"),
        ),
    }
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
/// resume gets the conversation so far in its kick instead).
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
    let kick = if resume {
        line.into()
    } else {
        let past = render(&items(events, id));
        format!(
            "Read {} and act on it. The thread so far, the last message new:\n{past}",
            brief.display()
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
    let on = crate::hosts::On::new(r, None);
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
    let d = json!({"thread": id, "agent": agent, "pane": pane, "workspace": wsid, "harness": p.harness,
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
            (&format!("{on} {})", crate::usage::date(r, *until, "+%H:%M")), &[]),
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
            Some(m)
        }
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
            format!("{}: {}{files}\n", s(i, "from"), s(i, "text"))
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
