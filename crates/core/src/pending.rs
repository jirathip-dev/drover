use crate::events::{age, append, Event};
use crate::Ctx;
use anyhow::Result;
use serde_json::json;

/// Refusals that only mean "not yet": the action stays pending.
pub const SOFT: [&str; 11] = [
    "HOST_BUSY",
    "HOST_FULL",
    "MEM_LOW",
    "CAP",
    "IN_FLIGHT_CAP",
    "LOGIN_NEEDED",
    "SPAWN_PENDING",
    "SERIAL_BUSY",
    "PAUSED",
    "RUNTIME_DOWN",
    "HOST_OFFLINE",
];
const SKIP: [&str; 2] = ["IN_FLIGHT", "ALREADY_DONE"];
const DISK: [&str; 2] = ["DISK_LOW", "DISK_BUDGET"];
/// An extend migrated from the old pending start back to its queue place.
pub const REQUEUED: &str = "extend-requeued";
/// A pending action unstarted this long brings its card back.
pub const BACK_SECS: u64 = 1800;
/// The reason of a start held by host load alone: it waits, its card never comes back for it.
pub const LOAD: &str = "HOST_BUSY (load)";

/// An issue's pending action: its latest `pending-action` with no `spawn` of the issue after it.
#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub action: String,
    pub role: String,
    pub since: u64,
    after: Option<u64>,
    pub reason: Option<String>,
    pub failed: bool,
}

impl Pending {
    /// Its card is back: a hard refusal, or (never an extend's) still not started after `BACK_SECS`.
    pub fn back(&self, now: u64) -> bool {
        self.failed || self.action != "extend" && now >= self.since + BACK_SECS && self.reason.as_deref() != Some(LOAD)
    }
    /// The row's line while it waits: `starting, waiting for host: <reason> · <age>`.
    pub fn line(&self, now: u64) -> Option<String> {
        let wait = age(now.saturating_sub(self.since));
        let why = (self.reason.clone()).unwrap_or_else(|| crate::promise::Say::Pass(String::new()).line());
        (!self.failed).then(|| format!("starting, waiting for host: {why} · {wait}"))
    }
    /// A returned card's reason: the refusal and the wait.
    pub fn why(&self, now: u64) -> String {
        let why = self.reason.as_deref().unwrap_or("no refusal logged");
        let wait = age(now.saturating_sub(self.since));
        let how = if self.failed { "refused" } else { "still waiting" };
        format!("{} did not start a lane ({how} after {wait}): {why}", self.action)
    }
}

/// `fleet` #`n`'s pending action, if any: no spawn of the issue after it, nor after its tap's `after` seq (the kick a decision fires may spawn before the record is logged).
pub fn of(events: &[Event], fleet: &str, n: u64) -> Option<Pending> {
    let all = crate::events::of_issue(events, fleet, n);
    let mine = || all.iter().copied().enumerate();
    let (i, e) = mine().rfind(|(_, e)| e.kind == "pending-action")?;
    let num = |k| e.data.get(k).and_then(serde_json::Value::as_u64);
    let (since, after) = (num("since").unwrap_or(e.ts), num("after").unwrap_or(u64::MAX));
    let started = mine().any(|(j, s)| s.kind == "spawn" && (j > i || s.seq > after) || j > i && s.kind == REQUEUED);
    (!started).then(|| Pending {
        action: e.str("action").unwrap_or_default().into(),
        role: e.str("role").unwrap_or_default().into(),
        since,
        after: num("after"),
        reason: e.str("reason").map(String::from),
        failed: e.data.get("failed") == Some(&json!(true)),
    })
}

fn since<'a>(events: &'a [Event], fleet: &str, n: u64) -> Option<Vec<&'a Event>> {
    let mut all = crate::events::of_issue(events, fleet, n);
    let queued =
        |e: &&Event| e.kind == REQUEUED || e.kind == "round-extended" && e.data.get("queued") == Some(&json!(true));
    let at = all.iter().rposition(queued)?;
    let rest = all.split_off(at);
    let ended = |e: &&Event| matches!(e.kind.as_str(), "spawn" | "needs-human" | "parked");
    (!rest.iter().any(ended)).then_some(rest)
}

/// `fleet` #`n` waits in the queue for its extra round's start.
pub fn requeued(events: &[Event], fleet: &str, n: u64) -> bool {
    since(events, fleet, n).is_some()
}

/// Its Queue row: `extended · waiting: <gate>`, the fleet's pause or drive's last refused start; `extended · starting` before any.
pub fn waits(events: &[Event], fleet: &str, n: u64, paused: bool) -> Option<String> {
    let rest = since(events, fleet, n)?;
    let tried = |e: &&&Event| e.kind == "driven" && e.str("step") == Some("start");
    let code = rest.iter().rfind(tried).and_then(|e| e.str("code"));
    let gate = if paused { Some("PAUSED") } else { code };
    Some(gate.map_or("extended · starting".into(), |g| format!("extended · waiting: {g}")))
}

/// Each extend still waiting on the old pending start (unstarted, not refused) goes back to its queue place once.
pub fn requeue_waiting(ctx: &Ctx, cfg: &crate::config::Config) -> Result<()> {
    let ev = crate::events::working(&ctx.state)?;
    let open = |k: &(String, u64)| ev.states.get(k).is_some_and(|s| !s.done() && s.state != "parked");
    let extends = ev
        .iter()
        .filter(|e| e.kind == "pending-action" && e.str("action") == Some("extend"));
    let keys: std::collections::BTreeSet<_> = extends.filter_map(|e| Some((e.fleet.clone(), e.issue?))).collect();
    for (f, n) in keys.into_iter().filter(open) {
        if of(&ev, &f, n).is_some_and(|p| p.action == "extend" && !p.failed) {
            append(
                &ctx.state,
                &Event::new(ctx.now, &f, Some(n), REQUEUED, json!({"by": "migrate"})),
            )?;
            crate::gate::requeue(ctx, cfg, &ev, &f, n)?;
        }
    }
    Ok(())
}

/// Log a tap's pending action, after its decision: a new one, its start not tried yet; a spawn logged past seq `after` (`None`: past this record) started it.
pub fn record(ctx: &Ctx, (fleet, n): (&str, u64), action: &str, role: &str, after: Option<u64>) -> Result<()> {
    let d = json!({"action": action, "role": role, "since": ctx.now, "after": after, "reason": null, "failed": false});
    append(&ctx.state, &Event::new(ctx.now, fleet, Some(n), "pending-action", d))
}

/// A start of `fleet` #`n` refused `code` (`why`): a pending action logs it, failed unless soft; whether it stays pending. a hard refusal of a requeued extend's start is a new needs-human.
pub fn refused(ctx: &Ctx, (fleet, n): (&str, u64), code: &str, why: &str) -> Result<bool> {
    let ev = crate::events::working(&ctx.state)?;
    let wait = SOFT.contains(&code) || SKIP.contains(&code) || DISK.contains(&code);
    let queued = ev.states.get(&(fleet.into(), n)).is_some_and(|s| s.state == "queued");
    if !wait && queued && requeued(&ev, fleet, n) {
        let d = json!({"code": code, "reason": format!("one more round did not start: {code}: {why}"), "start": true});
        append(&ctx.state, &Event::new(ctx.now, fleet, Some(n), "needs-human", d))?;
        return Ok(false);
    }
    let Some(p) = of(&ev, fleet, n).filter(|p| !p.failed) else {
        return Ok(false);
    };
    let soft = SOFT.contains(&code);
    let signals = why.split(" (top:").next().unwrap_or_default();
    let load = code == "HOST_BUSY"
        && signals.contains("load/cpu")
        && !signals.contains("pressure")
        && !signals.contains("swap");
    let reason = match () {
        () if load => LOAD.to_string(),
        () if soft => code.to_string(),
        () => format!("{code}: {why}"),
    };
    if SKIP.contains(&code) || soft && p.reason.as_deref() == Some(&reason) {
        return Ok(true);
    }
    let d = json!({"action": p.action, "role": p.role, "since": p.since, "after": p.after, "reason": reason,
        "failed": !soft});
    append(&ctx.state, &Event::new(ctx.now, fleet, Some(n), "pending-action", d))?;
    Ok(soft)
}
