mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::json;

fn fleet(name: &str, host: &str, extra: &str, issues: &[&str], flags: &[&str]) -> (drover::Ctx, FakeShed) {
    let cfg = CONFIG
        .replace("max_parallel_spawns = 0", &format!("max_parallel_spawns = {extra}"))
        .replace("[limits]", &format!("[hosts.mini]\n{host}\n\n[limits]"));
    let c = ctx(&tmpdir(name), &cfg);
    let add = [&["queue", "sendmeter", "add"][..], issues, flags].concat();
    assert!(issues.is_empty() || run(&c, &Fake::default(), &add).0 == 0);
    let sh = FakeShed::new();
    sh.attach(&c);
    (c, sh)
}

fn fake() -> Fake {
    Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.0 1.0 1.0 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "")
}

fn drive(c: &drover::Ctx) -> String {
    let (code, out) = run(c, &fake(), &["drive"]);
    assert_eq!(code, 0, "{out}");
    out
}

fn state(c: &drover::Ctx, n: u64) -> String {
    let st = fold(&read(&c.state).unwrap());
    st.get(&("sendmeter".to_string(), n))
        .map_or("queued".into(), |s| s.state.clone())
}

fn spawned(sh: &FakeShed) -> Vec<String> {
    let runs = sh.all("run").into_iter();
    runs.filter_map(|p| p["name"].as_str().map(String::from)).collect()
}

#[test]
fn over_max_working_a_start_waits_then_starts_when_a_lane_finishes() {
    let (c, sh) = fleet("slots-cap", "max_working = 2", "0", &["1", "2", "3"], &[]);
    let out = drive(&c);
    assert_eq!(spawned(&sh), ["sendmeter-impl-1", "sendmeter-impl-2"], "{out}");
    assert!(out.contains("waiting for a slot: 2/2 working on mini"), "{out}");
    assert_eq!((state(&c, 2), state(&c, 3)), ("impl".into(), "queued".into()));
    let (code, st) = run(&c, &fake(), &["status"]);
    assert_eq!(code, 0, "{st}");
    assert!(st.contains("mini: working 2/2"), "{st}");
    assert!(st.contains("waiting for a slot: 2/2 working on mini"), "{st}");

    sh.set("sendmeter-impl-1", |t| t["reported"] = json!({"state": "idle"}));
    let out = drive(&c);
    assert_eq!(state(&c, 3), "impl", "{out}");
    assert!(spawned(&sh).contains(&"sendmeter-impl-3".to_string()), "{out}");
}

#[test]
fn a_ready_or_needs_human_lane_holds_no_slot_even_if_its_agent_works() {
    let (c, sh) = fleet("slots-idle", "max_working = 1", "0", &["1", "2"], &[]);
    drive(&c);
    assert_eq!(spawned(&sh), ["sendmeter-impl-1"]);
    sh.set("sendmeter-impl-1", |t| t["reported"] = json!({"state": "working"}));
    for kind in ["ready", "needs-human"] {
        let e = Event::new(c.now, "sendmeter", Some(1), kind, json!({"reason": "x"}));
        append(&c.state, &e).unwrap();
    }
    let out = drive(&c);
    assert_eq!(state(&c, 2), "impl", "{out}");
}

#[test]
fn with_max_simulators_one_a_second_sim_lane_waits() {
    let (c, sh) = fleet(
        "slots-sim",
        "tags = [\"sim\"]\nmax_simulators = 1",
        "0",
        &["1", "2"],
        &["--needs", "sim"],
    );
    let out = drive(&c);
    assert_eq!(spawned(&sh), ["sendmeter-impl-1"], "{out}");
    assert!(out.contains("waiting for a slot: 1/1 simulators on mini"), "{out}");
    sh.set("sendmeter-impl-1", |t| t["reported"] = json!({"state": "idle"}));
    drive(&c);
    assert_eq!(state(&c, 2), "queued");
}

#[test]
fn kicked_starts_count_against_the_cap_before_any_lane_is_up() {
    let (c, _sh) = fleet("slots-kicks", "max_working = 2", "6", &["1", "2", "3"], &[]);
    let r = fake();
    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    let kick = |n: u64| {
        r.count(&format!(
            "sh -c nohup '/opt/drover/bin/drover' kick sendmeter {n} --why drive --now"
        ))
    };
    assert_eq!((kick(1), kick(2), kick(3)), (1, 1, 0), "{out}");
    let ev = read(&c.state).unwrap();
    let pass = ev.iter().rfind(|e| e.kind == "drive-pass").unwrap();
    let acts = pass.data["actions"].as_array().unwrap();
    let three = acts.iter().find(|a| a["issue"] == 3).unwrap();
    assert_eq!(three["code"], "HOST_FULL", "{pass:?}");
}

#[test]
fn a_recorded_rev_verdict_closes_the_rev_pane() {
    let (c, sh) = fleet("slots-verdict", "", "0", &[], &[]);
    let wt = c.worktrees.join("sendmeter/issue-12-rev");
    std::fs::create_dir_all(&wt).unwrap();
    let wt = wt.to_string_lossy().into_owned();
    let ev = |ts, kind: &str, d| append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
    let rev = "sendmeter-rev-12";
    ev(
        c.now - 600,
        "spawn",
        json!({"role": "rev", "agent": rev, "pane": rev, "worktree": wt}),
    );
    sh.lane(rev, "idle", &wt);
    let r = Fake::default()
        .reply(&format!("git -C {wt} rev-list --count"), "0\n")
        .reply("git -C", "");
    assert_eq!(run(&c, &r, &["check"]).0, 0);
    assert!(sh.has(rev), "no verdict yet: the reviewer stays");
    ev(
        c.now - 60,
        "verdict",
        json!({"verdict": "PASS", "sha": "abc", "body": "PASS"}),
    );
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(sh.called("kill", rev) && !sh.has(rev), "{out} {:?}", sh.methods());
}
