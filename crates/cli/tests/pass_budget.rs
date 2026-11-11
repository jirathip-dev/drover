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
        let d = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "pane": format!("w{n}:p1"),
            "workspace": format!("w{n}"), "worktree": wt, "branch": format!("issue-{n}")});
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

#[test]
fn a_fast_pass_over_a_100k_event_log_reads_it_once_and_stays_under_its_bound() {
    let c = fixture();
    let rows: Vec<_> = (1..=ISSUES)
        .map(|n| (format!("sendmeter-impl-{n}"), format!("w{n}:p1")))
        .collect();
    let list: Vec<_> = rows
        .iter()
        .map(|(a, p)| (a.as_str(), "idle", "/elsewhere", p.as_str()))
        .collect();
    let f = Fake::default()
        .reply("herdr agent list", &agents(&list))
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 100000.\nSwapouts: 100.\n");
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
