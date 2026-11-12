use crate::config::Config;
use crate::events::{append, emit, fold, Event};
use crate::runner::{ok, Runner};
use crate::Ctx;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;

const KINDS: [&str; 5] = ["slo-idle", "slo-tick", "self-heal", "stuck", "slo-ok"];
const DRIVE_HEALS: [&str; 3] = ["respawned-silent", "nudged", "marked-ready"];

enum Heal {
    StopTick(String),
    Recount(String, u64),
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

/// #855: each fleet SLO and missed promise: its known heal first, `stuck` if it still holds a pass later.
pub fn pass(
    ctx: &Ctx,
    cfg: &Config,
    (r, seen): (&dyn Runner, &dyn Runner),
    out: &mut dyn Write,
    fleets: &[&str],
) -> Result<()> {
    let events = crate::events::read(&ctx.state)?;
    let agents = crate::hosts::all_agents(cfg, seen)?;
    let mut now: Vec<Breach> = tick(ctx, cfg, r).into_iter().collect();
    for f in fleets {
        now.extend(idle(ctx, cfg, &events, &agents, f));
        now.extend(missed(ctx, &events, f));
    }
    let eps = episodes(&events);
    for (key, ep) in eps.iter().filter(|(k, _)| !now.iter().any(|b| &b.key == *k)) {
        append(
            &ctx.state,
            &Event::new(ctx.now, &ep[0].fleet, ep[0].issue, "slo-ok", json!({"key": key})),
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
        )?;
    }
    Ok(())
}

/// #855: the `stuck` alerts whose breach still holds: the owner's cards.
pub fn open_stuck(events: &[Event]) -> Vec<&Event> {
    let eps = episodes(events);
    eps.into_values()
        .filter_map(|ep| ep.into_iter().find(|e| e.kind == "stuck"))
        .collect()
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
    emit(&ctx.state, cfg, ev("stuck", json!({"key": b.key, "reason": why})))
}

fn heal(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, h: &Heal) -> String {
    match h {
        Heal::StopTick(pid) => {
            let gone = || ok(r, "kill", &["-0", pid]).is_err();
            let _ = ok(r, "kill", &["-TERM", pid]);
            for _ in 0..10 {
                if gone() {
                    return format!("stopped tick pid {pid} (SIGTERM)");
                }
                r.sleep(1);
            }
            let _ = ok(r, "kill", &["-KILL", pid]);
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
    }
}

fn tick(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> Option<Breach> {
    let text = std::fs::read_to_string(ctx.state.with_file_name("tick.step")).ok()?;
    let m: Value = serde_json::from_str(&text).unwrap_or_default();
    let since = m["since"]
        .as_u64()
        .filter(|t| ctx.now >= t + cfg.limits.slo_tick_minutes * 60)?;
    let pid = m["pid"]
        .as_u64()
        .filter(|p| *p != u64::from(std::process::id()))
        .map(|p| p.to_string());
    if pid.as_ref().is_some_and(|p| ok(r, "kill", &["-0", p]).is_err()) {
        return None;
    }
    let ours = |c: &str| c.contains("herdsman") && [" tick", " drive"].iter().any(|w| c.contains(w));
    let named = pid.filter(|p| ok(r, "ps", &["-o", "command=", "-p", p]).is_ok_and(|c| ours(&c)));
    let child = m["child"].as_str().filter(|c| !c.is_empty()).unwrap_or("none");
    let step = m["step"].as_str().unwrap_or("?");
    let mins = (ctx.now - since) / 60;
    Some(Breach {
        key: format!("slo-tick:{since}"),
        fleet: "herdsman".into(),
        issue: None,
        slo: Some("slo-tick"),
        reason: format!("tick running {mins} min, in {step}; waiting on: {child}"),
        heal: named.map(Heal::StopTick),
        since,
    })
}

fn idle(ctx: &Ctx, cfg: &Config, events: &[Event], agents: &[Value], fleet: &str) -> Option<Breach> {
    let at = ctx.state.with_file_name(format!("slo-idle-{fleet}.at"));
    let st = fold(events);
    let lanes: Vec<&str> = st
        .values()
        .filter(|s| s.fleet == fleet)
        .filter_map(|s| s.lane.as_deref())
        .collect();
    let working = |a: &Value| a["agent_status"] == "working" && lanes.iter().any(|l| a["name"] == *l);
    let next = crate::queue::startable(events, fleet)
        .first()
        .and_then(|n| n.parse::<u64>().ok());
    let Some(n) = next.filter(|_| !agents.iter().any(working)) else {
        let _ = std::fs::remove_file(&at);
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

fn missed(ctx: &Ctx, events: &[Event], fleet: &str) -> Vec<Breach> {
    let st = fold(events);
    let open = st.values().filter(|s| s.fleet == fleet).filter_map(|s| {
        let head = crate::gh::cached_pr(ctx, fleet, s.issue)["headRefOid"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let (p, why) = crate::promise::open_missed(events, s, &head, None)?;
        Some(Breach {
            key: format!("promise:{fleet}#{}:{}", s.issue, p.made_at),
            fleet: fleet.into(),
            issue: Some(s.issue),
            slo: None,
            reason: format!("promise `{}` missed: {why}", p.reason),
            heal: None,
            since: p.made_at,
        })
    });
    open.collect()
}
