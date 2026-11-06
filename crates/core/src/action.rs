//! #509: retry-safe side effects. A step (spawn, supersede, merge, label) logs an `intent` with a stable `action_id`
//! before it acts and `done` or `failed` with the same id after; a retry of a `done` id, or of one in flight, skips. An
//! intent left open past its timeout (a crash) is recovered from the real world, never blindly repeated. The ids come
//! from the step's own inputs, so a drive pass, a kick and a per-issue worker (#345) share them.
use crate::events::{append, Event};
use crate::gate::{refuse, Refusal};
use crate::runner::Runner;
use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// `fleet` #`issue`'s `step` at `key` (a head sha, or the round's last spawn).
pub fn id(fleet: &str, issue: u64, step: &str, key: &str) -> String {
    format!("{fleet}#{issue}:{step}@{key}")
}

/// What a step found for its id.
pub enum Seen {
    /// Its intent is logged: act, then `done`.
    Act(Intent),
    /// Done already (or recovered as done): this `done` event.
    Done(Event),
    /// Another caller's intent, not timed out.
    Busy(Event),
}

impl Seen {
    /// A skip as the refusal drive reports (`IN_FLIGHT`, `ALREADY_DONE`); `Act` hands its intent on.
    pub fn act(self) -> Result<Intent, Refusal> {
        match self {
            Seen::Act(i) => Ok(i),
            Seen::Done(e) => Err(refuse("ALREADY_DONE", format!("skipped: {} done", id_of(&e)))),
            Seen::Busy(e) => Err(refuse("IN_FLIGHT", format!("skipped: {} in flight", id_of(&e)))),
        }
    }
}

fn id_of(e: &Event) -> &str {
    e.str("action_id").unwrap_or("?")
}

/// The open intent of a logged action: `done` it once acted; dropped without (an error) it logs `failed`. A panic
/// leaves it open, as a crash does, for the next pass to recover.
pub struct Intent {
    state: PathBuf,
    ev: Event,
    open: bool,
}

impl Intent {
    pub fn done(mut self, d: Value) -> Result<()> {
        self.open = false;
        finish(&self.state, self.ev.ts, &self.ev, "done", d).map(drop)
    }
}

impl Drop for Intent {
    fn drop(&mut self) {
        if self.open && !std::thread::panicking() {
            let _ = finish(&self.state, self.ev.ts, &self.ev, "failed", json!({}));
        }
    }
}

/// `intent`'s outcome (`done` or `failed`) with its id and step, `d` beside; returns it.
pub fn finish(state: &Path, now: u64, intent: &Event, kind: &str, mut d: Value) -> Result<Event> {
    (d["action_id"], d["step"]) = (json!(id_of(intent)), json!(intent.str("step")));
    let e = Event::new(now, &intent.fleet, intent.issue, kind, d);
    append(state, &e).map(|()| e)
}

/// Log `step`'s intent at `key` (`inputs` beside) unless its id is done or in flight; an intent older than `secs`, or
/// whose process died, is recovered: `probe` checks the real world, `Some` outcome is `done`, `None` is `failed` and
/// this caller acts.
pub fn begin(
    r: &dyn Runner,
    state: &Path,
    now: u64,
    (fleet, issue, step, key): (&str, u64, &str, &str),
    mut inputs: Value,
    secs: u64,
    probe: &dyn Fn(&Event) -> Result<Option<Value>>,
) -> Result<Seen> {
    let id = id(fleet, issue, step, key);
    (inputs["action_id"], inputs["step"]) = (json!(id), json!(step));
    (inputs["pid"], inputs["host"]) = (json!(std::process::id()), json!(crate::db::host()));
    let ev = Event::new(now, fleet, Some(issue), "intent", inputs);
    let intent = |ev: Event| Intent {
        state: state.into(),
        ev,
        open: true,
    };
    let (last, took) = crate::db::claim(state, std::slice::from_ref(&ev), &id, &|l| {
        l.is_none_or(|l| l.kind == "failed")
    })?;
    let Some(last) = last.filter(|_| !took) else {
        return Ok(Seen::Act(intent(ev)));
    };
    if last.kind == "done" {
        return Ok(Seen::Done(last));
    }
    if last.ts + secs > now && !dead(r, &last) {
        return Ok(Seen::Busy(last));
    }
    if let Some(mut d) = probe(&last)? {
        d["recovered"] = json!(true);
        return Ok(Seen::Done(finish(state, now, &last, "done", d)?));
    }
    // Nothing happened: the stale intent failed and this one is logged, both only while it is still the latest.
    let mut failed = Event::new(
        now,
        fleet,
        Some(issue),
        "failed",
        json!({"action_id": id, "step": step}),
    );
    failed.data.insert("recovered".into(), json!(true));
    let (_, took) = crate::db::claim(state, &[failed, ev.clone()], &id, &|l| {
        l.is_some_and(|l| l.seq == last.seq)
    })?;
    Ok(if took { Seen::Act(intent(ev)) } else { Seen::Busy(last) })
}

/// An intent's process, on this host, is gone (a crash): no need to wait out its timeout.
pub fn dead(r: &dyn Runner, e: &Event) -> bool {
    let pid = e.data.get("pid").and_then(Value::as_u64).map(|p| p.to_string());
    let gone = |p: String| r.run("kill", &["-0".into(), p]).is_ok_and(|o| o.code != 0);
    e.str("host") == Some(&crate::db::host()) && pid.is_some_and(gone)
}

/// The latest intent, `done` or `failed` of `fleet` #`issue`'s `step` (any key).
pub fn last<'a>(events: &'a [Event], fleet: &str, issue: u64, step: &str) -> Option<&'a Event> {
    let mine = |e: &&Event| {
        ["intent", "done", "failed"].contains(&e.kind.as_str()) && e.fleet == fleet && e.issue == Some(issue)
    };
    events.iter().rev().filter(mine).find(|e| e.str("step") == Some(step))
}
