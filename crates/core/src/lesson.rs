//! #419: the lesson role: `herdsman lesson digest` (a fleet's recent mistakes) and `herdsman lesson` (its issue and
//! lane), run weekly by `tick` and on demand by a conductor.
use crate::access::{civil_month_day, civil_year};
use crate::config::Config;
use crate::events::{emit, plain, read, Event};
use crate::runner::{ok, Runner};
use crate::Ctx;
use anyhow::{Context, Result};
use serde_json::json;
use std::io::Write;

/// `herdsman lesson digest`: the last `days` of rev FAIL findings, the owner's lines in the fleet's conductor threads,
/// `by: owner` events and reverts on integration, secret-looking values redacted.
pub fn digest(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str, days: u64) -> Result<String> {
    let f = cfg.fleet(fleet)?;
    let since = ctx.now.saturating_sub(days * 86400);
    let events = read(&ctx.state)?;
    let recent: Vec<&Event> = events.iter().filter(|e| e.fleet == fleet && e.ts >= since).collect();
    let mut s =
        format!("# Lessons digest: {fleet}, last {days} days\n\n## Review FAIL findings (blocking, with proof)\n");
    for e in recent
        .iter()
        .filter(|e| e.kind == "verdict" && e.str("verdict") == Some("FAIL"))
    {
        let n = e.issue.unwrap_or_default();
        let round = events
            .iter()
            .filter(|v| v.is("verdict", fleet, n) && v.ts <= e.ts)
            .count();
        for (_, ls) in crate::gate::findings(e.str("body").unwrap_or_default())
            .into_iter()
            .filter(|f| f.0)
        {
            s += &format!("\n#{n} round {round}:\n{}\n", ls.join("\n"));
        }
    }
    s += "\n## The owner in conductor threads\n\n";
    let owner_msg = |e: &&&Event| {
        e.kind == "thread-msg" && e.str("from").is_some_and(|f| f != "conductor") && crate::conductor::link(e).is_none()
    };
    for e in recent.iter().filter(owner_msg) {
        let text = e.str("text").unwrap_or_default().replace('\n', "\n  ");
        s += &format!("- {}: {text}\n", e.str("thread").unwrap_or("?"));
    }
    s += "\n## By the owner (overrides, extends, parks, answers against the recommendation)\n\n";
    let asked = |e: &Event| {
        let ask = events
            .iter()
            .find(|a| a.kind == "needs-decision" && a.fleet == fleet && a.str("id") == e.str("id"));
        let rec = ask
            .and_then(|a| a.data["options"].get(usize::try_from(a.data["recommend"].as_u64()?.checked_sub(1)?).ok()?));
        rec.and_then(|o| o.as_str()).is_some_and(|o| Some(o) != e.str("choice"))
    };
    let owner = |e: &&&Event| match e.kind.as_str() {
        "decision" => e.str("by") != Some("default") && asked(e),
        k => e.str("by") == Some("owner") && !k.starts_with("secret"),
    };
    for e in recent.iter().filter(owner) {
        s += &format!("- {}: {}\n", e.kind, plain(e));
    }
    s += "\n## Reverts on integration\n\n";
    let local = f.local_path().to_string_lossy().into_owned();
    let integration = format!("origin/{}", f.integration);
    let log = ["log", &integration, "-i", "--grep=^revert", "--format=%ct %h %s"];
    for l in ok(r, "git", &[&["-C", local.as_str()][..], &log].concat())
        .unwrap_or_default()
        .lines()
    {
        if let Some((_, c)) = l
            .split_once(' ')
            .filter(|(t, _)| t.parse::<u64>().is_ok_and(|t| t >= since))
        {
            s += &format!("- {c}\n");
        }
    }
    Ok(s.lines().map(crate::lanes::redact).collect::<Vec<_>>().join("\n") + "\n")
}

/// `herdsman lesson`: file this week's lessons issue, queue it on top and spawn its `lesson` lane; false when the
/// lane's prompt was not confirmed (exit 4, as `spawn`).
pub fn run(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<bool> {
    let f = cfg.fleet(fleet)?;
    let day = ctx.now / 86400;
    let (y, (m, d)) = (civil_year(day), civil_month_day(day));
    let title = format!("lessons: {fleet} week of {y}-{m:02}-{d:02}");
    let body = include_str!("../roles/lesson-issue.md").replace("{fleet}", fleet);
    let url = ok(
        r,
        "gh",
        &["issue", "create", "-R", &f.repo, "--title", &title, "--body", &body],
    )?;
    let n: u64 = url
        .trim()
        .rsplit('/')
        .next()
        .and_then(|n| n.parse().ok())
        .context("gh issue create: no number")?;
    writeln!(out, "filed {fleet}#{n}: {title}")?;
    let op = crate::queue::op("add", &[n], true, None, None);
    writeln!(out, "{}", crate::queue_change(ctx, cfg, fleet, op, None)?)?;
    crate::spawn::spawn(ctx, cfg, r, out, (fleet, n, "lesson"), &crate::spawn::Opts::default())
}

/// #419: `tick`'s lesson run, at most once per 7 days per fleet (`lessons = "off"` opts out; a paused fleet waits).
pub fn weekly(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write) -> Result<()> {
    for name in cfg
        .fleet
        .iter()
        .filter(|(_, f)| f.lessons.as_deref() != Some("off") && !f.paused)
        .map(|x| x.0)
    {
        let at = ctx.state.with_file_name(format!("lesson-{name}.at"));
        let last = std::fs::read_to_string(&at)
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok());
        if last.is_some_and(|t| ctx.now < t + 7 * 86400) {
            continue;
        }
        std::fs::write(&at, ctx.now.to_string())?;
        // #517: a fleet under a week old, or with under 2 FAIL verdicts this week, has no repeated class to find.
        let (week, events) = (ctx.now.saturating_sub(7 * 86400), read(&ctx.state)?);
        let young = events.iter().find(|e| e.fleet == *name).is_none_or(|e| e.ts > week);
        let fails = (events.iter())
            .filter(|e| e.fleet == *name && e.ts >= week && e.kind == "verdict" && e.str("verdict") == Some("FAIL"))
            .count();
        if young || fails < 2 {
            let why = if young {
                "fleet under a week old"
            } else {
                "under 2 FAIL verdicts this week"
            };
            writeln!(out, "{name} lesson skipped: {why}")?;
            let e = Event::new(
                ctx.now,
                name,
                None,
                "lesson-skipped",
                json!({"why": why, "fails": fails}),
            );
            crate::events::append(&ctx.state, &e)?;
            continue;
        }
        if let Err(e) = run(ctx, cfg, r, out, name) {
            writeln!(out, "{name} lesson: {e:#}")?;
        }
    }
    Ok(())
}

/// #419: a lesson lane's `done --no-change` before any review: no class repeated, so herdsman closes its issue with
/// that line.
pub fn done(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, n): (&str, u64),
    why: &str,
) -> Result<()> {
    let st = crate::gate::issue_state(ctx, fleet, n)?;
    if st.is_none_or(|s| s.build.as_deref() != Some("lesson") || s.rounds > 0) {
        return Ok(());
    }
    let repo = &cfg.fleet(fleet)?.repo;
    ok(
        r,
        "gh",
        &["issue", "close", &n.to_string(), "-R", repo, "--comment", why],
    )?;
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, fleet, Some(n), "closed", json!({"why": why})),
    )?;
    Ok(writeln!(out, "closed {fleet}#{n}: {why}")?)
}

/// #517: the one prompt a lesson lane gets when its turn ends with no push and no `done`.
pub fn prompt(fleet: &str, n: u64) -> String {
    format!("no repeated class? end with `herdsman done --no-change \"no repeated class\" {fleet} {n}`")
}
