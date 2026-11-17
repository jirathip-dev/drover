use super::{pulse, Cause, Exit, Life, Session, Sessions, Status};
use crate::config::Config;
use crate::events::{append, last_spawn, Event, Json};
use crate::hosts::On;
use crate::runner::Runner;
use crate::Ctx;
use anyhow::Result;
use serde_json::{json, Value};
use shed_proto::EventKind;
use std::time::Duration;

/// Lane `agent`'s latest `lane-state` since its last spawn.
pub fn latest<'a>(events: &'a [Event], agent: &str) -> Option<&'a Event> {
    let from = last_spawn(events, agent).map_or(0, |e| e.ts);
    let mine = |e: &&Event| e.kind == "lane-state" && e.str("agent") == Some(agent) && e.ts >= from;
    events.iter().rev().find(mine)
}

/// Since when shed has said lane `agent`'s current status (drover's stamp; shed has no time on it).
pub fn since(events: &[Event], agent: &str) -> Option<u64> {
    latest(events, agent).map(|e| e.data.get("since").and_then(Value::as_u64).unwrap_or(e.ts))
}

/// The status lane `agent`'s latest stamp records (`gone` is none).
pub fn status(events: &[Event], agent: &str) -> Option<Status> {
    latest(events, agent)
        .and_then(|e| e.str("state"))
        .filter(|s| *s != "gone")
        .map(Status::word_of)
}

/// #662: how lane `agent`'s latest session ended, as its `gone` stamp after its last spawn says.
pub fn exit(events: &[Event], agent: &str) -> Option<Exit> {
    let e = latest(events, agent).filter(|e| e.str("state") == Some("gone"))?;
    Some(Exit {
        code: e
            .data
            .get("code")
            .and_then(Value::as_i64)
            .and_then(|c| i32::try_from(c).ok()),
        cause: Cause::parse(e.str("cause")?)?,
        note: e.str("note").map(String::from),
    })
}

fn whose(cfg: &Config, labels: &std::collections::BTreeMap<String, String>) -> Option<(String, Option<u64>)> {
    let fleet = labels.get("fleet").filter(|f| cfg.fleet.contains_key(*f))?;
    let issue = labels.get("issue").and_then(|i| i.parse().ok());
    (issue.is_some() || labels.contains_key("thread")).then(|| (fleet.clone(), issue))
}

fn stamp(ctx: &Ctx, events: &[Event], (fleet, issue): (&str, u64), agent: &str, mut d: Value) -> Result<bool> {
    let was = latest(events, agent);
    let same = |k: &str| was.is_some_and(|e| e.val(k) == d.at(k));
    if same("state") && same("note") && same("cause") {
        return Ok(false);
    }
    let word = |v: &Value| v.as_str().filter(|s| *s != "gone").map(Status::word_of);
    let kept = was.filter(|e| word(e.val("state")).is_some() && word(e.val("state")) == word(d.at("state")));
    let at = kept.map_or(ctx.now, |e| e.data.get("since").and_then(Value::as_u64).unwrap_or(e.ts));
    d.put("agent", json!(agent)).put("since", json!(at));
    append(&ctx.state, &Event::new(ctx.now, fleet, Some(issue), "lane-state", d))?;
    Ok(true)
}

fn revive(ctx: &Ctx, events: &[Event], (fleet, issue): (&str, u64), sp: &Event) -> Result<()> {
    let agent = sp.str("agent").unwrap_or_default();
    let mine = |e: &&Event| {
        e.kind == "lane-dead" && e.ts >= sp.ts && e.str("key").is_some_and(|k| k.ends_with(&format!(":{agent}")))
    };
    let key = events
        .iter()
        .rfind(mine)
        .map(|d| format!("alive:{}", d.str("key").unwrap_or_default()));
    let new = key.filter(|k| !events.iter().any(|e| e.kind == "lane-alive" && e.str("key") == Some(k)));
    let Some(key) = new else { return Ok(()) };
    let d = json!({"key": key, "agent": agent, "reason": format!("lane {agent} runs again on its host")});
    append(&ctx.state, &Event::new(ctx.now, fleet, Some(issue), "lane-alive", d))
}

fn gone(ctx: &Ctx, cfg: &Config, (fleet, issue): (&str, u64), agent: &str, exit: &Exit, mut d: Value) -> Result<bool> {
    let events = crate::events::working(&ctx.state)?;
    if latest(&events, agent).is_some_and(|e| e.str("state") == Some("gone")) {
        return Ok(false);
    }
    d.put("state", json!("gone"))
        .put("code", json!(exit.code))
        .put("cause", json!(exit.cause.word()));
    if let Some(n) = &exit.note {
        d.put("note", json!(n));
    }
    stamp(ctx, &events, (fleet, issue), agent, d)?;
    if exit.cause == Cause::Limit {
        let events = crate::events::working(&ctx.state)?;
        if let Some(sp) = last_spawn(&events, agent) {
            let d = crate::hosts::hold_alert(&events, sp, "hit the usage limit");
            if !events
                .iter()
                .any(|x| x.kind == "usage-limit" && x.val("key") == d.at("key"))
            {
                crate::events::emit(&ctx.state, cfg, Event::new(ctx.now, "drover", None, "usage-limit", d))?;
            }
        }
    }
    Ok(true)
}

/// One shed event into the log: its pulse, and its lane's `lane-state` stamp (#529: an exit or kill is `gone`, #662: with its cause); whether it stamped.
pub fn on_event(ctx: &Ctx, cfg: &Config, e: &shed_proto::Event) -> Result<bool> {
    let Some((fleet, issue)) = whose(cfg, &e.labels) else {
        return Ok(false);
    };
    if last_spawn(&crate::events::working(&ctx.state)?, &e.name).is_some_and(|sp| sp.str("machine").is_some()) {
        return Ok(false);
    }
    pulse::hear(ctx, &e.name, &e.kind, e.replay)?;
    let Some(issue) = issue else {
        return Ok(false);
    };
    let at = (fleet.as_str(), issue);
    let seq = json!({"shed_seq": e.seq});
    match &e.kind {
        EventKind::State(s) => {
            let mut d = json!({"state": s.state, "note": s.note});
            d.put("shed_seq", seq.at("shed_seq").clone());
            stamp(ctx, &crate::events::working(&ctx.state)?, at, &e.name, d)
        }
        EventKind::Exited { code } => {
            let open = pulse::update(ctx, &e.name, |p| *p = None)?.is_some_and(|p| p.turn_open);
            let last = status(&crate::events::working(&ctx.state)?, &e.name).unwrap_or(Status::Unreported);
            let cause = if open && last != Status::Limited {
                Cause::Crash
            } else {
                Cause::of(*code, last)
            };
            let exit = Exit {
                code: *code,
                cause,
                note: None,
            };
            gone(
                ctx,
                cfg,
                at,
                &e.name,
                &exit,
                json!({"why": "exited", "shed_seq": e.seq}),
            )
        }
        EventKind::Killed => {
            pulse::update(ctx, &e.name, |p| *p = None)?;
            let exit = Exit {
                code: None,
                cause: Cause::Killed,
                note: None,
            };
            gone(
                ctx,
                cfg,
                at,
                &e.name,
                &exit,
                json!({"why": "killed", "shed_seq": e.seq}),
            )
        }
        _ => Ok(false),
    }
}

fn word(s: Status) -> Option<&'static str> {
    match s {
        Status::Unreported => None,
        Status::Other => Some("stuck"),
        _ => Some(s.word()),
    }
}

fn look(ctx: &Ctx, cfg: &Config, r: &dyn Runner, events: &[Event], m: Option<&str>, s: &Session) -> Result<bool> {
    let (Some((fleet, Some(issue))), Some(sp)) = (whose(cfg, &s.labels), last_spawn(events, &s.name)) else {
        return Ok(false);
    };
    if sp.str("machine") != m {
        if !s.running() {
            let on = On::new(r, m);
            if let Some(Err(e)) = on.shed().map(|b| crate::term::Shed(b).rm(&on, &s.name)) {
                crate::stderr!("drover: {} on its old host: {e:#}", s.name);
            }
        }
        return Ok(false);
    }
    let at = (fleet.as_str(), issue);
    match s.life {
        Life::Running(st) => {
            revive(ctx, events, at, sp)?;
            let Some(w) = word(st) else { return Ok(false) };
            let was = status(events, &s.name);
            if was == Some(st) {
                return Ok(false);
            }
            stamp(ctx, events, at, &s.name, json!({"state": w, "note": s.note}))
        }
        Life::Exited { code, last } => {
            let exit = Exit {
                code,
                cause: Cause::of(code, last),
                note: s.note.clone(),
            };
            gone(ctx, cfg, at, &s.name, &exit, json!({"why": "exited"}))
        }
    }
}

/// A pass's look at every lane session on its spawn's host: a changed status or an exit stamped into the log as `lane-state`; how many. #1225: an exited one of the same name on another host is removed.
pub fn observe(ctx: &Ctx, cfg: &Config, r: &dyn Runner, sessions: &Sessions) -> Result<usize> {
    let mut n = 0;
    for (m, got) in &sessions.0 {
        for s in got.iter().flatten() {
            let events = crate::events::working(&ctx.state)?;
            n += usize::from(look(ctx, cfg, r, &events, m.as_deref(), s)?);
        }
    }
    Ok(n)
}

/// #298: drive's intake of shed's events since the last pass (`shed.seq` beside the log), each through `on_event`; how many stamped. #529: a running service follows the stream and keeps `shed.seq`.
pub fn intake(ctx: &Ctx, cfg: &Config, w: &crate::shed::Wire) -> Result<usize> {
    let got = crate::shed::events(
        w,
        crate::shed::seen(ctx),
        Duration::from_millis(cfg.limits.shed_read_millis),
    )?;
    let mut n = 0;
    for e in &got {
        n += usize::from(on_event(ctx, cfg, e)?);
    }
    if let Some(last) = got.iter().map(|e| e.seq).max() {
        std::fs::write(ctx.state.with_file_name("shed.seq"), last.to_string())?;
    }
    Ok(n)
}
