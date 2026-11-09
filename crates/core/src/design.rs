//! #412: an issue's optional design stage: the design lane's hand-off for review, and the owner's decision on it.
//! #730: an architect stage works the same way; its events name `role: architect` (none: `design`).
use crate::config::Config;
use crate::events::{emit, last_spawn, read, Event, IssueState};
use crate::runner::{ok, Runner};
use crate::Ctx;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

/// The design ask's options; free text is a revise note.
pub const OPTIONS: [&str; 3] = ["approve", "revise", "reject"];

/// A design's ask (`needs-decision` with `design: true`).
pub fn is_ask(e: &Event) -> bool {
    e.kind == "needs-decision" && e.data.get("design") == Some(&Value::Bool(true))
}

/// The head the issue's `role` stage started from: its first spawn's since the last reject.
fn pre<'a>(ev: &'a [Event], f: &str, n: u64, role: &str) -> Option<&'a str> {
    let from = ev
        .iter()
        .rposition(|e| e.is("design-rejected", f, n))
        .map_or(0, |i| i + 1);
    let design = |e: &&Event| e.is("spawn", f, n) && e.str("role") == Some(role);
    ev[from..]
        .iter()
        .filter(design)
        .find_map(|e| e.str("sha").filter(|s| !s.is_empty()))
}

/// A design lane that ended its turn (not working) with a pushed commit made since it spawned hands off:
/// `design-review`, then one ask that the board card and the conductor thread which queued it (if any) both answer.
pub fn handoff(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    agents: &[Value],
    s: &IssueState,
) -> Result<()> {
    let ev = read(&ctx.state)?;
    let Some(sp) = s
        .lane
        .as_deref()
        .filter(|_| s.state == "design")
        .and_then(|l| last_spawn(&ev, l))
    else {
        return Ok(());
    };
    let busy = agents
        .iter()
        .any(|a| sp.str("agent") == a["name"].as_str() && a["agent_status"] == "working");
    let Some(wt) = sp.str("worktree").filter(|_| !busy) else {
        return Ok(());
    };
    let log = ok(r, "git", &["-C", wt, "log", "-1", "--format=%H %ct"]).unwrap_or_default();
    let (head, t) = log.trim().split_once(' ').unwrap_or_default();
    let new = t.parse().is_ok_and(|t: u64| t > sp.ts) && sp.str("sha") != Some(head);
    if !new || crate::spawn::dirty(r, Path::new(wt)).is_some() {
        return Ok(());
    }
    let (f, n) = (s.fleet.as_str(), s.issue);
    let role = sp.str("role").unwrap_or("design");
    let pre = pre(&ev, f, n, role).unwrap_or(head);
    let dir = if role == "architect" {
        "docs/architecture"
    } else {
        "docs/design"
    };
    let names = ok(r, "git", &["-C", wt, "diff", "--name-only", pre, head, "--", dir]).unwrap_or_default();
    // #730: the ADR's folder, `docs/architecture/<slug>`, which the impl and rev briefs link once approved.
    let adr = names.lines().next().filter(|_| role == "architect");
    let adr = adr.map(|l| l.split('/').take(3).collect::<Vec<_>>().join("/"));
    let png = |l: &&str| l.to_ascii_lowercase().ends_with(".png");
    let files: Vec<Value> = names
        .lines()
        .filter(png)
        .map(|l| json!({"name": l, "path": Path::new(wt).join(l)}))
        .collect();
    writeln!(out, "{f} #{n} design-review: {head:.7}")?;
    let d = json!({"sha": head, "pre": pre, "agent": sp.str("agent"), "files": files, "role": role, "adr": adr});
    emit(&ctx.state, cfg, Event::new(ctx.now, f, Some(n), "design-review", d))?;
    let mark = |e: &&Event| {
        e.kind == "queue"
            && e.fleet == f
            && (e.data.get("design").is_some() || e.data.get("architect").is_some())
            && e.data["issues"].as_array().is_some_and(|a| a.contains(&json!(n)))
    };
    let thread = ev
        .iter()
        .rfind(mark)
        .and_then(|e| e.str("design_thread"))
        .filter(|t| !t.is_empty());
    let id = format!("q{}", ev.iter().filter(|e| e.kind == "needs-decision").count() + 1);
    let what = if role == "architect" {
        "Architecture (ADR)"
    } else {
        "Design"
    };
    let q = format!("{what} for #{n} is ready ({head:.7}): approve, revise (type a note) or reject?");
    let d = json!({"id": id, "question": q, "options": OPTIONS, "recommend": null, "kind": "owner", "design": true, "files": files,
        "thread": thread, "role": role});
    emit(&ctx.state, cfg, Event::new(ctx.now, f, Some(n), "needs-decision", d))
}

/// The owner's answer `text` to design ask `ask`: approve, reject, or (anything else) revise with it as the note.
/// Approve and reject end the design lane (its pane closes); reject pushes one commit back to the pre-design tree.
pub fn decide(ctx: &Ctx, cfg: &Config, r: &dyn Runner, ask: &Event, text: &str, by: &str) -> Result<()> {
    let (f, n) = (ask.fleet.as_str(), ask.issue.context("a design ask names its issue")?);
    let ev = read(&ctx.state)?;
    let review = ev.iter().rfind(|e| e.is("design-review", f, n));
    let sp = review.and_then(|e| last_spawn(&ev, e.str("agent")?));
    let (kind, mut d) = match text {
        "approve" => ("design-approved", json!({})),
        "reject" => ("design-rejected", json!({})),
        t => (
            "design-revise",
            json!({"note": t.trim_start_matches("revise").trim_start_matches(':').trim()}),
        ),
    };
    if let Some((agent, pane)) = sp
        .and_then(|e| Some((e.str("agent")?, e.str("pane")?)))
        .filter(|_| kind != "design-revise")
    {
        let _ = crate::herdr::close(r, pane);
        let c = json!({"agent": agent, "pane": pane, "reason": kind});
        emit(&ctx.state, cfg, Event::new(ctx.now, f, Some(n), "lane-closed", c))?;
    }
    if kind == "design-rejected" {
        let at = |k| sp.and_then(|e| e.str(k)).context("no design lane to revert");
        let pre = review.and_then(|e| e.str("pre")).context("no pre-design head")?;
        let (wt, branch) = (at("worktree")?, at("branch")?);
        let git = |a: &[&str]| ok(r, "git", &[&["-C", wt], a].concat()).map(|o| o.trim().to_string());
        let tree = git(&["rev-parse", &format!("{pre}^{{tree}}")])?;
        let msg = format!("design: revert the rejected design of #{n} (back to {pre:.7})");
        let c = git(&["commit-tree", &tree, "-p", "HEAD", "-m", &msg])?;
        git(&["reset", "--hard", &c])?;
        git(&["push", "origin", &format!("{c}:refs/heads/{branch}")])?;
        (d["pre"], d["sha"]) = (json!(pre), json!(c));
    }
    (d["id"], d["by"]) = (ask.data["id"].clone(), json!(by));
    (d["role"], d["adr"]) = (
        ask.data["role"].clone(),
        review.map_or(Value::Null, |e| e.data["adr"].clone()),
    );
    emit(&ctx.state, cfg, Event::new(ctx.now, f, Some(n), kind, d))
}
