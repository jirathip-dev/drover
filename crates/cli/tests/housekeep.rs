mod common;
use common::*;
use drover::events::{append, Event};
use drover::runner::{Out, Runner};
use serde_json::json;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

const MARKS: [(&str, &str); 6] = [
    ("check", "check.at"),
    ("clean", "clean.kept"),
    ("budget", "worktrees.du"),
    ("jobs", "lesson-sendmeter.at"),
    ("hotspots", "hotspots-sendmeter.day"),
    ("compact", "compact.at"),
];

fn host(r: Fake) -> Fake {
    r.reply("which shed", "/opt/homebrew/bin/shed\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "4\n")
        .reply("sysctl -n vm.loadavg", "{ 1.00 1.00 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "8\n")
        .reply("vm_stat", "Swapouts: 5.\n")
        .reply("git -C", "")
}

fn marks(c: &drover::Ctx) -> Vec<&'static str> {
    let left = MARKS.iter().filter(|m| c.state.with_file_name(m.1).exists());
    left.map(|m| m.0).collect()
}

#[test]
fn tick_and_its_fleet_passes_run_no_chore_and_housekeep_runs_each() {
    let c = ctx(&tmpdir("housekeep-apart"), CONFIG);
    let _sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let old = drover::timer::files(&c, drover::timer::CLEAN, &[], "", 1, cfg!(target_os = "macos")).remove(0);
    std::fs::create_dir_all(old.0.parent().unwrap()).unwrap();
    std::fs::write(&old.0, "old daily clean").unwrap();
    let (code, out) = run(&c, &host(Fake::default()), &["tick"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("sendmeter #12 start: REFUSED HOST_BUSY"),
        "the pass ran: {out}"
    );
    assert_eq!(marks(&c), Vec::<&str>::new(), "{out}");
    assert!(!old.0.exists(), "the tick's repair removed the old clean job: {out}");
    let (code, out) = run(&c, &host(Fake::default()), &["drive", "sendmeter"]);
    assert_eq!((code, marks(&c)), (0, vec![]), "{out}");

    let (code, out) = run(&c, &host(Fake::default()), &["housekeep"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(marks(&c), MARKS.map(|m| m.0), "{out}");
    let (_, doc) = run(&c, &Fake::default(), &["doctor"]);
    let row = doc
        .lines()
        .find(|l| l.contains(" housekeep — "))
        .unwrap_or_else(|| panic!("{doc}"));
    assert!(row.starts_with("✓ housekeep — ran 0s ago, took "), "{doc}");
}

struct Gate {
    inner: Fake,
    stop: String,
    state: Mutex<(bool, bool)>,
    moved: Condvar,
}

impl Gate {
    fn new(inner: Fake, stop: &str) -> Self {
        Gate {
            inner,
            stop: stop.into(),
            state: Mutex::new((false, false)),
            moved: Condvar::new(),
        }
    }
    fn entered(&self) {
        let s = self.state.lock().unwrap();
        let s = self
            .moved
            .wait_timeout_while(s, Duration::from_secs(60), |s| !s.0)
            .unwrap()
            .0;
        assert!(s.0, "no call reached the gate");
    }
    fn open(&self) {
        self.state.lock().unwrap().1 = true;
        self.moved.notify_all();
    }
}

impl Runner for Gate {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.run_in(prog, args, "")
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.run_for(prog, args, input, Duration::from_secs(60))
    }
    fn run_for(&self, prog: &str, args: &[String], input: &str, limit: Duration) -> anyhow::Result<Out> {
        let mut s = self.state.lock().unwrap();
        let line = format!("{prog} {}", args.join(" "));
        if s.0 || !line.starts_with(&self.stop) {
            s.1 |= s.0;
            self.moved.notify_all();
        } else {
            s.0 = true;
            self.moved.notify_all();
            s = self
                .moved
                .wait_timeout_while(s, Duration::from_secs(60), |s| !s.1)
                .unwrap()
                .0;
        }
        drop(s);
        self.inner.run_for(prog, args, input, limit)
    }
}

fn stuck_clean(c: &drover::Ctx) -> (String, Gate) {
    let w = c.worktrees.join("sendmeter").join("issue-7");
    std::fs::create_dir_all(&w).unwrap();
    let w = w.to_string_lossy().into_owned();
    let d = json!({"role": "impl", "agent": "impl-7", "worktree": w, "pane": "impl-7", "runtime": "shed"});
    append(&c.state, &Event::new(10, "sendmeter", Some(7), "spawn", d)).unwrap();
    let m = json!({"pr": "1", "merge_sha": "m", "into": "staging"});
    append(&c.state, &Event::new(c.now - 86_400, "sendmeter", Some(7), "merged", m)).unwrap();
    let git = host(Fake::default()).reply(&format!("git -C {w} rev-list"), "0\n");
    let gate = Gate::new(git, &format!("git -C {w}"));
    (w, gate)
}

#[test]
fn a_stuck_clean_does_not_delay_a_drive_pass_started_beside_it() {
    let c = ctx(&tmpdir("housekeep-beside"), CONFIG);
    let _sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let (w, gate) = stuck_clean(&c);
    std::thread::scope(|s| {
        let clean = s.spawn(|| run(&c, &gate, &["housekeep", "--only", "clean"]));
        gate.entered();
        let (code, out) = run(&c, &host(Fake::default()), &["tick"]);
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("sendmeter #12 start: REFUSED HOST_BUSY"), "{out}");
        assert!(!clean.is_finished(), "the clean is still stuck while the pass ran");
        gate.open();
        let (code, out) = clean.join().unwrap();
        assert_eq!(code, 0, "{out}");
        assert!(out.contains(&w), "the clean went on to the lane: {out}");
    });
}

#[test]
fn a_chore_past_its_budget_is_logged_and_skipped_and_the_rest_still_run() {
    let c = ctx(
        &tmpdir("housekeep-budget"),
        &CONFIG.replace("stall_minutes = 30", "stall_minutes = 30\nslow_secs = 1"),
    );
    let _sh = FakeShed::attached(&c);
    let (_, gate) = stuck_clean(&c);
    let (code, out) = run(&c, &gate, &["housekeep"]);
    assert_eq!(code, 0, "{out}");
    let (_, _, late) = drover::housekeep::last(&c).unwrap();
    assert_eq!(late, ["clean"], "{out}");
    assert!(out.contains("housekeep: clean past its 1 s budget; skipped"), "{out}");
    assert!(
        c.state.with_file_name("compact.at").exists(),
        "the last chore ran: {out}"
    );
}
