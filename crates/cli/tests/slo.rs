mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const NOW: u64 = 1_000_000;

fn setup(name: &str) -> (drover::Ctx, std::path::PathBuf, FakeShed) {
    let dir = tmpdir(name);
    let sink = dir.join("push.txt");
    let cfg = format!(
        "{CONFIG}worktree_budget_gb = 0.000004\n\n[[notify]]\ncmd = \"cat >> '{}'\"\non = [\"stuck\"]\n",
        sink.display()
    );
    let c = ctx(&dir, &cfg);
    let sh = FakeShed::attached(&c);
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let lane = c.worktrees.join("sendmeter/issue-9");
    std::fs::create_dir_all(&lane).unwrap();
    std::fs::write(lane.join("big.bin"), vec![1u8; 8192]).unwrap();
    (c, sink, sh)
}

fn at(c: &drover::Ctx, secs: u64) -> drover::Ctx {
    drover::Ctx {
        now: NOW + secs,
        ..c.clone()
    }
}

fn pass(c: &drover::Ctx, sh: &FakeShed, args: &[&str]) -> Fake {
    let r = Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("git ", "")
        .reply("sh -c nohup", "4321");
    let (code, out) = run(c, &sh.on(&r), args);
    assert_eq!(code, 0, "{out}");
    r
}

fn fast(c: &drover::Ctx, sh: &FakeShed) -> Fake {
    pass(c, sh, &["drive", "--fast"])
}

fn kinds(c: &drover::Ctx) -> Vec<String> {
    let ours = ["slo-idle", "self-heal", "stuck", "slo-ok"];
    let ev = read(&c.state).unwrap().to_vec().into_iter().map(|e| e.kind);
    ev.filter(|k| ours.contains(&k.as_str())).collect()
}

fn card(c: &drover::Ctx, sh: &FakeShed, now: u64) -> Option<Value> {
    let f = Fake::default();
    let r = sh.on(&f);
    let s = Server::new(c, &r, Some("127.0.0.1:7777".into()));
    let res = s.respond("GET /screens/cards HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n\r\n", now);
    let res = String::from_utf8_lossy(&res);
    let v: Value = serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    v.as_array()
        .unwrap()
        .iter()
        .find(|c| c["title"] == "Drive is stuck")
        .cloned()
}

#[test]
fn idle_21_minutes_with_spawnable_work_heals_and_never_makes_a_card() {
    let (c, sink, sh) = setup("slo-idle-stuck");
    pass(&c, &sh, &["check"]);
    fast(&c, &sh);
    assert!(kinds(&c).is_empty(), "idle starts now: no breach yet");
    fast(&at(&c, 19 * 60), &sh);
    assert!(kinds(&c).is_empty(), "19 min is inside the SLO");
    let r = fast(&at(&c, 21 * 60), &sh);
    assert_eq!(kinds(&c), ["slo-idle", "self-heal"]);
    assert!(
        r.called("sh -c nohup")
            && r.calls
                .borrow()
                .iter()
                .any(|l| l.contains("kick sendmeter 12 --why slo-idle"))
    );
    let ev = read(&c.state).unwrap();
    let heal = ev.iter().find(|e| e.kind == "self-heal").unwrap();
    assert_eq!(
        heal.str("did"),
        Some("recounted worktrees 0.0 → 0.0 GB; retried #12's spawn")
    );
    for m in [22, 26, 27, 40] {
        fast(&at(&c, m * 60), &sh);
    }
    assert_eq!(kinds(&c), ["slo-idle", "self-heal"], "a wait: no stuck");
    assert_eq!(std::fs::read_to_string(&sink).unwrap_or_default(), "");
    assert_eq!(card(&c, &sh, NOW + 40 * 60), None);
    sh.lane("sendmeter-impl-12", "working", "/x");
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12", "runtime": "shed", "worktree": "/x", "branch": "issue-12"});
    append(&c.state, &Event::new(NOW + 41 * 60, "sendmeter", Some(12), "spawn", d)).unwrap();
    fast(&at(&c, 42 * 60), &sh);
    assert_eq!(kinds(&c), ["slo-idle", "self-heal", "slo-ok"]);
}

#[test]
fn idle_held_by_host_busy_is_a_wait_not_a_card() {
    let (c, sink, sh) = setup("slo-idle-busy");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/issue-9")).unwrap();
    let d = json!({"code": "HOST_BUSY", "reason": "load 9.1 per cpu"});
    append(&c.state, &Event::new(NOW, "sendmeter", Some(12), "driven", d)).unwrap();
    for m in [0, 21, 30, 60] {
        fast(&at(&c, m * 60), &sh);
    }
    assert_eq!(kinds(&c), ["slo-idle"]);
    assert_eq!(std::fs::read_to_string(&sink).unwrap_or_default(), "");
    assert_eq!(card(&c, &sh, NOW + 60 * 60), None);
}

fn quiet(name: &str) -> (drover::Ctx, std::path::PathBuf, FakeShed) {
    let (c, sink, sh) = setup(name);
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/issue-9")).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "rm", "12"]).0, 0);
    (c, sink, sh)
}

fn ev(c: &drover::Ctx, n: u64, ts: u64, kind: &str, d: Value) {
    append(&c.state, &Event::new(NOW + ts, "sendmeter", Some(n), kind, d)).unwrap();
}

fn spawned(c: &drover::Ctx, n: u64) {
    let agent = format!("sendmeter-impl-{n}");
    let d = json!({"role": "impl", "agent": agent, "pane": agent, "runtime": "shed", "worktree": "/x", "branch": format!("issue-{n}")});
    ev(c, n, 0, "spawn", d);
}

#[test]
fn a_missed_promise_with_its_lane_gone_makes_no_card_and_a_live_one_clears() {
    let (c, sink, sh) = quiet("slo-promise");
    spawned(&c, 7);
    let line = "PASS · verify skipped · merging next pass";
    let p = json!({"kind": "merging", "reason": line, "deadline": NOW + 620, "state": "impl", "head": ""});
    ev(&c, 7, 20, "promise", p);
    let m = json!({"kind": "merging", "promised": line, "age": 680, "why": "drive took no step"});
    ev(&c, 7, 700, "promise-missed", m);
    let old = json!({"key": format!("promise:sendmeter#7:{}", NOW + 20), "reason": "⚠ missed 11m: drive took no step"});
    ev(&c, 7, 710, "stuck", old);
    assert!(card(&c, &sh, NOW + 710).is_some(), "the card live before deploy");
    fast(&at(&c, 720), &sh);
    assert_eq!(card(&c, &sh, NOW + 720), None, "cleared on the first pass");
    fast(&at(&c, 1100), &sh);
    assert_eq!(kinds(&c), ["stuck", "slo-ok"]);
    assert_eq!(std::fs::read_to_string(&sink).unwrap_or_default(), "");
}

#[test]
fn no_drive_pass_for_30_minutes_is_one_card_and_push_until_the_next_pass() {
    let (c, sink, sh) = quiet("slo-no-pass");
    let last = c.state.with_file_name("drive-sendmeter.last");
    std::fs::write(&last, NOW.to_string()).unwrap();
    fast(&at(&c, 29 * 60), &sh);
    assert!(kinds(&c).is_empty(), "{:?}", kinds(&c));
    fast(&at(&c, 31 * 60), &sh);
    fast(&at(&c, 36 * 60), &sh);
    assert_eq!(kinds(&c), ["stuck"]);
    let pushes = std::fs::read_to_string(&sink).unwrap();
    assert_eq!(
        pushes,
        "sendmeter stuck: no drive pass for 31 min; tried: nothing (no known heal)\n"
    );
    let shown = card(&c, &sh, NOW + 36 * 60).expect("the card").to_string();
    assert!(shown.contains("no drive pass for 31 min"), "{shown}");
    std::fs::write(&last, (NOW + 37 * 60).to_string()).unwrap();
    fast(&at(&c, 37 * 60), &sh);
    assert_eq!(kinds(&c), ["stuck", "slo-ok"]);
    assert_eq!(card(&c, &sh, NOW + 37 * 60), None);
}

fn failing(c: &drover::Ctx, n: u64) {
    spawned(c, n);
    for ts in [100, 400] {
        let d = json!({"step": "respawn", "error": "shed run: exit 1", "head": null});
        ev(c, n, ts, "spawn-error", d);
    }
}

#[test]
fn three_issues_drive_cannot_retry_push_once_and_show_one_card_with_a_force_each() {
    let (c, sink, sh) = quiet("slo-fold");
    for n in [7, 8, 9] {
        failing(&c, n);
    }
    let d = json!({"step": "respawn", "error": "shed run: exit 1", "head": null});
    ev(
        &c,
        10,
        0,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-10", "pane": "p", "worktree": "/x", "branch": "issue-10"}),
    );
    ev(&c, 10, 400, "spawn-error", d);
    fast(&at(&c, 500), &sh);
    fast(&at(&c, 800), &sh);
    assert_eq!(kinds(&c), ["stuck", "stuck", "stuck"], "one error is retried: no card");
    let pushes = std::fs::read_to_string(&sink).unwrap();
    assert_eq!(pushes.lines().count(), 1, "one push per fleet: {pushes}");
    assert!(
        pushes.contains("respawn fails the same way 2 times: shed run: exit 1"),
        "{pushes}"
    );
    let k = card(&c, &sh, NOW + 800).expect("one card");
    let shown = k.to_string();
    for n in [7, 8, 9] {
        let force = k["buttons"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["label"] == format!("Force #{n}"));
        let force = force.unwrap_or_else(|| panic!("{shown}"));
        assert_eq!(
            (force["action"].as_str(), &force["values"]),
            (Some("force"), &json!({"fleet": "sendmeter", "issue": n}))
        );
    }
    assert!(!shown.contains("drover why"), "{shown}");
    assert!(shown.contains("`drover waiting sendmeter`"), "{shown}");
    let (code, out) = run(&at(&c, 800), &sh.on(&Fake::default()), &["waiting", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn force_on_a_stuck_issue_spawns_it_past_the_load_limit_by_the_owner_and_its_line_clears() {
    std::env::remove_var("DROVER_LANE");
    std::env::remove_var("DROVER_ORCHESTRATOR");
    let (c, _sink, sh) = quiet("slo-force");
    failing(&c, 7);
    failing(&c, 8);
    fast(&at(&c, 500), &sh);
    assert!(card(&c, &sh, NOW + 500).is_some());
    let r = Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 40.0 40.0 40.0 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("git -C", "")
        .reply("gh issue view 7", r#"{"title":"t","body":"b"}"#);
    let on = sh.on(&r);
    let c5 = at(&c, 500);
    let s = Server::new(&c5, &on, Some("127.0.0.1:7777".into()));
    let cookie = pair(&s, &c, "", NOW + 500);
    let body = json!({"screen": "cards", "action": "force", "values": {"fleet": "sendmeter", "issue": 7}}).to_string();
    let req = format!(
        "POST /action HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&req, NOW + 500)).into_owned();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(
        res.contains("drover spawn sendmeter 7 impl") && res.contains("--force"),
        "{res}"
    );
    let ev = read(&c.state).unwrap();
    let o: Vec<_> = ev.iter().filter(|e| e.kind == "override").collect();
    assert_eq!(o.len(), 1, "{o:?}");
    assert_eq!(
        (o[0].issue, o[0].str("code"), o[0].str("by")),
        (Some(7), Some("HOST_BUSY"), Some("owner"))
    );
    let shown = card(&c, &sh, NOW + 500).expect("#8 still stuck").to_string();
    assert!(!shown.contains("Force #7") && shown.contains("Force #8"), "{shown}");
}

#[test]
fn a_missed_promise_a_hold_took_over_shows_no_stuck_card() {
    let (c, sink, sh) = setup("slo-missed-held");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/issue-9")).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "rm", "12"]).0, 0);
    let ev = |ts, kind: &str, d: Value| append(&c.state, &Event::new(NOW + ts, "sendmeter", Some(7), kind, d)).unwrap();
    let d = json!({"role": "impl", "agent": "sendmeter-impl-7", "pane": "sendmeter-impl-7", "runtime": "shed", "worktree": "/x", "branch": "issue-7"});
    ev(0, "spawn", d);
    let line = "pushed abc1234, waiting for drive (next pass ≤ 5 min) to spawn review";
    ev(
        20,
        "promise",
        json!({"kind": "next-pass", "reason": line, "deadline": NOW + 620, "state": "impl", "head": ""}),
    );
    ev(
        700,
        "promise-missed",
        json!({"kind": "next-pass", "promised": line, "age": 680, "why": "drive took no step"}),
    );
    ev(710, "held", json!({"on": [8]}));
    fast(&at(&c, 720), &sh);
    fast(&at(&c, 1020), &sh);
    assert!(!kinds(&c).contains(&"stuck".to_string()), "{:?}", kinds(&c));
    assert_eq!(card(&c, &sh, NOW + 1020), None);
    assert_eq!(std::fs::read_to_string(&sink).unwrap_or_default(), "");
}
