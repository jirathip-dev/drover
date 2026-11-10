//! #775 AC1, alone in its binary so no other call reaps (polls) the abandoned child during the check.
use herdsman::runner::{settle, Ended, Proc};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A child that never ends and ignores SIGKILL (one stuck in disk I/O); counts its kills and polls.
struct Unkillable(Arc<AtomicUsize>, Arc<AtomicUsize>);

impl Proc for Unkillable {
    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.1.fetch_add(1, Relaxed);
        Ok(None)
    }
    fn kill(&mut self) -> std::io::Result<()> {
        self.0.fetch_add(1, Relaxed);
        Ok(())
    }
}

/// #775 AC1: a child that ignores SIGKILL is killed at the limit and abandoned: the caller gets its timeout within the
/// limit plus 1 s, and afterwards nothing polls or waits on it.
#[test]
fn a_child_that_ignores_sigkill_times_out_at_the_limit_and_is_left_alone() {
    let (kills, polls) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let c = Unkillable(Arc::clone(&kills), Arc::clone(&polls));
    let t = Instant::now();
    let e = settle(None, Duration::from_millis(300), || Ok(c)).unwrap();
    let took = t.elapsed();
    assert_eq!(e, Ended::TimedOut);
    assert!(
        took >= Duration::from_millis(300) && took < Duration::from_millis(1300),
        "{took:?}"
    );
    assert_eq!(kills.load(Relaxed), 1, "killed once");
    let after = polls.load(Relaxed);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(polls.load(Relaxed), after, "no thread waits on the abandoned child");
}
