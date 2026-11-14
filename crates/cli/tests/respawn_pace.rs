//! #908: drive's respawns go through admission (no `--force`) and at most `respawn_per_pass` of them run a pass.
mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::json;

const T: u64 = 1_000_000;

fn ev(c: &drover::Ctx, n: u64, ts: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(n), kind, d)).unwrap();
}

fn sha(p: &str) -> String {
    format!("{p}{}", "0".repeat(40 - p.len()))
}

/// Issues 1..=`k` each in a fix round whose impl lane was killed: drive owes each a respawn.
fn killed(c: &drover::Ctx, k: u64) {
    for n in 1..=k {
        let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
        std::fs::create_dir_all(&wt).unwrap();
        let lane = |role: &str, agent: String| {
            json!({"role": role, "agent": agent, "pane": agent, "workspace": "", "runtime": "shed",
                "worktree": wt, "branch": format!("issue-{n}"), "confirmed": true})
        };
        ev(c, n, T - 900, "spawn", lane("impl", format!("sendmeter-impl-{n}")));
        ev(c, n, T - 890, "spawn", lane("rev", format!("sendmeter-rev-{n}-r1")));
        let v = json!({"verdict": "FAIL", "sha": sha(&n.to_string()), "body": "FAIL"});
        ev(c, n, T - 880, "verdict", v);
        ev(c, n, T - 870, "spawn", lane("impl", format!("sendmeter-impl-{n}")));
        let d = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}")});
        ev(c, n, T - 860, "lane-killed", d);
    }
}

fn host(load: &str) -> Fake {
    no_pr(load).reply("gh pr view", &pr().to_string())
}

fn pr() -> serde_json::Value {
    json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": sha("1"),
        "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "IN_PROGRESS"}]})
}

fn no_pr(load: &str) -> Fake {
    Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", &format!("{{ {load} 1.20 1.00 }}\n"))
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "")
        .reply("which shed", "/opt/shed/bin/shed\n")
}

fn spawns(c: &drover::Ctx) -> Vec<u64> {
    let all = read(&c.state).unwrap();
    let late = all
        .iter()
        .filter(|e| e.kind == "spawn" && e.ts >= T && e.str("role") == Some("impl"));
    late.filter_map(|e| e.issue).collect()
}

/// One `drive` pass at `now`: the issues whose impl lane it respawned (a respawned lane may then go to review).
fn pass(c: &drover::Ctx, r: &Fake, now: u64) -> Vec<u64> {
    let before = spawns(c).len();
    let (code, out) = run(&drover::Ctx { now, ..c.clone() }, r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    spawns(c).split_off(before)
}

fn no_force(c: &drover::Ctx) {
    let all = read(&c.state).unwrap();
    for e in all.iter().filter(|e| e.kind == "driven") {
        assert!(!e.str("cmd").unwrap_or_default().contains("--force"), "{e:?}");
    }
    assert!(all.iter().all(|e| e.kind != "override"), "drive overrode admission");
}

#[test]
fn fifteen_gone_lanes_wait_out_a_busy_host_then_come_back_paced_by_respawn_per_pass() {
    let c = ctx(&tmpdir("pace-15"), CONFIG);
    let _sh = FakeShed::attached(&c);
    killed(&c, 15);
    let busy = host("50.00");
    for k in 0..3 {
        assert_eq!(
            pass(&c, &busy, T + k * 60),
            Vec::<u64>::new(),
            "no respawn over the limit"
        );
    }
    no_force(&c);
    let calm = host("1.50");
    let mut back = vec![];
    for k in 3..20 {
        let got = pass(&c, &calm, T + k * 60);
        assert!(got.len() <= 2, "at most respawn_per_pass (2) a pass: {got:?}");
        assert_eq!(got.len(), 2.min(15 - back.len()), "{got:?}");
        back.extend(got);
        if back.len() == 15 {
            break;
        }
    }
    back.sort_unstable();
    assert_eq!(back, (1..=15).collect::<Vec<_>>(), "every lane comes back once");
    no_force(&c);
}

#[test]
fn respawn_per_pass_is_read_from_limits_and_a_fleet_overrides_it() {
    let limits = CONFIG.replace("[limits]\n", "[limits]\nrespawn_per_pass = 4\n");
    let c = ctx(&tmpdir("pace-limits"), &limits);
    let _sh = FakeShed::attached(&c);
    killed(&c, 6);
    assert_eq!(pass(&c, &host("1.50"), T).len(), 4);
    let fleet = limits.replace(
        "integration = \"staging\"",
        "integration = \"staging\"\nrespawn_per_pass = 3",
    );
    let c = ctx(&tmpdir("pace-fleet"), &fleet);
    let _sh = FakeShed::attached(&c);
    killed(&c, 6);
    assert_eq!(pass(&c, &host("1.50"), T).len(), 3);
}

/// #666's stall supersede (a lane that never started): under HOST_BUSY it waits, then respawns once there is room.
#[test]
fn a_failed_start_waits_for_room_and_is_never_forced() {
    let c = ctx(&tmpdir("pace-stall"), CONFIG);
    let sh = FakeShed::attached(&c);
    let wt = c.worktrees.join("sendmeter/issue-1");
    std::fs::create_dir_all(&wt).unwrap();
    let d = json!({"role": "impl", "agent": "sendmeter-impl-1", "pane": "sendmeter-impl-1", "workspace": "",
        "runtime": "shed", "worktree": wt, "branch": "issue-1", "confirmed": true});
    ev(&c, 1, T - 3600, "spawn", d);
    sh.lane("sendmeter-impl-1", "idle", &wt.to_string_lossy());
    sh.screen("sendmeter-impl-1", "\n  \n");
    assert_eq!(pass(&c, &no_pr("50.00"), T), Vec::<u64>::new());
    let held = read(&c.state).unwrap();
    let d = held.iter().rfind(|e| e.kind == "driven").expect("a driven step");
    assert_eq!(
        (d.str("step"), d.str("code")),
        (Some("respawn"), Some("HOST_BUSY")),
        "{d:?}"
    );
    no_force(&c);
    assert_eq!(pass(&c, &no_pr("1.50"), T + 60), [1]);
    no_force(&c);
}
