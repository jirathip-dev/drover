use crate::config::{Config, Fleet};
use crate::events::{fold, history, Event, Json, Log};
use crate::gate::{refuse, refused, Refusal};
use crate::runner::{ok, Runner};
use crate::Ctx;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// `drover compact`'s arguments: the fleet, then the act as the standalone `compact` binary takes it.
#[derive(clap::Args, Debug)]
pub struct Opts {
    pub fleet: String,
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        help = "compact's act: --cut <date|sha> [--dry-run | --thread <id>], --undo [--dry-run | --thread <id>], --reset"
    )]
    pub act: Vec<String>,
}

/// The repo's scratch folder `compact --dir` gets: the bare copy, the plan, the map and the records.
pub fn dir(ctx: &Ctx, repo: &str) -> PathBuf {
    ctx.state.with_file_name("compact").join(repo)
}

const CODES: &[&str] = &[
    "BAD_VALUE",
    "COMPACT_ARCHIVE",
    "COMPACT_MOVED",
    "COMPACT_NOT_READY",
    "COMPACT_TREE",
    "COMPACT_UNAPPROVED",
];

fn fleets<'a>(cfg: &'a Config, repo: &str) -> Vec<&'a String> {
    cfg.fleet.iter().filter(|(_, f)| f.repo == repo).map(|x| x.0).collect()
}

fn lanes(r: &dyn Runner, events: &[Event], fleets: &[&String]) -> Vec<String> {
    let st = fold(events);
    let open = |e: &&Event| {
        e.issue
            .is_some_and(|n| st.get(&(e.fleet.clone(), n)).is_some_and(|s| !s.done()))
    };
    let mut out: Vec<String> = vec![];
    for e in (events
        .iter()
        .filter(|e| e.kind == "spawn" && fleets.contains(&&e.fleet)))
    .filter(open)
    {
        let (Some(wt), m) = (e.str("worktree"), e.str("machine")) else {
            continue;
        };
        let lane = m.map_or(wt.to_string(), |m| format!("{m}:{wt}"));
        if !out.contains(&lane) && crate::hosts::On::new(r, m).exists(&Path::new(wt).join(".git")) {
            out.push(lane);
        }
    }
    out
}

fn saved(events: &[Event], fleets: &[&String]) -> String {
    let mut rows: Vec<String> = vec![];
    for e in events.iter().filter(|e| fleets.contains(&&e.fleet)) {
        let Some(i) = e.issue else { continue };
        for (_, k) in SAVED.iter().filter(|s| s.0 == e.kind) {
            rows.extend(e.str(k).map(|s| format!("{}\t{i}\t{s}\n", e.fleet)));
        }
    }
    rows.concat()
}

/// `drover compact <fleet> <act>`: the `compact` binary on the fleet's repo, clone, lane folders and saved ids; its records logged.
pub fn cli(ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write, o: Opts) -> Result<()> {
    let cfg = Config::load(&ctx.config)?;
    let f = cfg.fleet(&o.fleet)?;
    let d = dir(ctx, &f.repo);
    std::fs::create_dir_all(&d)?;
    record(ctx, &cfg, &f.repo)?;
    let names = fleets(&cfg, &f.repo);
    let events = history(&ctx.state)?;
    let list = d.join("saved.tsv");
    std::fs::write(&list, saved(&events, &names))?;
    let mut args: Vec<String> = vec!["--repo".into(), f.repo.clone(), "--line".into(), f.integration.clone()];
    args.extend(["--clone".into(), f.local_path().to_string_lossy().into()]);
    for l in lanes(r, &events, &names) {
        args.extend(["--lane".into(), l]);
    }
    for n in &names {
        args.extend(["--fleet".into(), (*n).clone()]);
    }
    args.extend(["--saved".into(), list.to_string_lossy().into()]);
    args.extend(["--dir".into(), d.to_string_lossy().into()]);
    args.extend(o.act);
    let res = (r.run("compact", &args))
        .context("no `compact` binary: cargo install --locked --git https://github.com/jirathip-dev/compact")?;
    write!(out, "{}", res.stdout)?;
    record(ctx, &cfg, &f.repo)?;
    let err = res.stderr.trim();
    let why = err.strip_prefix("REFUSED ").and_then(|e| e.split_once(": "));
    match why.and_then(|(c, why)| Some((*CODES.iter().find(|k| **k == c)?, why))) {
        _ if res.code == 0 => Ok(()),
        Some((code, why)) if res.code == 3 => refused(code, why),
        _ => anyhow::bail!("compact failed ({}): {err}", res.code),
    }
}

/// Log the `compact` binary's finished runs of `repo` once: its `moved.json` as `compacted`, its `undone` as `compact-undone`.
pub fn record(ctx: &Ctx, cfg: &Config, repo: &str) -> Result<()> {
    let d = dir(ctx, repo);
    let (moved, undone) = (d.join("moved.json"), d.join("undone"));
    if !moved.exists() && !undone.exists() {
        return Ok(());
    }
    let names = fleets(cfg, repo);
    let events = history(&ctx.state)?;
    let logged = |kind: &str, run: &str| {
        (events.iter()).any(|e| e.kind == kind && e.issue.is_none() && e.str("run") == Some(run))
    };
    let mut evs = vec![];
    if moved.exists() {
        let p: Value = serde_json::from_str(&std::fs::read_to_string(d.join("plan.json"))?)?;
        let run = p.at("new_tip").as_str().unwrap_or_default();
        if !logged("compacted", run) {
            let refs = p.at("refs").as_array().map_or(0, Vec::len);
            let head = json!({"run": run, "repo": p.at("repo"), "line": p.at("line"), "cut": p.at("cut"),
                "root": p.at("root"), "old_tip": p.at("old_tip"), "archive": p.at("archive"),
                "commits": p.at("commits"), "refs": refs});
            evs.extend((names.iter()).map(|n| Event::new(ctx.now, n, None, "compacted", head.clone()).passive()));
            let rows: Vec<Value> = serde_json::from_str(&std::fs::read_to_string(&moved)?)?;
            for x in &rows {
                let (Some(fleet), Some(i)) = (x.at("fleet").as_str(), x.at("issue").as_u64()) else {
                    continue;
                };
                let v = json!({"run": run, "moved": x.at("moved")});
                evs.push(Event::new(ctx.now, fleet, Some(i), "compacted", v).passive());
            }
        }
    }
    if undone.exists() {
        let run = std::fs::read_to_string(&undone)?.trim().to_string();
        if !logged("compact-undone", &run) {
            let v = json!({"run": run});
            evs.extend((names.iter()).map(|n| Event::new(ctx.now, n, None, "compact-undone", v.clone()).passive()));
        }
    }
    crate::db::append_all(&ctx.state, &evs)?;
    for f in [moved, undone].iter().filter(|f| f.exists()) {
        std::fs::rename(f, f.with_extension("logged"))?;
    }
    Ok(())
}

/// Every saved commit sha compact remaps, as (record kind, field); `--saved` lists only these.
pub const SAVED: &[(&str, &str)] = &[
    ("merged", "merge_sha"),
    ("merged", "verdict_sha"),
    ("verdict", "sha"),
    ("why", "sha"),
];

/// The sha-named fields compact leaves as they are, each with why.
pub const KEPT: &[(&str, &str)] = &[
    ("spawn_sha", "a verify verdict's lane start, compared before merge only"),
    (
        "red_sha",
        "a land's red integration tip, read only while that land runs",
    ),
];

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
    if let Some(e) = (cfg.fleet.get(fleet)).and_then(|f| record(ctx, cfg, &f.repo).err()) {
        writeln!(out, "compact records: {e:#}").ok();
    }
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
