mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::json;

const START: u64 = 10 * 60;

fn ev(c: &drover::Ctx, ts: u64, n: Option<u64>, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", n, kind, d)).unwrap();
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

fn six_lanes(c: &drover::Ctx, t0: u64) -> Vec<(String, String)> {
    (1..=6u64)
        .map(|n| {
            let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
            std::fs::create_dir_all(&wt).unwrap();
            let agent = format!("sendmeter-impl-{n}");
            let d = json!({"role": "impl", "agent": agent, "pane": agent, "workspace": "", "runtime": "shed",
                "worktree": wt, "branch": format!("issue-{n}"), "confirmed": true});
            ev(c, t0, Some(n), "spawn", d);
            (agent, wt.to_string_lossy().into_owned())
        })
        .collect()
}

fn fake() -> Fake {
    Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "")
        .reply("which shed", "/opt/shed/bin/shed\n")
}

fn drive(c: &drover::Ctx, r: &Fake) -> String {
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    out
}

#[test]
fn six_lanes_failing_to_start_on_one_host_trip_the_breaker_once_and_park_no_issue() {
    let c = ctx(&tmpdir("breaker-trip"), CONFIG);
    let t0 = 1_000_000;
    let sh = FakeShed::attached(&c);
    for (a, w) in six_lanes(&c, t0) {
        sh.lane(&a, "idle", &w);
        sh.screen(&a, "\n  \n");
    }
    sh.wait_ends("drover-canary", "timeout");
    let r = fake();
    let out = drive(&at(&c, t0 + START + 1), &r);
    assert_eq!(
        kinds(&c, "respawn").len(),
        2,
        "#908: respawn_per_pass (2) a pass: {out}"
    );
    assert!(kinds(&c, "runtime-down").is_empty(), "{out}");
    let out = drive(&at(&c, t0 + START + 2), &r);
    let down = kinds(&c, "runtime-down");
    assert_eq!(down.len(), 1, "{out}");
    assert_eq!(
        (down[0].str("host"), down[0].val("failures").as_u64()),
        (Some("local"), Some(5))
    );
    assert_eq!(down[0].str("sample"), Some("no output after its spawn"));
    let respawned = kinds(&c, "respawn").len();
    assert_eq!(respawned, 4, "the lanes before the trip are retried, none after: {out}");
    assert!(kinds(&c, "needs-human").is_empty(), "{out}");
    for k in 2..=4 {
        sh.lock().seen.clear();
        let out = drive(&at(&c, t0 + k * (START + 1)), &r);
        assert_eq!(sh.all("run").len(), 1, "only the canary runs: {out}");
        assert!(sh.called("run", "drover-canary"), "{out}");
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
                json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "why": "lane gone", "failed_on": "local"}),
            );
        }
    }
    let d = json!({"host": "local", "failures": 5, "sample": "lane gone", "reason": "runtime down on local"});
    ev(&c, t0 + 10, None, "runtime-down", d);
    let late = at(&c, t0 + 3600);
    let sh = FakeShed::attached(&c);
    sh.wait_ends("drover-canary", "timeout");
    let r = fake();
    let out = drive(&late, &r);
    assert!(out.contains("canary failed"), "{out}");
    assert_eq!(r.count("sh -c nohup"), 0, "{out}");
    assert!(kinds(&c, "needs-human").is_empty(), "{out}");
    sh.lock().waits.clear();
    let r = fake();
    let out = drive(&late, &r);
    let canary = sh
        .all("run")
        .into_iter()
        .find(|p| p["name"] == "drover-canary")
        .unwrap();
    assert_eq!(
        canary["cmd"],
        json!(["sh", "-c", "echo drover-canary-$((6*7))"]),
        "{out}"
    );
    assert_eq!(sh.last("wait")["output"], "drover-canary-42");
    let up = kinds(&c, "runtime-up");
    assert_eq!(up.len(), 1, "{out}");
    assert_eq!(up[0].str("host"), Some("local"));
    let kicks = r.count("sh -c nohup '/opt/drover/bin/drover' kick sendmeter");
    assert_eq!(kicks, 2, "respawns resume, at most max_parallel_spawns at once: {out}");
    assert!(kinds(&c, "needs-human").is_empty(), "{out}");
    assert!(kinds(&c, "runtime-down").len() == 1 && kinds(&c, "runtime-up").len() == 1);
}
