mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::json;

const DESIGN: &str = "\n[fleet.sendmeter.role.design]\nharness = \"hermes\"\nprofile = \"fleet-design\"\n";
const HEAD: &str = "5bf04afe07dae022b35684353f2040f9a37d33a6";

fn sctx(name: &str, cfg: &str) -> (drover::Ctx, FakeShed) {
    let c = ctx(&tmpdir(name), cfg);
    let sh = FakeShed::new();
    sh.attach(&c);
    (c, sh)
}

fn ev(c: &drover::Ctx, n: u64, ts: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(n), kind, d)).unwrap();
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

fn state(c: &drover::Ctx, n: u64) -> String {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), n)]
        .state
        .clone()
}

fn failed(c: &drover::Ctx, n: u64) {
    let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
    let d = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "pane": format!("sendmeter-impl-{n}"), "worktree": wt,
        "branch": format!("issue-{n}")});
    ev(c, n, 100, "spawn", d);
    let d = json!({"role": "rev", "check": true, "agent": format!("sendmeter-rev-{n}-r1"), "pane": format!("sendmeter-rev-{n}-r1"),
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

fn fake(pressure: &str) -> Fake {
    let pr = json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": HEAD,
        "statusCheckRollup": []});
    Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", pressure)
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("gh pr view", &pr.to_string())
}

fn waiting(c: &drover::Ctx, r: &Fake, n: u64) -> serde_json::Value {
    let (code, j) = run(c, r, &["waiting", "--json"]);
    assert_eq!(code, 0, "{j}");
    let all: serde_json::Value = serde_json::from_str(&j).unwrap();
    let mine = |w: &&serde_json::Value| w["issue"] == n && w["lane"].as_str().is_some_and(|l| !l.contains("design"));
    all.as_array()
        .unwrap()
        .iter()
        .rev()
        .find(mine)
        .cloned()
        .unwrap_or_default()
}

#[test]
fn a_finished_design_lane_is_adopted_once_and_never_hides_the_fix_round() {
    let (mut c, sh) = sctx("design-orphan", &format!("{CONFIG}{DESIGN}"));
    failed(&c, 12);
    let wt = c.worktrees.join("sendmeter/design-12");
    std::fs::create_dir_all(&wt).unwrap();
    sh.lane("sendmeter-design-12", "done", &wt.to_string_lossy());
    let mut r = fake("4\n").reply("git -C", "");
    r.replies.retain(|(p, _)| p != "which shed");
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
    let fix = "review FAIL, waiting for drive (next pass ≤ 5 min) to start fix round · held: SHED_MISSING";
    assert!(w["what"].as_str().is_some_and(|x| x.starts_with(fix)), "{w}");
    assert_eq!(
        w["next_cmd"].as_str(),
        Some("drover spawn sendmeter 12 impl --supersede"),
        "{w}"
    );
}

#[test]
fn a_respawn_is_named_after_the_build_lane() {
    let (c, _sh) = sctx("design-respawn", CONFIG);
    failed(&c, 12);
    ev(&c, 12, 500, "needs-human", json!({"code": "ROUND_LIMIT"}));
    ev(&c, 12, 600, "round-extended", json!({"by": "owner", "spawn": false}));
    let r = fake("4\n");
    let w = waiting(&c, &r, 12);
    assert_eq!(
        (w["lane"].as_str(), w["what"].as_str()),
        (Some("sendmeter-impl-12"), Some("lane gone, no push: respawning impl")),
        "{w}"
    );
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

#[test]
fn the_install_repair_unparks_a_lane_ends_a_design_adoption_caused() {
    let (c, _sh) = sctx("design-repair", &format!("{CONFIG}{DESIGN}"));
    for n in [12, 13] {
        failed(&c, n);
    }
    let d = json!({"role": "design", "adopted": true, "why": "orphan", "agent": "sendmeter-design-12",
        "pane": "sendmeter-design-12", "branch": "design-12"});
    ev(&c, 12, 500, "spawn", d);
    ev(
        &c,
        12,
        500,
        "spawn-adopted",
        json!({"agent": "sendmeter-design-12", "pane": "sendmeter-design-12", "why": "orphan"}),
    );
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
    let r = fake("4\n");
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
    run(&c, &r, &["drive"]);
    assert_eq!(kinds(&c, "repaired").len(), 1);
}

#[test]
fn a_pane_adopted_once_is_never_adopted_again() {
    let (c, sh) = sctx("design-once", &format!("{CONFIG}{DESIGN}"));
    failed(&c, 12);
    ev(
        &c,
        12,
        500,
        "spawn-adopted",
        json!({"agent": "sendmeter-design-12", "pane": "sendmeter-design-12", "why": "orphan"}),
    );
    let wt = c.worktrees.join("sendmeter/design-12");
    sh.lane("sendmeter-design-12", "done", &wt.to_string_lossy());
    let r = fake("4\n").reply("git -C", "");
    for args in [&["drive", "--fast"][..], &["check"]] {
        let (code, out) = run(&c, &r, args);
        assert_eq!(code, 0, "{args:?}: {out}");
    }
    let adopted = (kinds(&c, "spawn").len(), kinds(&c, "spawn-adopted").len());
    assert_eq!(adopted, (2, 1), "never adopted again: {:?}", sh.methods());
    assert_eq!(kinds(&c, "orphan-lane").len(), 0);
}
