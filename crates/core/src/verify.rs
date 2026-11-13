//! #417: the optional verify stage: whether it runs for an issue's head, the skip logged when it doesn't, and the
//! screenshots a verdict lists.
use crate::config::Config;
use crate::events::{emit, read, Event, IssueState};
use crate::runner::{ok, Runner};
use crate::Ctx;
use anyhow::Result;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

/// The repo's recipe the verifier follows.
pub const RECIPE: &str = ".drover/verify.md";

/// Why `fleet` #`n`'s verify stage runs (`None`: it doesn't): rev's `Repro: verify` PASS, its queue mark, else a
/// design stage or a PR touching `ui_paths`.
pub fn why(events: &[Event], ui_paths: &[String], (fleet, n): (&str, u64), files: &[String]) -> Option<String> {
    let mark = crate::queue::replay(events, fleet)
        .into_iter()
        .find(|i| i.issue == n)
        .and_then(|i| i.verify);
    let ui = || {
        let hit = |p: &&String| ui_paths.iter().any(|g| crate::config::glob(g.as_bytes(), p.as_bytes()));
        files.iter().find(hit).map(|p| format!("touches {p}"))
    };
    // #420: a `--repro` fix with no cheap test proves itself here.
    let repro = |e: &Event| e.is("verdict", fleet, n) && e.str("repro") == Some("verify");
    match mark {
        _ if events.iter().any(repro) => Some("Repro: verify".into()),
        Some(on) => on.then(|| "queued --verify".into()),
        None if events
            .iter()
            .any(|e| e.is("design-approved", fleet, n) && e.str("role") != Some("architect")) =>
        {
            Some("had a design stage".into())
        }
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
            let api = |p: &str| format!("repos/{}/contents/{p}?ref={sha}", f.repo);
            if crate::repo_read("verify.md", |p| ok(r, "gh", &["api", &api(p), "--silent"]).ok()).is_some() {
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

/// The PNGs (#731: and `.log`, `.json`, `.txt` evidence) verify's verdict.md (at `file`) lists, as `{name, path}`:
/// #547: only a relative word with no `..` that is a regular file inside the verify lane's folder `lane` (none: none).
pub fn shots(body: &str, file: &Path, lane: Option<&str>) -> Vec<Value> {
    let dir = file.parent().unwrap_or(Path::new(""));
    let words = body.split(|c: char| c.is_whitespace() || "`()[]<>\"'".contains(c));
    let plain = |w: &str| {
        Path::new(w)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
    };
    let mut out: Vec<Value> = vec![];
    for w in words.map(|w| w.trim_end_matches([',', '.', ':', ';'])) {
        let ext = Path::new(w).extension().map(std::ffi::OsStr::to_ascii_lowercase);
        let png = ext.is_some_and(|e| ["png", "log", "json", "txt"].iter().any(|x| e == *x)) && plain(w);
        let path = lane.filter(|_| png).and_then(|l| inside(&dir.join(w), Path::new(l)));
        if let Some(p) = path.filter(|_| !out.iter().any(|v| v["name"] == w)) {
            out.push(json!({"name": w, "path": p}));
        }
    }
    out
}

/// `path`, canonical, if it is a regular file inside `dir`.
fn inside(path: &Path, dir: &Path) -> Option<PathBuf> {
    let (p, d) = (path.canonicalize().ok()?, dir.canonicalize().ok()?);
    (p.starts_with(d) && p.is_file()).then_some(p)
}

/// #547: the shots of `fleet` #`n`'s last verdict the board shows and serves: a verify verdict's, each still a file
/// inside one of the issue's lane folders (older events kept any `.png` word).
pub fn served(events: &[Event], (fleet, n): (&str, u64), at: Option<&str>) -> Vec<(String, PathBuf)> {
    let lanes: Vec<&str> = (events.iter().filter(|e| e.is("spawn", fleet, n)))
        .filter_map(|e| e.str("worktree"))
        .collect();
    let last = (events.iter()).rfind(|e| e.is("verdict", fleet, n) && at.is_none_or(|s| e.str("sha") == Some(s)));
    let files = last
        .filter(|e| e.str("stage") == Some("verify"))
        .and_then(|e| e.data["files"].as_array());
    let shot = |v: &Value| {
        let (name, path) = (v["name"].as_str()?, Path::new(v["path"].as_str()?));
        let p = lanes.iter().find_map(|l| inside(path, Path::new(l)))?;
        Some((name.to_string(), p))
    };
    files.into_iter().flatten().filter_map(shot).collect()
}
