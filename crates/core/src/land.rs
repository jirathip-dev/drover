use crate::config::{glob, Config, Fleet};
use crate::events::{emit, Event};
use crate::overlap::git;
use crate::runner::{json, ok, Runner};
use crate::{gh, Ctx};
use anyhow::Result;
use serde_json::{json, Value};
use std::io::Write;

/// The fleet's open pause: its `land-paused` when no `land-resumed` came after it.
pub fn paused<'a>(events: &'a [Event], fleet: &str) -> Option<&'a Event> {
    let mine = |e: &&Event| e.fleet == fleet && matches!(e.kind.as_str(), "land-paused" | "land-resumed");
    events.iter().rfind(mine).filter(|e| e.kind == "land-paused")
}

/// Mark `pr` `landing: fast-first` when its fleet lands fast-first, landings are not paused and its diff at the head (read with `r`, else the cached one) touches no `full_first_paths`.
pub fn mark(ctx: &Ctx, r: Option<&dyn Runner>, f: &Fleet, (fleet, n): (&str, u64), ev: &[Event], pr: &mut Value) {
    let Some(l) = f.fast_first() else { return };
    let head = pr["headRefOid"].as_str().unwrap_or_default().to_string();
    if head.is_empty() || pr["state"] != "OPEN" || paused(ev, fleet).is_some() {
        return;
    }
    let files = match r {
        Some(r) => gh::diff_names(ctx, r, f, (fleet, n), &pr["number"].to_string(), &head).ok(),
        None => crate::usage::load::<Value>(&gh::cache_path(ctx, "diff", fleet, n))
            .filter(|c| c["head"] == head.as_str())
            .and_then(|c| serde_json::from_value::<Vec<String>>(c["files"].clone()).ok()),
    };
    let full = |p: &String| l.full_first_paths.iter().any(|g| glob(g.as_bytes(), p.as_bytes()));
    if files.is_some_and(|fs| !fs.iter().any(full)) {
        pr["landing"] = json!("fast-first");
    }
}

/// The board's word on #`n`'s landing: `landed (fast) · full suite running` until integration is green after it, `reverted: <job> red` once reverted.
pub fn note(events: &[Event], fleet: &str, n: u64) -> Option<String> {
    let at = |e: &Event| e.fleet == fleet && e.issue == Some(n) && matches!(e.kind.as_str(), "landed" | "reverted");
    let i = events.iter().rposition(at)?;
    let e = &events[i];
    if e.kind == "reverted" {
        return Some(format!("reverted: {} red", e.str("red_job").unwrap_or("?")));
    }
    let green = events[i..].iter().any(|x| x.fleet == fleet && x.kind == "land-green");
    (e.str("mode") == Some("fast-first") && !green).then(|| "landed (fast) · full suite running".into())
}

/// Swap-if-unchanged: integration's tip is in `head`, so the fast checks ran against what it lands on. Only the head is pinned (`--match-head-commit`): integration moving between this check and the merge is a small window left open.
pub fn based(r: &dyn Runner, f: &Fleet, head: &str) -> bool {
    let (dir, tip) = (f.local_path(), format!("origin/{}", f.integration));
    git(r, &dir, &["fetch", "-q", "origin"]).is_ok()
        && git(r, &dir, &["merge-base", "--is-ancestor", &tip, head]).is_ok()
}

pub(crate) fn suite(r: &dyn Runner, f: &Fleet, sha: &str) -> (&'static str, Value) {
    let jq = "[.check_runs[] | {__typename: \"CheckRun\", name, status: (.status | ascii_upcase), conclusion: \
        ((.conclusion // \"\") | ascii_upcase), databaseId: .id, startedAt: .started_at, completedAt: .completed_at, \
        detailsUrl: .details_url}]";
    let api = format!("repos/{}/commits/{sha}/check-runs?per_page=100", f.repo);
    let all = json(r, "gh", &["api", &api, "--jq", jq]).unwrap_or(json!([]));
    let c = gh::required(&all, &f.required_checks);
    let none = all.as_array().is_none_or(Vec::is_empty);
    ([gh::ci(&c), "none"][usize::from(none)], c)
}

fn red(checks: &Value) -> (String, String, bool) {
    let failed: Vec<&Value> = (checks.as_array().into_iter().flatten())
        .filter(|c| gh::check_state(c) == "failed")
        .collect();
    let first = failed.first().copied().unwrap_or(&Value::Null);
    let url = first["detailsUrl"].as_str().unwrap_or_default();
    let run = url.split("/runs/").nth(1).and_then(|t| t.split('/').next());
    let infra = !failed.is_empty() && failed.iter().all(|c| c["conclusion"] != "FAILURE");
    (gh::check_name(first).into(), run.unwrap_or_default().into(), infra)
}

/// One watch over a fast-first fleet's integration branch (each drive step): an open revert lands once its fast checks are green; then the tip's full suite: green logs `land-green` (and resumes), red pauses and goes after its culprit once.
pub fn watch(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<()> {
    let f = cfg.fleet(fleet)?;
    let Some(l) = f.fast_first() else { return Ok(()) };
    let ev = crate::events::read(&ctx.state)?;
    let mine = |k: &'static str| move |e: &&Event| e.fleet == fleet && e.kind == k;
    let log = |n: Option<u64>, k: &str, d: Value| emit(&ctx.state, cfg, Event::new(ctx.now, fleet, n, k, d));
    let done = |o: &&Event| {
        ev.iter()
            .filter(mine("reverted"))
            .any(|e| e.str("culprit") == o.str("culprit"))
    };
    for o in ev.iter().filter(mine("revert-opened")).filter(|o| !done(o)) {
        let num = o.str("pr").unwrap_or_default();
        let fields = "headRefOid,statusCheckRollup,state";
        let pr = json(r, "gh", &["pr", "view", num, "-R", &f.repo, "--json", fields])?;
        let head = pr["headRefOid"].as_str().unwrap_or_default();
        let st = gh::ci(&gh::required(&pr["statusCheckRollup"], &l.fast));
        if pr["state"] != "OPEN" || st != "green" {
            let told = |e: &Event| mine("revert-failed")(&e) && e.str("pr") == Some(num);
            if pr["state"] == "OPEN" && st == "failed" && !ev.iter().any(told) {
                writeln!(out, "{fleet} revert PR #{num}: fast checks red")?;
                let mut d = Value::Object(o.data.clone());
                d["reason"] = json!(format!("revert PR #{num}: fast checks red"));
                log(o.issue, "revert-failed", d)?;
            }
            continue;
        }
        let a = format!("pr merge {num} -R {} --merge --match-head-commit {head}", f.repo);
        ok(r, "gh", &a.split(' ').collect::<Vec<_>>())?;
        let how =
            format!("Its landing was reverted by PR #{num}: bring the change back (revert that revert) with the fix.");
        reopen(ctx, cfg, r, out, (f, fleet), o, ("reverted", &how))?;
    }
    let green = ev.iter().rposition(|e| mine("land-green")(&e)).map_or(0, |i| i + 1);
    let since: Vec<&Event> = ev[green..].iter().filter(mine("landed")).collect();
    let pause = paused(&ev, fleet);
    if since.is_empty() && pause.is_none() {
        return Ok(());
    }
    let api = format!("repos/{}/branches/{}", f.repo, f.integration);
    let tip = ok(r, "gh", &["api", &api, "--jq", ".commit.sha"])?.trim().to_string();
    let (st, checks) = suite(r, f, &tip);
    if st == "green" {
        log(None, "land-green", json!({"sha": tip}))?;
        if pause.is_some() {
            writeln!(out, "{fleet} landing resumed: {} green @ {tip:.7}", f.integration)?;
            log(None, "land-resumed", json!({"sha": tip}))?;
        }
    }
    if st != "failed" {
        return Ok(());
    }
    let (job, run, infra) = red(&checks);
    if pause.is_none() {
        let again = |e: &Event| mine("land-rerun")(&e) && e.str("sha") == Some(&tip);
        if infra && !ev.iter().any(again) {
            let _ = ok(r, "gh", &["run", "rerun", &run, "-R", &f.repo, "--failed"]);
            writeln!(
                out,
                "{fleet} {} red @ {tip:.7} ({job}): runner/infra, re-run once",
                f.integration
            )?;
            return log(None, "land-rerun", json!({"sha": tip, "job": job}));
        }
        let reason = format!("{} red @ {tip:.7}: {job}; fast-first landings paused", f.integration);
        writeln!(out, "{fleet} {reason}")?;
        let at = crate::usage::date(r, ctx.now, "+%H:%M");
        log(
            None,
            "land-paused",
            json!({"red_sha": tip, "job": job, "at": at, "reason": reason}),
        )?;
    }
    let Some(c) = culprit(ctx, cfg, r, (f, fleet), &ev, &since, &run)? else {
        return Ok(());
    };
    let sha = c.str("sha").unwrap_or_default();
    let met = |e: &Event| e.fleet == fleet && e.str("culprit") == Some(sha);
    if ev.iter().any(met) {
        return Ok(());
    }
    let mut d = json!({"culprit": sha, "landed_pr": c.str("pr"), "red_sha": tip, "red_job": job, "run": run});
    if c.str("mode") != Some("fast-first") {
        let how = "It landed full-first, so it was not reverted: fix it forward.";
        let o = Event::new(ctx.now, fleet, c.issue, "land-fix", d);
        return reopen(ctx, cfg, r, out, (f, fleet), &o, ("land-fix", how));
    }
    let n = c.issue.unwrap_or_default();
    let (dir, wt) = (f.local_path(), ctx.state.with_file_name(format!("revert-{fleet}")));
    let (wts, branch) = (wt.to_string_lossy(), format!("revert-{n}-{sha:.7}"));
    let tree = |a: &[&str]| git(r, &dir, &[&["worktree"][..], a].concat());
    git(r, &dir, &["fetch", "-q", "origin"])?;
    let _ = tree(&["remove", "--force", &wts]);
    tree(&["add", "-f", "--detach", &wts, &format!("origin/{}", f.integration)])?;
    let pushed = git(r, &wt, &["revert", "-m", "1", "--no-edit", sha]).and_then(|_| {
        git(
            r,
            &wt,
            &["push", "-q", "-f", "origin", &format!("HEAD:refs/heads/{branch}")],
        )
    });
    let _ = tree(&["remove", "--force", &wts]);
    if let Err(e) = pushed {
        writeln!(out, "{fleet} #{n} revert of {sha:.7} failed: {e:#}")?;
        (d["reason"], d["error"]) = (json!(format!("revert of {sha:.7} failed")), json!(format!("{e:#}")));
        return log(Some(n), "revert-failed", d);
    }
    let title = format!("Revert #{n}: `{job}` red on {}", f.integration);
    let body = format!(
        "Fast-first landing (#350): the full suite went red at {tip:.7} (`{job}`); this reverts #{n}'s landing {sha:.7}."
    );
    let a = [
        "--base",
        &f.integration,
        "--head",
        &branch,
        "--title",
        &title,
        "--body",
        &body,
    ];
    let url = ok(r, "gh", &[&["pr", "create", "-R", &f.repo][..], &a].concat())?;
    let num = url.trim().rsplit('/').next().unwrap_or_default().to_string();
    writeln!(out, "{fleet} #{n} revert PR #{num} opened: {job} red after {sha:.7}")?;
    (d["pr"], d["reason"]) = (json!(num), json!(format!("revert PR #{num} for {job} red")));
    log(Some(n), "revert-opened", d)
}

fn culprit<'a>(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    (f, fleet): (&Fleet, &str),
    ev: &[Event],
    since: &[&'a Event],
    run: &str,
) -> Result<Option<&'a Event>> {
    let fast: Vec<&'a Event> = (since.iter().copied())
        .filter(|e| e.str("mode") == Some("fast-first"))
        .collect();
    if fast.len() == 1 || since.len() < 2 {
        return Ok(fast.first().or(since.first()).copied());
    }
    for c in since {
        let sha = c.str("sha").unwrap_or_default();
        let at = |e: &&Event| e.fleet == fleet && e.kind == "land-bisect" && e.str("sha") == Some(sha);
        let bisect = ev.iter().rfind(at);
        let (mut st, checks) = suite(r, f, sha);
        if let Some(b) = bisect {
            let since_b = |c: &&Value| c["startedAt"].as_str().and_then(crate::usage::epoch) >= Some(b.ts);
            let fresh: Vec<Value> = (checks.as_array().into_iter().flatten())
                .filter(since_b)
                .cloned()
                .collect();
            st = if fresh.is_empty() {
                "pending"
            } else {
                gh::ci(&json!(fresh))
            };
        }
        match st {
            "green" => continue,
            "failed" if bisect.is_some() || !red(&checks).2 => return Ok(Some(c)),
            "pending" => return Ok(None),
            _ => {}
        }
        let tmp = format!("drover-bisect-{sha:.7}");
        let wf = json(r, "gh", &["run", "view", run, "-R", &f.repo, "--json", "workflowName"]).unwrap_or_default();
        let wf = wf["workflowName"].as_str().unwrap_or_default();
        let push = ["push", "-q", "-f", "origin", &format!("{sha}:refs/heads/{tmp}")];
        let sent = git(r, &f.local_path(), &push).and_then(|_| {
            anyhow::ensure!(!wf.is_empty(), "no workflow for run {run}");
            ok(r, "gh", &["workflow", "run", wf, "-R", &f.repo, "--ref", &tmp])
        });
        let told = |e: &Event| e.fleet == fleet && e.kind == "revert-failed" && e.str("sha") == Some(sha);
        let d = match sent {
            Ok(_) => (json!({"sha": sha, "ref": tmp}), "land-bisect"),
            Err(e) if !ev.iter().any(told) => {
                let reason = format!("bisect run for {sha:.7} did not start");
                (
                    json!({"sha": sha, "reason": reason, "error": format!("{e:#}")}),
                    "revert-failed",
                )
            }
            Err(_) => return Ok(None),
        };
        emit(&ctx.state, cfg, Event::new(ctx.now, fleet, c.issue, d.1, d.0))?;
        return Ok(None);
    }
    Ok(since.last().copied())
}

fn reopen(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (f, fleet): (&Fleet, &str),
    o: &Event,
    (kind, how): (&str, &str),
) -> Result<()> {
    let n = o.issue.unwrap_or_default();
    let s = |k| o.str(k).unwrap_or_default();
    let log = ok(r, "gh", &["run", "view", s("run"), "-R", &f.repo, "--log-failed"]).unwrap_or_default();
    let tail: Vec<&str> = log.lines().rev().take(40).collect();
    let tail: Vec<&str> = tail.into_iter().rev().collect();
    let mut d = Value::Object(o.data.clone());
    d["reason"] = json!(format!("{} red after #{n} landed: {}", f.integration, s("red_job")));
    writeln!(out, "{fleet} #{n} {kind}: {}", d["reason"].as_str().unwrap_or_default())?;
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), kind, d))?;
    let body = format!(
        "FAIL\n\n{} went red at {:.7} after this issue landed ({:.7}): `{}`. {how}\n\n```\n{}\n```\n",
        f.integration,
        s("red_sha"),
        s("culprit"),
        s("red_job"),
        crate::lanes::mask(&tail.join("\n"))
    );
    let p = f.flow();
    let d = json!({"verdict": "FAIL", "sha": s("culprit"), "stage": p.gate(), "loop": p.loop_of(p.gate()),
        "body": body, "by": "drive"});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), "verdict", d))?;
    let _ = ok(r, "gh", &["issue", "reopen", &n.to_string(), "-R", &f.repo]);
    gh::forget_pr(ctx, fleet, n);
    Ok(())
}

/// `drover stats land <fleet>`: per mode, the median PASS → landed time, landings per day, reverts (fixes for full-first) and the median time the gating checks took; then the minutes integration sat red (paused).
pub fn stats(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, fleet: &str) -> Result<()> {
    cfg.fleet(fleet)?;
    let ev = crate::events::history(&ctx.state)?;
    let ev: Vec<&Event> = ev.iter().filter(|e| e.fleet == fleet).collect();
    let median = |mut v: Vec<u64>| {
        v.sort_unstable();
        v.get(v.len() / 2).map_or("-".into(), |m| crate::events::age(*m))
    };
    for (mode, back) in [("fast-first", "reverted"), ("full-first", "land-fix")] {
        let lands: Vec<&&Event> = ev
            .iter()
            .filter(|e| e.kind == "landed" && e.str("mode") == Some(mode))
            .collect();
        let pass = |l: &Event| {
            let v = ev
                .iter()
                .rev()
                .find(|e| e.ts <= l.ts && e.issue == l.issue && e.str("verdict") == Some("PASS"));
            v.map(|v| l.ts - v.ts)
        };
        let span = lands.first().map_or(1, |l| ctx.now.saturating_sub(l.ts).max(86_400));
        let day = lands.len() as u64 * 864_000 / span;
        let secs = lands
            .iter()
            .filter_map(|l| l.data.get("checks_secs").and_then(Value::as_u64));
        writeln!(
            out,
            "{fleet} {mode}: {} landed · {}.{}/day · median PASS → landed {} · {} {back} · median checks {}",
            lands.len(),
            day / 10,
            day % 10,
            median(lands.iter().filter_map(|l| pass(l)).collect()),
            ev.iter().filter(|e| e.kind == back).count(),
            median(secs.collect()),
        )?;
    }
    let mut red = 0;
    for (i, p) in ev.iter().enumerate().filter(|(_, e)| e.kind == "land-paused") {
        let end = ev[i..]
            .iter()
            .find(|e| e.kind == "land-resumed")
            .map_or(ctx.now, |e| e.ts);
        red += end.saturating_sub(p.ts);
    }
    writeln!(out, "{fleet} integration red (landings paused): {} min", red / 60)?;
    Ok(())
}
