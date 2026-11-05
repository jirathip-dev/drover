//! #389: a remote lane's command never reaches the main host itself: it waits in its worktree's outbox, which drive
//! pulls over main → host ssh (#249) and runs.
use crate::events::{append, fold, last_spawn, read, Event};
use crate::{hosts::On, runner::Runner, Ctx};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{io::Write, path::PathBuf};

pub const QUEUED: &str = "queued for the main host: drive delivers it within 1 min\n";

/// A lane on another host (spawn exports `HERDSMAN_OUTBOX`): its `HERDSMAN_LANE`.
pub fn lane() -> Option<String> {
    std::env::var_os("HERDSMAN_OUTBOX")?;
    std::env::var("HERDSMAN_LANE").ok().filter(|l| !l.is_empty())
}

/// The lane's side: `args` as one line (`args`, `lane`, `ts` in ms) in the worktree holding the cwd.
pub fn queue(lane: &str, args: &[String]) -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    let wt = cwd.ancestors().find(|d| d.join(".git").exists());
    let p = wt.context("no worktree holds the cwd")?.join(".herdsman/outbox.jsonl");
    std::fs::create_dir_all(p.parent().context("no dir")?)?;
    let ts = std::time::UNIX_EPOCH.elapsed()?.as_millis();
    let mut f = std::fs::File::options().create(true).append(true).open(&p)?;
    writeln!(f, "{}", json!({"args": args, "lane": lane, "ts": ts}))?;
    Ok(p)
}

/// Each remote lane of a live issue: its spawn, `HERDSMAN_LANE` and outbox path.
fn boxes(events: &[Event]) -> Vec<(&Event, String, String)> {
    let live = fold(events).into_values().filter(|s| !s.done());
    let spawns = live.flat_map(|s| s.lanes.into_values());
    let remote = spawns.filter_map(|a| last_spawn(events, &a));
    let remote = remote.filter(|sp| sp.str("machine").is_some());
    let lane = |sp: &Event| Some(format!("{}:{}:{}", sp.fleet, sp.issue?, sp.str("role")?));
    remote
        .filter_map(|sp| Some((sp, lane(sp)?, format!("{}/.herdsman/outbox.jsonl", sp.str("worktree")?))))
        .collect()
}

/// The outbox moved aside (a leftover one first), then read; it is removed once every line was applied.
fn take(r: &dyn Runner, sp: &Event, p: &str) -> Result<String> {
    let sh = "d=\"$1.draining\"; [ -s \"$d\" ] || mv -f \"$1\" \"$d\" 2>/dev/null; cat \"$d\" 2>/dev/null; exit 0";
    crate::runner::ok(&On::of(r, Some(sp)), "sh", &crate::runner::argv(&["-c", sh, "sh", p]))
}

/// #337: the step's look reads every outbox in its one call per host.
pub fn look(r: &dyn Runner, events: &[Event]) {
    for (sp, _, p) in boxes(events) {
        drop(take(r, sp, &p));
    }
}

/// drive's side: each live remote lane's outbox of `fleets` (as the look `seen` read it), line by line, then cleared.
pub fn drain(ctx: &Ctx, (r, seen): (&dyn Runner, &dyn Runner), out: &mut dyn Write, fleets: &[&str]) -> Result<()> {
    let events = read(&ctx.state)?;
    for (sp, lane, p) in boxes(&events) {
        let text = fleets.contains(&sp.fleet.as_str()).then(|| take(seen, sp, &p));
        let Some(Ok(text)) = text.map(|t| t.inspect_err(|e| drop(writeln!(out, "outbox {lane}: {e:#}")))) else {
            continue;
        };
        for l in text.lines().filter(|l| !l.trim().is_empty()) {
            apply(ctx, r, out, sp, &lane, l)?;
        }
        if !text.trim().is_empty() {
            On::of(r, Some(sp)).remove(format!("{p}.draining").as_ref())?;
        }
    }
    Ok(())
}

/// One line, once per lane + ts: run as `lane` (from its spawn, never the line), or refused; logged `outbox` either way.
fn apply(ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write, sp: &Event, lane: &str, raw: &str) -> Result<()> {
    let v: Value = serde_json::from_str(raw).unwrap_or_default();
    let (ts, events) = (v["ts"].as_u64(), read(&ctx.state)?);
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
        writeln!(out, "outbox {lane}: {}", args[0])?;
        std::env::set_var("HERDSMAN_LANE", lane); // drive runs with no `HERDSMAN_LANE` of its own
        let res = crate::cli::run(&args.iter().map(String::as_str).collect::<Vec<_>>(), ctx, r, out);
        std::env::remove_var("HERDSMAN_LANE");
        d["code"] = json!(res.as_ref().ok());
        d["error"] = json!(res.err().map(|e| format!("{e:#}")));
    }
    append(&ctx.state, &Event::new(ctx.now, &sp.fleet, sp.issue, "outbox", d))
}

/// `check`'s side: each live remote lane whose outbox holds a line drive has not delivered in 5 min, with its oldest ts.
pub fn stale(r: &dyn Runner, events: &[Event], now: u64) -> Vec<(Event, String, u64)> {
    let sh = "cat \"$1\" \"$1.draining\" 2>/dev/null; exit 0";
    let oldest = |sp: &Event, p: &str| {
        let text = crate::runner::ok(&On::of(r, Some(sp)), "sh", &crate::runner::argv(&["-c", sh, "sh", p]));
        let ts = text
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok()?["ts"].as_u64())
            .min();
        ts.filter(|ts| now.saturating_mul(1000).saturating_sub(*ts) > 5 * 60 * 1000)
    };
    boxes(events)
        .into_iter()
        .filter_map(|(sp, lane, p)| Some((sp.clone(), lane, oldest(sp, &p)?)))
        .collect()
}
