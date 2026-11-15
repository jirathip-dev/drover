mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::json;

fn q(c: &drover::Ctx, args: &[&str]) -> (i32, String) {
    let mut a = vec!["queue", "sendmeter"];
    a.extend(args);
    run(c, &Fake::default(), &a)
}

fn merged(c: &drover::Ctx, ts: u64, n: u64) {
    let d = json!({"pr": "40", "merge_sha": "abc", "into": "staging"});
    append(&c.state, &Event::new(ts, "sendmeter", Some(n), "merged", d)).unwrap();
}

fn blockers(c: &drover::Ctx, n: u64) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_str(&q(c, &["ls", "--json"]).1).unwrap();
    let row = v.as_array().unwrap().iter().find(|r| r["issue"] == n).cloned();
    row.map_or(json!(null), |r| r["blocked_by"].clone())
}

#[test]
fn several_blockers_add_up_and_the_issue_starts_once_all_merged() {
    let c = ctx(&tmpdir("blockers"), CONFIG);
    let next = || run(&c, &Fake::default(), &["next", "sendmeter"]);
    q(&c, &["add", "40", "11", "12", "13"]);
    assert_eq!(q(&c, &["block", "40", "--on", "11", "--on", "12"]).0, 0);
    assert_eq!(blockers(&c, 40), json!([11, 12]));
    assert_eq!(q(&c, &["block", "40", "--on", "13", "--on", "11"]).0, 0);
    assert_eq!(blockers(&c, 40), json!([11, 12, 13]));
    let (_, ls) = q(&c, &["ls"]);
    assert!(ls.starts_with(" 1 #40    queued  blocked by #11, #12, #13\n"), "{ls}");
    let board = run(&c, &Fake::default(), &["board", "sendmeter"]).1;
    assert!(board.contains("on…\n         blocked by #11, #12, #13\n"), "{board}");
    q(&c, &["rm", "11"]);
    q(&c, &["rm", "12"]);
    q(&c, &["rm", "13"]);
    let start = || drover::queue::startable(&read(&c.state).unwrap(), "sendmeter");
    assert_eq!(next(), (2, "#40 is blocked by #11, #12, #13\n".into()));
    merged(&c, 100, 11);
    merged(&c, 110, 13);
    assert_eq!(blockers(&c, 40), json!([12]), "merged blockers drop out");
    assert_eq!(next(), (2, "#40 is blocked by #12\n".into()));
    assert_eq!(start(), [] as [std::string::String; 0]);
    merged(&c, 120, 12);
    assert_eq!(start(), ["40"]);
    assert_eq!(next(), (0, "40\n".into()));
}

#[test]
fn unblock_drops_one_or_all() {
    let c = ctx(&tmpdir("unblock"), CONFIG);
    q(&c, &["add", "40", "11", "12", "13"]);
    q(&c, &["block", "40", "--on", "11", "--on", "12", "--on", "13"]);
    assert_eq!(q(&c, &["unblock", "40", "--on", "12"]).0, 0);
    assert_eq!(blockers(&c, 40), json!([11, 13]));
    assert_eq!(q(&c, &["unblock", "40"]).0, 0);
    assert_eq!(blockers(&c, 40), json!([]));
    assert_eq!(q(&c, &["ls"]).1.lines().next(), Some(" 1 #40    queued"));
}

#[test]
fn a_self_block_or_a_cycle_is_refused_and_logs_nothing() {
    let c = ctx(&tmpdir("cycle"), CONFIG);
    q(&c, &["add", "11", "12", "13"]);
    q(&c, &["block", "12", "--on", "11"]);
    q(&c, &["block", "13", "--on", "12"]);
    let n = read(&c.state).unwrap().len();
    assert_eq!(q(&c, &["block", "11", "--on", "11"]).0, 1);
    assert_eq!(q(&c, &["block", "11", "--on", "13"]).0, 1);
    assert_eq!(read(&c.state).unwrap().len(), n);
    let mut queue = drover::queue::replay(&read(&c.state).unwrap(), "sendmeter");
    let err = drover::queue::apply(&mut queue, &json!({"op": "block", "issues": [11], "on": [13]}));
    assert_eq!(
        err.unwrap_err().to_string(),
        "#13 cannot block #11: #13 already waits on #11"
    );
    assert_eq!(blockers(&c, 11), json!([]));
}

#[test]
fn an_old_single_blocker_log_replays_as_a_set_of_one() {
    let c = ctx(&tmpdir("old-block"), CONFIG);
    for d in [
        json!({"op": "add", "issues": [11, 12]}),
        json!({"op": "block", "issues": [12], "on": 11}),
    ] {
        append(&c.state, &Event::new(10, "sendmeter", None, "queue", d)).unwrap();
    }
    assert_eq!(blockers(&c, 12), json!([11]));
    q(&c, &["block", "12", "--on", "13"]);
    assert_eq!(blockers(&c, 12), json!([11, 13]));
}
