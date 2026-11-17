use crate::config::Config;
use crate::events::{append, emit, fold, last_spawn, Event};
use crate::hosts::On;
use crate::lanes::Health;
use crate::runner::Runner;
use crate::session::{Session, Sessions};
use crate::waiting::Waiting;
use crate::{term, Ctx};
use anyhow::Result;
use serde_json::json;
use std::io::Write;

/// A lane that shows no output this long after its spawn (or its first prompt sent again) never started.
pub const START_SECS: u64 = 600;

/// What drive does about a stalled lane: respawn it (`why`), or a needs-human card (code, reason).
pub enum Act {
    Respawn(String, &'static str),
    Escalate(&'static str, String),
}

/// #666: each of `fleet`'s active lanes the hook refused twice since its spawn: its pane closed, one needs-human card carrying the refusal (a respawn would only be refused again).
pub fn refused(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, ev: &[Event], fleet: &str) -> Result<()> {
    for s in fold(ev).values().filter(|s| s.fleet == fleet) {
        let Some((role, lane)) = s.active_lane() else { continue };
        let Some(sp) = last_spawn(ev, lane) else { continue };
        let hits = refusals(ev, sp, role);
        let Some(last) = hits.last().filter(|_| hits.len() >= 2) else {
            continue;
        };
        if let Some(p) = sp.str("pane") {
            term::close(ctx, &On::of(r, Some(sp)), p).ok();
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

fn refusals<'a>(ev: &'a [Event], sp: &Event, role: &str) -> Vec<&'a Event> {
    let (f, n) = (&sp.fleet, sp.issue.unwrap_or_default());
    let lane = format!("{f}:{n}:{role}");
    let mine =
        |e: &&Event| e.is("hook-failed-closed", f, n) && e.ts >= sp.ts && e.str("lane").is_none_or(|l| l == lane);
    ev.iter().filter(mine).collect()
}

/// #666: what to do about row `w`'s live lane that nothing else waits on, when `lanes::health` says it never started or stalled.
pub fn stalled(ctx: &Ctx, cfg: &Config, r: &dyn Runner, ev: &[Event], sessions: &Sessions, w: &Waiting) -> Option<Act> {
    if w.who != "none" || w.next_cmd.is_some() {
        return None;
    }
    let s = fold(ev).remove(&(w.fleet.clone(), w.issue))?;
    let (lane, sp) = (&w.lane, last_spawn(ev, &w.lane)?);
    let (role, on) = (sp.str("role")?, On::of(r, Some(sp)));
    let resent = |e: &&Event| e.kind == "start-resent" && e.str("agent") == Some(lane) && e.ts >= sp.ts;
    let resent = ev.iter().rfind(resent);
    let (n, _) = crate::waiting::respawns(ev, &w.fleet, w.issue);
    let ago = |t: u64| crate::events::age(ctx.now.saturating_sub(t));
    match crate::lanes::health(ctx, cfg, ev, sessions, &s) {
        Health::NeverStarted { since } => {
            let acp = sp.str("runtime") == Some("shed") && cfg.shed_for(&w.fleet).is_some_and(|b| b.acp.is_some());
            let brief = sp
                .str("worktree")
                .and_then(|wt| on.read(&std::path::Path::new(wt).join(".brief.md")));
            if let Some(text) = brief.filter(|_| acp && resent.is_none()) {
                let sent = term::prompt(&on, lane, &text).is_ok();
                let d = json!({"agent": lane, "sent": sent});
                append(
                    &ctx.state,
                    &Event::new(ctx.now, &w.fleet, Some(w.issue), "start-resent", d),
                )
                .ok();
                return None;
            }
            Some(if n == 0 {
                Act::Respawn(role.into(), "failed start")
            } else {
                Act::Escalate(
                    "START_FAILED",
                    format!(
                        "lane {lane} never started: no report {} after its spawn, retried once",
                        ago(since)
                    ),
                )
            })
        }
        Health::Stalled { .. } if n == 0 => Some(Act::Respawn(role.into(), "lane stalled")),
        Health::Stalled { since } => {
            let note = sessions
                .get(lane)
                .and_then(|x| x.note.as_deref())
                .map(crate::lanes::mask);
            let hook = refusals(ev, sp, role)
                .last()
                .and_then(|e| e.str("cause"))
                .map(|c| format!("; hook: {c}"));
            let why = format!(
                "lane {lane} stalled again after its respawn: shed quiet for {}, no turn-end; shed says: {}{}",
                ago(since),
                note.unwrap_or_else(|| "nothing".into()),
                hook.unwrap_or_default()
            );
            Some(Act::Escalate("LANE_STALLED", why))
        }
        _ => None,
    }
}

/// #819: a check lane with no verdict since its spawn and nothing new from shed this long is replaced.
pub const STALE_CHECK_SECS: u64 = 2 * 3600;

/// #819: the minutes check lane `lane` of `fleet` #`n` has been quiet (no shed event, no status change), once that is [`STALE_CHECK_SECS`].
pub fn stale_check(ctx: &Ctx, ev: &[Event], agents: &[Session], (fleet, n): (&str, u64), lane: &str) -> Option<u64> {
    let i = ev
        .iter()
        .rposition(|e| e.kind == "spawn" && e.str("agent") == Some(lane))?;
    let sp = ev.get(i)?;
    if !agents.iter().any(|a| a.name == lane) || ev.iter().skip(i + 1).any(|e| e.is("verdict", fleet, n)) {
        return None;
    }
    let p = crate::session::pulse::of(ctx, lane);
    if p.as_ref().is_some_and(|p| !p.open_tools.is_empty()) {
        return None;
    }
    let heard = [
        p.map_or(0, |p| p.heard),
        crate::session::stamp::since(ev, lane).unwrap_or(0),
        sp.ts,
    ];
    let quiet = ctx.now.saturating_sub(heard.into_iter().max().unwrap_or_default());
    (quiet >= STALE_CHECK_SECS).then_some(quiet / 60)
}

/// #792: the prompt a silent ACP lane gets once its turn is cancelled.
pub const NUDGE: &str = "continue: your last step finished or failed; carry on, or end your turn with `drover turn`";

/// #792: a lane's process tree at or over this much of one core runs its open tool call.
pub const BUSY_CPU_PCT: u64 = 10;

/// #792: what drive does about a silent ACP lane (`fleet` #`issue`'s `lane`).
pub enum Silent {
    Nudge(String),
    Respawn(String, String),
    Escalate(String),
}

pub fn silent(ctx: &Ctx, cfg: &Config, ev: &[Event], agents: &[Session], fleet: &str) -> Vec<(u64, String, Silent)> {
    let mut out = vec![];
    for s in fold(ev).values().filter(|s| s.fleet == fleet) {
        let staged = s.lanes.iter().find_map(|(r, a)| {
            let on = s.state == "design" && crate::events::STAGES.contains(&r.as_str()) && s.lane.as_ref() == Some(a);
            on.then_some((r.as_str(), a.as_str()))
        });
        let Some((role, lane)) = s.active_lane().or(staged) else {
            continue;
        };
        let (Some(sp), Some(o)) = (last_spawn(ev, lane), crate::session::pulse::of(ctx, lane)) else {
            continue;
        };
        let open = !o.open_tools.is_empty() && o.cpu_pct >= Some(BUSY_CPU_PCT);
        let Some(a) = agents.iter().find(|a| a.name == lane) else {
            continue;
        };
        if !o.turn_open || open || crate::lanes::held(ev, a) {
            continue;
        }
        let mine = |k: &'static str| move |e: &&Event| e.kind == k && e.str("agent") == Some(lane);
        let nudged = ev.iter().rfind(mine("nudged")).filter(|e| e.ts >= sp.ts);
        let since = [o.heard, sp.ts, nudged.map_or(0, |e| e.ts)];
        let since = since.into_iter().max().unwrap_or_default();
        if ctx.now < since + cfg.limits.silent_minutes * 60 {
            continue;
        }
        let hour = |e: &&Event| ctx.now < e.ts + 3600;
        let respawned = ev.iter().rfind(mine("respawned-silent")).filter(hour);
        let recent = respawned.or(ev.iter().rfind(mine("nudged")).filter(hour));
        let quiet = crate::events::age(ctx.now - since);
        let unanswered = nudged.filter(|n| o.said.is_none_or(|t| t <= n.ts));
        let act = match (unanswered, respawned, recent) {
            (Some(_), None, _) => Silent::Respawn(role.into(), format!("silent {quiet} after its nudge")),
            (_, _, Some(e)) => Silent::Escalate(format!(
                "lane {lane} went silent mid-turn again ({quiet}) within the hour after its {}",
                if e.kind == "respawned-silent" {
                    "respawn"
                } else {
                    "nudge"
                }
            )),
            _ => Silent::Nudge(format!("silent {quiet} mid-turn")),
        };
        out.push((s.issue, lane.to_string(), act));
    }
    out
}

/// #792: cancel silent lane `lane`'s turn and prompt it once; logged `nudged`.
pub fn nudge(
    ctx: &Ctx,
    r: &dyn Runner,
    out: &mut dyn Write,
    ev: &[Event],
    (fleet, n): (&str, u64),
    lane: &str,
    why: &str,
) -> Result<()> {
    let on = On::of(r, last_spawn(ev, lane));
    if let Err(e) = term::awake(&on, lane) {
        return Ok(writeln!(out, "{fleet} #{n} nudge of {lane} waits: {e:#}")?);
    }
    let cancelled = crate::shed::cli(&on, &["cancel", lane]).is_ok();
    let sent = term::prompt(&on, lane, NUDGE).is_ok();
    writeln!(out, "{fleet} #{n} nudged {lane}: {why}")?;
    let d = json!({"agent": lane, "why": why, "cancelled": cancelled, "sent": sent});
    append(&ctx.state, &Event::new(ctx.now, fleet, Some(n), "nudged", d))
}
