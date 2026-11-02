//! #92 `herdsman drive`: one pass of the fleet loop, no AI; each action logs a passive `driven` event.
use crate::config::Config;
use crate::events::{append, emit, fold, read, Event, IssueState};
use crate::overlap::git;
use crate::runner::{ok, Runner};
use crate::{gh, host, queue, waiting, Ctx};
use anyhow::Result;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

/// A lock older than this belongs to a pass that died; the next pass takes it over.
const STALE_LOCK_SECS: u64 = 1800;

/// `<state dir>/drive-<fleet>.last`: the time of the fleet's last pass (the board's `last pass`).
pub fn last_pass(ctx: &Ctx, fleet: &str) -> Option<u64> {
    std::fs::read_to_string(state_dir(ctx).join(format!("drive-{fleet}.last")))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn state_dir(ctx: &Ctx) -> &Path {
    ctx.state.parent().unwrap_or(&ctx.state)
}

/// #140: a pass's lines as one toast, per action (`merged #1, #2 · started #3`), then `2 skipped (<codes>)`.
pub fn summary(fleet: &str, out: &str) -> String {
    if out.starts_with("drive: ") {
        return out.trim().into(); // another pass holds the lock
    }
    let (mut by, mut skip) = (BTreeMap::<&str, Vec<&str>>::new(), vec![]);
    for (head, res) in out.lines().filter_map(|l| l.split_once(": ")) {
        let (step, mut w) = (head.rsplit(' ').next().unwrap_or(head), res.split([' ', ':']));
        let past = PAST.iter().find(|d| d.starts_with(step)).unwrap_or(&step);
        let issue = head.split(' ').find(|n| n.starts_with('#'));
        match w.next().filter(|c| ["REFUSED", "exit", "error"].contains(c)) {
            Some(c) => skip.extend(if c == "REFUSED" { w.next() } else { Some(c) }),
            None => by.entry(past).or_default().extend(issue),
        }
    }
    let n = skip.len();
    skip.sort_unstable();
    skip.dedup();
    let mut parts: Vec<String> = by.iter().map(|(v, ns)| format!("{v} {}", ns.join(", "))).collect();
    parts.extend((n > 0).then(|| format!("{n} skipped ({})", skip.join(", "))));
    parts.extend(parts.is_empty().then(|| "nothing due".into()));
    // An action without an issue (clean) leaves a double space.
    format!("{fleet}: {}", parts.join(" · ").replace("  ", " ").trim_end())
}
const PAST: [&str; 3] = ["started", "merged", "reviewed"];

thread_local! {
    /// #166: this fleet's actions in the running pass, a silent repeat too: its `drive-pass` record.
    static ACTS: RefCell<Vec<Value>> = const { RefCell::new(vec![]) };
}

/// #166: a code in plain words (#141's table, here until it lands); the host's own reason follows a busy Mac.
fn words(code: Code, step: &str, result: &str) -> String {
    let why = result.split_once(": ").map_or(result, |p| p.1);
    let why = why.split("; retry later").next().unwrap_or(why);
    match code {
        None => PAST.iter().find(|d| d.starts_with(step)).map_or(step, |d| d).into(),
        Some("HOST_BUSY") => format!("Mac busy ({why})"),
        Some("DISK_LOW") => format!("disk nearly full ({why})"),
        Some("CAP" | "ALL_CAPPED") => "lane limit or usage cap reached".into(),
        Some("CI_NOT_GREEN") => "waiting for CI".into(),
        Some("SERIAL_BUSY") => "another one-at-a-time issue is in flight".into(),
        Some("LANE_LIVE") => "a lane is already on it".into(),
        Some("ROUND_LIMIT" | "UPDATE_LIMIT") => "out of rounds: needs you".into(),
        Some("EXIT" | "ERROR") => format!("failed: {result}"),
        Some(c) => format!("held back ({c})"),
    }
}

/// #166: the fleet's result line from a `drive-pass`: (tone, `HH:MM · started N · merged N · M waiting: <top reason>`).
/// Red on an error, green if it started or merged something, amber if everything was held back, else plain.
pub fn result(e: &Event) -> (&'static str, String) {
    let acts = e
        .data
        .get("actions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let n = |o: &str| acts.iter().filter(|a| a["outcome"] == o).count();
    let held: Vec<&str> = acts
        .iter()
        .filter(|a| a["outcome"] == "waiting")
        .filter_map(|a| a["words"].as_str())
        .collect();
    let top = held.iter().rev().max_by_key(|w| held.iter().filter(|x| x == w).count());
    let (started, merged, failed) = (n("started"), n("merged"), n("error"));
    let mut line = format!("{} · started {started} · merged {merged}", e.str("at").unwrap_or("?"));
    line += &top.map_or(String::new(), |w| format!(" · {} waiting: {w}", held.len()));
    if failed > 0 {
        line += &format!(" · {failed} failed");
    }
    let tone = match () {
        () if failed > 0 => "r",
        () if started + merged > 0 => "g",
        () if !held.is_empty() => "a",
        () => "m",
    };
    (tone, line)
}

/// #166: a pass is driving `fleet` now (the lock names the fleet a manual press drives; empty: every fleet).
pub fn running(ctx: &Ctx, fleet: &str) -> bool {
    std::fs::read_to_string(state_dir(ctx).join("drive.lock")).is_ok_and(|f| f.is_empty() || f == fleet)
}

/// A recorded action as `drive` prints it (`<fleet> #<n> <step>: <words>`, a refusal's code before its words).
fn line(fleet: &str, a: &Value) -> String {
    let who = a["issue"]
        .as_u64()
        .map_or(fleet.to_string(), |n| format!("{fleet} #{n}"));
    let (step, w) = (
        a["step"].as_str().unwrap_or_default(),
        a["words"].as_str().unwrap_or_default(),
    );
    match a["code"].as_str() {
        None => format!("{who} {step}: {w}"),
        Some("EXIT" | "ERROR") => format!("{who} {step}: error: {w}"),
        Some(c) => format!("{who} {step}: REFUSED {c}: {w}"),
    }
}

/// #166: a manual press that finds the lock held waits for that pass (at most 120 s), then prints its actions.
fn wait(ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write, lock: &Path, only: Option<&str>) -> Result<()> {
    for _ in 0..120 {
        if !lock.exists() {
            break;
        }
        r.sleep(1);
    }
    if lock.exists() {
        return Ok(writeln!(out, "drive: still driving · result will appear here")?);
    }
    let events = read(&ctx.state)?;
    let done = |e: &&Event| e.kind == "drive-pass" && only.is_none_or(|f| e.fleet == f);
    let ended = |e: &&Event| e.data.get("ended").and_then(Value::as_u64) >= Some(ctx.now);
    let Some(e) = events.iter().rev().filter(done).find(ended) else {
        return Ok(writeln!(out, "drive: the other pass ended without driving it")?);
    };
    let acts = e
        .data
        .get("actions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    acts.iter().try_for_each(|a| writeln!(out, "{}", line(&e.fleet, a)))?;
    Ok(())
}

/// One pass over every fleet (or `only`), under `<state dir>/drive.lock`; `manual`: a press.
pub fn drive(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    only: Option<&str>,
    manual: bool,
) -> Result<()> {
    only.map(|f| cfg.fleet(f)).transpose()?;
    std::fs::create_dir_all(state_dir(ctx))?;
    let _ = std::fs::write(ctx.state.with_file_name("drive.at"), ctx.now.to_string());
    let lock = state_dir(ctx).join("drive.lock");
    let age = |p: &Path| {
        p.metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
    };
    if age(&lock).is_some_and(|a| a.as_secs() > STALE_LOCK_SECS) {
        let _ = std::fs::remove_file(&lock);
    }
    let mine = std::fs::OpenOptions::new().write(true).create_new(true).open(&lock);
    if let Ok(mut f) = mine {
        let _ = f.write_all(only.unwrap_or_default().as_bytes());
    } else if manual {
        return wait(ctx, r, out, &lock, only);
    } else {
        return Ok(writeln!(
            out,
            "drive: another pass holds {}; nothing done",
            lock.display()
        )?);
    }
    let mine = |(n, f): &(&String, &crate::config::Fleet)| !f.paused && only.is_none_or(|o| o == n.as_str());
    // A paused fleet gets no pass (and no `last pass`); one fleet's error doesn't skip the others.
    let (mut res, mut start) = (Ok(()), true);
    for (name, _) in cfg.fleet.iter().filter(mine) {
        ACTS.with_borrow_mut(Vec::clear);
        let p = pass(ctx, cfg, r, out, name, start);
        if let Err(e) = &p {
            let w = format!("failed: {e:#}");
            ACTS.with_borrow_mut(|a| a.push(json!({"step": "pass", "outcome": "error", "code": "ERROR", "words": w})));
        }
        res = res.and(p.map(|more| start = more));
        let _ = std::fs::write(state_dir(ctx).join(format!("drive-{name}.last")), ctx.now.to_string());
        // #166: one `drive-pass` per fleet and pass, manual or automatic: the board's result line.
        let end = crate::now();
        let (by, at) = (
            if manual { "manual" } else { "auto" },
            crate::usage::date(r, end, "+%H:%M"),
        );
        let d = json!({"by": by, "started": ctx.now, "ended": end, "at": at, "actions": ACTS.take()});
        let _ = append(&ctx.state, &Event::new(ctx.now, name, None, "drive-pass", d));
    }
    let _ = std::fs::remove_file(&lock);
    res
}

/// One fleet: each item's due step, then start every startable queued issue in order, then clean merged lanes.
/// Each spawn samples the host afresh; a refused item is skipped, `CAP` ends starts, `HOST_BUSY`/`ALL_CAPPED` all fleets'.
fn pass(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str, start: bool) -> Result<bool> {
    let events = read(&ctx.state)?;
    let agents = host::agents(r)?;
    let f = cfg.fleet(fleet)?;
    let mut waits = waiting::scan(ctx, cfg, r, &events, &agents, true);
    // #129: ready PRs that conflict with nothing merge first; of a conflicting pair the older, the other is updated.
    let conflicts = crate::overlap::preview(ctx, cfg, r, fleet, &events).unwrap_or_default();
    waits.sort_by_key(|(s, _)| conflicts.contains_key(&s.issue));
    for (s, w) in waits
        .iter()
        .filter(|(s, w)| s.fleet == fleet && s.lane.as_ref() == Some(&w.lane))
    {
        let pr = gh::cached_pr(ctx, fleet, s.issue);
        let red = gh::ci(&gh::required(&pr["statusCheckRollup"], &f.required_checks)) == "failed";
        let n = s.issue.to_string();
        // #149: an unmarked open PR touching a serial path is marked serial here, not refused at merge.
        if pr["state"] == "OPEN" && !f.serial_paths.is_empty() && !queue::serial(&events, fleet).contains(&s.issue) {
            let (num, head) = (pr["number"].to_string(), pr["headRefOid"].as_str().unwrap_or_default());
            let files = gh::diff_names(ctx, r, f, (fleet, s.issue), &num, head).unwrap_or_default();
            if let Some(p) = files.into_iter().find(|p| f.serial_path(p)) {
                writeln!(out, "{fleet} #{n} serial-auto: {p}")?;
                let e = Event::new(ctx.now, fleet, Some(s.issue), "serial-auto", json!({"path": p}));
                emit(&ctx.state, cfg, e)?;
            }
        }
        if let Err(e) = crate::gate::sync_label(ctx, cfg, r, out, s, &pr) {
            writeln!(out, "{fleet} #{n} label: {e:#}")?;
        }
        // #148: a head auto-updated this pass has no CI yet; its merge waits for the next pass.
        let now = |e: &Event| e.is("auto-updated", fleet, s.issue) && e.ts == ctx.now;
        if let Some(e) = read(&ctx.state)?.into_iter().rev().find(now) {
            let to = e.str("to").unwrap_or_default();
            writeln!(out, "{fleet} #{n} ready (CI pending at {to:.7})")?;
        } else if w.who == "drive" {
            // `waiting` names the exact next command; drive runs it.
            let Some(cmd) = &w.next_cmd else { continue };
            let args: Vec<&str> = cmd.split_whitespace().skip(1).collect();
            let step = match (args[0], args.get(3).copied()) {
                ("spawn", Some("rev")) => "review",
                ("spawn", _) if cmd.ends_with("--update") => "conflict",
                ("spawn", _) if cmd.ends_with("--supersede") => "fix",
                ("spawn", _) => "respawn",
                (v, _) => v,
            };
            let code = act(ctx, cfg, r, out, &events, (fleet, Some(s.issue), step), &args)?;
            if step == "merge" && code.is_none() {
                if let Err(e) = auto_update(ctx, cfg, r, out, fleet) {
                    writeln!(out, "{fleet} auto-update: {e:#}")?;
                }
            }
        } else if s.state == "ready" && red {
            // Red CI after a PASS is a FAIL at the same head (no new round), the failed checks as the finding.
            let head = pr["headRefOid"].as_str().unwrap_or_default();
            let file = state_dir(ctx).join(format!("ci-{fleet}-{n}.md"));
            std::fs::write(&file, ci_findings(r, f, &pr, head))?;
            let file = file.to_string_lossy();
            let args = ["verdict", fleet, &n, "FAIL", "--sha", head, "--file", &file];
            act(ctx, cfg, r, out, &events, (fleet, Some(s.issue), "ci-red"), &args)?;
        }
    }
    let mut go = start;
    for n in queue::startable(&events, fleet).into_iter().take_while(|_| start) {
        let at = (fleet, n.parse().ok(), "start");
        let code = act(ctx, cfg, r, out, &events, at, &["spawn", fleet, &n, "impl"])?;
        go &= !matches!(code, Some("HOST_BUSY" | "DISK_LOW" | "ALL_CAPPED"));
        if matches!(code, Some("HOST_BUSY" | "DISK_LOW" | "CAP" | "ALL_CAPPED")) {
            break;
        }
    }
    let st = fold(&events);
    let merged = |e: &&Event| {
        e.issue
            .is_some_and(|n| st.get(&(fleet.into(), n)).is_some_and(|s| s.state == "merged"))
    };
    let left = |e: &&Event| e.str("worktree").is_some_and(|w| Path::new(w).exists());
    if events
        .iter()
        .filter(|e| e.kind == "spawn" && e.fleet == fleet)
        .filter(merged)
        .any(|e| left(&e))
    {
        act(
            ctx,
            cfg,
            r,
            out,
            &events,
            (fleet, None, "clean"),
            &["clean", fleet, "--yes"],
        )?;
    }
    Ok(go)
}

/// #129: after a merge, merge integration into every other open ready / in-review PR whose lane isn't working, oldest
/// PR first, in a scratch worktree. A clean merge is pushed (`auto-updated`); a ready PR's conflict gets an `--update` lane.
pub fn auto_update(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<()> {
    let (f, events, ag) = (
        cfg.fleet(fleet)?,
        read(&ctx.state)?,
        host::agents(r).unwrap_or_default(),
    );
    let busy = |l: &str| ag.iter().any(|a| a["name"] == l && a["agent_status"] == "working");
    let open = |s: &IssueState| {
        let pr = gh::cached_pr(ctx, fleet, s.issue);
        pr["number"].as_u64().filter(|_| pr["state"] == "OPEN")
    };
    let mut prs: Vec<(u64, IssueState)> = (fold(&events).into_values())
        .filter(|s| s.fleet == fleet && (s.state == "ready" || s.state.starts_with("review")))
        .filter(|s| !s.lane.as_deref().is_some_and(busy))
        .filter_map(|s| Some((open(&s)?, s)))
        .collect();
    prs.sort_by_key(|p| p.0);
    let (dir, wt) = (f.local_path(), ctx.state.with_file_name(format!("update-{fleet}")));
    let (wts, into) = (wt.to_string_lossy(), format!("origin/{}", f.integration));
    git(r, &dir, &["fetch", "-q", "origin"])?;
    // A scratch `git worktree`, not a herdr one: no pane, no lane.
    let tree = |a: &[&str]| git(r, &dir, &[&["worktree"][..], a].concat());
    for (_, s) in prs {
        let (n, branch) = (s.issue.to_string(), crate::gate::branch_of(Some(&s), s.issue));
        let from = git(r, &dir, &["rev-parse", &format!("origin/{branch}")])?;
        let _ = tree(&["remove", "--force", &wts]);
        tree(&["add", "-f", "--detach", &wts, &from])?;
        let clean = git(r, &wt, &["merge", "--no-edit", &into]).is_ok();
        let to = git(r, &wt, &["rev-parse", "HEAD"])?;
        let res = if !clean && s.state == "ready" {
            let args = ["spawn", fleet, &n, "impl", "--supersede", "--update"];
            act(ctx, cfg, r, out, &events, (fleet, Some(s.issue), "conflict"), &args).map(drop)
        } else if clean && to != from {
            git(r, &wt, &["push", "-q", "origin", &format!("HEAD:{branch}")]).and_then(|_| {
                gh::forget_pr(ctx, fleet, s.issue);
                writeln!(out, "{fleet} #{n} auto-updated: {from:.7} → {to:.7}")?;
                let d = json!({"from": from, "to": to});
                emit(
                    &ctx.state,
                    cfg,
                    Event::new(ctx.now, fleet, Some(s.issue), "auto-updated", d),
                )
            })
        } else {
            Ok(())
        };
        let _ = tree(&["remove", "--force", &wts]);
        res?;
    }
    Ok(())
}

/// The finding for red CI: the first failing check, then the tail of its run's failed-step log.
fn ci_findings(r: &dyn Runner, f: &crate::config::Fleet, pr: &serde_json::Value, head: &str) -> String {
    let checks = gh::required(&pr["statusCheckRollup"], &f.required_checks);
    let failed = checks
        .as_array()
        .into_iter()
        .flatten()
        .find(|c| gh::check_state(c) == "failed");
    let url = failed.and_then(|c| c["detailsUrl"].as_str()).unwrap_or_default();
    let run = url
        .split("/runs/")
        .nth(1)
        .and_then(|t| t.split('/').next())
        .unwrap_or_default();
    let log = (!run.is_empty()).then(|| ok(r, "gh", &["run", "view", run, "-R", &f.repo, "--log-failed"]).ok());
    let log = log.flatten().unwrap_or_default();
    let tail: Vec<&str> = log
        .lines()
        .rev()
        .take(40)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let what = gh::not_green(&checks).unwrap_or_else(|| "a check failed".into());
    // The tail lands in events and the fix brief: mask secret-looking runs first.
    format!(
        "FAIL\n\nCI is red at {head}: {what}.\n\n```\n{}\n```\n",
        crate::lanes::mask(&tail.join("\n"))
    )
}

/// (fleet, issue, step) of one action.
type At<'a> = (&'a str, Option<u64>, &'a str);
/// A refused or failed action's code (`REFUSED <code>`, `EXIT`, `ERROR`); `None` when it ran.
type Code = Option<&'static str>;

/// Run one CLI command in process (its own gates and refusals apply); print one line, log `driven`.
/// A refusal or error is logged once per item, step and outcome per hour, and retried next pass; returns its code.
fn act(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, ev: &[Event], at: At, c: &[&str]) -> Result<Code> {
    let (fleet, issue, step) = at;
    let mut buf = vec![];
    let res = crate::cli::run(c, ctx, r, &mut buf);
    let text = String::from_utf8_lossy(&buf);
    let (result, code) = match res {
        Ok(0) => (text.lines().next().unwrap_or("ok").to_string(), None),
        Ok(c) => (
            format!("exit {c}: {}", text.lines().next().unwrap_or_default()),
            Some("EXIT"),
        ),
        Err(e) => match e.downcast_ref::<crate::gate::Refusal>() {
            Some(rf) => (rf.to_string(), Some(rf.code)),
            None => (format!("error: {e:#}"), Some("ERROR")),
        },
    };
    // `clean` with nothing removable (a dirty merged lane) is not an action.
    if step == "clean" && code.is_none() && !text.contains("removed ") {
        return Ok(None);
    }
    let outcome = match code {
        None => PAST.iter().find(|d| d.starts_with(step)).map_or(step, |d| d),
        Some("EXIT" | "ERROR") => "error",
        Some(_) => "waiting",
    };
    let w = words(code, step, &result);
    ACTS.with_borrow_mut(|a| {
        a.push(json!({"issue": issue, "step": step, "outcome": outcome, "code": code, "words": w}));
    });
    let recent = |e: &Event| e.kind == "driven" && e.fleet == fleet && e.issue == issue && ctx.now < e.ts + 3600;
    let same = |e: &Event| e.str("step") == Some(step) && e.str("code") == code;
    // A repeated refusal is silent too (no cron mail every pass).
    if code.is_some() && ev.iter().any(|e| recent(e) && same(e)) {
        return Ok(code);
    }
    let who = issue.map_or(fleet.to_string(), |n| format!("{fleet} #{n}"));
    writeln!(out, "{who} {step}: {result}")?;
    let d = json!({"step": step, "cmd": format!("herdsman {}", c.join(" ")), "result": result, "code": code});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, issue, "driven", d)).map(|()| code)
}
