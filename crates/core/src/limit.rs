use crate::config::Config;
use crate::events::{append, emit, fold, last_spawn, read, Event};
use crate::hosts::On;
use crate::runner::Runner;
use crate::{gate, herdr, host, Ctx};
use anyhow::Result;
use serde_json::{json, Value};
use std::io::Write;

/// The prompt a resumed or waited-out lane gets.
pub const CONTINUE: &str = "continue";

/// The owner's `pick` (`switch` | `wait`) for `fleet` #`n`'s rate-limited lane, logged `limit-picked` by `by`.
pub fn pick(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, n): (&str, u64),
    pick: &str,
    by: &str,
) -> Result<()> {
    let events = read(&ctx.state)?;
    let lane = fold(&events).remove(&(fleet.to_string(), n));
    let lane = lane.as_ref().and_then(|s| s.active_lane());
    let sp = lane.and_then(|(_, l)| last_spawn(&events, l));
    let on = On::of(r, sp);
    let agents = host::agents(&on).unwrap_or_default();
    let limited = |a: &&Value| lane.is_some_and(|l| a["name"] == l.1) && a["agent_status"] == "rate-limited";
    let (Some((role, lane)), Some(sp), Some(a)) = (lane, sp, agents.iter().find(limited)) else {
        return gate::refused("NOT_RATE_LIMITED", format!("{fleet} #{n} has no rate-limited lane"));
    };
    let account = sp.str("account");
    if pick == "switch" && account.is_none() {
        return gate::refused(
            "NO_ACCOUNT",
            format!("{lane} runs on no [accounts] account: none to switch from"),
        );
    }
    let reset = a["agent_note"].as_str().unwrap_or_default();
    let words = format!("resets {}", reset.replace(" AM", "am").replace(" PM", "pm"));
    let at = crate::access::cap_until(cfg, r, &words, ctx.now);
    let d = json!({"agent": lane, "pick": pick, "by": by, "account": account, "reset": reset, "at": at});
    append(&ctx.state, &Event::new(ctx.now, fleet, Some(n), "limit-picked", d))?;
    let Some(acct) = account.filter(|_| pick == "switch") else {
        let when = crate::usage::date(r, at, "+%H:%M");
        return Ok(writeln!(out, "{fleet} #{n}: {lane} gets `{CONTINUE}` at {when}")?);
    };
    let why = format!("{acct}: lane {lane} is rate-limited; the owner switched it to the next account");
    let d = json!({"key": format!("{lane}:{acct}"), "reason": why, "agent": lane, "account": acct, "until": at,
        "harness": sp.str("harness"), "host": sp.str("host")});
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, fleet, Some(n), "account-capped", d),
    )?;
    let o = crate::spawn::Opts {
        supersede: true,
        resume: herdr::session_id(&on, lane),
        ..Default::default()
    };
    crate::spawn::spawn(ctx, cfg, r, out, (fleet, n, role), &o).map(drop)
}

/// Who picked: `by`, or in an orchestrator's thread the owner's `line` there since the lane went rate-limited.
pub fn by<'a>(ctx: &Ctx, line: Option<&str>, by: &'a str, (fleet, n): (&str, u64)) -> Result<&'a str> {
    let Some(t) = std::env::var("DROVER_THREAD").ok().filter(|t| !t.is_empty()) else {
        return Ok(by);
    };
    let events = read(&ctx.state)?;
    let limited = |e: &&Event| e.is("lane-state", fleet, n) && e.str("state") == Some("rate-limited");
    let since = events.iter().rfind(limited).map_or(u64::MAX, |e| e.seq);
    let said = |m: &Event| m.kind == "thread-msg" && m.seq > since && m.str("from") == Some("owner");
    if events
        .iter()
        .any(|m| said(m) && m.str("thread") == Some(&t) && m.str("text") == line)
    {
        return Ok(crate::TYPED);
    }
    gate::refused(
        "NOT_TYPED",
        format!("no owner line in {t} like that since #{n}'s lane hit its limit"),
    )
}

/// Each [wait] picked since its lane's latest limit, its reset time come and the lane still rate-limited: `continue`, once.
pub fn due(ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write, ev: &[Event], agents: &[Value], fleet: &str) -> Result<()> {
    let waits = ev
        .iter()
        .filter(|e| e.kind == "limit-picked" && e.fleet == fleet && e.str("pick") == Some("wait"));
    for p in waits {
        let lane = p.str("agent").unwrap_or_default();
        let mine = |k: &'static str| move |e: &&Event| e.kind == k && e.str("agent") == Some(lane);
        let hit = ev
            .iter()
            .filter(mine("lane-state"))
            .rfind(|e| e.str("state") == Some("rate-limited"));
        let sent = ev.iter().filter(mine("limit-continued")).any(|e| e.seq > p.seq);
        let limited = agents
            .iter()
            .any(|a| a["name"] == lane && a["agent_status"] == "rate-limited");
        let at = p.data.get("at").and_then(Value::as_u64).unwrap_or(u64::MAX);
        if ctx.now < at || sent || !limited || hit.is_some_and(|h| h.seq > p.seq) {
            continue;
        }
        let sent = herdr::prompt(&On::of(r, last_spawn(ev, lane)), lane, CONTINUE).is_ok();
        writeln!(out, "{fleet} {lane}: its reset time came; sent `{CONTINUE}`")?;
        let d = json!({"agent": lane, "sent": sent});
        append(&ctx.state, &Event::new(ctx.now, fleet, p.issue, "limit-continued", d))?;
    }
    Ok(())
}
