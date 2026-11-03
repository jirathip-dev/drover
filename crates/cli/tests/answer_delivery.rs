//! #325: a lane's ask stores its agent; the answer is typed into that lane (else the issue's current one), logged
//! `answer-delivered` / `answer-undelivered`, and drive's fast step retries an undelivered one.
//! Every test here runs inside the same lane (`HERDSMAN_LANE` is process-wide: its own test binary).
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use herdsman::serve::Server;
use serde_json::{json, Value};

const NOW: u64 = 1_000_000;
const LANE: &str = "sendmeter-impl-36";

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

fn spawn(c: &herdsman::Ctx, extra: Value) {
    let mut d = json!({"role": "impl", "agent": LANE, "pane": "w1:p1", "host": "mini"});
    d.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    append(&c.state, &Event::new(NOW - 60, "sendmeter", Some(36), "spawn", d)).unwrap();
}

/// `herdsman ask` run inside the lane `sendmeter:36:impl`.
fn lane_ask(c: &herdsman::Ctx) {
    std::env::set_var("HERDSMAN_LANE", "sendmeter:36:impl");
    let args = [
        "ask",
        "sendmeter",
        "36",
        "Split it?",
        "--option",
        "yes",
        "--option",
        "no",
    ];
    let (code, out) = run(c, &Fake::default(), &args);
    assert_eq!((code, out.as_str()), (0, "q1\n"));
}

fn ok_herdr(machine: &str) -> Fake {
    let m = if machine.is_empty() {
        String::new()
    } else {
        format!("--machine {machine} ")
    };
    Fake::default()
        .reply(&format!("herdr {m}agent prompt"), "")
        .reply(&format!("herdr {m}pane send-keys"), "")
        .reply(&format!("herdr {m}agent wait"), "")
}

#[test]
fn a_lane_ask_stores_its_agent_and_the_answer_is_typed_into_its_pane() {
    let c = ctx(&tmpdir("deliver-lane"), CONFIG);
    spawn(&c, json!({}));
    lane_ask(&c);
    let a = &kinds(&c, "needs-decision")[0];
    assert_eq!((a.str("agent"), a.str("host")), (Some(LANE), Some("mini")));
    let r = ok_herdr("");
    let (code, out) = run(&c, &r, &["answer", "sendmeter", "q1", "2", "--by", "conductor"]);
    assert_eq!((code, out.as_str()), (0, "sendmeter q1: no -> sendmeter-impl-36\n"));
    assert!(
        r.called("herdr agent prompt sendmeter-impl-36 DECISION q1: no"),
        "{:?}",
        r.calls.borrow()
    );
    assert!(r.index("herdr pane send-keys w1:p1 Enter") > r.index("herdr agent prompt"));
    let d = kinds(&c, "answer-delivered");
    assert_eq!(
        (d.len(), d[0].str("id"), d[0].str("agent")),
        (1, Some("q1"), Some(LANE))
    );
}

/// q159: the ask logged before its lane's `spawn` (no agent): the answer goes to the issue's current lane.
#[test]
fn an_ask_without_an_agent_is_answered_into_the_issues_current_lane() {
    let c = ctx(&tmpdir("deliver-null"), CONFIG);
    lane_ask(&c);
    assert_eq!(kinds(&c, "needs-decision")[0].str("agent"), None);
    spawn(&c, json!({}));
    let r = ok_herdr("");
    assert_eq!(run(&c, &r, &["answer", "sendmeter", "q1", "1"]).0, 0);
    assert!(
        r.called("herdr agent prompt sendmeter-impl-36 DECISION q1: yes"),
        "{:?}",
        r.calls.borrow()
    );
    assert_eq!(kinds(&c, "answer-delivered")[0].str("agent"), Some(LANE));
}

#[test]
fn a_remote_lane_is_answered_with_machine() {
    let c = ctx(&tmpdir("deliver-remote"), CONFIG);
    spawn(&c, json!({"host": "air", "machine": "air"}));
    lane_ask(&c);
    let r = ok_herdr("air");
    assert_eq!(run(&c, &r, &["answer", "sendmeter", "q1", "1"]).0, 0);
    assert!(
        r.called("herdr --machine air agent prompt sendmeter-impl-36 DECISION q1: yes"),
        "{:?}",
        r.calls.borrow()
    );
    assert!(r.called("herdr --machine air pane send-keys w1:p1 Enter"));
    assert_eq!(kinds(&c, "answer-delivered").len(), 1);
}

#[test]
fn drive_retries_an_undelivered_answer_until_it_lands_or_the_lane_is_gone() {
    let c = ctx(&tmpdir("deliver-retry"), CONFIG);
    spawn(&c, json!({}));
    lane_ask(&c);
    // herdr can't reach the agent: recorded, logged undelivered, red on the issue's card.
    assert_eq!(run(&c, &Fake::default(), &["answer", "sendmeter", "q1", "1"]).0, 0);
    let u = kinds(&c, "answer-undelivered");
    assert_eq!(
        (u.len(), u[0].str("agent"), u[0].str("text")),
        (1, Some(LANE), Some("DECISION q1: yes"))
    );
    let page = |r: &Fake| {
        let s = Server::new(&c, r, Some("127.0.0.1:7777".into()));
        String::from_utf8_lossy(&s.respond("GET / HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n\r\n", NOW)).into_owned()
    };
    assert!(page(&Fake::default()).contains("<span class=r>answer q1 not delivered: "));

    // Still unreachable: the fast step tries again, logging nothing new.
    let down = Fake::default();
    run(&c, &down, &["drive", "--fast"]); // (the pass itself errs on the unscripted herdr; the retry ran first)
    assert!(
        down.called("herdr agent prompt sendmeter-impl-36 DECISION q1: yes"),
        "{:?}",
        down.calls.borrow()
    );
    assert_eq!(kinds(&c, "answer-undelivered").len(), 1);

    // Reachable: typed with Enter, delivered, the red line gone, and no more tries.
    let up = ok_herdr("");
    run(&c, &up, &["drive", "--fast"]);
    assert!(up.called("herdr pane send-keys w1:p1 Enter"), "{:?}", up.calls.borrow());
    assert_eq!(kinds(&c, "answer-delivered")[0].str("agent"), Some(LANE));
    assert!(!page(&up).contains("not delivered"));
    let again = ok_herdr("");
    run(&c, &again, &["drive", "--fast"]);
    assert_eq!(again.count("herdr agent prompt"), 0);

    // A lane gone (closed) is not retried.
    let c = ctx(&tmpdir("deliver-gone"), CONFIG);
    spawn(&c, json!({}));
    lane_ask(&c);
    assert_eq!(run(&c, &Fake::default(), &["answer", "sendmeter", "q1", "1"]).0, 0);
    let closed = Event::new(NOW, "sendmeter", Some(36), "lane-closed", json!({"agent": LANE}));
    append(&c.state, &closed).unwrap();
    let r = ok_herdr("");
    run(&c, &r, &["drive", "--fast"]);
    assert_eq!(r.count("herdr agent prompt"), 0, "{:?}", r.calls.borrow());
}
