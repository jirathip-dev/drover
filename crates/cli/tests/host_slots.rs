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
    look(&c, &sh.on(&fake()));
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
    let acts = pass.val("actions").as_array().unwrap();
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

const HOSTS: &str = "[hosts.a]\nmachine = \"a\"\nmax_working = 4\n\n[hosts.b]\nmax_working = 4\ntags = [\"gpu\"]\n\n\
[hosts.c]\nmachine = \"c\"\nmax_working = 4\n\n[limits]";

fn three_hosts(name: &str, needs: &[&str]) -> (drover::Ctx, FakeShed) {
    let c = ctx(&tmpdir(name), &CONFIG.replace("[limits]", HOSTS));
    let add = [&["queue", "sendmeter", "add", "9"][..], needs].concat();
    assert_eq!(run(&c, &Fake::default(), &add).0, 0);
    let seen = json!({"a": {"up": false}, "b": {"up": true, "tags": ["gpu"]}, "c": {"up": true}});
    std::fs::write(c.state.with_file_name("hosts.json"), seen.to_string()).unwrap();
    let sh = FakeShed::new();
    sh.attach(&c);
    (c, sh)
}

fn bound(c: &drover::Ctx, n: u64, host: &str) -> Event {
    let at = |kind, d| append(&c.state, &Event::new(c.now, "sendmeter", Some(n), kind, d)).unwrap();
    at(
        "spawn-started",
        json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "host": host}),
    );
    Event::new(c.now, "sendmeter", Some(n), "spawn", json!({"role": "impl"}))
}

fn slot(c: &drover::Ctx, start: &[Event]) -> Option<String> {
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let ev = read(&c.state).unwrap();
    drover::hosts::slot_before(c, &cfg, &fake(), &ev, ("sendmeter", 9, "impl"), start)
}

#[test]
fn an_offline_host_and_a_full_one_leave_the_idle_host_free() {
    let (c, sh) = three_hosts("slots-offline", &[]);
    for n in 1..=4 {
        let d = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "pane": format!("sendmeter-impl-{n}")});
        append(&c.state, &Event::new(c.now, "sendmeter", Some(n), "spawn", d)).unwrap();
        sh.lane(&format!("sendmeter-impl-{n}"), "working", "/w");
    }
    assert_eq!(slot(&c, &[]), None);
}

#[test]
fn a_pending_start_counts_only_against_its_own_host() {
    let (c, _sh) = three_hosts("slots-bound", &[]);
    let start: Vec<Event> = (1..=4).map(|n| bound(&c, n, "b")).collect();
    assert_eq!(slot(&c, &start), None);
    let (c, _sh) = three_hosts("slots-bound-gpu", &["--needs", "gpu"]);
    let start: Vec<Event> = (1..=4).map(|n| bound(&c, n, "b")).collect();
    assert_eq!(
        slot(&c, &start).as_deref(),
        Some("waiting for a slot: 4/4 working on b")
    );
}

#[test]
fn when_every_up_host_is_full_the_hold_never_names_the_offline_one() {
    let (c, _sh) = three_hosts("slots-all-full", &[]);
    let start: Vec<Event> = (1..=8).map(|n| bound(&c, n, if n > 4 { "c" } else { "b" })).collect();
    let why = slot(&c, &start).unwrap_or_default();
    assert!(
        why == "waiting for a slot: 4/4 working on b" || why == "waiting for a slot: 4/4 working on c",
        "{why}"
    );
}

fn acp_lanes(c: &drover::Ctx, sh: &FakeShed, states: &[&str]) {
    for (n, st) in (1..).zip(states) {
        let name = format!("sendmeter-impl-{n}");
        let d = json!({"role": "impl", "agent": name, "pane": name});
        append(&c.state, &Event::new(c.now, "sendmeter", Some(n), "spawn", d)).unwrap();
        sh.lane(&name, st, "/w");
        sh.set(&name, |t| {
            t["acp"] = json!("claude-acp");
            if *st == "unreported" {
                t["reported"] = json!(null);
            }
        });
    }
}

#[test]
fn acp_panes_between_turns_hold_no_slot_and_a_new_start_is_placed() {
    let (c, sh) = fleet("slots-acp-idle", "max_working = 4", "0", &["20"], &[]);
    let cfg = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(
        &c.config,
        cfg.replace("[fleet.sendmeter]\n", "[fleet.sendmeter]\nmax_in_flight = 0\n"),
    )
    .unwrap();
    let ended = [
        "idle",
        "hibernated",
        "unreported",
        "done",
        "idle",
        "hibernated",
        "unreported",
        "idle",
        "done",
        "idle",
    ];
    acp_lanes(&c, &sh, &[&ended[..], &["working"]].concat());
    let (code, st) = run(&c, &fake(), &["status"]);
    assert_eq!(code, 0, "{st}");
    assert!(st.contains("mini: working 1/4"), "{st}");
    let out = drive(&c);
    assert!(!out.contains("waiting for a slot"), "{out}");
    assert!(spawned(&sh).contains(&"sendmeter-impl-20".to_string()), "{out}");
    assert_eq!(state(&c, 20), "impl", "{out}");
}

#[test]
fn four_mid_turn_acp_panes_fill_the_host_whatever_idle_ones_sit_there() {
    let (c, sh) = fleet("slots-acp-full", "max_working = 4", "0", &["20"], &[]);
    let states = [
        "working",
        "idle",
        "running",
        "hibernated",
        "working",
        "unreported",
        "working",
        "idle",
    ];
    acp_lanes(&c, &sh, &states);
    let out = drive(&c);
    assert!(out.contains("waiting for a slot: 4/4 working on mini"), "{out}");
    assert!(!spawned(&sh).contains(&"sendmeter-impl-20".to_string()), "{out}");
}
