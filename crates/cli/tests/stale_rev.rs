mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::json;

const HEAD: &str = "c000000000000000000000000000000000000000";
const REV: &str = "sendmeter-rev-12-r2";

fn ev(c: &drover::Ctx, ts: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

fn at(c: &drover::Ctx, now: u64) -> drover::Ctx {
    drover::Ctx { now, ..c.clone() }
}

fn stuck(name: &str) -> (drover::Ctx, FakeShed) {
    let c = ctx(&tmpdir(name), CONFIG);
    let sh = FakeShed::attached(&c);
    let impl_ = json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"});
    ev(&c, 10, "spawn", impl_.clone());
    ev(&c, 20, "pushed", json!({"sha": "a".repeat(40)}));
    let rev = |r: u32| json!({"role": "rev", "stage": "rev", "check": true, "agent": format!("sendmeter-rev-12-r{r}")});
    ev(&c, 21, "spawn", rev(1));
    let d = json!({"verdict": "FAIL", "sha": "a".repeat(40), "stage": "rev", "loop": "rev", "body": "FAIL"});
    ev(&c, 30, "verdict", d);
    ev(&c, 40, "spawn", impl_.clone());
    ev(&c, 50, "pushed", json!({"sha": "b".repeat(40)}));
    let wt = c.worktrees.join("sendmeter/rev-12-r2");
    std::fs::create_dir_all(&wt).unwrap();
    let mut d = rev(2);
    d["pane"] = json!(REV);
    d["workspace"] = json!("");
    d["worktree"] = json!(wt);
    d["branch"] = json!("rev-12-r2");
    ev(&c, 60, "spawn", d);
    ev(&c, 70, "spawn", impl_);
    ev(&c, 80, "pushed", json!({"sha": HEAD}));
    (c, sh)
}

fn fake(sh: &FakeShed, screen: &str) -> Fake {
    sh.lane(REV, "working", "/w/rev-12-r2");
    sh.screen(REV, screen);
    let pr = json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": HEAD,
        "mergeable": "MERGEABLE", "statusCheckRollup": []});
    Fake::default()
        .reply("gh pr view issue-12", &pr.to_string())
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "")
        .reply("which shed", "/opt/shed/bin/shed\n")
}

fn starts(sh: &FakeShed) -> usize {
    sh.all("run").iter().filter(|p| p["name"] == REV).count()
}

fn drive(c: &drover::Ctx, r: &Fake) -> String {
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    out
}

const QUIET: &str = "Reading crates/core/src/drive.rs\n> ";

#[test]
fn a_live_reviewer_quiet_two_hours_without_a_verdict_is_replaced_in_the_same_round() {
    let (c, sh) = stuck("stale-rev");
    let t0 = 1_000_000;
    let r = fake(&sh, QUIET);
    let out = drive(&at(&c, t0), &r);
    assert!(kinds(&c, "rev-stale-replaced").is_empty(), "{out}");
    assert_eq!(starts(&sh), 0, "{out}");
    let r = fake(&sh, QUIET);
    let out = drive(&at(&c, t0 + 119 * 60), &r);
    assert!(kinds(&c, "rev-stale-replaced").is_empty(), "{out}");
    let r = fake(&sh, QUIET);
    let out = drive(&at(&c, t0 + 121 * 60), &r);
    let st = kinds(&c, "rev-stale-replaced");
    assert_eq!(st.len(), 1, "{out}");
    assert_eq!(st[0].str("agent"), Some("sendmeter-rev-12-r2"));
    assert_eq!(st[0].str("new"), Some("sendmeter-rev-12-r2"));
    assert_eq!(st[0].data["idle_minutes"].as_u64(), Some(121));
    assert_eq!(starts(&sh), 1, "{out}");
    let s = fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)].clone();
    assert_eq!(s.state, "review(2)", "no round burned");
    assert_eq!(s.rounds, 1);
}

#[test]
fn a_reviewer_whose_pane_changed_is_not_replaced() {
    let (c, sh) = stuck("stale-rev-busy");
    let t0 = 1_000_000;
    drive(&at(&c, t0), &fake(&sh, QUIET));
    let r = fake(
        &sh,
        "Reading crates/core/src/drive.rs\nRunning cargo test -p drover\n> ",
    );
    let out = drive(&at(&c, t0 + 121 * 60), &r);
    assert!(kinds(&c, "rev-stale-replaced").is_empty(), "{out}");
    assert_eq!(starts(&sh), 0, "{out}");
    let r = fake(
        &sh,
        "Reading crates/core/src/drive.rs\nRunning cargo test -p drover\n> ",
    );
    let out = drive(&at(&c, t0 + 200 * 60), &r);
    assert!(kinds(&c, "rev-stale-replaced").is_empty(), "{out}");
}

#[test]
fn a_reviewer_mid_tool_call_is_not_replaced() {
    let (c, sh) = stuck("stale-rev-tool");
    let t0 = 1_000_000;
    drive(&at(&c, t0), &fake(&sh, QUIET));
    let open = json!({"sendmeter-rev-12-r2": {"turn": true, "tools": {"t7": "cargo test"}}});
    std::fs::write(c.state.with_file_name("shed-out.json"), open.to_string()).unwrap();
    let r = fake(&sh, QUIET);
    let out = drive(&at(&c, t0 + 121 * 60), &r);
    assert!(kinds(&c, "rev-stale-replaced").is_empty(), "{out}");
    assert_eq!(starts(&sh), 0, "{out}");
}
