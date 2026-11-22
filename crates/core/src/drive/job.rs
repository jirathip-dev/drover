use super::{take, Lock, ACTS, KICK};
use crate::config::Config;
use crate::events::IssueState;
use crate::runner::Runner;
use crate::waiting::Waiting;
use crate::Ctx;
use anyhow::Result;
use serde_json::json;
use std::io::Write;

pub(super) fn planned(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (s, w): (&IssueState, &Waiting),
) -> Result<bool> {
    let (fleet, n) = (s.fleet.as_str(), s.issue);
    let spawn = w.next_cmd.as_deref().is_some_and(|c| c.starts_with("drover spawn "));
    if KICK.get().is_some() || cfg.limits.max_parallel_spawns == 0 || w.who != "drive" || spawn {
        return Ok(false);
    }
    let pid = super::kick(ctx, r, fleet, n, "plan");
    writeln!(
        out,
        "{fleet} #{n} job: {}",
        if pid.is_some() { "kicked" } else { "not started" }
    )?;
    let a = json!({"issue": n, "step": "job", "outcome": "pending", "words": "pending", "kick": true});
    ACTS.with_borrow_mut(|m| m.entry(fleet.into()).or_default().push(a));
    Ok(true)
}

pub(super) fn slot(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str) -> Option<Lock> {
    let (t0, secs) = (
        crate::now(),
        cfg.limits.kick_max_secs + cfg.limits.gate_check_minutes * 60,
    );
    for _ in 0..=cfg.limits.kick_max_secs {
        let ctx = &Ctx {
            now: ctx.now + crate::now().saturating_sub(t0),
            ..ctx.clone()
        };
        let free = (0..cfg.limits.job_max).find_map(|i| take(ctx, r, &format!("job-{fleet}-{i}.lock"), secs, ""));
        if free.is_some() {
            return free;
        }
        r.sleep(1);
    }
    None
}

pub(crate) fn merging(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str, n: u64) -> Result<Lock> {
    let key = format!("merge-{fleet}.lock");
    if let Some(l) = take(ctx, r, &key, cfg.limits.gate_check_minutes * 60, &n.to_string()) {
        return Ok(l);
    }
    let by = super::held(ctx, &key).map_or("another issue".into(), |l| format!("#{}", l.data));
    crate::gate::refused(
        "IN_FLIGHT",
        format!("skipped: {by} is merging in {fleet}; one merge at a time"),
    )
}

pub(super) fn limit(ctx: &Ctx, cfg: &Config, l: &crate::db::Lease) -> u64 {
    let merge = crate::db::leases(&ctx.state, "merge-");
    if merge.iter().any(|m| m.pid == l.pid && m.host == l.host) {
        return cfg.limits.gate_check_minutes * 60;
    }
    cfg.limits.kick_max_secs
}
