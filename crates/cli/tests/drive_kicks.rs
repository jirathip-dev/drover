//! #318: a pass decides its spawns and launches each as a detached kick, never waiting on a harness start;
//! `max_parallel_spawns` bounds them, a `spawn-<fleet>-<n>.pending` marker stops a double launch until the kick ends.
mod common;
use common::*;
use drover::events::read;

fn fleet(name: &str, max: usize, issues: u64) -> (drover::Ctx, FakeShed) {
    fleet_with(name, max, "", issues, &[])
}

fn fleet_with(name: &str, max: usize, extra: &str, issues: u64, flags: &[&str]) -> (drover::Ctx, FakeShed) {
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
    let sh = FakeShed::new();
    sh.attach(&c);
    (c, sh)
}

fn started(sh: &FakeShed) -> bool {
    !sh.all("run").is_empty()
}

fn codes(d: &serde_json::Value) -> Vec<String> {
    let a = d["actions"].as_array().unwrap();
    a.iter().map(|a| a["code"].as_str().unwrap_or("").to_string()).collect()
}

/// Spawns succeed; `load` is the 1-min load on 10 cores (≥ 25 is a busy host).
fn fake(load: &str) -> Fake {
    fake_with(load, Fake::default())
}

/// `fake` after `f`'s own replies.
fn fake_with(load: &str, f: Fake) -> Fake {
    f.reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", &format!("{{ {load} {load} {load} }}\n"))
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "")
}

/// #604: the pass's kicks still starting when it ended (the fake's kicks never run; none `spawns_launched`).
fn pending(d: &serde_json::Value) -> u64 {
    assert_eq!(d["spawns_launched"], 0, "{d}");
    let a = d["actions"].as_array().unwrap();
    a.iter().filter(|a| a["outcome"] == "pending").count() as u64
}

fn launched(r: &Fake, n: u64) -> usize {
    r.count(&format!(
        "sh -c nohup '/opt/drover/bin/drover' kick sendmeter {n} --why drive --now"
    ))
}

fn marker(c: &drover::Ctx, n: u64) -> Option<drover::db::Lease> {
    lease_on(c, &format!("spawn-sendmeter-{n}.pending"))
}

fn pass(c: &drover::Ctx, r: &Fake) -> serde_json::Value {
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    let ev = read(&c.state).unwrap();
    serde_json::Value::Object(ev.iter().rfind(|e| e.kind == "drive-pass").unwrap().data.clone())
}

#[test]
fn a_pass_with_ten_due_spawns_launches_ten_kicks_and_starts_no_harness() {
    let (c, sh) = fleet("kicks-ten", 11, 10);
    // The harness start never returns quickly; the pass must not reach it.
    let r = fake("1.0");
    let t0 = std::time::Instant::now();
    let d = pass(&c, &r);
    assert!(t0.elapsed().as_secs() < 5);
    let m = sh.methods();
    assert!(
        !started(&sh) && !m.iter().any(|m| m == "wait" || m == "send"),
        "{m:?} {:?}",
        r.calls.borrow()
    );
    assert!(!r.called("git -C /tmp/does-not-matter/sendmeter worktree add"));
    assert!((1..=10).all(|n| launched(&r, n) == 1 && marker(&c, n).is_some()));
    assert_eq!(pending(&d), 10, "{d}");
    assert!(d["secs"].is_number());
}

#[test]
fn kicks_respect_max_parallel_spawns_and_admission() {
    let (c, sh) = fleet("kicks-max", 3, 5);
    let r = fake("1.0");
    let d = pass(&c, &r);
    // #513: starts leave the last slot to in-flight work.
    assert_eq!(pending(&d), 2, "{d}");
    assert!((1..=2).all(|n| launched(&r, n) == 1) && (3..=5).all(|n| launched(&r, n) == 0));
    let held = d["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["code"] == "SPAWN_PENDING")
        .count();
    assert_eq!(held, 3, "{d}");
    // Kick #1 re-checks admission: a busy host gives up quietly, no harness start, its marker cleared.
    let busy = fake("40.0");
    let (code, out) = run(&c, &busy, &["kick", "sendmeter", "1", "--why", "drive", "--now"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("HOST_BUSY"), "{out}");
    assert!(!started(&sh) && marker(&c, 1).is_none());
    // Its slot is free again: the next pass launches #1 once more, not #2 still starting nor #3 (the reserved slot).
    let r = fake("1.0");
    pass(&c, &r);
    assert_eq!((launched(&r, 1), launched(&r, 2), launched(&r, 3)), (1, 0, 0));
}

#[test]
fn two_quick_passes_launch_each_spawn_once() {
    let (c, _sh) = fleet("kicks-twice", 6, 2);
    let r = fake("1.0");
    pass(&c, &r);
    pass(&c, &r);
    assert_eq!((launched(&r, 1), launched(&r, 2)), (1, 1), "{:?}", r.calls.borrow());
}

#[test]
fn a_failed_kick_clears_its_marker_and_the_next_pass_retries() {
    let (c, sh) = fleet("kicks-fail", 6, 1);
    let r = fake("1.0");
    pass(&c, &r);
    assert!(marker(&c, 1).is_some());
    let broken = Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.0 1.0 1.0 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n");
    let _ = run(&c, &broken, &["kick", "sendmeter", "1", "--why", "drive", "--now"]);
    assert!(!started(&sh) && marker(&c, 1).is_none());
    let r = fake("1.0");
    pass(&c, &r);
    assert_eq!(launched(&r, 1), 1);
}

#[test]
fn a_stale_marker_does_not_hold_the_issue_forever() {
    let (c, _sh) = fleet("kicks-stale", 6, 1);
    mark(&c, 1, 4242, 3600);
    let r = fake("1.0").reply("kill -0 4242", "");
    pass(&c, &r);
    assert_eq!(launched(&r, 1), 1);
}

/// The pass counts the spawns its kicks are still starting: one `serial_max = 1` slot, one `max_impl = 1` lane.
#[test]
fn a_pass_counts_starting_spawns_against_serial_and_cap() {
    let (c, sh) = fleet_with("kicks-serial", 6, r#"serial_paths = ["db/**"]"#, 2, &["--serial"]);
    let r = fake("1.0");
    let d = pass(&c, &r);
    assert_eq!((launched(&r, 1), launched(&r, 2)), (1, 0), "{d}");
    assert_eq!(codes(&d)[1], "SERIAL_BUSY", "{d}");
    // Kick #2 on its own (a lane hook) is refused too while #1 starts.
    let hook = fake("1.0");
    let (code, out) = run(&c, &hook, &["kick", "sendmeter", "2", "--why", "stop", "--now"]);
    assert_eq!(code, 0, "{out}");
    assert!(!started(&sh), "{out}");

    let (c, _sh) = fleet_with("kicks-cap", 6, "max_impl = 1", 3, &[]);
    let r = fake("1.0");
    let d = pass(&c, &r);
    assert_eq!(pending(&d), 1, "{d}");
    assert_eq!(codes(&d), ["", "CAP"], "CAP ends the fleet's starts: {d}");
}

/// #304's lane-hook kick on an issue a pass launched leaves that kick's marker alone.
#[test]
fn a_hook_kick_never_clears_a_launched_kicks_marker() {
    let (c, _sh) = fleet("kicks-hook", 6, 1);
    pass(&c, &fake("1.0"));
    let (code, out) = run(&c, &fake("1.0"), &["kick", "sendmeter", "1", "--why", "stop", "--now"]);
    assert_eq!(code, 0, "{out}");
    assert!(marker(&c, 1).is_some());
    let r = fake("1.0");
    pass(&c, &r);
    assert_eq!(launched(&r, 1), 0);
}

/// #333: a marker for `n` holding `pid`, taken `age` secs ago (#344: a lease for `spawn_pending_max_secs`).
fn mark(c: &drover::Ctx, n: u64, pid: u32, age: u64) {
    let e = serde_json::json!({"ts": 1, "fleet": "sendmeter", "issue": n, "type": "spawn", "role": "impl", "pid": pid});
    let key = format!("spawn-sendmeter-{n}.pending");
    let l = drover::db::take(&c.state, &key, c.now - age, 300, &e.to_string(), &|_| true).unwrap();
    assert!(drover::db::renew(&c.state, &l, &drover::db::Lease { pid, ..l.clone() }));
}

fn stale(c: &drover::Ctx) -> Vec<drover::events::Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == "stale-marker")
        .collect()
}

#[test]
fn a_marker_carries_its_kicks_pid_and_goes_when_that_pid_dies() {
    let (c, _sh) = fleet("kicks-pid", 6, 1);
    pass(&c, &fake("1.0").reply("sh -c nohup", "4242\n"));
    let m: serde_json::Value = serde_json::from_str(&marker(&c, 1).unwrap().data).unwrap();
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
    let (c, _sh) = fleet("kicks-old", 6, 1);
    mark(&c, 1, 4242, 400);
    let r = fake("1.0").reply("kill -0 4242", "");
    pass(&c, &r);
    assert_eq!((launched(&r, 1), stale(&c).len()), (1, 1));
    // Live pid that started after the marker was written: a reused pid.
    let (c, _sh) = fleet("kicks-reused", 6, 1);
    mark(&c, 1, 4242, 100);
    let r = fake("1.0")
        .reply("kill -0 4242", "")
        .reply("ps -o etime= -p 4242", "00:30\n");
    pass(&c, &r);
    assert_eq!((launched(&r, 1), stale(&c).len()), (1, 1));
}

#[test]
fn six_dead_markers_never_hold_a_new_spawn() {
    let (c, _sh) = fleet("kicks-six-dead", 6, 1);
    (11..=16).for_each(|n| mark(&c, n, 4242, 10));
    let r = fake("1.0");
    let d = pass(&c, &r);
    assert_eq!((launched(&r, 1), Some(pending(&d))), (1, Some(1)), "{d}");
    assert_eq!(stale(&c).len(), 6);
    assert!((11..=16).all(|n| marker(&c, n).is_none()));
}

struct Panics(Fake);
impl drover::runner::Runner for Panics {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        assert!(
            !(prog == "git" && args.get(2..4) == Some(&["worktree".into(), "add".into()])),
            "boom"
        );
        self.0.run(prog, args)
    }
    fn sleep(&self, _: u64) {}
}

#[test]
fn a_panicking_kick_leaves_no_marker_or_lock() {
    let (c, _sh) = fleet("kicks-panic", 6, 1);
    pass(&c, &fake("1.0"));
    assert!(marker(&c, 1).is_some());
    let p = Panics(fake("1.0"));
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(&c, &p, &["kick", "sendmeter", "1", "--why", "drive", "--now"])
    }));
    assert!(res.is_err(), "the kick panicked");
    assert!(marker(&c, 1).is_none() && lease_on(&c, "kick-sendmeter-1.lock").is_none());
}

#[test]
fn spawn_pending_for_15_minutes_alerts_once() {
    let (c, _sh) = fleet("kicks-held", 6, 1);
    let held = |ts| {
        let a = serde_json::json!([{"issue": 3, "step": "spawn", "outcome": "waiting", "code": "SPAWN_PENDING"}]);
        let e = drover::events::Event::new(ts, "sendmeter", None, "drive-pass", serde_json::json!({"actions": a}));
        drover::events::append(&c.state, &e).unwrap();
    };
    held(c.now - 600);
    let alerts = || {
        read(&c.state)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "spawn-held")
            .count()
    };
    assert_eq!(run(&c, &Fake::default(), &["check"]).0, 0);
    assert_eq!(alerts(), 0, "held 10 min");
    // Still held in every pass 7 min later: 17 min in all.
    let later = drover::Ctx {
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
    let (c, _sh) = fleet("kicks-disk-low", 6, 1);
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

/// #416: a spawn still starting toward a full host (bazzite under `min_free_gb`) holds no `max_parallel_spawns` slot:
/// the one bound elsewhere launches, and the full host's own is refused `DISK_LOW` without a kick.
#[test]
fn a_full_hosts_pending_spawn_holds_no_slot_for_another_host() {
    let hosts = "\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n";
    let (c, _sh) = fleet_with("kicks-full-host", 1, hosts, 2, &[]);
    let d = serde_json::json!({"role": "impl", "agent": "sendmeter-impl-1", "pane": "sendmeter-impl-1", "host": "bazzite",
        "machine": "bazzite", "worktree": "/home/me/worktrees/sendmeter/issue-1", "branch": "issue-1"});
    drover::events::append(
        &c.state,
        &drover::events::Event::new(1, "sendmeter", Some(1), "spawn", d),
    )
    .unwrap();
    mark(&c, 1, 4242, 0);
    let ssh = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C -oControlPersist=10m bazzite -- ";
    let df = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/sda 9 8 1048576 99% /\n";
    let r = fake("1.0")
        .reply(&format!("{ssh}'df' '-k' '/'"), df)
        .reply("kill -0 4242", "");
    let d = pass(&c, &r);
    assert!(!codes(&d).contains(&"SPAWN_PENDING".to_string()), "{d}");
    assert_eq!((launched(&r, 1), launched(&r, 2)), (0, 1), "{d}");
}

/// #513: issue `n` (not queued) built, pushed and FAILed: its fix round is due.
fn fix_due(c: &drover::Ctx, n: u64) {
    let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
    std::fs::create_dir_all(&wt).unwrap();
    let d = serde_json::json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "pane": format!("sendmeter-impl-{n}"),
        "worktree": wt, "branch": format!("issue-{n}")});
    let at = |kind, d| drover::events::Event::new(10, "sendmeter", Some(n), kind, d);
    drover::events::append(&c.state, &at("spawn", d)).unwrap();
    let v = serde_json::json!({"verdict": "FAIL", "sha": "a".repeat(40), "body": "FAIL\nsrc/x.rs:3 off by one"});
    drover::events::append(&c.state, &at("verdict", v)).unwrap();
}

/// #513: in-flight fix rounds launch before any start, and starts leave the last slot free.
#[test]
fn fix_rounds_launch_before_starts_and_starts_keep_one_slot_free() {
    let (c, _sh) = fleet("kicks-order", 6, 10);
    (21..=22).for_each(|n| fix_due(&c, n));
    let r = fake("1.0");
    let d = pass(&c, &r);
    let at = |n: u64| r.index(&format!("sh -c nohup '/opt/drover/bin/drover' kick sendmeter {n} "));
    let (fixes, starts) = ([at(21), at(22)], (1..=10).filter_map(at).collect::<Vec<_>>());
    assert!(fixes.iter().all(Option::is_some), "{d}");
    assert!(starts.iter().all(|s| fixes.iter().all(|f| f < &Some(*s))), "{d}");
    assert_eq!((starts.len(), Some(pending(&d))), (3, Some(5)), "{d}");
}

/// #513: five starts still starting: a due fix round takes the reserved sixth slot; a start does not.
#[test]
fn a_due_fix_round_takes_the_reserved_slot() {
    let (c, _sh) = fleet("kicks-reserved", 6, 1);
    (11..=15).for_each(|n| mark(&c, n, 4242, 10));
    fix_due(&c, 21);
    let r = fake("1.0")
        .reply("kill -0 4242", "")
        .reply("ps -o etime= -p 4242", "  00:30\n");
    let d = pass(&c, &r);
    assert_eq!((launched(&r, 21), launched(&r, 1)), (1, 0), "{d}");
    assert!(codes(&d).contains(&"SPAWN_PENDING".to_string()), "{d}");
}

/// #513: one slot, nothing in flight: a start still launches.
#[test]
fn one_slot_and_nothing_in_flight_still_starts() {
    let (c, _sh) = fleet("kicks-one", 1, 2);
    let r = fake("1.0");
    let d = pass(&c, &r);
    assert_eq!((launched(&r, 1), launched(&r, 2)), (1, 0), "{d}");
}

/// #560: issue `n` (not queued) built, pushed and its PR open: its review is due. Its worktree's head for `fake_with`.
fn review_due(c: &drover::Ctx, n: u64, f: Fake) -> Fake {
    let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
    std::fs::create_dir_all(&wt).unwrap();
    let d = serde_json::json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "pane": format!("sendmeter-impl-{n}"),
        "worktree": wt, "branch": format!("issue-{n}")});
    let at = |kind, d| drover::events::Event::new(10, "sendmeter", Some(n), kind, d);
    drover::events::append(&c.state, &at("spawn", d)).unwrap();
    let head = "b".repeat(40);
    drover::events::append(&c.state, &at("pushed", serde_json::json!({"sha": head}))).unwrap();
    let cache = c.state.parent().unwrap().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let pr = serde_json::json!({"number": 100 + n, "state": "OPEN", "headRefOid": head, "statusCheckRollup": []});
    let pr = serde_json::json!({"ts": c.now, "branch": format!("issue-{n}"), "pr": pr});
    std::fs::write(cache.join(format!("pr-sendmeter-{n}.json")), pr.to_string()).unwrap();
    let git = format!("git -C {}", wt.display());
    f.reply(&format!("{git} status"), "")
        .reply(&format!("{git} rev-list"), "0\n")
        .reply(&format!("{git} rev-parse"), &format!("{head}\n"))
        .reply(&format!("{git} log"), &format!("{head} {}", c.now - 600))
}

/// #560: room for one spawn, a fix round and a pushed head both due and a queued issue: the review spawns.
#[test]
fn room_for_one_spawn_goes_to_the_review() {
    let (c, _sh) = fleet("kicks-review-first", 1, 1);
    fix_due(&c, 21);
    let r = fake_with("1.0", review_due(&c, 22, Fake::default()));
    let d = pass(&c, &r);
    assert_eq!((launched(&r, 22), launched(&r, 21), launched(&r, 1)), (1, 0, 0), "{d}");
}

/// #560: a busy host launches nothing: the review waits `HOST_BUSY` and no start's kick takes the room after it.
#[test]
fn a_busy_host_holds_every_spawn_behind_the_review() {
    let (c, _sh) = fleet("kicks-busy", 6, 2);
    let f = review_due(&c, 22, Fake::default());
    let r = fake_with("40.0", f);
    let d = pass(&c, &r);
    assert_eq!((launched(&r, 22), launched(&r, 1), launched(&r, 2)), (0, 0, 0), "{d}");
    let a = d["actions"].as_array().unwrap();
    assert!(a.iter().any(|a| a["issue"] == 22 && a["code"] == "HOST_BUSY"), "{d}");
    assert!(a.iter().all(|a| a["step"] != "start"), "no start tried: {d}");
    // The room back: the review launches first.
    let r = fake_with("1.0", review_due(&c, 22, Fake::default()));
    pass(&c, &r);
    let at = |n: u64| r.index(&format!("sh -c nohup '/opt/drover/bin/drover' kick sendmeter {n} "));
    assert!(at(22).is_some() && at(22) < at(1), "{:?}", r.calls.borrow());
}

/// #560: a review held for room says how many reviews drive tries before it.
#[test]
fn waiting_names_the_reviews_ahead() {
    let (c, _sh) = fleet("kicks-review-order", 1, 1);
    let f = (21..=23).fold(Fake::default(), |f, n| review_due(&c, n, f));
    let r = fake_with("1.0", f);
    pass(&c, &r);
    assert_eq!(launched(&r, 21), 1);
    let (code, out) = run(&c, &r, &["waiting", "sendmeter", "--json"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(r#""what": "review waits behind 1 earlier review","#),
        "{out}"
    );
    assert!(out.contains("review waits behind 2 earlier reviews"), "{out}");
}

/// #604: a runner whose detached kick runs to its end at once (its own thread, on a host at `load`).
struct Inline(Fake, drover::Ctx, &'static str);
impl drover::runner::Runner for Inline {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        self.0.run(prog, args)
    }
    fn sleep(&self, _: u64) {}
    fn detach(&self, _: &str, args: &[String], _: &std::path::Path) -> anyhow::Result<u32> {
        let (c, a, load) = (self.1.clone(), args.to_vec(), self.2);
        std::thread::spawn(move || {
            let a: Vec<&str> = a.iter().map(String::as_str).collect();
            run(&c, &fake(load), &a)
        })
        .join()
        .unwrap();
        Ok(0)
    }
}

fn act(d: &serde_json::Value, n: u64) -> serde_json::Value {
    d["actions"]
        .as_array()
        .unwrap()
        .iter()
        .rfind(|a| a["issue"] == n)
        .cloned()
        .unwrap()
}

/// #604: a kick refused HOST_BUSY inside its pass is logged as that refusal, never `launched`/`started`.
#[test]
fn a_kick_refused_host_busy_is_logged_refused_not_launched() {
    let (c, _sh) = fleet("kicks-604-busy", 6, 1);
    let r = Inline(fake("1.0"), c.clone(), "40.0");
    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("sendmeter #1 start: REFUSED HOST_BUSY: "), "{out}");
    assert!(!out.contains("launched") && !out.contains("pending"), "{out}");
    let ev = read(&c.state).unwrap();
    let d = serde_json::Value::Object(ev.iter().rfind(|e| e.kind == "drive-pass").unwrap().data.clone());
    let a = act(&d, 1);
    assert_eq!(
        (&a["outcome"], &a["code"]),
        (&serde_json::json!("waiting"), &serde_json::json!("HOST_BUSY")),
        "{d}"
    );
    assert_eq!(d["spawns_launched"], 0, "{d}");
    let e = ev.iter().rfind(|e| e.kind == "drive-pass").unwrap();
    assert!(!drover::drive::result(e).1.contains("started 1"), "{d}");
    // `waiting` names the refusal for the queued issue.
    let (code, out) = run(&c, &fake("1.0"), &["waiting", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("HOST_BUSY"), "{out}");
}

/// #604: a kick still running when its pass ends is logged `pending`; its refusal then updates that action.
#[test]
fn a_kick_running_past_its_pass_is_pending_then_updated() {
    let (c, _sh) = fleet("kicks-604-pending", 6, 1);
    let (code, out) = run(&c, &fake("1.0"), &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("sendmeter #1 start: pending") && !out.contains("launched"),
        "{out}"
    );
    let last = || {
        let ev = read(&c.state).unwrap();
        let e = ev
            .iter()
            .rfind(|e| e.kind == "drive-pass" && e.data.get("kick").is_none_or(|k| k.is_null()));
        serde_json::Value::Object(e.unwrap().data.clone())
    };
    let d = last();
    assert_eq!(
        (&act(&d, 1)["outcome"], &d["spawns_launched"]),
        (&serde_json::json!("pending"), &serde_json::json!(0)),
        "{d}"
    );
    let (code, out) = run(
        &c,
        &fake("40.0"),
        &["kick", "sendmeter", "1", "--why", "drive", "--now"],
    );
    assert_eq!(code, 0, "{out}");
    let d = last();
    let a = act(&d, 1);
    assert_eq!(
        (&a["outcome"], &a["code"]),
        (&serde_json::json!("waiting"), &serde_json::json!("HOST_BUSY")),
        "{d}"
    );
    assert_eq!(d["spawns_launched"], 0, "{d}");
}

/// #604: every start refused by the same host gate for 30 min alerts `host-busy <host>: <why>` once per episode.
#[test]
fn every_start_refused_by_one_host_gate_for_30_min_alerts_once() {
    let (c, _sh) = fleet("kicks-604-alert", 6, 1);
    let pass = |ts| {
        let a = serde_json::json!([{"issue": 3, "step": "start", "outcome": "waiting", "code": "HOST_BUSY",
            "why": "mini: memory pressure warn; retry later or pass --force"}]);
        let e = drover::events::Event::new(ts, "sendmeter", None, "drive-pass", serde_json::json!({"actions": a}));
        drover::events::append(&c.state, &e).unwrap();
    };
    let alerts = || read(&c.state).unwrap().iter().filter(|e| e.kind == "host-busy").count();
    pass(c.now - 1200);
    pass(c.now - 60);
    assert_eq!(run(&c, &Fake::default(), &["check"]).0, 0);
    assert_eq!(alerts(), 0, "busy 20 min");
    let later = drover::Ctx {
        now: c.now + 600,
        ..c.clone()
    };
    pass(c.now + 540);
    let (code, out) = run(&later, &Fake::default(), &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("host-busy mini: memory pressure warn"), "{out}");
    run(&later, &Fake::default(), &["check"]);
    assert_eq!(alerts(), 1, "once per episode");
}

/// #604: a kick that logs its `spawn` inside its pass counts as launched and reads `started`.
#[test]
fn a_kick_that_spawns_inside_its_pass_counts_as_launched() {
    let (c, _sh) = fleet("kicks-604-ok", 6, 1);
    let (code, out) = run(&c, &Inline(fake("1.0"), c.clone(), "1.0"), &["drive"]);
    assert_eq!(code, 0, "{out}");
    let ev = read(&c.state).unwrap();
    assert!(ev.iter().any(|e| e.is("spawn", "sendmeter", 1)), "{out}");
    let d = serde_json::Value::Object(ev.iter().rfind(|e| e.kind == "drive-pass").unwrap().data.clone());
    assert_eq!(
        (&act(&d, 1)["outcome"], &d["spawns_launched"]),
        (&serde_json::json!("started"), &serde_json::json!(1)),
        "{d}"
    );
    assert!(out.contains("sendmeter #1 start: started"), "{out}");
}

/// #638: fleets `names`, each with `max_parallel_spawns = max` and issues 1..=`issues` queued.
fn fleets(dir: &str, names: &[&str], max: usize, issues: u64) -> (drover::Ctx, FakeShed) {
    let (fleet, limits) = CONFIG.split_once("[limits]").unwrap();
    let all: String = names.iter().map(|n| fleet.replace("sendmeter", n)).collect();
    let limits = limits.replace("max_parallel_spawns = 0", &format!("max_parallel_spawns = {max}"));
    let c = ctx(&tmpdir(dir), &format!("{all}[limits]{limits}"));
    for n in names {
        let mut add = vec!["queue".to_string(), n.to_string(), "add".into()];
        add.extend((1..=issues).map(|i| i.to_string()));
        let add: Vec<&str> = add.iter().map(String::as_str).collect();
        assert_eq!(run(&c, &Fake::default(), &add).0, 0);
    }
    let sh = FakeShed::new();
    sh.attach(&c);
    (c, sh)
}

/// #638: the fleets a pass's kicks went to, in launch order.
fn kicked_fleets(r: &Fake) -> Vec<String> {
    let calls = r.calls.borrow();
    let kick = |c: &String| {
        Some(
            c.strip_prefix("sh -c nohup '/opt/drover/bin/drover' kick ")?
                .split(' ')
                .next()?
                .to_string(),
        )
    };
    calls.iter().filter_map(kick).collect()
}

/// #638: three fleets with five ready issues each and six start slots: each pass starts two per fleet, one fleet
/// at a time, and the fleet that goes first rotates pass by pass.
#[test]
fn starts_share_slots_round_robin_across_fleets() {
    let names = ["alpha", "beta", "gamma"];
    // #513: starts leave the last slot to in-flight work: seven slots are six for starts.
    let (c, _sh) = fleets("kicks-638-fair", &names, 7, 5);
    let mut firsts = vec![];
    for _ in 0..3 {
        let r = fake("1.0");
        let (code, out) = run(&c, &r, &["drive"]);
        assert_eq!(code, 0, "{out}");
        let k = kicked_fleets(&r);
        for n in names {
            assert_eq!(k.iter().filter(|f| *f == n).count(), 2, "{n}: {k:?}");
        }
        assert_eq!(
            k[..3].iter().collect::<std::collections::BTreeSet<_>>().len(),
            3,
            "one each first: {k:?}"
        );
        firsts.push(k[0].clone());
        // Every kick ends before the next pass: its slot is free again.
        for l in drover::db::leases(&c.state, "spawn-") {
            drover::db::release(&c.state, &l.key, None);
        }
    }
    assert_eq!(firsts, ["alpha", "beta", "gamma"]);
}

struct Watch(Fake, drover::Ctx, Shared<Vec<bool>>);
impl drover::runner::Runner for Watch {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        if prog == "df" {
            self.2.borrow_mut().push(marker(&self.1, 1).is_some());
        }
        self.0.run(prog, args)
    }
    fn sleep(&self, _: u64) {}
}

/// #638: a kick refused HOST_BUSY gives its slot back at the refusal, not when its whole pass ends.
#[test]
fn a_kick_refused_host_busy_releases_its_slot_in_the_same_pass() {
    let (c, _sh) = fleet("kicks-638-busy", 6, 1);
    pass(&c, &fake("1.0"));
    assert!(marker(&c, 1).is_some());
    let r = Watch(fake("40.0"), c.clone(), Shared::default());
    let (code, out) = run(&c, &r, &["kick", "sendmeter", "1", "--why", "drive", "--now"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("HOST_BUSY"), "{out}");
    let seen = r.2.borrow().clone();
    // Held while the kick starts; free at the look the pass takes after the refusal.
    assert!(seen.first() == Some(&true) && seen.last() == Some(&false), "{seen:?}");
}

/// #638: `waiting` names who holds the spawn slots.
#[test]
fn waiting_names_the_slot_holders() {
    let (c, _sh) = fleets("kicks-638-slots", &["alpha", "beta"], 4, 2);
    pass(&c, &fake("1.0"));
    let (code, out) = run(&c, &fake("1.0"), &["waiting"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("slots: alpha 2, beta 1\n"), "{out}");
}

fn broken(f: Fake) -> Fake {
    let mut f = fake_with("1.0", f);
    let used = drover::runner::Out {
        code: 1,
        stderr: "fatal: 'issue-1' is already used by worktree at '/w/old/worktrees/x'".into(),
        ..Default::default()
    };
    f.replies
        .insert(0, ("git -C /tmp/does-not-matter/sendmeter worktree add".into(), used));
    f
}

/// #724: `fleet` #`n`'s launched kick runs on `r` and its spawn fails, as a detached kick would.
fn fail_kick(c: &drover::Ctx, r: &Fake, fleet: &str, n: u64) {
    let (code, out) = run(c, r, &["kick", fleet, &n.to_string(), "--why", "drive", "--now"]);
    assert_eq!(code, 0, "{out}");
}

/// #724: six due reviews whose spawn always fails the same way never starve another fleet's healthy start: it
/// spawns within two passes (each failing kick runs, fails and frees its slot between passes).
#[test]
fn failing_retries_never_starve_another_fleets_start() {
    let (c, _sh) = fleets("kicks-724-starve", &["sendmeter", "beta"], 6, 1);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "rm", "1"]).0, 0);
    let due = || (21..=26).fold(Fake::default(), |f, n| review_due(&c, n, f));
    let mut beta = 0;
    for _ in 0..2 {
        let r = fake_with("1.0", due());
        let (code, out) = run(&c, &r, &["drive"]);
        assert_eq!(code, 0, "{out}");
        beta += kicked_fleets(&r).iter().filter(|f| *f == "beta").count();
        for n in (21..=26).filter(|n| lease_on(&c, &format!("spawn-sendmeter-{n}.pending")).is_some()) {
            fail_kick(&c, &broken(due()), "sendmeter", n);
        }
    }
    assert_eq!(beta, 1, "the healthy start spawned");
}

/// #724: a failing issue backs off once it fails the same way twice (no attempt within 5 min of its last failure,
/// its `waiting` line says why); past the wait it is retried; an owner's kick retries it at once.
#[test]
fn a_failing_spawn_backs_off_and_a_kick_retries_it_at_once() {
    let (c, sh) = fleet("kicks-724-backoff", 6, 1);
    pass(&c, &fake("1.0"));
    let bad = broken(Fake::default());
    fail_kick(&c, &bad, "sendmeter", 1);
    // A first failure is retried at once (#333); the same failure again backs off.
    let r = fake("1.0");
    pass(&c, &r);
    assert_eq!(launched(&r, 1), 1, "a first failure retries at once");
    fail_kick(&c, &bad, "sendmeter", 1);
    let at = |s: u64| drover::Ctx {
        now: c.now + s,
        ..c.clone()
    };
    for s in [0, 60, 299] {
        let r = fake("1.0");
        pass(&at(s), &r);
        assert_eq!(launched(&r, 1), 0, "an attempt {s}s after the failure");
    }
    let (code, out) = run(&at(120), &fake("1.0"), &["waiting", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("retrying in 3m: "), "{out}");
    let r = fake("1.0");
    pass(&at(301), &r);
    assert_eq!(launched(&r, 1), 1, "retried past the wait");
    fail_kick(&at(302), &bad, "sendmeter", 1);
    // Backing off again; the owner's kick tries the spawn at once.
    let r = fake("1.0");
    pass(&at(310), &r);
    assert_eq!(launched(&r, 1), 0);
    let r = fake("1.0");
    let (code, out) = run(&at(311), &r, &["kick", "sendmeter", "1", "--why", "owner", "--now"]);
    assert_eq!(code, 0, "{out}");
    assert!(started(&sh), "{out}");
}

/// #724: one fleet with six queued starts and another with one: the second gets a slot in the first pass.
#[test]
fn a_fleet_with_one_start_gets_a_slot_beside_one_with_six() {
    let (c, _sh) = fleets("kicks-724-fair", &["alpha", "beta"], 6, 1);
    let add = ["queue", "alpha", "add", "2", "3", "4", "5", "6"];
    assert_eq!(run(&c, &Fake::default(), &add).0, 0);
    let r = fake("1.0");
    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    let k = kicked_fleets(&r);
    assert_eq!(k.iter().filter(|f| *f == "beta").count(), 1, "{k:?}");
}

/// #724: three identical failures alert `spawn-failing` once; a fourth alerts nothing new.
#[test]
fn three_identical_spawn_failures_alert_once() {
    let (c, _sh) = fleet("kicks-724-alert", 6, 1);
    let alerts = || {
        let ev = read(&c.state).unwrap().to_vec();
        ev.into_iter().filter(|e| e.kind == "spawn-failing").collect::<Vec<_>>()
    };
    for i in 0..2 {
        fail_kick(&c, &broken(Fake::default()), "sendmeter", 1);
        assert_eq!(run(&c, &Fake::default(), &["check"]).0, 0);
        assert!(alerts().is_empty(), "after {} failures", i + 1);
    }
    fail_kick(&c, &broken(Fake::default()), "sendmeter", 1);
    let (code, out) = run(&c, &Fake::default(), &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("spawn-failing sendmeter#1: ") && out.contains("already used by worktree"),
        "{out}"
    );
    assert_eq!(alerts().len(), 1);
    fail_kick(&c, &broken(Fake::default()), "sendmeter", 1);
    assert_eq!(run(&c, &Fake::default(), &["check"]).0, 0);
    assert_eq!(alerts().len(), 1, "a fourth identical failure");
}

/// #724: passes run and a queued issue waits only for a spawn slot: `waiting` says so (naming the failing retries),
/// never `tick stuck?`.
#[test]
fn waiting_names_a_slot_wait_not_a_stuck_tick() {
    let (c, _sh) = fleet("kicks-724-stuck", 6, 1);
    std::fs::write(
        c.state.with_file_name("drive-sendmeter.last"),
        (c.now - 1200).to_string(),
    )
    .unwrap();
    std::fs::write(c.state.with_file_name("drive.at"), (c.now - 60).to_string()).unwrap();
    fail_kick(
        &drover::Ctx {
            now: c.now - 100,
            ..c.clone()
        },
        &broken(Fake::default()),
        "sendmeter",
        1,
    );
    let a = serde_json::json!([{"issue": 1, "step": "start", "outcome": "waiting", "code": "SPAWN_PENDING"}]);
    let e = drover::events::Event::new(
        c.now - 60,
        "sendmeter",
        None,
        "drive-pass",
        serde_json::json!({"actions": a}),
    );
    drover::events::append(&c.state, &e).unwrap();
    let (code, out) = run(&c, &fake("1.0"), &["waiting", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("→ waiting for a spawn slot (1 failing retries hold them)"),
        "{out}"
    );
    assert!(!out.contains("tick stuck?"), "{out}");
}

fn dead_six_live_one(name: &str) -> (drover::Ctx, Fake, FakeShed) {
    let (c, sh) = fleet(name, 6, 1);
    (11..=16).for_each(|n| mark(&c, n, 4242, 10));
    mark(&c, 17, 4343, 30);
    let r = fake("1.0")
        .reply("kill -0 4343", "")
        .reply("ps -o etime= -p 4343", "  00:40\n");
    (c, r, sh)
}

#[test]
fn a_dead_kicks_marker_counts_nowhere_and_the_hold_names_the_live_one() {
    let (c, r, _sh) = dead_six_live_one("kicks-812-dead");
    let (code, out) = run(&c, &r, &["waiting"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("slots: sendmeter 1\n"), "{out}");
    let (c, r, _sh) = dead_six_live_one("kicks-812-held");
    let held = serde_json::json!([{"issue": 3, "step": "spawn", "outcome": "waiting", "code": "SPAWN_PENDING"}]);
    for ts in [c.now - 1200, c.now - 60] {
        let d = serde_json::json!({"actions": held});
        drover::events::append(
            &c.state,
            &drover::events::Event::new(ts, "sendmeter", None, "drive-pass", d),
        )
        .unwrap();
    }
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    let ev = read(&c.state).unwrap();
    let alert = ev.iter().rfind(|e| e.kind == "spawn-held").expect("spawn-held");
    let why = alert.str("reason").unwrap_or_default();
    assert!(why.contains("spawn-sendmeter-17.pending 30s"), "{why}");
    assert!(!why.contains("spawn-sendmeter-11.pending"), "{why}");
    assert_eq!(stale(&c).len(), 6);
}

#[test]
fn waiting_names_the_marker_that_holds_the_slot() {
    let (c, r, _sh) = dead_six_live_one("kicks-812-waiting");
    fix_due(&c, 21);
    let d = serde_json::json!({"step": "fix", "cmd": "drover spawn sendmeter 21 impl --supersede",
        "result": "REFUSED SPAWN_PENDING: 6 spawns starting", "code": "SPAWN_PENDING"});
    drover::events::append(
        &c.state,
        &drover::events::Event::new(c.now - 30, "sendmeter", Some(21), "driven", d),
    )
    .unwrap();
    let (code, out) = run(&c, &r, &["waiting", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("waiting for a spawn slot: spawn-sendmeter-17.pending 30s"),
        "{out}"
    );
}

#[test]
fn a_manual_spawn_and_drives_launch_agree_on_the_slots() {
    let (c, r, _sh) = dead_six_live_one("kicks-812-manual");
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "1", "impl"]);
    assert_eq!(code, 0, "{out}");
    let (c, r, _sh) = dead_six_live_one("kicks-812-drive");
    let d = pass(&c, &r);
    assert_eq!(launched(&r, 1), 1, "{d}");
    assert!(!codes(&d).contains(&"SPAWN_PENDING".to_string()), "{d}");
}
