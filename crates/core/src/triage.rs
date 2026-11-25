use crate::config::Config;
use crate::events::{append, fold, open_asks, working, Event, Json};
use crate::gate::refused;
use crate::runner::{argv, json, ok, Runner};
use crate::Ctx;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::Write;

/// The head of a folded issue's `<details>` block, which holds its text before the fold.
pub const FOLDED: &str = "<details><summary>Earlier text";

/// `drover triage close|fold|label|docs …`, a triage lane's verbs.
#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    cmd: Verb,
}

#[derive(clap::Subcommand)]
enum Verb {
    #[command(
        about = "Close an issue that is done, a duplicate or superseded, with one comment linking the evidence (a triage lane; never a queued, laned, asked or `keep` one)"
    )]
    Close {
        fleet: String,
        issue: u64,
        #[arg(long, value_parser = ["done", "duplicate", "superseded"])]
        why: String,
        #[arg(help = "`#N` (the PR or the issue that replaced it), a URL, or a commit sha on the integration branch")]
        #[arg(long)]
        evidence: String,
    },
    #[command(
        about = "Fold an issue's amendments: its body becomes `## Spec (current)` over the old text in a <details> block"
    )]
    Fold {
        fleet: String,
        issue: u64,
        #[arg(help = "A file with the current spec")]
        #[arg(long)]
        spec: std::path::PathBuf,
    },
    #[command(about = "Add labels to an issue: `stale?` for one triage can't decide, or the fleet's own")]
    Label {
        fleet: String,
        issue: u64,
        #[arg(required = true)]
        labels: Vec<String>,
    },
    #[command(
        about = "File the run's one `docs:` issue: each stale, contradicting or bloated agent-read passage, one per line; none, no issue"
    )]
    Docs {
        fleet: String,
        #[arg(
            help = "A file of findings, one per line: `file:line: what is wrong: the code or decision that proves it`"
        )]
        #[arg(long)]
        findings: std::path::PathBuf,
        #[arg(help = "The checkout whose README.md intent section is checked")]
        #[arg(long, default_value = ".")]
        root: std::path::PathBuf,
    },
}

/// Run a triage lane's verb.
pub fn cli(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, a: Args) -> Result<i32> {
    match a.cmd {
        Verb::Close {
            fleet,
            issue,
            why,
            evidence,
        } => close(ctx, cfg, r, out, (&fleet, issue), (&why, &evidence))?,
        Verb::Fold { fleet, issue, spec } => fold_spec(ctx, cfg, r, out, (&fleet, issue), &spec)?,
        Verb::Label { fleet, issue, labels } => label(ctx, cfg, r, out, (&fleet, issue), &labels)?,
        Verb::Docs { fleet, findings, root } => docs(ctx, cfg, r, out, &fleet, (&findings, &root))?,
    }
    Ok(0)
}

/// `body` without its folded block: the text a later amendment or fold reads.
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
    let keep = (issue.at("labels").as_array().into_iter().flatten()).any(|l| l.at("name") == "keep");
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
    let lane = crate::config::env("DROVER_LANE").unwrap_or_default();
    let mut it = lane.split(':');
    let own = (it.next() == Some(fleet)).then(|| it.next()?.parse().ok()).flatten();
    d.put("lane", json!(lane));
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

/// `drover triage close`: close an unprotected issue that is done, a duplicate or superseded, with one comment linking the evidence and its undo.
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

/// `drover triage fold`: an unprotected issue's body becomes `## Spec (current)` (the `spec` file) over its old text in a `<details>` block; `NOTHING_TO_FOLD` without amendments.
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
    let old = issue.at("body").as_str().unwrap_or_default().trim_end();
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
    crate::hygiene::gh(
        ctx,
        r,
        cfg,
        fleet,
        &["issue", "edit", &n.to_string(), "-R", repo, "--body", &body],
    )?;
    let undo = "its earlier text is in the folded block";
    log(
        ctx,
        fleet,
        "triage-folded",
        json!({"target": n, "amendments": amendments, "undo": undo}),
    )?;
    Ok(writeln!(out, "folded {fleet}#{n}: {amendments} amendments")?)
}

/// `drover triage label`: add `labels` (`stale?`, the fleet's own) to an unprotected issue.
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

/// `drover triage docs`: file the run's one `docs:` issue from the `findings` file (one per line); no finding, no issue; a README with no non-goals goes first.
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
    let lane = crate::config::env("DROVER_LANE").unwrap_or_default();
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
    let url = crate::hygiene::gh(
        ctx,
        r,
        cfg,
        fleet,
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
