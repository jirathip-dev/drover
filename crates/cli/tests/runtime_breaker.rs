mod common;
use common::*;
use herdsman::events::{append, read, Event};
use serde_json::json;

const START: u64 = 10 * 60;

fn ev(c: &herdsman::Ctx, ts: u64, n: Option<u64>, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", n, kind, d)).unwrap();
}

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

fn at(c: &herdsman::Ctx, now: u64) -> herdsman::Ctx {
    herdsman::Ctx { now, ..c.clone() }
}

fn six_lanes(c: &herdsman::Ctx, t0: u64) -> Vec<(String, String, String)> {
    (1..=6u64)
        .map(|n| {
            let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
            std::fs::create_dir_all(&wt).unwrap();
            let (agent, pane) = (format!("sendmeter-impl-{n}"), format!("w9:p{n}"));
            let d = json!({"role": "impl", "agent": agent, "pane": pane, "workspace": "w9", "worktree": wt,
                "branch": format!("issue-{n}"), "confirmed": true});
            ev(c, t0, Some(n), "spawn", d);
            (agent, wt.to_string_lossy().into_owned(), pane)
        })
        .collect()
}

fn fake(agents: &str, screen: &str) -> Fake {
    Fake::default()
        .reply("herdr agent list", agents)
        .reply(
            "herdr workspace create",
            r#"{"result":{"root_pane":{"pane_id":"w8:c1"}}}"#,
        )
        .reply("herdr pane read", screen)
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "")
        .reply("herdr worktree", r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#)
        .reply(
            "herdr pane list",
            r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#,
        )
        .reply("herdr pane", "{}")
        .reply("herdr agent", "{}")
        .reply("herdr workspace", "{}")
}

fn drive(c: &herdsman::Ctx, r: &Fake) -> String {
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    out
}

#[test]
fn six_lanes_failing_to_start_on_one_host_trip_the_breaker_once_and_park_no_issue() {
    let c = ctx(&tmpdir("breaker-trip"), CONFIG);
    let t0 = 1_000_000;
    let lanes = six_lanes(&c, t0);
    let rows: Vec<_> = lanes
        .iter()
        .map(|(a, w, p)| (a.as_str(), "idle", w.as_str(), p.as_str()))
        .collect();
    let r = fake(&agents(&rows), "\n  \n");
    let out = drive(&at(&c, t0 + START + 1), &r);
    let down = kinds(&c, "runtime-down");
    assert_eq!(down.len(), 1, "{out}");
    assert_eq!(
        (down[0].str("host"), down[0].data["failures"].as_u64()),
        (Some("local"), Some(5))
    );
    assert_eq!(down[0].str("sample"), Some("no output after its spawn"));
    let respawned = kinds(&c, "respawn").len();
    assert_eq!(respawned, 4, "the lanes before the trip are retried, none after: {out}");
    assert!(kinds(&c, "needs-human").is_empty(), "{out}");
    for k in 2..=4 {
        r.calls.borrow_mut().clear();
        let out = drive(&at(&c, t0 + k * (START + 1)), &r);
        assert!(!r.called("herdr agent start"), "{out}");
        assert!(out.contains("runtime down on local: canary failed"), "{out}");
    }
    assert_eq!(kinds(&c, "respawn").len(), respawned);
    assert!(kinds(&c, "needs-human").is_empty());
    assert_eq!(kinds(&c, "runtime-down").len(), 1);
    assert!(kinds(&c, "runtime-up").is_empty());
    let (code, w) = run(&at(&c, t0 + 4 * START), &r, &["waiting", "sendmeter"]);
    assert_eq!(code, 0, "{w}");
    assert_eq!(w.matches("runtime down on local since 13:").count(), 6, "{w}");
    let (code, s) = run(&at(&c, t0 + 4 * START), &r, &["status", "sendmeter"]);
    assert_eq!(code, 0, "{s}");
    assert!(s.contains("runtime down on local since 13:"), "{s}");
    let (code, o) = run(&c, &r, &["spawn", "sendmeter", "1", "impl", "--supersede"]);
    assert_eq!(code, 3, "{o}");
    assert!(o.contains("REFUSED RUNTIME_DOWN"), "{o}");
}

#[test]
fn a_passing_canary_closes_the_breaker_and_the_held_lanes_respawn_paced_by_admission() {
    let cfg = CONFIG.replace("max_parallel_spawns = 0", "max_parallel_spawns = 2");
    let c = ctx(&tmpdir("breaker-up"), &cfg);
    let t0 = 1_000_000;
    six_lanes(&c, t0);
    for n in 1..=6 {
        for k in 1..=2 {
            ev(
                &c,
                t0 + k,
                Some(n),
                "respawn",
                json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "why": "lane gone"}),
            );
        }
        let d = json!({"host": "local", "agent": format!("sendmeter-impl-{n}"), "spawn": t0, "why": "lane gone"});
        ev(&c, t0 + 5, Some(n), "lane-failed", d);
    }
    let d = json!({"host": "local", "failures": 5, "sample": "lane gone", "reason": "runtime down on local"});
    ev(&c, t0 + 10, None, "runtime-down", d);
    let late = at(&c, t0 + 3600);
    let r = fake(NO_AGENTS, "");
    let out = drive(&late, &r);
    assert!(out.contains("canary failed"), "{out}");
    assert_eq!(r.count("sh -c nohup"), 0, "{out}");
    assert!(kinds(&c, "needs-human").is_empty(), "{out}");
    let r = fake(NO_AGENTS, "herdsman-canary-42\n$ ");
    let out = drive(&late, &r);
    assert!(
        r.called("herdr pane run w8:c1 echo herdsman-canary-$((6*7))"),
        "{:?}",
        r.calls.borrow()
    );
    let up = kinds(&c, "runtime-up");
    assert_eq!(up.len(), 1, "{out}");
    assert_eq!(up[0].str("host"), Some("local"));
    let kicks = r.count("sh -c nohup '/opt/herdsman/bin/herdsman' kick sendmeter");
    assert_eq!(kicks, 2, "respawns resume, at most max_parallel_spawns at once: {out}");
    assert!(kinds(&c, "needs-human").is_empty(), "{out}");
    assert!(kinds(&c, "runtime-down").len() == 1 && kinds(&c, "runtime-up").len() == 1);
}
