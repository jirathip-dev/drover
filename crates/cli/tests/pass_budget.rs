mod common;
use common::*;
use herdsman::events::{append, Event};
use serde_json::json;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Instant;

const EVENTS: u64 = 100_000;
const ISSUES: u64 = 40;

fn fixture() -> herdsman::Ctx {
    let mut c = ctx(&tmpdir("pass-budget"), CONFIG);
    append(&c.state, &Event::new(1, "sendmeter", None, "drive-pass", json!({}))).unwrap();
    let mut db = rusqlite::Connection::open(c.state.with_file_name("state.db")).unwrap();
    let tx = db.transaction().unwrap();
    let add = |e: Event| {
        let q = "INSERT INTO events (ts, fleet, issue, type, json) VALUES (?1, ?2, ?3, ?4, ?5)";
        let j = serde_json::to_string(&e).unwrap();
        tx.execute(q, rusqlite::params![e.ts, e.fleet, e.issue, e.kind, j])
            .unwrap();
    };
    for n in 1..=ISSUES {
        let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
        let d = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "pane": format!("sendmeter-impl-{n}"),
            "workspace": "", "worktree": wt, "branch": format!("issue-{n}")});
        add(Event::new(10 + n, "sendmeter", Some(n), "spawn", d));
    }
    for i in 0..EVENTS {
        let (ts, n) = (100 + i, i % ISSUES + 1);
        add(match i % 4 {
            0 => Event::new(
                ts,
                "sendmeter",
                None,
                "drive-pass",
                json!({"by": "fast", "actions": []}),
            ),
            1 => Event::new(
                ts,
                "sendmeter",
                Some(n),
                "lane-state",
                json!({"agent": format!("sendmeter-impl-{n}")}),
            ),
            2 => Event::new(ts, "sendmeter", None, "kick", json!({"n": i})),
            _ => Event::new(ts, "sendmeter", Some(n), "driven", json!({"step": "review"})),
        });
    }
    tx.commit().unwrap();
    c.now = 100 + EVENTS + 60;
    c
}

fn lanes() -> OnShed {
    let r = OnShed::new(
        Fake::default()
            .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
            .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
            .reply("sysctl -n hw.ncpu", "10\n")
            .reply("vm_stat", "Pages free: 100000.\nSwapouts: 100.\n"),
    );
    for n in 1..=ISSUES {
        r.1.lane(&format!("sendmeter-impl-{n}"), "idle", "/elsewhere");
    }
    r
}

#[test]
fn a_fast_pass_over_a_100k_event_log_reads_it_once_and_stays_under_its_bound() {
    let c = fixture();
    let f = lanes();
    let (r0, t0) = (herdsman::db::ROWS.load(Relaxed), Instant::now());
    let (code, out) = run(&c, &f, &["drive", "--fast"]);
    let (secs, rows) = (t0.elapsed().as_secs_f64(), herdsman::db::ROWS.load(Relaxed) - r0);
    assert_eq!(code, 0, "{out}");
    let len = EVENTS + ISSUES + 1;
    assert!(rows >= len as usize, "the pass read the log: {rows}");
    assert!(
        rows < 2 * len as usize,
        "one full-log read, then only new rows: {rows} rows of a {len}-event log"
    );
    assert!(secs < 10.0, "a fast pass took {secs:.1}s");
}

#[test]
#[ignore]
fn pass_profile_on_the_fixture_log() {
    let runs: usize = std::env::var("HERDSMAN_PROFILE_RUNS").map_or(3, |n| n.parse().unwrap());
    for i in 0..runs {
        let c = fixture();
        let f = lanes();
        let cpu = || {
            let o = std::process::Command::new("ps")
                .args(["-o", "cputime=", "-p", &std::process::id().to_string()])
                .output()
                .unwrap();
            let t = String::from_utf8_lossy(&o.stdout).trim().to_string();
            t.split(':').fold(0.0, |a, p| a * 60.0 + p.parse::<f64>().unwrap())
        };
        let (r0, c0, t0) = (herdsman::db::ROWS.load(Relaxed), cpu(), Instant::now());
        let (code, _) = run(&c, &f, &["drive", "--fast"]);
        let (wall, cpu, rows) = (
            t0.elapsed().as_secs_f64(),
            cpu() - c0,
            herdsman::db::ROWS.load(Relaxed) - r0,
        );
        let len = EVENTS + ISSUES + 1;
        println!(
            "run {i}: exit {code} wall {wall:.2}s cpu {cpu:.2}s rows {rows} ({:.1} full-log reads)",
            rows as f64 / len as f64
        );
    }
}
