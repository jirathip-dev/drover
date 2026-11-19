use crate::config::{Config, Fleet};
use crate::events::{fold, last_spawn, working, Json};
use crate::hosts::On;
use crate::runner::{argv, json, Runner};
use crate::Ctx;
use anyhow::Result;
use std::path::Path;
use std::time::Duration;

/// The refusal code of a `turn done` whose head is unpushed or fails a done check.
pub const CODE: &str = "DONE_CHECK";

/// What `turn done`'s gate found: nothing to gate (no open PR with an unreviewed head), a pass at the head, or a refusal (code, why, head).
pub enum Gate {
    Skip,
    Pass(String),
    Refused(&'static str, String, String),
}

/// `turn done`'s hard gate for `role`'s lane, in its worktree on its host: the worktree at the PR head, the PR body anchored there, then each done check.
pub fn gate(ctx: &Ctx, cfg: &Config, r: &dyn Runner, (fleet, n): (&str, u64), role: &str) -> Result<Gate> {
    let f = cfg.fleet(fleet)?;
    let events = working(&ctx.state)?;
    let Some(s) = fold(&events).remove(&(fleet.to_string(), n)) else {
        return Ok(Gate::Skip);
    };
    let agent = s.lanes.get(role).cloned().unwrap_or_default();
    let (Some(sp), branch) = (last_spawn(&events, &agent), crate::gate::branch_of(Some(&s), n)) else {
        return Ok(Gate::Skip);
    };
    let fields = ["pr", "view", &branch, "-R", &f.repo, "--json", "body,headRefOid,state"];
    let pr = json(r, "gh", &fields).unwrap_or_default();
    let head = pr.at("headRefOid").as_str().unwrap_or_default().to_string();
    if head.is_empty() || pr.at("state") != "OPEN" || s.reviewed(&head) {
        return Ok(Gate::Skip);
    }
    let (on, wt) = (On::of(r, Some(sp)), sp.str("worktree").unwrap_or_default());
    let git = |a: &[&str]| {
        on.run("git", &argv(&[&["-C", wt][..], a].concat()))
            .ok()
            .filter(|o| o.code == 0)
            .map(|o| o.stdout.trim().to_string())
    };
    let refuse = |code: &'static str, why: String| Ok(Gate::Refused(code, why, head.clone()));
    let at = git(&["rev-parse", "HEAD"]).unwrap_or_default();
    if !at.eq_ignore_ascii_case(&head) {
        let why = format!("push first: the worktree is at {at:.7}, the PR head is {head:.7}");
        return refuse(CODE, why);
    }
    let subject = |c: &str| git(&["log", "-1", "--format=%s", c]);
    let parent = |c: &str| git(&["rev-parse", &format!("{c}^1")]);
    let text = pr.at("body").as_str().unwrap_or_default();
    if let Err(e) = body(text, &head, &f.integration, &subject, &parent) {
        return refuse("PR_BODY", e);
    }
    let limit = Duration::from_secs(cfg.limits.gate_check_minutes * 60);
    for c in checks(f, &on, Path::new(wt)) {
        let sh = argv(&["-c", &format!("cd \"$0\" && {c}"), wt]);
        let o = match on.run_for("sh", &sh, "", limit) {
            Ok(o) if o.code == 0 => continue,
            Ok(o) => o,
            Err(e) => return refuse(CODE, format!("`{c}` cannot start: {e:#}")),
        };
        let what = match o.code {
            124 => format!("timed out after {} min", cfg.limits.gate_check_minutes),
            126 | 127 => "cannot start".into(),
            x => format!("failed (exit {x})"),
        };
        let all = crate::lanes::mask(&format!("{}{}", o.stdout, o.stderr));
        let lines: Vec<&str> = all.lines().filter(|l| !l.trim().is_empty()).collect();
        let tail = lines.get(lines.len().saturating_sub(10)..).unwrap_or_default();
        return refuse(CODE, format!("`{c}` {what}:\n{}", tail.join("\n")));
    }
    Ok(Gate::Pass(head))
}

/// The fleet's done checks; unset = arch-check where the worktree has `.drover/arch.toml`, `cargo fmt --check` where it has a `Cargo.toml`.
fn checks(f: &Fleet, on: &On, wt: &Path) -> Vec<String> {
    if let Some(c) = &f.done_checks {
        return c.clone();
    }
    let (arch, cargo) = (wt.join(".drover/arch.toml"), wt.join("Cargo.toml"));
    let (arch, cargo) = (arch.to_string_lossy(), cargo.to_string_lossy());
    let there = on.existing(&[&arch, &cargo]);
    [(&arch, "drover arch-check"), (&cargo, "cargo fmt --check")]
        .into_iter()
        .filter(|(p, _)| there.contains(p.as_ref()))
        .map(|(_, c)| c.to_string())
        .collect()
}

/// `body` has `## For humans`, no "Generated with" line, and names `head` as `scripts/evidence-check.sh` does.
pub fn body(
    body: &str,
    head: &str,
    base: &str,
    subject: &dyn Fn(&str) -> Option<String>,
    parent: &dyn Fn(&str) -> Option<String>,
) -> Result<(), String> {
    let lines = || body.lines().map(str::trim);
    if !lines().any(|l| l.eq_ignore_ascii_case("## For humans")) {
        return Err("the PR body has no `## For humans` section: add 2-4 plain lines, `gh pr edit --body`".into());
    }
    let generated = |l: &str| {
        l.trim_start_matches(|c: char| !c.is_ascii_alphanumeric())
            .starts_with("Generated with")
    };
    if let Some(l) = lines().find(|l| generated(l)) {
        return Err(format!("the PR body carries `{l}`: drop it, `gh pr edit --body`"));
    }
    let low = body.to_ascii_lowercase();
    let names = |c: &str| {
        let c = c.get(..7).unwrap_or(c).to_ascii_lowercase();
        (low.match_indices(&c)).any(|(i, _)| !low[..i].ends_with(|ch: char| ch.is_ascii_hexdigit()))
    };
    let merge = |s: &str| {
        let s = s
            .strip_prefix("Merge ")
            .map(|s| s.strip_prefix("remote-tracking ").unwrap_or(s));
        let s = s.map(|s| s.strip_prefix("branch ").unwrap_or(s));
        let s = s.map(|s| s.strip_prefix('\'').unwrap_or(s));
        let s = s.and_then(|s| s.strip_prefix("origin/").unwrap_or(s).strip_prefix(base));
        s.is_some_and(|t| matches!(t.strip_prefix('\'').unwrap_or(t).chars().next(), None | Some(' ')))
    };
    let mut c = head.to_string();
    while !names(&c) {
        match subject(&c).filter(|s| merge(s)).and_then(|_| parent(&c)) {
            Some(p) => c = p,
            None => {
                return Err(format!(
                    "the PR body's evidence names no sha of head {head:.7}: re-run the gates at the head and `gh pr edit --body` with it"
                ))
            }
        }
    }
    Ok(())
}
