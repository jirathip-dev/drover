use super::fire::fire_event;
use crate::config::Config;
use crate::events::Event;
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
    Ok(())
}
