//! #417: the optional verify stage: whether it runs for an issue's head, the skip logged when it doesn't, and the
//! screenshots a verdict lists.
use crate::config::Config;
use crate::events::{emit, read, Event, IssueState};
use crate::runner::{ok, Runner};
use crate::Ctx;
use anyhow::Result;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

/// The repo's recipe the verifier follows.
pub const RECIPE: &str = ".herdsman/verify.md";

/// Why `fleet` #`n`'s verify stage runs (`None`: it doesn't): forced by its queue mark, else a design stage or a
/// PR touching `ui_paths`.
pub fn why(events: &[Event], ui_paths: &[String], (fleet, n): (&str, u64), files: &[String]) -> Option<String> {
    let mark = crate::queue::replay(events, fleet)
        .into_iter()
        .find(|i| i.issue == n)
        .and_then(|i| i.verify);
    let ui = || {
        let hit = |p: &&String| ui_paths.iter().any(|g| crate::config::glob(g.as_bytes(), p.as_bytes()));
        files.iter().find(hit).map(|p| format!("touches {p}"))
    };
    match mark {
        Some(on) => on.then(|| "queued --verify".into()),
        None if events.iter().any(|e| e.is("design-approved", fleet, n)) => Some("had a design stage".into()),
        None => ui(),
    }
}

/// Drive's step before spawning the verify lane at a PASS: true if it runs; else `verify-skipped` (not needed) or
/// `verify-no-recipe` (needed, but the head has no recipe) is logged at that sha, which passes the stage there.
pub fn due(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, s: &IssueState, pr: &Value) -> Result<bool> {
    let (fleet, n, f) = (s.fleet.as_str(), s.issue, cfg.fleet(&s.fleet)?);
    let Some((sha, _, from)) = s.verdicts.last().filter(|v| v.1 == "PASS") else {
        return Ok(true);
    };
    let stage = f.flow().next(from, "PASS").map_or("verify", |t| t.0);
    let num = pr["number"].to_string();
    let files = crate::gh::diff_names(ctx, r, f, (fleet, n), &num, sha).unwrap_or_default();
    let events = read(&ctx.state)?;
    let (kind, why) = match why(&events, &f.ui_paths, (fleet, n), &files) {
        None => (
            "verify-skipped",
            "no design stage, no UI files, not queued --verify".to_string(),
        ),
        Some(w) => {
            let api = format!("repos/{}/contents/{RECIPE}?ref={sha}", f.repo);
            if ok(r, "gh", &["api", &api, "--silent"]).is_ok() {
                return Ok(true);
            }
            ("verify-no-recipe", format!("{w}, but the repo has no {RECIPE}"))
        }
    };
    writeln!(out, "{fleet} #{n} {kind} at {sha:.7}: {why}")?;
    let d = json!({"sha": sha, "stage": stage, "reason": why});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), kind, d))?;
    Ok(false)
}

/// The PNGs verdict.md (at `file`) lists, as `{name, path}`: a path relative to it is in its lane's folder.
pub fn shots(body: &str, file: &Path) -> Vec<Value> {
    let dir = file.parent().unwrap_or(Path::new(""));
    let words = body.split(|c: char| c.is_whitespace() || "`()[]<>\"'".contains(c));
    let mut out: Vec<Value> = vec![];
    for w in words.map(|w| w.trim_end_matches([',', '.', ':', ';'])) {
        if w.to_ascii_lowercase().ends_with(".png") && !out.iter().any(|v| v["name"] == w) {
            out.push(json!({"name": w, "path": dir.join(w)}));
        }
    }
    out
}
