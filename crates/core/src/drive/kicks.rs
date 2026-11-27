use super::Lock;
use crate::config::Config;
use crate::events::Json;
use crate::events::{append, Event};
use crate::runner::{ok, Runner};
use crate::Ctx;
use anyhow::Result;
use serde_json::json;
use std::io::Write;

pub(super) fn in_flight(ctx: &Ctx, out: &mut dyn Write, (fleet, n): (&str, u64), why: &str) -> Result<()> {
    let e = Event::new(ctx.now, fleet, Some(n), "kick-in-flight", json!({"why": why}));
    append(&ctx.state, &e.passive())?;
    Ok(writeln!(
        out,
        "kick {fleet} #{n}: one runs already; it drives this one after it"
    )?)
}

pub(super) fn launched(why: &str) -> bool {
    matches!(why, "drive" | "plan")
}

fn runner(fleet: &str) -> String {
    format!("kicks-{fleet}.lock")
}

pub(super) fn queue(ctx: &Ctx, (fleet, n): (&str, u64), why: &str, id: Option<&str>) {
    let (key, ttl) = (
        format!("kick-{fleet}-{n}.queued"),
        crate::config::limits().spawn_pending_max_secs,
    );
    let d = json!({"why": why, "id": id}).to_string();
    drop(crate::db::take(&ctx.state, &key, ctx.now, ttl, &d, &|_| false));
}

pub(super) fn queued(ctx: &Ctx, fleet: &str, take: bool) -> Vec<(u64, String, Option<String>)> {
    let pre = format!("kick-{fleet}-");
    let item = |l: crate::db::Lease| {
        let n = l.key.strip_prefix(&pre)?.strip_suffix(".queued")?.parse().ok()?;
        if l.expires_at <= ctx.now || take && !crate::db::release(&ctx.state, &l.key, Some(&l)) {
            return None;
        }
        let v: serde_json::Value = serde_json::from_str(&l.data).unwrap_or_default();
        let why = v.at("why").as_str().unwrap_or("kick").to_string();
        Some((n, why, v.at("id").as_str().map(String::from)))
    };
    crate::db::leases(&ctx.state, &pre)
        .into_iter()
        .filter_map(item)
        .collect()
}

pub(super) fn starting(ctx: &Ctx, r: &dyn Runner, fleet: &str) -> Option<crate::db::Lease> {
    let ttl = crate::config::limits().kick_max_secs;
    crate::db::take(&ctx.state, &runner(fleet), ctx.now, ttl, "starting", &|l| {
        super::gone(r, l, ctx.now)
    })
}

pub(super) fn started(ctx: &Ctx, l: &crate::db::Lease, pid: Option<u32>) {
    match pid {
        Some(pid) => drop(crate::db::renew(&ctx.state, l, &crate::db::Lease { pid, ..l.clone() })),
        None => drop(crate::db::release(&ctx.state, &l.key, Some(l))),
    }
}

pub(super) fn claim(ctx: &Ctx, r: &dyn Runner, fleet: &str) -> Option<Lock> {
    let (key, ttl) = (runner(fleet), crate::config::limits().kick_max_secs);
    let me = crate::db::Lease {
        key: key.clone(),
        pid: crate::sim::pid(),
        host: crate::db::host(),
        at: ctx.now,
        expires_at: ctx.now + ttl,
        data: "running".into(),
    };
    let handed = super::held(ctx, &key).filter(|l| l.data == "starting");
    if handed.is_some_and(|l| crate::db::renew(&ctx.state, &l, &me)) {
        return Some(Lock(ctx.state.clone(), me));
    }
    super::take(ctx, r, &key, ttl, "running")
}

pub(super) fn drain(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    fleet: &str,
    run: Lock,
) -> Result<()> {
    let (t0, mut run, mut waited, mut told) = (crate::now(), Some(run), false, std::collections::BTreeSet::new());
    for _ in 0..30 {
        let (ctx, round) = (&later(ctx, t0), crate::now());
        let due = queued(ctx, fleet, true);
        if due.is_empty() && !waited {
            r.sleep(10);
            waited = true;
            continue;
        }
        if due.is_empty() {
            drop(run.take());
            if queued(ctx, fleet, false).is_empty() {
                return Ok(());
            }
            let Some(l) = claim(ctx, r, fleet) else {
                return Ok(());
            };
            run = Some(l);
            continue;
        }
        waited = false;
        let mut busy = false;
        for (n, why, id) in due {
            if let Some(Lock(_, l)) = run.as_mut() {
                let secs = cfg.limits.kick_max_secs + cfg.limits.gate_check_minutes * 60;
                let renewed = crate::db::Lease {
                    expires_at: ctx.now + crate::now().saturating_sub(round) + secs,
                    ..l.clone()
                };
                if crate::db::renew(&ctx.state, l, &renewed) {
                    *l = renewed;
                }
            }
            match one(ctx, cfg, r, out, (fleet, n), (&why, id.as_deref())) {
                Ok(true) => {
                    if told.insert(n) {
                        in_flight(ctx, out, (fleet, n), &why)?;
                    }
                    queue(ctx, (fleet, n), &why, None);
                    busy = true;
                }
                Ok(false) => {}
                Err(e) => writeln!(out, "kick {fleet} #{n}: {e:#}")?,
            }
        }
        if busy {
            r.sleep(10);
        }
    }
    Ok(())
}

pub(super) fn overdue(ctx: &Ctx, cfg: &Config, r: &dyn Runner, l: &crate::db::Lease, remove: bool) -> bool {
    let (age, max) = (ctx.now.saturating_sub(l.at), super::job::limit(ctx, cfg, l));
    let id = l.key.strip_prefix("kick-").and_then(|k| k.strip_suffix(".lock"));
    let Some((fleet, n)) = id.and_then(|i| i.rsplit_once('-')).filter(|_| age > max) else {
        return false;
    };
    if remove && l.pid != 0 && l.host == crate::db::host() && !super::gone(r, l, ctx.now) {
        let killed = ok(r, "kill", &["-TERM", &format!("-{}", l.pid)]).is_ok();
        let d = json!({"pid": l.pid, "age": age, "limit": max, "killed": killed});
        append(
            &ctx.state,
            &Event::new(ctx.now, fleet, n.parse().ok(), "kick-killed", d).passive(),
        )
        .ok();
    }
    true
}

fn later(ctx: &Ctx, t0: u64) -> Ctx {
    Ctx {
        now: ctx.now + crate::now().saturating_sub(t0),
        ..ctx.clone()
    }
}

pub(super) fn one(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, n): (&str, u64),
    (why, id): (&str, Option<&str>),
) -> Result<bool> {
    if let Some(id) = id {
        if !crate::db::processed(&ctx.state, &format!("{fleet}#{n}:{why}:{id}"), ctx.now)? {
            writeln!(out, "kick {why} {id}: already processed")?;
            return Ok(false);
        }
    }
    let ttl = cfg.limits.spawn_pending_max_secs;
    let pending = (why == "drive").then(|| super::held(ctx, &format!("spawn-{fleet}-{n}.pending")));
    let pending = pending.flatten().map(|l| Lock(ctx.state.clone(), l));
    let Some(_k) = super::take(ctx, r, &format!("kick-{fleet}-{n}.lock"), ttl, fleet) else {
        return Ok(true);
    };
    let _slot = match pending.is_none().then(|| super::job::slot(ctx, cfg, r, fleet)) {
        Some(None) => {
            writeln!(
                out,
                "kick {fleet} #{n}: every job_max slot of {fleet} is busy; not driven"
            )?;
            return Ok(false);
        }
        slot => slot.flatten(),
    };
    let mut buf = vec![];
    super::KICK.set(Some(n));
    super::SLOT.set(pending.is_some());
    let res = super::drive(ctx, cfg, r, &mut buf, fleet, false);
    super::KICK.set(None);
    super::SLOT.set(false);
    drop(pending);
    let text = String::from_utf8_lossy(&buf).into_owned();
    out.write_all(&buf)?;
    let busy = text.starts_with("drive: ");
    if !busy {
        let d = json!({"why": why, "result": super::summary(fleet, &text)});
        append(&ctx.state, &Event::new(ctx.now, fleet, Some(n), "kick", d))?;
    }
    res.map(|()| busy)
}
