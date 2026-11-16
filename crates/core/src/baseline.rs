use crate::config::{set_in, write_atomic, Baseline, Config, Fleet};
use crate::events::{emit, Event};
use crate::overlap::git;
use crate::runner::Runner;
use crate::Ctx;
use anyhow::{Context, Result};
use serde_json::json;
use std::io::Write;
use std::path::Path;

/// #1099: what a lane worktree's `git log` and `blame` see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    Since(Baseline),
    Full(Why),
}

/// #1099: why a worktree keeps its full history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    NoBaseline,
    CutBefore { head: String },
    Failed(String),
}

impl View {
    /// Spawn's output line; none while the fleet has no baseline.
    pub fn line(&self, branch: &str) -> Option<String> {
        match self {
            View::Since(b) => Some(format!("history: since {}", b.short())),
            View::Full(Why::NoBaseline) => None,
            View::Full(Why::CutBefore { head }) => Some(format!(
                "history: full ({branch}: {head} was cut before the baseline; it gets the view once it merges integration in)"
            )),
            View::Full(Why::Failed(e)) => Some(format!("history: full (baseline-failed: {e})")),
        }
    }
    /// The brief's History line, for `Since` only.
    pub fn brief(&self) -> Option<String> {
        let View::Since(b) = self else { return None };
        Some(format!(
            "History: `git log` and `git blame` here start at {}, the fleet's baseline. Older commits still open by sha; \
             `drover why <file:line>` and `git --no-replace-objects log` see through it.",
            b.short()
        ))
    }
}

/// Clone level, idempotent, from any worktree of the clone: config first, a reset if the baseline changed, then the missing grafts.
pub fn grafts(r: &dyn Runner, f: &Fleet, dir: &Path) -> Result<()> {
    let g = |a: &[&str]| git(r, dir, a);
    let had = g(&["config", "--get", "drover.baseline"])
        .ok()
        .filter(|s| !s.is_empty());
    let want = f.baseline.as_ref().map(Baseline::sha);
    let int = format!("origin/{}", f.integration);
    if let Some(t) = want.filter(|t| had.as_deref() != Some(*t)) {
        g(&["merge-base", "--is-ancestor", t, &int]).with_context(|| format!("{t:.12} is not on {int}"))?;
        g(&["config", "extensions.worktreeConfig", "true"])?;
        g(&["config", "core.useReplaceRefs", "false"])?;
    }
    if had.is_some() && had.as_deref() != want {
        let list = g(&["worktree", "list", "--porcelain"])?;
        for wt in list.lines().filter_map(|l| l.strip_prefix("worktree ")) {
            git(
                r,
                Path::new(wt),
                &["config", "--worktree", "--unset", "core.useReplaceRefs"],
            )
            .ok();
        }
        let refs = g(&["replace", "-l"])?;
        let refs: Vec<&str> = refs.lines().filter(|l| !l.is_empty()).collect();
        if !refs.is_empty() {
            g(&[&["replace", "-d"][..], &refs].concat())?;
        }
    }
    let Some(t) = want else {
        if had.is_some() {
            g(&["config", "--unset", "drover.baseline"])?;
            g(&["config", "--unset", "core.useReplaceRefs"]).ok();
        }
        return Ok(());
    };
    if had.as_deref() != Some(t) {
        g(&["config", "drover.baseline", t])?;
    }
    let have = g(&["replace", "-l"])?;
    let cut = g(&["rev-list", "--boundary", "HEAD", &int, &format!("^{t}")])?;
    let boundary = cut.lines().filter_map(|l| l.strip_prefix('-'));
    for sha in std::iter::once(t).chain(boundary.filter(|s| *s != t)) {
        if !have.lines().any(|h| h == sha) {
            g(&["replace", "--graft", sha])?;
        }
    }
    Ok(())
}

/// Worktree level: `grafts`, then the switch, on only if every head contains the baseline. Never an error: a failure is `Full(Failed)`.
pub fn view(r: &dyn Runner, f: &Fleet, wt: &Path, heads: &[&str]) -> View {
    let v = match (grafts(r, f, wt), &f.baseline) {
        (Err(e), _) => View::Full(Why::Failed(format!("{e:#}"))),
        (Ok(()), None) => return View::Full(Why::NoBaseline),
        (Ok(()), Some(b)) => {
            let cut = heads
                .iter()
                .find(|h| git(r, wt, &["merge-base", "--is-ancestor", b.sha(), h]).is_err());
            cut.map_or(View::Since(b.clone()), |h| {
                View::Full(Why::CutBefore { head: (*h).into() })
            })
        }
    };
    let on = ["config", "--worktree", "core.useReplaceRefs", "true"];
    if let View::Full(_) = v {
        git(r, wt, &["config", "--worktree", "--unset", "core.useReplaceRefs"]).ok();
        return v;
    }
    match git(r, wt, &on) {
        Ok(_) => v,
        Err(e) => View::Full(Why::Failed(format!("{e:#}"))),
    }
}

/// `drover baseline <fleet> [<sha>]`: resolve (default the integration tip), check it is on integration, write the key, log `baseline-set`.
pub fn set(ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write, fleet: &str, sha: Option<&str>) -> Result<()> {
    let text = std::fs::read_to_string(&ctx.config)?;
    let cfg = Config::parse(&text)?;
    let f = cfg.fleet(fleet)?;
    let dir = f.local_path();
    let int = format!("origin/{}", f.integration);
    git(r, &dir, &["fetch", "-q", "origin", &f.integration])?;
    let want = sha.unwrap_or(&int);
    let Ok(t) = git(
        r,
        &dir,
        &["rev-parse", "--verify", "--quiet", &format!("{want}^{{commit}}")],
    ) else {
        return crate::gate::refused("BAD_VALUE", format!("{want} is no commit in {}", dir.display()));
    };
    if git(r, &dir, &["merge-base", "--is-ancestor", &t, &int]).is_err() {
        let why = format!(
            "{want} is not on {}; a lane's branch could never contain it",
            f.integration
        );
        return crate::gate::refused("BASELINE_OFF_INTEGRATION", why);
    }
    let old = f.baseline.as_ref().map(|b| b.short().to_string());
    if f.baseline.as_ref().is_some_and(|b| b.sha() == t) {
        writeln!(out, "drover baseline: already {t:.12}")?;
        return Ok(());
    }
    let new = set_in(&text, &format!("[fleet.{fleet}]"), "baseline", &format!("\"{t}\""))?;
    let new_cfg = Config::parse(&new)?;
    write_atomic(&ctx.config, &new)?;
    let d = json!({"sha": t, "old": f.baseline.as_ref().map(Baseline::sha), "by": "owner"});
    emit(
        &ctx.state,
        &new_cfg,
        Event::new(ctx.now, fleet, None, "baseline-set", d).passive(),
    )?;
    let count = |a: &[&str]| git(r, &dir, a).map_or(0, |o| o.lines().count());
    let seen = count(&["rev-list", "--boundary", &int, &format!("^{t}")]);
    let all = count(&["rev-list", &int]);
    writeln!(
        out,
        "drover baseline: {} -> {t:.12} ({seen} of {all} commits; branches cut before it keep full history)",
        old.as_deref().unwrap_or("none")
    )?;
    Ok(())
}

/// The workflow page's "History baseline" section.
pub fn doc() -> String {
    "\n## History baseline\n\n\
     `drover baseline <fleet> [<sha>]` (the owner's; default: the integration tip) sets `[fleet.<f>] baseline`. A lane \
     worktree whose branch contains it sees `git log`, `blame` and `shortlog` stop there: the baseline and the fork \
     point of each older branch merged since are grafted parentless (`git replace --graft`). Nothing is rewritten: \
     old shas still open with `git show`, GitHub, pushes and `refs/notes/why` are untouched, and drover's own git \
     (gate, merge preview, `drover why`) runs with `--no-replace-objects`. The owner's checkout and a branch cut \
     before the baseline keep full history; spawn prints `history: full (…)` for the latter.\n\n\
     ```mermaid\nstateDiagram-v2\n    [*] --> Full: worktree add\n    \
     Full --> Since: spawn, every head contains the baseline\n    \
     Full --> Full: spawn, no baseline, a head cut before it, or git failed (baseline-failed)\n    \
     Since --> Full: spawn, a head cut before it\n    \
     Since --> Full: the clone's baseline changed or was removed\n    \
     Since --> Full: by hand, git config --worktree core.useReplaceRefs false\n```\n\n\
     Undo, at four sizes:\n\n\
     - one command: `git --no-replace-objects log`;\n\
     - one worktree, until its lane's next spawn: `git config --worktree core.useReplaceRefs false`;\n\
     - one clone, until drover's next spawn there: `git replace -d $(git replace -l)`;\n\
     - the fleet: remove `baseline` from `[fleet.<f>]`; each clone drops its grafts at its next spawn or preview.\n"
        .into()
}
