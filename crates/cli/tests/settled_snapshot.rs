mod common;
use common::*;
use drover::db::ROWS;
use drover::events::{fold, lanes, read, working, Event};
use serde_json::json;
use std::sync::atomic::Ordering::Relaxed;

const NOW: u64 = 1_000_000;
const EVENTS: usize = 30_000;
const LANES: u64 = 2_000;
const PER_LANE: usize = 8;
const THREADS: u64 = 190;
const OPEN_THREADS: u64 = 10;
const MSGS: usize = 10;
const OPEN: u64 = LANES + 1;
const KEPT: u64 = LANES + 2;

fn fixture(name: &str) -> drover::Ctx {
    let c = ctx(&tmpdir(name), CONFIG);
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let mut log = vec![];
    let ev = |n: u64, kind: &str, d: serde_json::Value| {
        let mut e = json!({"ts": 1000 + n, "fleet": "sendmeter", "issue": n, "type": kind});
        e.as_object_mut().unwrap().extend(d.as_object().unwrap().clone());
        e
    };
    let spawn = |n: u64, wt: &str| {
        let agent = format!("sendmeter-impl-{n}");
        ev(
            n,
            "spawn",
            json!({"role": "impl", "agent": agent, "worktree": wt, "pane": agent}),
        )
    };
    for n in 1..=LANES {
        let wt = format!("/gone/sendmeter/issue-{n}");
        log.push(spawn(n, &wt));
        for k in 0..5 {
            log.push(ev(n, "verdict", json!({"verdict": "PASS", "sha": format!("s{k}")})));
        }
        log.push(ev(n, "merged", json!({"pr": "1", "merge_sha": "m", "into": "staging"})));
        log.push(ev(n, "clean", json!({"worktree": wt, "pane": "p", "why": "merged"})));
    }
    for t in 1..=THREADS {
        let id = format!("t{t}");
        log.push(json!({"ts": 2000, "fleet": "sendmeter", "type": "thread-new", "thread": id, "title": format!("thread {t}")}));
        for m in 0..MSGS {
            log.push(json!({"ts": 2000 + m, "fleet": "sendmeter", "type": "thread-msg", "thread": id, "from": "owner", "text": format!("message {m}")}));
        }
        if t > OPEN_THREADS {
            log.push(json!({"ts": 3000, "fleet": "sendmeter", "type": "thread-archive", "thread": id}));
        }
    }
    let kept = c.worktrees.join("sendmeter/issue-kept");
    std::fs::create_dir_all(&kept).unwrap();
    log.push(spawn(KEPT, &kept.to_string_lossy()));
    log.push(ev(
        KEPT,
        "merged",
        json!({"pr": "2", "merge_sha": "m", "into": "staging"}),
    ));
    while log.len() < EVENTS - 2 {
        log.push(json!({"ts": 5000, "fleet": "sendmeter", "type": "note", "text": "later"}));
    }
    log.push(json!({"ts": NOW - 60, "fleet": "sendmeter", "issue": OPEN, "type": "queue", "action": "add"}));
    log.push(spawn(OPEN, "/gone/open"));
    let jsonl: String = log.iter().map(|e| format!("{e}\n")).collect();
    std::fs::write(&c.state, jsonl).unwrap();
    drover::events::append(&c.state, &Event::new(NOW, "sendmeter", None, "note", json!({}))).unwrap();
    c
}

fn reads(c: &drover::Ctx, args: &[&str]) -> (usize, String) {
    let sh = FakeShed::new();
    sh.attach(c);
    let (r0, t0) = (ROWS.load(Relaxed), std::time::Instant::now());
    let (code, out) = run(c, &Fake::default().reply("git", ""), args);
    let (rows, ms) = (ROWS.load(Relaxed) - r0, t0.elapsed().as_millis());
    eprintln!(
        "{args:?}: exit {code}, {rows} rows, {ms} ms, {} lines out",
        out.lines().count()
    );
    assert_eq!(code, 0, "{out}");
    (rows, out)
}

static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());

const COMMANDS: [&[&str]; 4] = [&["status"], &["waiting"], &["clean"], &["thread", "ls"]];

#[test]
fn settled_reads_cost() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for args in COMMANDS {
        reads(&fixture(&format!("cost-{}", args[0])), args);
    }
}

#[test]
fn settled_issues_and_archived_threads_are_read_from_the_snapshot_only() {
    let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let settled = usize::try_from(LANES).unwrap() * PER_LANE;
    let archived = usize::try_from(THREADS - OPEN_THREADS).unwrap() * (MSGS - 1);
    let later = EVENTS + 1 - settled - archived;
    for args in COMMANDS {
        let c = fixture(&format!("settled-{}", args[0]));
        let (rows, out) = reads(&c, args);
        assert!(
            rows <= later,
            "{args:?} reads {rows} rows: no settled issue's event and no archived thread's message"
        );
        match args[0] {
            "status" => {
                let clean_me: Vec<_> = out.lines().filter(|l| l.contains("clean me")).collect();
                assert_eq!(clean_me.len(), 1, "{out}");
                assert!(clean_me[0].contains("#2002"), "{out}");
            }
            "clean" => assert!(out.contains("issue-kept") && out.lines().count() == 2, "{out}"),
            "thread" => assert_eq!(out.lines().count(), usize::try_from(OPEN_THREADS).unwrap() + 1, "{out}"),
            _ => {}
        }
    }

    let c = fixture("settled-fold");
    let w = working(&c.state).unwrap();
    assert_eq!(w.len(), later, "the working set is the snapshot's later events");
    assert!(w.iter().all(|e| e.issue.is_none_or(|n| n > LANES)));
    let all = read(&c.state).unwrap();
    let json = |s: drover::events::States| serde_json::to_string(&s.into_values().collect::<Vec<_>>()).unwrap();
    let full = json(fold(&all.to_vec()));
    assert_eq!(
        json(fold(&w)),
        full,
        "the snapshot plus later events folds as the whole log"
    );
    let st = fold(&w);
    assert!(st[&("sendmeter".into(), 1)].settled() && !st[&("sendmeter".into(), KEPT)].settled());
    let unsettled = |v: Vec<&Event>| {
        v.iter()
            .filter(|e| e.issue.is_none_or(|n| n > LANES))
            .map(|e| e.seq)
            .collect::<Vec<_>>()
    };
    assert_eq!(unsettled(lanes(&w)), unsettled(lanes(&all)));
    drover::db::rebuild(&c.state).unwrap();
    assert_eq!(
        json(drover::db::issues(&c.state).unwrap()),
        full,
        "the snapshot rebuilt from the log"
    );
    let (_, show) = run(&c, &Fake::default(), &["thread", "show", "t190"]);
    assert!(
        show.contains("message 9"),
        "an archived thread still shows in full: {show}"
    );
}
