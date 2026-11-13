//! #972: a worktree root on a dead disk (its listing never returns) costs a pass one probe: drive and check go on,
//! a new lane takes the next root, the stuck root's lanes show `root unreachable`, `root-hung` alerts once per
//! episode; a tick stuck on one step past 3 × `slow_secs` shows in `doctor` and the next `drive --fast`.
mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const NOW: u64 = 1_000_000;

struct Roots {
    c: drover::Ctx,
    dead: PathBuf,
    good: PathBuf,
}

fn setup(name: &str) -> Roots {
    let d = tmpdir(name);
    let (dead, good) = (d.join("nvme"), d.join("internal"));
    for p in [&dead, &good] {
        std::fs::create_dir_all(p.join("sendmeter")).unwrap();
    }
    let roots = format!("roots = [\"{}\", \"{}\"]", dead.display(), good.display());
    let base = CONFIG
        .replace("harness  =  \"hermes\"", "harness = \"claude\"")
        .replace("[limits]\n", "[limits]\ndisk_trust_secs = 0\n");
    let c = ctx(&d, &format!("{base}\n[hosts.mini]\n{roots}\n"));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    // #11's lane works on the dead disk.
    let wt = dead.join("sendmeter/issue-11");
    let d = json!({"role": "impl", "agent": "sendmeter-impl-11", "harness": "claude", "pane": "w8:p1",
                   "workspace": "w8", "worktree": wt, "root": dead, "branch": "issue-11", "host": "mini"});
    append(&c.state, &Event::new(NOW - 600, "sendmeter", Some(11), "spawn", d)).unwrap();
    Roots { c, dead, good }
}

fn host(r: &Roots) -> Fake {
    let wt = r.dead.join("sendmeter/issue-11");
    Fake::default()
        .reply(
            "herdr status",
            "client:\n  version: 0.9.5\n\nserver:\n  status: running\n  version: 0.9.5\n",
        )
        .reply(
            "herdr agent list",
            &agents(&[("sendmeter-impl-11", "working", &wt.to_string_lossy(), "w8:p1")]),
        )
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 0.4 1 1 }")
        .reply("sysctl -n hw.ncpu", "8")
        .reply("vm_stat", "Swapouts: 5.\n")
        .reply("gh issue view", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply("git ", "")
        .reply(
            "herdr worktree create",
            r#"{"result":{"workspace":{"workspace_id":"w1"}}}"#,
        )
        .reply(
            "herdr pane list --workspace w1",
            r#"{"result":{"panes":[{"pane_id":"w1:p1","agent":null}]}}"#,
        )
        .reply("herdr pane run w1:p1 export", "{}")
        .reply("herdr agent start", "{}")
        .reply("herdr agent wait", "{}")
}

/// The disk under `root` stops answering: its listing blocks until the sender drops.
fn hang(root: &Path) -> std::sync::mpsc::Sender<()> {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let dir = root.to_string_lossy().into_owned();
    assert!(drover::runner::hung(&dir, Duration::ZERO, move |_| {
        let _ = rx.recv();
        None
    }));
    tx
}

fn hung_alerts(c: &drover::Ctx) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "root-hung")
        .collect()
}

#[test]
fn a_root_whose_listing_never_returns_costs_one_probe_and_the_pass_goes_on() {
    let r = setup("hung-root-tick");
    let _held = hang(&r.dead);
    assert_eq!(run(&r.c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let f = host(&r);
    let t = Instant::now();
    let (code, out) = run(&r.c, &f, &["tick"]);
    let took = t.elapsed();
    assert_eq!(code, 0, "{out}");
    assert!(
        took < drover::runner::probe_wait() + Duration::from_secs(20),
        "{took:?}"
    );
    assert!(
        f.called("herdr agent start sendmeter-impl-12"),
        "drive still starts: {out}"
    );
    let events = read(&r.c.state).unwrap();
    let sp = events
        .iter()
        .rfind(|e| e.is("spawn", "sendmeter", 12))
        .expect("#12 spawned");
    assert_eq!(
        sp.str("root"),
        Some(&*r.good.to_string_lossy()),
        "#12 takes the root that answers"
    );
    let alerts = hung_alerts(&r.c);
    assert_eq!(alerts.len(), 1, "{alerts:#?}");
    assert_eq!(alerts[0].str("root"), Some(&*r.dead.to_string_lossy()));
    let (_, w) = run(&r.c, &f, &["waiting", "sendmeter"]);
    let line = w.lines().find(|l| l.contains("#11")).unwrap_or_default();
    assert!(line.contains("root unreachable"), "{w}");
}

#[test]
fn root_hung_alerts_once_per_episode_and_clears_when_the_root_answers() {
    let r = setup("hung-root-episode");
    let f = host(&r);
    let held = hang(&r.dead);
    for _ in 0..2 {
        assert_eq!(run(&r.c, &f, &["check"]).0, 0);
    }
    assert_eq!(hung_alerts(&r.c).len(), 1, "once while it stays hung");
    drop(held);
    let back = Instant::now() + Duration::from_secs(5);
    while drover::runner::hung(&r.dead.to_string_lossy(), Duration::from_secs(1), |_| None) {
        assert!(Instant::now() < back, "the probe ended");
    }
    assert_eq!(run(&r.c, &f, &["check"]).0, 0);
    let events = read(&r.c.state).unwrap();
    assert!(events.iter().any(|e| e.kind == "root-answers"), "the episode clears");
    let (_, w) = run(&r.c, &f, &["waiting", "sendmeter"]);
    assert!(!w.contains("root unreachable"), "{w}");
    let _again = hang(&r.dead);
    assert_eq!(run(&r.c, &f, &["check"]).0, 0);
    assert_eq!(hung_alerts(&r.c).len(), 2, "a new episode alerts again");
}

#[test]
fn a_tick_stuck_on_one_step_shows_in_doctor_and_the_next_fast_drive() {
    let r = setup("hung-root-step");
    let mark = r.c.state.with_file_name("tick.step");
    let step = |at: u64| json!({"since": NOW - 4000, "at": at, "step": "check", "child": "", "pid": 1}).to_string();
    std::fs::write(&mark, step(NOW - 120)).unwrap();
    let (_, out) = run(&r.c, &Fake::default(), &["doctor"]);
    assert!(
        !out.lines().any(|l| l.starts_with('✗') && l.contains("tick step")),
        "{out}"
    );
    std::fs::write(&mark, step(NOW - 3 * 300 - 60)).unwrap();
    let (_, out) = run(&r.c, &Fake::default(), &["doctor"]);
    let row = out.lines().find(|l| l.contains("tick step")).unwrap_or_default();
    assert!(row.starts_with('✗') && row.contains("check"), "{out}");
    let (_, out) = run(&r.c, &host(&r), &["drive", "--fast"]);
    assert!(out.contains("tick stuck in check"), "{out}");
}
