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
