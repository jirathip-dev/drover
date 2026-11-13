//! #574: `drover amend`: an owner-decided amendment written into the issue body itself (a body edit, never a
//! comment), so checks that read the spec (board-freeze) see it, and their failed runs on the open PR go again.
use crate::config::Config;
use crate::events::{emit, read, Event};
use crate::runner::{json, ok, Runner};
use crate::Ctx;
use crate::{gate, gh};
use anyhow::Result;
use serde_json::json;
use std::io::Write;

/// The PR checks that read the issue's spec: a failed run of one re-runs after an amendment.
const SPEC_CHECKS: &[&str] = &["board-freeze"];

/// Append `- **Amendment (<basis>)**: <text>` under the body's `## Amendments` (made once), log `amend`, then
/// re-run the open PR's failed spec checks. `basis` must be an ask on this issue answered by the owner (or by
/// default); else `NO_DECISION`, before any `gh` call.
pub fn amend(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, issue): (&str, u64),
    basis: &str,
    text: &str,
) -> Result<()> {
    let f = cfg.fleet(fleet)?;
    let events = read(&ctx.state)?;
    let is = |e: &&Event, k: &str| e.kind == k && e.fleet == fleet && e.str("id") == Some(basis);
    let asked = events.iter().find(|e| is(e, "needs-decision")).and_then(|a| a.issue);
    let by = events.iter().rev().find(|e| is(e, "decision"));
    let by = by.and_then(|d| d.str("by"));
    if asked != Some(issue) || !matches!(by, Some("owner" | "default" | crate::TYPED)) {
        let why = format!("{basis} is no ask on {fleet}#{issue} the owner answered");
        return gate::refused("NO_DECISION", why);
    }
    let text = text.trim();
    if text.is_empty() || text.chars().any(char::is_control) {
        return gate::refused("BAD_VALUE", "an amendment is one line of text");
    }
    let n = issue.to_string();
    let body = json(r, "gh", &["issue", "view", &n, "-R", &f.repo, "--json", "body"])?;
    let body = body["body"].as_str().unwrap_or_default().trim_end();
    let head = if body.lines().any(|l| l.trim() == "## Amendments") {
        "\n"
    } else {
        "\n\n## Amendments\n"
    };
    let body = format!("{body}{head}- **Amendment ({basis})**: {text}\n");
    ok(r, "gh", &["issue", "edit", &n, "-R", &f.repo, "--body", &body])?;
    let d = json!({"id": basis, "text": text});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(issue), "amend", d))?;
    writeln!(out, "{fleet} #{issue}: amended ({basis})")?;
    let branch = gate::branch_of(gate::issue_state(ctx, fleet, issue)?.as_ref(), issue);
    let a = [
        "pr",
        "view",
        &branch,
        "-R",
        &f.repo,
        "--json",
        "state,statusCheckRollup",
    ];
    let pr = json(r, "gh", &a).unwrap_or_default();
    let checks = pr["statusCheckRollup"].as_array().filter(|_| pr["state"] == "OPEN");
    for c in checks.into_iter().flatten() {
        let (name, url) = (gh::check_name(c), c["detailsUrl"].as_str().unwrap_or_default());
        let run = url.split("/runs/").nth(1).and_then(|t| t.split('/').next());
        if let Some(run) = run.filter(|_| SPEC_CHECKS.contains(&name) && gh::check_state(c) == "failed") {
            ok(r, "gh", &["run", "rerun", run, "-R", &f.repo, "--failed"])?;
            writeln!(out, "{fleet} #{issue}: re-ran {name} (run {run})")?;
        }
    }
    Ok(())
}
