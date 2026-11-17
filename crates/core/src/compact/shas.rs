use crate::config::{Config, Fleet};
use crate::events::{Event, Log};
use crate::gate::{refuse, Refusal};
use crate::runner::{ok, Runner};
use crate::Ctx;
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

/// A compact run as its `compacted` record names it.
#[derive(Debug, Clone)]
pub struct Run {
    pub run: String,
    pub repo: String,
    pub line: String,
    pub cut: String,
    pub root: String,
    pub archive: String,
    pub undone: bool,
}

/// The bridge between the two id spaces: stored ids to the commits that hold them today.
#[derive(Debug, Default)]
pub struct Shas {
    runs: Vec<Run>,
    now: HashMap<String, String>,
}

impl Shas {
    /// The `compacted` and `compact-undone` records of `events`, folded.
    pub fn of(events: &[Event]) -> Shas {
        let undone = |run: &str| {
            events
                .iter()
                .any(|e| e.kind == "compact-undone" && e.str("run") == Some(run))
        };
        let mut s = Shas::default();
        for e in events.iter().filter(|e| e.kind == "compacted") {
            let run = e.str("run").unwrap_or_default();
            let back = undone(run);
            if e.issue.is_none() {
                let k = |k: &str| e.str(k).unwrap_or_default().to_string();
                s.runs.push(Run {
                    run: run.to_string(),
                    repo: k("repo"),
                    line: k("line"),
                    cut: k("cut"),
                    root: k("root"),
                    archive: k("archive"),
                    undone: back,
                });
                continue;
            }
            for m in e.val("moved").as_array().into_iter().flatten() {
                let (Some(from), Some(to)) = (m["from"].as_str(), m["to"].as_str()) else {
                    continue;
                };
                if back {
                    s.now.insert(to.to_string(), from.to_string());
                    continue;
                }
                let held = m["held"].as_array().into_iter().flatten().filter_map(Value::as_str);
                for h in held.chain([from]) {
                    s.now.insert(h.to_string(), to.to_string());
                }
            }
        }
        s
    }
    /// The commit that holds `stored` today: itself unless a compact in force moved it, or an undone one made it.
    pub fn now<'a>(&'a self, stored: &'a str) -> &'a str {
        self.now.get(stored).map_or(stored, String::as_str)
    }
    /// The run whose root `sha` is, while it is in force: the line is older than the cut.
    pub fn root(&self, sha: &str) -> Option<&Run> {
        self.runs.iter().find(|r| !r.undone && r.root == sha)
    }
    /// The newest run of `repo` still in force.
    pub fn last(&self, repo: &str) -> Option<&Run> {
        self.runs.iter().rfind(|r| r.repo == repo && !r.undone)
    }
}

/// `events`, once the fleet's clone stands on its newest compact root (its `origin/<line>`).
pub fn on_root(r: &dyn Runner, events: Arc<Log>, (fleet, f): (&str, &Fleet)) -> Result<Arc<Log>, Refusal> {
    let line = format!("origin/{}", f.integration);
    rooted(r, &events, (fleet, f), &f.local_path(), &line).map(|()| events)
}

/// Drive takes fleet `fleet`: not paused, and its clone on the newest compact root (else it says why in `out`).
pub fn drives(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str, out: &mut dyn Write) -> bool {
    let Some(f) = cfg.fleet.get(fleet).filter(|f| !f.paused) else {
        return false;
    };
    let Ok(events) = crate::events::working(&ctx.state) else {
        return true;
    };
    on_root(r, events, (fleet, f)).map_err(|e| writeln!(out, "{e}")).is_ok()
}

/// PR head `head` stands on the root (fetched first): merge's door.
pub fn head(r: &dyn Runner, events: &[Event], (fleet, f): (&str, &Fleet), head: String) -> Result<String, Refusal> {
    rooted(r, events, (fleet, f), &f.local_path(), &head).map(|()| head)
}

/// `rev` in `dir` (a sha is fetched first) stands on the newest compact root of `f`'s repo; else `OLD_ROOT`, naming `--reset`.
pub fn rooted(
    r: &dyn Runner,
    events: &[Event],
    (fleet, f): (&str, &Fleet),
    dir: &Path,
    rev: &str,
) -> Result<(), Refusal> {
    let shas = Shas::of(events);
    let Some(run) = shas.last(&f.repo) else {
        return Ok(());
    };
    let d = dir.to_string_lossy();
    if rev.len() == 40 {
        ok(r, "git", &["-C", &d, "fetch", "-q", "origin", rev]).ok();
    }
    if ok(r, "git", &["-C", &d, "merge-base", "--is-ancestor", &run.root, rev]).is_ok() {
        return Ok(());
    }
    Err(refuse(
        "OLD_ROOT",
        format!(
            "{d} ({rev:.12}) is not on {}'s compacted history → drover compact {fleet} --reset",
            f.repo
        ),
    ))
}
