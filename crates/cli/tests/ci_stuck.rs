mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::json;

fn stuck(updated: &str) -> (drover::Ctx, Fake, FakeShed) {
    let c = ctx(&tmpdir("drive-ci-stuck"), CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
    let wt = c.worktrees.join("sendmeter").join("issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let ev = |ts, kind: &str, d| append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
    let head = format!("abc{}", "0".repeat(37));
    ev(
        10,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12", "workspace": "",
            "runtime": "shed", "worktree": wt, "branch": "issue-12"}),
    );
    ev(30, "verdict", json!({"verdict": "PASS", "sha": head, "body": "PASS"}));
    let check = json!({"__typename": "CheckRun", "name": "actionlint", "status": "IN_PROGRESS",
        "conclusion": null, "databaseId": 4242, "startedAt": "1970-01-12T13:00:00Z",
        "detailsUrl": "https://github.com/jirathip-dev/sendmeter/actions/runs/777/job/4242"});
    let pr = json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": head,
        "statusCheckRollup": [check]});
    let r = Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("gh pr view issue-12", &pr.to_string())
        .reply(
            "gh api repos/jirathip-dev/sendmeter/actions/runs/777 --jq",
            &format!("completed {updated}\n"),
        )
        .reply("gh run rerun", "")
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply("git -C", "");
    (c, r, sh)
}

fn drive(c: &drover::Ctx, r: &Fake) {
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
}

fn kinds(c: &drover::Ctx, k: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == k)
        .cloned()
        .collect()
}

fn state(c: &drover::Ctx) -> String {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)]
        .state
        .clone()
}

#[test]
fn a_check_pending_after_its_run_completed_is_rerun_once_then_alerts() {
    let (c, r, _sh) = stuck("1970-01-12T13:10:00Z");
    drive(&c, &r);
    assert_eq!(state(&c), "ready");
    assert_eq!(
        r.count("gh run rerun --job 4242 -R jirathip-dev/sendmeter"),
        1,
        "{:?}",
        r.calls.borrow()
    );
    assert_eq!(kinds(&c, "ci-rerun").len(), 1);
    drive(&c, &r);
    drive(&c, &r);
    assert_eq!(r.count("gh run rerun"), 1, "no second re-run for the same head");
    assert_eq!(kinds(&c, "ci-rerun").len(), 1);
    let alerts = kinds(&c, "ci-stuck");
    assert_eq!(alerts.len(), 1);
    assert!(alerts[0].str("reason").unwrap().contains("`actionlint`"));
    assert_eq!(state(&c), "ready");
}

#[test]
fn a_run_completed_under_the_limit_ago_is_not_rerun() {
    let (c, r, _sh) = stuck("1970-01-12T13:40:00Z");
    drive(&c, &r);
    assert!(!r.called("gh run rerun"), "{:?}", r.calls.borrow());
    assert!(kinds(&c, "ci-rerun").is_empty());
}
