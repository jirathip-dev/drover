//! `herdsman check` (alerts, each once per state) and `herdsman clean` (reclaim merged or closed lanes).

use crate::config::Config;
use crate::events::{age, emit, fold, last_spawn, open_asks, plain, read, Event, IssueState};
use crate::gate::gate;
use crate::host;
use crate::hosts::On;
use crate::runner::{ok, Runner};
use crate::spawn::{changes, dirty, OWN_FILES};
use crate::Ctx;
use crate::{gh, herdr};
use anyhow::{Context, Result};
use serde_json::json;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

/// Head sha of an open PR that passes every merge gate, if any.
fn mergeable(ctx: &Ctx, r: &dyn Runner, cfg: &Config, s: &IssueState) -> Option<String> {
    let f = cfg.fleet(&s.fleet).ok()?;
    let branch = crate::gate::branch_of(Some(s), s.issue);
    let pr = gh::pr(ctx, r, f, (&s.fleet, s.issue), &branch, ctx.now).ok()?;
    (pr["state"] == "OPEN").then_some(())?;
    gate(f, &pr, Some(s), false).ok()
}

/// `(url, prompt)` when the issue's latest amendment is missing from the lane's `.brief.md` (#101).
fn amendment(ctx: &Ctx, on: &On, cfg: &Config, s: &IssueState, wt: &str) -> Option<(String, String)> {
    let (f, r) = (cfg.fleet(&s.fleet).ok()?, on.r);
    let brief = on.read(&Path::new(wt).join(".brief.md"))?;
    let spec = gh::spec(ctx, r, f, &s.fleet, s.issue, true).ok()?;
    let url = spec["amended"].as_str().filter(|u| !brief.contains(*u))?;
    let n = s.issue;
    let fleet = &s.fleet;
    let line = format!("AMENDMENT on #{n} ({url}): run `herdsman brief {fleet} {n}` and apply it");
    Some((url.to_string(), line))
}

/// Each alert's dedup key is built from the issue's last activity: once per state, again after a change.
pub fn check(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write) -> Result<()> {
    let events = read(&ctx.state)?;
    PUSHES.set((0, 0));
    let _ = std::fs::write(ctx.state.with_file_name("check.at"), ctx.now.to_string());
    let agents = host::agents(r)
        .map_err(|e| eprintln!("herdsman check: herdr agent list failed ({e:#}); skipping lane checks"))
        .ok();
    if let Some(a) = &agents {
        crate::conductor::sleep_idle(ctx, cfg, r, &events, a)?; // #355
    }
    let stall = cfg.limits.stall_minutes * 60;
    // #157: each host's agents; a lane on an unreachable host is skipped (it never pages).
    let remote = crate::hosts::survey(ctx, cfg, r, false);
    crate::cloud::tick(ctx, cfg, r, &remote, out)?;
    crate::setup::reap(ctx, r, &events)?;
    for (name, f) in &cfg.fleet {
        let _ = crate::overlap::preview(ctx, cfg, r, name, &events);
        // #331: a queued issue never spawned had no cached spec, so no Queue row title: fetch it once (cached after).
        for i in crate::queue::replay(&events, name) {
            let _ = gh::spec(ctx, r, f, name, i.issue, false);
        }
    }
    // #333: a crashed kick's marker or lock goes on the next check; an issue SPAWN_PENDING has held back in every
    // pass for 15 min alerts once per episode (its fleet's earliest hold is the key).
    crate::drive::markers(ctx, cfg, r, true);
    let mut held = BTreeMap::<(&str, u64), (u64, u64)>::new();
    for e in events.iter().filter(|e| e.kind == "drive-pass") {
        let acts = e.data.get("actions").and_then(serde_json::Value::as_array);
        for a in acts.into_iter().flatten() {
            let Some(n) = a["issue"].as_u64() else { continue };
            if a["code"] == "SPAWN_PENDING" {
                held.entry((&e.fleet, n)).or_insert((e.ts, e.ts)).1 = e.ts;
            } else {
                held.remove(&(e.fleet.as_str(), n));
            }
        }
    }
    held.retain(|_, (since, last)| ctx.now >= *since + 900 && ctx.now < *last + 900);
    for name in cfg.fleet.keys() {
        let mine: Vec<_> = held.iter().filter(|h| h.0 .0 == name).collect();
        let Some(since) = mine.iter().map(|h| h.1 .0).min() else {
            continue;
        };
        let ns: Vec<String> = mine.iter().map(|h| format!("#{}", h.0 .1)).collect();
        let why = format!(
            "{} held back by SPAWN_PENDING for {}",
            ns.join(", "),
            age(ctx.now - since)
        );
        let d = json!({"key": since, "reason": why});
        fire(ctx, cfg, out, &events, (name, None), "spawn-held", d)?;
    }
    for s in fold(&events).values().filter(|s| !s.done()) {
        let idle = ctx.now.saturating_sub(s.last_ts);
        let mut hits = vec![];
        let mut capped = None;
        if let (Some(agents), Some((role, lane))) = (&agents, s.active_lane()) {
            // Agents that report their own state (dsh-TUI) can lose the name: match the lane's pane.
            let spawned = last_spawn(&events, lane);
            let r = &On::of(r, spawned);
            let there = spawned.filter(|_| r.machine.is_some()).and_then(|e| e.str("host"));
            let theirs = there.map(|h| remote.get(h).cloned().flatten());
            let Some(agents) = theirs.as_ref().map_or(Some(agents), Option::as_ref) else {
                continue;
            };
            let pane = spawned.and_then(|e| e.str("pane"));
            let unnamed =
                |a: &&serde_json::Value| a["name"].as_str().is_none_or(str::is_empty) && a["pane_id"].as_str() == pane;
            let agent = agents
                .iter()
                .find(|a| a["name"] == lane)
                .or_else(|| agents.iter().find(unnamed));
            let status = agent.map(|a| a["agent_status"].as_str().unwrap_or("unknown"));
            // #83: a gone lane with a clean, pushed worktree is parked: one `lane-dead`, never a stall.
            let wt = spawned.and_then(|e| e.str("worktree"));
            let parked = status.is_none() && wt.is_some_and(|wt| dirty(r, Path::new(wt)).is_none());
            let key = if parked {
                format!("parked:{lane}")
            } else {
                format!("{}:{lane}", s.last_ts)
            };
            // #248: a lane herdsman closed once its handoff was logged is not gone.
            let mine = |e: &Event| e.kind == "lane-closed" && e.str("agent") == Some(lane);
            let closed = spawned.is_some_and(|sp| events.iter().any(|e| mine(e) && e.ts >= sp.ts));
            if status.is_none() && !closed {
                hits.push((
                    "lane-dead",
                    key.clone(),
                    format!("{role} lane {lane} is gone ({})", s.state),
                ));
            }
            // A lane on a named account that shows its harness's usage-limit banner caps that account.
            // #302: with a `fallback` chain, any lane's limit or sign-out rests its account (else its harness).
            let account = spawned.and_then(|e| e.str("account"));
            let chain = cfg
                .fleet(&s.fleet)
                .is_ok_and(|f| f.role.get(role).is_some_and(|r| !r.fallback.is_empty()));
            if let (Some(p), Some(sp), true) = (pane, spawned, status.is_some() && (account.is_some() || chain)) {
                let screen = herdr::read(r, p).unwrap_or_default();
                let name = account.or(sp.str("harness")).unwrap_or_default();
                let (harness, host) = (sp.str("harness"), sp.str("host"));
                let rest = |until| json!({"agent": lane, "harness": harness, "account": account, "host": host, "until": until});
                let h = crate::harness::get(harness.unwrap_or_default());
                if let Some(until) = h.usage_limited(cfg, r, &screen, ctx.now) {
                    let why = format!("{name}: {role} lane {lane} hit the usage limit; spawns skip {name} for now");
                    let kind = ["agent-unavailable", "account-capped"][usize::from(account.is_some())];
                    hits.push((kind, format!("{lane}:{name}"), why));
                    capped = Some(rest(until));
                } else if chain && h.signed_out(&screen) {
                    let why = format!("{name}: {role} lane {lane} is signed out; spawns skip {name} for now");
                    hits.push(("agent-unavailable", format!("{lane}:{name}:auth"), why));
                    capped = Some(rest(ctx.now + cfg.limits.fallback_rest_minutes * 60));
                }
            }
            if let Some(p) = pane.filter(|_| status.is_some()) {
                if let Some(f) = crate::lanes::log(&ctx.state, lane) {
                    crate::lanes::roll(&f, &crate::lanes::body(&herdr::recent(r, p, "200").unwrap_or_default()));
                }
                if let Some((url, line)) = wt.and_then(|wt| amendment(ctx, r, cfg, s, wt)) {
                    let key = format!("{lane}:{url}");
                    let seen = events.iter().any(|e| e.str("key") == Some(&key));
                    let dsh = spawned.and_then(|e| e.str("harness")) == Some("dsh");
                    if !seen && herdr::reply(r, dsh, lane, p, &line).is_ok() {
                        hits.push(("amended", key, line));
                    }
                }
            }
            // #223: a lane done/idle past `idle_reap_minutes` keeps its agent but not what the agent started.
            let trim =
                wt.filter(|_| matches!(status, Some("done" | "idle")) && idle > cfg.limits.idle_reap_minutes * 60);
            let trimmed = trim.map(|wt| host::reap(r, Path::new(wt), true));
            if let Some((killed, mb)) = trimmed.filter(|t| !t.0.is_empty()) {
                let d = json!({"agent": lane, "killed": killed, "freed_mb": mb});
                let e = Event::new(ctx.now, &s.fleet, Some(s.issue), "lane-trimmed", d);
                emit(&ctx.state, cfg, e)?;
            }
            if idle > stall && status != Some("working") && !parked {
                let st = status.unwrap_or("gone");
                // #319: a settled fix lane at the FAILed head says what it is missing.
                let pr = gh::cached_pr(ctx, &s.fleet, s.issue);
                let same = s.state.starts_with("fix") && pr["headRefOid"].as_str().is_some_and(|h| s.reviewed(h));
                let ci = cfg
                    .fleet(&s.fleet)
                    .map(|f| gh::ci(&gh::required(&pr["statusCheckRollup"], &f.required_checks)));
                let why = match ci {
                    Ok(ci) if same => format!(
                        "fix lane idle, head unchanged, CI {ci}: run herdsman done {} {} --no-change \"<why>\" or push",
                        s.fleet, s.issue
                    ),
                    _ => format!("{role} lane {lane} is {st}"),
                };
                hits.push((
                    "stall",
                    key,
                    format!("no event for {} in {}; {why}", age(idle), s.state),
                ));
            }
        }
        if s.state == "ready" && idle > stall {
            if let Some(head) = mergeable(ctx, r, cfg, s) {
                let why = format!(
                    "PASS @ {} with green CI, unmerged for {}; run `herdsman merge`",
                    &head[..7.min(head.len())],
                    age(idle)
                );
                hits.push(("ready-unmerged", format!("{}:{head}", s.last_ts), why));
            }
        }
        for (kind, key, reason) in hits {
            let mut d = json!({"key": key, "reason": reason});
            if let (Some(serde_json::Value::Object(c)), "account-capped" | "agent-unavailable") = (&capped, kind) {
                c.iter().for_each(|(k, v)| d[k] = v.clone());
            }
            fire(ctx, cfg, out, &events, (&s.fleet, Some(s.issue)), kind, d)?;
        }
    }
    // #107: a herdr agent named like a lane (`<fleet>-<role>-<n>…`) that no `spawn` event logged; once per agent.
    // #376: only one of no tracked issue: drive adopts (or closes) the rest.
    let tracked = fold(&events);
    for name in agents.iter().flatten().filter_map(|a| a["name"].as_str()) {
        let lane = crate::spawn::shaped(cfg, &json!({"name": name}));
        let lane = lane.filter(|l| l.2.is_some() && !tracked.contains_key(&(l.0.to_string(), l.1)));
        if let Some(l) = lane.filter(|_| last_spawn(&events, name).is_none()) {
            let why = format!("herdr agent {name} has no spawn event and matches no tracked issue");
            fire(
                ctx,
                cfg,
                out,
                &events,
                (l.0, Some(l.1)),
                "orphan-lane",
                json!({"key": name, "reason": why}),
            )?;
        }
    }
    let open = open_asks(&events);
    // #248: a done lane whose handoff is logged closes its session after `done_close_minutes`; its worktree stays.
    for a in agents.iter().flatten().filter(|a| a["agent_status"] == "done") {
        let e = a["name"].as_str().and_then(|n| last_spawn(&events, n));
        let (Some(name), Some(pane), Some(e)) = (a["name"].as_str(), a["pane_id"].as_str(), e) else {
            continue;
        };
        let (Some(n), Some(wt), None) = (e.issue, e.str("worktree"), On::of(r, Some(e)).machine) else {
            continue;
        };
        let asking = open.iter().any(|q| q.str("agent") == Some(name));
        let due = handoff(r, &events, e, (n, wt)).filter(|t| ctx.now >= t + cfg.limits.done_close_minutes * 60);
        let Some(at) = due.filter(|_| !asking) else { continue };
        if let Some(why) = dirty(r, Path::new(wt)) {
            let d = json!({"key": format!("{name}:{at}"), "agent": name, "why": why, "reason": format!("{name} stays open: {why}")});
            fire(ctx, cfg, out, &events, (&e.fleet, Some(n)), "lane-kept-dirty", d)?;
            continue;
        }
        let mb = host::held(r, &[a]).first().copied().unwrap_or_default();
        if herdr::close(r, pane).is_ok() {
            let d = json!({"key": format!("{name}:{at}"), "agent": name, "freed_mb": mb, "reason": "done"});
            fire(ctx, cfg, out, &events, (&e.fleet, Some(n)), "lane-closed", d)?;
        }
    }
    let routine = |a: &&&Event| a.str("kind") == Some("routine");
    let due = |a: &&&Event| ctx.now.saturating_sub(a.ts) >= cfg.limits.ask_default_minutes * 60;
    for a in open.iter().filter(routine).filter(due) {
        let id = a.str("id").unwrap_or_default();
        let rec = a.data["recommend"].to_string();
        crate::answer(ctx, cfg, r, out, (&a.fleet, id), &rec, ("default", None))?;
    }
    let waiting = |a: &&&Event| !routine(a) && a.str("from").is_none() && ctx.now.saturating_sub(a.ts) > 30 * 60;
    for a in open.iter().filter(waiting) {
        let why = format!(
            "{} unanswered for {}: {}",
            a.str("id").unwrap_or("?"),
            age(ctx.now.saturating_sub(a.ts)),
            a.str("question").unwrap_or("?")
        );
        fire(
            ctx,
            cfg,
            out,
            &events,
            (&a.fleet, a.issue),
            "ask-waiting",
            json!({"key": a.str("id"), "reason": why}),
        )?;
    }
    if let (sent, held @ 1..) = PUSHES.replace((0, 0)) {
        let d = json!({"key": ctx.now, "reason": format!("{} questions waiting — open the board", sent + held)});
        fire(ctx, cfg, out, &events, ("herdsman", None), "ask-waiting", d)?;
    }
    // #87: devices subscribed but no webpush notifier: the page's alerts never send; once per device count.
    let subs = crate::webpush::load(&ctx.state).len();
    if subs > 0 && cfg.webpush_sub().is_none() {
        let why = format!("{subs} device(s) subscribed, but no type = \"webpush\" notifier: alerts are off");
        fire(
            ctx,
            cfg,
            out,
            &events,
            ("herdsman", None),
            "alerts-off",
            json!({"key": subs, "reason": why}),
        )?;
    }
    // DeepSeek under `warn_below`: once per crossing (a recovery logs `provider-ok`).
    if let Some(Ok((b, low))) = crate::usage::deepseek(ctx, cfg, r) {
        let last = events
            .iter()
            .rev()
            .find(|e| e.fleet == "deepseek" && matches!(e.kind.as_str(), "provider-low" | "provider-ok"));
        if low != last.is_some_and(|e| e.kind == "provider-low") {
            let warn = cfg.providers.deepseek.as_ref().map_or(0.0, |p| p.warn_below);
            let why = format!("balance {} {:.2} (warn below {warn})", b.currency, b.total);
            let ev = Event::new(
                ctx.now,
                "deepseek",
                None,
                if low { "provider-low" } else { "provider-ok" },
                json!({"reason": why}),
            );
            if low {
                writeln!(out, "{}", plain(&ev))?;
            }
            emit(&ctx.state, cfg, ev)?;
        }
    }
    // #186: processes left in closed lanes' worktrees.
    if let Some(a) = &agents {
        leftovers(ctx, cfg, r, a, true)?;
    }
    // #160: the worktree total (bytes, when) for the Services disk row; `serve` never walks or runs `du`.
    let _ = std::fs::write(
        ctx.state.with_file_name("worktrees.du"),
        format!("{} {}", du(&ctx.worktrees), ctx.now),
    );
    Ok(())
}

/// #248: when lane `e` handed off: the latest `pushed`/`verdict` since it spawned (design: its latest commit since).
fn handoff(r: &dyn Runner, events: &[Event], e: &Event, (n, wt): (u64, &str)) -> Option<u64> {
    if e.str("role") == Some("design") {
        let t = ok(r, "git", &["-C", wt, "log", "-1", "--format=%ct"]).ok()?;
        let t = t.trim().parse().ok();
        return t.filter(|t| *t > e.ts);
    }
    let by = |x: &&Event| x.ts >= e.ts && (x.is("pushed", &e.fleet, n) || x.is("verdict", &e.fleet, n));
    events.iter().rev().find(by).map(|x| x.ts)
}

thread_local!(static PUSHES: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) });

/// Log and print one alert unless one of its kind already carries `d`'s `key`; #283: past `max_pushes_per_pass`, no push.
fn fire(
    ctx: &Ctx,
    cfg: &Config,
    out: &mut dyn Write,
    events: &[Event],
    (fleet, issue): (&str, Option<u64>),
    kind: &str,
    d: serde_json::Value,
) -> Result<()> {
    if events
        .iter()
        .any(|e| e.kind == kind && e.fleet == fleet && e.issue == issue && e.data.get("key") == d.get("key"))
    {
        return Ok(());
    }
    let ev = Event::new(ctx.now, fleet, issue, kind, d);
    writeln!(out, "{}", plain(&ev))?;
    let ((sent, held), push) = (PUSHES.get(), crate::config::WEBPUSH_ON.contains(&kind));
    if push && sent >= cfg.limits.max_pushes_per_pass {
        PUSHES.set((sent, held + 1));
        return crate::events::append(&ctx.state, &ev);
    }
    PUSHES.set((sent + usize::from(push), held));
    emit(&ctx.state, cfg, ev)
}

/// After `herdsman merge`: close the issue's clean lane panes, then `herdsman clean --yes` them.
pub fn teardown(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str, issue: u64) -> Result<()> {
    let events = read(&ctx.state)?;
    let mut seen = std::collections::BTreeSet::new();
    for e in events.iter().rev().filter(|e| e.is("spawn", fleet, issue)) {
        let (Some(wt), Some(pane), on) = (e.str("worktree"), e.str("pane"), On::of(r, Some(e))) else {
            continue;
        };
        if e.str("role") == Some("design") || !seen.insert(wt) || !on.exists(Path::new(wt)) {
            continue;
        }
        match dirty(&on, Path::new(wt)) {
            Some(why) => writeln!(out, "keep pane {pane} ({wt}): {why}")?,
            None => drop(herdr::close(&on, pane)),
        }
    }
    if seen.is_empty() {
        return Ok(());
    }
    clean(ctx, cfg, r, out, Some(fleet), (Some(issue), None), true)
}

/// Remove worktree + pane of lanes whose issue is merged (or closed on GitHub). Dry run unless `yes`.
/// `--lane` (one agent's worktree) refuses `UNSAFE_CLEAN` with the reason when it cannot go.
pub fn clean(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    fleet: Option<&str>,
    (issue, lane): (Option<u64>, Option<&str>),
    yes: bool,
) -> Result<()> {
    let events = read(&ctx.state)?;
    let states = fold(&events);
    let agents = crate::hosts::all_agents(cfg, r).context("cannot list herdr agents; refusing to clean blind")?;
    let mut seen = std::collections::BTreeSet::new();
    let mut failed = 0;
    let mut freed = 0;
    let mut closed = BTreeMap::new();
    // Newest spawn per worktree wins (it has the current pane).
    for e in events
        .iter()
        .rev()
        .filter(|e| e.kind == "spawn" && fleet.is_none_or(|f| f == e.fleet))
    {
        let (Some(n), Some(wt)) = (e.issue, e.str("worktree")) else {
            continue;
        };
        if issue.is_some_and(|i| i != n) || lane.is_some_and(|a| e.str("agent") != Some(a)) {
            continue;
        }
        let on = On::of(r, Some(e));
        if seen.contains(wt) || !on.exists(Path::new(wt)) {
            continue;
        }
        let unsafe_lane =
            |why: &str| crate::gate::refused("UNSAFE_CLEAN", format!("{}: {why}", lane.unwrap_or_default()));
        // Merged by `herdsman merge`, or closed on GitHub by any other route.
        let why = if states.get(&(e.fleet.clone(), n)).is_some_and(|s| s.state == "merged") {
            "merged"
        } else if closed_issues(ctx, cfg, r, out, &mut closed, &e.fleet).contains(&n) {
            "issue closed"
        } else if lane.is_some() {
            let p = Some(gh::cached_pr(ctx, &e.fleet, n))
                .filter(|p| p["state"] == "OPEN")
                .map(|p| p["number"].clone());
            return unsafe_lane(&p.map_or("only after merge".into(), |n| {
                format!("only after merge (PR #{n} open)")
            }));
        } else {
            continue;
        };
        seen.insert(wt);
        let pane = e.str("pane").unwrap_or_default();
        let live = !host::live_in(&agents, e.str("agent").unwrap_or_default(), Path::new(wt)).is_empty();
        let keep = if live {
            Some("live agent".to_string())
        } else {
            if yes {
                reap(ctx, cfg, &on, (&e.fleet, Some(n)), wt)?;
                if on.machine.is_none() {
                    freed += purge(r, cfg, &e.fleet, out, Path::new(wt), false)?;
                }
            }
            dirty(&on, Path::new(wt))
        };
        let keep = match keep {
            Some(why) if !yes && !live && lane.is_none() => {
                writeln!(out, "would save {wt} to wip/ ({why}), then remove it")?;
                continue;
            }
            Some(_) if yes && !live && lane.is_none() => save(ctx, cfg, &on, out, &events, e, wt)?,
            k => k,
        };
        if let Some(reason) = keep {
            if lane.is_some() {
                return unsafe_lane(&reason);
            }
            writeln!(out, "keep {wt}: {reason}")?;
            continue;
        }
        if !yes {
            writeln!(out, "would remove {wt} (pane {pane}, {}#{n} {why})", e.fleet)?;
            continue;
        }
        if let Err(err) = remove(
            &on,
            cfg,
            &e.fleet,
            wt,
            e.str("workspace")
                .or_else(|| pane.split(':').next())
                .unwrap_or_default(),
        ) {
            writeln!(out, "failed {wt}: {err:#}")?;
            failed += 1;
            continue;
        }
        if !pane.is_empty() {
            let _ = herdr::close(&on, pane);
        }
        drop_log(ctx, e.str("agent"));
        writeln!(out, "removed {wt} ({}#{n} {why})", e.fleet)?;
        emit(
            &ctx.state,
            cfg,
            Event::new(
                ctx.now,
                &e.fleet,
                Some(n),
                "clean",
                json!({"worktree": wt, "pane": pane, "why": why}),
            ),
        )?;
    }
    if lane.is_some() && seen.is_empty() {
        return crate::gate::refused(
            "UNSAFE_CLEAN",
            format!("{}: no worktree in the log", lane.unwrap_or_default()),
        );
    }
    if !yes {
        writeln!(out, "dry run: nothing changed; pass --yes to remove")?;
    } else if freed > 0 {
        writeln!(out, "freed {freed} bytes of build folders")?;
    }
    if failed > 0 {
        anyhow::bail!("{failed} worktree(s) could not be removed; see above");
    }
    Ok(())
}

/// #186: TERM, then KILL, the processes inside `wt`; logs `lane-reaped` when any were signalled.
pub fn reap(ctx: &Ctx, cfg: &Config, r: &dyn Runner, (fleet, issue): (&str, Option<u64>), wt: &str) -> Result<usize> {
    let names = host::reap(r, Path::new(wt), false).0;
    if !names.is_empty() {
        let d = json!({"worktree": wt, "killed": names.len(), "names": names});
        emit(&ctx.state, cfg, Event::new(ctx.now, fleet, issue, "lane-reaped", d))?;
    }
    Ok(names.len())
}

/// #186: the processes in worktrees of closed lanes (newest spawn not its issue's active lane, no live herdr agent
/// there) as `(pid, command, cwd)`; with `yes` each such worktree is reaped.
pub fn leftovers(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    agents: &[serde_json::Value],
    yes: bool,
) -> Result<Vec<(String, String, String)>> {
    let (events, ps, mut seen, mut found) = (
        read(&ctx.state)?,
        host::cwds(r),
        std::collections::BTreeSet::new(),
        vec![],
    );
    let st = fold(&events);
    for e in events.iter().rev().filter(|e| e.kind == "spawn") {
        let (Some(wt), Some(n), agent, None) = (e.str("worktree"), e.issue, e.str("agent"), e.str("machine")) else {
            continue;
        };
        let active = st
            .get(&(e.fleet.clone(), n))
            .and_then(IssueState::active_lane)
            .is_some_and(|l| Some(l.1) == agent);
        let here = host::inside(&ps, Path::new(wt));
        if !seen.insert(wt)
            || active
            || here.is_empty()
            || !host::live_in(agents, agent.unwrap_or("\0"), Path::new(wt)).is_empty()
        {
            continue;
        }
        if yes {
            reap(ctx, cfg, r, (&e.fleet, Some(n)), wt)?;
        }
        found.extend(here);
    }
    Ok(found)
}

const BUILD_DIRS: &[&str] = &["target", "node_modules", ".next", "dist", ".cache"];
/// #103: delete a lane's build folders (#164: plus the fleet's `build_dirs`), even when the worktree stays; the bytes
/// freed (#361: with `dry`, listed and counted, none removed). One git does not ignore stays.
pub fn purge(r: &dyn Runner, cfg: &Config, fleet: &str, out: &mut dyn Write, wt: &Path, dry: bool) -> Result<u64> {
    let mut freed = 0;
    let more = cfg.fleet.get(fleet).map_or(&[][..], |f| &f.build_dirs);
    for d in BUILD_DIRS.iter().copied().chain(more.iter().map(String::as_str)) {
        let dir = wt.join(d);
        if !dir.is_dir() {
            continue;
        }
        if ok(r, "git", &["-C", &wt.to_string_lossy(), "check-ignore", "-q", d]).is_err() {
            writeln!(out, "keep {}: not git-ignored", dir.display())?;
            continue;
        }
        let size = du(&dir);
        let _ = dry.then(|| writeln!(out, "  would remove {} ({:.2} GB)", dir.display(), gb(size)));
        if dry || std::fs::remove_dir_all(&dir).is_ok() {
            freed += size;
        }
    }
    Ok(freed)
}

/// #164: a merged or closed lane's unsaved work goes to `wip/<agent>-<date>`, the remote sha read back, so the
/// worktree can go (`None`); a secret in it keeps the worktree, `wip-blocked` once (to the phone): the reason.
fn save(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    events: &[Event],
    e: &Event,
    wt: &str,
) -> Result<Option<String>> {
    let agent = e.str("agent").unwrap_or_default();
    let w = wip_branch(ctx, r, agent);
    let git = |a: &[&str]| ok(r, "git", &[&["-C", wt][..], a].concat());
    let sha = match snapshot(r, wt, &w, "wip: auto-saved by herdsman") {
        Err(Snap::Secret(hit)) => {
            let why = format!("unsaved work has a secret ({hit})");
            let d = json!({"key": wt, "agent": agent, "reason": format!("{wt}: {why}; kept")});
            fire(ctx, cfg, out, events, (&e.fleet, e.issue), "wip-blocked", d)?;
            return Ok(Some(why));
        }
        Err(Snap::Failed(err) | Snap::Push(err)) => return Ok(Some(format!("wip save failed: {err:#}"))),
        Ok(()) => git(&["rev-parse", "HEAD"])?.trim().to_string(),
    };
    let remote = git(&["ls-remote", "origin", &format!("refs/heads/{w}")])?;
    if remote.split_whitespace().next() != Some(sha.as_str()) {
        return Ok(Some(format!("{w} reads back `{}`, not {sha}", remote.trim())));
    }
    writeln!(out, "saved {wt} to {w} @ {sha:.7}")?;
    let d = json!({"agent": agent, "branch": w, "sha": sha});
    emit(&ctx.state, cfg, Event::new(ctx.now, &e.fleet, e.issue, "wip-saved", d))?;
    Ok(None)
}

/// #164: the worktree total by a full walk (drive's pass, never a request), stamped for spawn's `DISK_BUDGET` and the
/// Host tab. Over `worktree_budget_gb`, the idlest lanes' build folders go first (`budget-freed`), never a working one's.
pub fn budget(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write) -> Result<()> {
    let mut total = du(&ctx.worktrees);
    let over = |t: u64| gb(t) > cfg.limits.worktree_budget_gb;
    if over(total) {
        let (events, agents) = (read(&ctx.state)?, host::agents(r)?);
        let states = fold(&events);
        let mut seen = std::collections::BTreeSet::new();
        let mut lanes: Vec<(u64, &Event, &str)> = (events.iter().rev())
            .filter(|e| e.kind == "spawn" && e.str("role") != Some("orch"))
            .filter_map(|e| Some((e, e.str("worktree")?)))
            .filter(|(_, w)| Path::new(w).starts_with(&ctx.worktrees) && Path::new(w).exists() && seen.insert(*w))
            .filter(|(e, _)| {
                !agents
                    .iter()
                    .any(|a| a["name"].as_str() == e.str("agent") && a["agent_status"] == "working")
            })
            .map(|(e, w)| {
                (
                    e.issue
                        .and_then(|n| states.get(&(e.fleet.clone(), n)))
                        .map_or(e.ts, |s| s.last_ts),
                    e,
                    w,
                )
            })
            .collect();
        lanes.sort_by_key(|l| l.0);
        for (_, e, wt) in lanes {
            if !over(total) {
                break;
            }
            let bytes = purge(r, cfg, &e.fleet, out, Path::new(wt), false)?;
            if bytes > 0 {
                total = total.saturating_sub(bytes);
                writeln!(out, "budget: freed {bytes} bytes of build folders in {wt}")?;
                let d = json!({"agent": e.str("agent"), "bytes": bytes});
                emit(
                    &ctx.state,
                    cfg,
                    Event::new(ctx.now, &e.fleet, e.issue, "budget-freed", d),
                )?;
            }
        }
    }
    Ok(std::fs::write(
        ctx.state.with_file_name("worktrees.du"),
        format!("{total} {}", ctx.now),
    )?)
}

const NOTE: &str = "(logical sizes; APFS clones may free less)";
/// #361: build folders of worktrees idle 7 days, previewed; removed with `yes`, or on a `y` (rechecked, then removed).
pub fn reclaim(ctx: &Ctx, cfg: &Config, r: &dyn Runner, o: &mut dyn Write, i: &mut dyn Read, yes: bool) -> Result<()> {
    let (events, agents, ps) = (read(&ctx.state)?, host::agents(r)?, host::cwds(r));
    let (mut seen, mut total) = (std::collections::BTreeSet::new(), 0);
    for e in events.iter().rev().filter(|e| e.kind == "spawn") {
        let wt = Path::new(e.str("worktree").unwrap_or_default());
        if e.str("machine").is_some() || !wt.starts_with(&ctx.worktrees) || !wt.is_dir() || !seen.insert(wt) {
            continue;
        }
        let why = match touched(r, wt).map(|t| ctx.now.saturating_sub(t)) {
            _ if !host::live_in(&agents, e.str("agent").unwrap_or("\0"), wt).is_empty() => "a live lane".into(),
            _ if !host::inside(&ps, wt).is_empty() => "a process runs there".into(),
            Some(a) if a >= 7 * 86_400 => String::new(),
            Some(a) => format!("touched {} ago", age(a)),
            None => "HEAD or status unreadable".into(),
        };
        if why.is_empty() {
            writeln!(o, "{}:", wt.display())?;
            total += purge(r, cfg, &e.fleet, o, wt, !yes)?;
        } else {
            writeln!(o, "skip {}: {why}", wt.display())?;
        }
    }
    let done = if yes { "freed" } else { "reclaimable" };
    writeln!(o, "{:.2} GB {done} {NOTE}", gb(total))?;
    if yes {
        return Ok(());
    }
    let mut b = [0];
    if total > 0 && write!(o, "apply? [y/N] ").is_ok() && i.read(&mut b).is_ok() && b == *b"y" {
        return reclaim(ctx, cfg, r, o, i, true);
    }
    Ok(writeln!(o, "preview: nothing changed; pass --apply to remove")?)
}

/// #361: the latest of HEAD's last move and the mtimes of changed, deleted (nearest folder) or untracked files.
fn touched(r: &dyn Runner, wt: &Path) -> Option<u64> {
    let git = |a: &[&str]| ok(r, "git", &[&["-C", &*wt.to_string_lossy()][..], a].concat()).ok();
    let head = git(&["log", "-g", "-1", "--date=unix", "--format=%gd", "HEAD"])?;
    let mut t: u64 = head.trim().strip_prefix("HEAD@{")?.strip_suffix('}')?.parse().ok()?;
    for f in git(&["status", "--porcelain", "-z", "--untracked-files=all"])?.split_terminator('\0') {
        let f = f.get(3..).filter(|_| f.as_bytes().get(2) == Some(&b' ')).unwrap_or(f);
        let m = wt.join(f).ancestors().find_map(|a| a.metadata().ok());
        t = t.max(m.map_or(0, |m| std::os::unix::fs::MetadataExt::mtime(&m).unsigned_abs()));
    }
    Some(t)
}

#[allow(clippy::cast_precision_loss)] // GB to one decimal
fn gb(bytes: u64) -> f64 {
    bytes as f64 / 1_073_741_824.0
}

/// The last walk's worktree total in GB (`check`'s or `budget`'s, both stamp `worktrees.du`) and when.
pub fn worktree_du(ctx: &Ctx) -> Option<(f64, Option<u64>)> {
    let s = std::fs::read_to_string(ctx.state.with_file_name("worktrees.du")).ok()?;
    let mut n = s.split_whitespace();
    Some((gb(n.next()?.parse().ok()?), n.next().and_then(|t| t.parse().ok())))
}

pub fn worktree_gb(ctx: &Ctx) -> Option<f64> {
    worktree_du(ctx).map(|d| d.0)
}

/// #164: folders under the worktree root that no configured fleet's repo owns.
pub fn orphan_dirs(ctx: &Ctx, cfg: &Config) -> Vec<std::path::PathBuf> {
    let ours: Vec<&str> = cfg.fleet.values().map(crate::config::Fleet::repo_name).collect();
    let all = std::fs::read_dir(&ctx.worktrees).into_iter().flatten().flatten();
    all.map(|d| d.path())
        .filter(|p| p.is_dir() && !p.file_name().is_some_and(|n| ours.iter().any(|o| n == *o)))
        .collect()
}

/// `orphan <path> <size>` per orphan folder; removed only with `remove`. The count.
pub fn orphans(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, remove: bool) -> Result<usize> {
    let found = orphan_dirs(ctx, cfg);
    for p in &found {
        if remove {
            std::fs::remove_dir_all(p).with_context(|| format!("removing {}", p.display()))?;
            writeln!(out, "removed orphan {}", p.display())?;
        } else {
            writeln!(out, "orphan {} {:.1} GB", p.display(), gb(du(p)))?;
        }
    }
    Ok(found.len())
}

/// #138: the lane's rolling transcript goes with it.
fn drop_log(ctx: &Ctx, agent: Option<&str>) {
    let _ = crate::lanes::log(&ctx.state, agent.unwrap_or_default()).map(std::fs::remove_file);
}

/// Bytes of blocks used under `p` (symlinks not followed), like `du -k`, not apparent size; the disk row's total.
pub fn du(p: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => std::fs::read_dir(p).map_or(0, |d| d.flatten().map(|e| du(&e.path())).sum()),
        m => m.map_or(0, |m| m.blocks() * 512),
    }
}

/// The fleet's closed issue numbers, read once per fleet per run; a failed read is reported and counts as none.
fn closed_issues<'a>(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    memo: &'a mut BTreeMap<String, Vec<u64>>,
    fleet: &str,
) -> &'a Vec<u64> {
    if !memo.contains_key(fleet) {
        let got = cfg.fleet(fleet).and_then(|f| gh::closed(ctx, r, f, fleet, ctx.now));
        let list = got.unwrap_or_else(|e| {
            let _ = writeln!(
                out,
                "cannot list closed issues of {fleet} ({e:#}); only merged lanes are candidates"
            );
            vec![]
        });
        memo.insert(fleet.to_string(), list);
    }
    &memo[fleet]
}

/// herdr first; else `git worktree remove` (never `--force`) of the re-checked tree minus `OWN_FILES`.
pub fn remove(r: &On, cfg: &Config, fleet: &str, wt: &str, ws: &str) -> Result<()> {
    if !ws.is_empty() && herdr::worktree_remove(r, ws)? {
        return Ok(());
    }
    if let Some(why) = dirty(r, Path::new(wt)) {
        anyhow::bail!("{why}");
    }
    for f in OWN_FILES {
        r.remove(&Path::new(wt).join(f)).context(format!("removing {f}"))?;
    }
    // #157: on another host the repo checkout's path is its own; git finds it from the worktree.
    let local = match r.machine {
        None => cfg.fleet(fleet)?.local_path().to_string_lossy().into_owned(),
        Some(_) => wt.into(),
    };
    ok(r, "git", &["-C", &local, "worktree", "remove", wt])?;
    Ok(())
}

/// The agent's newest spawn, unless a `lane-killed` followed it; else `NO_SUCH_AGENT`.
fn spawned<'a>(events: &'a [Event], agent: &str) -> Result<&'a Event> {
    let i = events
        .iter()
        .rposition(|e| e.kind == "spawn" && e.str("agent") == Some(agent));
    match i.filter(|i| {
        !events[*i..]
            .iter()
            .any(|e| e.kind == "lane-killed" && e.str("agent") == Some(agent))
    }) {
        Some(i) => Ok(&events[i]),
        None => crate::gate::refused("NO_SUCH_AGENT", format!("no live lane `{agent}` in the log")),
    }
}

/// `herdsman stop`: Esc to the agent's pane (it pauses); logs `stop`.
pub fn stop(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, agent: &str) -> Result<()> {
    let events = read(&ctx.state)?;
    let e = spawned(&events, agent)?;
    herdr::keys(&On::of(r, Some(e)), e.str("pane").unwrap_or_default(), "Escape")?;
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, &e.fleet, e.issue, "stop", json!({"agent": agent})),
    )?;
    Ok(writeln!(out, "sent Esc to {agent}")?)
}

/// `herdsman close`: close a non-lane herdr agent's pane (by name or pane id); files stay. Logs `closed-other`.
pub fn close(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, agent: &str) -> Result<()> {
    let (events, agents) = (read(&ctx.state)?, herdr::agents(r)?);
    let a = agents.iter().find(|a| a["name"] == agent || a["pane_id"] == agent);
    let name = a.and_then(|a| a["name"].as_str()).unwrap_or(agent);
    if last_spawn(&events, agent).or(last_spawn(&events, name)).is_some() {
        return crate::gate::refused("IS_LANE", format!("`{name}` is a lane: use stop, kill or clean"));
    }
    let pane = a.and_then(|a| a["pane_id"].as_str()).context("no such herdr agent")?;
    herdr::close(r, pane)?;
    let ev = Event::new(ctx.now, "", None, "closed-other", json!({"agent": name}));
    emit(&ctx.state, cfg, ev)?;
    Ok(writeln!(out, "closed {name}'s pane; its worktree and files stay")?)
}

/// `herdsman kill`: the dry-run line (what is lost); with `yes`, unpushed or dirty work goes to
/// `wip/<agent>-<stamp>` first (refused `SECRET_IN_WIP`, naming `file:line`, on a real secret), then the pane closes.
/// The worktree is kept.
pub fn kill(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, agent: &str, yes: bool) -> Result<()> {
    let events = read(&ctx.state)?;
    let e = spawned(&events, agent)?;
    let role = e.str("role").unwrap_or_default();
    let st = e.issue.and_then(|n| fold(&events).remove(&(e.fleet.clone(), n)));
    let on = On::of(r, Some(e));
    let wt = e.str("worktree").filter(|w| on.exists(Path::new(w)));
    let git = |a: &[&str]| ok(&on, "git", &[&["-C", wt.unwrap_or_default()][..], a].concat());
    let (dirty, ahead) = match wt {
        Some(_) => {
            let st = git(&["status", "--porcelain"])?;
            (
                changes(&st),
                git(&["rev-list", "--count", "HEAD", "--not", "--remotes"])?
                    .trim()
                    .parse()?,
            )
        }
        None => (0, 0),
    };
    let wip = (dirty + ahead > 0).then(|| wip_branch(ctx, r, agent));
    let pr = e.issue.zip(cfg.fleet.get(&e.fleet)).and_then(|(n, f)| {
        let b = crate::gate::branch_of(st.as_ref(), n);
        gh::pr(ctx, r, f, (&e.fleet, n), &b, ctx.now).ok()
    });
    let pr = pr
        .filter(|p| p["state"] == "OPEN")
        .map_or(String::new(), |p| format!(" · PR #{} open", p["number"]));
    let lost = match &wip {
        Some(w) => format!("{ahead} unpushed commit(s), {dirty} dirty file(s) → {w}"),
        None => "nothing unpushed".into(),
    };
    writeln!(
        out,
        "{agent} · {} · {lost}{pr} · pane closes, worktree kept",
        st.map_or(role.to_string(), |s| s.state)
    )?;
    if !yes {
        return Ok(writeln!(out, "dry run: nothing changed; pass --yes to kill")?);
    }
    if let Some(w) = &wip {
        match snapshot(r, wt.unwrap_or_default(), w, "wip: kill snapshot") {
            Err(Snap::Failed(e)) => {
                return crate::gate::refused(
                    "SNAPSHOT_FAILED",
                    format!("{agent}: wip snapshot failed ({e:#}); index restored, nothing pushed, pane kept"),
                )
            }
            Err(Snap::Secret(hit)) => {
                return crate::gate::refused(
                    "SECRET_IN_WIP",
                    format!("{agent}'s unpushed diff has a secret ({hit}); nothing pushed, pane kept"),
                )
            }
            Err(Snap::Push(e)) => return Err(e),
            Ok(()) => {}
        }
    }
    herdr::close(&on, e.str("pane").unwrap_or_default())?;
    drop_log(ctx, Some(agent));
    let d = json!({"agent": agent, "role": role, "pane": e.str("pane"), "wip": wip});
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, &e.fleet, e.issue, "lane-killed", d),
    )?;
    Ok(writeln!(
        out,
        "killed {agent}{}",
        wip.map_or(String::new(), |w| format!(": work pushed to {w}"))
    )?)
}

/// `wip/<agent>-<YYYYmmdd-HHMM>` (the epoch when `date` is unreadable).
fn wip_branch(ctx: &Ctx, r: &dyn Runner, agent: &str) -> String {
    let stamp = crate::usage::date(r, ctx.now, "+%Y%m%d-%H%M");
    let ok = stamp.chars().all(|c| c.is_ascii_digit() || c == '-');
    format!("wip/{agent}-{}", if ok { stamp } else { ctx.now.to_string() })
}

/// #254: push `wt`'s unpushed or dirty work to `w` (`snapshot`, its secret scan first).
pub fn save_wip(r: &dyn Runner, wt: &str, w: &str, msg: &str) -> Result<()> {
    snapshot(r, wt, w, msg).map_err(|e| match e {
        Snap::Secret(hit) => anyhow::anyhow!("a secret in the diff ({hit})"),
        Snap::Failed(e) | Snap::Push(e) => e,
    })
}

/// Why a wip snapshot stopped: git failed (index restored), a secret in the diff (index restored), or the push.
enum Snap {
    Failed(anyhow::Error),
    Secret(String),
    Push(anyhow::Error),
}

/// Stage all but `OWN_FILES`, scan the staged and unpushed diff for a secret (#96, the first as `file:line: masked`),
/// commit any uncommitted change with `msg` and push HEAD to `w`.
fn snapshot(r: &dyn Runner, wt: &str, w: &str, msg: &str) -> std::result::Result<(), Snap> {
    let git = |a: &[&str]| ok(r, "git", &[&["-C", wt][..], a].concat());
    // Stage all, then unstage the scratch: an `:(exclude)` of an ignored file fails `git add`.
    let undo = |e: anyhow::Error| {
        let _ = git(&["reset", "-q"]);
        Snap::Failed(e)
    };
    let mut commit = false;
    let mut snap = || -> Result<(String, String)> {
        commit = changes(&git(&["status", "--porcelain"])?) > 0;
        git(&["add", "-A"])?;
        git(&[&["reset", "-q", "--"][..], OWN_FILES].concat())?;
        Ok((
            git(&["diff", "--cached"])?,
            git(&["log", "-p", "--format=", "HEAD", "--not", "--remotes"])?,
        ))
    };
    let (staged, log) = snap().map_err(undo)?;
    // The first added hunk line (a file header runs `diff --git` → `@@`) with a secret, as `file:line: masked`.
    let mut head = false;
    let mut file = "";
    let mut n = 0;
    let mut hit = None;
    for l in staged.lines().chain(log.lines()) {
        head = (head || l.starts_with("diff --git ")) && !l.starts_with("@@");
        if let Some((_, h)) = l.strip_prefix("@@ ").and_then(|h| h.split_once('+')) {
            n = h.split([',', ' ']).next().and_then(|s| s.parse().ok()).unwrap_or(1);
        } else if head {
            file = l.strip_prefix("+++ b/").unwrap_or(file);
        } else if let Some(a) = l.strip_prefix('+').filter(|_| hit.is_none()) {
            // Mask every secret on the line, not only the first.
            let mut m = a.trim().to_owned();
            while let Some(v) = crate::lanes::secret(&m) {
                m = m.replace(v, "•••");
            }
            if m != a.trim() {
                hit = Some(format!("{file}:{n}: {m}"));
            }
        }
        if !head && l.starts_with(['+', ' ']) {
            n += 1;
        }
    }
    if let Some(hit) = hit {
        let _ = git(&["reset", "-q"]);
        return Err(Snap::Secret(hit));
    }
    if commit {
        // A host with no git identity still saves: herdsman signs only when none is configured.
        let unset = ["user.name", "user.email"].iter().any(|k| git(&["config", k]).is_err());
        let id: &[&str] = if unset {
            &["-c", "user.name=herdsman", "-c", "user.email=herdsman@localhost"]
        } else {
            &[]
        };
        git(&[id, &["commit", "-q", "--no-verify", "-m", msg]].concat()).map_err(undo)?;
    }
    git(&["push", "origin", &format!("HEAD:refs/heads/{w}")]).map_err(Snap::Push)?;
    Ok(())
}
