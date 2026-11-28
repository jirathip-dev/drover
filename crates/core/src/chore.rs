use crate::config::Config;
use crate::events::{emit, working, Event};
use crate::Ctx;
use anyhow::Result;
use serde_json::json;

const LESSON_FAILS: usize = 2;

fn chat<'a>(events: &'a [Event], fleet: &str) -> Option<&'a str> {
    let mine = |e: &&Event| e.kind == "orchestrator-attached" && e.fleet == fleet && e.str("via") != Some("profile");
    let ids = events.iter().filter(mine).filter_map(|e| e.str("thread"));
    let ids: Vec<&str> = ids.collect();
    ids.into_iter()
        .rev()
        .find(|t| crate::orchestrator::attached(events, t).is_some_and(|a| a.fleet == fleet))
}

fn since<'a>(events: &'a [Event], fleet: &str, chore: &str) -> impl Iterator<Item = &'a Event> {
    let reset = |e: &Event| {
        e.fleet == fleet
            && (e.str("chore") == Some(chore)
                || chore == "lessons" && e.kind == "job-filed" && e.str("job") == Some("lesson"))
    };
    let at = events.iter().rposition(reset).map_or(0, |i| i + 1);
    events.get(at..).unwrap_or_default().iter()
}

/// Chore `chore` of `fleet`: a `system` row whose prompt is `line` and `cmd` to the repo's attached chat, else an owner card naming the command.
pub fn nudge(ctx: &Ctx, cfg: &Config, fleet: &str, chore: &str, (line, cmd): (&str, &str)) -> Result<()> {
    let events = working(&ctx.state)?;
    let text = format!("{line}: `{cmd}`");
    let e = match chat(&events, fleet) {
        Some(t) => {
            let d = json!({"thread": t, "from": "system", "text": text, "chore": chore});
            Event::new(ctx.now, fleet, None, "thread-msg", d)
        }
        None => Event::new(
            ctx.now,
            fleet,
            None,
            "nudge",
            json!({"chore": chore, "reason": text, "cmd": cmd}),
        )
        .push(),
    };
    emit(&ctx.state, cfg, e.passive())
}

/// After a merge into `fleet`'s integration branch: every `[limits] promote_after_merges` merges since the last nudge, a promotion to main.
pub fn promote(ctx: &Ctx, cfg: &Config, fleet: &str) -> Result<()> {
    let (every, f) = (cfg.limits.promote_after_merges, cfg.fleet(fleet)?);
    let events = working(&ctx.state)?;
    let merged = since(&events, fleet, "promote").filter(|e| e.kind == "merged" && e.fleet == fleet);
    if every == 0 || u64::try_from(merged.count()).unwrap_or_default() < every {
        return Ok(());
    }
    let line = format!("{} has {every} merges not promoted to main", f.integration);
    let cmd = format!(
        "gh pr create -R {} -B main -H {} -t \"Promote {} to main\" -b \"\"",
        f.repo, f.integration, f.integration
    );
    nudge(ctx, cfg, fleet, "promote", (&line, &cmd))
}

/// After a review verdict of `fleet`: its second FAIL since the last lessons nudge or lesson issue, run the lesson job.
pub fn lessons(ctx: &Ctx, cfg: &Config, fleet: &str) -> Result<()> {
    let events = working(&ctx.state)?;
    let fail = |e: &&Event| {
        e.kind == "verdict" && e.fleet == fleet && e.str("verdict") == Some("FAIL") && e.val("form_only") != true
    };
    if since(&events, fleet, "lessons").filter(fail).count() < LESSON_FAILS {
        return Ok(());
    }
    let line = format!("{fleet} has {LESSON_FAILS} review FAILs since its last lessons run");
    nudge(
        ctx,
        cfg,
        fleet,
        "lessons",
        (&line, &format!("drover job {fleet} lesson")),
    )
}
