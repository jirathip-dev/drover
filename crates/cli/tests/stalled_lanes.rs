//! #666: a stalled lane is superseded once, then the owner's (one needs-human card with its last screen); a lane that
//! never started is retried once, then escalated; a lane the hook keeps refusing is ended with the refusal on its card.
mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::json;

const VERIFY: &str = "\n[fleet.sendmeter.role.verify]\nharness = \"hermes\"\nprofile = \"fleet-verify\"\n";
const STALL: u64 = 30 * 60;

fn config() -> String {
    CONFIG.replace(
        "integration = \"staging\"   # never main",
        "integration = \"staging\"\nui_paths = [\"web/**\"]",
    ) + VERIFY
}

fn sha(p: &str) -> String {
    format!("{p}{}", "0".repeat(40 - p.len()))
}

fn ev(c: &drover::Ctx, ts: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

fn state(c: &drover::Ctx) -> String {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)]
        .state
        .clone()
}

fn at(c: &drover::Ctx, now: u64) -> drover::Ctx {
    drover::Ctx { now, ..c.clone() }
}

fn pr(head: &str) -> String {
    json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": head, "mergeable": "MERGEABLE",
        "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}]})
    .to_string()
}

fn fake() -> Fake {
    Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply(
            "gh api repos/jirathip-dev/sendmeter/pulls/100/files",
            r#"[{"filename":"web/a.ts"}]"#,
        )
        .reply("git -C", "")
        .reply("which shed", "/opt/shed/bin/shed\n")
}

fn shed_on(c: &drover::Ctx, name: &str, status: &str, cwd: &str, screen: &str) -> FakeShed {
    let sh = FakeShed::new();
    sh.attach(c);
    sh.lane(name, status, cwd);
    sh.screen(name, screen);
    sh
}

fn drive(c: &drover::Ctx, r: &Fake) -> String {
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    out
}

/// #12 built and rev PASSed at `head`; its verify lane spawned at `t` in a worktree with no verdict.md.
fn verify_lane(name: &str, t: u64) -> (drover::Ctx, String) {
    let c = ctx(&tmpdir(name), &config());
    let head = sha("abc");
    ev(
        &c,
        10,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"}),
    );
    ev(&c, 20, "pushed", json!({"sha": head}));
    ev(
        &c,
        21,
        "spawn",
        json!({"role": "rev", "stage": "rev", "check": true, "agent": "sendmeter-rev-12-r1"}),
    );
    let d = json!({"verdict": "PASS", "sha": head, "stage": "rev", "loop": "rev", "body": "PASS"});
    ev(&c, 30, "verdict", d);
    let wt = c.worktrees.join("sendmeter/verify-12-r1");
    std::fs::create_dir_all(&wt).unwrap();
    let d = json!({"role": "verify", "stage": "verify", "check": true, "agent": "sendmeter-verify-12-r1",
        "pane": "sendmeter-verify-12-r1", "workspace": "", "runtime": "shed", "worktree": wt, "branch": "verify-12-r1", "sha": head});
    ev(&c, t, "spawn", d);
    (c, head)
}

/// AC 2 (the regression): cooee-verify-34-r3 ended with no verdict.md and sat 7 h with only `stall` events.
#[test]
fn a_verify_lane_that_ends_without_a_verdict_is_respawned_once_then_needs_a_human() {
    let t0 = 1_000_000 - 60;
    let (c, head) = verify_lane("stall-verify", t0);
    let screen = "Opened the app at 1440 wide\nSaved shot-1440.png\n> ";
    let sh = shed_on(&c, "sendmeter-verify-12-r1", "done", "/w/verify-12-r1", screen);
    let r = fake().reply("gh pr view issue-12", &pr(&head));
    // Inside `stall_minutes`: nothing yet.
    drive(&c, &r);
    assert!(kinds(&c, "respawn").is_empty());
    assert!(!sh.called("run", "sendmeter-verify-12-r1"));
    // Stalled: superseded once with a fresh lane of the same role and round.
    let c1 = at(&c, t0 + STALL + 1);
    let out = drive(&c1, &r);
    let rs = kinds(&c, "respawn");
    assert_eq!(rs.len(), 1, "{out}");
    assert_eq!(
        (rs[0].str("role"), rs[0].str("why")),
        (Some("verify"), Some("lane stalled"))
    );
    assert!(sh.called("run", "sendmeter-verify-12-r1"), "{out}");
    assert!(state(&c).starts_with("review"), "{}", state(&c));
    sh.lane("sendmeter-verify-12-r1", "done", "/w/verify-12-r1");
    sh.screen("sendmeter-verify-12-r1", screen);
    // The respawned lane stalls too: one needs-human card with the evidence (its last screen lines).
    let c2 = at(&c, t0 + 2 * STALL + 2);
    let out = drive(&c2, &r);
    assert_eq!(state(&c), "needs-human", "{out}");
    let nh = kinds(&c, "needs-human");
    assert_eq!(nh.len(), 1);
    assert_eq!(nh[0].str("code"), Some("LANE_STALLED"));
    let why = nh[0].str("reason").unwrap();
    assert!(
        why.contains("sendmeter-verify-12-r1") && why.contains("Saved shot-1440.png"),
        "{why}"
    );
    assert_eq!(kinds(&c, "respawn").len(), 1, "never a second respawn");
    // It never loops.
    drive(&at(&c, t0 + 4 * STALL), &r);
    assert_eq!(kinds(&c, "needs-human").len(), 1);
    assert_eq!(kinds(&c, "respawn").len(), 1);
}

#[test]
fn a_blank_pane_after_the_start_window_is_retried_once_then_escalated() {
    let c = ctx(&tmpdir("stall-blank"), CONFIG);
    let t0 = 1_000_000 - 60;
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12", "workspace": "", "runtime": "shed", "worktree": wt,
        "branch": "issue-12", "confirmed": true});
    ev(&c, t0, "spawn", d);
    let sh = shed_on(&c, "sendmeter-impl-12", "idle", &wt.to_string_lossy(), "\n  \n");
    let r = fake();
    drive(&c, &r);
    assert!(kinds(&c, "respawn").is_empty(), "inside the start window");
    let start = 10 * 60;
    let out = drive(&at(&c, t0 + start + 1), &r);
    let rs = kinds(&c, "respawn");
    assert_eq!(rs.len(), 1, "{out}");
    assert_eq!(
        (rs[0].str("role"), rs[0].str("why")),
        (Some("impl"), Some("failed start"))
    );
    assert!(sh.called("run", "sendmeter-impl-12"), "{out}");
    sh.lane("sendmeter-impl-12", "idle", &wt.to_string_lossy());
    sh.screen("sendmeter-impl-12", "\n  \n");
    // The retry shows nothing either: escalated, with no stall wait.
    let out = drive(&at(&c, t0 + 2 * start + 2), &r);
    assert_eq!(state(&c), "needs-human", "{out}");
    let nh = kinds(&c, "needs-human");
    assert_eq!(nh.len(), 1);
    assert_eq!(nh[0].str("code"), Some("START_FAILED"));
    assert!(nh[0].str("reason").unwrap().contains("sendmeter-impl-12"), "{nh:?}");
    drive(&at(&c, t0 + 4 * start), &r);
    assert_eq!((kinds(&c, "needs-human").len(), kinds(&c, "respawn").len()), (1, 1));
}

#[test]
fn repeated_hook_refusals_end_the_lane_with_an_escalation_that_carries_the_refusal() {
    let c = ctx(&tmpdir("stall-hook"), CONFIG);
    let t0 = 1_000_000 - 120;
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12", "workspace": "", "runtime": "shed", "worktree": wt,
        "branch": "issue-12", "confirmed": true});
    ev(&c, t0, "spawn", d);
    let cause = "fleet `gone` is not in fleet.toml";
    let refused = json!({"cause": cause, "lane": "sendmeter:12:impl"});
    ev(&c, t0 + 30, "hook-failed-closed", refused.clone());
    let screen = "Bash(git status)\n  REFUSED HOOK_FAILED_CLOSED\n> ";
    let sh = shed_on(&c, "sendmeter-impl-12", "idle", &wt.to_string_lossy(), screen);
    let r = fake();
    drive(&c, &r);
    assert!(kinds(&c, "needs-human").is_empty(), "one refusal is no loop yet");
    assert!(!sh.called("kill", "sendmeter-impl-12"));
    ev(&c, t0 + 60, "hook-failed-closed", refused);
    let out = drive(&c, &r);
    assert!(sh.called("kill", "sendmeter-impl-12"), "the lane is ended: {out}");
    assert_eq!(state(&c), "needs-human", "{out}");
    let nh = kinds(&c, "needs-human");
    assert_eq!(nh.len(), 1);
    assert_eq!(nh[0].str("code"), Some("HOOK_REFUSED"));
    let why = nh[0].str("reason").unwrap();
    assert!(why.contains(&format!("REFUSED HOOK_FAILED_CLOSED: {cause}")), "{why}");
    assert!(kinds(&c, "respawn").is_empty(), "never respawned into the same refusal");
    drive(&at(&c, 1_000_000 + 3 * STALL), &r);
    assert_eq!(kinds(&c, "needs-human").len(), 1);
}
