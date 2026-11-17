use crate::config::Fleet;
use crate::events::{fold, Event};
use crate::hosts::On;
use crate::runner::Runner;
use anyhow::Result;
use std::io::Write;
use std::path::PathBuf;

const RESET: &str = "cd \"$1\" && git fetch -q --prune --force origin '+refs/heads/*:refs/remotes/origin/*' \
    '+refs/notes/*:refs/notes/*' && b=$(git symbolic-ref -q --short HEAD) && git reset -q --keep \"origin/$b\"";

/// The lane folders of `fleets`' open issues that exist, with their host (none: this one).
pub fn folders(r: &dyn Runner, events: &[Event], fleets: &[&String]) -> Vec<(PathBuf, Option<String>)> {
    let st = fold(events);
    let open = |e: &&Event| {
        e.issue
            .is_some_and(|n| st.get(&(e.fleet.clone(), n)).is_some_and(|s| !s.done()))
    };
    let mut out: Vec<(PathBuf, Option<String>)> = vec![];
    for e in (events
        .iter()
        .filter(|e| e.kind == "spawn" && fleets.contains(&&e.fleet)))
    .filter(open)
    {
        let (Some(wt), m) = (e.str("worktree").map(PathBuf::from), e.str("machine").map(String::from)) else {
            continue;
        };
        if !out.iter().any(|x| x.0 == wt) && On::new(r, m.as_deref()).exists(&wt.join(".git")) {
            out.push((wt, m));
        }
    }
    out
}

/// Reset the fleet's clone and every lane folder that answers onto origin's branches; files stay as they are.
pub fn all(r: &dyn Runner, f: &Fleet, events: &[Event], fleets: &[&String], out: &mut dyn Write) -> Result<()> {
    let dirs = [(f.local_path(), None)].into_iter().chain(folders(r, events, fleets));
    for (d, m) in dirs {
        match On::new(r, m.as_deref()).sh(RESET, &d, "") {
            Ok(o) if o.code == 0 => writeln!(out, "reset {}", d.display())?,
            Ok(o) => writeln!(out, "could not reset {}: {}", d.display(), o.stderr.trim())?,
            Err(e) => writeln!(out, "could not reset {}: {e:#}", d.display())?,
        }
    }
    Ok(())
}
