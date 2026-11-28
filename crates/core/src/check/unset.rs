use crate::config::Config;
use crate::events::Event;
use anyhow::Result;
use std::collections::BTreeMap;
use std::io::Write;

pub(super) fn unconfigured<'e>(
    cfg: &Config,
    out: &mut dyn Write,
    events: &[Event],
    lane: Option<&str>,
    spawns: impl Iterator<Item = &'e Event>,
) -> Result<Vec<&'e Event>> {
    let mut unset: BTreeMap<&str, BTreeMap<&str, u64>> = BTreeMap::new();
    let mut kept = vec![];
    for e in spawns {
        let gone = e
            .str("machine")
            .filter(|m| lane.is_none() && !crate::hosts::configured(cfg, m));
        match (gone, e.str("worktree")) {
            (Some(m), Some(wt)) => {
                unset
                    .entry(e.str("host").unwrap_or(m))
                    .or_default()
                    .entry(wt)
                    .or_insert(e.ts);
            }
            _ => kept.push(e),
        }
    }
    for c in events.iter().filter(|c| c.kind == "clean") {
        let wt = c.str("worktree").unwrap_or_default();
        for w in unset.values_mut().filter(|w| w.get(wt).is_some_and(|ts| c.ts >= *ts)) {
            w.remove(wt);
        }
    }
    for (host, w) in unset.iter().filter(|w| !w.1.is_empty()) {
        writeln!(out, "{host}: {} worktrees, host not configured", w.len())?;
    }
    Ok(kept)
}
