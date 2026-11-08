//! #666: a lane that stops making progress is acted on, not left to `stall` alerts. A live lane idle past
//! `stall_minutes` with no push, verdict or turn is superseded once by a fresh lane of its role; one that shows no output
//! `START_SECS` after its spawn never started (an ACP session gets its first prompt again first) and is retried once; a
//! lane the hook keeps refusing is ended. Each then goes to the owner as one needs-human card with its evidence.
use crate::config::Config;
use crate::events::{append, emit, fold, last_spawn, Event};
use crate::hosts::On;
use crate::runner::Runner;
use crate::waiting::Waiting;
use crate::{herdr, Ctx};
use anyhow::Result;
use serde_json::{json, Value};
use std::io::Write;

/// A lane that shows no output this long after its spawn (or its first prompt sent again) never started.
pub const START_SECS: u64 = 600;

/// What drive does about a stalled lane: respawn it (`why`), or a needs-human card (code, reason).
pub enum Act {
    Respawn(String, &'static str),
    Escalate(&'static str, String),
}

/// #666: each of `fleet`'s active lanes the hook refused twice since its spawn: its pane closed, one needs-human card
/// carrying the refusal (a respawn would only be refused again).
pub fn refused(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, ev: &[Event], fleet: &str) -> Result<()> {
    for s in fold(ev).values().filter(|s| s.fleet == fleet) {
        let Some((role, lane)) = s.active_lane() else { continue };
        let Some(sp) = last_spawn(ev, lane) else { continue };
        let hits = refusals(ev, sp, role);
        let Some(last) = hits.last().filter(|_| hits.len() >= 2) else {
            continue;
        };
        if let Some(p) = sp.str("pane") {
            let _ = herdr::close(&On::of(r, Some(sp)), p);
            let d = json!({"agent": lane, "pane": p, "reason": "hook-refused"});
            append(&ctx.state, &Event::new(ctx.now, fleet, Some(s.issue), "lane-closed", d))?;
        }
        let why = format!(
            "hook refused {lane} {} times, lane ended: REFUSED HOOK_FAILED_CLOSED: {}",
            hits.len(),
            last.str("cause").unwrap_or("?")
        );
        writeln!(out, "{fleet} #{} needs-human: {why}", s.issue)?;
        let d = json!({"code": "HOOK_REFUSED", "reason": why, "agent": lane});
        emit(
            &ctx.state,
            cfg,
            Event::new(ctx.now, fleet, Some(s.issue), "needs-human", d),
        )?;
    }
    Ok(())
}

/// The `hook-failed-closed` refusals of spawn `sp`'s lane (`<fleet>:<issue>:<role>`) since it started.
fn refusals<'a>(ev: &'a [Event], sp: &Event, role: &str) -> Vec<&'a Event> {
    let (f, n) = (&sp.fleet, sp.issue.unwrap_or_default());
    let lane = format!("{f}:{n}:{role}");
    let mine =
        |e: &&Event| e.is("hook-failed-closed", f, n) && e.ts >= sp.ts && e.str("lane").is_none_or(|l| l == lane);
    ev.iter().filter(mine).collect()
}

/// #666: what to do about row `w`'s live lane, idle with no push or verdict (and no turn reported), if anything yet.
pub fn stalled(ctx: &Ctx, cfg: &Config, r: &dyn Runner, ev: &[Event], agents: &[Value], w: &Waiting) -> Option<Act> {
    let idle = ["idle without pushing", "rev idle without"];
    if w.who != "none" || w.next_cmd.is_some() || !idle.iter().any(|p| w.what.starts_with(p)) {
        return None;
    }
    let (lane, sp) = (&w.lane, last_spawn(ev, &w.lane)?);
    agents.iter().find(|a| a["name"] == lane.as_str())?;
    let (role, pane, on) = (sp.str("role")?, sp.str("pane")?, On::of(r, Some(sp)));
    let resent = |e: &&Event| e.kind == "start-resent" && e.str("agent") == Some(lane) && e.ts >= sp.ts;
    let resent = ev.iter().rfind(resent);
    let since = resent.map_or(sp.ts, |e| e.ts);
    let (start, stall) = (since + START_SECS, w.since + cfg.limits.stall_minutes * 60);
    if ctx.now < start.min(stall) {
        return None;
    }
    let screen = herdr::read(&on, pane).ok();
    let (n, _) = crate::waiting::respawns(ev, &w.fleet, w.issue);
    let ago = |t: u64| crate::events::age(ctx.now.saturating_sub(t));
    if screen.as_deref().is_some_and(|t| t.trim().is_empty()) {
        if ctx.now < start {
            return None;
        }
        // Amendment A: an ACP session whose brief never landed as its first prompt gets it again, once.
        let acp = sp.str("runtime") == Some("shed") && cfg.shed_for(&w.fleet).is_some_and(|b| b.acp.is_some());
        let brief = sp
            .str("worktree")
            .and_then(|wt| on.read(&std::path::Path::new(wt).join(".brief.md")));
        if let Some(text) = brief.filter(|_| acp && resent.is_none()) {
            let sent = herdr::prompt(&on, lane, &text).is_ok();
            let d = json!({"agent": lane, "sent": sent});
            let _ = append(
                &ctx.state,
                &Event::new(ctx.now, &w.fleet, Some(w.issue), "start-resent", d),
            );
            return None;
        }
        return Some(if n == 0 {
            Act::Respawn(role.into(), "failed start")
        } else {
            Act::Escalate(
                "START_FAILED",
                format!(
                    "lane {lane} never started: no output {} after its spawn, retried once",
                    ago(since)
                ),
            )
        });
    }
    if ctx.now < stall {
        return None;
    }
    if n == 0 {
        return Some(Act::Respawn(role.into(), "lane stalled"));
    }
    let tail = crate::lanes::tail(screen.as_deref().unwrap_or_default(), 5).join(" ⏎ ");
    let hook = refusals(ev, sp, role)
        .last()
        .and_then(|e| e.str("cause"))
        .map(|c| format!("; hook: {c}"));
    let why = format!(
        "lane {lane} stalled again after its respawn: no push, verdict or turn for {}; last screen: {tail}{}",
        ago(w.since),
        hook.unwrap_or_default()
    );
    Some(Act::Escalate("LANE_STALLED", why))
}
