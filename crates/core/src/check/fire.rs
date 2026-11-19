use crate::config::Config;
use crate::events::{emit, plain, Event};
use crate::Ctx;
use anyhow::Result;
use std::io::Write;

thread_local!(pub(super) static PUSHES: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) });

pub(super) fn fire(
    ctx: &Ctx,
    cfg: &Config,
    out: &mut dyn Write,
    events: &[Event],
    (fleet, issue): (&str, Option<u64>),
    kind: &str,
    d: serde_json::Value,
) -> Result<()> {
    fire_event(ctx, cfg, out, events, Event::new(ctx.now, fleet, issue, kind, d))
}

fn fire_event(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, events: &[Event], ev: Event) -> Result<()> {
    let key = ev.data.get("key");
    if (events.iter())
        .any(|e| e.kind == ev.kind && e.fleet == ev.fleet && e.issue == ev.issue && e.data.get("key") == key)
    {
        return Ok(());
    }
    writeln!(out, "{}", plain(&ev))?;
    if ev.str("wave").is_some() {
        return crate::events::append(&ctx.state, &ev);
    }
    let ((sent, held), push) = (PUSHES.get(), ev.is_push());
    if push && sent >= cfg.limits.max_pushes_per_pass {
        PUSHES.set((sent, held + 1));
        return crate::events::append(&ctx.state, &ev);
    }
    PUSHES.set((sent + usize::from(push), held));
    emit(&ctx.state, cfg, ev)
}
