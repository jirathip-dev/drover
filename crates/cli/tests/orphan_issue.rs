//! #493: `check` alerts `orphan-issue` once for an open issue nothing drives for 30 min; `waiting` says so.
mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::{json, Value};

const T0: u64 = 1_000_000;
const SHA: &str = "1111111111111111111111111111111111111111";

fn ev(c: &drover::Ctx, ts: u64, issue: Option<u64>, kind: &str, d: Value) {
    append(&c.state, &Event::new(ts, "sendmeter", issue, kind, d)).unwrap();
}

fn spawn(c: &drover::Ctx, ts: u64, n: u64, role: &str) -> String {
    let agent = format!("sendmeter-{role}-{n}");
    let d = json!({"role": role, "agent": agent, "branch": format!("issue-{n}"), "check": role == "rev"});
    ev(c, ts, Some(n), "spawn", d);
    agent
}

/// A drive pass at `ts` that acted on `acts` (issue, refusal code).
fn pass(c: &drover::Ctx, ts: u64, acts: &[(u64, Option<&str>)]) {
    let a: Vec<Value> = acts
        .iter()
        .map(|(n, code)| json!({"issue": n, "step": "x", "code": code}))
        .collect();
    ev(c, ts, None, "drive-pass", json!({"actions": a}));
}

/// `check` at `now`; the `orphan-issue` alerts logged so far.
fn check_at(c: &mut drover::Ctx, r: &Fake, now: u64) -> Vec<Event> {
    c.now = now;
    let (code, out) = run(c, r, &["check"]);
    assert_eq!(code, 0, "{out}");
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == "orphan-issue")
        .collect()
}

/// Each stall shape: silent at 29 min, one alert at 30 min, none more at 40 min and 2 h.
fn fires_once(c: &mut drover::Ctx, r: &Fake, quiet_since: u64, n: u64) -> Event {
    assert!(check_at(c, r, quiet_since + 1740).is_empty(), "not before 30 min");
    let got = check_at(c, r, quiet_since + 1800);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].issue, Some(n));
    assert_eq!(check_at(c, r, quiet_since + 2400).len(), 1, "once per episode");
    assert_eq!(check_at(c, r, quiet_since + 7200).len(), 1, "once per episode");
    got[0].clone()
}

#[test]
fn fix_lane_ended_without_pushing_alerts_once_at_30_min() {
    // #444's shape: a fix lane gone, no push, and drive no longer acting on it.
    let d = tmpdir("orphan-444");
    let mut c = ctx(&d, CONFIG);
    spawn(&c, T0, 7, "impl");
    ev(&c, T0 + 10, Some(7), "pushed", json!({"sha": SHA}));
    spawn(&c, T0 + 20, 7, "rev");
    ev(&c, T0 + 30, Some(7), "verdict", json!({"verdict": "FAIL", "sha": SHA}));
    let lane = spawn(&c, T0 + 40, 7, "impl");
    pass(&c, T0 + 40, &[(7, None)]);
    let r = Fake::default().reply("herdr agent list", NO_AGENTS);
    let e = fires_once(&mut c, &r, T0 + 40, 7);
    let why = e.str("reason").unwrap();
    assert!(why.contains("#7 fix(1): nothing drives it for 30m"), "{why}");
    assert!(
        why.contains(&format!("last event spawn 30m ago; last lane {lane} (gone)")),
        "{why}"
    );
}

#[test]
fn no_change_fix_round_with_merge_label_alerts_once() {
    // #445's shape: `done --no-change` under a merge label, drive passing but skipping the issue.
    let d = tmpdir("orphan-445");
    let cfg = CONFIG.replace(
        "integration = \"staging\"",
        "integration = \"staging\"\nmerge_label = \"full-ci\"",
    );
    let mut c = ctx(&d, &cfg);
    spawn(&c, T0, 8, "impl");
    ev(&c, T0 + 10, Some(8), "pushed", json!({"sha": SHA}));
    spawn(&c, T0 + 20, 8, "rev");
    ev(&c, T0 + 30, Some(8), "verdict", json!({"verdict": "FAIL", "sha": SHA}));
    let lane = spawn(&c, T0 + 40, 8, "impl");
    ev(&c, T0 + 50, Some(8), "fix-done", json!({"why": "CI-infra red"}));
    for t in (0..8).map(|k| T0 + 300 * k) {
        pass(&c, t, &[]); // drive runs, but nothing about #8
    }
    let r = Fake::default().reply("herdr agent list", &agents(&[(&lane, "idle", "/x", "w1:p1")]));
    let e = fires_once(&mut c, &r, T0 + 50, 8);
    assert!(e.str("reason").unwrap().contains(&format!("last lane {lane} (idle)")));
}

#[test]
fn undelivered_answer_alerts_once_after_the_ask_closes() {
    // #424's shape: the ask is a named wait; once answered, nothing delivers it and nothing drives the issue.
    let d = tmpdir("orphan-424");
    let mut c = ctx(&d, CONFIG);
    let lane = spawn(&c, T0, 9, "impl");
    pass(&c, T0, &[]); // drive runs, but nothing about #9
    let ask = json!({"id": "a1", "agent": lane, "question": "which?", "options": ["x", "y"]});
    ev(&c, T0 + 10, Some(9), "needs-decision", ask);
    let r = Fake::default().reply("herdr agent list", &agents(&[(&lane, "idle", "/x", "w1:p1")]));
    assert!(
        check_at(&mut c, &r, T0 + 7200).is_empty(),
        "an open ask is a named wait"
    );
    ev(&c, T0 + 7200, Some(9), "decision", json!({"id": "a1", "choice": "x"}));
    fires_once(&mut c, &r, T0 + 7200, 9);
}

#[test]
fn drive_going_silent_on_an_issue_alerts_30_min_after_its_last_action() {
    // #446's shape: drive acted on the review, then stopped seeing anything due.
    let d = tmpdir("orphan-446");
    let mut c = ctx(&d, CONFIG);
    spawn(&c, T0, 10, "impl");
    ev(&c, T0 + 10, Some(10), "pushed", json!({"sha": SHA}));
    let lane = spawn(&c, T0 + 20, 10, "rev");
    for t in (1..=6).map(|k| T0 + 300 * k) {
        pass(&c, t, &[(10, Some("EXIT"))]);
    }
    let r = Fake::default().reply("herdr agent list", &agents(&[(&lane, "done", "/x", "w1:p1")]));
    fires_once(&mut c, &r, T0 + 1800, 10);
}

#[test]
fn a_healthy_fleet_raises_none() {
    let d = tmpdir("orphan-healthy");
    let mut c = ctx(&d, CONFIG);
    // Working lane.
    let working = spawn(&c, T0, 1, "impl");
    // A review drive retries every pass (refused: a reviewer slot).
    spawn(&c, T0, 2, "impl");
    ev(&c, T0 + 10, Some(2), "pushed", json!({"sha": SHA}));
    // Ready, CI still running.
    spawn(&c, T0, 3, "impl");
    spawn(&c, T0 + 10, 3, "rev");
    ev(&c, T0 + 20, Some(3), "verdict", json!({"verdict": "PASS", "sha": SHA}));
    // Held until #1 merges.
    spawn(&c, T0, 4, "impl");
    ev(&c, T0 + 10, Some(4), "held", json!({"on": 1}));
    // An open owner ask.
    let asking = spawn(&c, T0, 5, "impl");
    ev(
        &c,
        T0 + 10,
        Some(5),
        "needs-decision",
        json!({"id": "a5", "agent": asking, "question": "?"}),
    );
    // The next queued issue, refused HOST_BUSY each pass; one behind it, blocked.
    ev(&c, T0, None, "queue", json!({"op": "add", "issues": [6, 11]}));
    ev(&c, T0, None, "queue", json!({"op": "block", "issues": [11], "on": 6}));
    for t in (0..=24).map(|k| T0 + 300 * k) {
        pass(&c, t, &[(2, Some("CAP")), (6, Some("HOST_BUSY"))]);
    }
    let r = Fake::default().reply("herdr agent list", &agents(&[(&working, "working", "/x", "w1:p1")]));
    assert!(check_at(&mut c, &r, T0 + 7200).is_empty());
    // `waiting` lists every guarded issue; none is orphaned.
    let (code, out) = run(&c, &r, &["waiting", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    for n in 1..=6 {
        assert!(out.contains(&format!("sendmeter #{n} ")), "#{n} missing: {out}");
    }
    assert!(
        out.contains(&format!("#1 none: waiting on {working} (working)")),
        "{out}"
    );
    assert!(!out.contains("nothing drives this"), "{out}");
}

#[test]
fn a_paused_fleet_raises_none() {
    let d = tmpdir("orphan-paused");
    let mut c = ctx(
        &d,
        &CONFIG.replace("integration = \"staging\"", "integration = \"staging\"\npaused = true"),
    );
    spawn(&c, T0, 7, "impl");
    pass(&c, T0, &[]); // drive runs, but nothing about #7
    let r = Fake::default().reply("herdr agent list", NO_AGENTS);
    assert!(check_at(&mut c, &r, T0 + 7200).is_empty());
}

#[test]
fn waiting_names_an_issue_nothing_drives() {
    let d = tmpdir("orphan-waiting");
    let mut c = ctx(&d, CONFIG);
    spawn(&c, T0, 7, "impl");
    ev(&c, T0, None, "queue", json!({"op": "add", "issues": [12]}));
    pass(&c, T0, &[]); // drive runs, but acts on neither
    let r = Fake::default().reply("herdr agent list", NO_AGENTS);
    c.now = T0 + 2400;
    let (code, out) = run(&c, &r, &["waiting"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("sendmeter #7 drive: lane gone, no push: respawning impl · nothing drives this (since 40m)"),
        "{out}"
    );
    assert!(
        out.contains("sendmeter #12 drive: next in the queue: drive starts it · nothing drives this (since 40m)"),
        "{out}"
    );
}

#[test]
fn a_lane_live_loop_on_a_finished_lane_is_an_orphan() {
    // #534's shape: every pass refused LANE_LIVE on a reviewer done hours ago; that drives nothing.
    let d = tmpdir("orphan-534");
    let mut c = ctx(&d, CONFIG);
    spawn(&c, T0, 7, "impl");
    ev(&c, T0 + 10, Some(7), "pushed", json!({"sha": SHA}));
    for t in [100, 700, 1300] {
        let a = json!({"issue": 7, "step": "review", "code": "LANE_LIVE",
            "words": "an old reviewer (done) is in the way: replacing it"});
        ev(&c, T0 + t, None, "drive-pass", json!({"actions": [a]}));
    }
    let r = Fake::default().reply("herdr agent list", NO_AGENTS);
    fires_once(&mut c, &r, T0 + 10, 7);
}

/// #558: `waiting` names why drive hasn't started a queued issue, never `→ ?`: a paused fleet, or drive's last pass
/// of the fleet and, past 10 min, a stuck tick.
#[test]
fn waiting_names_why_drive_has_not_started_a_queued_issue() {
    let d = tmpdir("orphan-558");
    let mut c = ctx(&d, CONFIG);
    ev(&c, T0, None, "queue", json!({"op": "add", "issues": [12]}));
    let r = Fake::default().reply("herdr agent list", NO_AGENTS);
    let line = |c: &drover::Ctx| {
        let (code, out) = run(c, &r, &["waiting", "sendmeter"]);
        assert_eq!(code, 0, "{out}");
        assert!(!out.contains("→ ?"), "{out}");
        out.lines()
            .find(|l| l.starts_with("sendmeter #12 drive: "))
            .unwrap()
            .to_string()
    };
    let last = c.state.parent().unwrap().join("drive-sendmeter.last");
    std::fs::write(&last, T0.to_string()).unwrap();
    c.now = T0 + 120;
    assert!(line(&c).ends_with("→ drive's next pass (last 2m ago)"), "{}", line(&c));
    c.now = T0 + 1200;
    let l = line(&c);
    assert!(
        l.ends_with("→ drive's next pass (last 20m ago): tick stuck? drover doctor"),
        "{l}"
    );
    let paused = CONFIG.replace("integration = \"staging\"", "integration = \"staging\"\npaused = true");
    std::fs::write(&c.config, paused).unwrap();
    assert!(line(&c).ends_with("→ paused: drover resume sendmeter"), "{}", line(&c));
}

/// #791: drive's last finished pass is 30 min old but a tick runs with a fresh heartbeat: `running for Nm`, never
/// `tick stuck?`; no heartbeat for 2 intervals (slow_secs 300) reads stuck.
#[test]
fn waiting_reads_a_long_running_tick_as_running_not_stuck() {
    let d = tmpdir("orphan-791");
    let mut c = ctx(&d, CONFIG);
    ev(&c, T0, None, "queue", json!({"op": "add", "issues": [12]}));
    let r = Fake::default().reply("herdr agent list", NO_AGENTS);
    let line = |c: &drover::Ctx| {
        let (code, out) = run(c, &r, &["waiting", "sendmeter"]);
        assert_eq!(code, 0, "{out}");
        out.lines()
            .find(|l| l.starts_with("sendmeter #12 drive: "))
            .unwrap()
            .to_string()
    };
    let st = c.state.parent().unwrap();
    std::fs::write(st.join("drive-sendmeter.last"), T0.to_string()).unwrap();
    c.now = T0 + 1800;
    std::fs::write(st.join("tick.beat"), format!("{} {}", c.now - 360, c.now - 30)).unwrap();
    let l = line(&c);
    assert!(l.ends_with("→ drive's pass running for 6m"), "{l}");
    std::fs::write(st.join("tick.beat"), format!("{} {}", c.now - 1200, c.now - 660)).unwrap();
    let l = line(&c);
    assert!(
        l.ends_with("→ drive's next pass (last 30m ago): tick stuck? drover doctor"),
        "{l}"
    );
}
