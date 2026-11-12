//! #778: a card tap that starts a lane ([one more round], an answer whose lane is gone, a design approve or revise)
//! stays answered while the lane waits to start. After its decision the tap logs a `pending-action`; drive retries
//! the spawn every pass, and each refusal it meets is logged again on the record (`reason`). A soft refusal (a busy
//! host, no slot) keeps it pending; a hard one ends it `failed`. Its card is back once failed or 30 min unstarted.
use crate::events::{age, append, Event};
use crate::Ctx;
use anyhow::Result;
use serde_json::json;

/// Refusals that only mean "not yet": the action stays pending.
pub const SOFT: [&str; 8] = [
    "HOST_BUSY",
    "MEM_LOW",
    "CAP",
    "ALL_CAPPED",
    "SPAWN_PENDING",
    "SERIAL_BUSY",
    "PAUSED",
    "HOST_OFFLINE",
];
/// Another pass is on the same spawn (#509), or did it: neither waits nor fails.
const SKIP: [&str; 2] = ["IN_FLIGHT", "ALREADY_DONE"];
/// A pending action unstarted this long brings its card back.
pub const BACK_SECS: u64 = 1800;
/// #825: the reason of a start held by host load alone: it waits, its card never comes back for it.
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
    /// Its card is back: a hard refusal, or still not started after `BACK_SECS`.
    pub fn back(&self, now: u64) -> bool {
        self.failed || now >= self.since + BACK_SECS && self.reason.as_deref() != Some(LOAD)
    }
    /// The row's line while it waits: `starting, waiting for host: <reason> · <age>`.
    pub fn line(&self, now: u64) -> Option<String> {
        let why = self
            .reason
            .clone()
            .unwrap_or_else(|| crate::promise::Say::Pass(String::new()).line());
        let wait = age(now.saturating_sub(self.since));
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

/// `fleet` #`n`'s pending action, if any: no spawn of the issue after it, nor after its tap's `after` seq (the kick
/// a decision fires may spawn before the record is logged).
pub fn of(events: &[Event], fleet: &str, n: u64) -> Option<Pending> {
    let all = crate::events::of_issue(events, fleet, n);
    let mine = || all.iter().copied().enumerate();
    let (i, e) = mine().rfind(|(_, e)| e.kind == "pending-action")?;
    let num = |k| e.data.get(k).and_then(serde_json::Value::as_u64);
    let (since, after) = (num("since").unwrap_or(e.ts), num("after").unwrap_or(u64::MAX));
    let started = mine().any(|(j, s)| s.kind == "spawn" && (j > i || s.seq > after));
    (!started).then(|| Pending {
        action: e.str("action").unwrap_or_default().into(),
        role: e.str("role").unwrap_or_default().into(),
        since,
        after: num("after"),
        reason: e.str("reason").map(String::from),
        failed: e.data.get("failed") == Some(&json!(true)),
    })
}

/// Log a tap's pending action, after its decision: a new one, its start not tried yet; a spawn logged past seq `after`
/// (`None`: past this record) started it.
pub fn record(ctx: &Ctx, (fleet, n): (&str, u64), action: &str, role: &str, after: Option<u64>) -> Result<()> {
    let d = json!({"action": action, "role": role, "since": ctx.now, "after": after, "reason": null, "failed": false});
    append(&ctx.state, &Event::new(ctx.now, fleet, Some(n), "pending-action", d))
}

/// A start of `fleet` #`n` refused `code` (`why`): a pending action logs it, failed unless soft; whether it stays pending.
pub fn refused(ctx: &Ctx, (fleet, n): (&str, u64), code: &str, why: &str) -> Result<bool> {
    let Some(p) = of(&crate::events::read(&ctx.state)?, fleet, n).filter(|p| !p.failed) else {
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
