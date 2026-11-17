use crate::config::Config;
use crate::events::{append, emit, fold, Event, Json};
use crate::runner::{ok, Runner};
use crate::Ctx;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

const KINDS: [&str; 5] = ["slo-idle", "slo-tick", "self-heal", "stuck", "slo-ok"];
const DRIVE_HEALS: [&str; 3] = ["respawned-silent", "nudged", "marked-ready"];
const WAITS: [&str; 1] = ["slo-idle:"];
const PROGRESS: [&str; 5] = ["spawn", "verdict", "pushed", "lane-pushed", "turn-end"];
const LOAD: [&str; 3] = ["HOST_BUSY", "MEM_LOW", "DISK_LOW"];

enum Heal {
    StopTick(String),
    Recount(String, u64),
    Restart(Stall),
    Move(Stall),
}

struct Stall {
    fleet: String,
    issue: u64,
    role: String,
    step: String,
    from: String,
    reason: String,
}

struct Breach {
    key: String,
    fleet: String,
    issue: Option<u64>,
    slo: Option<&'static str>,
    reason: String,
    heal: Option<Heal>,
    since: u64,
}

/// #855: each fleet SLO: its known heal first; #1183: `stuck` a pass later only when drive itself is broken.
pub fn pass(
    ctx: &Ctx,
    cfg: &Config,
    (r, seen): (&dyn Runner, &dyn Runner),
    out: &mut dyn Write,
    fleets: &[&str],
) -> Result<()> {
    let events = crate::events::working(&ctx.state)?;
    let agents = crate::session::all(cfg, seen).checked()?.running();
    let mut now: Vec<Breach> = tick(ctx, cfg, r).into_iter().collect();
    for f in fleets {
        now.extend(idle(ctx, cfg, &events, &agents, f));
        now.extend(no_pass(ctx, cfg, f));
        now.extend(errors(ctx, &events, f));
        now.extend(stalls(ctx, cfg, &events, f));
    }
    let eps = episodes(&events);
    let mut loud: BTreeSet<String> = eps
        .iter()
        .filter(|(k, ep)| broken(k) && now.iter().any(|b| &b.key == *k) && ep.iter().any(|e| e.kind == "stuck"))
        .filter_map(|(_, ep)| Some(ep.first()?.fleet.clone()))
        .collect();
    for (key, ep) in eps.iter().filter(|(k, _)| !now.iter().any(|b| &b.key == *k)) {
        let Some(e) = ep.first() else { continue };
        append(
            &ctx.state,
            &Event::new(ctx.now, &e.fleet, e.issue, "slo-ok", json!({"key": key})),
        )?;
    }
    for b in &now {
        step(
            ctx,
            cfg,
            r,
            out,
            &events,
            eps.get(&b.key).map_or(&[][..], Vec::as_slice),
            b,
            &mut loud,
        )?;
    }
    Ok(())
}

/// #855: the `stuck` alerts whose breach still holds, by fleet: one owner's card each (#1093); an issue's goes once its lane starts.
pub fn open_stuck(events: &[Event]) -> BTreeMap<&str, Vec<&Event>> {
    let mut m: BTreeMap<&str, Vec<&Event>> = BTreeMap::new();
    let started = |e: &Event| {
        e.issue
            .is_some_and(|n| events.iter().any(|x| x.seq > e.seq && x.is("spawn", &e.fleet, n)))
    };
    for e in episodes(events)
        .into_iter()
        .filter(|(k, _)| broken(k))
        .filter_map(|(_, ep)| ep.into_iter().find(|e| e.kind == "stuck"))
        .filter(|e| !started(e) && !e.str("reason").is_some_and(crate::promise::reviews_ahead))
    {
        m.entry(e.fleet.as_str()).or_default().push(e);
    }
    m
}

fn broken(key: &str) -> bool {
    !WAITS.iter().any(|w| key.starts_with(w))
}

fn episodes(events: &[Event]) -> BTreeMap<String, Vec<&Event>> {
    let mut m: BTreeMap<String, Vec<&Event>> = BTreeMap::new();
    for e in events.iter().filter(|e| KINDS.contains(&e.kind.as_str())) {
        let ep = m.entry(e.str("key").unwrap_or_default().into()).or_default();
        if e.kind == "slo-ok" {
            ep.clear();
        } else {
            ep.push(e);
        }
    }
    m.retain(|_, ep| !ep.is_empty());
    m
}

fn step(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    events: &[Event],
    ep: &[&Event],
    b: &Breach,
    loud: &mut BTreeSet<String>,
) -> Result<()> {
    if ep.iter().any(|e| e.kind == "stuck") {
        return Ok(());
    }
    let ev = |kind: &str, d: Value| Event::new(ctx.now, &b.fleet, b.issue, kind, d);
    if let Some(slo) = b.slo.filter(|_| ep.is_empty()) {
        writeln!(out, "{slo}: {}", b.reason)?;
        emit(&ctx.state, cfg, ev(slo, json!({"key": b.key, "reason": b.reason})))?;
    }
    let mine = |e: &&Event| e.fleet == b.fleet && e.issue.is_some() && e.issue == b.issue && e.ts >= b.since;
    let by_drive = events
        .iter()
        .filter(mine)
        .filter(|e| DRIVE_HEALS.contains(&e.kind.as_str()));
    let mut tried: Vec<(u64, String)> = by_drive.map(|e| (e.ts, e.kind.clone())).collect();
    tried.extend(
        ep.iter()
            .filter(|e| e.kind == "self-heal")
            .map(|e| (e.ts, e.str("did").unwrap_or("?").into())),
    );
    if let Some(h) = b.heal.as_ref().filter(|_| tried.is_empty()) {
        let did = heal(ctx, cfg, r, out, h);
        writeln!(out, "self-heal: {did}")?;
        return append(
            &ctx.state,
            &ev("self-heal", json!({"key": b.key, "did": did, "reason": b.reason})),
        );
    }
    if !broken(&b.key) {
        return Ok(());
    }
    if tried.iter().any(|t| ctx.now < t.0 + cfg.limits.slow_secs) {
        return Ok(());
    }
    let tried: Vec<String> = tried.into_iter().map(|t| t.1).collect();
    let tried = if tried.is_empty() {
        "nothing (no known heal)".into()
    } else {
        tried.join(", ")
    };
    let why = format!("{}; tried: {tried}", b.reason);
    let e = ev("stuck", json!({"key": b.key, "reason": why}));
    if loud.insert(b.fleet.clone()) {
        emit(&ctx.state, cfg, e)
    } else {
        append(&ctx.state, &e)
    }
}

fn heal(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, h: &Heal) -> String {
    match h {
        Heal::StopTick(pid) => {
            let gone = || ok(r, "kill", &["-0", pid]).is_err();
            ok(r, "kill", &["-TERM", pid]).ok();
            for _ in 0..10 {
                if gone() {
                    return format!("stopped tick pid {pid} (SIGTERM)");
                }
                r.sleep(1);
            }
            ok(r, "kill", &["-KILL", pid]).ok();
            format!("stopped tick pid {pid} (SIGKILL after 10 s)")
        }
        Heal::Recount(fleet, n) => {
            let was = crate::check::worktree_gb(ctx).unwrap_or_default();
            if let Err(e) = crate::check::budget(ctx, cfg, r, out) {
                return format!("recount failed: {e:#}");
            }
            let now = crate::check::worktree_gb(ctx).unwrap_or_default();
            crate::drive::kick(ctx, r, fleet, *n, "slo-idle");
            format!("recounted worktrees {was:.1} → {now:.1} GB; retried #{n}'s spawn")
        }
        Heal::Restart(s) => {
            if let Err(e) = log(ctx, s, "stall-restart") {
                return format!("restart not logged: {e:#}");
            }
            let n = s.issue.to_string();
            match crate::cli::run(&["spawn", &s.fleet, &n, &s.role, "--supersede"], ctx, r, out) {
                Ok(0) => format!("restarted #{n} {}: wip saved, lane cleaned, respawned", s.role),
                Ok(c) => format!("restart of #{n} {}: exit {c}", s.role),
                Err(e) => format!("restart of #{n} {}: {e:#}", s.role),
            }
        }
        Heal::Move(s) => {
            if let Err(e) = log(ctx, s, "stall-move") {
                return format!("move not logged: {e:#}");
            }
            crate::drive::kick(ctx, r, &s.fleet, s.issue, "stall-move");
            format!("moved #{} off {}; retried its spawn", s.issue, s.from)
        }
    }
}

fn log(ctx: &Ctx, s: &Stall, kind: &str) -> Result<()> {
    let d = json!({"role": s.role, "step": s.step, "from": s.from, "reason": s.reason, "by": "drover"});
    append(&ctx.state, &Event::new(ctx.now, &s.fleet, Some(s.issue), kind, d))
}

fn stalls(ctx: &Ctx, cfg: &Config, events: &[Event], fleet: &str) -> Vec<Breach> {
    let hours = (cfg.fleet(fleet).ok().and_then(|f| f.stall_restart_hours)).unwrap_or(cfg.limits.stall_restart_hours);
    let st = fold(events);
    let q = crate::queue::view(events, &st, fleet);
    let asks = crate::events::open_asks(events);
    let mut out = vec![];
    for s in st
        .values()
        .filter(|s| s.fleet == fleet && s.state != "ready" && crate::queue::in_flight(&s.state))
    {
        let n = s.issue;
        let mine = crate::events::of_issue(events, fleet, n);
        let Some(at) = mine.iter().rposition(|e| PROGRESS.contains(&e.kind.as_str())) else {
            continue;
        };
        let (Some(last), since) = (mine.get(at), mine.get(at + 1..).unwrap_or_default()) else {
            continue;
        };
        let code = |e: &&&Event| e.kind == "driven" && e.str("code").is_some();
        let codes: Vec<&str> = since.iter().filter(code).filter_map(|e| e.str("code")).collect();
        let held = !crate::events::held_on(events, s).is_empty()
            || q.iter()
                .any(|i| i.issue == n && (i.blocked_by.is_some() || !i.ext.is_empty()));
        let pr = crate::gh::cached_pr(ctx, fleet, n);
        let ci = !pr.is_null() && crate::gh::ci(pr.at("statusCheckRollup")) != "green";
        let asked = asks.iter().any(|e| e.fleet == fleet && e.issue == Some(n));
        let promise = mine
            .iter()
            .rfind(|e| matches!(e.kind.as_str(), "promise" | "promise-void"));
        let slot = promise.and_then(|e| e.str("kind").filter(|_| e.kind == "promise"));
        let slot = slot.is_some_and(|k| matches!(k, "spawn-pending" | "waiting-on"));
        let lock = format!("drive-{fleet}-{n}.lock");
        let driving = crate::db::leases(&ctx.state, &lock)
            .iter()
            .any(|l| l.expires_at > ctx.now);
        let load = codes.last().is_some_and(|c| LOAD.contains(c));
        if held || ci || asked || slot || driving || !load && !codes.is_empty() {
            continue;
        }
        let first = since
            .iter()
            .filter(code)
            .find(|e| e.str("code").is_some_and(|c| LOAD.contains(&c)));
        let start = first.filter(|_| load).map_or(last.ts, |e| e.ts);
        if ctx.now < start + hours * 3600 {
            continue;
        }
        let spawn = mine.iter().rfind(|e| e.kind == "spawn");
        let fail = mine
            .iter()
            .rfind(|e| matches!(e.kind.as_str(), "spawn" | "verdict"))
            .is_some_and(|e| e.str("verdict") == Some("FAIL"));
        let role = match (fail, spawn.and_then(|e| e.str("role"))) {
            (false, Some(r)) => r,
            _ => s.build.as_deref().unwrap_or("impl"),
        };
        let step = format!("{role}@{}", s.rounds);
        let from = spawn.and_then(|e| e.str("host")).unwrap_or("local");
        let done = |k: &str| mine.iter().any(|e| e.kind == k && e.str("step") == Some(&step));
        let (reason, kind) = if load {
            let code = codes.last().copied().unwrap_or_default();
            (
                format!("#{n} {role} waited {hours}h+ on {from}'s load ({code})"),
                "stall-move",
            )
        } else {
            (
                format!("#{n} {role} stalled {hours}h+: no push, verdict or lane event"),
                "stall-restart",
            )
        };
        let stall = Stall {
            fleet: fleet.into(),
            issue: n,
            role: role.into(),
            step: step.clone(),
            from: from.into(),
            reason: reason.clone(),
        };
        let heal = (!done(kind)).then_some(if load { Heal::Move(stall) } else { Heal::Restart(stall) });
        out.push(Breach {
            key: format!("stall:{fleet}#{n}:{step}"),
            fleet: fleet.into(),
            issue: Some(n),
            slo: None,
            reason,
            heal,
            since: start,
        });
    }
    out
}

fn tick(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> Option<Breach> {
    let text = std::fs::read_to_string(ctx.state.with_file_name("tick.step")).ok()?;
    let m: Value = serde_json::from_str(&text).unwrap_or_default();
    let since = m
        .at("since")
        .as_u64()
        .filter(|t| ctx.now >= t + cfg.limits.slo_tick_minutes * 60)?;
    let pid = m
        .at("pid")
        .as_u64()
        .filter(|p| *p != u64::from(crate::sim::pid()))
        .map(|p| p.to_string());
    if pid.as_ref().is_some_and(|p| ok(r, "kill", &["-0", p]).is_err()) {
        return None;
    }
    let ours =
        |c: &str| (c.contains("drover") || c.contains("herdsman")) && [" tick", " drive"].iter().any(|w| c.contains(w));
    let named = pid.filter(|p| ok(r, "ps", &["-o", "command=", "-p", p]).is_ok_and(|c| ours(&c)));
    let child = m.at("child").as_str().filter(|c| !c.is_empty()).unwrap_or("none");
    let step = m.at("step").as_str().unwrap_or("?");
    let mins = (ctx.now - since) / 60;
    Some(Breach {
        key: format!("slo-tick:{since}"),
        fleet: "drover".into(),
        issue: None,
        slo: Some("slo-tick"),
        reason: format!("tick running {mins} min, in {step}; waiting on: {child}"),
        heal: named.map(Heal::StopTick),
        since,
    })
}

fn idle(ctx: &Ctx, cfg: &Config, events: &[Event], agents: &[crate::session::Session], fleet: &str) -> Option<Breach> {
    let at = ctx.state.with_file_name(format!("slo-idle-{fleet}.at"));
    let st = fold(events);
    let lanes: Vec<&str> = st
        .values()
        .filter(|s| s.fleet == fleet)
        .filter_map(|s| s.lane.as_deref())
        .collect();
    let working = |a: &crate::session::Session| a.busy() && lanes.iter().any(|l| a.name == *l);
    let next = crate::queue::startable(events, fleet)
        .first()
        .and_then(|n| n.parse::<u64>().ok());
    let Some(n) = next.filter(|_| !agents.iter().any(working)) else {
        std::fs::remove_file(&at).ok();
        return None;
    };
    let since = std::fs::read_to_string(&at).ok().and_then(|s| s.trim().parse().ok());
    let since = since.or_else(|| std::fs::write(&at, ctx.now.to_string()).ok().map(|()| ctx.now))?;
    if ctx.now < since + cfg.limits.slo_idle_minutes * 60 {
        return None;
    }
    let over = crate::check::worktree_gb(ctx).filter(|g| *g > cfg.limits.worktree_budget_gb);
    let refused = events
        .iter()
        .rev()
        .find(|e| e.fleet == fleet && e.issue == Some(n) && e.str("code").is_some());
    let held = match (over, refused) {
        (Some(gb), _) => format!("; held: DISK_BUDGET (worktrees {gb:.1} GB)"),
        (None, Some(e)) => format!(
            "; #{n}'s last start: {} {}",
            e.str("code").unwrap_or_default(),
            e.str("reason").unwrap_or_default()
        ),
        (None, None) => String::new(),
    };
    Some(Breach {
        key: format!("slo-idle:{fleet}:{since}"),
        fleet: fleet.into(),
        issue: None,
        slo: Some("slo-idle"),
        reason: format!(
            "0 lanes working for {} min with #{n} spawnable{held}",
            (ctx.now - since) / 60
        ),
        heal: over.map(|_| Heal::Recount(fleet.into(), n)),
        since,
    })
}

fn no_pass(ctx: &Ctx, cfg: &Config, fleet: &str) -> Option<Breach> {
    let last = crate::drive::last_pass(ctx, fleet)?;
    let mins = ctx.now.saturating_sub(last) / 60;
    (mins >= cfg.limits.slo_pass_minutes).then(|| Breach {
        key: format!("no-pass:{fleet}:{last}"),
        fleet: fleet.into(),
        issue: None,
        slo: None,
        reason: format!("no drive pass for {mins} min"),
        heal: None,
        since: last,
    })
}

fn errors(ctx: &Ctx, events: &[Event], fleet: &str) -> Vec<Breach> {
    let slow = |e: &&Event| {
        e.kind == "drive-pass"
            && e.fleet == fleet
            && e.str("by") != Some("fast")
            && e.data.get("kick").is_none_or(Value::is_null)
    };
    let last = events.iter().rfind(slow);
    let acts = last
        .and_then(|e| e.data.get("actions")?.as_array())
        .into_iter()
        .flatten();
    let failed = acts
        .filter(|a| a["step"] == "pass" && a["outcome"] == "error")
        .map(|a| Breach {
            key: format!("pass-error:{fleet}"),
            fleet: fleet.into(),
            issue: None,
            slo: None,
            reason: format!("drive's pass {}", a.at("words").as_str().unwrap_or("failed")),
            heal: None,
            since: last.map_or(ctx.now, |e| e.ts),
        });
    let mut out: Vec<Breach> = failed.take(1).collect();
    let st = fold(events);
    let open = st
        .values()
        .filter(|s| s.fleet == fleet && !matches!(s.state.as_str(), "merged" | "closed"));
    for s in open {
        let f = crate::drive::failures(events, fleet, s.issue);
        let Some((k, error)) = crate::drive::same_run(&f).filter(|(k, _)| *k >= 2) else {
            continue;
        };
        let (Some(first), Some(last)) = (f.len().checked_sub(k).and_then(|i| f.get(i)), f.last()) else {
            continue;
        };
        let head = crate::gh::cached_pr(ctx, fleet, s.issue).at("headRefOid").clone();
        if head.as_str() != last.str("head") {
            continue;
        }
        out.push(Breach {
            key: format!("error:{fleet}#{}", s.issue),
            fleet: fleet.into(),
            issue: Some(s.issue),
            slo: None,
            reason: format!(
                "{} fails the same way {k} times: {error}",
                last.str("step").unwrap_or("spawn")
            ),
            heal: None,
            since: first.ts,
        });
    }
    out
}

/// #1183: a stuck card's [force]: the issue's `drover waiting` next command, once, with `--force`.
pub fn force(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, (fleet, n): (&str, u64)) -> Result<String> {
    let events = crate::events::working(&ctx.state)?;
    let agents = crate::session::all(cfg, r).checked()?.running();
    let rows = crate::waiting::scan(ctx, cfg, r, &events, &agents, false, Some(fleet));
    let next = rows
        .into_iter()
        .find(|(s, _)| s.issue == n)
        .and_then(|(_, w)| w.next_cmd);
    let Some(cmd) = next.filter(|c| c.starts_with("drover spawn ")) else {
        return crate::gate::refused("BAD_VALUE", format!("{fleet} #{n} has no spawn to force"));
    };
    let mut args: Vec<&str> = cmd.split_whitespace().skip(1).collect();
    args.push("--force");
    match crate::cli::run(&args, ctx, r, out)? {
        0 => Ok(format!("{cmd} --force")),
        c => anyhow::bail!("{cmd} --force: exit {c}"),
    }
}
