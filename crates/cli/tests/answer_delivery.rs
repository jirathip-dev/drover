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
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
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
    let (code, out) = run(&c, &r, &["answer", "sendmeter", "q1", "2", "--by", "orchestrator"]);
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
        let page =
            String::from_utf8_lossy(&s.respond("GET / HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n\r\n", NOW)).into_owned();
        with_queue(&page, &s, "127.0.0.1:7777", NOW) // #268: the Queue tab as the board draws it
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

/// #424: a supersede logs the old lane's close before the new spawn, naming its pane; the new lane's ask is answered.
#[test]
fn a_superseded_lanes_successor_gets_its_answer_typed_into_its_pane() {
    let c = ctx(&tmpdir("deliver-supersede"), CONFIG);
    let wt = c.worktrees.join("sendmeter/issue-36");
    std::fs::create_dir_all(&wt).unwrap();
    spawn(&c, json!({"worktree": wt, "branch": "issue-36"}));
    let r = Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply(&format!("git -C {} rev-list", wt.display()), "0\n")
        .reply("git -C", "")
        .reply("gh issue view 36", r#"{"title":"t","body":"b"}"#)
        .reply("herdr worktree", r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#)
        .reply(
            "herdr pane list",
            r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#,
        )
        .reply("herdr pane run", "{}")
        .reply("herdr agent start", "{}")
        .reply("herdr pane", "{}")
        .reply("herdr agent prompt", "{}")
        .reply("herdr agent wait", "{}");
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "36", "impl", "--supersede"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    let ev = read(&c.state).unwrap().to_vec();
    let closed = ev
        .iter()
        .position(|e| e.kind == "lane-closed")
        .expect("old lane closed");
    let spawned = ev.iter().rposition(|e| e.kind == "spawn").unwrap();
    assert!(closed < spawned, "{ev:?}");
    assert_eq!(
        (ev[closed].str("pane"), ev[spawned].str("pane")),
        (Some("w1:p1"), Some("w9:p1"))
    );
    lane_ask(&c);
    let r = ok_herdr("");
    let (code, out) = run(&c, &r, &["answer", "sendmeter", "q1", "1"]);
    assert_eq!((code, out.as_str()), (0, "sendmeter q1: yes -> sendmeter-impl-36\n"));
    assert!(r.called("herdr pane send-keys w9:p1 Enter"), "{:?}", r.calls.borrow());
    assert_eq!(kinds(&c, "answer-delivered").len(), 1);
}

/// #424: the #410 log (new spawn, then the old lane's pane-less `superseded` close) leaves the new lane open.
#[test]
fn a_supersede_close_logged_after_the_new_spawn_does_not_close_it() {
    let c = ctx(&tmpdir("deliver-410"), CONFIG);
    spawn(&c, json!({}));
    spawn(&c, json!({"pane": "w2:p1"}));
    let d = json!({"agent": LANE, "reason": "superseded"});
    append(&c.state, &Event::new(NOW - 50, "sendmeter", Some(36), "lane-closed", d)).unwrap();
    lane_ask(&c);
    let r = ok_herdr("");
    let (code, out) = run(&c, &r, &["answer", "sendmeter", "q1", "1"]);
    assert_eq!((code, out.as_str()), (0, "sendmeter q1: yes -> sendmeter-impl-36\n"));
    assert!(r.called("herdr pane send-keys w2:p1 Enter"), "{:?}", r.calls.borrow());
}

/// #424: no open lane, but the asking agent is live in the issue's worktree: typed there, `answer-delivered-live`.
#[test]
fn a_closed_lane_whose_agent_still_runs_in_its_worktree_gets_the_answer() {
    let c = ctx(&tmpdir("deliver-live"), CONFIG);
    spawn(&c, json!({"worktree": "/wt/issue-36"}));
    lane_ask(&c);
    append(
        &c.state,
        &Event::new(NOW, "sendmeter", Some(36), "lane-closed", json!({"agent": LANE})),
    )
    .unwrap();
    let r = ok_herdr("").reply(
        "herdr agent list",
        &agents(&[(LANE, "idle", "/wt/issue-36/src", "w5:p2")]),
    );
    let (code, out) = run(&c, &r, &["answer", "sendmeter", "q1", "1"]);
    assert_eq!((code, out.as_str()), (0, "sendmeter q1: yes -> sendmeter-impl-36\n"));
    assert!(r.called("herdr pane send-keys w5:p2 Enter"), "{:?}", r.calls.borrow());
    let d = kinds(&c, "answer-delivered-live");
    assert_eq!((d.len(), d[0].str("agent")), (1, Some(LANE)));
    assert!(kinds(&c, "answer-undelivered").is_empty());
}

/// #424: an answer whose lane really is gone is logged `answer-undelivered`; `check` alerts it once, not again.
#[test]
fn an_answer_with_no_lane_alerts_once_from_check() {
    let c = ctx(&tmpdir("deliver-alert"), CONFIG);
    spawn(&c, json!({"worktree": "/wt/issue-36"}));
    lane_ask(&c);
    append(
        &c.state,
        &Event::new(NOW, "sendmeter", Some(36), "lane-closed", json!({"agent": LANE})),
    )
    .unwrap();
    let r = ok_herdr("").reply("herdr agent list", NO_AGENTS);
    assert_eq!(run(&c, &r, &["answer", "sendmeter", "q1", "1"]).0, 0);
    let u = kinds(&c, "answer-undelivered");
    assert_eq!((u.len(), u[0].str("agent")), (1, None));
    let (_, out) = run(&c, &r, &["check"]);
    assert!(out.contains("answer-undelivered"), "{out}");
    let (_, out) = run(&c, &r, &["check"]);
    assert!(!out.contains("answer-undelivered"), "{out}");
    let u = kinds(&c, "answer-undelivered");
    assert_eq!((u.len(), u[1].str("key")), (2, Some("q1")));
}
