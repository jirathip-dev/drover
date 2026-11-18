use crate::config::Config;
use crate::events::{append, emit, last_spawn, working, Event};
use crate::hosts::On;
use crate::runner::Runner;
use crate::session::Exit;
use crate::Ctx;
use anyhow::Result;
use serde_json::{json, Value};
use std::io::Write;

/// The line a canary types; its output, not the echoed line, holds `CANARY_OUT`.
pub const CANARY: &str = "echo drover-canary-$((6*7))";
pub const CANARY_OUT: &str = "drover-canary-42";

/// The host spawn `sp`'s lane runs on (`local` without `[hosts]`).
pub fn host(sp: &Event) -> &str {
    sp.str("host").unwrap_or("local")
}

const WAVE_LANES: usize = 3;

/// Lanes of one host gone by an adapter crash together: `WAVE_LANES` or more within `[limits] crash_wave_minutes`.
pub struct Wave {
    pub host: String,
    pub at: u64,
    pub agents: Vec<String>,
    pub said: String,
}

impl Wave {
    /// The one key its alert and its lanes' `lane-dead` rows carry.
    pub fn key(&self) -> String {
        format!("{}:{}", self.host, self.at)
    }
}

/// Every host's crash waves among the lanes whose latest stamp is an adapter crash.
pub fn waves(cfg: &Config, ev: &[Event]) -> Vec<Wave> {
    let crash = crate::session::Cause::Crash.word();
    let mut dead: Vec<(&str, u64, &str, String)> = crate::events::of_kind(ev, &["lane-state"])
        .into_iter()
        .filter(|e| e.str("state") == Some("gone") && e.str("cause") == Some(crash))
        .filter_map(|e| {
            let agent = e.str("agent")?;
            let latest = crate::session::stamp::latest(ev, agent).is_some_and(|l| std::ptr::eq(l, e));
            let exit = crate::session::stamp::exit(ev, agent).filter(|_| latest)?;
            Some((host(last_spawn(ev, agent)?), e.ts, agent, exit.line()))
        })
        .collect();
    dead.sort_by_key(|d| (d.0, d.1));
    let window = cfg.limits.crash_wave_minutes * 60;
    let mut out: Vec<Wave> = vec![];
    for (h, ts, agent, said) in dead {
        match out.last_mut().filter(|w| w.host == h && ts <= w.at + window) {
            Some(w) => w.agents.push(agent.into()),
            None => out.push(Wave {
                host: h.into(),
                at: ts,
                agents: vec![agent.into()],
                said,
            }),
        }
    }
    out.retain(|w| w.agents.len() >= WAVE_LANES);
    out
}

/// The crash wave lane `agent` died in, if any.
pub fn wave_of(cfg: &Config, ev: &[Event], agent: &str) -> Option<Wave> {
    waves(cfg, ev).into_iter().find(|w| w.agents.iter().any(|a| a == agent))
}

/// A dead lane's `lane-dead` fields: its exit's cause, and the crash wave it died in (whose one alert stands for it).
pub fn dead_tags(cfg: &Config, ev: &[Event], lane: &str, exit: Option<&Exit>) -> Vec<(&'static str, Value)> {
    let cause = exit.map(|x| ("cause", json!(x.cause.word())));
    let wave = wave_of(cfg, ev, lane).map(|w| ("wave", json!(w.key())));
    cause.into_iter().chain(wave).collect()
}

/// One `lane-wave` alert per wave: its lanes come back together once the host admits them, not `respawn_per_pass` a pass.
pub fn wave_alert(ctx: &Ctx, cfg: &Config, w: &Wave) -> Result<()> {
    let key = w.key();
    if working(&ctx.state)?
        .iter()
        .any(|e| e.kind == "lane-wave" && e.str("key") == Some(&key))
    {
        return Ok(());
    }
    let reason = format!(
        "{} lanes on {} died within {} min ({}): respawning them together once the host is fine",
        w.agents.len(),
        w.host,
        cfg.limits.crash_wave_minutes,
        w.said
    );
    let d = json!({"key": key, "host": w.host, "agents": w.agents, "reason": reason});
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, "drover", None, "lane-wave", d).push(),
    )
}

/// Host `h`'s `runtime-down` while its breaker is open.
pub fn down<'a>(ev: &'a [Event], h: &str) -> Option<&'a Event> {
    let on = crate::events::of_kind(ev, &["runtime-down", "runtime-up"]);
    on.into_iter()
        .rfind(|e| e.str("host") == Some(h))
        .filter(|e| e.kind == "runtime-down")
}

/// `runtime down on <host> since <HH:MM> UTC` while host `h`'s breaker is open.
pub fn says(ev: &[Event], h: &str) -> Option<String> {
    let t = down(ev, h)?.ts;
    Some(format!(
        "runtime down on {h} since {:02}:{:02} UTC",
        t / 3600 % 24,
        t / 60 % 60
    ))
}

/// The host lane `lane`'s last spawn in `ev` ran on, stamped `failed_on` on the `respawn` or `needs-human` it ends in.
pub fn blame<'a>(ev: &'a [Event], lane: &str) -> Option<&'a str> {
    last_spawn(ev, lane).map(host)
}

fn failure_on(e: &Event, h: &str) -> bool {
    let ends =
        e.kind == "respawn" || e.kind == "needs-human" && matches!(e.str("code"), Some("LANE_ENDS" | "START_FAILED"));
    ends && e.str("failed_on") == Some(h)
}

/// Lane `lane` failed to start or ended with no push: true while its host's breaker is open (trips it at the limit).
pub fn failed(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, fleet: &str, lane: &str, why: &str) -> Result<bool> {
    let ev = working(&ctx.state)?;
    let Some(sp) = last_spawn(&ev, lane) else {
        return Ok(false);
    };
    let h = host(sp);
    if down(&ev, h).is_some() {
        return Ok(true);
    }
    let l = &cfg.limits;
    let up = ev.iter().rfind(|e| e.kind == "runtime-up" && e.str("host") == Some(h));
    if up.is_some_and(|u| sp.ts < u.ts) {
        return Ok(false);
    }
    let since = up
        .map_or(0, |e| e.ts)
        .max(ctx.now.saturating_sub(l.runtime_down_minutes * 60));
    let count = 1 + ev.iter().filter(|e| e.ts >= since && failure_on(e, h)).count();
    if u64::try_from(count).unwrap_or(u64::MAX) < l.runtime_down_failures {
        return Ok(false);
    }
    let reason = format!(
        "runtime down on {h}: {count} lanes failed to start or ended without a push in {} min (last: {lane}: {why}); spawns there held until a canary passes",
        l.runtime_down_minutes
    );
    writeln!(out, "{reason}")?;
    let d = json!({"host": h, "failures": count, "sample": why, "reason": reason});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, None, "runtime-down", d))?;
    Ok(true)
}

/// Each host whose breaker is open runs one canary; a pass closes it (`runtime-up`).
pub fn probe(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write) -> Result<()> {
    let ev = working(&ctx.state)?;
    let open = |e: &&Event| {
        e.kind == "runtime-down"
            && e.str("host")
                .is_some_and(|h| down(&ev, h).is_some_and(|d| std::ptr::eq(d, *e)))
    };
    for e in ev.iter().filter(open) {
        let h = e.str("host").unwrap_or_default();
        let on = On::new(r, cfg.hosts.get(h).and_then(|c| c.machine.as_deref()));
        let ok = crate::runner::Runner::shed(&on).is_some_and(|b| crate::term::Shed(b).canary(&on));
        if !ok {
            writeln!(out, "runtime down on {h}: canary failed")?;
            continue;
        }
        writeln!(out, "runtime up on {h}: canary passed")?;
        let since = e.ts.saturating_sub(cfg.limits.runtime_down_minutes * 60);
        let mut held: Vec<(&str, u64)> = (ev.iter())
            .filter(|f| f.ts >= since && f.kind == "respawn" && f.str("failed_on") == Some(h))
            .filter_map(|f| Some((f.fleet.as_str(), f.issue?)))
            .collect();
        held.sort_unstable();
        held.dedup();
        for (fleet, n) in held {
            append(
                &ctx.state,
                &Event::new(ctx.now, fleet, Some(n), "runtime-cleared", json!({"host": h})),
            )?;
        }
        let d = json!({"host": h, "down_since": e.ts});
        append(&ctx.state, &Event::new(ctx.now, &e.fleet, None, "runtime-up", d))?;
    }
    Ok(())
}

const UNLANDED: &str = "PROMPTS_UNLANDED";

/// True while drive holds every respawn: `runtime_down_failures` lanes' prompts never landed in `runtime_down_minutes` (trips it once: one `needs-human` naming the shed builds); a landed spawn clears it.
pub fn unlanded(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<bool> {
    let ev = working(&ctx.state)?;
    let unconfirmed = |e: &Event| e.kind == "spawn" && e.data.get("confirmed") == Some(&json!(false));
    let last = |k: &dyn Fn(&Event) -> bool| ev.iter().enumerate().rfind(|(_, e)| k(e));
    let trip = last(&|e| e.kind == "needs-human" && e.str("code") == Some(UNLANDED));
    let clear = last(&|e| e.kind == "spawn" && !unconfirmed(e));
    if trip.is_some_and(|t| clear.is_none_or(|c| c.0 < t.0)) {
        return Ok(true);
    }
    let l = &cfg.limits;
    let since = (trip.map_or(0, |t| t.1.ts + 1)).max(ctx.now.saturating_sub(l.runtime_down_minutes * 60));
    let mut lanes: Vec<&str> = (ev.iter().filter(|e| e.ts >= since && unconfirmed(e)))
        .filter_map(|e| e.str("agent"))
        .collect();
    lanes.sort_unstable();
    lanes.dedup();
    if u64::try_from(lanes.len()).unwrap_or(u64::MAX) < l.runtime_down_failures {
        return Ok(false);
    }
    let reason = format!(
        "{} lanes' prompts never landed in {} min ({}); drive respawns none until a prompt lands; if shed is at fault, {}",
        lanes.len(),
        l.runtime_down_minutes,
        crate::shed::builds(r),
        crate::shed::UPDATE
    );
    writeln!(out, "{fleet} needs-human: {reason}")?;
    let d = json!({"code": UNLANDED, "reason": reason, "lanes": lanes});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, None, "needs-human", d))?;
    Ok(true)
}
