use crate::config::Config;
use crate::events::{working, Json};
use crate::lesson::ymd;
use crate::runner::{argv, ok, Runner};
use crate::Ctx;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

const DAYS: u64 = 14;
const HOT: usize = 5;
const HOT_IN: usize = 20;
const SWEEP: usize = 40;
/// How a hotspots issue's title starts.
pub const TITLE: &str = "hotspots: ";

/// What the window held: merges counted, sweeps (over `SWEEP` files) skipped, the bar, and decided files over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub merges: usize,
    pub sweeps: usize,
    pub bar: usize,
    pub decided: usize,
}

/// A file the window's bar or more counted merges touched in `DAYS` days, the distinct PR pairs it was predicted to conflict in and its PRs' update rounds.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Hot {
    pub file: String,
    pub prs: usize,
    pub conflicts: usize,
    pub updates: usize,
}

/// `fleet`'s hot files from its integration branch's merges and every same-repo fleet's log, most-touched first.
pub fn find(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str) -> Result<(Window, Vec<Hot>)> {
    let f = cfg.fleet(fleet)?;
    let since = ctx.now.saturating_sub(DAYS * 86400);
    let (local, range) = (
        f.local_path().to_string_lossy().into_owned(),
        format!("origin/{}", f.integration),
    );
    let args = [
        "-C",
        &local,
        "log",
        &range,
        "--first-parent",
        "--diff-merges=first-parent",
        "--name-only",
    ];
    let since_at = format!("--since=@{since}");
    let log = ok(r, "git", &[&args[..], &[since_at.as_str(), "--format=%x00%H"]].concat())?;
    let mut counted = Vec::<(String, Vec<String>)>::new();
    for l in log.lines().filter(|l| !l.is_empty()) {
        match (l.strip_prefix('\0'), counted.last_mut()) {
            (Some(h), _) => counted.push((h.to_string(), vec![])),
            (None, Some(m)) => m.1.push(l.to_string()),
            (None, None) => {}
        }
    }
    let all = counted.len();
    counted.retain(|m| m.1.len() <= SWEEP);
    let bar = HOT.max(counted.len() / HOT_IN);
    let mut touched = BTreeMap::<String, Vec<String>>::new();
    for (sha, files) in &counted {
        for file in files {
            touched.entry(file.clone()).or_default().push(sha.clone());
        }
    }
    let decided = decided(r, f)?;
    let tip = ok(r, "git", &["-C", &local, "ls-tree", "-r", "--name-only", &range])?;
    let tip: BTreeSet<&str> = tip.lines().collect();
    touched.retain(|f, s| tip.contains(f.as_str()) && s.len() >= bar);
    let before = touched.len();
    touched.retain(|f, _| !crate::arch::any(&decided, f));
    let window = Window {
        merges: counted.len(),
        sweeps: all - counted.len(),
        bar,
        decided: before - touched.len(),
    };
    let (events, predicted) = (
        working(&ctx.state)?,
        crate::events::since(&ctx.state, &["conflict-predicted"], since)?,
    );
    let same = |e: &&crate::events::Event| cfg.fleet(&e.fleet).is_ok_and(|g| g.repo == f.repo);
    let updates = |sha: &String| {
        let merged = events
            .iter()
            .filter(same)
            .find(|e| e.kind == "merged" && e.str("merge_sha") == Some(sha));
        merged.map_or(0, |m| {
            let upd = |e: &&crate::events::Event| e.kind == "spawn" && e.data.get("update") == Some(&json!(true));
            events
                .iter()
                .filter(|e| e.fleet == m.fleet && e.issue == m.issue)
                .filter(upd)
                .count()
        })
    };
    let conflicts = |file: &str| {
        let on = |e: &&crate::events::Event| {
            e.data
                .get("files")
                .and_then(Value::as_array)
                .is_some_and(|a| a.contains(&json!(file)))
        };
        let pair = |e: &crate::events::Event| (e.fleet.clone(), e.issue, e.data.get("other").map(Value::to_string));
        let pairs: BTreeSet<_> = predicted.iter().filter(same).filter(on).map(pair).collect();
        pairs.len()
    };
    let mut hot: Vec<Hot> = (touched.into_iter())
        .map(|(file, shas)| Hot {
            prs: shas.len(),
            conflicts: conflicts(&file),
            updates: shas.iter().map(updates).sum(),
            file,
        })
        .collect();
    hot.sort_by(|a, b| b.prs.cmp(&a.prs).then_with(|| a.file.cmp(&b.file)));
    Ok((window, hot))
}

fn decided(r: &dyn Runner, f: &crate::config::Fleet) -> Result<Vec<String>> {
    let local = f.local_path().to_string_lossy().into_owned();
    let show = |p: &str| {
        let at = format!("origin/{}:{p}", f.integration);
        let o = r.run("git", &argv(&["-C", &local, "show", &at])).ok()?;
        (o.code == 0).then_some(o.stdout)
    };
    crate::repo_read("arch.toml", show).map_or(Ok(vec![]), |t| crate::arch::decided(&t))
}

/// Log `hot-files` when the stored hot files less those arch.toml now marks decided differ from the last one.
pub fn sync(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str) -> Result<()> {
    let Ok(text) = std::fs::read_to_string(path(ctx, fleet)) else {
        return Ok(());
    };
    let hot: Vec<Hot> = serde_json::from_str(&text)?;
    let last = crate::queue::hot_files(&working(&ctx.state)?, fleet);
    if hot.is_empty() && last.is_empty() {
        return Ok(());
    }
    let decided = decided(r, cfg.fleet(fleet)?)?;
    let files: Vec<String> = (hot.into_iter().map(|h| h.file))
        .filter(|p| !crate::arch::any(&decided, p))
        .collect();
    if files != last {
        let e = crate::events::Event::new(ctx.now, fleet, None, "hot-files", json!({"files": files}));
        crate::events::emit(&ctx.state, cfg, e)?;
    }
    Ok(())
}

/// The report `drover hotspots` prints and its issue's body.
pub fn report(fleet: &str, w: &Window, hot: &[Hot]) -> String {
    let Window {
        merges,
        sweeps,
        bar,
        decided,
    } = w;
    let mut s = format!(
        "Hot files in {fleet}: {bar} or more of {merges} merged PRs (1 in {HOT_IN}, at least {HOT}) in the last {DAYS} days\n{sweeps} sweeps (over {SWEEP} files) not counted · {decided} decided files (frozen, generated or shared) not listed\n\n"
    );
    if hot.is_empty() {
        return s + "none\n";
    }
    s += "| file | merged PRs | conflict-predicted | update rounds |\n|---|---|---|---|\n";
    for h in hot {
        s += &format!("| `{}` | {} | {} | {} |\n", h.file, h.prs, h.conflicts, h.updates);
    }
    s
}

/// Where each run keeps `fleet`'s current hot files.
pub fn path(ctx: &Ctx, fleet: &str) -> std::path::PathBuf {
    ctx.state.with_file_name(format!("hotspots-{fleet}.json"))
}

fn store(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str) -> Result<(Window, Vec<Hot>)> {
    let (w, hot) = find(ctx, cfg, r, fleet)?;
    std::fs::write(path(ctx, fleet), serde_json::to_string_pretty(&hot)?)?;
    Ok((w, hot))
}

/// Report and store the hot files; with any, open (or update) the week's issue and queue it `--architect`.
pub fn run(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<()> {
    let (w, hot) = store(ctx, cfg, r, fleet)?;
    let body = report(fleet, &w, &hot);
    write!(out, "{body}")?;
    if hot.is_empty() {
        return Ok(());
    }
    file(ctx, cfg, r, out, fleet, &body)
}

fn file(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str, body: &str) -> Result<()> {
    let f = cfg.fleet(fleet)?;
    let day = ctx.now / 86400;
    let title = format!("{TITLE}{fleet} week of {}", ymd(day - (day + 3) % 7));
    let search = format!("\"{title}\" in:title");
    let list = [
        "issue",
        "list",
        "-R",
        &f.repo,
        "--state",
        "open",
        "--search",
        &search,
        "--json",
        "number,title",
    ];
    let open: Value = serde_json::from_str(&ok(r, "gh", &list)?).unwrap_or_default();
    let found = (open.as_array().into_iter().flatten()).find(|i| i["title"] == title.as_str());
    let n = if let Some(n) = found.and_then(|i| i["number"].as_u64()) {
        ok(
            r,
            "gh",
            &["issue", "edit", &n.to_string(), "-R", &f.repo, "--body", body],
        )?;
        writeln!(out, "updated {fleet}#{n}: {title}")?;
        n
    } else {
        let url = ok(
            r,
            "gh",
            &["issue", "create", "-R", &f.repo, "--title", &title, "--body", body],
        )?;
        let n = (url.trim().rsplit('/').next().and_then(|n| n.parse().ok())).context("gh issue create: no number")?;
        writeln!(out, "filed {fleet}#{n}: {title}")?;
        n
    };
    if crate::queue::replay(&working(&ctx.state)?, fleet)
        .iter()
        .all(|i| i.issue != n)
    {
        let mut op = crate::queue::op("add", &[n], false, None, None, None);
        op.put("architect", json!(true));
        writeln!(out, "{}", crate::queue_change(ctx, cfg, fleet, op, None)?)?;
    }
    Ok(())
}

/// `tick`'s run: store each fleet's list once a day, file its issue once in 7 days (`hotspots = "off"` opts out; a paused fleet waits).
pub fn daily(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write) -> Result<()> {
    let on = |f: &crate::config::Fleet| f.hotspots.as_deref() != Some("off") && !f.paused;
    let due = |name: &str, ext: &str, every: u64| {
        let at = ctx.state.with_file_name(format!("hotspots-{name}.{ext}"));
        let last = std::fs::read_to_string(&at)
            .ok()
            .and_then(|t| t.trim().parse::<u64>().ok());
        (last.is_none_or(|t| ctx.now >= t + every)).then_some(at)
    };
    for name in cfg.fleet.iter().filter(|(_, f)| on(f)).map(|x| x.0) {
        let Some(day) = due(name, "day", 86400) else { continue };
        std::fs::write(&day, ctx.now.to_string())?;
        let filed = store(ctx, cfg, r, name).and_then(|(w, hot)| {
            let Some(at) = due(name, "at", 7 * 86400).filter(|_| !hot.is_empty()) else {
                return Ok(());
            };
            std::fs::write(&at, ctx.now.to_string())?;
            let body = report(name, &w, &hot);
            write!(out, "{body}")?;
            file(ctx, cfg, r, out, name, &body)
        });
        if let Err(e) = filed {
            writeln!(out, "{name} hotspots: {e:#}")?;
        }
    }
    Ok(())
}
