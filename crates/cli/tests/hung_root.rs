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
    let dead = r.dead.to_string_lossy();
    let calls = f.calls.borrow().clone();
    assert!(
        !calls.iter().any(|c| c.starts_with("git") && c.contains(&*dead)),
        "{calls:#?}"
    );
    let (_, w) = run(&r.c, &f, &["waiting", "sendmeter"]);
    let line = w.lines().find(|l| l.contains("#11")).unwrap_or_default();
    assert!(line.contains("root hung"), "{w}");
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
    assert!(!w.contains("root hung"), "{w}");
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

fn lane(c: &drover::Ctx, n: u64, root: &Path) -> PathBuf {
    let wt = root.join(format!("sendmeter/issue-{n}"));
    std::fs::create_dir_all(&wt).unwrap();
    let fifo = std::process::Command::new("mkfifo").arg(wt.join(".git")).status();
    assert!(fifo.unwrap().success(), "a .git whose open never returns");
    let d = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "harness": "claude", "pane": "w9:p1",
                   "workspace": "w9", "worktree": wt, "root": root, "branch": format!("issue-{n}"), "host": "mini"});
    append(&c.state, &Event::new(NOW - 500, "sendmeter", Some(n), "spawn", d)).unwrap();
    append(
        &c.state,
        &Event::new(NOW - 400, "sendmeter", Some(n), "merged", json!({"pr": n})),
    )
    .unwrap();
    wt
}

fn within(c: &drover::Ctx, f: Fake, args: &'static [&'static str]) -> (i32, String) {
    let (tx, rx) = std::sync::mpsc::channel();
    let c = c.clone();
    std::thread::spawn(move || drop(tx.send(run(&c, &f, args))));
    rx.recv_timeout(Duration::from_secs(10))
        .unwrap_or_else(|_| panic!("drover {args:?} still running after 10 s"))
}

#[test]
fn a_lane_whose_reads_block_or_whose_root_was_removed_never_holds_clean_or_the_tick() {
    let r = setup("lane-reads-block");
    let cfg = std::fs::read_to_string(&r.c.config).unwrap();
    std::fs::write(&r.c.config, cfg.replace("[limits]\n", "[limits]\nlane_read_secs = 1\n")).unwrap();
    let hung = lane(&r.c, 13, &r.good);
    let old = r.dead.with_file_name("removed");
    let off = lane(&r.c, 14, &old);
    let (code, out) = within(&r.c, host(&r), &["clean", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&format!("keep {}: off-root", off.display())), "{out}");
    assert!(out.contains(&format!("keep {}: root-hung", hung.display())), "{out}");
    let (code, out) = within(&r.c, host(&r), &["tick"]);
    assert_eq!(code, 0, "{out}");
    let events = read(&r.c.state).unwrap();
    let marks: Vec<&Event> = (events.iter())
        .filter(|e| e.kind == "root-hung" && e.str("worktree").is_some())
        .collect();
    assert_eq!(marks.len(), 1, "once for the episode: {marks:#?}");
    assert_eq!(marks[0].str("worktree"), Some(&*hung.to_string_lossy()));
    assert!(!events.iter().any(|e| e.kind == "clean"), "neither is removed");
}

#[test]
fn a_pass_stuck_past_three_pass_max_secs_is_logged_and_the_next_pass_takes_its_lock() {
    let r = setup("pass-stuck");
    let f = host(&r).reply("kill -0", "").reply("ps -o etime=", "01:00:00");
    assert_eq!(run(&r.c, &f, &["drive", "--fast"]).0, 0);
    let at = |ts: u64| drover::db::take(&r.c.state, "drive-fast.lock", ts, 360, "", &|_| false);
    let live = at(NOW - 300).expect("a pass holds the lock");
    let (_, out) = run(&r.c, &f, &["drive", "--fast"]);
    assert!(out.contains("another pass holds drive-fast.lock"), "{out}");
    drover::db::release(&r.c.state, "drive-fast.lock", Some(&live));
    at(NOW - 361).expect("a pass that hung 361 s ago");
    let (code, out) = run(&r.c, &f, &["drive", "--fast"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("taken over"), "{out}");
    let events = read(&r.c.state).unwrap();
    let stuck: Vec<&Event> = events.iter().filter(|e| e.kind == "pass-stuck").collect();
    assert_eq!(stuck.len(), 1, "{stuck:#?}");
    assert_eq!(stuck[0].str("step"), Some("after backlog, checkpoint"), "{stuck:#?}");
}

#[test]
fn a_probe_another_thread_started_is_waited_out_not_taken_for_a_hang() {
    let d = tmpdir("probe-in-flight");
    let dir = d.to_string_lossy().into_owned();
    let (began, rx) = std::sync::mpsc::channel();
    let first = {
        let dir = dir.clone();
        std::thread::spawn(move || {
            drover::runner::hung(&dir, Duration::from_secs(5), move |_| {
                began.send(()).unwrap();
                std::thread::sleep(Duration::from_millis(300));
                None
            })
        })
    };
    rx.recv().unwrap();
    assert!(
        !drover::runner::hung(&dir, Duration::from_secs(5), |_| None),
        "a probe still in its time answers"
    );
    assert!(!first.join().unwrap());
}
