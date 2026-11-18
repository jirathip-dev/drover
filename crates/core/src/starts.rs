use crate::config::{Config, StartRelease};
use crate::events::Event;
use crate::events::Json;
use crate::runner::Runner;
use crate::{queue, Ctx};
use serde_json::{json, Value};

/// The asking fleet, its weight and the weight of the other fleets with something to start.
pub type Me<'a> = (&'a str, usize, usize);

fn main_host(cfg: &Config) -> &str {
    let main = cfg.hosts.iter().find(|h| h.1.machine.is_none());
    main.map_or("local", |h| h.0.as_str())
}

fn host_of<'a>(cfg: &'a Config, p: &'a Event) -> &'a str {
    p.str("host")
        .filter(|h| cfg.hosts.contains_key(*h))
        .unwrap_or(main_host(cfg))
}

fn hosts(cfg: &Config) -> Vec<&str> {
    if cfg.hosts.is_empty() {
        return vec![main_host(cfg)];
    }
    (cfg.hosts.iter().filter(|h| h.1.enabled))
        .map(|h| h.0.as_str())
        .collect()
}

fn freed(cfg: &Config, h: &str, p: &Event) -> bool {
    p.data.get("started") == Some(&json!(true)) && cfg.start_release(h) == StartRelease::Started
}

fn full(cfg: &Config, live: &[&Event], h: &str, (fleet, weight, others): Me, start: bool) -> Option<String> {
    let h = if cfg.hosts.contains_key(h) { h } else { main_host(cfg) };
    let on: Vec<&&Event> = live
        .iter()
        .filter(|p| host_of(cfg, p) == h && !freed(cfg, h, p))
        .collect();
    let max = cfg.start_max(h);
    let main = cfg.hosts.get(h).is_none_or(|c| c.machine.is_none());
    let room = max.saturating_sub(usize::from(main && start && max > 1));
    let mine = on.iter().filter(|p| p.fleet == fleet).count();
    let share = start && mine * (weight + others) >= room * weight;
    let full = on.len() >= room || others > 0 && (mine >= max.div_ceil(2) || share);
    full.then(|| format!("{h} {}/{max} starting", on.len()))
}

/// Every enabled host's start slots are full for `me`: each host's `<host> N/max starting`.
pub fn every_full(cfg: &Config, live: &[&Event], me: Me, start: bool) -> Option<String> {
    let all = hosts(cfg);
    let held: Vec<String> = all.iter().map_while(|h| full(cfg, live, h, me, start)).collect();
    (held.len() == all.len() && !held.is_empty()).then(|| held.join(", "))
}

/// The start slots held now, as a refusal names them (`<host> N/max starting: <marker> <age>, …`); empty when none is.
pub fn held_now(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> String {
    let pending = crate::drive::pending(ctx, cfg, r);
    let on = |h: &str| {
        pending
            .iter()
            .filter(|p| host_of(cfg, p) == h && !freed(cfg, h, p))
            .count()
    };
    let full: Vec<String> = (hosts(cfg).into_iter())
        .filter_map(|h| Some((h, on(h))).filter(|x| x.1 > 0))
        .map(|(h, n)| format!("{h} {n}/{} starting", cfg.start_max(h)))
        .collect();
    if full.is_empty() {
        return String::new();
    }
    format!("{}: {}", full.join(", "), crate::drive::held_by(ctx, cfg, r))
}

/// Spawn marker `l`'s spawn has ended since it was taken: logged, failed with `spawn-error`, or its intent `failed`.
pub fn ended(ev: &[Event], l: &crate::db::Lease) -> bool {
    let Some(p) = serde_json::from_str::<Event>(&l.data)
        .ok()
        .filter(|p| p.issue.is_some())
    else {
        return false;
    };
    let end = |e: &&Event| {
        e.fleet == p.fleet && e.issue == p.issue && ["spawn", "spawn-error", "failed"].contains(&e.kind.as_str())
    };
    ev.iter().rev().take_while(|e| e.ts >= l.at).any(|e| end(&e))
}

/// Some host has a free start slot, the `pending` spawns counted.
pub fn free(cfg: &Config, pending: &[Event]) -> bool {
    let live: Vec<&Event> = pending.iter().collect();
    hosts(cfg)
        .iter()
        .any(|h| full(cfg, &live, h, ("", 1, 0), false).is_none())
}

/// Why placement passes over host `h` for `fleet` #`n`: its start slots (`SPAWN_PENDING`) or its working slots (`HOST_FULL`) are full.
pub fn skip(
    ctx: &Ctx,
    (cfg, r): (&Config, &dyn Runner),
    ev: &[Event],
    (all, live): (&[Event], &[&Event]),
    (me, start, n, role): (Me, bool, u64, &str),
    h: &str,
) -> Option<(&'static str, String)> {
    if let Some(why) = full(cfg, live, h, me, start) {
        return Some(("SPAWN_PENDING", why));
    }
    let on: Vec<&Event> = all.iter().filter(|p| host_of(cfg, p) == h).collect();
    let why = crate::hosts::working_full(ctx, cfg, r, ev, (me.0, n, role), &on, (h, cfg.hosts.get(h)?))?;
    Some(("HOST_FULL", why))
}

fn marker(ctx: &Ctx, fleet: &str, n: u64) -> Option<(crate::db::Lease, Event)> {
    let l = crate::drive::own_marker(ctx, fleet, n)?;
    let e = serde_json::from_str(&l.data).ok()?;
    Some((l, e))
}

/// The host drive's admission placed this kick's own spawn of `fleet` #`n` on.
pub fn admitted(ctx: &Ctx, fleet: &str, n: u64) -> Option<String> {
    marker(ctx, fleet, n)?.1.str("host").map(String::from)
}

/// This kick's own spawn has its harness up: where the host's `start_release` is `started`, its start slot frees.
pub fn started(ctx: &Ctx, fleet: &str, n: u64) {
    let Some((l, mut e)) = marker(ctx, fleet, n) else {
        return;
    };
    e.data.insert("started".into(), true.into());
    if let Ok(data) = serde_json::to_string(&e) {
        crate::db::renew(&ctx.state, &l, &crate::db::Lease { data, ..l.clone() });
    }
}

/// The summed weight of the other unpaused fleets with something to start, or held for a start slot last pass.
pub fn others(ctx: &Ctx, cfg: &Config, ev: &[Event], fleet: &str) -> usize {
    let others = cfg.fleet.iter().filter(|(g, f)| *g != fleet && !f.paused);
    let waits = |g: &str| {
        let start = |n: &String| n.parse().is_ok_and(|n| crate::drive::backoff(ctx, ev, g, n).is_none());
        let pass = |e: &&Event| e.kind == "drive-pass" && e.fleet == g && e.data.get("kick").is_none_or(Value::is_null);
        let held = ev
            .iter()
            .rfind(pass)
            .and_then(|e| e.data.get("actions")?.as_array().cloned());
        queue::startable(ev, g).iter().any(start) || held.into_iter().flatten().any(|a| a.at("code") == "SPAWN_PENDING")
    };
    others
        .filter(|(g, _)| waits(g))
        .map(|(_, f)| f.weight.unwrap_or(1).max(1))
        .sum()
}

/// A start refused with `code` holds back the fleet's other starts this pass.
pub fn stops(code: Option<&str>) -> bool {
    matches!(
        code,
        Some(
            "HOST_BUSY"
                | "HOST_UPDATING"
                | "DISK_LOW"
                | "DISK_BUDGET"
                | "MEM_LOW"
                | "LOGIN_NEEDED"
                | "CAP"
                | "IN_FLIGHT_CAP"
        )
    )
}

/// `fleet` has a review FAIL with no spawn after it: a fix round waits for a slot.
pub fn fix_due(ev: &[Event], fleet: &str) -> bool {
    let fail = |e: &Event| e.kind == "verdict" && e.str("verdict") == Some("FAIL");
    let mut last = std::collections::BTreeMap::new();
    for e in ev.iter().filter(|e| e.fleet == fleet && (e.kind == "spawn" || fail(e))) {
        last.insert(e.issue, e.kind == "verdict");
    }
    last.into_values().any(|due| due)
}
