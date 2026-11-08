//! #680: the seams the deterministic simulation (crates/cli/tests/sim) sets on each of its process threads: the clock,
//! the pid leases carry, and a yield hook at each state.db write. Unset (every real process), each is a no-op.
use crate::events::Event;
use std::cell::RefCell;

/// A state.db write the simulation's scheduler may switch processes before.
pub enum Point<'a> {
    Append(&'a Event),
    Lease(&'a str),
}

/// One simulated process's clock, pid and yield hook.
pub struct Hooks {
    pub now: Box<dyn Fn() -> u64>,
    pub pid: u32,
    pub point: Box<dyn Fn(&Point)>,
}

thread_local!(static HOOKS: RefCell<Option<Hooks>> = const { RefCell::new(None) });

/// Install (or, `None`, remove) this thread's hooks.
pub fn set(h: Option<Hooks>) {
    HOOKS.set(h);
}

pub(crate) fn now() -> Option<u64> {
    HOOKS.with_borrow(|h| h.as_ref().map(|h| (h.now)()))
}

pub(crate) fn pid() -> u32 {
    HOOKS
        .with_borrow(|h| h.as_ref().map(|h| h.pid))
        .unwrap_or_else(std::process::id)
}

pub(crate) fn point(p: &Point) {
    HOOKS.with_borrow(|h| h.as_ref().map(|h| (h.point)(p)));
}
