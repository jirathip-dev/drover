use super::ACTS;
use crate::config::Config;
use crate::events::{emit, working, Event};
use crate::{gh, waiting, Ctx};
use anyhow::Result;
use serde_json::json;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;

thread_local! {
    static SKIPS: RefCell<BTreeMap<u64, String>> = const { RefCell::new(BTreeMap::new()) };
}

pub(super) fn clear() {
    SKIPS.with_borrow_mut(BTreeMap::clear);
}

pub(super) fn skip(n: u64, why: String) {
    SKIPS.with_borrow_mut(|m| m.insert(n, why));
}

pub(super) fn cards(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, fleet: &str) -> Result<()> {
    let sessions = crate::link::view::View::load(ctx, cfg)?.sessions().checked()?;
    let (agents, ev, f) = (sessions.running(), working(&ctx.state)?, cfg.fleet(fleet)?);
    let log = waiting::Log::new(&ev);
    let acted = |n: u64| ACTS.with_borrow(|m| m.get(fleet).is_some_and(|a| a.iter().any(|a| a["issue"] == n)));
    for s in waiting::guarded(&ev, cfg)
        .iter()
        .filter(|s| s.fleet == fleet && !acted(s.issue))
    {
        let pr = gh::cached_pr(ctx, fleet, s.issue);
        let Some(since) = waiting::undriven(&log, s, f, &pr, &agents, ctx.now) else {
            continue;
        };
        let marked = since + waiting::ORPHAN_SECS;
        let passes = ev
            .iter()
            .filter(|e| e.kind == "drive-pass" && e.fleet == fleet && e.ts >= marked);
        let key = format!("undriven:{since}");
        if passes.count() < 2
            || ev
                .iter()
                .any(|e| e.is("stall", fleet, s.issue) && e.str("key") == Some(&key))
        {
            continue;
        }
        let step = SKIPS.with_borrow(|m| m.get(&s.issue).cloned());
        let step = step.unwrap_or_else(|| "not in drive's plan".into());
        let reason = format!(
            "nothing drives this (since {}): {step}",
            crate::events::age(ctx.now.saturating_sub(since))
        );
        writeln!(out, "{fleet} #{} stall: {reason}", s.issue)?;
        let d = json!({"key": key, "reason": reason, "step": step});
        emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(s.issue), "stall", d))?;
    }
    Ok(())
}
