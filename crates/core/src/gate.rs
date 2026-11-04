//! `herdsman verdict` and `herdsman merge`: the review → merge gate.
use crate::config::{Check, Config, Fleet, Premerge};
use crate::events::{emit, fold, read, Event, IssueState};
use crate::overlap::git;
use crate::runner::{argv, json, ok, Runner};
use crate::Ctx;
use crate::{check, gh, herdr};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

#[derive(Debug)]
pub struct Refusal {
    pub code: &'static str,
    pub msg: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "REFUSED {}: {}", self.code, self.msg)
    }
}
impl std::error::Error for Refusal {}

pub fn refuse(code: &'static str, msg: impl Into<String>) -> Refusal {
    Refusal { code, msg: msg.into() }
}

/// `refuse` as an `anyhow` error.
pub fn refused<T>(code: &'static str, msg: impl Into<String>) -> Result<T> {
    Err(refuse(code, msg).into())
}

fn is_main(b: &str) -> bool {
    b == "main" || b == "master"
}

pub fn issue_state(ctx: &Ctx, fleet: &str, issue: u64) -> Result<Option<IssueState>> {
    Ok(fold(&read(&ctx.state)?).remove(&(fleet.to_string(), issue)))
}

pub fn branch_of(st: Option<&IssueState>, issue: u64) -> String {
    st.and_then(|s| s.branch.clone())
        .unwrap_or_else(|| format!("issue-{issue}"))
}

#[allow(clippy::too_many_arguments)] // the CLI's verdict arguments, one each
pub fn verdict(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, issue): (&str, u64),
    pass: bool,
    sha: &str,
    file: &str,
) -> Result<()> {
    let f = cfg.fleet(fleet)?;
    if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return refused("BAD_SHA", format!("--sha {sha} must be a full 40-hex commit sha"));
    }
    // #249: a verdict.md in a remote review lane's worktree is read on that lane's host.
    let evs = read(&ctx.state)?;
    let reviewer = |e: &&Event| e.is("spawn", fleet, issue) && crate::events::checks(e);
    let at = evs
        .iter()
        .rfind(reviewer)
        .filter(|e| e.str("worktree").is_some_and(|w| file.starts_with(w)));
    let body = (crate::hosts::On::of(r, at).read(Path::new(file)))
        .with_context(|| format!("cannot read verdict file {file}"))?;
    anyhow::ensure!(!body.trim().is_empty(), "verdict file {file} is empty");
    let st = issue_state(ctx, fleet, issue)?;
    let branch = branch_of(st.as_ref(), issue);
    let api = format!("repos/{}/branches/{}", f.repo, branch);
    let head = ok(r, "gh", &["api", &api, "--jq", ".commit.sha"])?.trim().to_string();
    // #207: a head that merges `sha` with integration and adds nothing takes the verdict at `sha`; merge carries it.
    let carried = || {
        st.as_ref()
            .is_some_and(|s| crate::overlap::carries(r, f, s, sha, &head))
    };
    if !head.eq_ignore_ascii_case(sha) && !carried() {
        return refused("STALE_SHA", format!("{branch} head is {head}, not {sha}"));
    }
    let head = sha.to_ascii_lowercase();
    let v = if pass { "PASS" } else { "FAIL" };
    // #173: a review lane's verdict is its check stage's; any other (CI red while ready) is the gate's, and counts
    // against the rounds of the check its fix goes back to.
    let p = f.flow();
    let stage = match st.as_ref() {
        Some(s) if s.state.starts_with("review") && s.check.is_some() => s.stage.as_str(),
        _ => p.gate(),
    };
    let lp = p.loop_of(stage);
    // #228: `FAIL (form-only)` with every blocker tagged a form category doesn't count, `form_rounds` times per issue.
    let (used, of) = (st.as_ref().map_or(0, |s| s.form), f.form_rounds.unwrap_or_default());
    let tagged = !pass && body.lines().next().is_some_and(|l| l.trim() == "FAIL (form-only)");
    let demoted = if !tagged {
        None
    } else if !form_only(&body, f.form_categories.as_deref().unwrap_or_default()) {
        Some("form-only-rejected")
    } else {
        (used >= of).then_some("form-rounds-exhausted")
    };
    if let Some(k) = demoted {
        writeln!(out, "{k}: recorded as a plain FAIL")?;
        emit(
            &ctx.state,
            cfg,
            Event::new(ctx.now, fleet, Some(issue), k, json!({"sha": head})),
        )?;
    }
    let form = tagged && demoted.is_none();
    let round = st.as_ref().map_or(1, |s| {
        let new = !s.verdicts.iter().any(|x| x.0.eq_ignore_ascii_case(&head) && x.2 == lp);
        s.stage_rounds.get(lp).copied().unwrap_or_default() + u32::from(new && !form)
    });
    let data = json!({"verdict": v, "sha": head, "branch": branch, "file": file, "body": body, "round": round,
        "stage": stage, "loop": lp, "form_only": form, "spawn_sha": at.and_then(|e| e.str("sha"))});
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, fleet, Some(issue), "verdict", data),
    )?;
    let (limit, of_limit) = limit(f, lp, st.as_ref());
    let (v, note) = if form {
        ("FAIL (form-only)", format!(" · not counted · form {}/{of}", used + 1))
    } else {
        (v, String::new())
    };
    writeln!(
        out,
        "recorded {v} for {fleet}#{issue} @ {head} (round {round}{of_limit}{note})"
    )?;
    if !pass && !form && round >= limit {
        let of = match used {
            0 => String::new(),
            n => format!(", {n} form-only not counted"),
        };
        let d = json!({"reason": format!("review rounds ran out ({round}{of_limit}{of})"), "findings": body});
        emit(
            &ctx.state,
            cfg,
            Event::new(ctx.now, fleet, Some(issue), "needs-human", d),
        )?;
        writeln!(out, "round limit reached → needs-human")?;
    }
    if pass && f.merge_label.is_some() {
        // The label is retried by the next drive pass; the verdict stands either way.
        gh::forget_pr(ctx, fleet, issue);
        let pr = gh::pr(ctx, r, f, (fleet, issue), &branch, ctx.now)?;
        if let Some(s) = issue_state(ctx, fleet, issue)? {
            if let Err(e) = sync_label(ctx, cfg, r, out, &s, &pr) {
                writeln!(out, "label: {e:#}")?;
            }
        }
    }
    // #128: the verdict is the reviewer's only work: remove its worktree and close its pane, or keep a dirty one.
    let events = read(&ctx.state)?;
    // #164: a PASS at the head leaves the builder waiting for merge: its build folders go (a fix round rebuilds).
    let is_impl = |e: &&Event| pass && e.is("spawn", fleet, issue) && !crate::events::checks(e);
    if let Some(b) = events.iter().rfind(is_impl) {
        let wt = Path::new(b.str("worktree").unwrap_or_default());
        let working = |a: &serde_json::Value| a["name"].as_str() == b.str("agent") && a["agent_status"] == "working";
        if wt.is_dir() && herdr::agents(r).is_ok_and(|a| !a.iter().any(working)) {
            check::purge(r, cfg, fleet, out, wt)?;
        }
    }
    let is_rev = |e: &&Event| e.is("spawn", fleet, issue) && crate::events::checks(e);
    let rev = events.iter().rfind(is_rev);
    let on = rev.map(|e| crate::hosts::On::of(r, Some(e)));
    let exists = |w: &&str| on.as_ref().is_some_and(|on| on.exists(Path::new(w)));
    let (Some((e, wt)), Some(r)) = (rev.and_then(|e| Some((e, e.str("worktree").filter(exists)?))), &on) else {
        return Ok(());
    };
    let (agent, pane) = (e.str("agent"), e.str("pane").unwrap_or_default());
    // A reviewer herdr still shows `working` (or an unreadable agent list) is left alone.
    let busy = herdr::agents(r).map_or(true, |a| {
        a.iter()
            .any(|a| a["name"].as_str() == agent && a["agent_status"] == "working")
    });
    if busy {
        return Ok(());
    }
    // #164: its build folders go now, even when a dirty worktree stays.
    check::purge(r, cfg, fleet, out, Path::new(wt))?;
    let ws = e.str("workspace").unwrap_or_default();
    let gone = match crate::spawn::dirty(r, Path::new(wt)) {
        Some(why) => Err(anyhow::anyhow!(why)),
        None => check::reap(ctx, cfg, r, (fleet, Some(issue)), wt)
            .and_then(|_| check::remove(r, cfg, fleet, wt, ws))
            .and_then(|()| herdr::close(r, pane).map(drop)),
    };
    let kind = if gone.is_ok() { "lane-closed" } else { "lane-kept" };
    let reason = gone.map_or_else(|why| format!("{wt}: {why:#}"), |()| "verdict recorded".into());
    writeln!(out, "{kind} {}: {reason}", agent.unwrap_or_default())?;
    let d = json!({"agent": agent, "reason": reason});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(issue), kind, d))
}

/// A PASS stands at `head`: its own, or (#129) that of the head an auto-update chain started from.
fn passed(events: &[Event], s: &IssueState, head: &str) -> bool {
    let mut h = head;
    loop {
        if let Some((_, v, _)) = s.verdicts.iter().rev().find(|(x, ..)| x.eq_ignore_ascii_case(h)) {
            return v == "PASS";
        }
        let up = events
            .iter()
            .find(|e| e.is("auto-updated", &s.fleet, s.issue) && e.str("to") == Some(h));
        match up.and_then(|e| e.str("from")) {
            Some(from) if from != h => h = from,
            _ => return false,
        }
    }
}

/// #150: keep the fleet's `merge_label` on the open PR exactly while a PASS stands at its head: add it once
/// (`labelled`), take it off a head that moved past the PASS (`unlabelled`), so a stale PASS never starts the full run.
pub fn sync_label(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    s: &IssueState,
    pr: &Value,
) -> Result<()> {
    let (fleet, f) = (s.fleet.as_str(), cfg.fleet(&s.fleet)?);
    let head = pr["headRefOid"].as_str().unwrap_or_default();
    let (Some(label), Some(num)) = (&f.merge_label, pr["number"].as_u64()) else {
        return Ok(());
    };
    // #350: a fast-first PR lands without its full run.
    if s.done() || pr["state"] != "OPEN" || head.is_empty() || pr["landing"] == "fast-first" {
        return Ok(());
    }
    // #285: a pure merge of the PASS head with integration (#207's carry) keeps the PASS, so the label stays.
    let carried = || s.verdicts.iter().any(|v| v.1 == "PASS") && crate::overlap::pure(r, f, s, head);
    let (flag, kind) = match (
        passed(&read(&ctx.state)?, s, head) || carried(),
        gh::has_label(pr, label),
    ) {
        (true, false) => ("--add-label", "labelled"),
        (false, true) => ("--remove-label", "unlabelled"),
        _ => return Ok(()),
    };
    let n = num.to_string();
    ok(r, "gh", &["pr", "edit", &n, "-R", &f.repo, flag, label])?;
    gh::forget_pr(ctx, fleet, s.issue);
    writeln!(out, "{fleet} #{} {kind} PR #{n} `{label}` @ {head:.7}", s.issue)?;
    let d = json!({"label": label, "pr": num, "sha": head});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(s.issue), kind, d))
}

#[doc(hidden)]
pub fn ci_green(rollup: &Value, required: &[String]) -> Result<(), String> {
    rollup
        .as_array()
        .filter(|a| !a.is_empty() || !required.is_empty())
        .ok_or("no checks reported")?;
    let prefix = if required.is_empty() { "" } else { "required " };
    gh::not_green(&gh::required(rollup, required)).map_or(Ok(()), |m| Err(format!("{prefix}{m}")))
}

/// #228: every blocking finding (a `blocking` line, not `non-blocking`) is tagged `blocking [<category>]` with one
/// of `cats`; at least one blocker.
#[doc(hidden)]
pub fn form_only(body: &str, cats: &[String]) -> bool {
    // #249: a markdown heading (`## Blocking`) is no finding.
    let blockers: Vec<String> = (body.lines().skip(1).map(str::to_lowercase))
        .filter(|l| !l.trim_start().starts_with('#') && l.contains("blocking") && !l.contains("non-blocking"))
        .collect();
    let tag = |l: &str| {
        let (_, rest) = l.split_once("blocking")?;
        let rest = rest.trim_start_matches(['*', '_', ':', ' ']).strip_prefix('[')?;
        rest.split_once(']').map(|t| t.0.trim().to_string())
    };
    !blockers.is_empty() && blockers.iter().all(|l| tag(l).is_some_and(|t| cats.contains(&t)))
}

/// #309: a check stage's rounds (unset: no limit) plus the owner's extends, worded `/3` or ` of 9+1 (owner extended)`.
pub fn limit(f: &Fleet, stage: &str, st: Option<&IssueState>) -> (u32, String) {
    let base = f.flow().get(stage).and_then(|s| s.rounds).unwrap_or(u32::MAX);
    match st.map_or((0, base), |s| (s.extra, base.saturating_add(s.extra).max(s.granted))) {
        (0, l) => (l, format!("/{l}")),
        (_, l) => (l, format!(" of {base}+{} (owner extended)", l - base)),
    }
}

/// Pure merge gate: every check stage's PASS at the head, within its rounds (`over`: past them). Returns the head sha
/// that may be merged.
pub fn gate(f: &Fleet, pr: &Value, st: Option<&IssueState>, over: bool) -> Result<String, Refusal> {
    let integration = f.integration.as_str();
    let base = pr["baseRefName"].as_str().unwrap_or_default();
    if is_main(integration) || is_main(base) {
        return Err(refuse(
            "MAIN_TARGET",
            format!("target `{integration}`/base `{base}`: main is promoted by a human only"),
        ));
    }
    if base != integration {
        return Err(refuse(
            "BASE_MISMATCH",
            format!("PR base `{base}` ≠ integration `{integration}`"),
        ));
    }
    let head = pr["headRefOid"].as_str().unwrap_or_default().to_string();
    let verdicts = st.map(|s| s.verdicts.as_slice()).unwrap_or_default();
    if verdicts.is_empty() {
        return Err(refuse(
            "NO_VERDICT",
            "no review verdict recorded; run a reviewer and `herdsman verdict`",
        ));
    }
    let at = |x: &&(String, String, String)| x.0.eq_ignore_ascii_case(&head);
    let Some((_, v, _)) = verdicts.iter().rev().find(at) else {
        return Err(refuse(
            "STALE_VERDICT",
            format!("no verdict for head {head}; re-review the current head"),
        ));
    };
    if v != "PASS" {
        return Err(refuse("VERDICT_FAIL", format!("verdict for {head} is {v}")));
    }
    // #173: a 2-check pipeline merges only with a PASS from each at this head.
    for c in f.flow().checks() {
        let last = verdicts.iter().rev().filter(at).find(|x| x.2 == c.name);
        match last.map(|x| x.1.as_str()) {
            Some("PASS") => {}
            Some(v) => return Err(refuse("VERDICT_FAIL", format!("{} verdict for {head} is {v}", c.name))),
            None => return Err(refuse("STALE_VERDICT", format!("no {} PASS for head {head}", c.name))),
        }
        let n = st.map_or(0, |s| s.stage_rounds.get(&c.name).copied().unwrap_or_default());
        let (limit, of) = limit(f, &c.name, st);
        if n > limit && !over {
            return Err(refuse("ROUND_LIMIT", format!("review rounds ran out ({n}{of})")));
        }
    }
    ci_green(&pr["statusCheckRollup"], f.checks(pr)).map_err(|m| refuse("CI_NOT_GREEN", format!("{m} at {head}")))?;
    Ok(head)
}

/// #198: the merge commit reads like a changelog: `<title> (#PR)`, then the PR body's `## For humans` lines and a
/// facts line. Returns (subject, body, whether the summary was there).
pub fn merge_message(pr: &Value, issue: u64, rounds: u32, sha: &str) -> (String, String, bool) {
    let title = pr["title"].as_str().unwrap_or_default().trim();
    let strip = |open: &str, close: char| {
        let t = title.strip_prefix(open)?;
        let rest = t.trim_start_matches(|c: char| c.is_ascii_digit());
        (rest.len() < t.len()).then_some(rest)?.strip_prefix(close)
    };
    let title = strip("[#", ']').or_else(|| strip("#", ':')).unwrap_or(title).trim();
    let num = pr["number"].as_u64().unwrap_or_default();
    let human: Vec<&str> = (pr["body"].as_str().unwrap_or_default().lines())
        .skip_while(|l| !l.trim().eq_ignore_ascii_case("## For humans"))
        .skip(1)
        .take_while(|l| !l.starts_with('#'))
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let files = pr["files"].as_array().into_iter().flatten();
    let src: i64 = (files.filter(|x| x["path"].as_str().is_some_and(|p| p.starts_with("src/"))))
        .map(|x| x["additions"].as_i64().unwrap_or(0) - x["deletions"].as_i64().unwrap_or(0))
        .sum();
    let s = if rounds == 1 { "" } else { "s" };
    let facts = format!("Closes #{issue} · {rounds} review round{s} · src {src:+} · PASS {sha:.7}");
    let body = if human.is_empty() {
        facts
    } else {
        format!("{}\n\n{facts}", human.join("\n"))
    };
    (format!("{title} (#{num})"), body, !human.is_empty())
}

fn premerge_file(ctx: &Ctx, fleet: &str, issue: u64) -> std::path::PathBuf {
    ctx.state.with_file_name(format!("premerge-{fleet}-{issue}.md"))
}

/// #272: the fleet's `premerge` checks on `head` merged into current integration, in a scratch worktree; the seconds
/// they took (`None`: no checks). Integration moving during the checks re-runs them once. A failed check is a fix round.
fn premerge(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, issue): (&str, u64),
    head: &str,
) -> Result<Result<Option<u64>, Refusal>> {
    let f = cfg.fleet(fleet)?;
    let checks = premerge_checks(ctx, f);
    if checks.is_empty() {
        return Ok(Ok(None));
    }
    let (wt, into) = (
        ctx.state.with_file_name(format!("premerge-{fleet}")),
        format!("origin/{}", f.integration),
    );
    let (dir, wts) = (f.local_path(), wt.to_string_lossy());
    let tip = || git(r, &dir, &["fetch", "-q", "origin"]).and_then(|_| git(r, &dir, &["rev-parse", &into]));
    let tree = |a: &[&str]| git(r, &dir, &[&["worktree"][..], a].concat());
    for _ in 0..2 {
        let base = tip()?;
        let _ = tree(&["remove", "--force", &wts]);
        tree(&["add", "-f", "--detach", &wts, &base])?;
        let clean = git(r, &wt, &["merge", "--no-ff", "--no-edit", head]).is_ok();
        let t = std::time::Instant::now();
        let fail = clean.then(|| run_checks(r, &wt, &base, &checks)).flatten();
        let secs = t.elapsed().as_secs();
        let _ = tree(&["remove", "--force", &wts]);
        if !clean {
            return Ok(Err(refuse("NOT_MERGEABLE", format!("{head:.7} conflicts with {into}"))));
        }
        if let Some((c, o)) = fail {
            let at = format!("{head:.7} merged with {into} @ {base:.7}, {secs}s");
            return Err(premerge_failed(ctx, cfg, r, out, (fleet, issue), (head, &at), (c, &o))?.into());
        }
        if tip()? == base {
            return Ok(Ok(Some(secs)));
        }
    }
    Ok(Err(refuse(
        "STAGING_MOVED",
        format!("{into} moved during the premerge checks twice"),
    )))
}

pub(crate) fn premerge_checks(ctx: &Ctx, f: &Fleet) -> Vec<Check> {
    let has = f.local_path().join("ci/cap.toml").exists();
    let cap = has.then(|| Premerge::Cmd(format!("{} cap-check --config ci/cap.toml", ctx.exe)));
    let check = |p| match p {
        Premerge::Cmd(run) => Check {
            name: run.clone(),
            run,
            ..Check::default()
        },
        Premerge::Named(c) => c,
    };
    let all = f.premerge.clone().unwrap_or(cap.into_iter().collect());
    all.into_iter().map(check).collect()
}

/// #373: the first failing check (and its output) in the scratch worktree `wt`, `base` merged with the PR.
pub(crate) fn run_checks<'a>(r: &dyn Runner, wt: &Path, base: &str, cs: &'a [Check]) -> Option<(&'a Check, String)> {
    let diff = || git(r, wt, &["diff", "--name-only", base, "HEAD"]).unwrap_or_default();
    let files = cs.iter().any(|c| !c.when.is_empty()).then(diff).unwrap_or_default();
    let glob = |g: &String| files.lines().any(|p| crate::config::glob(g.as_bytes(), p.as_bytes()));
    let hit = |c: &&Check| c.when.is_empty() || c.when.iter().any(&glob);
    cs.iter().filter(hit).find_map(|c| {
        let sh = argv(&["-c", &format!("cd \"$0\" && {}", c.run), &wt.to_string_lossy()]);
        let o = r.run("sh", &sh).unwrap_or_default();
        (o.code != 0).then(|| (c, format!("{}{}", o.stdout, o.stderr)))
    })
}

/// #272/#373: a failed check at `head` is a FAIL verdict (a fix round briefed with its output or `fix` text, no new
/// review round) and `premerge-failed`; returns the refusal: the check's `code` (else `STAGING_WOULD_BREAK`).
pub(crate) fn premerge_failed(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, issue): (&str, u64),
    (head, at): (&str, &str),
    (c, o): (&Check, &str),
) -> Result<Refusal> {
    let o = crate::lanes::mask(o);
    let lines: Vec<&str> = o.lines().filter(|l| !l.trim().is_empty()).collect();
    let err = |l: &&&str| c.code.is_none() && l.contains("error");
    let first = lines.iter().find(err).or(lines.first()).map_or("failed", |l| l.trim());
    let msg = c.code.as_ref().map_or(format!("{}: {first}", c.name), |_| first.into());
    let block = format!("\n```\n{}\n```\n", lines[lines.len().saturating_sub(20)..].join("\n"));
    let min = o.lines().find_map(|l| l.split_once("min_version="));
    let min = min.map_or("", |m| m.1.trim());
    let tpl = |t: &String| t.replace("{output}", &block).replace("{min}", min);
    let file = premerge_file(ctx, fleet, issue);
    let fix = c.fix.as_ref().map_or(format!("```\n{o}\n```"), tpl);
    std::fs::write(&file, format!("FAIL\n\nPremerge on {at}: {msg}\n\n{fix}\n"))?;
    let what = c.fix.as_deref().and_then(|t| t.split_whitespace().next());
    let what = what.map(|w| format!(": {} needed", w.to_lowercase()));
    let code: &'static str = c.code.clone().map_or("STAGING_WOULD_BREAK", |k| &*k.leak());
    let note = crate::lanes::mask(&format!("{} failed{}", c.name, what.unwrap_or_default()));
    let d = json!({"name": crate::lanes::mask(&c.name), "code": code, "note": note});
    let ev = Event::new(ctx.now, fleet, Some(issue), "premerge-failed", d);
    emit(&ctx.state, cfg, ev)?;
    verdict(ctx, cfg, r, out, (fleet, issue), false, head, &file.to_string_lossy())?;
    Ok(refuse(code, msg))
}

/// #294: the serial groups (name order) `n`'s PR diff touches and it is not in yet, each with the first such file.
pub(crate) fn unjoined<'a>(
    ctx: &Ctx,
    r: &dyn Runner,
    f: &'a Fleet,
    (fleet, n): (&str, u64),
    (num, head): (&str, &str),
) -> Result<Vec<(&'a crate::config::Serial, String)>> {
    let ev = read(&ctx.state)?;
    let mine = crate::queue::groups(&crate::queue::replay(&ev, fleet), &ev, f, fleet, n);
    let mut new: Vec<_> = f
        .serial
        .iter()
        .filter(|g| !mine.iter().any(|m| m.name == g.name))
        .collect();
    new.sort_by(|a, b| a.name.cmp(&b.name));
    let files = if new.is_empty() {
        vec![]
    } else {
        gh::diff_names(ctx, r, f, (fleet, n), num, head)?
    };
    Ok(new
        .into_iter()
        .filter_map(|g| g.hit(&files).map(|p| (g, p.clone())))
        .collect())
}

/// `over` (`--owner-override`, #130) lifts only `ROUND_LIMIT`.
pub fn merge(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    fleet: &str,
    issue: u64,
    over: bool,
) -> Result<()> {
    let f = cfg.fleet(fleet)?;
    let events = read(&ctx.state)?;
    let st = fold(&events).remove(&(fleet.to_string(), issue));
    // A retry after a merge is a no-op: no PR lookup, no event, no alert.
    if st.as_ref().is_some_and(|s| s.state == "merged") {
        let m = events.iter().find(|e| e.is("merged", fleet, issue));
        let s = |k| m.and_then(|e| e.str(k)).unwrap_or("?");
        writeln!(out, "already merged → {} (PR #{})", s("merge_sha"), s("pr"))?;
        return Ok(());
    }
    let branch = branch_of(st.as_ref(), issue);
    let mut pr = if is_main(&f.integration) {
        json!({"baseRefName": f.integration})
    } else {
        match gh::pr(ctx, r, f, (fleet, issue), &branch, ctx.now)? {
            v if v["state"] == "OPEN" => v,
            _ => json!({"error": "no-pr"}),
        }
    };
    crate::land::mark(ctx, Some(r), f, (fleet, issue), &events, &mut pr);
    let fast = pr["landing"] == "fast-first";
    // #129: an auto-update (a merge of integration that adds nothing) keeps the PASS of the head it updated.
    let h = pr["headRefOid"].as_str().unwrap_or_default();
    let st = st.map(|mut s| {
        if !h.is_empty() && !s.reviewed(h) && crate::overlap::pure(r, f, &s, h) {
            s.verdicts
                .extend(f.flow().checks().map(|c| (h.into(), "PASS".into(), c.name.clone())));
        }
        s
    });
    let num = pr["number"].as_u64().unwrap_or_default().to_string();
    let res = if pr.get("error").is_some() {
        Err(refuse("NO_PR", format!("no open PR for branch {branch} in {}", f.repo)))
    } else {
        gate(f, &pr, st.as_ref(), over)
    };
    // #294: belt and braces: a group the PR touches but drive never joined it to.
    let res = match res {
        Ok(head) => match unjoined(ctx, r, f, (fleet, issue), (&num, &head))?.first() {
            Some((g, p)) => Err(refuse(
                "SERIAL_UNMARKED",
                format!("{}: PR touches {p}; queue #{issue} with `--serial {0}` first", g.name),
            )),
            None => Ok(head),
        },
        res => res,
    };
    // #350: swap-if-unchanged: a fast-first PR lands only on the integration its fast checks ran against.
    let res = match res {
        Ok(h) if fast && !crate::land::based(r, f, &h) => Err(refuse(
            "BEHIND",
            format!(
                "{} moved past {h:.7}; drive catches it up, its fast checks run again",
                f.integration
            ),
        )),
        res => res,
    };
    let res = match res {
        Ok(h) => premerge(ctx, cfg, r, out, (fleet, issue), &h)?.map(|s| (h, s)),
        Err(rf) => Err(rf),
    };
    let (head, secs) = match res {
        Ok(h) => h,
        // #148: CI not reported yet or still running is no block; the item stays ready, waiting for CI.
        Err(rf)
            if rf.code == "STAGING_MOVED"
                || rf.code == "BEHIND"
                || rf.code == "CI_NOT_GREEN"
                    && gh::ci(&gh::required(&pr["statusCheckRollup"], f.checks(&pr))) == "pending" =>
        {
            return Err(rf.into());
        }
        Err(rf) => {
            let kind = if rf.code == "ROUND_LIMIT" {
                "needs-human"
            } else {
                "blocked"
            };
            let d = json!({"code": rf.code, "reason": rf.msg});
            emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(issue), kind, d))?;
            return Err(rf.into());
        }
    };
    gh::forget_pr(ctx, fleet, issue);
    let view = ["pr", "view", &num, "-R", &f.repo, "--json", "number,title,body,files"];
    let info = json(r, "gh", &view).unwrap_or_else(|_| pr.clone());
    let (subject, body, human) = merge_message(&info, issue, st.as_ref().map_or(0, |s| s.rounds), &head);
    if !human {
        let d = json!({"pr": num});
        emit(
            &ctx.state,
            cfg,
            Event::new(ctx.now, fleet, Some(issue), "no-human-summary", d),
        )?;
    }
    let m = [
        "--merge",
        "--match-head-commit",
        &head,
        "--subject",
        &subject,
        "--body",
        &body,
    ];
    ok(r, "gh", &[&["pr", "merge", &num, "-R", &f.repo][..], &m].concat())?;
    let mc = json(r, "gh", &["pr", "view", &num, "-R", &f.repo, "--json", "mergeCommit"])?;
    let merge_sha = mc["mergeCommit"]["oid"].as_str().unwrap_or("?").to_string();
    let mut d = json!({"pr": num, "merge_sha": merge_sha, "verdict_sha": head, "into": f.integration});
    if let Some(s) = secs {
        d["premerge_secs"] = json!(s);
    }
    if over {
        (d["by"], d["override"]) = (json!("owner"), json!("ROUND_LIMIT"));
    }
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(issue), "merged", d))?;
    if f.land.is_some() {
        // #350: what `land::watch` and `stats land` read: the landing's mode and how long its gating checks took.
        let checks = gh::required(&pr["statusCheckRollup"], f.checks(&pr));
        let at = |k: &'static str| {
            checks
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(move |c| crate::usage::epoch(c[k].as_str()?))
        };
        let secs = at("completedAt")
            .max()
            .zip(at("startedAt").min())
            .map(|(b, a)| b.saturating_sub(a));
        let mode = if fast { "fast-first" } else { "full-first" };
        let d = json!({"mode": mode, "sha": merge_sha, "pr": num, "checks_secs": secs});
        emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(issue), "landed", d))?;
    }
    // The one GitHub comment fleet writes: ≤ 5 lines, merge sha + verdict sha.
    let evidence = format!(
        "Merged via `herdsman merge`: PR #{num} → `{}` @ `{merge_sha}`\nPinned PASS verdict sha: `{head}`{}",
        f.integration,
        if over { "\nOwner override: ROUND_LIMIT" } else { "" }
    );
    let n = issue.to_string();
    writeln!(out, "{evidence}")?;
    let comment = ["issue", "comment", &n, "-R", &f.repo, "--body", &evidence];
    if let Err(e) = ok(r, "gh", &comment).or_else(|_| ok(r, "gh", &comment)) {
        let reason = format!("PR #{num} merged @ {merge_sha}, but the evidence comment failed twice: {e:#}");
        emit(
            &ctx.state,
            cfg,
            Event::new(
                ctx.now,
                fleet,
                Some(issue),
                "evidence-missing",
                json!({"reason": reason}),
            ),
        )?;
        bail!("{reason}; issue #{issue} left open");
    }
    // Already closed by `Closes #N` is fine.
    let _ = r.run(
        "gh",
        &argv(&["issue", "close", &n, "-R", &f.repo, "--reason", "completed"]),
    );
    // The merge stands either way; a teardown problem is reported, not fatal.
    if let Err(e) = crate::check::teardown(ctx, cfg, r, out, fleet, issue) {
        writeln!(out, "teardown: {e:#}")?;
    }
    if f.repo.ends_with("/herdsman") {
        // Dogfood fleet: say how to pick up the merged code; never run it.
        let exe = std::path::Path::new(&ctx.exe);
        let root = exe.parent().and_then(|b| b.parent()).unwrap_or(exe);
        writeln!(
            out,
            "cargo install --path {}/crates/cli --locked --root {} --force",
            f.local_path().display(),
            root.display()
        )?;
    }
    Ok(())
}

/// #130: a needs-human card's `override` / `extend` / `park`; #151: `herdsman extend|park` (`spawn: false` = `--no-spawn`).
pub fn needs_human(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, n): (&str, u64),
    op: &str,
    (spawn, by): (bool, &str),
) -> Result<()> {
    // Only from needs-human (#217: or a failed extend's card): a stale card or a double tap never parks a merged
    // issue or extends twice.
    let st = issue_state(ctx, fleet, n)?.map(|s| (s.state, s.last_event));
    let (state, last) = st.as_ref().map_or(("unknown", ""), |(s, l)| (s.as_str(), l.as_str()));
    if state != "needs-human" && last != "extend-failed" {
        return refused("NOT_NEEDS_HUMAN", format!("#{n} is {state}, not needs-human"));
    }
    let ev = |k| Event::new(ctx.now, fleet, Some(n), k, json!({"by": by}));
    match op {
        "override" => merge(ctx, cfg, r, out, fleet, n, true)?,
        "extend" => {
            // #217: the decision is logged first (the card goes at once); a failed spawn is drive's next due step.
            // Under drive.lock: a pass meanwhile would start the same round; a running pass leaves it to the next.
            let held = crate::drive::hold(ctx, r, fleet);
            if state == "needs-human" {
                let d = json!({"by": by, "spawn": spawn});
                crate::events::append(&ctx.state, &Event::new(ctx.now, fleet, Some(n), "round-extended", d))?;
            }
            if spawn && held.is_none() {
                writeln!(out, "#{n}: a drive pass is running; the next pass starts the round")?;
            } else if spawn {
                let o = crate::spawn::Opts {
                    supersede: true,
                    ..Default::default()
                };
                if let Err(e) = crate::spawn::spawn(ctx, cfg, r, out, (fleet, n, "impl"), &o) {
                    let d = json!({"by": by, "reason": format!("{e:#}")});
                    crate::events::append(&ctx.state, &Event::new(ctx.now, fleet, Some(n), "extend-failed", d))?;
                    return Err(e);
                }
            }
        }
        _ => {
            crate::events::append(&ctx.state, &ev("parked"))?;
            // #186: a parked issue's lanes are closed: reap what they left (a live agent's worktree is spared).
            if let Ok(a) = herdr::agents(r) {
                check::leftovers(ctx, cfg, r, &a, true)?;
            }
        }
    }
    writeln!(out, "#{n}: {op}")?;
    Ok(())
}
