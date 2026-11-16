use crate::config::Config;
use crate::events::{append, fold, open_asks, working, Event};
use crate::gate::refused;
use crate::runner::{argv, json, ok, Runner};
use crate::Ctx;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::Write;

/// #1100: the head of a folded issue's `<details>` block, which holds its text before the fold.
pub const FOLDED: &str = "<details><summary>Earlier text";

/// #1100: `drover triage <fleet>`: file this week's triage issue, queue it on top and spawn its `triage` lane; false when the lane's prompt was not confirmed (exit 4).
pub fn run(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<bool> {
    let title = format!("triage: {fleet} week of {}", crate::lesson::ymd(ctx.now / 86400));
    let body = include_str!("../roles/triage-issue.md").replace("{fleet}", fleet);
    crate::lesson::file(ctx, cfg, r, out, (fleet, "triage", None), (&title, &body))
}

/// #1100: `tick`'s triage run, once per 7 days for each fleet with `triage = "weekly"` (a paused fleet waits).
pub fn weekly(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write) -> Result<()> {
    let on = |f: &crate::config::Fleet| f.triage.as_deref() == Some("weekly") && !f.paused;
    for name in cfg.fleet.iter().filter(|(_, f)| on(f)).map(|x| x.0) {
        if !crate::timer::weekly_due(ctx, &format!("triage-{name}"))? {
            continue;
        }
        if let Err(e) = run(ctx, cfg, r, out, name) {
            writeln!(out, "{name} triage: {e:#}")?;
        }
    }
    Ok(())
}

/// #1100: `body` without its folded block: the text a later amendment or fold reads.
pub fn live(body: &str) -> String {
    match body.split_once(FOLDED) {
        Some((head, rest)) => head.to_string() + rest.rsplit_once("</details>").map_or("", |x| x.1),
        None => body.into(),
    }
}

fn unprotected(ctx: &Ctx, cfg: &Config, r: &dyn Runner, (fleet, n): (&str, u64)) -> Result<Value> {
    let events = working(&ctx.state)?;
    let st = fold(&events);
    let queued = crate::queue::view(&events, &st, fleet).iter().any(|i| i.issue == n);
    let laned = st.get(&(fleet.to_string(), n)).is_some_and(|s| !s.done());
    let asked = open_asks(&events)
        .iter()
        .any(|a| a.fleet == fleet && a.issue == Some(n));
    let repo = &cfg.fleet(fleet)?.repo;
    let issue = json(
        r,
        "gh",
        &["issue", "view", &n.to_string(), "-R", repo, "--json", "body,labels"],
    )?;
    let keep = (issue["labels"].as_array().into_iter().flatten()).any(|l| l["name"] == "keep");
    let why = [
        (queued, "is queued"),
        (laned, "has a lane"),
        (asked, "waits on an open ask"),
        (keep, "is labeled keep"),
    ];
    match why.iter().find(|w| w.0) {
        Some((_, w)) => refused("PROTECTED", format!("{fleet}#{n} {w}: triage never touches it")),
        None => Ok(issue),
    }
}

fn log(ctx: &Ctx, fleet: &str, kind: &str, mut d: Value) -> Result<()> {
    let lane = std::env::var("DROVER_LANE").unwrap_or_default();
    let mut it = lane.split(':');
    let own = (it.next() == Some(fleet)).then(|| it.next()?.parse().ok()).flatten();
    d["lane"] = json!(lane);
    append(&ctx.state, &Event::new(ctx.now, fleet, own, kind, d))
}

fn link(r: &dyn Runner, f: &crate::config::Fleet, why: &str, evidence: &str) -> Result<String> {
    let e = evidence.trim();
    let base = format!("https://github.com/{}", f.repo);
    if let Some(n) = e.strip_prefix('#').filter(|n| n.parse::<u64>().is_ok()) {
        return Ok(format!("{base}/issues/{n}"));
    }
    if e.starts_with("https://") && !e.contains(char::is_whitespace) {
        return Ok(e.into());
    }
    let sha = (7..=40).contains(&e.len()) && e.chars().all(|c| c.is_ascii_hexdigit());
    if !sha || why != "done" {
        return refused("BAD_VALUE", "--evidence is `#N`, a URL, or (for `done`) a commit sha");
    }
    let local = f.local_path().to_string_lossy().into_owned();
    let on = format!("origin/{}", f.integration);
    let a = argv(&["-C", &local, "merge-base", "--is-ancestor", e, &on]);
    if r.run("git", &a).is_ok_and(|o| o.code == 0) {
        Ok(format!("{base}/commit/{e}"))
    } else {
        refused(
            "NOT_ON_INTEGRATION",
            format!("{e} is not on {on}: the issue is not done there"),
        )
    }
}

/// #1100: `drover triage close`: close an unprotected issue that is done, a duplicate or superseded, with one comment linking the evidence and its undo.
pub fn close(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, n): (&str, u64),
    (why, evidence): (&str, &str),
) -> Result<()> {
    let f = cfg.fleet(fleet)?;
    let url = link(r, f, why, evidence)?;
    unprotected(ctx, cfg, r, (fleet, n))?;
    let undo = format!("gh issue reopen {n} -R {}", f.repo);
    let text = format!("triage: {why}: {url}\n\nUndo: `{undo}`");
    let reason = if why == "done" { "completed" } else { "not planned" };
    let a = [
        "issue",
        "close",
        &n.to_string(),
        "-R",
        &f.repo,
        "--reason",
        reason,
        "--comment",
        &text,
    ];
    ok(r, "gh", &a)?;
    log(
        ctx,
        fleet,
        "triage-closed",
        json!({"target": n, "why": why, "evidence": url, "undo": undo}),
    )?;
    Ok(writeln!(out, "closed {fleet}#{n}: {why}: {url}")?)
}

/// #1100: `drover triage fold`: an unprotected issue's body becomes `## Spec (current)` (the `spec` file) over its old text in a `<details>` block; `NOTHING_TO_FOLD` without amendments.
pub fn fold_spec(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, n): (&str, u64),
    spec: &std::path::Path,
) -> Result<()> {
    let spec = std::fs::read_to_string(spec)?;
    if spec.trim().is_empty() {
        return refused("BAD_VALUE", "--spec is an empty file");
    }
    let issue = unprotected(ctx, cfg, r, (fleet, n))?;
    let old = issue["body"].as_str().unwrap_or_default().trim_end();
    let cur = live(old);
    let amendments = cur.lines().filter(|l| l.starts_with("- **Amendment")).count();
    if !cur.lines().any(|l| l.trim() == "## Amendments") {
        return refused("NOTHING_TO_FOLD", format!("{fleet}#{n} has no `## Amendments` to fold"));
    }
    let day = crate::lesson::ymd(ctx.now / 86400);
    let body = format!(
        "## Spec (current)\n\n{}\n\n{FOLDED}, folded by triage {day}</summary>\n\n{old}\n\n</details>\n",
        spec.trim()
    );
    let repo = &cfg.fleet(fleet)?.repo;
    ok(r, "gh", &["issue", "edit", &n.to_string(), "-R", repo, "--body", &body])?;
    let undo = "its earlier text is in the folded block";
    log(
        ctx,
        fleet,
        "triage-folded",
        json!({"target": n, "amendments": amendments, "undo": undo}),
    )?;
    Ok(writeln!(out, "folded {fleet}#{n}: {amendments} amendments")?)
}

/// #1100: `drover triage label`: add `labels` (`stale?`, the fleet's own) to an unprotected issue.
pub fn label(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, n): (&str, u64),
    labels: &[String],
) -> Result<()> {
    unprotected(ctx, cfg, r, (fleet, n))?;
    let (repo, l) = (&cfg.fleet(fleet)?.repo, labels.join(","));
    ok(
        r,
        "gh",
        &["issue", "edit", &n.to_string(), "-R", repo, "--add-label", &l],
    )?;
    let undo = format!("gh issue edit {n} -R {repo} --remove-label {l}");
    log(
        ctx,
        fleet,
        "triage-labeled",
        json!({"target": n, "labels": labels, "undo": undo}),
    )?;
    Ok(writeln!(out, "labeled {fleet}#{n}: {l}")?)
}

/// #1100: a triage lane's `done --no-change <note>`: the summary of what it closed, folded and flagged (each with its undo) goes to the fleet's orchestrator thread, and closes its issue.
pub fn done(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, n): (&str, u64),
    note: &str,
) -> Result<()> {
    let events = working(&ctx.state)?;
    let mine: Vec<&Event> = (events.iter())
        .filter(|e| e.fleet == fleet && e.issue == Some(n) && e.kind.starts_with("triage-"))
        .collect();
    let count = |k: &str| mine.iter().filter(|e| e.kind == k).count();
    let mut s = format!(
        "triage {fleet}#{n}: {} closed, {} folded, {} flagged\n",
        count("triage-closed"),
        count("triage-folded"),
        count("triage-labeled")
    );
    for e in &mine {
        let what = match e.kind.as_str() {
            "triage-closed" => format!(
                "closed ({}: {})",
                e.str("why").unwrap_or_default(),
                e.str("evidence").unwrap_or_default()
            ),
            "triage-folded" => format!("folded ({} amendments)", e.val("amendments")),
            "triage-docs" => format!("docs issue ({} findings)", e.val("findings")),
            _ => format!("labeled {}", e.val("labels")),
        };
        let undo = e.str("undo").unwrap_or_default();
        s += &format!("- #{} {what} · undo: `{undo}`\n", e.val("target"));
    }
    s += &format!("note: {note}");
    match crate::orchestrator::route(cfg, &events, fleet) {
        Some(id) => crate::orchestrator::reply(ctx, cfg, id, &s, &[])?,
        None => writeln!(out, "{fleet}: no orchestrator thread; the summary is on #{n}")?,
    }
    crate::lesson::close(ctx, cfg, r, out, (fleet, n), &s)
}

/// #1100: `drover triage docs`: file the run's one `docs:` issue from the `findings` file (one per line); no finding, no issue; a README with no non-goals goes first.
pub fn docs(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    fleet: &str,
    (findings, root): (&std::path::Path, &std::path::Path),
) -> Result<()> {
    let text = std::fs::read_to_string(findings)?;
    let mut items: Vec<String> = (text.lines().map(str::trim))
        .filter(|l| !l.is_empty())
        .map(|l| format!("- {}", l.trim_start_matches("- ")))
        .collect();
    if items.is_empty() {
        return Ok(writeln!(out, "{fleet}: no docs finding, no docs issue")?);
    }
    let lane = std::env::var("DROVER_LANE").unwrap_or_default();
    let filed = working(&ctx.state)?
        .iter()
        .any(|e| e.kind == "triage-docs" && e.str("lane") == Some(&lane));
    if filed {
        return refused("ALREADY_FILED", format!("{lane} filed its one docs issue already"));
    }
    let readme = std::fs::read_to_string(root.join("README.md")).unwrap_or_default();
    let intent = |l: &str| l.starts_with('#') && l.to_lowercase().contains("non-goals");
    if !readme.lines().any(intent) {
        items.insert(
            0,
            "- README.md:1: no intent section (what it is, principles, non-goals): the owner writes it".into(),
        );
    }
    let f = cfg.fleet(fleet)?;
    let title = format!(
        "docs: {fleet} stale agent-read text, week of {}",
        crate::lesson::ymd(ctx.now / 86400)
    );
    let body = format!(
        "Triage ({lane}) found these passages stale, contradicting or bloated. Fix each in its file; never edit the \
         README's top (intent) section.\n\n{}\n",
        items.join("\n")
    );
    let url = ok(
        r,
        "gh",
        &["issue", "create", "-R", &f.repo, "--title", &title, "--body", &body],
    )?;
    let n: u64 = (url.trim().rsplit('/').next().and_then(|n| n.parse().ok())).context("gh issue create: no number")?;
    let undo = format!("gh issue close {n} -R {}", f.repo);
    log(
        ctx,
        fleet,
        "triage-docs",
        json!({"target": n, "findings": items.len(), "undo": undo}),
    )?;
    Ok(writeln!(out, "filed {fleet}#{n}: {title} ({} findings)", items.len())?)
}
