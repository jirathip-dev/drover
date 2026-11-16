use crate::events::{append, fold, last_spawn, read, Event};
use crate::hosts::On;
use crate::runner::Runner;
use crate::session::{Session, Status};
use crate::{gate, term, Ctx};
use anyhow::Result;
use serde_json::{json, Value};
use std::io::Write;

/// The prompt a waited-out lane gets.
pub const CONTINUE: &str = "continue";

/// The owner's [wait] for `fleet` #`n`'s rate-limited lane: logged `limit-picked` by `by`; `due` sends `continue` at shed's reset time.
pub fn wait(ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write, (fleet, n): (&str, u64), by: &str) -> Result<()> {
    let events = read(&ctx.state)?;
    let lane = fold(&events).remove(&(fleet.to_string(), n));
    let lane = lane.as_ref().and_then(|s| s.active_lane()).map(|(_, l)| l.to_string());
    let on = On::of(r, lane.as_deref().and_then(|l| last_spawn(&events, l)));
    let a = lane.as_deref().and_then(|l| crate::session::get(&on, l).ok().flatten());
    let Some(a) = a.filter(|a| a.status() == Some(Status::Limited)) else {
        return gate::refused("NOT_RATE_LIMITED", format!("{fleet} #{n} has no rate-limited lane"));
    };
    let reset = a.note.as_deref().unwrap_or_default();
    let Some(at) = reset_at(r, reset, ctx.now) else {
        return gate::refused(
            "NO_RESET",
            format!("{} is rate-limited with no reset time from shed", a.name),
        );
    };
    let d = json!({"agent": a.name, "pick": "wait", "by": by, "reset": reset, "at": at});
    append(
        &ctx.state,
        &Event::new(ctx.now, fleet, Some(n), "limit-picked", d).passive(),
    )?;
    let when = crate::usage::date(r, at, "+%H:%M");
    Ok(writeln!(out, "{fleet} #{n}: {} gets `{CONTINUE}` at {when}", a.name)?)
}

fn reset_at(r: &dyn Runner, note: &str, now: u64) -> Option<u64> {
    let s = note
        .to_lowercase()
        .replace([',', '(', ')', '.'], " ")
        .replace(" am", "am")
        .replace(" pm", "pm");
    let words: Vec<&str> = s.split_whitespace().collect();
    let secs = words.iter().find_map(|w| {
        let hm = w.strip_suffix("am").or(w.strip_suffix("pm"))?;
        let (h, m) = hm.split_once(':').unwrap_or((hm, "0"));
        let pm = if w.ends_with("pm") { 12 } else { 0 };
        Some((h.parse::<u64>().ok()? % 12 + pm) * 3600 + m.parse::<u64>().ok()? * 60)
    })?;
    let months = "jan feb mar apr may jun jul aug sep oct nov dec";
    let date = words.windows(2).find_map(|w| {
        let [mon, day] = w else { return None };
        let m = u64::try_from(months.split(' ').position(|x| mon.starts_with(x))?).ok()? + 1;
        Some((m, day.parse::<u64>().ok()?))
    });
    let z = crate::usage::date(r, now, "+%z").parse::<i64>().unwrap_or(0);
    let off = z / 100 * 3600 + z % 100 * 60;
    let today = now.saturating_add_signed(off) / 86400;
    (today..today + 400)
        .filter(|&d| date.is_none_or(|md| md == crate::access::civil_month_day(d)))
        .map(|d| (d * 86400 + secs).saturating_add_signed(-off))
        .find(|&t| t > now)
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
pub fn due(
    ctx: &Ctx,
    r: &dyn Runner,
    out: &mut dyn Write,
    ev: &[Event],
    agents: &[Session],
    fleet: &str,
) -> Result<()> {
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
            .any(|a| a.name == lane && a.status() == Some(Status::Limited));
        let at = p.data.get("at").and_then(Value::as_u64).unwrap_or(u64::MAX);
        if ctx.now < at || sent || !limited || hit.is_some_and(|h| h.seq > p.seq) {
            continue;
        }
        let sent = term::prompt(&On::of(r, last_spawn(ev, lane)), lane, CONTINUE).is_ok();
        writeln!(out, "{fleet} {lane}: its reset time came; sent `{CONTINUE}`")?;
        let d = json!({"agent": lane, "sent": sent});
        append(
            &ctx.state,
            &Event::new(ctx.now, fleet, p.issue, "limit-continued", d).passive(),
        )?;
    }
    Ok(())
}
