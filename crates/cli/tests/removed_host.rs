mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::link::view::HostSeen;
use serde_json::json;

const GONE: &str = "boat1";

fn sha() -> String {
    "a".repeat(40)
}

fn drive_fake() -> Fake {
    let pr = json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": sha(),
        "mergeable": "MERGEABLE", "statusCheckRollup": []});
    Fake::default()
        .reply(
            "gh pr view issue-12 -R jirathip-dev/sendmeter --json body",
            "Closes #12\n\n## For humans\n\nx\n",
        )
        .reply("gh api repos/jirathip-dev/sendmeter/branches/issue-12", &sha())
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("gh pr view issue-12", &pr.to_string())
        .reply("gh api repos/jirathip-dev/sendmeter/pulls/100/files", "src/a.rs\n")
        .reply("gh issue", "")
        .reply("git -C", "")
        .reply("which shed", "/opt/shed/bin/shed\n")
}

fn on_gone_host(c: &drover::Ctx, ts: u64, role: &str, agent: &str, check: bool) {
    let wt = format!("/home/user/.herdr/worktrees/sendmeter/{agent}");
    let d = json!({"role": role, "stage": role, "check": check, "agent": agent, "branch": "issue-12",
        "worktree": wt, "host": GONE, "machine": GONE, "sha": sha()});
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), "spawn", d)).unwrap();
    let row = HostSeen {
        host: GONE.into(),
        link: "live".into(),
        heard: Some(ts),
        ..HostSeen::default()
    };
    let seen = json!({"session": null, "worktree": {"path": wt}}).to_string();
    drover::db::view_put(&c.state, &row, Some(&[(agent.into(), seen)])).unwrap();
}

#[test]
fn a_verify_lane_on_the_main_host_is_driven_past_an_earlier_lane_on_a_removed_host() {
    let d = tmpdir("removed-host-verdict");
    let c = ctx(&d, CONFIG);
    on_gone_host(&c, 1, "impl", "sendmeter-impl-12", false);
    on_gone_host(&c, 2, "rev", "sendmeter-rev-12", true);
    let wt = d.join("verify-wt");
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(wt.join("verdict.md"), "FAIL\n\n- the meter is blank\n").unwrap();
    let sp = json!({"role": "rev", "stage": "rev", "check": true, "agent": "sendmeter-rev-12-r3",
        "branch": "issue-12", "worktree": wt.to_string_lossy(), "sha": sha()});
    append(&c.state, &Event::new(3, "sendmeter", Some(12), "spawn", sp)).unwrap();
    let sh = FakeShed::new();
    sh.attach(&c);
    let (code, out) = run(&c, &drive_fake(), &["waiting", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("host offline"), "{out}");
    let (code, out) = run(&c, &drive_fake(), &["drive"]);
    assert_eq!(code, 0, "{out}");
    let v: Vec<Event> = read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "verdict")
        .cloned()
        .collect();
    assert_eq!(v.len(), 1, "{out}");
    assert_eq!(v[0].str("verdict"), Some("FAIL"), "{out}");
}

#[test]
fn a_lane_only_on_a_removed_host_is_an_owner_card_naming_the_host_and_the_lane() {
    let d = tmpdir("removed-host-card");
    let c = ctx(&d, CONFIG);
    on_gone_host(&c, 1, "impl", "sendmeter-impl-12", false);
    let sh = FakeShed::new();
    sh.attach(&c);
    let (code, out) = run(&c, &drive_fake(), &["waiting", "sendmeter", "--json"]);
    assert_eq!(code, 0, "{out}");
    let rows: serde_json::Value = serde_json::from_str(&out).unwrap();
    let row = rows.as_array().unwrap().iter().find(|w| w["issue"] == 12).unwrap();
    assert_eq!(row["who"], "owner", "{out}");
    let what = row["what"].as_str().unwrap();
    assert!(what.contains(GONE) && what.contains("sendmeter-impl-12"), "{what}");
}
