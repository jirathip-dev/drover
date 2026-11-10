//! #500: a finished design lane adopted as an orphan is adopted once and never hides the build round: no repeated
//! `orphan-lane`, no `lane gone` respawn, no `LANE_ENDS`; a respawn is named after the build lane; an issue parked
//! that way before the fix is put back.
mod common;
use common::*;
use herdsman::events::{append, fold, read, Event};
use serde_json::json;

const DESIGN: &str = "\n[fleet.sendmeter.role.design]\nharness = \"hermes\"\nprofile = \"fleet-design\"\n";
const HEAD: &str = "5bf04afe07dae022b35684353f2040f9a37d33a6";

fn ev(c: &herdsman::Ctx, n: u64, ts: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(n), kind, d)).unwrap();
}

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

fn state(c: &herdsman::Ctx, n: u64) -> String {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), n)]
        .state
        .clone()
}

/// #n built, reviewed (the second review's prompt never confirmed, as #404's), FAILed at HEAD; its impl lane closed.
fn failed(c: &herdsman::Ctx, n: u64) {
    let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
    let d = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "pane": format!("w9:i{n}"), "worktree": wt,
        "branch": format!("issue-{n}")});
    ev(c, n, 100, "spawn", d);
    let d = json!({"role": "rev", "check": true, "agent": format!("sendmeter-rev-{n}-r1"), "pane": format!("w9:r{n}"),
        "worktree": c.worktrees.join(format!("sendmeter/rev-{n}-r1")), "confirmed": false, "sha": HEAD});
    ev(c, n, 200, "spawn", d);
    ev(
        c,
        n,
        300,
        "verdict",
        json!({"verdict": "FAIL", "sha": HEAD, "stage": "rev"}),
    );
    let d = json!({"agent": format!("sendmeter-impl-{n}"), "reason": "done"});
    ev(c, n, 400, "lane-closed", d);
}

/// A calm or busy host (no spawn starts while busy), the PR at HEAD, and herdr listing `agents`.
fn fake(agents: &str, pressure: &str) -> Fake {
    let pr = json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": HEAD,
        "statusCheckRollup": []});
    Fake::default()
        .reply("herdr agent list", agents)
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", pressure)
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("gh pr view", &pr.to_string())
}

fn waiting(c: &herdsman::Ctx, r: &Fake, n: u64) -> serde_json::Value {
    let (code, j) = run(c, r, &["waiting", "--json"]);
    assert_eq!(code, 0, "{j}");
    let all: serde_json::Value = serde_json::from_str(&j).unwrap();
    let mine = |w: &&serde_json::Value| w["issue"] == n && w["lane"].as_str().is_some_and(|l| !l.contains("design"));
    // The issue's own row comes after those its older lanes share.
    all.as_array()
        .unwrap()
        .iter()
        .rev()
        .find(mine)
        .cloned()
        .unwrap_or_default()
}

/// The #404 sequence: a CI-only FAIL, then the owner's finished design pane, never logged, seen pass after pass.
#[test]
fn a_finished_design_lane_is_adopted_once_and_never_hides_the_fix_round() {
    let mut c = ctx(&tmpdir("design-orphan"), &format!("{CONFIG}{DESIGN}"));
    failed(&c, 12);
    let wt = c.worktrees.join("sendmeter/design-12");
    std::fs::create_dir_all(&wt).unwrap();
    let list = agents(&[("sendmeter-design-12", "done", &wt.to_string_lossy(), "w115:p1")]);
    let pane = json!({"result": {"pane": {"pane_id": "w115:p1", "agent": "hermes", "workspace_id": "w115"}}});
    let r = fake(&list, "4\n")
        .reply("herdr pane get w115:p1", &pane.to_string())
        .reply("herdr agent rename w115:p1 sendmeter-design-12", "{}")
        .reply("git -C", "");
    for _ in 0..7 {
        for args in [&["drive", "--fast"][..], &["check"], &["drive"]] {
            let (code, out) = run(&c, &r, args);
            assert_eq!(code, 0, "{args:?}: {out}");
        }
        c.now += 600;
    }
    let adopted = kinds(&c, "spawn");
    let adopted: Vec<_> = adopted.iter().filter(|e| e.str("role") == Some("design")).collect();
    assert_eq!(adopted.len(), 1, "one adoption: {adopted:?}");
    assert_eq!(kinds(&c, "spawn-adopted").len(), 1);
    assert!(kinds(&c, "orphan-lane").is_empty(), "{:?}", kinds(&c, "orphan-lane"));
    assert!(kinds(&c, "respawn").is_empty(), "{:?}", kinds(&c, "respawn"));
    assert!(kinds(&c, "needs-human").is_empty(), "{:?}", kinds(&c, "needs-human"));
    assert_eq!(state(&c, 12), "fix(1)");
    let w = waiting(&c, &r, 12);
    assert_eq!(
        (w["what"].as_str(), w["next_cmd"].as_str()),
        (
            Some("review FAIL, waiting for drive (next pass ≤ 5 min) to start fix round"),
            Some("herdsman spawn sendmeter 12 impl --supersede")
        ),
        "{w}"
    );
}

/// A fix round the owner extended without a spawn, its build lane gone: the respawn names `sendmeter-impl-12`, the
/// build lane, never the reviewer that ran last.
#[test]
fn a_respawn_is_named_after_the_build_lane() {
    let c = ctx(&tmpdir("design-respawn"), CONFIG);
    failed(&c, 12);
    ev(&c, 12, 500, "needs-human", json!({"code": "ROUND_LIMIT"}));
    ev(&c, 12, 600, "round-extended", json!({"by": "owner", "spawn": false}));
    let r = fake(NO_AGENTS, "4\n");
    let w = waiting(&c, &r, 12);
    assert_eq!(
        (w["lane"].as_str(), w["what"].as_str()),
        (Some("sendmeter-impl-12"), Some("lane gone, no push: respawning impl")),
        "{w}"
    );
    // Two respawns since: the park names the build lane too.
    ev(
        &c,
        12,
        700,
        "respawn",
        json!({"agent": "sendmeter-impl-12", "role": "impl", "why": "lane gone"}),
    );
    ev(
        &c,
        12,
        800,
        "respawn",
        json!({"agent": "sendmeter-impl-12", "role": "impl", "why": "lane gone"}),
    );
    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    let parked = kinds(&c, "needs-human");
    assert_eq!(
        (parked.len(), parked[1].str("code"), parked[1].str("agent")),
        (2, Some("LANE_ENDS"), Some("sendmeter-impl-12")),
        "{out}"
    );
}

/// An issue parked `LANE_ENDS` before #500 because of a design adoption is put back in its fix round; one whose lane
/// really kept ending stays parked.
#[test]
fn the_install_repair_unparks_a_lane_ends_a_design_adoption_caused() {
    let c = ctx(&tmpdir("design-repair"), &format!("{CONFIG}{DESIGN}"));
    for n in [12, 13] {
        failed(&c, n);
    }
    let d = json!({"role": "design", "adopted": true, "why": "orphan", "agent": "sendmeter-design-12", "pane": "w115:p1",
        "branch": "design-12"});
    ev(&c, 12, 500, "spawn", d);
    ev(
        &c,
        12,
        500,
        "spawn-adopted",
        json!({"agent": "sendmeter-design-12", "pane": "w115:p1", "why": "orphan"}),
    );
    // #13: its own lane-dead after the FAIL, no design lane at all.
    ev(
        &c,
        13,
        500,
        "pushed",
        json!({"sha": "1111111111111111111111111111111111111111"}),
    );
    for n in [12, 13] {
        let gone = json!({"agent": format!("sendmeter-rev-{n}-r1"), "role": "impl", "why": "lane gone"});
        ev(&c, n, 600, "respawn", gone.clone());
        ev(&c, n, 2600, "respawn", gone);
        let d = json!({"code": "LANE_ENDS", "reason": "lane keeps ending without a push"});
        ev(&c, n, 5000, "needs-human", d);
    }
    // The old fold: the adoption ended the verdict wait, so the park held.
    let r = fake(NO_AGENTS, "4\n");
    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    let fixed = kinds(&c, "repaired");
    assert_eq!(fixed.len(), 1, "{out}");
    assert_eq!(
        (fixed[0].issue, fixed[0].str("state"), fixed[0].str("code")),
        (Some(12), Some("fix(1)"), Some("LANE_ENDS"))
    );
    assert_eq!((state(&c, 12), state(&c, 13)), ("fix(1)".into(), "needs-human".into()));
    assert_eq!(
        waiting(&c, &r, 12)["what"],
        "review FAIL, waiting for drive (next pass ≤ 5 min) to start fix round · held: HOST_BUSY memory pressure critical"
    );
    // Once repaired, never again.
    run(&c, &r, &["drive"]);
    assert_eq!(kinds(&c, "repaired").len(), 1);
}

/// One adoption per pane, ever: a pane a `spawn-adopted` names is no orphan, even with its `spawn` missing.
#[test]
fn a_pane_adopted_once_is_never_adopted_again() {
    let c = ctx(&tmpdir("design-once"), &format!("{CONFIG}{DESIGN}"));
    failed(&c, 12);
    ev(
        &c,
        12,
        500,
        "spawn-adopted",
        json!({"agent": "sendmeter-design-12", "pane": "w115:p1", "why": "orphan"}),
    );
    let wt = c.worktrees.join("sendmeter/design-12");
    let list = agents(&[("sendmeter-design-12", "done", &wt.to_string_lossy(), "w115:p1")]);
    let pane = json!({"result": {"pane": {"pane_id": "w115:p1", "agent": "hermes", "workspace_id": "w115"}}});
    let r = fake(&list, "4\n")
        .reply("herdr pane get w115:p1", &pane.to_string())
        .reply("herdr", "{}")
        .reply("git -C", "");
    for args in [&["drive", "--fast"][..], &["check"]] {
        let (code, out) = run(&c, &r, args);
        assert_eq!(code, 0, "{args:?}: {out}");
    }
    assert!(!r.called("herdr agent rename"), "{:?}", r.calls.borrow());
    assert_eq!((kinds(&c, "spawn").len(), kinds(&c, "orphan-lane").len()), (2, 0));
}
