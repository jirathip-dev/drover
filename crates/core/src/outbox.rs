//! #389: a lane on another host whose ssh forward to the main host fails queues the command in its worktree's
//! outbox; drive's fast step reads every remote lane's outbox over main → host ssh and runs each line there as
//! that lane (the #249 pattern).
use crate::events::{append, fold, last_spawn, read, Event};
use crate::runner::{argv, ok, Runner};
use crate::Ctx;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const FILE: &str = ".herdsman/outbox.jsonl";
pub const QUEUED: &str = "queued for the main host (no route): drive delivers it within 1 min\n";

/// The lane's side: `args` as one line (`args`, `lane`, `ts` in ms) in the worktree holding the cwd.
pub fn queue(lane: &str, args: &[String]) -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    let wt = cwd
        .ancestors()
        .find(|d| d.join(".git").exists())
        .context("no worktree holds the cwd")?;
    let p = wt.join(FILE);
    std::fs::create_dir_all(wt.join(".herdsman"))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis();
    let mut f = std::fs::File::options().create(true).append(true).open(&p)?;
    writeln!(f, "{}", json!({"args": args, "lane": lane, "ts": ts}))?;
    Ok(p)
}

/// The outbox moved aside (a leftover one first), then read; it is removed once every line was applied.
const TAKE: &str =
    "[ -s \"$1.draining\" ] || mv -f \"$1\" \"$1.draining\" 2>/dev/null; cat \"$1.draining\" 2>/dev/null; exit 0";

/// Each remote lane of a live issue: its spawn, `HERDSMAN_LANE` and outbox path.
fn boxes(events: &[Event]) -> Vec<(&Event, String, String)> {
    let live = fold(events).into_values().filter(|s| !s.done());
    let spawns = live
        .flat_map(|s| s.lanes.into_values())
        .filter_map(|a| last_spawn(events, &a));
    spawns
        .filter(|sp| sp.str("machine").is_some())
        .filter_map(|sp| {
            let (wt, role) = (sp.str("worktree")?, sp.str("role")?);
            let n = sp.issue.map_or("-".into(), |n| n.to_string());
            Some((
                sp,
                format!("{}:{n}:{role}", sp.fleet),
                Path::new(wt).join(FILE).display().to_string(),
            ))
        })
        .collect()
}

fn take(r: &dyn Runner, sp: &Event, p: &str) -> Result<String> {
    ok(&crate::hosts::On::of(r, Some(sp)), "sh", &argv(&["-c", TAKE, "sh", p]))
}

/// #337: the step's look reads every outbox in its one call per host.
pub fn look(r: &dyn Runner, events: &[Event]) {
    for (sp, _, p) in boxes(events) {
        let _ = take(r, sp, &p);
    }
}

/// drive's side: each live remote lane's outbox of `fleets` (as the step's look `seen` read it), line by line, then
/// cleared.
pub fn drain(ctx: &Ctx, (r, seen): (&dyn Runner, &dyn Runner), out: &mut dyn Write, fleets: &[&str]) -> Result<()> {
    let events = read(&ctx.state)?;
    for (sp, lane, p) in boxes(&events)
        .into_iter()
        .filter(|b| fleets.contains(&b.0.fleet.as_str()))
    {
        let text = match take(seen, sp, &p) {
            Ok(t) => t,
            Err(e) => {
                writeln!(out, "outbox {lane}: {e:#}")?;
                continue;
            }
        };
        for l in text.lines().filter(|l| !l.trim().is_empty()) {
            apply(ctx, r, out, sp, &lane, l)?;
        }
        if !text.trim().is_empty() {
            crate::hosts::On::of(r, Some(sp)).remove(Path::new(&format!("{p}.draining")))?;
        }
    }
    Ok(())
}

/// One line, once per lane + ts: run as `lane` (from its spawn, never the line), or refused; logged `outbox` either way.
fn apply(ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write, sp: &Event, lane: &str, raw: &str) -> Result<()> {
    let v: Value = serde_json::from_str(raw).unwrap_or_default();
    let ts = v["ts"].as_u64();
    let events = read(&ctx.state)?;
    let seen = |e: &Event| e.kind == "outbox" && e.str("lane") == Some(lane) && e.data["sent"].as_u64() == ts;
    if ts.is_some() && events.iter().any(seen) {
        return Ok(());
    }
    let args: Vec<String> = serde_json::from_value(v["args"].clone()).unwrap_or_default();
    let why = match v["lane"].as_str() {
        _ if ts.is_none() || args.is_empty() => Some("unreadable line".into()),
        Some(l) if l == lane => crate::cli::foreign(&args, lane, &events),
        _ => Some(format!("names lane {}, not {lane}", v["lane"])),
    };
    let mut d = json!({"lane": lane, "sent": ts, "args": args});
    if let Some(w) = &why {
        d["refused"] = json!(w);
        writeln!(out, "outbox {lane}: refused: {w}")?;
    } else {
        let was = std::env::var("HERDSMAN_LANE");
        std::env::set_var("HERDSMAN_LANE", lane);
        let mut buf = vec![];
        let words: Vec<&str> = args.iter().map(String::as_str).collect();
        let res = crate::cli::run(&words, ctx, r, &mut buf);
        match was {
            Ok(w) => std::env::set_var("HERDSMAN_LANE", w),
            Err(_) => std::env::remove_var("HERDSMAN_LANE"),
        }
        let res = res.map_err(|e| format!("{e:#}"));
        d["code"] = json!(res.as_ref().ok());
        d["error"] = json!(res.as_ref().err());
        let said = String::from_utf8_lossy(&buf);
        writeln!(
            out,
            "outbox {lane}: {} → {}",
            args[0],
            said.lines().next().unwrap_or_default()
        )?;
    }
    append(&ctx.state, &Event::new(ctx.now, &sp.fleet, sp.issue, "outbox", d))
}
