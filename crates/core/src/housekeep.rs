use crate::config::Config;
use crate::events::Json;
use crate::runner::Runner;
use crate::Ctx;
use anyhow::Result;
use serde_json::json;
use std::io::Write;

/// The housekeeping chores in order and the `[limits]` keys that time them; each runs under its own `slow_secs` budget.
pub const CHORES: [(&str, &[&str]); 10] = [
    (
        "check",
        &[
            "stall_minutes",
            "ask_default_minutes",
            "ask_waiting_minutes",
            "ask_expire_hours",
            "outbox_stale_minutes",
            "acp_start_minutes",
            "idle_reap_minutes",
            "done_close_minutes",
            "idle_merged_minutes",
        ],
    ),
    ("clean", &[]),
    ("budget", &[]),
    ("hogs", &[]),
    ("backlog", &[]),
    ("trim", &[]),
    ("jobs", &[]),
    ("soak", &[]),
    ("hotspots", &[]),
    ("compact", &[]),
];

fn chore(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, name: &str) -> Result<()> {
    match name {
        "check" => crate::check::check(ctx, cfg, r, out),
        "clean" => crate::check::scheduled(ctx, cfg, r, out),
        "budget" => crate::check::budget(ctx, cfg, r, out),
        "hogs" => crate::check::hogs(ctx, cfg, r, out),
        "backlog" => backlog(ctx, cfg, r, out),
        "trim" => crate::check::trim_hosts(ctx, cfg, r, out),
        "jobs" => crate::jobs::tick(ctx, cfg, r, out),
        "soak" => crate::soak::nightly(ctx, cfg, r),
        "hotspots" => crate::hotspots::daily(ctx, cfg, r, out),
        _ => compact(ctx, out),
    }
}

fn backlog(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write) -> Result<()> {
    for name in cfg.fleet.keys().filter(|n| crate::reroot::drives(ctx, cfg, r, n, out)) {
        crate::gh::backlog(ctx, r, cfg.fleet(name)?, name)
            .unwrap_or_else(|e| crate::stderr!("drover: {name} backlog: {e:#}"));
    }
    Ok(())
}

fn compact(ctx: &Ctx, out: &mut dyn Write) -> Result<()> {
    if !crate::timer::due(ctx, "compact", 86_400)? {
        return Ok(());
    }
    match crate::db::compact(&ctx.state, ctx.now) {
        Ok((gone, moved)) => writeln!(out, "compact: {gone} heartbeats deleted, {moved} events archived")?,
        Err(e) => writeln!(out, "compact: {e:#}")?,
    }
    Ok(())
}

/// `drover housekeep [--only CHORE]`: each chore in its own thread under its own `slow_secs` budget; one past it is logged and left behind while the next runs; `housekeep.last` records the run.
pub fn run(ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write, only: Option<&str>) -> Result<i32> {
    let cfg = crate::check::config(ctx)?;
    let (t0, budget) = (crate::clock::instant(), cfg.limits.slow_secs);
    let (mut late, mut failed) = (vec![], vec![]);
    let chores = CHORES.iter().map(|c| c.0).filter(|c| only.is_none_or(|o| o == *c));
    std::thread::scope(|s| -> Result<()> {
        for name in chores {
            let (tx, rx) = std::sync::mpsc::channel();
            let cfg = &cfg;
            s.spawn(move || {
                let off = crate::hosts::disabled(cfg);
                let mut pass = crate::hosts::Pass::new(r, None).sparing(off, Some((ctx, cfg)));
                pass.until = crate::clock::instant().checked_add(std::time::Duration::from_secs(budget));
                pass.step(name);
                let mut buf = vec![];
                let res = chore(ctx, cfg, &pass, &mut buf, name);
                tx.send((buf, res)).ok();
            });
            match rx.recv_timeout(std::time::Duration::from_secs(budget)) {
                Ok((buf, res)) => {
                    out.write_all(&buf)?;
                    if let Err(e) = res {
                        writeln!(out, "{name}: {e:#}")?;
                        failed.push(name);
                    }
                }
                Err(_) => {
                    writeln!(out, "housekeep: {name} past its {budget} s budget; skipped")?;
                    late.push(name);
                }
            }
        }
        let d = json!({"at": ctx.now, "secs": t0.elapsed().as_secs(), "late": late, "failed": failed});
        std::fs::write(ctx.state.with_file_name("housekeep.last"), d.to_string())?;
        Ok(())
    })?;
    Ok(i32::from(!failed.is_empty()))
}

/// The last run's start, length in seconds and the chores past their budget, from `housekeep.last`.
pub fn last(ctx: &Ctx) -> Option<(u64, u64, Vec<String>)> {
    let text = std::fs::read_to_string(ctx.state.with_file_name("housekeep.last")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let late = v.at("late").as_array().into_iter().flatten();
    let late = late.filter_map(|l| l.as_str().map(String::from)).collect();
    Some((v.at("at").as_u64()?, v.at("secs").as_u64()?, late))
}
