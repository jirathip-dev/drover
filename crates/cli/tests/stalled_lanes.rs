//! #666: a stalled lane is superseded once, then the owner's (one needs-human card with its last screen); a lane that
//! never started is retried once, then escalated; a lane the hook keeps refusing is ended with the refusal on its card.
mod common;
use common::*;
use herdsman::events::{append, fold, read, Event};
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

fn ev(c: &herdsman::Ctx, ts: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
}

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

fn state(c: &herdsman::Ctx) -> String {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)]
        .state
        .clone()
}

fn at(c: &herdsman::Ctx, now: u64) -> herdsman::Ctx {
    herdsman::Ctx { now, ..c.clone() }
}

fn pr(head: &str) -> String {
    json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": head, "mergeable": "MERGEABLE",
        "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}]})
    .to_string()
}

/// Spawns succeed and the host is calm; herdr lists `agents` and every pane shows `screen`.
fn fake(agents: &str, screen: &str) -> Fake {
    Fake::default()
        .reply("herdr agent list", agents)
        .reply("herdr pane read", screen)
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
        .reply("herdr worktree", r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#)
        .reply(
            "herdr pane list",
            r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#,
        )
        .reply("herdr pane", "{}")
        .reply("herdr agent", "{}")
}

fn drive(c: &herdsman::Ctx, r: &Fake) -> String {
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    out
}

/// #12 built and rev PASSed at `head`; its verify lane spawned at `t` in a worktree with no verdict.md.
fn verify_lane(name: &str, t: u64) -> (herdsman::Ctx, String) {
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
        "pane": "w9:v1", "workspace": "w9", "worktree": wt, "branch": "verify-12-r1", "sha": head});
    ev(&c, t, "spawn", d);
    (c, head)
}

/// AC 2 (the regression): cooee-verify-34-r3 ended with no verdict.md and sat 7 h with only `stall` events.
#[test]
fn a_verify_lane_that_ends_without_a_verdict_is_respawned_once_then_needs_a_human() {
    let t0 = 1_000_000 - 60;
    let (c, head) = verify_lane("stall-verify", t0);
    let list = agents(&[("sendmeter-verify-12-r1", "done", "/w/verify-12-r1", "w9:v1")]);
    let r =
        fake(&list, "Opened the app at 1440 wide\nSaved shot-1440.png\n> ").reply("gh pr view issue-12", &pr(&head));
    // Inside `stall_minutes`: nothing yet.
    drive(&c, &r);
    assert!(kinds(&c, "respawn").is_empty());
    assert!(!r.called("herdr agent start sendmeter-verify"));
    // Stalled: superseded once with a fresh lane of the same role and round.
    let c1 = at(&c, t0 + STALL + 1);
    let out = drive(&c1, &r);
    let rs = kinds(&c, "respawn");
    assert_eq!(rs.len(), 1, "{out}");
    assert_eq!(
        (rs[0].str("role"), rs[0].str("why")),
        (Some("verify"), Some("lane stalled"))
    );
    assert!(r.called("herdr agent start sendmeter-verify-12-r1"), "{out}");
    assert!(state(&c).starts_with("review"), "{}", state(&c));
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

/// AC 3: cooee-drover #12's pane never started (blank) and sat 28 h.
#[test]
fn a_blank_pane_after_the_start_window_is_retried_once_then_escalated() {
    let c = ctx(&tmpdir("stall-blank"), CONFIG);
    let t0 = 1_000_000 - 60;
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "w9:p1", "workspace": "w9", "worktree": wt,
        "branch": "issue-12", "confirmed": true});
    ev(&c, t0, "spawn", d);
    let list = agents(&[("sendmeter-impl-12", "idle", &wt.to_string_lossy(), "w9:p1")]);
    let r = fake(&list, "\n  \n");
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
    assert!(r.called("herdr agent start sendmeter-impl-12"), "{out}");
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

/// AC 4: cooee-drover #13's lane was refused by the hook for an unknown fleet and sat 28 h.
#[test]
fn repeated_hook_refusals_end_the_lane_with_an_escalation_that_carries_the_refusal() {
    let c = ctx(&tmpdir("stall-hook"), CONFIG);
    let t0 = 1_000_000 - 120;
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "w9:p1", "workspace": "w9", "worktree": wt,
        "branch": "issue-12", "confirmed": true});
    ev(&c, t0, "spawn", d);
    let cause = "fleet `gone` is not in fleet.toml";
    let refused = json!({"cause": cause, "lane": "sendmeter:12:impl"});
    ev(&c, t0 + 30, "hook-failed-closed", refused.clone());
    let list = agents(&[("sendmeter-impl-12", "idle", &wt.to_string_lossy(), "w9:p1")]);
    let r = fake(&list, "Bash(git status)\n  REFUSED HOOK_FAILED_CLOSED\n> ");
    drive(&c, &r);
    assert!(kinds(&c, "needs-human").is_empty(), "one refusal is no loop yet");
    assert!(!r.called("herdr pane close w9:p1"));
    ev(&c, t0 + 60, "hook-failed-closed", refused);
    let out = drive(&c, &r);
    assert!(r.called("herdr pane close w9:p1"), "the lane is ended: {out}");
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
