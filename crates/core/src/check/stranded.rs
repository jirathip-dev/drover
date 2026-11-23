use super::fire::fire_event;
use crate::config::Config;
use crate::events::{age, Event};
use crate::Ctx;
use anyhow::Result;
use serde_json::json;
use std::io::Write;

pub(super) fn alert(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, events: &[Event]) -> Result<()> {
    let all = crate::events::with_states(events, |st| crate::queue::stranded(events, st, cfg));
    let mut by = std::collections::BTreeMap::<(String, u64), Vec<crate::queue::Stranded>>::new();
    for s in all {
        by.entry((s.fleet.clone(), s.issue)).or_default().push(s);
    }
    for ((fleet, n), ss) in by {
        let key: Vec<&str> = ss.iter().map(|s| s.blocker.as_str()).collect();
        let why: Vec<String> = ss
            .iter()
            .map(|s| {
                let what = if s.closed {
                    "is closed unmerged"
                } else {
                    "is open in no queue with no lane"
                };
                format!("blocker {} {what}: run `{}`", s.blocker, s.next)
            })
            .collect();
        let d = json!({"key": key.join(","), "reason": format!("#{n} never starts: {}", why.join("; "))});
        fire_event(
            ctx,
            cfg,
            out,
            events,
            Event::new(ctx.now, &fleet, Some(n), "blocker-stranded", d).push(),
        )?;
    }
    drive_stalled(ctx, cfg, out, events)
}

fn drive_stalled(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, events: &[Event]) -> Result<()> {
    let due = crate::waiting::guarded(events, cfg);
    for (name, f) in &cfg.fleet {
        let Some(last) = crate::drive::last_pass(ctx, name) else {
            continue;
        };
        let late = ctx.now.saturating_sub(last);
        if f.paused || late <= 3 * cfg.limits.slow_secs || !due.iter().any(|s| s.fleet == *name) {
            continue;
        }
        let lock = format!("drive-{name}.lock");
        let error = events
            .iter()
            .rev()
            .take_while(|e| e.ts >= last)
            .find_map(|e| match e.kind.as_str() {
                "pass-slow" if e.fleet == *name => Some(format!("killed after {} s in {}", e.val("secs"), step(e))),
                "pass-stuck" if e.str("lock") == Some(&lock) => Some(format!("lock taken over {}", step(e))),
                _ => None,
            });
        let error = error.unwrap_or_else(|| "no error logged: see drive.log".into());
        let why = format!(
            "{name}: no drive pass finished for {} with work due; {error}",
            age(late)
        );
        let d = json!({"key": last, "last": last, "error": error, "reason": why});
        fire_event(
            ctx,
            cfg,
            out,
            events,
            Event::new(ctx.now, name, None, "drive-stalled", d).push(),
        )?;
    }
    Ok(())
}

fn step(e: &Event) -> &str {
    e.str("step").unwrap_or("?")
}
