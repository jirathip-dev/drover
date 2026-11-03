//! #318: a pass decides its spawns and launches each as a detached kick, never waiting on a harness start;
//! `max_parallel_spawns` bounds them, a `spawn-<fleet>-<n>.pending` marker stops a double launch until the kick ends.
mod common;
use common::*;
use herdsman::events::read;

fn fleet(name: &str, max: usize, issues: u64) -> herdsman::Ctx {
    fleet_with(name, max, "", issues, &[])
}

/// A fleet with `extra` config lines and issues 1..=`issues` queued with `flags`.
fn fleet_with(name: &str, max: usize, extra: &str, issues: u64, flags: &[&str]) -> herdsman::Ctx {
    let cfg = CONFIG
        .replace("max_parallel_spawns = 0", &format!("max_parallel_spawns = {max}"))
        .replace(
            "integration = \"staging\"",
            &format!("integration = \"staging\"\n{extra}"),
        );
    let c = ctx(&tmpdir(name), &cfg);
    let mut add = vec!["queue".to_string(), "sendmeter".into(), "add".into()];
    add.extend((1..=issues).map(|n| n.to_string()));
    add.extend(flags.iter().map(|f| f.to_string()));
    let add: Vec<&str> = add.iter().map(String::as_str).collect();
    assert_eq!(run(&c, &Fake::default(), &add).0, 0);
    c
}

fn codes(d: &serde_json::Value) -> Vec<String> {
    let a = d["actions"].as_array().unwrap();
    a.iter().map(|a| a["code"].as_str().unwrap_or("").to_string()).collect()
}

/// Spawns succeed; `load` is the 1-min load on 10 cores (≥ 25 is a busy host).
fn fake(load: &str) -> Fake {
    Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", &format!("{{ {load} {load} {load} }}\n"))
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("herdr worktree", r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#)
        .reply(
            "herdr pane list",
            r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#,
        )
        .reply("herdr", "{}")
        .reply("git -C", "")
}

fn launched(r: &Fake, n: u64) -> usize {
    r.count(&format!(
        "sh -c nohup '/opt/herdsman/bin/herdsman' kick sendmeter {n} --why drive --now"
    ))
}

fn marker(c: &herdsman::Ctx, n: u64) -> std::path::PathBuf {
    c.state.with_file_name(format!("spawn-sendmeter-{n}.pending"))
}

fn pass(c: &herdsman::Ctx, r: &Fake) -> serde_json::Value {
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    let ev = read(&c.state).unwrap();
    serde_json::Value::Object(ev.iter().rfind(|e| e.kind == "drive-pass").unwrap().data.clone())
}

#[test]
fn a_pass_with_ten_due_spawns_launches_ten_kicks_and_starts_no_harness() {
    let c = fleet("kicks-ten", 10, 10);
    // The harness start never returns quickly; the pass must not reach it.
    let r = fake("1.0")
        .reply("herdr agent start", "{}")
        .reply("herdr agent wait", "{}");
    let t0 = std::time::Instant::now();
    let d = pass(&c, &r);
    assert!(t0.elapsed().as_secs() < 5);
    assert!(
        !r.called("herdr agent start") && !r.called("herdr agent wait"),
        "{:?}",
        r.calls.borrow()
    );
    assert!((1..=10).all(|n| launched(&r, n) == 1 && marker(&c, n).exists()));
    assert_eq!(d["spawns_launched"], 10, "{d}");
    assert!(d["secs"].is_number());
}

#[test]
fn kicks_respect_max_parallel_spawns_and_admission() {
    let c = fleet("kicks-max", 3, 5);
    let r = fake("1.0");
    let d = pass(&c, &r);
    assert_eq!(d["spawns_launched"], 3, "{d}");
    assert!((1..=3).all(|n| launched(&r, n) == 1) && (4..=5).all(|n| launched(&r, n) == 0));
    let held = d["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["code"] == "SPAWN_PENDING")
        .count();
    assert_eq!(held, 2, "{d}");
    // Kick #1 re-checks admission: a busy host gives up quietly, no harness start, its marker cleared.
    let busy = fake("40.0");
    let (code, out) = run(&c, &busy, &["kick", "sendmeter", "1", "--why", "drive", "--now"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("HOST_BUSY"), "{out}");
    assert!(!busy.called("herdr agent start") && !marker(&c, 1).exists());
    // Its slot is free again: the next pass launches #1 once more and #4, not #2/#3 still starting.
    let r = fake("1.0");
    pass(&c, &r);
    assert_eq!((launched(&r, 1), launched(&r, 2), launched(&r, 4)), (1, 0, 0));
}

#[test]
fn two_quick_passes_launch_each_spawn_once() {
    let c = fleet("kicks-twice", 6, 2);
    let r = fake("1.0");
    pass(&c, &r);
    pass(&c, &r);
    assert_eq!((launched(&r, 1), launched(&r, 2)), (1, 1), "{:?}", r.calls.borrow());
}

#[test]
fn a_failed_kick_clears_its_marker_and_the_next_pass_retries() {
    let c = fleet("kicks-fail", 6, 1);
    let r = fake("1.0");
    pass(&c, &r);
    assert!(marker(&c, 1).exists());
    // The kick's spawn fails (no herdr worktree): the marker goes with the kick.
    let broken = Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.0 1.0 1.0 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n");
    let _ = run(&c, &broken, &["kick", "sendmeter", "1", "--why", "drive", "--now"]);
    assert!(!broken.called("herdr agent start") && !marker(&c, 1).exists());
    let r = fake("1.0");
    pass(&c, &r);
    assert_eq!(launched(&r, 1), 1);
}

#[test]
fn a_stale_marker_does_not_hold_the_issue_forever() {
    let c = fleet("kicks-stale", 6, 1);
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let f = std::fs::File::create(marker(&c, 1)).unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    f.set_modified(old).unwrap();
    let r = fake("1.0");
    pass(&c, &r);
    assert_eq!(launched(&r, 1), 1);
}

/// The pass counts the spawns its kicks are still starting: one `serial_max = 1` slot, one `max_impl = 1` lane.
#[test]
fn a_pass_counts_starting_spawns_against_serial_and_cap() {
    let c = fleet_with("kicks-serial", 6, r#"serial_paths = ["db/**"]"#, 2, &["--serial"]);
    let r = fake("1.0");
    let d = pass(&c, &r);
    assert_eq!((launched(&r, 1), launched(&r, 2)), (1, 0), "{d}");
    assert_eq!(codes(&d)[1], "SERIAL_BUSY", "{d}");
    // Kick #2 on its own (a lane hook) is refused too while #1 starts.
    let hook = fake("1.0");
    let (code, out) = run(&c, &hook, &["kick", "sendmeter", "2", "--why", "stop", "--now"]);
    assert_eq!(code, 0, "{out}");
    assert!(!hook.called("herdr agent start"), "{out}");

    let c = fleet_with("kicks-cap", 6, "max_impl = 1", 3, &[]);
    let r = fake("1.0");
    let d = pass(&c, &r);
    assert_eq!(d["spawns_launched"], 1, "{d}");
    assert_eq!(codes(&d), ["", "CAP"], "CAP ends the fleet's starts: {d}");
}

/// #304's lane-hook kick on an issue a pass launched leaves that kick's marker alone.
#[test]
fn a_hook_kick_never_clears_a_launched_kicks_marker() {
    let c = fleet("kicks-hook", 6, 1);
    pass(&c, &fake("1.0"));
    let (code, out) = run(&c, &fake("1.0"), &["kick", "sendmeter", "1", "--why", "stop", "--now"]);
    assert_eq!(code, 0, "{out}");
    assert!(marker(&c, 1).exists());
    let r = fake("1.0");
    pass(&c, &r);
    assert_eq!(launched(&r, 1), 0);
}
