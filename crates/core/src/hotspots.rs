use crate::config::Config;
use crate::events::read;
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

/// #889: a file `HOT` or more merged PRs touched in `DAYS` days, the distinct PR pairs it was predicted to conflict in (#973) and its PRs' update rounds.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Hot {
    pub file: String,
    pub prs: usize,
    pub conflicts: usize,
    pub updates: usize,
}

/// #889: `fleet`'s hot files from its integration branch's merges and every same-repo fleet's log, most-touched first.
pub fn find(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str) -> Result<Vec<Hot>> {
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
    let mut touched = BTreeMap::<String, Vec<String>>::new();
    let mut sha = "";
    for l in log.lines().filter(|l| !l.is_empty()) {
        match l.strip_prefix('\0') {
            Some(h) => sha = h,
            None => touched.entry(l.to_string()).or_default().push(sha.to_string()),
        }
    }
    let show = |p: &str| {
        let o = r
            .run("git", &argv(&["-C", &local, "show", &format!("{range}:{p}")]))
            .ok()?;
        (o.code == 0).then_some(o.stdout)
    };
    let converted = match crate::repo_read("arch.toml", show) {
        Some(text) => crate::arch::converted(&text)?,
        None => vec![],
    };
    let tip = ok(r, "git", &["-C", &local, "ls-tree", "-r", "--name-only", &range])?;
    let tip: BTreeSet<&str> = tip.lines().collect();
    touched.retain(|f, _| tip.contains(f.as_str()) && !crate::arch::any(&converted, f));
    let events = read(&ctx.state)?;
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
        let recent = |e: &&crate::events::Event| e.kind == "conflict-predicted" && e.ts >= since;
        let pair = |e: &crate::events::Event| (e.fleet.clone(), e.issue, e.data.get("other").map(Value::to_string));
        let pairs: BTreeSet<_> = events.iter().filter(same).filter(recent).filter(on).map(pair).collect();
        pairs.len()
    };
    let mut hot: Vec<Hot> = (touched.into_iter().filter(|(_, s)| s.len() >= HOT))
        .map(|(file, shas)| Hot {
            prs: shas.len(),
            conflicts: conflicts(&file),
            updates: shas.iter().map(updates).sum(),
            file,
        })
        .collect();
    hot.sort_by(|a, b| b.prs.cmp(&a.prs).then_with(|| a.file.cmp(&b.file)));
    Ok(hot)
}

/// #889: the report `drover hotspots` prints and its issue's body.
pub fn report(fleet: &str, hot: &[Hot]) -> String {
    let mut s = format!("Hot files in {fleet}: {HOT} or more merged PRs in the last {DAYS} days\n\n");
    if hot.is_empty() {
        return s + "none\n";
    }
    s += "| file | merged PRs | conflict-predicted | update rounds |\n|---|---|---|---|\n";
    for h in hot {
        s += &format!("| `{}` | {} | {} | {} |\n", h.file, h.prs, h.conflicts, h.updates);
    }
    s
}

/// #889: where each run keeps `fleet`'s current hot files (#891 reads them).
pub fn path(ctx: &Ctx, fleet: &str) -> std::path::PathBuf {
    ctx.state.with_file_name(format!("hotspots-{fleet}.json"))
}

/// #889: report and store the hot files; with any, open (or update) the week's issue and queue it `--architect`.
pub fn run(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<()> {
    let f = cfg.fleet(fleet)?;
    let hot = find(ctx, cfg, r, fleet)?;
    std::fs::write(path(ctx, fleet), serde_json::to_string_pretty(&hot)?)?;
    let body = report(fleet, &hot);
    write!(out, "{body}")?;
    if hot.is_empty() {
        return Ok(());
    }
    let day = ctx.now / 86400;
    let title = format!("hotspots: {fleet} week of {}", ymd(day - (day + 3) % 7));
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
            &["issue", "edit", &n.to_string(), "-R", &f.repo, "--body", &body],
        )?;
        writeln!(out, "updated {fleet}#{n}: {title}")?;
        n
    } else {
        let url = ok(
            r,
            "gh",
            &["issue", "create", "-R", &f.repo, "--title", &title, "--body", &body],
        )?;
        let n = (url.trim().rsplit('/').next().and_then(|n| n.parse().ok())).context("gh issue create: no number")?;
        writeln!(out, "filed {fleet}#{n}: {title}")?;
        n
    };
    if crate::queue::replay(&read(&ctx.state)?, fleet)
        .iter()
        .all(|i| i.issue != n)
    {
        let mut op = crate::queue::op("add", &[n], false, None, None, None);
        op["architect"] = json!(true);
        writeln!(out, "{}", crate::queue_change(ctx, cfg, fleet, op, None)?)?;
    }
    Ok(())
}

/// #889: `tick`'s run, at most once per 7 days per fleet (`hotspots = "off"` opts out; a paused fleet waits).
pub fn weekly(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write) -> Result<()> {
    let on = |f: &crate::config::Fleet| f.hotspots.as_deref() != Some("off") && !f.paused;
    for name in cfg.fleet.iter().filter(|(_, f)| on(f)).map(|x| x.0) {
        let at = ctx.state.with_file_name(format!("hotspots-{name}.at"));
        let last = std::fs::read_to_string(&at)
            .ok()
            .and_then(|t| t.trim().parse::<u64>().ok());
        if last.is_some_and(|t| ctx.now < t + 7 * 86400) {
            continue;
        }
        std::fs::write(&at, ctx.now.to_string())?;
        if let Err(e) = run(ctx, cfg, r, out, name) {
            writeln!(out, "{name} hotspots: {e:#}")?;
        }
    }
    Ok(())
}
