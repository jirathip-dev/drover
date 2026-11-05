//! fleet: agents act only through this CLI; it owns state and refuses rule breaks.
pub mod access;
pub mod berth;
pub mod check;
#[allow(clippy::doc_markdown)] // its doc comments are clap help text, printed verbatim
mod cli;
pub mod cloud;
pub mod conductor;
pub mod config;
pub mod db;
pub mod design;
pub mod devices;
pub mod doctor;
pub mod drive;
pub mod events;
pub mod gate;
pub mod gh;
pub mod harness;
pub mod herdr;
pub mod hook;
pub mod host;
pub mod hosts;
pub mod land;
pub mod lanes;
pub mod lesson;
pub mod overlap;
pub mod pipeline;
pub mod propose;
pub mod protection;
pub mod queue;
pub mod report;
pub mod runner;
pub mod secret;
pub mod setup;
pub mod snapshot;
pub mod spawn;
pub mod telegram;
pub mod timer;
pub mod usage;
pub mod verify;
pub mod waiting;
pub mod webpush;

use anyhow::{bail, Result};
pub use cli::{main_with, usage_doc, HELP_AGENT_MAX, SERVE};
use config::{home, set_in, set_value, write_atomic, Config};
use events::{emit, read, Event};
use runner::Runner;
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;

/// #235: `herdsman --version`: the crate version and the git sha it is built from (build.rs).
pub const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("HERDSMAN_SHA"), ")");

#[derive(Clone)]
pub struct Ctx {
    pub config: PathBuf,
    pub state: PathBuf,
    pub worktrees: PathBuf,
    /// Hermes profiles root (`<root>/<profile>/config.yaml`).
    pub hermes: PathBuf,
    pub exe: String,
    pub now: u64,
    /// Tree `herdsman migrate` works on: `HERDSMAN_HOME`, else `$HOME`.
    pub home: PathBuf,
    /// Pre-rename fallbacks in use this run; data commands print them on stderr.
    pub hints: Vec<String>,
}

impl Ctx {
    pub fn from_env() -> Ctx {
        let env = |k: &str, d: PathBuf| var(k).map_or(d, PathBuf::from);
        let root = var("HERDSMAN_HOME").map_or_else(home, PathBuf::from);
        let mut hints = vec![];
        // Read the pre-rename dir, never move it, while the new one is missing.
        let mut path = |k: &str, what: &str, d: &str, file: &str| {
            if let Some(p) = var(k) {
                return PathBuf::from(p);
            }
            let (old, new) = (root.join(d).join("fleet"), root.join(d).join("herdsman"));
            if old.is_dir() && !new.exists() {
                hints.push(format!(
                    "herdsman: found pre-rename {what} at {}; run 'herdsman migrate' to move it",
                    old.display()
                ));
                return old.join(file);
            }
            new.join(file)
        };
        let config = path("FLEET_CONFIG", "config", ".config", "fleet.toml");
        let state = path("FLEET_STATE", "state", ".local/state", "events.jsonl");
        Ctx {
            config,
            state,
            worktrees: env("FLEET_WORKTREES", home().join(".herdr/worktrees")),
            hermes: env("FLEET_HERMES_PROFILES", home().join(".hermes/profiles")),
            exe: std::env::current_exe().map_or("herdsman".into(), |p| p.to_string_lossy().into()),
            now: now(),
            home: root,
            hints,
        }
    }
}

/// A path variable, with set-but-empty treated as unset (an empty path would mean the cwd).
fn var(k: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(k).filter(|v| !v.is_empty())
}

/// `herdsman migrate` (docs/architecture.md): both moves or neither.
pub fn migrate(home: &std::path::Path, out: &mut dyn Write) -> Result<()> {
    let mut moved: Vec<(PathBuf, PathBuf)> = vec![];
    for d in [".config", ".local/state"] {
        let (old, new) = (home.join(d).join("fleet"), home.join(d).join("herdsman"));
        if new.exists() {
            writeln!(out, "nothing to move: {} already exists", new.display())?;
            continue;
        }
        if !old.is_dir() {
            writeln!(out, "nothing to move: no {}", old.display())?;
            continue;
        }
        if let Err(e) = std::fs::rename(&old, &new) {
            let mut msg = format!("moving {} -> {} failed: {e}", old.display(), new.display());
            for (o, n) in moved.iter().rev() {
                msg += &match std::fs::rename(n, o) {
                    Ok(()) => format!("; rolled back {} -> {}", n.display(), o.display()),
                    Err(e) => format!("; ROLLBACK FAILED {} -> {}: {e}", n.display(), o.display()),
                };
            }
            bail!(msg);
        }
        writeln!(out, "moved {} -> {}", old.display(), new.display())?;
        moved.push((old, new));
    }
    Ok(())
}

/// Seconds since the epoch.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Apply one queue change and log it (`herdsman queue`, and the page's ▲/▼ with `via`); the new order as shown.
pub fn queue_change(ctx: &Ctx, cfg: &Config, fleet: &str, d: Value, via: Option<&str>) -> Result<String> {
    cfg.fleet(fleet)?;
    let events = read(&ctx.state)?;
    let mut q = queue::replay(&events, fleet);
    queue::apply(&mut q, &d)?;
    // #425: blocking an issue with a lane holds that lane until the blocker merges.
    let n = d["issues"][0].as_u64().filter(|_| d["op"] == "block");
    let s = n.and_then(|n| events::fold(&events).remove(&(fleet.into(), n)));
    let held = s.and_then(|s| s.active_lane().map(|l| json!({"agent": l.1, "on": d["on"]})));
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, fleet, None, "queue", with_via(d, via)),
    )?;
    if let Some(h) = held {
        emit(&ctx.state, cfg, Event::new(ctx.now, fleet, n, "held", h))?;
    }
    let order: Vec<String> = q.iter().map(|i| format!("#{}", i.issue)).collect();
    let order = if order.is_empty() {
        "empty".into()
    } else {
        order.join(" ")
    };
    Ok(format!("{fleet} queue: {order}"))
}

/// `herdsman set` (and the page's model/effort dropdowns, with `via`): `wanted` is (key, new value).
pub fn set(
    ctx: &Ctx,
    out: &mut dyn Write,
    fleet: &str,
    role: &str,
    wanted: Vec<(&str, String)>,
    via: Option<&str>,
) -> Result<()> {
    let text = std::fs::read_to_string(&ctx.config)?;
    let cfg = Config::parse(&text)?;
    let cur = cfg.fleet(fleet)?.role(role)?.clone();
    if wanted.is_empty() {
        bail!("nothing to set: pass --harness, --model, --profile or --effort");
    }
    // Values become TOML strings: a quote, backslash or control char could inject lines (e.g. `[[notify]]`).
    if let Some((k, v)) = wanted
        .iter()
        .find(|(_, v)| v.is_empty() || v.chars().any(|c| c == '"' || c == '\\' || c.is_control()))
    {
        return gate::refused(
            "BAD_VALUE",
            format!("{k} {v:?}: empty, or has a quote, backslash or control char"),
        );
    }
    if let Some((_, m)) = wanted.iter().find(|(k, m)| *k == "model" && !cfg.models.contains(m)) {
        return gate::refused("BAD_VALUE", format!("model {m:?} is not in `models` {:?}", cfg.models));
    }
    let mut new = text.clone();
    let mut changes = vec![];
    for (key, val) in wanted {
        let old = match key {
            "harness" => Some(cur.harness.clone()),
            "model" => cur.model.clone(),
            "profile" => cur.profile.clone(),
            _ => cur.effort.clone(),
        };
        let old = old.unwrap_or_else(|| "-".into());
        writeln!(out, "{fleet}.{role} {key}: {old} -> {val}")?;
        if old != val {
            new = set_value(&new, fleet, role, key, &val)?;
            changes.push(with_via(json!({"role": role, "key": key, "old": old, "new": val}), via));
        }
    }
    let new_cfg = Config::parse(&new)?;
    if new != text {
        write_atomic(&ctx.config, &new)?;
    }
    for c in changes {
        emit(&ctx.state, &new_cfg, Event::new(ctx.now, fleet, None, "set", c))?;
    }
    Ok(())
}

/// Event data `d`, plus `"via"` for the page's writes.
fn with_via(mut d: Value, via: Option<&str>) -> Value {
    if let Some(v) = via {
        d["via"] = json!(v);
    }
    d
}

/// `herdsman pause|resume <fleet>` (and the page's button, with `via`): `paused = true|false`.
pub fn pause(ctx: &Ctx, out: &mut dyn Write, fleet: &str, paused: bool, via: Option<&str>) -> Result<()> {
    let text = std::fs::read_to_string(&ctx.config)?;
    let cfg = Config::parse(&text)?;
    let word = if paused { "paused" } else { "resumed" };
    if cfg.fleet(fleet)?.paused == paused {
        writeln!(out, "{fleet}: already {word}")?;
        return Ok(());
    }
    let new = set_in(&text, &format!("[fleet.{fleet}]"), "paused", &paused.to_string())?;
    let new_cfg = Config::parse(&new)?;
    write_atomic(&ctx.config, &new)?;
    emit(
        &ctx.state,
        &new_cfg,
        Event::new(
            ctx.now,
            fleet,
            None,
            if paused { "pause" } else { "resume" },
            with_via(json!({}), via),
        ),
    )?;
    writeln!(out, "{fleet}: {word}")?;
    Ok(())
}

/// #99: owner words a routine ask may not use (`OWNER_TOPIC`); a backstop, not the definition (roles/usage-tail.md).
const OWNER_WORDS: &str = "main production prod prods secret secrets token tokens delete deleted deletes deleting \
    deletion spend spends spending spent cost costs costly dependency dependencies scope scopes";

/// `herdsman ask`: log a `needs-decision` for the asking lane (`HERDSMAN_LANE`, else none), notify, print its id.
pub fn ask(
    ctx: &Ctx,
    cfg: &Config,
    out: &mut dyn Write,
    (fleet, issue): (&str, Option<u64>),
    q: &str,
    opts: &[String],
    (rec, kind, thread): (Option<usize>, &str, Option<&str>),
) -> Result<()> {
    cfg.fleet(fleet)?;
    let text = format!("{q} {}", opts.join(" ")).to_lowercase();
    let mut words = text.split(|c: char| !c.is_alphanumeric());
    if kind == "routine" && rec.is_none() {
        return gate::refused("NO_RECOMMENDATION", "a routine ask needs --recommend <n>");
    }
    if kind == "routine" && (text.contains('$') || words.any(|w| OWNER_WORDS.split_whitespace().any(|o| o == w))) {
        return gate::refused("OWNER_TOPIC", "this ask touches an owner topic; ask it as --kind owner");
    }
    if q.trim().is_empty() || opts.len() > 4 || rec.is_some_and(|n| n == 0 || n > opts.len()) {
        return gate::refused(
            "BAD_VALUE",
            "an ask is a non-empty question, at most 4 options, --recommend one of them",
        );
    }
    let events = read(&ctx.state)?;
    if let Some(t) = thread {
        conductor::find(&events, t)?; // #355: its options become the thread's choice rows
    }
    let id = format!("q{}", events.iter().filter(|e| e.kind == "needs-decision").count() + 1);
    // `<fleet>:<issue|->:<role>` names the newest spawn of that lane.
    let lane = std::env::var("HERDSMAN_LANE").unwrap_or_default();
    let at = |e: &Event| {
        format!(
            "{}:{}:{}",
            e.fleet,
            e.issue.map_or("-".into(), |n| n.to_string()),
            e.str("role").unwrap_or_default()
        )
    };
    // #446: a lane asking during its startup is already its `spawn-started`, not the lane before it.
    let starts = |e: &&Event| matches!(e.kind.as_str(), "spawn" | "spawn-started");
    let spawned = events.iter().rev().filter(starts).find(|e| at(e) == lane);
    let (a, h) = spawned.map_or((None, None), |e| (e.str("agent"), e.str("host"))); // #325: and its host
    let mut d =
        json!({"id": id, "question": q, "options": opts, "recommend": rec, "agent": a, "host": h, "kind": kind});
    if let Some(t) = thread {
        d["thread"] = json!(t);
    }
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, issue, "needs-decision", d))?;
    Ok(writeln!(out, "{id}")?)
}

/// `herdsman answer` (and the page's needs-you card, with `via`): log the decision, prompt the asking agent.
pub fn answer(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn runner::Runner,
    out: &mut dyn Write,
    (fleet, id): (&str, &str),
    choice: &str,
    (by, via): (&str, Option<&str>),
) -> Result<()> {
    let events = read(&ctx.state)?;
    let is = |e: &&Event, k: &str| e.kind == k && e.fleet == fleet && e.str("id") == Some(id);
    let Some(ask) = events.iter().find(|e| is(e, "needs-decision")) else {
        return gate::refused("NO_SUCH_ASK", format!("{fleet} has no ask {id}"));
    };
    let last = events.iter().rev().find(|e| is(e, "decision"));
    if let Some(d) = last.filter(|d| d.str("by") != Some("default")) {
        return gate::refused(
            "ALREADY_ANSWERED",
            format!(
                "{id} was answered by {}: {}",
                d.str("by").unwrap_or("?"),
                d.str("choice").unwrap_or("?")
            ),
        );
    }
    let text = choice
        .parse::<usize>()
        .ok()
        .and_then(|n| ask.data["options"].get(n.checked_sub(1)?)?.as_str())
        .unwrap_or(choice);
    if text.trim().is_empty() || text.chars().any(char::is_control) || text.chars().count() > 2000 {
        let why = "an answer is an option number or one line of text, at most 2,000 chars";
        return gate::refused("BAD_VALUE", why);
    }
    // #412: a design's decision acts first (a failed revert leaves the ask open), and before the kick.
    let design = crate::design::is_ask(ask);
    if design {
        crate::design::decide(ctx, cfg, r, ask, text, by)?;
    }
    let d = with_via(json!({"id": id, "choice": text, "by": by}), via);
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, ask.issue, "decision", d))?;
    let why = via.map_or("answer", |_| "board");
    if let Some(n) = ask.issue {
        let _ = crate::drive::kick(ctx, r, fleet, n, why); // #304: after the decision is logged
    }
    let mins = cfg.limits.ask_default_minutes;
    let note = (by == "default").then(|| format!(" (default after {mins} min, nobody replied)"));
    let msg = format!("DECISION {id}: {text}{}", note.unwrap_or_default());
    if let Some(t) = ask.str("thread") {
        let agent = conductor::say(ctx, cfg, r, (t, &msg, &[]), (by, true))?; // #355: back into the thread's conversation
        let q = ask.str("question").unwrap_or_default();
        conductor::flow(ctx, cfg, t, &format!("answered {id}: {q} → {text}"))?; // #413: into the thread it came from
        return Ok(writeln!(out, "{fleet} {id}: {text} -> {agent}")?);
    }
    if design {
        return Ok(writeln!(out, "{fleet} {id}: {text}")?);
    }
    let Some(agent) = deliver(ctx, cfg, r, ask, &msg, true)? else {
        let merged = events::closed(&events, ask) == Some("closed: merged");
        let open = ask.issue.filter(|_| !merged);
        let next = open.map(|n| format!("; the next lane on #{n} gets it in its brief"));
        let gone = format!("recorded; asking agent gone, not prompted{}", next.unwrap_or_default());
        return Ok(writeln!(out, "{fleet} {id}: {text} {gone}")?);
    };
    Ok(writeln!(out, "{fleet} {id}: {text} -> {agent}")?)
}

/// #325: the lane an answer to `ask` goes to: the asking agent's, else (unknown or closed) the issue's current one.
fn answer_lane<'a>(events: &'a [Event], ask: &Event) -> Option<&'a Event> {
    let open = |sp: &&Event| {
        let lane = ["lane-closed", "lane-killed", "lane-reaped"];
        let ends = |e: &Event| lane.contains(&e.kind.as_str()) && events::ends(e, sp);
        let mut later = events.iter().skip_while(|e| !std::ptr::eq(*e, *sp));
        !later.any(|e| e.fleet == ask.fleet && (ends(e) || e.kind == "merged" && e.issue == ask.issue))
    };
    let on_issue = |e: &&Event| ask.issue.is_some_and(|n| e.is("spawn", &ask.fleet, n));
    let own = ask.str("agent").and_then(|a| events::last_spawn(events, a));
    own.filter(open).or_else(|| events.iter().rfind(on_issue)).filter(open)
}

/// #325: type the answer `msg` into its lane, Enter; log `answer-delivered`, or (`first`: it must land) `-undelivered`.
pub fn deliver(ctx: &Ctx, cfg: &Config, r: &dyn Runner, ask: &Event, msg: &str, first: bool) -> Result<Option<String>> {
    let events = read(&ctx.state)?;
    let sp = answer_lane(&events, ask);
    // #424: no open lane, but the asking agent still runs in its lane's worktree: typed into that pane.
    let was = ask.str("agent").and_then(|a| events::last_spawn(&events, a));
    let agent = sp.or(was).and_then(|e| e.str("agent")).unwrap_or_default();
    let on = &crate::hosts::On::of(r, sp.or(was));
    let wt = sp.map_or(was.and_then(|e| e.str("worktree")), |_| None);
    let all = wt.and_then(|_| crate::host::agents(on).ok()).unwrap_or_default();
    let inside = crate::host::live_in(&all, "", std::path::Path::new(wt.unwrap_or_default()));
    let live = inside.into_iter().find(|a| a["name"] == agent);
    let pane = sp.and_then(|e| e.str("pane")).or_else(|| live?["pane_id"].as_str());
    let typed = |p| herdr::prompt(on, agent, msg).and_then(|_| herdr::keys(on, p, "Enter"));
    let why = match pane.map(|p| (p, typed(p))) {
        None => "no open lane for this ask".into(),
        Some((_, Err(e))) => format!("{e:#}"),
        Some((p, Ok(_))) if first && !herdr::landed(on, agent, p, msg) => "the agent did not pick it up".into(),
        Some(_) => String::new(),
    };
    let kind = match (why.is_empty(), live) {
        (false, _) => "answer-undelivered",
        (_, Some(_)) => "answer-delivered-live",
        _ => "answer-delivered",
    };
    let d = json!({"id": ask.str("id"), "agent": pane.and(Some(agent)), "reason": why, "text": msg});
    if first || why.is_empty() {
        emit(&ctx.state, cfg, Event::new(ctx.now, &ask.fleet, ask.issue, kind, d))?;
    }
    Ok(why.is_empty().then(|| agent.into()))
}

/// #325: answers whose last delivery failed, newest first (one per ask).
pub fn undelivered<'a>(events: &'a [Event], fleet: &str) -> Vec<&'a Event> {
    let tries = |e: &&Event| e.fleet == fleet && e.kind.starts_with("answer-");
    let mut seen = std::collections::BTreeSet::new();
    let last = events.iter().rev().filter(tries).filter(|e| seen.insert(e.str("id")));
    last.filter(|e| e.kind == "answer-undelivered").collect()
}

/// `herdsman asks`: the open asks (`all`: closed ones too, with why), oldest first, with age.
pub fn asks(ctx: &Ctx, out: &mut dyn Write, fleet: Option<&str>, as_json: bool, all: bool) -> Result<()> {
    let events = read(&ctx.state)?;
    let shown = |e: &&Event| fleet.is_none_or(|f| f == e.fleet) && (all || events::closed(&events, e).is_none());
    let open: Vec<&Event> = events
        .iter()
        .filter(|e| e.kind == "needs-decision")
        .filter(shown)
        .collect();
    if as_json {
        // #446: and why each closed (`closed`, null while open).
        let mut rows = Vec::new();
        for e in &open {
            let mut v = serde_json::to_value(e)?;
            v["closed"] = json!(events::closed(&events, e));
            rows.push(v);
        }
        writeln!(out, "{}", serde_json::to_string_pretty(&rows)?)?;
    }
    for e in open.iter().filter(|_| !as_json) {
        let at = e.issue.map_or("-".into(), |n| format!("#{n}"));
        let s = |k| e.str(k).unwrap_or_default();
        let why = events::closed(&events, e).map_or(String::new(), |w| format!(" [{w}]"));
        writeln!(
            out,
            "{} {} {at} {} {}: {}{why}",
            e.fleet,
            s("id"),
            events::age(ctx.now.saturating_sub(e.ts)),
            s("agent"),
            s("question")
        )?;
    }
    Ok(())
}
