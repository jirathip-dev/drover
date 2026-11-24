use crate::config::Config;
use crate::events::{append, Event};
use crate::runner::{ok, Runner};
use crate::Ctx;
use anyhow::Result;
use serde_json::json;
use std::io::Write;

pub(super) fn in_flight(ctx: &Ctx, out: &mut dyn Write, (fleet, n): (&str, u64), why: &str) -> Result<()> {
    let e = Event::new(ctx.now, fleet, Some(n), "kick-in-flight", json!({"why": why}));
    append(&ctx.state, &e.passive())?;
    Ok(writeln!(
        out,
        "kick {fleet} #{n}: one runs already; it drives this one after it"
    )?)
}

pub(super) fn coalesced(ctx: &Ctx, r: &dyn Runner, (fleet, n): (&str, u64), why: &str) -> bool {
    let Some(l) = super::held(ctx, &format!("kick-{fleet}-{n}.lock")) else {
        return false;
    };
    if super::gone(r, &l, ctx.now) {
        return false;
    }
    let ttl = crate::config::limits().spawn_pending_max_secs;
    let again = format!("kick-{fleet}-{n}.again");
    drop(crate::db::take(&ctx.state, &again, ctx.now, ttl, why, &|_| true));
    true
}

pub(super) fn overdue(ctx: &Ctx, cfg: &Config, r: &dyn Runner, l: &crate::db::Lease, remove: bool) -> bool {
    let (age, max) = (ctx.now.saturating_sub(l.at), super::job::limit(ctx, cfg, l));
    let id = l.key.strip_prefix("kick-").and_then(|k| k.strip_suffix(".lock"));
    let Some((fleet, n)) = id.and_then(|i| i.rsplit_once('-')).filter(|_| age > max) else {
        return false;
    };
    if remove && l.pid != 0 && l.host == crate::db::host() && !super::gone(r, l, ctx.now) {
        let killed = ok(r, "kill", &["-TERM", &format!("-{}", l.pid)]).is_ok();
        let d = json!({"pid": l.pid, "age": age, "limit": max, "killed": killed});
        append(
            &ctx.state,
            &Event::new(ctx.now, fleet, n.parse().ok(), "kick-killed", d).passive(),
        )
        .ok();
    }
    true
}
