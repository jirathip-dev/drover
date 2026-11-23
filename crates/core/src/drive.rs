use crate::config::Config;
use crate::events::{append, emit, fold, working, Event, IssueState, Json};
use crate::runner::{ok, Runner};
use crate::session::Session;
use crate::waiting::Waiting;
use crate::{gh, queue, stall, waiting, Ctx};
use anyhow::Result;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

pub(crate) mod job;
mod kicks;
mod pickup;
mod remote;
mod undriven;

/// A fleet pass's steps in order (`drive-<fleet>.lock.step` names the running one, `pass-slow` the one its deadline hit) and the `[limits]` keys that time them.
pub const STEPS: [(&str, &[&str]); 4] = [
    (
        "look, repair",
        &[
            "pass_max_secs",
            "ssh_quick_secs",
            "ssh_call_secs",
            "import_max_age_hours",
        ],
    ),
    ("closed issues", &[]),
    (
        "PR reads, gates, steps",
        &[
            "webhook_poll_minutes",
            "silent_minutes",
            "ci_start_minutes",
            "ci_stuck_minutes",
            "gate_check_minutes",
            "review_rounds",
            "update_rounds",
            "job_max",
        ],
    ),
    (
        "spawns",
        &[
            "max_parallel_spawns",
            "start_max",
            "start_release",
            "spawn_pending_max_secs",
            "admit_wait_secs",
            "respawn_max",
            "respawn_window_minutes",
            "overflow_after_minutes",
        ],
    ),
];
const STALE_LOCK_SECS: u64 = 3600;

pub(crate) struct Lock(std::path::PathBuf, crate::db::Lease);
impl Drop for Lock {
    fn drop(&mut self) {
        crate::db::release(&self.0, &self.1.key, Some(&self.1));
    }
}

fn take(ctx: &Ctx, r: &dyn Runner, key: &str, secs: u64, data: &str) -> Option<Lock> {
    let now = ctx.now;
    let dead = |l: &crate::db::Lease| gone(r, l, now);
    crate::db::take(&ctx.state, key, now, secs, data, &dead).map(|l| Lock(ctx.state.clone(), l))
}

fn held(ctx: &Ctx, key: &str) -> Option<crate::db::Lease> {
    let mut all = crate::db::leases(&ctx.state, key).into_iter();
    all.find(|l| l.key == key && l.expires_at > ctx.now)
}

fn gone(r: &dyn Runner, l: &crate::db::Lease, now: u64) -> bool {
    l.pid != 0 && l.host == crate::db::host() && !alive(r, &l.pid.to_string(), now.saturating_sub(l.at))
}

fn alive(r: &dyn Runner, pid: &str, age: u64) -> bool {
    let up = |s: String| {
        let (d, hms) = s
            .trim()
            .split_once('-')
            .map_or(Some((0, s.trim())), |(d, t)| Some((d.parse::<u64>().ok()?, t)))?;
        Some(
            d * 86400
                + hms
                    .split(':')
                    .try_fold(0, |a, x| Some(a * 60 + x.parse::<u64>().ok()?))?,
        )
    };
    let up = || ok(r, "ps", &["-o", "etime=", "-p", pid]).ok().and_then(up);
    ok(r, "kill", &["-0", pid]).is_ok() && up().is_none_or(|up| up + 5 >= age)
}

/// Each spawn marker and kick lock (leases), live or stale: stale once its pid is gone (or reused) or it expired. `remove`: release each stale one, log `stale-marker`.
pub fn markers(ctx: &Ctx, cfg: &Config, r: &dyn Runner, remove: bool) -> (Vec<String>, Vec<Event>) {
    let (mut live, mut stale) = (vec![], vec![]);
    let mut all = crate::db::leases(&ctx.state, "spawn-");
    all.extend(crate::db::leases(&ctx.state, "kick-"));
    let ev = crate::events::working(&ctx.state).unwrap_or_default();
    for l in all {
        let name = l.key.clone();
        let spawn = name.strip_prefix("spawn-").and_then(|n| n.strip_suffix(".pending"));
        let Some(id) = spawn.or_else(|| name.strip_prefix("kick-")?.strip_suffix(".lock")) else {
            continue;
        };
        let age = ctx.now.saturating_sub(l.at);
        if !kicks::overdue(ctx, cfg, r, &l, remove) && holds(ctx, cfg, r, &ev, &l) {
            live.push(name);
            continue;
        }
        let (fleet, n) = id.rsplit_once('-').map_or((id, None), |(f, n)| (f, n.parse().ok()));
        let e = Event::new(
            ctx.now,
            fleet,
            n,
            "stale-marker",
            json!({"file": name, "pid": l.pid, "age": age}),
        );
        if remove && crate::db::release(&ctx.state, &l.key, Some(&l)) {
            append(&ctx.state, &e).ok();
        }
        stale.push(e);
    }
    (live, stale)
}

fn holds(ctx: &Ctx, cfg: &Config, r: &dyn Runner, ev: &[Event], l: &crate::db::Lease) -> bool {
    l.expires_at > ctx.now
        && ctx.now.saturating_sub(l.at) <= cfg.limits.spawn_pending_max_secs
        && !gone(r, l, ctx.now)
        && !crate::starts::ended(ev, l)
}

fn starting(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> Vec<crate::db::Lease> {
    let ev = crate::events::working(&ctx.state).unwrap_or_default();
    let mut v: Vec<_> = (crate::db::leases(&ctx.state, "spawn-").into_iter())
        .filter(|l| l.key.ends_with(".pending") && holds(ctx, cfg, r, &ev, l))
        .collect();
    v.sort_by_key(|l| l.at);
    v
}

/// What holds the spawn slots, each marker's key and age (`spawn-a-1.pending 4m, …`); empty when none does.
pub fn held_by(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> String {
    let at = |l: &crate::db::Lease| format!("{} {}", l.key, crate::events::age(ctx.now.saturating_sub(l.at)));
    starting(ctx, cfg, r).iter().map(at).collect::<Vec<_>>().join(", ")
}

/// `spawns starting N/<max>` and the stale markers swept in the last day: doctor's and the Host tab's line.
pub fn spawns_line(ctx: &Ctx, cfg: &Config, r: &dyn Runner, events: &[Event]) -> String {
    let n = markers(ctx, cfg, r, false)
        .0
        .iter()
        .filter(|m| m.starts_with("spawn-"))
        .count();
    let day = |e: &&Event| e.kind == "stale-marker" && ctx.now < e.ts + 86400;
    let mut seen: Vec<(&str, usize)> = vec![];
    for f in events.iter().filter(day).filter_map(|e| e.str("file")) {
        match seen.iter_mut().find(|s| s.0 == f) {
            Some(s) => s.1 += 1,
            None => seen.push((f, 1)),
        }
    }
    let more = seen.len().saturating_sub(5);
    let mut gone: Vec<String> = seen
        .iter()
        .take(5)
        .map(|(f, n)| if *n > 1 { format!("{f} ×{n}") } else { f.to_string() })
        .collect();
    gone.extend((more > 0).then(|| format!("+{more} more")));
    let gone = (!gone.is_empty()).then(|| format!(" · stale removed (24 h): {}", gone.join(", ")));
    format!(
        "spawns starting {n}/{}{}",
        cfg.limits.max_parallel_spawns,
        gone.unwrap_or_default()
    )
}

/// The last fast step: (started, secs) from `drive-fast.at`.
pub fn fast_step(ctx: &Ctx) -> Option<(u64, Option<f64>)> {
    let s = std::fs::read_to_string(ctx.state.with_file_name("drive-fast.at")).ok()?;
    let mut w = s.split_whitespace();
    Some((w.next()?.parse().ok()?, w.next().and_then(|x| x.parse().ok())))
}

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

/// A pass's lines as one toast, per action (`merged #1, #2 · started #3`), then `2 skipped (<codes>)`.
pub fn summary(fleet: &str, out: &str) -> String {
    if out.starts_with("drive: ") {
        return out.trim().into();
    }
    let (mut by, mut skip) = (BTreeMap::<&str, Vec<&str>>::new(), vec![]);
    for (head, res) in out.lines().filter_map(|l| l.split_once(": ")) {
        let (step, mut w) = (head.rsplit(' ').next().unwrap_or(head), res.split([' ', ':']));
        let past = if res == "pending" {
            &"pending"
        } else {
            PAST.iter().find(|d| d.starts_with(step)).unwrap_or(&step)
        };
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
    format!("{fleet}: {}", parts.join(" · ").replace("  ", " ").trim_end())
}
const PAST: [&str; 3] = ["started", "merged", "reviewed"];

thread_local! {
    static ACTS: RefCell<BTreeMap<String, Vec<Value>>> = const { RefCell::new(BTreeMap::new()) };
    static KICK: Cell<Option<u64>> = const { Cell::new(None) };
    static IN_ACT: Cell<bool> = const { Cell::new(false) };
    static ROOM: RefCell<(bool, Option<(&'static str, String)>)> = const { RefCell::new((false, None)) };
    static SLOT: Cell<bool> = const { Cell::new(false) };
}

/// A pass's own command runs (its `clean` leaves disabled hosts alone).
pub fn in_act() -> bool {
    IN_ACT.get()
}

/// `drover kick`: drive `issue`'s due steps now in a detached `kick --now` (none from a lane on another host), output to `kick.log`; a no-op inside a pass's own command. the detached kick's pid; in its own session.
pub fn kick(ctx: &Ctx, r: &dyn Runner, fleet: &str, issue: u64, why: &str) -> Option<u32> {
    kick_once(ctx, r, (fleet, issue), why, None)
}

/// `kick`, carrying its trigger's `id` (a verdict's sha, a decision): the same trigger is driven once.
pub fn kick_once(ctx: &Ctx, r: &dyn Runner, (fleet, issue): (&str, u64), why: &str, id: Option<&str>) -> Option<u32> {
    let mut a = crate::runner::argv(&["kick", fleet, &issue.to_string(), "--why", why]);
    a.extend(id.into_iter().flat_map(|i| ["--id".to_string(), i.into()]));
    if IN_ACT.get() || crate::outbox::lane().is_some() {
        return None;
    }
    if let Some(pid) = crate::service::kick(ctx, fleet, issue, why, id) {
        return Some(pid);
    }
    a.insert(5, "--now".into());
    r.detach(&ctx.exe, &a, &state_dir(ctx).join("kick.log")).ok()
}

/// `kick --now`: one pass of `issue` alone under the drive lock. A kick while one runs, or within the 10 s after it, is coalesced into one follow-up; a busy lock is retried as one too.
pub fn kicked(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, k: Kicked) -> Result<()> {
    let (fleet, n, why, now, id) = k;
    if !now {
        cfg.fleet(fleet)?;
        kick_once(ctx, r, (fleet, n), why, id);
        return Ok(());
    }
    if let Some(id) = id {
        if !crate::db::processed(&ctx.state, &format!("{fleet}#{n}:{why}:{id}"), ctx.now)? {
            return Ok(writeln!(out, "kick {why} {id}: already processed")?);
        }
    }
    let (again, ttl) = (format!("kick-{fleet}-{n}.again"), cfg.limits.spawn_pending_max_secs);
    let pending = (why == "drive").then(|| held(ctx, &format!("spawn-{fleet}-{n}.pending")));
    let mut pending = pending.flatten().map(|l| Lock(ctx.state.clone(), l));
    let flag = || crate::db::take(&ctx.state, &again, ctx.now, ttl, why, &|_| true).map(drop);
    let Some(k) = take(ctx, r, &format!("kick-{fleet}-{n}.lock"), ttl, fleet) else {
        flag();
        return kicks::in_flight(ctx, out, (fleet, n), why);
    };
    let _slot = match pending.is_none().then(|| job::slot(ctx, cfg, r, fleet)) {
        Some(None) => {
            return Ok(writeln!(
                out,
                "kick {fleet} #{n}: every job_max slot of {fleet} is busy; not driven"
            )?)
        }
        slot => slot.flatten(),
    };
    let start = crate::now();
    for _ in 0..30 {
        let ctx = &Ctx {
            now: ctx.now + crate::now().saturating_sub(start),
            ..ctx.clone()
        };
        let expires_at = ctx.now + ttl;
        crate::db::renew(
            &ctx.state,
            &k.1,
            &crate::db::Lease {
                expires_at,
                ..k.1.clone()
            },
        );
        let mut buf = vec![];
        KICK.set(Some(n));
        SLOT.set(pending.is_some());
        let res = drive(ctx, cfg, r, &mut buf, fleet, false);
        KICK.set(None);
        SLOT.set(false);
        drop(pending.take());
        let text = String::from_utf8_lossy(&buf).into_owned();
        out.write_all(&buf)?;
        if text.starts_with("drive: ") {
            flag();
        } else {
            let d = json!({"why": why, "result": summary(fleet, &text)});
            append(&ctx.state, &Event::new(ctx.now, fleet, Some(n), "kick", d))?;
        }
        res?;
        r.sleep(10);
        if !crate::db::release(&ctx.state, &again, None) {
            break;
        }
    }
    Ok(())
}

/// `kick --now`'s fleet, issue, why, `--now` and trigger id.
pub type Kicked<'a> = (&'a str, u64, &'a str, bool, Option<&'a str>);

fn launch(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    ev: &[Event],
    (fleet, n): (&str, u64),
    c: &[&str],
    start: bool,
) -> Result<i32> {
    let (l, mut d) = admitted(ctx, cfg, r, ev, (fleet, n), c, start)?;
    let ev = |d: &Value| serde_json::to_string(&Event::new(ctx.now, fleet, Some(n), "spawn", d.clone()));
    let pid = kick(ctx, r, fleet, n, "drive");
    d.put("pid", pid.into());
    let data = if pid.is_some() { ev(&d)? } else { l.data.clone() };
    let pid = pid.unwrap_or(0);
    crate::db::renew(&ctx.state, &l, &crate::db::Lease { pid, data, ..l.clone() });
    Ok(0)
}

fn admitted(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    ev: &[Event],
    (fleet, n): (&str, u64),
    c: &[&str],
    start: bool,
) -> Result<(crate::db::Lease, Value)> {
    let p: &dyn Runner = &remote::Memo::here(r);
    let role = c.get(3).copied().unwrap_or_default();
    markers(ctx, cfg, r, true);
    let low =
        |e: &Event| crate::host::free_gb(&crate::hosts::On::of(p, Some(e))).is_some_and(|g| g < cfg.limits.min_free_gb);
    let bound = |p: &Event| {
        ev.iter()
            .rev()
            .find(|e| p.issue.is_some_and(|n| e.is("spawn", &p.fleet, n)))
    };
    let full: Vec<_> = (pending(ctx, cfg, r).into_iter())
        .filter(|p| bound(p).is_some_and(low))
        .map(|p| (p.fleet, p.issue))
        .collect();
    let others = crate::starts::others(ctx, cfg, ev, fleet);
    let counts = |p: &&Event| !full.iter().any(|f| f.0 == p.fleet && f.1 == p.issue);
    let me = (fleet, cfg.fleet(fleet)?.weight.unwrap_or(1).max(1), others);
    let by = || {
        Some(held_by(ctx, cfg, r))
            .filter(|b| !b.is_empty())
            .map(|b| format!(": {b}"))
            .unwrap_or_default()
    };
    let starting = |all: &[Event]| {
        if all.iter().any(|e| e.fleet == fleet && e.issue == Some(n)) {
            return crate::gate::refused("SPAWN_PENDING", format!("#{n} is starting{}", by()));
        }
        Ok(())
    };
    let skip = |all: &[Event], h: &str| {
        let live: Vec<&Event> = all.iter().filter(counts).collect();
        crate::starts::skip(ctx, (cfg, p), ev, (all, &live), (me, start, n, role), h)
    };
    let all = pending(ctx, cfg, r);
    starting(&all)?;
    let live: Vec<&Event> = all.iter().filter(counts).collect();
    if let Some(held) = crate::starts::every_full(cfg, &live, me, start) {
        return crate::gate::refused("SPAWN_PENDING", held + &by());
    }
    crate::spawn::lane_admit(ctx, (cfg, r), ev, (fleet, n, role), false)?;
    let placed = crate::hosts::place(ctx, cfg, p, ev, (fleet, n, role), (None, &|h| skip(&all, h)))?;
    if let Some((code, why)) = room(ctx, cfg, r) {
        return crate::gate::refused(code, why);
    }
    let host = placed.as_ref().map_or("local", |p| p.name.as_str());
    let _admit = admit(ctx, cfg, r, host)?;
    let all = pending(ctx, cfg, r);
    starting(&all)?;
    if let Some((code, why)) = skip(&all, host) {
        return crate::gate::refused(code, why + &by());
    }
    let mut d = json!({"role": role, "check": cfg.fleet(fleet)?.flow().is_check(role)});
    if let Some(p) = placed {
        d.put("host", p.name.into());
    }
    let (key, ttl) = (format!("spawn-{fleet}-{n}.pending"), cfg.limits.spawn_pending_max_secs);
    let ev = |d: &Value| serde_json::to_string(&Event::new(ctx.now, fleet, Some(n), "spawn", d.clone()));
    let l = crate::db::take(&ctx.state, &key, ctx.now, ttl, &ev(&d)?, &|_| true);
    let l = l.ok_or_else(|| anyhow::anyhow!("cannot lease {key}"))?;
    Ok((l, d))
}

/// The spawn marker this kick holds for `fleet` #`n`, if it holds one.
pub fn own_marker(ctx: &Ctx, fleet: &str, n: u64) -> Option<crate::db::Lease> {
    held(ctx, &format!("spawn-{fleet}-{n}.pending")).filter(|_| SLOT.get())
}

type Admitter = (std::path::PathBuf, u32, String);
static ADMITTING: std::sync::Mutex<std::collections::BTreeSet<Admitter>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());
static ADMITTED: std::sync::Condvar = std::sync::Condvar::new();

struct Admitting(Admitter);
impl Drop for Admitting {
    fn drop(&mut self) {
        ADMITTING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.0);
        ADMITTED.notify_all();
    }
}

fn admit(ctx: &Ctx, cfg: &Config, r: &dyn Runner, host: &str) -> Result<(Lock, Admitting)> {
    let me = (ctx.state.clone(), crate::sim::pid(), host.to_string());
    let held = ADMITTING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut held = (ADMITTED.wait_while(held, |h| h.contains(&me))).unwrap_or_else(std::sync::PoisonError::into_inner);
    held.insert(me.clone());
    drop(held);
    let me = Admitting(me);
    let key = format!("admit-{host}.lock");
    for left in (0..=cfg.limits.admit_wait_secs).rev() {
        let now = crate::now();
        if let Some(l) = crate::db::take(&ctx.state, &key, now, 60, "", &|l| gone(r, l, now)) {
            return Ok((Lock(ctx.state.clone(), l), me));
        }
        if left > 0 {
            r.sleep(1);
        }
    }
    crate::gate::refused("SPAWN_PENDING", format!("another pass is admitting a spawn on {host}"))
}

pub(crate) fn failures<'a>(ev: &'a [Event], fleet: &str, n: u64) -> Vec<&'a Event> {
    let mine = ev.iter().rev().filter(|e| e.fleet == fleet && e.issue == Some(n));
    let mut v: Vec<&Event> = mine
        .take_while(|e| e.kind != "spawn")
        .filter(|e| e.kind == "spawn-error" && !crate::slo::cwd_gone(e.str("error")))
        .filter(|e| !e.str("error").is_some_and(|x| x.contains(crate::hosts::OFFLINE)))
        .collect();
    v.reverse();
    v
}

pub(crate) fn same_run<'a>(f: &[&'a Event]) -> Option<(usize, &'a str)> {
    let last = f.last()?;
    let same = |e: &&&Event| e.str("error") == last.str("error") && e.str("head") == last.str("head");
    Some((f.iter().rev().take_while(same).count(), last.str("error")?))
}

/// A spawn whose last attempt failed with the same error as the one before waits before its next try: 5 min, doubling to 1 h. A first or changed error, or a new push, retries at once (a kick never waits). (secs left, error).
pub fn backoff(ctx: &Ctx, ev: &[Event], fleet: &str, n: u64) -> Option<(u64, String)> {
    let f = failures(ev, fleet, n);
    let (k, error) = same_run(&f)?;
    let last = f.last()?;
    let pushed = gh::cached_pr(ctx, fleet, n).at("headRefOid").as_str() != last.str("head");
    let wait = (300u64 << k.checked_sub(2)?.min(4)).min(3600);
    let left = (last.ts + wait).saturating_sub(ctx.now);
    (!pushed && left > 0).then(|| (left, error.to_string()))
}

/// `retrying in Nm: <error>` while `fleet` #`n` backs off.
pub fn retrying(ctx: &Ctx, ev: &[Event], fleet: &str, n: u64) -> Option<String> {
    backoff(ctx, ev, fleet, n).map(|(left, e)| format!("retrying in {}m: {e}", left.div_ceil(60)))
}

/// Passes run (the fast job started one within 10 min) and `fleet` #`n`'s last try waited only for a spawn slot: the line naming how many failing retries hold them.
pub fn slot_wait(ctx: &Ctx, ev: &[Event], fleet: &str, n: u64) -> Option<String> {
    (ctx.now < fast_step(ctx)?.0 + 600).then_some(())?;
    let pass = |e: &&Event| e.kind == "drive-pass" && e.fleet == fleet;
    let acts = ev.iter().filter(pass).filter_map(|e| e.data.get("actions")?.as_array());
    let last = acts.flatten().rfind(|a| a["issue"] == n)?;
    (last["code"] == "SPAWN_PENDING").then_some(())?;
    let ids: std::collections::BTreeSet<_> = ev.iter().filter_map(|e| Some((&e.fleet, e.issue?))).collect();
    let failing = |(f, i): &(&String, u64)| failures(ev, f, *i).last().is_some_and(|e| ctx.now < e.ts + 3600);
    let k = ids.iter().filter(|x| failing(x)).count();
    Some(crate::promise::Say::SpawnSlot(None).line() + &format!(" ({k} failing retries hold them)"))
}

fn room(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> Option<(&'static str, String)> {
    if cfg.hosts.values().any(|h| h.enabled && h.machine.is_some()) {
        return None;
    }
    ROOM.with_borrow_mut(|s| {
        if !s.0 {
            *s = (true, crate::hosts::gate(ctx, cfg, &crate::hosts::On::new(r, None)).1);
        }
        s.1.clone()
    })
}

pub(crate) fn pending(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> Vec<Event> {
    let live = starting(ctx, cfg, r).into_iter();
    live.filter_map(|l| serde_json::from_str(&l.data).ok()).collect()
}

pub(crate) fn words(code: Option<&str>, step: &str, result: &str) -> String {
    let why = result.split_once(": ").map_or(result, |p| p.1);
    let why = why.split("; retry later").next().unwrap_or(why);
    match code {
        None => PAST.iter().find(|d| d.starts_with(step)).map_or(step, |d| d).into(),
        Some("HOST_BUSY") => format!("Mac busy ({why})"),
        Some("DISK_LOW") => format!("disk nearly full ({why})"),
        Some("CAP") => "lane limit reached".into(),
        Some("LOGIN_NEEDED") => format!("login needed ({why})"),
        Some("CI_NOT_GREEN") => "waiting for CI".into(),
        Some("SPAWN_PENDING") => match why.split_once(": ") {
            Some((_, by)) => crate::promise::Say::SpawnSlot(Some(by.into())).line(),
            None => crate::promise::Say::SpawnSlot(None).line(),
        },
        Some("SPAWN_BACKOFF" | "HOST_FULL") => why.into(),
        Some("SERIAL_BUSY") => "another one-at-a-time issue is in flight".into(),
        Some("IN_FLIGHT" | "ALREADY_DONE") => "skipped: another pass is on it or did it".into(),
        Some("LANE_LIVE") if why.starts_with("stale pane") => why.into(),
        Some("LANE_LIVE") => match why.strip_prefix("finished (").and_then(|w| w.split_once(')')) {
            Some((st, _)) => format!(
                "an old {} ({st}) is in the way: replacing it",
                if step == "review" { "reviewer" } else { "lane" }
            ),
            None => "a lane is already on it".into(),
        },
        Some("ROUND_LIMIT" | "UPDATE_LIMIT") => "out of rounds: needs you".into(),
        Some("EXIT" | "ERROR") => format!("failed: {result}"),
        Some(c) => format!("held back ({c})"),
    }
}

/// The fleet's result line from a `drive-pass`: (tone, `HH:MM · started N · merged N · M waiting: <top reason>`). Red on an error, green if it started or merged something, amber if everything was held back, else plain.
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
    line += &e
        .data
        .get("secs")
        .and_then(Value::as_f64)
        .map_or(String::new(), |s| format!(" · {s} s"));
    let tone = match () {
        () if failed > 0 => "r",
        () if started + merged > 0 => "g",
        () if !held.is_empty() => "a",
        () => "m",
    };
    (tone, line)
}

/// A pass is driving `fleet` now (empty: any fleet).
pub fn running(ctx: &Ctx, fleet: &str) -> bool {
    let name = |l: &crate::db::Lease| l.key.strip_prefix("drive-")?.strip_suffix(".lock").map(str::to_string);
    let pass = |f: &str| f.rsplit('-').next().is_none_or(|n| n.parse::<u64>().is_err());
    let live = crate::db::leases(&ctx.state, "drive-")
        .into_iter()
        .filter(|l| l.expires_at > ctx.now);
    live.filter_map(|l| name(&l))
        .any(|f| pass(&f) && (fleet.is_empty() || f == fleet))
}

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
        Some(c) => format!("{who} {step}: REFUSED {c}: {}", a["why"].as_str().unwrap_or(w)),
    }
}

fn wait(ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write, lock: &str, fleet: &str) -> Result<()> {
    for _ in 0..120 {
        if held(ctx, lock).is_none() {
            break;
        }
        r.sleep(1);
    }
    if held(ctx, lock).is_some() {
        return Ok(writeln!(out, "drive: still driving · result will appear here")?);
    }
    let events = working(&ctx.state)?;
    let done = |e: &&Event| e.kind == "drive-pass" && e.fleet == fleet;
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

/// `drover drive`, the fast job and the tick's: each `[hosts]` machine added to shed, roots, offline hosts, every host with no fresh word polled at once, the runtime canary, stale panes, duplicates and the SLOs, then each fleet's pass started as its own process; it waits for none.
pub fn start(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write) -> Result<()> {
    let t0 = crate::clock::instant();
    std::fs::create_dir_all(state_dir(ctx))?;
    let stamp = ctx.state.with_file_name("drive-fast.at");
    std::fs::write(&stamp, ctx.now.to_string()).ok();
    if let Some((step, secs)) = crate::timer::stuck_step(ctx, cfg) {
        writeln!(
            out,
            "! tick stuck in {step} for {}: drover doctor",
            crate::events::age(secs)
        )?;
    }
    let events = working(&ctx.state)?;
    if let Err(e) = crate::hosts::sample(ctx, cfg, r, &events) {
        writeln!(out, "host history: {e:#}")?;
    }
    if let Some(b) = r.shed() {
        match crate::shed::add_machines(cfg, r) {
            Ok(added) => added
                .iter()
                .try_for_each(|m| writeln!(out, "shed: machine {m} added"))?,
            Err(e) => writeln!(out, "shed machines: {e:#}")?,
        }
        let res = crate::session::stamp::intake(ctx, cfg, &b.wire);
        if let (false, Err(e)) = (crate::shed::version_alert(ctx, cfg, &res)?, &res) {
            writeln!(out, "shed events: {e:#}")?;
        }
    }
    if let Err(e) = crate::pending::requeue_waiting(ctx, cfg) {
        writeln!(out, "requeue extends: {e:#}")?;
    }
    for u in cfg.fleet.keys().flat_map(|f| crate::undelivered(&events, f)) {
        if let Err(e) = crate::deliver(ctx, cfg, r, u, u.str("text").unwrap_or_default(), false) {
            writeln!(out, "redeliver: {e:#}")?;
        }
    }
    if let Err(e) = crate::secret::secrets_left(ctx, r, &events) {
        writeln!(out, "secrets files: {e:#}")?;
    }
    if let Err(e) = crate::roots::watch(ctx, cfg, r, out) {
        writeln!(out, "roots: {e:#}")?;
    }
    if let Err(e) = crate::hosts::evacuate(ctx, cfg, r, out) {
        writeln!(out, "offline hosts: {e:#}")?;
    }
    crate::link::look(ctx, cfg, r, out, true).map_or_else(|e| drop(writeln!(out, "look: {e:#}")), drop);
    let on = |n: &&str| crate::compact::drives(ctx, cfg, r, n, out);
    let fleets: Vec<&str> = cfg.fleet.keys().map(String::as_str).filter(on).collect();
    if !fleets.is_empty() {
        if let Some(_one) = take(ctx, r, "orphans.lock", cfg.limits.pass_max_secs, "") {
            crate::spawn::orphans(ctx, cfg, r, out).map_or_else(|e| drop(writeln!(out, "orphans: {e:#}")), drop);
        }
        crate::breaker::probe(ctx, cfg, r, out).unwrap_or_else(|e| drop(writeln!(out, "runtime canary: {e:#}")));
    }
    crate::check::stale(ctx, cfg, r, out).unwrap_or_else(|e| crate::stderr!("drover: stale panes: {e:#}"));
    crate::check::dedup(ctx, cfg, r, out).unwrap_or_else(|e| crate::stderr!("drover: lane-dup: {e:#}"));
    crate::slo::pass(ctx, cfg, r, out, &fleets).unwrap_or_else(|e| drop(writeln!(out, "slo: {e:#}")));
    if !fleets.is_empty() && r.shed().is_none() {
        anyhow::bail!(crate::config::NO_RUNTIME);
    }
    for f in &fleets {
        start_pass(ctx, r, out, f)?;
    }
    let secs = (t0.elapsed().as_secs_f64() * 10.0).round() / 10.0;
    std::fs::write(&stamp, format!("{} {secs}", ctx.now)).ok();
    Ok(())
}

fn start_pass(ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<()> {
    if held(ctx, &format!("drive-{fleet}.lock")).is_some() || crate::service::wake(ctx, fleet) {
        return Ok(());
    }
    let args = crate::runner::argv(&["drive", fleet]);
    if let Err(e) = r.detach(&ctx.exe, &args, &state_dir(ctx).join("drive.log")) {
        writeln!(out, "{fleet}: pass not started: {e:#}")?;
    }
    Ok(())
}

/// The tick's slow chores beside the passes: the disk budget, lanes hogging the CPU, then each fleet's backlog.
pub fn chores(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write) -> Result<()> {
    if let Err(e) = crate::check::budget(ctx, cfg, r, out) {
        writeln!(out, "budget: {e:#}")?;
    }
    crate::check::hogs(ctx, cfg, r, out).unwrap_or_else(|e| crate::stderr!("drover: lane-hog: {e:#}"));
    for name in cfg.fleet.keys().filter(|n| crate::compact::drives(ctx, cfg, r, n, out)) {
        crate::gh::backlog(ctx, r, cfg.fleet(name)?, name)
            .unwrap_or_else(|e| crate::stderr!("drover: {name} backlog: {e:#}"));
    }
    Ok(())
}

/// `drover drive`: with a fleet, its pass under its own `pass_max_secs` deadline; without, every fleet's pass started.
pub fn job(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: Option<&str>) -> Result<()> {
    let Some(f) = fleet else { return start(ctx, cfg, r, out) };
    let _end = deadline(ctx, cfg, f);
    drive(ctx, cfg, r, out, f, false)
}

fn deadline(ctx: &Ctx, cfg: &Config, fleet: &str) -> std::sync::mpsc::Sender<()> {
    let (end, rx) = std::sync::mpsc::channel::<()>();
    let (state, fleet, secs) = (ctx.state.clone(), fleet.to_string(), cfg.limits.pass_max_secs);
    std::thread::spawn(move || {
        let late = rx.recv_timeout(std::time::Duration::from_secs(secs));
        if late != Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
            return;
        }
        let mark = state.with_file_name(format!("drive-{fleet}.lock.step"));
        let step = std::fs::read_to_string(&mark).unwrap_or_default();
        std::fs::write(
            mark.with_file_name(format!("drive-{fleet}.last")),
            crate::now().to_string(),
        )
        .ok();
        let d = json!({"secs": secs, "step": step.trim(), "ended": true});
        append(&state, &Event::new(crate::now(), &fleet, None, "pass-slow", d)).ok();
        crate::runner::exit(124);
    });
    end
}

/// Fleet `fleet`'s pass under `drive-<fleet>.lock` (a kick's: its issue alone, under the kick's lock); `manual`: a press, which waits out a pass that holds it.
pub fn drive(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str, manual: bool) -> Result<()> {
    cfg.fleet(fleet)?;
    std::fs::create_dir_all(state_dir(ctx))?;
    let lock = format!("drive-{fleet}.lock");
    let _held = match KICK.get() {
        Some(_) => None,
        None => match lease(ctx, r, out, &lock, 2 * cfg.limits.pass_max_secs, fleet)? {
            Some(l) => Some(l),
            None if manual => return wait(ctx, r, out, &lock, fleet),
            None => return Ok(writeln!(out, "drive: another pass holds {lock}; nothing done")?),
        },
    };
    if !crate::compact::drives(ctx, cfg, r, fleet, out) {
        return Ok(());
    }
    let catchup = if KICK.get().is_some() {
        0
    } else {
        cfg.limits.webhook_poll_minutes * 60
    };
    let was = (
        gh::FORCE.replace(KICK.get().is_some()),
        gh::CATCHUP.replace(catchup),
        gh::PROBE.replace(true),
    );
    let file = ctx.state.with_file_name(format!("{lock}.step"));
    let mark = |step: &str| {
        debug_assert!(STEPS.iter().any(|(s, _)| *s == step), "{step} is not in STEPS");
        if KICK.get().is_none() {
            std::fs::write(&file, step).ok();
        }
    };
    mark("look, repair");
    crate::link::look(ctx, cfg, r, out, KICK.get().is_some()).map_or_else(|e| drop(writeln!(out, "look: {e:#}")), drop);
    if let Err(e) = repair(ctx, cfg, r, out, fleet) {
        writeln!(out, "repair: {e:#}")?;
    }
    let seen = crate::snapshot::take(ctx, cfg, r);
    let grew = crate::db::last_seq(&ctx.state);
    let res = one(ctx, cfg, (r, &seen), out, (fleet, manual), &mark);
    if crate::db::last_seq(&ctx.state) != grew {
        crate::snapshot::take(ctx, cfg, r);
    }
    gh::FORCE.set(was.0);
    gh::CATCHUP.set(was.1);
    gh::PROBE.set(was.2);
    res
}

fn lease(ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write, lock: &str, max: u64, data: &str) -> Result<Option<Lock>> {
    let mark = ctx.state.with_file_name(format!("{lock}.step"));
    let stuck = (crate::db::leases(&ctx.state, lock).into_iter())
        .find(|l| l.key == lock && l.expires_at <= ctx.now && !gone(r, l, ctx.now));
    let Some(held) = take(ctx, r, lock, max, data) else {
        return Ok(None);
    };
    if let Some(l) = stuck {
        let at = std::fs::read_to_string(&mark).unwrap_or_default();
        let mine = l.pid != 0 && l.host == crate::db::host();
        let stopped = mine && ok(r, "kill", &["-KILL", &l.pid.to_string()]).is_ok();
        let d = json!({"lock": lock, "pid": l.pid, "since": l.at, "step": format!("after {}", at.trim()), "stopped": stopped});
        writeln!(out, "! a pass held {lock} past {max} s, {}: taken over", d.at("step"))?;
        append(&ctx.state, &Event::new(ctx.now, "drover", None, "pass-stuck", d)).ok();
    }
    std::fs::write(&mark, "start").ok();
    Ok(Some(held))
}

fn one(
    ctx: &Ctx,
    cfg: &Config,
    (r, seen): (&dyn Runner, &dyn Runner),
    out: &mut dyn Write,
    (name, manual): (&str, bool),
    mark: &dyn Fn(&str),
) -> Result<()> {
    let mut laps = remote::Laps::new();
    let tally = remote::Tally(r, std::sync::Mutex::default());
    let r: &dyn Runner = &tally;
    ACTS.with_borrow_mut(BTreeMap::clear);
    undriven::clear();
    ROOM.set((false, None));
    mark("closed issues");
    if KICK.get().is_none() {
        let closed =
            working(&ctx.state).and_then(|ev| crate::check::closed_on_github((ctx, cfg, r, out), ev, Some(name)));
        failed(closed.map(drop), out, name, "closed issues");
    }
    laps.lap("closed issues");
    mark("PR reads, gates, steps");
    let mut res = failed(
        pass(ctx, cfg, (r, seen), out, name),
        out,
        name,
        "PR reads, gates, steps",
    );
    laps.lap("PR reads, gates, steps");
    mark("spawns");
    res = res.or(failed(starts(ctx, cfg, r, out, name), out, name, "spawns"));
    if KICK.get().is_none() {
        res = res.or(failed(undriven::cards(ctx, cfg, out, name), out, name, "spawns"));
    }
    laps.lap("spawns");
    let acts = ACTS.with_borrow_mut(|m| m.remove(name)).unwrap_or_default();
    if KICK.get().is_none() {
        std::fs::write(state_dir(ctx).join(format!("drive-{name}.last")), ctx.now.to_string()).ok();
    }
    if !acts.is_empty() || KICK.get().is_none() {
        let end = crate::now();
        let (by, at) = (
            if manual { "manual" } else { "auto" },
            crate::usage::date(r, end, "+%H:%M"),
        );
        let d = json!({"by": by, "started": ctx.now, "ended": end, "at": at, "actions": acts, "kick": KICK.get(), "remote": tally.json()});
        append(&ctx.state, &Event::new(ctx.now, name, None, "drive-pass", laps.with(d))).ok();
    }
    res.map_or(Ok(()), Err)
}

fn failed(res: Result<()>, out: &mut dyn Write, fleet: &str, step: &str) -> Option<anyhow::Error> {
    let e = res.err()?;
    writeln!(out, "{fleet} {step}: {e:#}").ok();
    let a = json!({"step": "pass", "outcome": "error", "code": "ERROR", "words": format!("failed in {step}: {e:#}")});
    ACTS.with_borrow_mut(|m| m.entry(fleet.into()).or_default().push(a));
    Some(e)
}

fn repair(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<()> {
    let events = working(&ctx.state)?;
    let log = crate::waiting::Log::new(&events);
    let mut agents: Option<Vec<Session>> = None;
    for s in fold(&events).into_values().filter(|s| s.fleet == fleet) {
        let on = log.blockers(&s.fleet, s.issue);
        if on.is_empty() || !gh::cached_pr(ctx, &s.fleet, s.issue).at("number").is_null() {
            continue;
        }
        if !(matches!(s.state.as_str(), "needs-human" | "impl") || s.state.starts_with("fix")) {
            continue;
        }
        if s.state == "impl" {
            let live = match &mut agents {
                Some(a) => a,
                None => agents.insert(crate::link::view::View::load(ctx, cfg)?.sessions().checked()?.running()),
            };
            let lane = s.active_lane().is_some_and(|(_, l)| live.iter().any(|a| a.name == l));
            if lane
                || pending(ctx, cfg, r)
                    .iter()
                    .any(|p| p.fleet == s.fleet && p.issue == Some(s.issue))
            {
                continue;
            }
        }
        let why = format!("blocked by {on}");
        writeln!(out, "{} #{} repaired: back to queued ({why})", s.fleet, s.issue)?;
        let d = json!({"state": "queued", "reason": why});
        emit(
            &ctx.state,
            cfg,
            Event::new(ctx.now, &s.fleet, Some(s.issue), "repaired", d),
        )?;
    }
    let events = working(&ctx.state)?;
    let reset = ["pushed", "verdict", "needs-human", "round-extended"];
    for s in fold(&events)
        .into_values()
        .filter(|s| s.state == "needs-human" && s.fleet == fleet)
    {
        let mine: Vec<(usize, &Event)> = (events.iter().enumerate())
            .filter(|(_, e)| e.fleet == s.fleet && e.issue == Some(s.issue))
            .collect();
        let Some((at, &(ix, nh))) = mine.iter().enumerate().rfind(|(_, (_, e))| e.kind == "needs-human") else {
            continue;
        };
        if crate::lesson::no_pr(s.build.as_deref()) && nh.str("code") == Some("LANE_ENDS") {
            crate::lesson::idle_done(ctx, cfg, r, out, (&s.fleet, s.issue), s.build.as_deref())?;
            continue;
        }
        let from = (mine.iter().take(at)).rposition(|(_, e)| reset.contains(&e.kind.as_str()));
        let round = mine.get(from.map_or(0, |i| i + 1)..at).unwrap_or_default();
        let exited = |e: &Event| e.kind == "lane-state" && e.str("state") == Some("gone");
        let trimmed = round
            .windows(2)
            .any(|w| matches!(w, [a, b] if a.1.kind == "lane-trimmed" && exited(b.1)));
        let again = mine
            .iter()
            .any(|(_, e)| e.kind == "repaired" && e.val("trimmed") == true);
        if nh.str("code") == Some("LANE_ENDS") && trimmed && !again {
            let Some(was) = fold(events.get(..ix).unwrap_or_default())
                .remove(&(s.fleet.clone(), s.issue))
                .map(|b| b.state)
            else {
                continue;
            };
            writeln!(
                out,
                "{} #{} repaired: back to {was} (its lane ended after an idle trim)",
                s.fleet, s.issue
            )?;
            let why = "the lane ended after an idle trim killed its agent: one respawn in its worktree";
            let d = json!({"code": "LANE_ENDS", "state": was, "reason": why, "trimmed": true});
            emit(
                &ctx.state,
                cfg,
                Event::new(ctx.now, &s.fleet, Some(s.issue), "repaired", d),
            )?;
            continue;
        }
        let adopted = |e: &Event| crate::events::design(e) && e.data.get("adopted") == Some(&json!(true));
        let Some(first) = round.iter().position(|(_, e)| e.kind == "respawn") else {
            continue;
        };
        let gone = (round.iter())
            .all(|(_, e)| e.kind != "respawn" || e.str("why").is_some_and(|w| w.starts_with("lane gone")));
        if nh.str("code") != Some("LANE_ENDS") || !gone || !round.iter().take(first).any(|(_, e)| adopted(e)) {
            continue;
        }
        let key = (s.fleet.clone(), s.issue);
        let Some(was) = fold(events.get(..ix).unwrap_or_default()).remove(&key).map(|b| b.state) else {
            continue;
        };
        writeln!(
            out,
            "{} #{} repaired: back to {was} (LANE_ENDS came from a design lane's adoption)",
            s.fleet, s.issue
        )?;
        let why = "the lane gone came from a design lane's adoption, not the build round";
        let d = json!({"code": "LANE_ENDS", "state": was, "reason": why});
        emit(
            &ctx.state,
            cfg,
            Event::new(ctx.now, &s.fleet, Some(s.issue), "repaired", d),
        )?;
    }
    Ok(())
}

fn save(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    f: &crate::config::Fleet,
    (events, n, lane): (&[Event], u64, &str),
    why: &str,
) -> Result<()> {
    let sp = crate::events::last_spawn(events, lane);
    let (Some(wt), Some(branch)) = (sp.and_then(|e| e.str("worktree")), sp.and_then(|e| e.str("branch"))) else {
        return Ok(());
    };
    if [f.integration.as_str(), "main", "master"].contains(&branch) {
        return Ok(());
    }
    let on = crate::hosts::On::of(r, sp);
    let to = format!("HEAD:refs/heads/{branch}");
    let o = on.run("git", &crate::runner::argv(&["-C", wt, "push", "origin", &to]))?;
    let fleet = sp.map_or("", |e| e.fleet.as_str());
    if o.code != 0 {
        return Ok(writeln!(
            out,
            "{fleet} #{n} push of {lane}'s commits failed: {}",
            o.stderr.trim()
        )?);
    }
    if o.stderr.contains("up-to-date") {
        return Ok(());
    }
    writeln!(out, "{fleet} #{n} pushed {lane}'s local commits to {branch} ({why})")?;
    let d = json!({"agent": lane, "branch": branch, "why": why});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), "lane-pushed", d))
}

fn silent(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, agents: &[Session], fleet: &str) -> Result<()> {
    let ev = working(&ctx.state)?;
    let found = stall::silent(ctx, cfg, &ev, agents, fleet);
    for (n, lane, a) in found.into_iter().filter(|x| KICK.get().is_none_or(|k| k == x.0)) {
        match a {
            stall::Silent::Nudge(why) => stall::nudge(ctx, r, out, &ev, (fleet, n), &lane, &why)?,
            stall::Silent::Respawn(role, why) => {
                let num = n.to_string();
                let args = ["spawn", fleet, &num, &role, "--supersede"];
                if act(ctx, cfg, r, out, &ev, (fleet, Some(n), "respawn"), &args)?.is_none() {
                    let d = json!({"role": role, "agent": lane, "why": why});
                    append(&ctx.state, &Event::new(ctx.now, fleet, Some(n), "respawned-silent", d))?;
                }
            }
            stall::Silent::Escalate(why) => escalate(ctx, cfg, out, (fleet, n), (&lane, None), "LANE_SILENT", &why)?,
        }
    }
    crate::limit::due(ctx, r, out, &ev, agents, fleet)?;
    let reviews = fold(&ev)
        .into_values()
        .filter(|s| s.fleet == fleet && s.state.starts_with("review"));
    for s in reviews.filter(|s| KICK.get().is_none_or(|k| k == s.issue)) {
        let Some((role, old)) = s.active_lane() else { continue };
        if !agents.iter().any(|a| a.name == old && a.busy()) {
            continue;
        }
        let Some(mins) = stall::stale_check(ctx, &ev, agents, (fleet, s.issue), old) else {
            continue;
        };
        let (n, step) = (s.issue.to_string(), (fleet, Some(s.issue), "review"));
        if act(ctx, cfg, r, out, &ev, step, &["spawn", fleet, &n, role, "--supersede"])?.is_none() {
            stale_replaced(ctx, cfg, out, (fleet, s.issue), role, old, mins)?;
        }
    }
    Ok(())
}

fn stale_replaced(
    ctx: &Ctx,
    cfg: &Config,
    out: &mut dyn Write,
    (fleet, n): (&str, u64),
    role: &str,
    old: &str,
    mins: u64,
) -> Result<()> {
    let ev = working(&ctx.state)?;
    let new = |e: &&Event| e.is("spawn", fleet, n) && e.str("role") == Some(role) && e.ts >= ctx.now;
    let new = ev
        .iter()
        .rfind(new)
        .and_then(|e| e.str("agent"))
        .unwrap_or("a lane still starting");
    let why = format!("{old} quiet {mins}m with no verdict: replaced by {new}");
    writeln!(out, "{fleet} #{n} {why}")?;
    let d = json!({"agent": old, "role": role, "idle_minutes": mins, "new": new, "reason": why});
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, fleet, Some(n), "rev-stale-replaced", d),
    )
}

fn escalate(
    ctx: &Ctx,
    cfg: &Config,
    out: &mut dyn Write,
    (fleet, n): (&str, u64),
    (lane, on): (&str, Option<&str>),
    code: &str,
    why: &str,
) -> Result<()> {
    writeln!(out, "{fleet} #{n} needs-human: {why}")?;
    let d = json!({"code": code, "reason": why, "agent": lane, "failed_on": on});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), "needs-human", d))
}

fn turn_prompt(what: &str) -> Option<(&'static str, &'static str)> {
    if what == waiting::TURN_NUDGE {
        Some((
            "done",
            "drover: you reported `done`, but nothing new is pushed at the PR head (or there is no \
            PR). Push and open the PR, then `drover turn done`; or `drover turn blocked --why <reason>`.",
        ))
    } else if what.ends_with(waiting::TURN_ASK) {
        Some((
            "need-input",
            "drover: you reported `need-input`, but filed no `drover ask`: no card reaches the owner. \
            Ask with `drover ask <fleet> <issue> …`, then `drover turn need-input`; or `drover turn blocked \
            --why <reason>`.",
        ))
    } else if what == waiting::TURN_GO_ON {
        Some((
            "working",
            "drover: continue the task; end the turn with `drover turn …`.",
        ))
    } else {
        None
    }
}

fn pass(
    ctx: &Ctx,
    cfg: &Config,
    (r, seen): (&dyn Runner, &dyn Runner),
    out: &mut dyn Write,
    fleet: &str,
) -> Result<()> {
    let f = cfg.fleet(fleet)?;
    let sessions = crate::link::view::View::load(ctx, cfg)?.sessions().checked()?;
    let agents = sessions.running();
    let ev = working(&ctx.state)?;
    let st = fold(&ev);
    for s in st.values().filter(|s| s.fleet == fleet && !s.done()) {
        let on = crate::events::held_on(&ev, s);
        let open = |b: &u64| st.get(&(fleet.into(), *b)).is_none_or(|b| b.state != "merged");
        if on.is_empty() || on.iter().any(open) {
            continue;
        }
        let merged = ev.iter().filter(|e| e.kind == "merged" && e.fleet == fleet);
        let at = merged
            .filter(|e| e.issue.is_some_and(|b| on.contains(&b)))
            .map(|e| e.ts)
            .max();
        let outlived = ev
            .iter()
            .any(|e| e.kind == "drive-pass" && e.fleet == fleet && at.is_some_and(|t| e.ts > t));
        let mut episode = ev.iter().rev().take_while(|e| !e.is("held", fleet, s.issue));
        if outlived && !episode.any(|e| e.is("hold-stale", fleet, s.issue)) {
            let d = json!({"on": on, "merged_at": at});
            let e = Event::new(ctx.now, fleet, Some(s.issue), "hold-stale", d);
            emit(&ctx.state, cfg, e)?;
        }
        let Some((role, lane)) = s.active_lane() else {
            let e = Event::new(ctx.now, fleet, Some(s.issue), "released", json!({"on": on}));
            emit(&ctx.state, cfg, e)?;
            continue;
        };
        let sp = crate::events::last_spawn(&ev, lane);
        let live = agents.iter().any(|a| a.name == lane);
        let pane = sp.and_then(|e| e.str("pane")).filter(|_| live);
        let away = crate::hosts::offline(ctx, Some(cfg), sp).is_some();
        if away {
            continue;
        }
        let text = format!(
            "{} merged; rebase on {} and continue",
            crate::queue::hashes(&on),
            f.integration
        );
        let (dsh, n) = (sp.and_then(|e| e.str("harness")) == Some("dsh"), s.issue.to_string());
        let c = ["spawn", fleet, &n, role, "--supersede"];
        let woke = match pane {
            Some(p) => crate::term::tell(&crate::hosts::On::of(r, sp), dsh, lane, p, &text).is_ok(),
            None => act(ctx, cfg, r, out, &ev, (fleet, None, "respawn"), &c)?.is_none(),
        };
        let d = json!({"agent": lane, "on": on, "respawn": !live});
        let e = Event::new(ctx.now, fleet, Some(s.issue), "released", d);
        if woke {
            emit(&ctx.state, cfg, e)?;
        }
    }
    for s in st.values().filter(|s| s.fleet == fleet && s.state == "design") {
        crate::design::handoff(ctx, cfg, r, out, &agents, s)?;
    }
    crate::stall::refused(ctx, cfg, r, out, &working(&ctx.state)?, fleet)?;
    silent(ctx, cfg, r, out, &agents, fleet)?;
    if let Err(e) = crate::hotspots::sync(ctx, cfg, r, fleet) {
        writeln!(out, "{fleet} hotspots: {e:#}")?;
    }
    let events = working(&ctx.state)?;
    let one = |(s, _): &(IssueState, Waiting)| KICK.get().is_none_or(|k| k == s.issue);
    let mut waits = waiting::scan(ctx, cfg, seen, &events, &agents, true, Some(fleet));
    waits.retain(one);
    let promised = crate::promise::pass(ctx, cfg, fleet, &events, &waits);
    failed(promised, out, fleet, "promises");
    failed(external(ctx, cfg, out, fleet), out, fleet, "outside changes");
    if handoff(ctx, cfg, r, out, fleet, &events, &waits)? {
        waits = waiting::scan(ctx, cfg, seen, &working(&ctx.state)?, &agents, false, Some(fleet));
        waits.retain(one);
    }
    let conflicts = crate::overlap::preview(ctx, cfg, r, fleet, &events).unwrap_or_default();
    let rev = |w: &Waiting| {
        w.next_cmd
            .as_deref()
            .and_then(|c| c.split(' ').nth(4))
            .is_some_and(|r| f.family(r) == "rev")
    };
    let verdict = |w: &Waiting| w.next_cmd.as_deref().is_some_and(|c| c.starts_with("drover verdict "));
    let merge = |w: &Waiting| w.next_cmd.as_deref().is_some_and(|c| c.starts_with("drover merge "));
    waits.sort_by_key(|(s, w)| (!verdict(w), !merge(w), !rev(w), conflicts.contains_key(&s.issue)));
    let (mut updated, mut spawns_held): (bool, Code) = (false, None);
    for (s, w) in waits
        .iter()
        .filter(|(s, w)| s.fleet == fleet && !s.done() && s.lane.as_ref() == Some(&w.lane))
    {
        undriven::skip(s.issue, format!("no step of drive's pass ran on \"{}\"", w.what));
        let lock = format!("drive-{fleet}-{}.lock", s.issue);
        let mine = take(ctx, r, &lock, STALE_LOCK_SECS, fleet);
        if mine.is_none() && !SLOT.get() {
            let by = held(ctx, &lock).map_or(String::new(), |l| format!(" by pid {}", l.pid));
            undriven::skip(s.issue, format!("skipped: {lock} is held{by}"));
            continue;
        }
        let mut pr = gh::cached_pr(ctx, fleet, s.issue);
        crate::land::mark(ctx, Some(r), f, (fleet, s.issue), &events, &mut pr);
        let ci = gh::ci(&gh::required(pr.at("statusCheckRollup"), f.checks(&pr)));
        let gone = gh::missing(ctx, None, cfg, (fleet, s.issue), &events, &pr).filter(|m| w.what.contains(m));
        let n = s.issue.to_string();
        if pr.at("state") == "OPEN" && !queue::serials(&events, f, fleet).is_empty() {
            serial(ctx, cfg, r, out, (fleet, s), &pr)?;
        }
        if let Err(e) = crate::gate::sync_label(ctx, cfg, r, out, s, &pr) {
            writeln!(out, "{fleet} #{n} label: {e:#}")?;
        }
        if crate::gate::ready_draft(ctx, cfg, r, out, s, &pr)? && merge(w) {
            continue;
        }
        let fetched = match pickup::fetch(ctx, cfg, r, out, &events, (s, w), &pr)? {
            pickup::Picked::Held => {
                undriven::skip(s.issue, "skipped: its verdict pickup is held".into());
                continue;
            }
            pickup::Picked::Fetched(x) => Some(x),
            pickup::Picked::No => None,
        };
        let w = fetched.as_ref().unwrap_or(w);
        if let Some(e) = crate::events::last_at(&working(&ctx.state)?, "auto-updated", (fleet, s.issue), ctx.now) {
            let to = e.str("to").unwrap_or_default();
            writeln!(out, "{fleet} #{n} ready (CI pending at {to:.7})")?;
        } else if job::planned(ctx, cfg, r, out, (s, w))? {
        } else if w.who == "drive" && w.what == waiting::LESSON_PROMPT {
            let p = crate::lesson::prompt(fleet, s.issue, s.build.as_deref());
            let Some(sent) = tell(r, &events, &w.lane, &p) else {
                continue;
            };
            writeln!(
                out,
                "{fleet} #{n} lesson lane prompted: {}",
                if sent { "sent" } else { "not sent" }
            )?;
            let d = json!({"agent": w.lane, "sent": sent});
            append(
                &ctx.state,
                &Event::new(ctx.now, fleet, Some(s.issue), "lesson-prompted", d),
            )?;
        } else if let Some((state, text)) = turn_prompt(&w.what).filter(|_| w.who == "drive") {
            let Some(sent) = tell(r, &events, &w.lane, text).or((state != "working").then_some(false)) else {
                continue;
            };
            writeln!(
                out,
                "{fleet} #{n} turn {state}: {}",
                if sent { "prompted" } else { "not sent" }
            )?;
            let d = json!({"agent": w.lane, "state": state, "sent": sent});
            append(
                &ctx.state,
                &Event::new(ctx.now, fleet, Some(s.issue), "turn-prompted", d),
            )?;
        } else if w.who == "drive" && w.what.starts_with(waiting::TURN_BLOCKED) {
            let why = &w.what[waiting::TURN_BLOCKED.len()..];
            writeln!(out, "{fleet} #{n} needs-human: lane blocked: {why}")?;
            let d = json!({"code": "LANE_BLOCKED", "reason": format!("lane blocked: {why}"), "agent": w.lane});
            emit(
                &ctx.state,
                cfg,
                Event::new(ctx.now, fleet, Some(s.issue), "needs-human", d),
            )?;
        } else if w.who == "drive" && w.what == waiting::LESSON_CLOSE {
            crate::lesson::idle_done(ctx, cfg, r, out, (fleet, s.issue), s.build.as_deref())?;
        } else if pickup::stuck(ctx, cfg, out, f, (&events, &agents), (s, w))? {
        } else if w.who == "drive" {
            let gone = w.what.split_once(waiting::NO_PUSH);
            let ends =
                gone.filter(|g| (w.next_cmd.is_none() || g.1.starts_with("respawning")) && !waiting::resumed(g.0));
            if let Some(g) = ends {
                if crate::breaker::unlanded(ctx, cfg, r, out, fleet)?
                    || crate::breaker::failed(ctx, cfg, out, fleet, &w.lane, g.0)?
                {
                    continue;
                }
            }
            if gone.is_some() && w.next_cmd.is_none() {
                let why = "lane keeps ending without a push";
                writeln!(out, "{fleet} #{n} needs-human: {why}")?;
                let on = crate::breaker::blame(&events, &w.lane);
                let d = json!({"code": "LANE_ENDS", "reason": why, "agent": w.lane, "failed_on": on});
                emit(
                    &ctx.state,
                    cfg,
                    Event::new(ctx.now, fleet, Some(s.issue), "needs-human", d),
                )?;
            }
            if gone.is_some_and(|g| !g.1.starts_with("respawning")) {
                continue;
            }
            let Some(cmd) = &w.next_cmd else { continue };
            let args: Vec<&str> = cmd.split_whitespace().skip(1).collect();
            let arg = |i: usize| args.get(i).copied().unwrap_or_default();
            if let Some(c) = spawns_held.filter(|_| arg(0) == "spawn") {
                undriven::skip(s.issue, format!("skipped: an earlier spawn this pass was refused {c}"));
                continue;
            }
            let step = match (arg(0), args.get(3).copied()) {
                _ if gone.is_some() => "respawn",
                ("spawn", Some(r)) if f.flow().is_check(r) => "review",
                ("spawn", _) if cmd.ends_with("--update") => "conflict",
                ("spawn", _) if cmd.ends_with("--supersede") => "fix",
                ("spawn", _) => "respawn",
                (v, _) => v,
            };
            if step == "review" && arg(3) == "verify" && !crate::verify::due(ctx, cfg, r, out, s, &pr)? {
                continue;
            }
            let verdict_fail = step == "verdict" && arg(3) == "FAIL";
            let at = (fleet, s.issue, w.lane.as_str());
            if verdict_fail && no_proof(ctx, cfg, r, out, (&events, &agents), at, &args)?.is_some() {
                continue;
            }
            let repro =
                (step == "verdict" && arg(3) == "PASS").then(|| repro_seen(r, &events, (fleet, s.issue), &args));
            if repro.as_ref().is_some_and(|x| x.1) {
                repro_stuck(ctx, cfg, out, &events, (fleet, s.issue, &w.lane))?;
                continue;
            }
            if w.what.starts_with("reviewer exited") {
                let d = json!({"role": args.get(3), "why": "exited without a verdict"});
                append(&ctx.state, &Event::new(ctx.now, fleet, Some(s.issue), "lane-lost", d))?;
            }
            let cause = crate::session::stamp::exit(&events, &w.lane)
                .map(|x| x.cause)
                .filter(|c| c.resumable());
            let cause = cause.map(crate::session::Cause::word);
            if let Some(why) = cause.filter(|_| arg(0) == "spawn") {
                save(ctx, cfg, r, out, f, (&events, s.issue, &w.lane), why)?;
            }
            let mut code = act(ctx, cfg, r, out, &events, (fleet, Some(s.issue), step), &args)?;
            if arg(0) == "spawn" && crate::starts::stops(code) {
                spawns_held = spawns_held.or(code);
            }
            pickup::after(ctx, out, &events, (w, step), code)?;
            let stale = (code == Some("LANE_LIVE") && step == "review")
                .then(|| s.lanes.get(arg(3)))
                .flatten()
                .and_then(|l| Some((l, stall::stale_check(ctx, &events, &agents, (fleet, s.issue), l)?)));
            if code == Some("LANE_LIVE") && arg(0) == "spawn" && (last_words(fleet).contains(OLD) || stale.is_some()) {
                let a = [&args[..], &["--supersede"]].concat();
                code = act(ctx, cfg, r, out, &events, (fleet, Some(s.issue), step), &a)?;
            }
            if let Some((old, mins)) = stale.filter(|_| code.is_none()) {
                stale_replaced(ctx, cfg, out, (fleet, s.issue), arg(3), old, mins)?;
            }
            if let Some(((hash, _), c)) = repro.zip(code.filter(|c| REV_FIXES.contains(c))) {
                repro_refused(
                    ctx,
                    cfg,
                    (r, out),
                    (&events, &agents),
                    (fleet, s.issue, &w.lane),
                    (&args, &hash, c),
                )?;
            }
            if let Some((why, _)) = gone.filter(|_| code.is_none()) {
                let on = crate::breaker::blame(&events, &w.lane);
                let d = json!({"role": args.get(3), "agent": w.lane, "why": why, "failed_on": on});
                append(&ctx.state, &Event::new(ctx.now, fleet, Some(s.issue), "respawn", d))?;
            }
            if let Some(c) = code.filter(|c| gone.is_some() && *c != "RESPAWN_LOOP" && owner_refusal(c)) {
                let why = ACTS.with_borrow(|m| m.get(fleet).and_then(|a| a.last()).map(|a| a["why"].clone()));
                let reason = format!(
                    "{} respawn refused {c}: {}",
                    arg(3),
                    why.as_ref().and_then(Value::as_str).unwrap_or("?")
                );
                writeln!(out, "{fleet} #{n} needs-human: {reason}")?;
                let d = json!({"code": "RESPAWN_REFUSED", "refused": c, "reason": reason, "agent": w.lane});
                emit(
                    &ctx.state,
                    cfg,
                    Event::new(ctx.now, fleet, Some(s.issue), "needs-human", d),
                )?;
            }
            let mut merge = step == "merge";
            let full = f.merge_label.is_some() && pr.at("landing") != "fast-first";
            let green = ci == "green"
                && pr.at("isDraft") != true
                && pr.at("headRefOid").as_str() == args.get(5).copied()
                && !full;
            let verified = || -> Result<bool> {
                let st = fold(&working(&ctx.state)?).remove(&(fleet.to_string(), s.issue));
                Ok(crate::gate::verified(f, st.as_ref(), arg(5), false).is_ok())
            };
            if step == "verdict" && code.is_none() && arg(3) == "PASS" && green && verified()? {
                merge = true;
                code = if pr.at("mergeable") == "CONFLICTING" {
                    Some("NOT_MERGEABLE")
                } else {
                    act(
                        ctx,
                        cfg,
                        r,
                        out,
                        &events,
                        (fleet, Some(s.issue), "merge"),
                        &["merge", fleet, &n],
                    )?
                };
            }
            if merge && conflicted(fleet, code) {
                update_lane(ctx, cfg, r, out, &events, fleet, s.issue)?;
            } else if merge && code.is_none() {
                updated = true;
                catch_up(ctx, cfg, r, out, fleet)?;
            }
        } else if let Some(a) = stall::stalled(ctx, cfg, r, &events, &sessions, w) {
            let start = matches!(
                &a,
                stall::Act::Respawn(_, "failed start") | stall::Act::Escalate("START_FAILED", _)
            );
            if start
                && (crate::breaker::unlanded(ctx, cfg, r, out, fleet)?
                    || crate::breaker::failed(ctx, cfg, out, fleet, &w.lane, "no output after its spawn")?)
            {
                continue;
            }
            match a {
                stall::Act::Respawn(role, why) => {
                    let args = ["spawn", fleet, &n, &role, "--supersede"];
                    let code = act(ctx, cfg, r, out, &events, (fleet, Some(s.issue), "respawn"), &args)?;
                    if code.is_none() {
                        let on = crate::breaker::blame(&events, &w.lane);
                        let d = json!({"role": role, "agent": w.lane, "why": why, "failed_on": on});
                        let kind = if why == stall::KEY_401 { "self-heal" } else { "respawn" };
                        append(&ctx.state, &Event::new(ctx.now, fleet, Some(s.issue), kind, d))?;
                    }
                }
                stall::Act::Escalate(code, why) => {
                    let on = crate::breaker::blame(&events, &w.lane);
                    escalate(ctx, cfg, out, (fleet, s.issue), (&w.lane, on), code, &why)?;
                }
            }
        } else if s.state == "blocked" && s.last_event != "no-action" {
            writeln!(out, "{fleet} #{n} no-action: {}", w.what)?;
            let d = json!({"why": w.what});
            append(&ctx.state, &Event::new(ctx.now, fleet, Some(s.issue), "no-action", d))?;
        } else if s.state == "ready" && (ci == "failed" || gone.is_some()) {
            ci_red(ctx, cfg, (r, seen), out, (&events, &agents), s, (&pr, gone.as_deref()))?;
        } else if s.state == "ready" && ci == "pending" {
            gh::unstick(ctx, cfg, r, out, (fleet, s.issue), &events, &pr)?;
        }
    }
    if !updated {
        catch_up(ctx, cfg, r, out, fleet)?;
    }
    failed(ext_closed(ctx, cfg, r, out, fleet), out, fleet, "blockers");
    if KICK.get().is_none() {
        if let Err(e) = crate::land::watch(ctx, cfg, r, out, fleet) {
            writeln!(out, "{fleet} land: {e:#}")?;
        }
    }
    Ok(())
}

fn ci_red(
    ctx: &Ctx,
    cfg: &Config,
    (r, seen): (&dyn Runner, &dyn Runner),
    out: &mut dyn Write,
    (events, agents): (&[Event], &[Session]),
    s: &IssueState,
    (pr, gone): (&Value, Option<&str>),
) -> Result<()> {
    let (fleet, n, f) = (s.fleet.as_str(), s.issue.to_string(), cfg.fleet(&s.fleet)?);
    let head = pr.at("headRefOid").as_str().unwrap_or_default();
    let file = state_dir(ctx).join(format!("ci-{fleet}-{n}.md"));
    let gone = gone.map(|m| format!("FAIL\n\n{m} at {head}: no checks; fix the workflow.\n"));
    std::fs::write(&file, gone.unwrap_or_else(|| ci_findings(r, f, pr, head)))?;
    let file = file.to_string_lossy();
    let args = ["verdict", fleet, &n, "FAIL", "--sha", head, "--file", &file];
    if act(ctx, cfg, r, out, events, (fleet, Some(s.issue), "ci-red"), &args)?.is_some() {
        return Ok(());
    }
    let ev = working(&ctx.state)?;
    let this = |x: &IssueState| x.fleet == fleet && x.issue == s.issue;
    let row = waiting::scan_where(ctx, cfg, seen, (&ev, agents), false, this).pop();
    let Some(c) = row.and_then(|x| x.1.next_cmd).filter(|c| c.ends_with(" --supersede")) else {
        return Ok(());
    };
    let a: Vec<&str> = c.split_whitespace().skip(1).collect();
    act(ctx, cfg, r, out, &ev, (fleet, Some(s.issue), "fix"), &a).map(drop)
}

fn no_proof(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (ev, agents): (&[Event], &[Session]),
    (fleet, n, lane): (&str, u64, &str),
    args: &[&str],
) -> Result<Option<Code>> {
    let sp = ev.iter().rfind(|e| e.is("spawn", fleet, n) && crate::events::checks(e));
    let on = crate::hosts::On::of(r, sp);
    let body = crate::link::read(ctx, cfg, r, sp, args.get(7).unwrap_or(&"")).unwrap_or_default();
    let Some((code, why)) = crate::gate::unfit(&body) else {
        return Ok(None);
    };
    let hash = &crate::devices::sha256(body.as_bytes())[..16];
    let mine: Vec<&Event> = (ev.iter())
        .filter(|e| e.is("proof-refused", fleet, n) && sp.is_some_and(|p| e.ts >= p.ts))
        .collect();
    let seen = mine
        .iter()
        .any(|e| e.str("sha") == args.get(5).copied() && e.str("hash") == Some(hash));
    let pane = sp.and_then(|e| e.str("pane"));
    let open = pane.filter(|_| agents.iter().any(|a| a.name == lane));
    let told = mine.iter().any(|e| e.val("prompted") == true);
    if !seen {
        act(ctx, cfg, r, out, ev, (fleet, Some(n), "verdict"), args)?;
        let fix = if code == "NO_PROOF" {
            "Add the Proof blocks"
        } else {
            "Drop those findings"
        };
        let text = format!("drover verdict refused your verdict.md: REFUSED {code}: {why}. {fix}.");
        let tell =
            |p| crate::term::reply(&on, false, lane, p, &text).is_ok() && crate::term::landed(&on, lane, p, &text);
        let prompted = open.filter(|_| !told).is_some_and(tell);
        let d = json!({"sha": args.get(5), "hash": hash, "agent": lane, "prompted": prompted});
        emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), "proof-refused", d))?;
        if prompted {
            return Ok(Some(Some(code)));
        }
    }
    let spawned = ev
        .iter()
        .rposition(|e| e.is("spawn", fleet, n) && crate::events::checks(e));
    let mut before = ev.iter().take(spawned.unwrap_or_default());
    if before.any(|e| e.is("proof-refused", fleet, n) && e.str("sha") == args.get(5).copied()) {
        let why = format!("a re-review at the same head was refused {code} too: {why}");
        escalate(ctx, cfg, out, (fleet, n), (lane, None), code, &why)?;
        return Ok(Some(Some(code)));
    }
    let role = sp.and_then(|e| e.str("role")).unwrap_or("rev");
    let (num, why) = (n.to_string(), format!("verdict refused {code}: re-review"));
    let c = ["spawn", fleet, &num, role, "--supersede"];
    let code = act(ctx, cfg, r, out, ev, (fleet, Some(n), "review"), &c)?;
    if code.is_none() {
        let d = json!({"role": role, "agent": lane, "why": why});
        append(&ctx.state, &Event::new(ctx.now, fleet, Some(n), "lane-lost", d))?;
    }
    Ok(Some(code))
}

fn last_why(fleet: &str) -> Option<String> {
    ACTS.with_borrow(|m| m.get(fleet)?.last()?["why"].as_str().map(String::from))
}

fn repro_seen(r: &dyn Runner, ev: &[Event], (fleet, n): (&str, u64), args: &[&str]) -> (String, bool) {
    let sp = ev.iter().rfind(|e| e.is("spawn", fleet, n) && crate::events::checks(e));
    let body = crate::hosts::On::of(r, sp)
        .read(Path::new(args.get(7).unwrap_or(&"")))
        .unwrap_or_default();
    let hash = crate::devices::sha256(body.as_bytes())[..16].to_string();
    let seen = ev.iter().any(|e| {
        e.is("repro-refused", fleet, n)
            && sp.is_some_and(|p| e.ts >= p.ts)
            && e.str("sha") == args.get(5).copied()
            && e.str("hash") == Some(&hash)
    });
    (hash, seen)
}

fn repro_stuck(
    ctx: &Ctx,
    cfg: &Config,
    out: &mut dyn Write,
    ev: &[Event],
    (fleet, n, lane): crate::hosts::Lane,
) -> Result<()> {
    let sp = ev.iter().rfind(|e| e.is("spawn", fleet, n) && crate::events::checks(e));
    let refused = |e: &&Event| e.is("repro-refused", fleet, n) && sp.is_some_and(|p| e.ts >= p.ts);
    let mins = cfg.limits.stall_minutes;
    let Some(e) = ev.iter().rfind(refused).filter(|e| e.ts + mins * 60 <= ctx.now) else {
        return Ok(());
    };
    let (code, why) = (e.str("code").unwrap_or("NO_REPRO"), e.str("why").unwrap_or_default());
    let since = if e.val("prompted") == true {
        "the re-prompt"
    } else {
        "it (the reviewer could not be told)"
    };
    let why = format!("verdict refused {code}, not corrected {mins} min after {since}: {why}");
    escalate(ctx, cfg, out, (fleet, n), (lane, None), code, &why)
}

fn repro_refused(
    ctx: &Ctx,
    cfg: &Config,
    (r, out): (&dyn Runner, &mut dyn Write),
    (ev, agents): (&[Event], &[Session]),
    (fleet, n, lane): (&str, u64, &str),
    (args, hash, code): (&[&str], &str, &str),
) -> Result<()> {
    let sp = ev.iter().rfind(|e| e.is("spawn", fleet, n) && crate::events::checks(e));
    let on = crate::hosts::On::of(r, sp);
    let why = ACTS.with_borrow(|m| m.get(fleet).and_then(|a| a.last()?["why"].as_str().map(String::from)));
    let why = why.unwrap_or_default();
    let told = ev
        .iter()
        .any(|e| e.is("repro-refused", fleet, n) && sp.is_some_and(|p| e.ts >= p.ts) && e.val("prompted") == true);
    let pane = sp
        .and_then(|e| e.str("pane"))
        .filter(|_| !told && agents.iter().any(|a| a.name == lane));
    let block = if code == "NO_REPRO" { "Repro" } else { "Perf" };
    let text = format!("drover verdict refused your verdict.md: REFUSED {code}: {why}. Fix the {block} block.");
    let tell = |p| crate::term::reply(&on, false, lane, p, &text).is_ok() && crate::term::landed(&on, lane, p, &text);
    let prompted = pane.is_some_and(tell);
    let d = json!({"sha": args.get(5), "hash": hash, "agent": lane, "why": why, "prompted": prompted, "code": code});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), "repro-refused", d))?;
    if told {
        let why = format!("verdict refused {code} again after the re-prompt: {why}");
        escalate(ctx, cfg, out, (fleet, n), (lane, None), code, &why)?;
    }
    Ok(())
}

pub(crate) fn tell(r: &dyn Runner, events: &[Event], lane: &str, text: &str) -> Option<bool> {
    let sp = crate::events::last_spawn(events, lane);
    let dsh = sp.and_then(|e| e.str("harness")) == Some("dsh");
    let pane = sp.and_then(|e| e.str("pane")).unwrap_or_default();
    let on = crate::hosts::On::of(r, sp);
    crate::term::awake(&on, lane).ok()?;
    Some(crate::term::reply(&on, dsh, lane, pane, text).is_ok())
}

const REV_FIXES: [&str; 2] = ["NO_REPRO", "NO_BASELINE"];

pub(crate) const OLD: &str = "is in the way: replacing it";

pub(crate) fn owner_refusal(code: &str) -> bool {
    let owner = [
        "RESPAWN_LOOP",
        "DESIGN_MAIN_ONLY",
        "MCP_FORBIDDEN",
        "ACP_UNGUARDED",
        "ACCOUNT_ENV",
        "SECRET_IN_WIP",
    ];
    owner.contains(&code)
}

fn last_words(fleet: &str) -> String {
    let last = ACTS.with_borrow(|m| m.get(fleet).and_then(|a| a.last()).map(|a| a["words"].to_string()));
    last.unwrap_or_default()
}

fn conflicted(fleet: &str, code: Code) -> bool {
    let w = last_words(fleet).to_lowercase();
    code == Some("NOT_MERGEABLE") || code.is_some() && (w.contains("not mergeable") || w.contains("conflict"))
}

fn serial(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, s): (&str, &IssueState),
    pr: &Value,
) -> Result<()> {
    let (f, n, ev) = (cfg.fleet(fleet)?, s.issue, working(&ctx.state)?);
    let q = queue::replay(&ev, fleet);
    let (num, head) = (pr["number"].to_string(), pr["headRefOid"].as_str().unwrap_or_default());
    for g in queue::groups(&q, &[], f, fleet, n) {
        let has = |e: &Event| e.is("serial-joined", fleet, n) && e.str("group") == Some(&g.name);
        if !ev.iter().any(has) {
            let d = json!({"group": g.name, "by": "mark"});
            emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), "serial-joined", d))?;
        }
    }
    for (g, p) in crate::gate::unjoined(ctx, r, f, (fleet, n), (&num, head)).unwrap_or_default() {
        writeln!(out, "{fleet} #{n} serial-joined {}: {p}", g.name)?;
        let d = json!({"group": g.name, "by": "diff", "path": p});
        emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), "serial-joined", d))?;
    }
    let now = working(&ctx.state)?;
    for g in queue::groups(&q, &now, f, fleet, n) {
        if queue::held(&now, fleet, n, &g.name) {
            let e = Event::new(ctx.now, fleet, Some(n), "serial-slot", json!({"group": g.name}));
            emit(&ctx.state, cfg, e)?;
        }
    }
    Ok(())
}

pub(crate) fn update_lane(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    ev: &[Event],
    fleet: &str,
    issue: u64,
) -> Result<Code> {
    let p = cfg.fleet(fleet)?.flow();
    let b = p
        .next(p.gate(), "conflict")
        .and_then(|t| p.role(t.0))
        .unwrap_or(p.start());
    let n = issue.to_string();
    act(
        ctx,
        cfg,
        r,
        out,
        ev,
        (fleet, Some(issue), "conflict"),
        &["spawn", fleet, &n, b, "--supersede", "--update"],
    )
}

fn handoff(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    fleet: &str,
    ev: &[Event],
    waits: &[(IssueState, Waiting)],
) -> Result<bool> {
    let f = cfg.fleet(fleet)?;
    let mut any = false;
    crate::status::sync(ctx, cfg, r, out, fleet)?;
    for (s, w) in waits
        .iter()
        .filter(|(s, w)| s.fleet == fleet && s.lane.as_ref() == Some(&w.lane))
    {
        let pr = gh::cached_pr(ctx, fleet, s.issue);
        let head = pr.at("headRefOid").as_str().unwrap_or_default();
        let building = matches!(s.state.split('(').next(), Some("impl" | "fix"));
        if !building || pr.at("state") != "OPEN" || head.is_empty() {
            continue;
        }
        let handed = ev
            .iter()
            .any(|e| e.is("pushed", fleet, s.issue) && e.str("sha") == Some(head));
        let ci = gh::ci(&gh::required(pr.at("statusCheckRollup"), &f.required_checks));
        let turn = waiting::turn(ev, s);
        let done = s.reviewed(head) && waiting::fix_done(ev, fleet, s.issue).is_some()
            || turn.is_some_and(|t| t.str("state") == Some("done"));
        let unlabelled = f.merge_label.as_deref().is_some_and(|l| !gh::has_label(&pr, l));
        let quiet = w.who == "none" || w.what.contains(waiting::NO_PUSH) || turn.is_some();
        let idle = done || quiet && ctx.now >= w.since + 120;
        if s.reviewed(head) {
            if idle && (done || unlabelled || ci != "pending") {
                any |= no_change(ctx, cfg, out, (fleet, s.issue), ev, head, ci)?;
            }
            continue;
        }
        if handed {
            continue;
        }
        let gated = |e: &Event| e.is("turn-end", fleet, s.issue) && e.str("sha") == Some(head);
        if !ev.iter().any(gated) {
            any |= crate::done::ungated(ctx, cfg, r, out, (fleet, s.issue), ev, (&w.lane, head), idle)?;
            continue;
        }
        writeln!(out, "{fleet} #{} pushed: {head:.7} via pr", s.issue)?;
        let d = json!({"sha": head, "via": "pr"});
        emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(s.issue), "pushed", d))?;
        any = true;
    }
    Ok(any)
}

fn no_change(
    ctx: &Ctx,
    cfg: &Config,
    out: &mut dyn Write,
    (fleet, n): (&str, u64),
    ev: &[Event],
    head: &str,
    ci: &str,
) -> Result<bool> {
    let Some((v, since)) =
        (ev.iter().rposition(|e| e.is("verdict", fleet, n))).and_then(|i| ev.get(i..)?.split_first())
    else {
        return Ok(false);
    };
    let at = v.str("sha").is_some_and(|x| x.eq_ignore_ascii_case(head));
    if v.str("verdict") != Some("FAIL") || !at || since.iter().any(|e| e.is("fix-no-change", fleet, n)) {
        return Ok(false);
    }
    let gate = cfg
        .fleet(fleet)?
        .flow()
        .get(v.str("stage").unwrap_or_default())
        .is_some_and(|g| g.kind == "gate");
    let pass = ci == "green" && (gate || crate::gate::form_only(v.str("body").unwrap_or_default(), &["ci".into()]));
    let done = since
        .iter()
        .rfind(|e| e.is("fix-done", fleet, n))
        .and_then(|e| e.str("why"));
    let Some(why) = done.or(pass.then_some("CI-only FAIL, CI green at the same head")) else {
        return Ok(false);
    };
    writeln!(out, "{fleet} #{n} fix-no-change at {head:.7}: {why} (CI {ci})")?;
    let d = json!({"sha": head, "why": why, "ci": ci});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), "fix-no-change", d))?;
    if pass {
        let body = format!("PASS\n\nNo new commit: the CI-only FAIL at {head} is resolved, CI is green there now.\n");
        let d = json!({"verdict": "PASS", "sha": head, "stage": v.str("stage"), "loop": v.str("loop"), "body": body,
            "by": "drive"});
        emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), "verdict", d))?;
    }
    Ok(true)
}

fn external(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, fleet: &str) -> Result<()> {
    let events = working(&ctx.state)?;
    for s in fold(&events)
        .into_values()
        .filter(|s| s.fleet == fleet && !s.done() && s.state != "parked" && KICK.get().is_none_or(|k| k == s.issue))
    {
        let pr = gh::cached_pr(ctx, fleet, s.issue);
        let num = pr.at("number").to_string();
        let seen = |e: &Event| e.is("blocked", fleet, s.issue) && e.str("pr") == Some(&num);
        let back = |e: &Event| e.issue == Some(s.issue) && e.fleet == fleet && e.str("landed_pr") == Some(&num);
        let (kind, mut d) = match pr.at("state").as_str() {
            Some("MERGED") if crate::gate::unlogged_merge(&events, fleet, s.issue).is_some() => continue,
            Some("MERGED") if !events.iter().any(back) => {
                let m = json!({"into": pr.at("baseRefName"), "merge_sha": pr.at("mergeCommit").at("oid"), "merged_at": pr.at("mergedAt")});
                ("merged", m)
            }
            Some("CLOSED") if !events.iter().any(seen) => (
                "blocked",
                json!({"code": "PR_CLOSED", "reason": "PR closed outside drover"}),
            ),
            _ => continue,
        };
        d.put("pr", json!(num)).put("by", json!("external"));
        writeln!(out, "{fleet} #{} {kind} outside drover (PR #{num})", s.issue)?;
        emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(s.issue), kind, d))?;
    }
    Ok(())
}

fn ext_closed(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<()> {
    let events = working(&ctx.state)?;
    let refs: std::collections::BTreeSet<String> = queue::view(&events, &fold(&events), fleet)
        .into_iter()
        .flat_map(|i| i.blockers.into_iter().map(move |b| format!("{fleet}#{b}")).chain(i.ext))
        .collect();
    for b in refs {
        let Some((repo, n)) = b.rsplit_once('#') else { continue };
        let repo = match cfg.fleet.get(repo) {
            Some(f) => &f.repo,
            None => repo,
        };
        let p = gh::cache_path(ctx, "blocker", &b.replace(['/', '#'], "-"), 0);
        let ts = crate::usage::load::<Value>(&p).and_then(|c| c.at("ts").as_u64());
        if ts.is_some_and(|t| ctx.now < t + 300) {
            continue;
        }
        let v = crate::runner::json(r, "gh", &["issue", "view", n, "-R", repo, "--json", "state"]);
        gh::store(&p, &json!({"ts": ctx.now}))?;
        if v.is_ok_and(|v| v.at("state") == "CLOSED") {
            writeln!(out, "{fleet}: blocker {b} closed")?;
            emit(
                &ctx.state,
                cfg,
                Event::new(ctx.now, fleet, None, "blocker-closed", json!({"ref": b})),
            )?;
        }
    }
    Ok(())
}

/// Each pending spawn's fleet with its count, most first (`cooee 3, berth 2`): who holds the slots.
pub fn holders(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> String {
    let mut by = BTreeMap::<String, usize>::new();
    for p in pending(ctx, cfg, r) {
        *by.entry(p.fleet).or_default() += 1;
    }
    let mut by: Vec<_> = by.into_iter().collect();
    by.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let by: Vec<String> = by.iter().map(|(f, n)| format!("{f} {n}")).collect();
    by.join(", ")
}

fn starts(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<()> {
    let events = working(&ctx.state)?;
    let one = |n: &String| KICK.get().is_none_or(|k| n == &k.to_string());
    let acts = |f: &dyn Fn(&Value) -> bool| ACTS.with_borrow(|m| m.get(fleet).is_some_and(|v| v.iter().any(f)));
    let tried =
        |n: &String| acts(&|a| a["step"] == "start" && a["issue"].as_u64().is_some_and(|i| n == &i.to_string()));
    let stop = crate::starts::stops;
    let (mut res, mut go) = (Ok(()), !acts(&|a| a["step"] == "start" && stop(a["code"].as_str())));
    let queued = (queue::startable(&events, fleet).into_iter()).filter(|n| one(n) && !tried(n));
    for n in queued {
        if !go {
            break;
        }
        let at = (fleet, n.parse().ok(), "start");
        let design = n.parse().ok().and_then(|n| queue::wants_design(&events, fleet, n));
        let again = n.parse().is_ok_and(|n| crate::pending::requeued(&events, fleet, n));
        let f = cfg.fleet(fleet)?;
        let own = n.parse().ok().and_then(|n| {
            let sp = crate::events::of_issue(&events, fleet, n)
                .into_iter()
                .rfind(|e| e.kind == "spawn")?;
            sp.str("role").filter(|r| matches!(*r, "lesson" | "triage" | "deps"))
        });
        let c = [
            "spawn",
            fleet,
            &n,
            design.or(own).unwrap_or(f.flow().start()),
            "--supersede",
        ];
        let code = match act(
            ctx,
            cfg,
            r,
            out,
            &events,
            at,
            c.get(..4 + usize::from(design.is_some() || again)).unwrap_or(&c[..]),
        ) {
            Ok(c) => c,
            Err(e) => {
                res = Err(e);
                break;
            }
        };
        go &= !stop(code);
    }
    res.and(crate::pending::stalled(ctx, cfg, fleet))
        .and(clean_merged(ctx, cfg, r, out, &events, fleet))
}

fn clean_merged(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    events: &[Event],
    fleet: &str,
) -> Result<()> {
    let st = fold(events);
    let merged = |e: &&Event| {
        e.issue
            .is_some_and(|n| st.get(&(fleet.into(), n)).is_some_and(IssueState::done))
    };
    let left = (events.iter())
        .filter(|e| matches!(e.kind.as_str(), "spawn" | "spawn-started") && e.fleet == fleet)
        .filter(merged)
        .filter(|e| !crate::hosts::held(ctx, cfg, e));
    if KICK.get().is_none() && !crate::hosts::present(r, left).is_empty() {
        let c = (fleet, None, "clean");
        act(ctx, cfg, r, out, events, c, &["clean", fleet, "--yes"])?;
    }
    Ok(())
}

fn catch_up(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<()> {
    if KICK.get().is_some() {
        return Ok(());
    }
    let Err(e) = crate::land::auto_update(ctx, cfg, r, out, fleet) else {
        return Ok(());
    };
    writeln!(out, "{fleet} auto-update: {e:#}")?;
    let reason = format!("{e:#}").split_whitespace().collect::<Vec<_>>().join(" ");
    let kinds = ["auto-update-failed", "auto-updated"];
    let events = working(&ctx.state)?;
    let last = events
        .iter()
        .rfind(|e| e.fleet == fleet && kinds.contains(&e.kind.as_str()));
    if last.is_some_and(|e| e.kind == kinds[0] && e.str("reason") == Some(&reason)) {
        return Ok(());
    }
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, fleet, None, kinds[0], json!({"reason": reason})),
    )
}

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
    format!(
        "FAIL\n\nCI is red at {head}: {what}.\n\n```\n{}\n```\n",
        crate::lanes::mask(&tail.join("\n"))
    )
}

fn paced(cfg: &Config, fleet: &str) -> Option<String> {
    let max = cfg
        .fleet
        .get(fleet)
        .and_then(|f| f.respawn_per_pass)
        .unwrap_or(cfg.limits.respawn_per_pass);
    let ran = |a: &Value| a["step"] == "respawn" && a["code"].is_null();
    let n = ACTS.with_borrow(|m| m.get(fleet).map_or(0, |v| v.iter().filter(|a| ran(a)).count()));
    (n >= max).then(|| format!("{n} respawns this pass (respawn_per_pass {max}); the rest wait their turn"))
}

type At<'a> = (&'a str, Option<u64>, &'a str);
type Code = Option<&'static str>;

fn act(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, ev: &[Event], at: At, c: &[&str]) -> Result<Code> {
    let (fleet, issue, step) = at;
    let mut buf = vec![];
    let kicks = c.first() == Some(&"spawn") && KICK.get().is_none() && cfg.limits.max_parallel_spawns > 0;
    IN_ACT.set(!kicks);
    let spawn = issue.filter(|_| c.first() == Some(&"spawn"));
    let back = spawn
        .filter(|_| KICK.get().is_none())
        .and_then(|n| retrying(ctx, ev, fleet, n));
    let respawn = c.first() == Some(&"spawn") && step == "respawn" && KICK.get().is_none();
    let lane = issue.and_then(|n| {
        crate::events::of_issue(ev, fleet, n)
            .into_iter()
            .rfind(|e| e.kind == "spawn")
    });
    let wave = lane
        .and_then(|e| e.str("agent"))
        .filter(|_| respawn)
        .and_then(|a| crate::breaker::wave_of(cfg, ev, a));
    if let Some(w) = &wave {
        crate::breaker::wave_alert(ctx, cfg, w)?;
    }
    let paced = (respawn && wave.is_none()).then(|| paced(cfg, fleet)).flatten();
    let full = || {
        (spawn.filter(|_| KICK.get().is_none() && !kicks).zip(c.get(3)))
            .and_then(|(n, role)| crate::hosts::slot_before(ctx, cfg, r, ev, (fleet, n, role), &[]))
    };
    let marked = spawn.filter(|_| KICK.get().is_some() && cfg.limits.max_parallel_spawns > 0 && !SLOT.get());
    let res = match (back, paced, issue.filter(|_| kicks)) {
        (Some(why), _, _) => crate::gate::refused("SPAWN_BACKOFF", why),
        (None, Some(why), _) => crate::gate::refused("RESPAWN_PACED", why),
        (None, None, Some(n)) => launch(ctx, cfg, r, ev, (fleet, n), c, step == "start"),
        (None, None, None) => match (full(), marked) {
            (Some(why), _) => crate::gate::refused("HOST_FULL", why),
            (None, Some(n)) => admitted(ctx, cfg, r, ev, (fleet, n), c, false).and_then(|(l, _)| {
                let _marker = Lock(ctx.state.clone(), l);
                SLOT.set(true);
                let res = crate::cli::run(c, ctx, r, &mut buf);
                SLOT.set(false);
                res
            }),
            (None, None) => crate::cli::run(c, ctx, r, &mut buf),
        },
    };
    IN_ACT.set(false);
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
    let offline = matches!(code, Some("EXIT" | "ERROR")) && result.contains(crate::hosts::OFFLINE);
    let code = if offline { Some("HOST_OFFLINE") } else { code };
    if let Some(n) = issue.filter(|_| code.is_some() && c.first() == Some(&"spawn") && SLOT.get()) {
        crate::db::release(&ctx.state, &format!("spawn-{fleet}-{n}.pending"), None);
    }
    if spawn.is_some() && code == Some("HOST_OFFLINE") && r.recheck() {
        return act(ctx, cfg, r, out, ev, at, c);
    }
    if let Some(n) = spawn.filter(|_| matches!(code, Some("EXIT" | "ERROR"))) {
        let why = result
            .split_once(": ")
            .map_or(&*result, |p| p.1)
            .lines()
            .next()
            .unwrap_or_default();
        let error = why.split("; closed ").next().unwrap_or(why);
        let head = gh::cached_pr(ctx, fleet, n).at("headRefOid").clone();
        let d = json!({"step": step, "error": error, "head": head});
        append(&ctx.state, &Event::new(ctx.now, fleet, Some(n), "spawn-error", d))?;
    }
    if step == "clean" && code.is_none() && !text.contains("removed ") {
        return Ok(None);
    }
    let outcome = match code {
        None if kicks && issue.is_some() => "pending",
        None => PAST.iter().find(|d| d.starts_with(step)).map_or(step, |d| d),
        Some("EXIT" | "ERROR") => "error",
        Some(_) => "waiting",
    };
    let w = if outcome == "pending" {
        outcome.into()
    } else {
        words(code, step, &result)
    };
    let why = code.map(|_| result.split_once(": ").map_or(&*result, |p| p.1));
    if let Some((n, k)) = spawn.zip(code).filter(|_| {
        !cfg.fleet(fleet)
            .is_ok_and(|f| f.flow().is_check(c.get(3).copied().unwrap_or_default()))
    }) {
        crate::pending::refused(ctx, (fleet, n), k, why.unwrap_or_default())?;
    }
    ACTS.with_borrow_mut(|a| {
        a.entry(fleet.into()).or_default().push(
            json!({"issue": issue, "step": step, "outcome": outcome, "code": code, "words": w, "why": why, "kick": kicks}),
        );
    });
    let who = issue.map_or(fleet.to_string(), |n| format!("{fleet} #{n}"));
    if outcome == "pending" {
        writeln!(out, "{who} {step}: pending")?;
        return Ok(None);
    }
    let recent = |e: &Event| e.kind == "driven" && e.fleet == fleet && e.issue == issue && ctx.now < e.ts + 3600;
    let same = |e: &Event| e.str("step") == Some(step) && e.str("code") == code;
    if code.is_some() && ev.iter().any(|e| recent(e) && same(e)) {
        return Ok(code);
    }
    writeln!(out, "{who} {step}: {result}")?;
    let d = json!({"step": step, "cmd": format!("drover {}", c.join(" ")), "result": result, "code": code});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, issue, "driven", d)).map(|()| code)
}
