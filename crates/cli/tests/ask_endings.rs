//! #584: an ask closes on every real ending: the owner's typed answer (`answer --typed`), a conductor's withdraw, a
//! handoff delivered or failed, 24 h expiry unless its lane is live (that one re-alerts once a day); a one-time sweep.
mod common;
use common::*;
use herdsman::events::{append, open_asks, read, Event};
use herdsman::serve::Server;
use serde_json::{json, Value};

const NOW: u64 = 1_000_000;
const H: u64 = 3600;
const LANE: &str = "sendmeter-impl-36";

fn log(c: &herdsman::Ctx, ts: u64, issue: Option<u64>, kind: &str, d: Value) {
    append(&c.state, &Event::new(ts, "sendmeter", issue, kind, d)).unwrap();
}

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

fn open(c: &herdsman::Ctx) -> Vec<String> {
    let events = read(&c.state).unwrap();
    open_asks(&events)
        .iter()
        .map(|a| a.str("id").unwrap().to_string())
        .collect()
}

/// Threads t1 and t2 (`ts` before everything else).
fn threads(c: &herdsman::Ctx, ts: u64) {
    for t in ["t1", "t2"] {
        log(
            c,
            ts,
            None,
            "thread-new",
            json!({"thread": t, "title": format!("thread {t}")}),
        );
    }
}

fn thread_ask(c: &herdsman::Ctx, ts: u64, id: &str, thread: &str) {
    let d = json!({"id": id, "question": "Ship it?", "options": ["yes", "no"], "recommend": 1, "kind": "owner",
        "thread": thread, "agent": null});
    log(c, ts, None, "needs-decision", d);
}

fn said(c: &herdsman::Ctx, ts: u64, thread: &str, from: &str, text: &str) {
    log(
        c,
        ts,
        None,
        "thread-msg",
        json!({"thread": thread, "from": from, "text": text}),
    );
}

/// The `herdsman asks --all` row of ask `id`.
fn row(c: &herdsman::Ctx, id: &str) -> String {
    let all = run(c, &Fake::default(), &["asks", "--all"]).1;
    let row = all.lines().find(|l| l.split(' ').nth(1) == Some(id));
    row.unwrap_or_else(|| panic!("no {id} in {all}")).to_string()
}

#[test]
fn a_typed_answer_closes_the_ask_only_with_the_owners_exact_line_after_it_in_its_thread() {
    let c = ctx(&tmpdir("typed"), CONFIG);
    threads(&c, NOW - 200);
    said(&c, NOW - 100, "t1", "owner", "before the ask");
    thread_ask(&c, NOW - 90, "q1", "t1");
    said(&c, NOW - 80, "t2", "owner", "in another thread");
    said(&c, NOW - 70, "t1", "orchestrator", "the orchestrator's line");
    said(&c, NOW - 60, "t1", "owner", "Ok as recommended then");
    for (line, why) in [
        ("before the ask", "before"),
        ("in another thread", "t2"),
        ("the orchestrator's line", "no owner message"),
        ("never said", "no owner message"),
    ] {
        let (code, out) = run(&c, &Fake::default(), &["answer", "sendmeter", "q1", "--typed", line]);
        assert_eq!(code, 3, "{line}: {out}");
        assert!(out.contains("REFUSED NOT_TYPED") && out.contains(why), "{line}: {out}");
    }
    assert!(kinds(&c, "decision").is_empty());
    assert_eq!(open(&c), ["q1"]);

    let (code, out) = run(
        &c,
        &Fake::default(),
        &["answer", "sendmeter", "q1", "--typed", "Ok as recommended then"],
    );
    assert_eq!(code, 0, "{out}");
    let d = &kinds(&c, "decision")[0];
    assert_eq!(
        (d.str("id"), d.str("by"), d.str("choice")),
        (Some("q1"), Some("owner (typed)"), Some("Ok as recommended then"))
    );
    assert!(open(&c).is_empty());
    assert!(row(&c, "q1").ends_with("[answered]"), "{}", row(&c, "q1"));
}

#[test]
fn a_withdrawn_ask_closes_and_names_the_ask_that_replaced_it() {
    let c = ctx(&tmpdir("withdraw"), CONFIG);
    threads(&c, NOW - 200);
    thread_ask(&c, NOW - 90, "q1", "t1");
    thread_ask(&c, NOW - 60, "q2", "t1");
    let (code, out) = run(
        &c,
        &Fake::default(),
        &["ask", "withdraw", "sendmeter", "q2", "--by", "orchestrator-t2"],
    );
    assert_eq!(code, 3, "only its own thread's orchestrator withdraws it: {out}");
    let (code, out) = run(
        &c,
        &Fake::default(),
        &[
            "ask",
            "withdraw",
            "sendmeter",
            "q1",
            "--by",
            "orchestrator-t1",
            "--for",
            "q2",
        ],
    );
    assert_eq!(code, 0, "{out}");
    assert_eq!(kinds(&c, "ask-withdrawn").len(), 1);
    assert_eq!(open(&c), ["q2"]);
    assert!(row(&c, "q1").ends_with("[withdrawn → q2]"), "{}", row(&c, "q1"));
}

/// A conductor role (signed in with account C) whose fleet runs on shed `sh` as ACP sessions, its profiles synced
/// (the real binary their hook), and a handoff card from t1 to the repo with t2 and a new thread as its rows.
fn handoff(name: &str, sh: &FakeShed) -> herdsman::Ctx {
    let cfg = format!(
        "{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\naccount = \"C\"\n\n\
         [fleet.sendmeter.runtime]\nkind = \"shed-acp\"\nsocket = \"{}\"\n\n[accounts]\nC = \"C\"\n",
        sh.sock.display()
    );
    let mut c = ctx(&tmpdir(name), &cfg);
    c.exe = env!("CARGO_BIN_EXE_herdsman").into();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    threads(&c, NOW - 200);
    let d = json!({"id": "q1", "question": "Hand off \"Grill\" to which sendmeter thread?",
        "options": ["thread t2 · t2", "new thread: Grill"], "recommend": null, "kind": "owner", "thread": "t1",
        "handoff": {"repo": "sendmeter", "title": "Grill", "text": "grill #12", "targets": ["t2", "new"]}});
    log(&c, NOW - 90, None, "needs-decision", d);
    c
}

#[test]
fn a_handoff_ask_closes_when_it_is_delivered_and_when_it_fails() {
    let sh = FakeShed::new();
    let c = handoff("handoff-ok", &sh);
    // shed's CLI starts t2's conductor as an ACP session (its installed adapter) and sends it the handoff.
    let agents = json!([{"id": "claude-acp", "version": "0.101.0", "distribution": "npx", "cmd": ["echo"]}]);
    let f = Fake::default()
        .reply("shed agents --json", &agents.to_string())
        .reply("shed run", "")
        .reply("shed send", "");
    let (code, out) = run(&c, &sh.cli(&f), &["answer", "sendmeter", "q1", "1"]);
    assert_eq!(code, 0, "{out} {:?}", f.calls.borrow());
    let sent = f
        .calls
        .borrow()
        .iter()
        .any(|l| l.starts_with("shed send orchestrator-t2 ") && l.contains("grill #12"));
    assert!(sent, "{:?}", f.calls.borrow());
    let herdr = |l: &&String| l.starts_with("herdr") && *l != "herdr agent list";
    assert!(
        !f.calls.borrow().iter().any(|l| herdr(&l)),
        "no herdr pane: {:?}",
        f.calls.borrow()
    );
    assert!(open(&c).is_empty());
    assert!(row(&c, "q1").ends_with("[delivered to t2]"), "{}", row(&c, "q1"));

    // shed is down: nothing reaches t2's conductor; the card closes as failed.
    let c = handoff("handoff-failed", &FakeShed::new());
    let (code, out) = run(&c, &Fake::default(), &["answer", "sendmeter", "q1", "1"]);
    assert_ne!(code, 0, "{out}");
    assert!(open(&c).is_empty());
    assert!(row(&c, "q1").ends_with("[failed]"), "{}", row(&c, "q1"));
}

fn check(c: &herdsman::Ctx, r: &Fake) -> String {
    let (code, out) = run(c, r, &["check"]);
    assert_eq!(code, 0, "{out}");
    out
}

/// `[chat·N]`: the threads showing `! choose`.
fn choosing(c: &herdsman::Ctx) -> u64 {
    let r = Fake::default();
    let s = Server::new(c, &r, Some("127.0.0.1:7777".into()));
    let needs: Value = serde_json::from_str(&api(&s, "127.0.0.1:7777", "", c.now, "needs")).unwrap();
    needs["choosing"].as_u64().unwrap()
}

#[test]
fn a_thread_ask_nobody_waits_on_expires_after_24_h_and_its_choose_goes() {
    let mut c = ctx(&tmpdir("expire-thread"), CONFIG);
    let _sh = FakeShed::attached(&c);
    threads(&c, NOW - 26 * H);
    thread_ask(&c, NOW - 25 * H, "q1", "t1");
    assert_eq!(choosing(&c), 1);
    let r = Fake::default();
    c.now = NOW;
    check(&c, &r);
    assert_eq!(kinds(&c, "ask-expired").len(), 1);
    assert!(open(&c).is_empty());
    assert_eq!(choosing(&c), 0);
    assert!(row(&c, "q1").ends_with("[expired]"), "{}", row(&c, "q1"));
}

/// The asking lane's spawn on #36, and its ask `id` at `ts`.
fn lane_ask(c: &herdsman::Ctx, ts: u64, id: &str) {
    log(
        c,
        ts - 60,
        Some(36),
        "spawn",
        json!({"role": "impl", "agent": LANE, "pane": LANE}),
    );
    let d = json!({"id": id, "question": "Split the PR?", "options": ["yes", "no"], "recommend": 1,
        "kind": "owner", "agent": LANE});
    log(c, ts, Some(36), "needs-decision", d);
}

#[test]
fn a_live_lanes_ask_never_expires_and_re_alerts_once_a_day() {
    let mut c = ctx(&tmpdir("expire-live"), CONFIG);
    let sh = FakeShed::attached(&c);
    sh.lane(LANE, "idle", "/x");
    let t0 = NOW - 30 * H;
    lane_ask(&c, t0, "q1");
    let r = Fake::default();
    let waiting = |c: &herdsman::Ctx| kinds(c, "ask-waiting").len();
    c.now = t0 + 40 * 60;
    check(&c, &r);
    assert_eq!(waiting(&c), 1);
    for h in [2, 12, 23] {
        c.now = t0 + h * H;
        check(&c, &r);
        assert_eq!(waiting(&c), 1, "none again before 24 h (at {h} h)");
    }
    c.now = t0 + 25 * H;
    check(&c, &r);
    assert_eq!(waiting(&c), 2, "once again at 24 h");
    c.now = t0 + 26 * H;
    check(&c, &r);
    assert_eq!(waiting(&c), 2);
    assert!(kinds(&c, "ask-expired").is_empty());
    assert_eq!(open(&c), ["q1"]);
}

#[test]
fn an_ask_whose_lane_was_killed_expires_after_24_h() {
    let mut c = ctx(&tmpdir("expire-killed"), CONFIG);
    let _sh = FakeShed::attached(&c);
    lane_ask(&c, NOW - 25 * H, "q1");
    c.now = NOW;
    check(&c, &Fake::default());
    assert_eq!(kinds(&c, "ask-expired").len(), 1);
    assert!(open(&c).is_empty());
}

#[test]
fn the_sweep_closes_a_logs_stale_asks_once() {
    let mut c = ctx(&tmpdir("sweep"), CONFIG);
    let sh = FakeShed::attached(&c);
    sh.lane(LANE, "idle", "/x");
    threads(&c, NOW - 40 * H);
    // A handoff card whose handoff went out (before #584 no decision was logged for it).
    let d = json!({"id": "q1", "question": "Hand off \"Grill\" to which sendmeter thread?",
        "options": ["thread t2 · t2"], "recommend": null, "kind": "owner", "thread": "t1",
        "handoff": {"repo": "sendmeter", "title": "Grill", "text": "grill #12", "targets": ["t2"]}});
    log(&c, NOW - 3 * H, None, "needs-decision", d);
    let d = json!({"thread": "t1", "from": "t1", "to": "t2", "title": "Grill", "text": "grill #12"});
    log(&c, NOW - 3 * H + 10, None, "thread-handoff", d);
    thread_ask(&c, NOW - 30 * H, "q2", "t1");
    lane_ask(&c, NOW - 2 * H, "q3");
    assert_eq!(open(&c), ["q1", "q2", "q3"]);
    let r = Fake::default();
    c.now = NOW;
    check(&c, &r);
    assert_eq!(open(&c), ["q3"], "the lane's ask still waits");
    let swept = kinds(&c, "asks-swept");
    assert_eq!(swept.len(), 1);
    assert_eq!(swept[0].data["count"], 2);
    assert!(row(&c, "q1").ends_with("[delivered to t2]"), "{}", row(&c, "q1"));
    assert!(row(&c, "q2").ends_with("[expired]"), "{}", row(&c, "q2"));
    let n = read(&c.state).unwrap().len();
    c.now = NOW + 60;
    check(&c, &r);
    assert_eq!(kinds(&c, "asks-swept").len(), 1, "the sweep runs once");
    let after = read(&c.state).unwrap();
    assert!(
        after[n..].iter().all(|e| !e.kind.starts_with("ask")),
        "{:?}",
        &after[n..]
    );
}
