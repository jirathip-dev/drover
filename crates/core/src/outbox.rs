use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::PathBuf;

pub const QUEUED: &str = "queued for the main host: drive delivers it within 1 min\n";

/// A lane on another host (spawn exports `DROVER_OUTBOX`): its `DROVER_LANE`.
pub fn lane() -> Option<String> {
    crate::config::env_os("DROVER_OUTBOX")?;
    crate::config::env("DROVER_LANE").filter(|l| !l.is_empty())
}

/// The lane's side: `args` as one line (`args`, `lane`, `ts` in ms) in the worktree holding the cwd.
pub fn queue(lane: &str, args: &[String]) -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    let wt = cwd.ancestors().find(|d| d.join(".git").exists());
    let p = wt.context("no worktree holds the cwd")?.join(".drover/outbox.jsonl");
    std::fs::create_dir_all(p.parent().context("no dir")?)?;
    let mut f = std::fs::File::options().create(true).read(true).append(true).open(&p)?;
    f.lock()?;
    let mut text = String::new();
    f.read_to_string(&mut text)?;
    let last = text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok()?.get("ts")?.as_u64())
        .max();
    let ts = (crate::clock::nanos() / 1_000_000).max(last.map_or(0, |t| u128::from(t) + 1));
    writeln!(f, "{}", json!({"args": args, "lane": lane, "ts": ts}))?;
    Ok(p)
}
