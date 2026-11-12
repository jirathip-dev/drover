//! #906: ask ids are allocated with the insert, so two writers racing on one state dir never share one; a pair of asks
//! that already share an id is resolved by the caller's thread (withdraw, answer), else refused `AMBIGUOUS_ASK`.
mod common;
use common::*;
use herdsman::events::{append, closed, read, Event};
use herdsman::sim::{Hooks, Point};
use serde_json::{json, Value};

const NOW: u64 = 1_000_000;

fn log(c: &herdsman::Ctx, issue: Option<u64>, kind: &str, d: Value) {
    append(&c.state, &Event::new(NOW - 100, "sendmeter", issue, kind, d)).unwrap();
}

fn asks(c: &herdsman::Ctx) -> Vec<Event> {
    let events = read(&c.state).unwrap();
    events.into_iter().filter(|e| e.kind == "needs-decision").collect()
}

/// Each ask's thread and how it closed, in log order.
fn endings(c: &herdsman::Ctx) -> Vec<(String, Option<&'static str>)> {
    let events = read(&c.state).unwrap();
    let asks = events.iter().filter(|e| e.kind == "needs-decision");
    asks.map(|a| (a.str("thread").unwrap_or("-").to_string(), closed(&events, a)))
        .collect()
}

fn thread_ask(c: &herdsman::Ctx, issue: Option<u64>, id: &str, thread: &str) {
    let d = json!({"id": id, "question": "Ship it?", "options": ["yes", "no"], "recommend": 1, "kind": "owner",
        "thread": thread, "agent": null});
    log(c, issue, "needs-decision", d);
}

#[test]
fn two_asks_racing_on_one_state_dir_get_distinct_ids() {
    let c = ctx(&tmpdir("ask-race"), CONFIG);
    let other = c.clone();
    let fired = std::cell::Cell::new(false);
    let point = move |p: &Point| {
        let Point::Append(e) = p else { return };
        if e.kind != "needs-decision" || fired.replace(true) {
            return;
        }
        let o = other.clone();
        let b = std::thread::spawn(move || run(&o, &Fake::default(), &["ask", "sendmeter", "12", "Second?"]));
        assert_eq!(b.join().unwrap().0, 0);
    };
    herdsman::sim::set(Some(Hooks {
        now: Box::new(|| NOW),
        pid: std::process::id(),
        point: Box::new(point),
    }));
    let (code, out) = run(&c, &Fake::default(), &["ask", "sendmeter", "12", "First?"]);
    herdsman::sim::set(None);
    assert_eq!(code, 0, "{out}");
    let ids: Vec<String> = asks(&c).iter().map(|a| a.str("id").unwrap().into()).collect();
    assert_eq!(ids.len(), 2, "{ids:?}");
    assert_ne!(ids[0], ids[1], "two racing asks share an id");
    assert_eq!(
        out.trim(),
        asks(&c)[1].str("id").unwrap(),
        "ask prints the id it logged"
    );
}

#[test]
fn a_new_ask_takes_the_highest_id_plus_one_even_when_the_count_lags() {
    let c = ctx(&tmpdir("ask-max"), CONFIG);
    thread_ask(&c, None, "q7", "t1");
    let (code, out) = run(&c, &Fake::default(), &["ask", "sendmeter", "12", "Next?"]);
    assert_eq!((code, out.trim()), (0, "q8"), "{out}");
}

#[test]
fn withdraw_on_a_shared_id_closes_the_callers_threads_ask_and_leaves_the_other_alone() {
    let c = ctx(&tmpdir("ask-dup"), CONFIG);
    for t in ["t1", "t2", "t3"] {
        log(&c, None, "thread-new", json!({"thread": t, "title": t}));
    }
    thread_ask(&c, Some(12), "q1", "t1");
    log(
        &c,
        Some(12),
        "decision",
        json!({"id": "q1", "choice": "yes", "by": "owner"}),
    );
    thread_ask(&c, None, "q1", "t2");
    thread_ask(&c, None, "q2", "t1");
    thread_ask(&c, None, "q2", "t2");
    assert_eq!(
        endings(&c),
        [
            ("t1".into(), Some("answered")),
            ("t2".into(), None),
            ("t1".into(), None),
            ("t2".into(), None)
        ]
    );

    let (code, out) = run(&c, &Fake::default(), &["answer", "sendmeter", "q2", "1"]);
    assert_eq!(code, 3, "{out}");
    let seqs: Vec<u64> = asks(&c).iter().map(|a| a.seq).collect();
    assert!(out.contains("REFUSED AMBIGUOUS_ASK"), "{out}");
    assert!(
        out.contains(&seqs[2].to_string()) && out.contains(&seqs[3].to_string()),
        "{out}"
    );
    let w = ["ask", "withdraw", "sendmeter", "q2", "--by", "orchestrator-t3"];
    let (code, out) = run(&c, &Fake::default(), &w);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED AMBIGUOUS_ASK"), "{out}");

    for (id, t) in [("q1", "orchestrator-t2"), ("q2", "orchestrator-t2")] {
        let (code, out) = run(&c, &Fake::default(), &["ask", "withdraw", "sendmeter", id, "--by", t]);
        assert_eq!(code, 0, "{id}: {out}");
    }
    assert_eq!(
        endings(&c),
        [
            ("t1".into(), Some("answered")),
            ("t2".into(), Some("withdrawn")),
            ("t1".into(), None),
            ("t2".into(), Some("withdrawn"))
        ]
    );
}

#[test]
fn no_ask_id_is_counted_from_the_log_in_core() {
    let mut dirs = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/src")];
    let mut bad = vec![];
    while let Some(d) = dirs.pop() {
        for p in std::fs::read_dir(&d).unwrap().map(|f| f.unwrap().path()) {
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            let flat: String = text.split_whitespace().collect();
            if p.is_dir() {
                dirs.push(p);
            } else if flat.contains("needs-decision\").count()") {
                bad.push(p.display().to_string());
            }
        }
    }
    assert!(bad.is_empty(), "ask ids counted from the log: {bad:?}");
}
