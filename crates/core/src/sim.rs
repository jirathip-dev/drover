use crate::events::Event;
use std::cell::RefCell;

/// A state.db write the simulation's scheduler may switch processes before, or a disk walk's entry.
pub enum Point<'a> {
    Append(&'a Event),
    Lease(&'a str),
    Walk(&'a std::path::Path),
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
        .unwrap_or_else(crate::runner::pid)
}

pub(crate) fn point(p: &Point) {
    HOOKS.with_borrow(|h| h.as_ref().map(|h| (h.point)(p)));
}

pub(crate) fn lstat(p: &std::path::Path) -> std::io::Result<std::fs::Metadata> {
    point(&Point::Walk(p));
    std::fs::symlink_metadata(p)
}
