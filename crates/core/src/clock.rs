use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Seconds since the epoch: the sim's clock when one runs, else the wall clock.
pub fn now() -> u64 {
    crate::sim::now().unwrap_or_else(wall)
}

/// The wall clock's seconds since the epoch, whatever the sim says.
pub fn wall() -> u64 {
    since_epoch().as_secs()
}

/// The wall clock's nanoseconds since the epoch.
pub fn nanos() -> u128 {
    since_epoch().as_nanos()
}

fn since_epoch() -> Duration {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default()
}

/// A monotonic instant.
pub fn instant() -> Instant {
    Instant::now()
}

/// Block this thread for `d`.
pub fn sleep(d: Duration) {
    std::thread::sleep(d);
}

thread_local!(static DEADLINE: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) });

/// This thread's work ends in `d` (a pass's or a kick's `pass_max_secs`).
pub fn end_in(d: Duration) {
    DEADLINE.set(Some(Instant::now() + d));
}

/// The time left before this thread's deadline, if it has one.
pub fn left() -> Option<Duration> {
    DEADLINE.get().map(|t| t.saturating_duration_since(Instant::now()))
}
