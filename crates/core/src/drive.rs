//! #92 `herdsman drive`: one pass of the fleet loop, no AI; each action logs a passive `driven` event.
use crate::config::Config;
use crate::events::{append, emit, fold, read, Event, IssueState};
use crate::overlap::git;
use crate::runner::{ok, Runner};
use crate::waiting::Waiting;
use crate::{gh, queue, waiting, Ctx};
use anyhow::Result;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

/// #168: a lock without a live pid older than this belongs to a pass that died; the next pass takes it over.
const STALE_LOCK_SECS: u64 = 600;

/// #168: `drive.lock` holds the pass's pid, then the fleet a manual press drives (#166); dropping the guard (error or panic included) removes it.
struct Lock(std::path::PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The lock, unless a live pass holds it: a dead pid's lock is stale at once, one without a pid after `STALE_LOCK_SECS`.
fn take(r: &dyn Runner, lock: &Path, fleet: &str) -> Option<Lock> {
    let held = std::fs::read_to_string(lock).unwrap_or_default();
    let pid = held.split_whitespace().next().filter(|p| p.parse::<u32>().is_ok());
    let age = lock
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok());
    let dead = match pid {
        None => age.is_some_and(|a| a.as_secs() > STALE_LOCK_SECS),
        Some(pid) => !alive(r, pid, age.map_or(0, |a| a.as_secs())),
    };
    if dead {
        let _ = std::fs::remove_file(lock);
    }
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(lock)
        .ok()?;
    let _ = write!(f, "{} {fleet}", std::process::id());
    Some(Lock(lock.into()))
}

/// #333: `pid` runs and started no later than `age` secs ago (a reused pid started after its file); no `ps`: alive.
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

/// #333: each spawn marker and kick lock, live or stale: stale once its pid is gone (or reused) or it is older than
/// `spawn_pending_max_secs` (a marker without a pid: by age alone). `remove`: delete each stale one, log `stale-marker`.
pub fn markers(ctx: &Ctx, cfg: &Config, r: &dyn Runner, remove: bool) -> (Vec<String>, Vec<Event>) {
    let (mut live, mut stale) = (vec![], vec![]);
    for p in std::fs::read_dir(state_dir(ctx))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
    {
        let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let spawn = name.strip_prefix("spawn-").and_then(|n| n.strip_suffix(".pending"));
        let Some(id) = spawn.or_else(|| name.strip_prefix("kick-")?.strip_suffix(".lock")) else {
            continue;
        };
        let Some(age) = p
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
        else {
            continue;
        };
        let (age, text) = (age.as_secs(), std::fs::read_to_string(&p).unwrap_or_default());
        let pid = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v["pid"].as_u64());
        let pid = pid.or_else(|| text.split_whitespace().next()?.parse().ok());
        if age <= cfg.limits.spawn_pending_max_secs && pid.is_none_or(|p| alive(r, &p.to_string(), age)) {
            live.push(name);
            continue;
        }
        let (fleet, n) = id.rsplit_once('-').map_or((id, None), |(f, n)| (f, n.parse().ok()));
        let e = Event::new(
            ctx.now,
            fleet,
            n,
            "stale-marker",
            json!({"file": name, "pid": pid, "age": age}),
        );
        if remove && std::fs::remove_file(&p).is_ok() {
            let _ = append(&ctx.state, &e);
        }
        stale.push(e);
    }
    (live, stale)
}

/// #333: `spawns starting N/<max>` and the stale markers swept in the last day: doctor's and the Host tab's line.
pub fn spawns_line(ctx: &Ctx, cfg: &Config, r: &dyn Runner, events: &[Event]) -> String {
    let n = markers(ctx, cfg, r, false)
        .0
        .iter()
        .filter(|m| m.starts_with("spawn-"))
        .count();
    let day = |e: &&Event| e.kind == "stale-marker" && ctx.now < e.ts + 86400;
    let gone: Vec<&str> = events.iter().filter(day).filter_map(|e| e.str("file")).collect();
    let gone = (!gone.is_empty()).then(|| format!(" · stale removed (24 h): {}", gone.join(", ")));
    format!(
        "spawns starting {n}/{}{}",
        cfg.limits.max_parallel_spawns,
        gone.unwrap_or_default()
    )
}

/// #246: the last fast step: (started, secs) from `drive-fast.at`.
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

/// #217: `drive.lock` over an extend's spawn, so no pass starts the same round meanwhile; `None` while a pass runs.
pub fn hold(ctx: &Ctx, r: &dyn Runner, fleet: &str) -> Option<impl Drop> {
    std::fs::create_dir_all(state_dir(ctx)).ok()?;
    take(r, &state_dir(ctx).join("drive.lock"), fleet)
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
    /// #166: each fleet's actions in the running pass, a silent repeat too: its `drive-pass` record.
    static ACTS: RefCell<BTreeMap<String, Vec<Value>>> = const { RefCell::new(BTreeMap::new()) };
    /// #304: the one issue a kick drives (`None`: all); `IN_ACT` while a pass runs a command, which then kicks nothing.
    static KICK: Cell<Option<u64>> = const { Cell::new(None) };
    static IN_ACT: Cell<bool> = const { Cell::new(false) };
}

/// #304: `herdsman kick`: drive `issue`'s due steps now in a detached `kick --now` (a lane on another host: on the
/// main host), output to `kick.log`; a no-op inside a pass's own command. #333: the detached kick's pid; #340: in its
/// own session.
pub fn kick(ctx: &Ctx, r: &dyn Runner, fleet: &str, issue: u64, why: &str) -> Option<u32> {
    let mut a = crate::runner::argv(&["kick", fleet, &issue.to_string(), "--why", why]);
    if IN_ACT.get() || crate::cli::forward(ctx, r, &a).is_some() {
        return None;
    }
    a.push("--now".into());
    r.detach(&ctx.exe, &a, &state_dir(ctx).join("kick.log")).ok()
}

/// #304: `kick --now`: one pass of `issue` alone under the drive lock. A kick while one runs, or within the 10 s
/// after it, is coalesced into one follow-up; a busy lock is retried as one too.
pub fn kicked(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, k: (&str, u64, &str, bool)) -> Result<()> {
    let (fleet, n, why, now) = k;
    if !now {
        cfg.fleet(fleet)?;
        kick(ctx, r, fleet, n, why);
        return Ok(());
    }
    std::fs::create_dir_all(state_dir(ctx))?;
    let again = state_dir(ctx).join(format!("kick-{fleet}-{n}.again"));
    let mut pending = (why == "drive").then(|| Lock(state_dir(ctx).join(format!("spawn-{fleet}-{n}.pending"))));
    let Some(k) = take(r, &state_dir(ctx).join(format!("kick-{fleet}-{n}.lock")), fleet) else {
        return Ok(std::fs::write(&again, why)?);
    };
    for _ in 0..30 {
        // #333: a running kick's lock stays younger than `spawn_pending_max_secs`.
        let lock = std::fs::File::options().append(true).open(&k.0);
        let _ = lock.and_then(|f| f.set_modified(std::time::SystemTime::now()));
        let mut buf = vec![];
        KICK.set(Some(n));
        let res = drive(ctx, cfg, r, &mut buf, Some(fleet), false, false);
        KICK.set(None);
        drop(pending.take()); // #318: a launched kick's marker goes after its first run
        let text = String::from_utf8_lossy(&buf).into_owned();
        out.write_all(&buf)?;
        if text.starts_with("drive: ") {
            std::fs::write(&again, why)?;
        } else {
            let d = json!({"why": why, "result": summary(fleet, &text)});
            append(&ctx.state, &Event::new(ctx.now, fleet, Some(n), "kick", d))?;
        }
        res?;
        r.sleep(10);
        if std::fs::remove_file(&again).is_err() {
            break;
        }
    }
    Ok(())
}

/// #318: a pass's spawn as its own detached kick, at most `max_parallel_spawns` at once, admitted counting those starting.
fn launch(ctx: &Ctx, cfg: &Config, r: &dyn Runner, ev: &[Event], fleet: &str, n: u64, role: &str) -> Result<i32> {
    markers(ctx, cfg, r, true); // #333: only live kicks count; a dead or old marker goes first
    let live = pending(ctx);
    if live.len() >= cfg.limits.max_parallel_spawns || live.iter().any(|e| e.fleet == fleet && e.issue == Some(n)) {
        return crate::gate::refused("SPAWN_PENDING", format!("{} spawns starting", live.len()));
    }
    crate::spawn::lane_admit(ctx, cfg.fleet(fleet)?, ev, fleet, n, role)?;
    let mut d = json!({"role": role, "check": cfg.fleet(fleet)?.flow().is_check(role)});
    let marker = state_dir(ctx).join(format!("spawn-{fleet}-{n}.pending"));
    std::fs::write(
        &marker,
        serde_json::to_string(&Event::new(ctx.now, fleet, Some(n), "spawn", d.clone()))?,
    )?;
    // #333: the marker carries the detached kick's own pid: its death makes the marker stale at once.
    if let Some(pid) = kick(ctx, r, fleet, n, "drive").filter(|_| marker.exists()) {
        d["pid"] = pid.into();
        std::fs::write(
            &marker,
            serde_json::to_string(&Event::new(ctx.now, fleet, Some(n), "spawn", d))?,
        )?;
    }
    Ok(0)
}

/// #318: the `spawn` in each live `spawn-<fleet>-<n>.pending`, one a launched kick is still starting.
pub(crate) fn pending(ctx: &Ctx) -> Vec<Event> {
    let young = |p: &Path| Some(p.metadata().ok()?.modified().ok()?.elapsed().ok()?.as_secs() < STALE_LOCK_SECS);
    let live = |p: &std::path::PathBuf| p.extension() == Some("pending".as_ref()) && young(p) == Some(true);
    let spawn = |p: std::path::PathBuf| serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok();
    let each = std::fs::read_dir(state_dir(ctx)).into_iter().flatten().flatten();
    each.map(|e| e.path()).filter(live).filter_map(spawn).collect()
}

/// #166: a code in plain words (#141's table, here until it lands); the host's own reason follows a busy Mac.
pub(crate) fn words(code: Option<&str>, step: &str, result: &str) -> String {
    let why = result.split_once(": ").map_or(result, |p| p.1);
    let why = why.split("; retry later").next().unwrap_or(why);
    match code {
        None => PAST.iter().find(|d| d.starts_with(step)).map_or(step, |d| d).into(),
        Some("HOST_BUSY") => format!("Mac busy ({why})"),
        Some("DISK_LOW") => format!("disk nearly full ({why})"),
        Some("CAP" | "ALL_CAPPED") => "lane limit or usage cap reached".into(),
        Some("CI_NOT_GREEN") => "waiting for CI".into(),
        Some("SERIAL_BUSY") => "another one-at-a-time issue is in flight".into(),
        Some("LANE_LIVE") if why.starts_with("stale pane") => why.into(),
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
    let f = std::fs::read_to_string(state_dir(ctx).join("drive.lock"));
    f.is_ok_and(|f| f.split_whitespace().nth(1).is_none_or(|f| f == fleet))
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

/// One pass over every fleet (or `only`), under `<state dir>/drive.lock`; `manual`: a press. #212 `fast`: verdicts
/// and merges only, PRs read afresh, under `drive-fast.lock`, so a slow pass starting lanes never holds it up.
pub fn drive(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    only: Option<&str>,
    manual: bool,
    fast: bool,
) -> Result<()> {
    let t0 = std::time::Instant::now();
    only.map(|f| cfg.fleet(f)).transpose()?;
    std::fs::create_dir_all(state_dir(ctx))?;
    let i = usize::from(fast);
    gh::FORCE.set(fast || KICK.get().is_some());
    let stamp = ctx.state.with_file_name(["drive.at", "drive-fast.at"][i]);
    let _ = std::fs::write(&stamp, ctx.now.to_string());
    let lock = state_dir(ctx).join(["drive.lock", "drive-fast.lock"][i]);
    let mine = || take(r, &lock, only.unwrap_or_default()).map(Some); // #318: a kick's lock is `kick-<fleet>-<n>.lock`
    let Some(_held) = KICK.get().map_or_else(mine, |_| Some(None)) else {
        if manual {
            return wait(ctx, r, out, &lock, only);
        }
        return Ok(writeln!(
            out,
            "drive: another pass holds {}; nothing done",
            lock.display()
        )?);
    };
    // #234: the fast step samples every host into the 24 h history (the Host tab's sparklines).
    if fast {
        if let Err(e) = crate::hosts::sample(ctx, cfg, r, &crate::events::read(&ctx.state)?) {
            writeln!(out, "host history: {e:#}")?;
        }
    }
    // #298: berth's state changes since the last pass, logged per lane; the agent list each pass polls is the fallback.
    if let Some(b) = r.berth().filter(|_| !fast) {
        if let Err(e) = crate::berth::intake(ctx, cfg, &b.wire) {
            writeln!(out, "berth events: {e:#}")?;
        }
    }
    // #164: the disk budget before any start (the tick's pass only: a page's one-fleet drive skips the walk).
    if only.is_none() {
        if let Err(e) = crate::check::budget(ctx, cfg, r, out) {
            writeln!(out, "budget: {e:#}")?;
        }
    }
    let mine = |(n, f): &(&String, &crate::config::Fleet)| !f.paused && only.is_none_or(|o| o == n.as_str());
    let fleets: Vec<&str> = cfg.fleet.iter().filter(mine).map(|(n, _)| n.as_str()).collect();
    // #325: an answer that never reached its lane is typed again each fast step, until it lands or the lane is gone.
    let events = fast
        .then(|| crate::events::read(&ctx.state).ok())
        .flatten()
        .unwrap_or_default();
    for u in fleets.iter().flat_map(|f| crate::undelivered(&events, f)) {
        if let Err(e) = crate::deliver(ctx, cfg, r, u, u.str("text").unwrap_or_default(), false) {
            writeln!(out, "redeliver: {e:#}")?;
        }
    }
    // A paused fleet gets no pass (and no `last pass`); one fleet's error doesn't skip the others.
    let (mut res, mut start) = (Ok(()), true);
    ACTS.with_borrow_mut(BTreeMap::clear);
    let mut fail = |name: &str, e: anyhow::Error| {
        let w = format!("failed: {e:#}");
        let a = json!({"step": "pass", "outcome": "error", "code": "ERROR", "words": w});
        ACTS.with_borrow_mut(|m| m.entry(name.into()).or_default().push(a));
        res = Err(e);
    };
    // #337: one look per step (each remote host in one ssh call), saved for the board; the passes scan it.
    let log = || std::fs::metadata(&ctx.state).map(|m| m.len()).ok();
    let mut seen = if fleets.is_empty() {
        crate::snapshot::Snap::read(ctx, r) // every fleet paused: nothing to look at
    } else {
        crate::snapshot::take(ctx, cfg, r)
    };
    // #376: lane-shaped panes no spawn logged are adopted (or closed, stale) before any step reads the lanes.
    if !fleets.is_empty() && KICK.get().is_none() {
        match crate::spawn::orphans(ctx, cfg, (r, &seen), out) {
            Ok(true) => seen = crate::snapshot::take(ctx, cfg, r),
            Ok(false) => {}
            Err(e) => writeln!(out, "orphans: {e:#}")?,
        }
    }
    let grew = log();
    // #182: every fleet's due steps (reviews first) before any fleet starts new work.
    let mut due = vec![];
    for name in &fleets {
        match pass(ctx, cfg, (r, &seen), out, name, fast) {
            Ok(go) => {
                start &= go;
                due.push(*name);
            }
            Err(e) => fail(name, e),
        }
    }
    for name in due.into_iter().filter(|_| !fast) {
        match starts(ctx, cfg, r, out, name, start) {
            Ok(go) => start = go,
            Err(e) => fail(name, e),
        }
    }
    // #337: a step that logged something is looked at again, so the board shows what it did.
    if log() != grew {
        crate::snapshot::take(ctx, cfg, r);
    }
    // #246: the step's length beside its start (`drive-fast.at`: `<started> <secs>`), for the Host tab.
    let secs = (t0.elapsed().as_secs_f64() * 10.0).round() / 10.0;
    if fast {
        let _ = std::fs::write(&stamp, format!("{} {secs}", ctx.now));
    }
    for &name in &fleets {
        let acts = ACTS.with_borrow_mut(|m| m.remove(name)).unwrap_or_default();
        if !fast && KICK.get().is_none() {
            let _ = std::fs::write(state_dir(ctx).join(format!("drive-{name}.last")), ctx.now.to_string());
        } else if acts.is_empty() {
            continue; // a fast step or kick that did nothing logs no pass
        }
        // #166: one `drive-pass` per fleet and pass, manual or automatic: the board's result line.
        let end = crate::now();
        let (by, at) = (
            ["auto", "manual", "fast"][usize::from(manual) + 2 * usize::from(fast)],
            crate::usage::date(r, end, "+%H:%M"),
        );
        let d = json!({"by": by, "started": ctx.now, "ended": end, "at": at, "secs": secs,
            "spawns_launched": acts.iter().filter(|a| a["kick"] == true && a["code"].is_null()).count(), "actions": acts});
        let _ = append(&ctx.state, &Event::new(ctx.now, name, None, "drive-pass", d));
    }
    // #343: a slow pass folds the log into its checkpoint, and rotates a log over `rotate_mb`; #365: reads backlogs.
    if !fast && KICK.get().is_none() {
        for n in &fleets {
            crate::gh::backlog(ctx, r, cfg.fleet(n)?, n).unwrap_or_else(|e| eprintln!("herdsman: {n} backlog: {e:#}"));
        }
        let max = cfg.limits.rotate_mb << 20;
        let cp = crate::events::checkpoint(&ctx.state).and_then(|_| crate::events::rotate(&ctx.state, max, ctx.now));
        cp.unwrap_or_else(|e| eprintln!("herdsman: checkpoint: {e:#}"));
    }
    res
}

/// One fleet: each item's due step, reviews first. Each spawn samples the host afresh; false when a spawn met a
/// busy host or full disk, so no fleet starts new work this pass. `fast`: verdicts and merges only (#212).
fn pass(
    ctx: &Ctx,
    cfg: &Config,
    (r, seen): (&dyn Runner, &dyn Runner),
    out: &mut dyn Write,
    fleet: &str,
    fast: bool,
) -> Result<bool> {
    let f = cfg.fleet(fleet)?;
    let events = read(&ctx.state)?;
    let agents = crate::hosts::all_agents(cfg, seen)?;
    let one = |(s, _): &(IssueState, Waiting)| KICK.get().is_none_or(|k| k == s.issue);
    let mut waits = waiting::scan(ctx, cfg, seen, &events, &agents, true);
    waits.retain(one);
    external(ctx, cfg, r, out, fleet)?;
    if KICK.get().is_none() {
        if let Err(e) = crate::land::watch(ctx, cfg, r, out, fleet) {
            writeln!(out, "{fleet} land: {e:#}")?;
        }
    }
    if !fast && handoff(ctx, cfg, out, fleet, &events, &waits)? {
        waits = waiting::scan(ctx, cfg, seen, &read(&ctx.state)?, &agents, false);
        waits.retain(one);
    }
    // #129: ready PRs that conflict with nothing merge first; of a conflicting pair the older, the other is updated.
    let preview = || crate::overlap::preview(ctx, cfg, r, fleet, &events).ok();
    let conflicts = (!fast).then(preview).flatten().unwrap_or_default();
    // #173: a review is a spawn of any check stage's role, not only `rev`.
    let rev = |w: &Waiting| {
        w.next_cmd
            .as_deref()
            .and_then(|c| c.split(' ').nth(4))
            .is_some_and(|r| f.family(r) == "rev")
    };
    // #207: every written verdict is recorded before any merge (and the auto-update after it) moves a head.
    let verdict = |w: &Waiting| {
        w.next_cmd
            .as_deref()
            .is_some_and(|c| c.starts_with("herdsman verdict "))
    };
    waits.sort_by_key(|(s, w)| (!verdict(w), !rev(w), conflicts.contains_key(&s.issue)));
    let (mut go, mut updated) = (true, false);
    for (s, w) in waits
        .iter()
        .filter(|(s, w)| s.fleet == fleet && s.lane.as_ref() == Some(&w.lane))
    {
        // #212: the fast and slow steps never act on one issue at once.
        let lock = state_dir(ctx).join(format!("drive-{fleet}-{}.lock", s.issue));
        let Some(_mine) = take(r, &lock, fleet) else { continue };
        let mut pr = gh::cached_pr(ctx, fleet, s.issue);
        crate::land::mark(ctx, Some(r), f, (fleet, s.issue), &events, &mut pr);
        let ci = gh::ci(&gh::required(&pr["statusCheckRollup"], f.checks(&pr)));
        let n = s.issue.to_string();
        // #294: a PR's diff joins its serial groups; an issue waiting for a slot does nothing else this pass.
        if pr["state"] == "OPEN" && !f.serial.is_empty() && serial(ctx, cfg, r, out, (fleet, s), &pr, fast)? {
            continue;
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
                ("spawn", Some(r)) if f.flow().is_check(r) => "review",
                ("spawn", _) if cmd.ends_with("--update") => "conflict",
                ("spawn", _) if cmd.ends_with("--supersede") => "fix",
                ("spawn", _) => "respawn",
                (v, _) => v,
            };
            if fast && !matches!(step, "verdict" | "merge") {
                continue;
            }
            if w.what.starts_with("reviewer exited") {
                let d = json!({"role": args.get(3), "why": "exited without a verdict"});
                append(&ctx.state, &Event::new(ctx.now, fleet, Some(s.issue), "lane-lost", d))?;
            }
            let mut code = act(ctx, cfg, r, out, &events, (fleet, Some(s.issue), step), &args)?;
            go &= !matches!(code, Some("HOST_BUSY" | "DISK_LOW"));
            // #182: a PASS recorded this pass merges in it once CI is green at that head (a merge label's full run
            // starts only now, so that one waits for the next pass).
            let mut merge = step == "merge";
            let full = f.merge_label.is_some() && pr["landing"] != "fast-first";
            let green = ci == "green" && pr["headRefOid"].as_str() == args.get(5).copied() && !full;
            if step == "verdict" && code.is_none() && args[3] == "PASS" && green {
                merge = true;
                code = if pr["mergeable"] == "CONFLICTING" {
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
            // #214: a merge that conflicts (known before, or failed on it) gets its `--update` lane instead.
            if merge && !fast && conflicted(fleet, code) {
                let code = update_lane(ctx, cfg, r, out, &events, fleet, s.issue)?;
                go &= !matches!(code, Some("HOST_BUSY" | "DISK_LOW"));
            } else if merge && !fast && code.is_none() {
                updated = true;
                if let Err(e) = auto_update(ctx, cfg, r, out, fleet) {
                    writeln!(out, "{fleet} auto-update: {e:#}")?;
                }
            }
        } else if s.state == "blocked" && s.last_event != "no-action" {
            // #214: a block drive has no step for is logged once, never left silent.
            writeln!(out, "{fleet} #{n} no-action: {}", w.what)?;
            let d = json!({"why": w.what});
            append(&ctx.state, &Event::new(ctx.now, fleet, Some(s.issue), "no-action", d))?;
        } else if !fast && s.state == "ready" && ci == "failed" {
            // Red CI after a PASS is a FAIL at the same head (no new round), the failed checks as the finding.
            let head = pr["headRefOid"].as_str().unwrap_or_default();
            let file = state_dir(ctx).join(format!("ci-{fleet}-{n}.md"));
            std::fs::write(&file, ci_findings(r, f, &pr, head))?;
            let file = file.to_string_lossy();
            let args = ["verdict", fleet, &n, "FAIL", "--sha", head, "--file", &file];
            act(ctx, cfg, r, out, &events, (fleet, Some(s.issue), "ci-red"), &args)?;
        }
    }
    // #214: clean catch-ups run every slow pass, not only after a merge.
    if !updated && !fast {
        if let Err(e) = auto_update(ctx, cfg, r, out, fleet) {
            writeln!(out, "{fleet} auto-update: {e:#}")?;
        }
    }
    Ok(go)
}

/// #214: the merge just tried was refused or failed on a conflict (its words say so).
fn conflicted(fleet: &str, code: Code) -> bool {
    let last = ACTS.with_borrow(|m| m.get(fleet).and_then(|a| a.last()).map(|a| a["words"].to_string()));
    let w = last.unwrap_or_default().to_lowercase();
    code == Some("NOT_MERGEABLE") || code.is_some() && (w.contains("not mergeable") || w.contains("conflict"))
}

/// #294: join `s` to each serial group its PR's diff first touches (`serial-joined`); joining a full one, or any after
/// one it waits for, holds it there (`serial-held`) and tells its lane to stop. A slot that frees goes to the first held
/// issue in queue order (`serial-slot`); once it holds all its groups (name order: no deadlock) it gets an update round.
/// True while it waits.
fn serial(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, s): (&str, &IssueState),
    pr: &Value,
    fast: bool,
) -> Result<bool> {
    let (f, n, ev) = (cfg.fleet(fleet)?, s.issue, read(&ctx.state)?);
    let (st, q) = (fold(&ev), queue::replay(&ev, fleet));
    let mine = queue::groups(&q, &ev, f, fleet, n);
    let (mut wait, mut text) = (mine.iter().any(|g| queue::held(&ev, fleet, n, &g.name)), None);
    let (num, head) = (pr["number"].to_string(), pr["headRefOid"].as_str().unwrap_or_default());
    for g in queue::groups(&q, &[], f, fleet, n) {
        let has = |e: &Event| e.is("serial-joined", fleet, n) && e.str("group") == Some(&g.name);
        if !ev.iter().any(has) {
            let d = json!({"group": g.name, "by": "mark"});
            emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), "serial-joined", d))?;
        }
    }
    let new = crate::gate::unjoined(ctx, r, f, (fleet, n), (&num, head)).unwrap_or_default();
    // Drive sees a push before its review, so a first join at review/ready is work from before the group (grandfathered).
    let old = s.state.starts_with("review") || s.state == "ready";
    let mut by = ev
        .iter()
        .rev()
        .find(|e| e.is("serial-held", fleet, n))
        .and_then(|e| e.data["holder"].as_u64());
    for (g, p) in &new {
        writeln!(out, "{fleet} #{n} serial-joined {}: {p}", g.name)?;
        let d = json!({"group": g.name, "by": "diff", "path": p});
        emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), "serial-joined", d))?;
        let h = queue::holders(&ev, &st, f, fleet, &g.name);
        if !old && (wait || h.len() >= g.max) {
            let h = h.first().map(|h| h.issue).or(by).unwrap_or(0);
            by = Some(h);
            let d = json!({"group": g.name, "holder": h});
            emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), "serial-held", d))?;
            let stop = format!(
                "stop: {} is held by #{h}; you'll get an update round when it merges",
                g.name
            );
            (wait, text) = (true, text.or(Some(stop)));
        }
    }
    // Ordered locking: held in one group, it lets go of its slots in later ones, so no two issues hold-and-wait crossed.
    let (now, mut blocker) = (read(&ctx.state)?, None);
    for g in queue::groups(&q, &now, f, fleet, n) {
        let h = queue::held(&now, fleet, n, &g.name);
        if let (false, Some(b)) = (h, blocker) {
            let d = json!({"group": g.name, "holder": b, "by": "order"});
            emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), "serial-held", d))?;
        } else if h && blocker.is_none() {
            let last = now
                .iter()
                .rev()
                .find(|e| e.is("serial-held", fleet, n) && e.str("group") == Some(&g.name));
            blocker = Some(last.and_then(|e| e.data["holder"].as_u64()).unwrap_or(0));
        }
    }
    if let (Some(text), Some(lane)) = (text, s.lane.as_deref()) {
        writeln!(out, "{fleet} #{n} serial-held: {text}")?;
        let sp = crate::events::last_spawn(&ev, lane);
        let pane = sp.and_then(|e| e.str("pane")).unwrap_or_default();
        let dsh = sp.and_then(|e| e.str("harness")) == Some("dsh");
        crate::herdr::reply(&crate::hosts::On::of(r, sp), dsh, lane, pane, &text).ok();
    }
    if !wait || fast || !new.is_empty() {
        return Ok(wait);
    }
    let pos = |i: u64| q.iter().position(|x| x.issue == i).unwrap_or(usize::MAX);
    for g in mine.into_iter().filter(|g| queue::held(&ev, fleet, n, &g.name)) {
        // Another waiter is a candidate only once this is its first held group (its earlier slots taken).
        let first_held = |i: u64| {
            queue::groups(&q, &ev, f, fleet, i)
                .into_iter()
                .find(|y| queue::held(&ev, fleet, i, &y.name))
        };
        let waits = |x: &&IssueState| {
            x.fleet == fleet
                && queue::in_flight(&x.state)
                && queue::held(&ev, fleet, x.issue, &g.name)
                && (x.issue == n || first_held(x.issue).is_some_and(|y| y.name == g.name))
        };
        let first = st.values().filter(waits).min_by_key(|x| (pos(x.issue), x.issue));
        if queue::holders(&ev, &st, f, fleet, &g.name).len() >= g.max || first.map(|x| x.issue) != Some(n) {
            return Ok(true);
        }
        let e = Event::new(ctx.now, fleet, Some(n), "serial-slot", json!({"group": g.name}));
        emit(&ctx.state, cfg, e)?;
    }
    update_lane(ctx, cfg, r, out, &ev, fleet, n)?;
    Ok(true)
}

/// A ready PR's conflict: the `--update` round of the role the gate's `conflict` outcome goes to.
fn update_lane(
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

/// #182: an impl/fix lane idle ≥ 2 min whose open PR head moved past its last review or handoff, CI done there: the
/// PR hands the head off (`pushed`, via pr) though the lane never did, and its review is due. True if one was logged.
fn handoff(
    ctx: &Ctx,
    cfg: &Config,
    out: &mut dyn Write,
    fleet: &str,
    ev: &[Event],
    waits: &[(IssueState, Waiting)],
) -> Result<bool> {
    let f = cfg.fleet(fleet)?;
    let mut any = false;
    for (s, w) in waits
        .iter()
        .filter(|(s, w)| s.fleet == fleet && s.lane.as_ref() == Some(&w.lane))
    {
        let pr = gh::cached_pr(ctx, fleet, s.issue);
        let head = pr["headRefOid"].as_str().unwrap_or_default();
        let handed = ev
            .iter()
            .any(|e| e.is("pushed", fleet, s.issue) && e.str("sha") == Some(head));
        let building = matches!(s.state.split('(').next(), Some("impl" | "fix"));
        let ci = gh::ci(&gh::required(&pr["statusCheckRollup"], &f.required_checks));
        let idle = w.who == "none" && ctx.now >= w.since + 120;
        if !(idle && building && ci != "pending" && pr["state"] == "OPEN") || head.is_empty() {
            continue;
        }
        if s.reviewed(head) {
            any |= no_change(ctx, cfg, out, (fleet, s.issue), ev, head, ci)?;
            continue;
        }
        if handed {
            continue;
        }
        writeln!(out, "{fleet} #{} pushed: {head:.7} via pr", s.issue)?;
        let d = json!({"sha": head, "via": "pr"});
        emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(s.issue), "pushed", d))?;
        any = true;
    }
    Ok(any)
}

/// #319: a settled fix lane left the head of the FAIL as it was. A CI-only FAIL (a gate's CI red, or every blocker tagged
/// `[ci]`) with CI green there now is resolved: a PASS at that head, so it merges. Any other FAIL after `herdsman done
/// --no-change` is reviewed again at the same head (`waiting`). True if `fix-no-change` was logged.
fn no_change(
    ctx: &Ctx,
    cfg: &Config,
    out: &mut dyn Write,
    (fleet, n): (&str, u64),
    ev: &[Event],
    head: &str,
    ci: &str,
) -> Result<bool> {
    let Some(i) = ev.iter().rposition(|e| e.is("verdict", fleet, n)) else {
        return Ok(false);
    };
    let (v, since) = (&ev[i], &ev[i + 1..]);
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

/// #285: a tracked issue whose PR (as last read) was merged or closed outside herdsman is logged once, no comment:
/// `merged`/`closed` `by: external`, or `blocked` while its issue is open; `clean` takes its lanes.
fn external(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<()> {
    let events = read(&ctx.state)?;
    for s in fold(&events)
        .into_values()
        .filter(|s| s.fleet == fleet && !s.done() && s.state != "parked" && KICK.get().is_none_or(|k| k == s.issue))
    {
        let pr = gh::cached_pr(ctx, fleet, s.issue);
        let num = pr["number"].to_string();
        let seen = |e: &Event| e.is("blocked", fleet, s.issue) && e.str("pr") == Some(&num);
        // #350: the PR of a landing reverted (or fixed forward) stays merged; its issue is in a fix round.
        let back = |e: &Event| e.issue == Some(s.issue) && e.fleet == fleet && e.str("landed_pr") == Some(&num);
        let closed = || gh::closed(ctx, r, cfg.fleet(fleet)?, fleet, ctx.now).map(|c| c.contains(&s.issue));
        let (kind, mut d) = match pr["state"].as_str() {
            Some("MERGED") if !events.iter().any(back) => {
                let m = json!({"into": pr["baseRefName"], "merge_sha": pr["mergeCommit"]["oid"], "merged_at": pr["mergedAt"]});
                ("merged", m)
            }
            Some("CLOSED") if closed()? => ("closed", json!({})),
            Some("CLOSED") if !events.iter().any(seen) => (
                "blocked",
                json!({"code": "PR_CLOSED", "reason": "PR closed outside herdsman"}),
            ),
            _ => continue,
        };
        (d["pr"], d["by"]) = (json!(num), json!("external"));
        writeln!(out, "{fleet} #{} {kind} outside herdsman (PR #{num})", s.issue)?;
        emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(s.issue), kind, d))?;
    }
    Ok(())
}

/// One fleet's starts (every startable queued issue in order), then clean merged lanes. A refused item is skipped,
/// `CAP` ends this fleet's starts, `HOST_BUSY`/`DISK_LOW`/`ALL_CAPPED` all fleets' (false).
fn starts(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str, start: bool) -> Result<bool> {
    let (events, f) = (read(&ctx.state)?, cfg.fleet(fleet)?);
    let mut go = start;
    let one = |n: &String| KICK.get().is_none_or(|k| n == &k.to_string());
    for n in queue::startable(&events, fleet)
        .into_iter()
        .filter(one)
        .take_while(|_| start)
    {
        let at = (fleet, n.parse().ok(), "start");
        let code = act(ctx, cfg, r, out, &events, at, &["spawn", fleet, &n, f.flow().start()])?;
        go &= !matches!(
            code,
            Some("HOST_BUSY" | "DISK_LOW" | "DISK_BUDGET" | "MEM_LOW" | "ALL_CAPPED")
        );
        if matches!(
            code,
            Some("HOST_BUSY" | "DISK_LOW" | "DISK_BUDGET" | "MEM_LOW" | "CAP" | "ALL_CAPPED")
        ) {
            break;
        }
    }
    let st = fold(&events);
    let merged = |e: &&Event| {
        e.issue
            .is_some_and(|n| st.get(&(fleet.into(), n)).is_some_and(IssueState::done))
    };
    let left = |e: &&Event| {
        e.str("worktree")
            .is_some_and(|w| crate::hosts::On::of(r, Some(e)).exists(Path::new(w)))
    };
    if KICK.get().is_none()
        && events
            .iter()
            .filter(|e| e.kind == "spawn" && e.fleet == fleet)
            .filter(merged)
            .any(|e| left(&e))
    {
        let c = (fleet, None, "clean");
        act(ctx, cfg, r, out, &events, c, &["clean", fleet, "--yes"])?;
    }
    Ok(go)
}

/// #129: after a merge, merge integration into every other open ready PR whose lane isn't working, oldest PR first, in
/// a scratch worktree (#207: never one in review, whose verdict would go stale). A clean merge is pushed (`auto-updated`)
/// only when needed (#285: `update_ready`), else the PR merges as it is; a ready PR's conflict gets an `--update` lane.
pub fn auto_update(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<()> {
    if KICK.get().is_some() {
        return Ok(()); // #304: a kick leaves every other issue to the timers
    }
    let (f, events, ag) = (
        cfg.fleet(fleet)?,
        read(&ctx.state)?,
        crate::hosts::all_agents(cfg, r).unwrap_or_default(),
    );
    let busy = |l: &str| ag.iter().any(|a| a["name"] == l && a["agent_status"] == "working");
    let open = |s: &IssueState| {
        let pr = gh::cached_pr(ctx, fleet, s.issue);
        pr["number"].as_u64().filter(|_| pr["state"] == "OPEN")
    };
    let mut prs: Vec<(u64, IssueState)> = (fold(&events).into_values())
        .filter(|s| s.fleet == fleet && s.state == "ready")
        .filter(|s| !s.lane.as_deref().is_some_and(busy))
        .filter_map(|s| Some((open(&s)?, s)))
        .collect();
    if prs.is_empty() {
        return Ok(());
    }
    prs.sort_by_key(|p| p.0);
    let (dir, wt) = (f.local_path(), ctx.state.with_file_name(format!("update-{fleet}")));
    let (wts, into) = (wt.to_string_lossy(), format!("origin/{}", f.integration));
    git(r, &dir, &["fetch", "-q", "origin"])?;
    let checks = crate::gate::premerge_checks(ctx, f);
    // A scratch `git worktree`, not a herdr one: no pane, no lane.
    let tree = |a: &[&str]| git(r, &dir, &[&["worktree"][..], a].concat());
    for (_, s) in prs {
        let (n, branch) = (s.issue.to_string(), crate::gate::branch_of(Some(&s), s.issue));
        let from = git(r, &dir, &["rev-parse", &format!("origin/{branch}")])?;
        let _ = tree(&["remove", "--force", &wts]);
        tree(&["add", "-f", "--detach", &wts, &from])?;
        let clean = git(r, &wt, &["merge", "--no-edit", &into]).is_ok();
        let to = git(r, &wt, &["rev-parse", "HEAD"])?;
        // #350: a behind fast-first PR is always caught up: it lands only on the integration its fast checks ran on.
        let mut pr = gh::cached_pr(ctx, fleet, s.issue);
        crate::land::mark(ctx, Some(r), f, (fleet, s.issue), &events, &mut pr);
        let push = to != from && (pr["landing"] == "fast-first" || gh::update_ready(ctx, Some(r), f, fleet));
        // #373: the premerge checks re-run on the updated head before it is pushed; a failure is its fix round.
        let fail = crate::gate::run_checks(r, &wt, &into, if clean && push { &checks[..] } else { &[] });
        let res = if !clean {
            update_lane(ctx, cfg, r, out, &events, fleet, s.issue).map(drop)
        } else if let Some((c, o)) = fail {
            let at = format!("{from:.7} updated onto {into}");
            let rf = crate::gate::premerge_failed(ctx, cfg, r, out, (fleet, s.issue), (&from, &at), (c, &o));
            rf.and_then(|rf| Ok(writeln!(out, "{fleet} #{n} {rf}")?))
        } else if push {
            git(r, &wt, &["push", "-q", "origin", &format!("HEAD:{branch}")]).and_then(|_| {
                gh::forget_pr(ctx, fleet, s.issue);
                writeln!(out, "{fleet} #{n} auto-updated: {from:.7} → {to:.7}")?;
                let d = json!({"from": from, "to": to});
                emit(
                    &ctx.state,
                    cfg,
                    Event::new(ctx.now, fleet, Some(s.issue), "auto-updated", d),
                )?;
                // #285: the PASS carries to the new head: its merge label goes back on now, so the full run starts.
                pr["headRefOid"] = json!(to);
                crate::gate::sync_label(ctx, cfg, r, out, &s, &pr)
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
    let kicks = c[0] == "spawn" && KICK.get().is_none() && cfg.limits.max_parallel_spawns > 0;
    IN_ACT.set(!kicks);
    let res = match issue.filter(|_| kicks) {
        Some(n) => launch(ctx, cfg, r, ev, fleet, n, c[3]),
        None => crate::cli::run(c, ctx, r, &mut buf),
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
        a.entry(fleet.into())
            .or_default()
            .push(json!({"issue": issue, "step": step, "outcome": outcome, "code": code, "words": w, "kick": kicks}));
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
