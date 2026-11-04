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

/// #333: a marker for `n` holding `pid`, last written `age` secs ago.
fn mark(c: &herdsman::Ctx, n: u64, pid: u32, age: u64) {
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let e = serde_json::json!({"ts": 1, "fleet": "sendmeter", "issue": n, "type": "spawn", "role": "impl", "pid": pid});
    std::fs::write(marker(c, n), e.to_string()).unwrap();
    let f = std::fs::File::options().append(true).open(marker(c, n)).unwrap();
    f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(age))
        .unwrap();
}

fn stale(c: &herdsman::Ctx) -> Vec<herdsman::events::Event> {
    read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "stale-marker")
        .collect()
}

#[test]
fn a_marker_carries_its_kicks_pid_and_goes_when_that_pid_dies() {
    let c = fleet("kicks-pid", 6, 1);
    pass(&c, &fake("1.0").reply("sh -c nohup", "4242\n"));
    let m: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(marker(&c, 1)).unwrap()).unwrap();
    assert_eq!(m["pid"], 4242, "{m}");
    // The kick runs: the issue is held back, no second launch.
    let r = fake("1.0")
        .reply("kill -0 4242", "")
        .reply("ps -o etime= -p 4242", "  00:30\n");
    pass(&c, &r);
    assert_eq!(launched(&r, 1), 0);
    assert!(stale(&c).is_empty());
    // Its pid is gone: the marker is removed, logged, not counted; the next pass launches #1 again.
    let r = fake("1.0");
    pass(&c, &r);
    assert_eq!(launched(&r, 1), 1);
    let s = stale(&c);
    assert_eq!(
        (s.len(), s[0].issue, &s[0].data["pid"]),
        (1, Some(1), &serde_json::json!(4242)),
        "{s:?}"
    );
    assert_eq!(s[0].str("file"), Some("spawn-sendmeter-1.pending"));
}

#[test]
fn an_old_or_reused_pid_marker_is_stale() {
    // Live pid, but older than spawn_pending_max_secs (300).
    let c = fleet("kicks-old", 6, 1);
    mark(&c, 1, 4242, 400);
    let r = fake("1.0").reply("kill -0 4242", "");
    pass(&c, &r);
    assert_eq!((launched(&r, 1), stale(&c).len()), (1, 1));
    // Live pid that started after the marker was written: a reused pid.
    let c = fleet("kicks-reused", 6, 1);
    mark(&c, 1, 4242, 100);
    let r = fake("1.0")
        .reply("kill -0 4242", "")
        .reply("ps -o etime= -p 4242", "00:30\n");
    pass(&c, &r);
    assert_eq!((launched(&r, 1), stale(&c).len()), (1, 1));
}

#[test]
fn six_dead_markers_never_hold_a_new_spawn() {
    let c = fleet("kicks-six-dead", 6, 1);
    (11..=16).for_each(|n| mark(&c, n, 4242, 10));
    let r = fake("1.0");
    let d = pass(&c, &r);
    assert_eq!((launched(&r, 1), d["spawns_launched"].as_u64()), (1, Some(1)), "{d}");
    assert_eq!(stale(&c).len(), 6);
    assert!((11..=16).all(|n| !marker(&c, n).exists()));
}

/// A runner whose harness start panics.
struct Panics(Fake);
impl herdsman::runner::Runner for Panics {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<herdsman::runner::Out> {
        assert!(
            !(prog == "herdr" && args.starts_with(&["agent".into(), "start".into()])),
            "boom"
        );
        self.0.run(prog, args)
    }
    fn sleep(&self, _: u64) {}
}

#[test]
fn a_panicking_kick_leaves_no_marker_or_lock() {
    let c = fleet("kicks-panic", 6, 1);
    pass(&c, &fake("1.0"));
    assert!(marker(&c, 1).exists());
    let p = Panics(fake("1.0"));
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(&c, &p, &["kick", "sendmeter", "1", "--why", "drive", "--now"])
    }));
    assert!(res.is_err(), "the kick panicked");
    let lock = c.state.with_file_name("kick-sendmeter-1.lock");
    assert!(!marker(&c, 1).exists() && !lock.exists());
}

#[test]
fn spawn_pending_for_15_minutes_alerts_once() {
    let c = fleet("kicks-held", 6, 1);
    let held = |ts| {
        let a = serde_json::json!([{"issue": 3, "step": "spawn", "outcome": "waiting", "code": "SPAWN_PENDING"}]);
        let e = herdsman::events::Event::new(ts, "sendmeter", None, "drive-pass", serde_json::json!({"actions": a}));
        herdsman::events::append(&c.state, &e).unwrap();
    };
    held(c.now - 600);
    let alerts = || {
        read(&c.state)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == "spawn-held")
            .count()
    };
    assert_eq!(run(&c, &Fake::default(), &["check"]).0, 0);
    assert_eq!(alerts(), 0, "held 10 min");
    // Still held in every pass 7 min later: 17 min in all.
    let later = herdsman::Ctx {
        now: c.now + 420,
        ..c.clone()
    };
    held(c.now + 360);
    let (code, out) = run(&later, &Fake::default(), &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("#3 held back by SPAWN_PENDING for 17m"), "{out}");
    run(&later, &Fake::default(), &["check"]);
    assert_eq!(alerts(), 1, "once per episode");
}

/// #438: a launched spawn its kick then refuses DISK_LOW logs only the refusal, never a `driven … ok`.
#[test]
fn a_kick_refused_disk_low_leaves_no_driven_ok() {
    let c = fleet("kicks-disk-low", 6, 1);
    pass(&c, &fake("1.0"));
    let df = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 239362496 9 2097152 99% /\n";
    let (code, out) = run(
        &c,
        &fake("1.0").reply("df -k /", df),
        &["kick", "sendmeter", "1", "--why", "drive", "--now"],
    );
    assert_eq!(code, 0, "{out}");
    let ev = read(&c.state).unwrap();
    let driven: Vec<_> = ev.iter().filter(|e| e.kind == "driven" && e.issue == Some(1)).collect();
    assert!(
        !driven.is_empty() && driven.iter().all(|e| e.str("code") == Some("DISK_LOW")),
        "{driven:?}"
    );
}
