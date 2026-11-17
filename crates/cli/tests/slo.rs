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

fn fast(c: &drover::Ctx, sh: &FakeShed) -> Fake {
    let r = Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("git ", "")
        .reply("sh -c nohup", "4321");
    let (code, out) = run(c, &sh.on(&r), &["drive", "--fast"]);
    assert_eq!(code, 0, "{out}");
    r
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
    let idle = ev.iter().find(|e| e.kind == "slo-idle").unwrap();
    assert_eq!(
        idle.str("reason"),
        Some("0 lanes working for 21 min with #12 spawnable; held: DISK_BUDGET (worktrees 0.0 GB)")
    );
    let heal = ev.iter().find(|e| e.kind == "self-heal").unwrap();
    assert_eq!(
        heal.str("did"),
        Some("recounted worktrees 0.0 → 0.0 GB; retried #12's spawn")
    );
    fast(&at(&c, 22 * 60), &sh);
    assert_eq!(kinds(&c), ["slo-idle", "self-heal"], "the heal gets one pass");
    assert_eq!(std::fs::read_to_string(&sink).unwrap_or_default(), "");
    fast(&at(&c, 26 * 60), &sh);
    fast(&at(&c, 40 * 60), &sh);
    assert_eq!(kinds(&c), ["slo-idle", "self-heal"], "a wait drive acts on: no card");
    assert_eq!(std::fs::read_to_string(&sink).unwrap_or_default(), "");
    assert_eq!(card(&c, &sh, NOW + 40 * 60), None);
    sh.lane("sendmeter-impl-12", "working", "/x");
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12", "runtime": "shed", "worktree": "/x", "branch": "issue-12"});
    append(&c.state, &Event::new(NOW + 41 * 60, "sendmeter", Some(12), "spawn", d)).unwrap();
    fast(&at(&c, 42 * 60), &sh);
    assert_eq!(kinds(&c), ["slo-idle", "self-heal", "slo-ok"]);
}

#[test]
fn idle_that_recovers_after_the_heal_never_alerts() {
    let (c, sink, sh) = setup("slo-idle-recovers");
    fast(&c, &sh);
    fast(&at(&c, 21 * 60), &sh);
    assert_eq!(kinds(&c), ["slo-idle", "self-heal"]);
    sh.lane("sendmeter-impl-12", "working", "/x");
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12", "runtime": "shed", "worktree": "/x", "branch": "issue-12"});
    append(&c.state, &Event::new(NOW + 22 * 60, "sendmeter", Some(12), "spawn", d)).unwrap();
    fast(&at(&c, 23 * 60), &sh);
    fast(&at(&c, 30 * 60), &sh);
    assert_eq!(kinds(&c), ["slo-idle", "self-heal", "slo-ok"]);
    assert_eq!(std::fs::read_to_string(&sink).unwrap_or_default(), "");
    assert_eq!(card(&c, &sh, NOW + 30 * 60), None);
}

#[test]
fn idle_held_by_host_busy_makes_no_card() {
    let (c, sink, sh) = setup("slo-idle-busy");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/issue-9")).unwrap();
    let d = json!({"code": "HOST_BUSY", "reason": "load 9.1 per cpu"});
    append(&c.state, &Event::new(NOW, "sendmeter", Some(12), "driven", d)).unwrap();
    fast(&c, &sh);
    fast(&at(&c, 21 * 60), &sh);
    fast(&at(&c, 30 * 60), &sh);
    assert_eq!(kinds(&c), ["slo-idle"]);
    assert_eq!(std::fs::read_to_string(&sink).unwrap_or_default(), "");
    assert_eq!(card(&c, &sh, NOW + 30 * 60), None);
}

fn quiet(name: &str) -> (drover::Ctx, std::path::PathBuf, FakeShed) {
    let (c, sink, sh) = setup(name);
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/issue-9")).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "rm", "12"]).0, 0);
    (c, sink, sh)
}

fn last_pass(c: &drover::Ctx, at: u64) {
    std::fs::write(c.state.with_file_name("drive-sendmeter.last"), at.to_string()).unwrap();
}

#[test]
fn no_drive_pass_for_30_minutes_shows_one_card_and_one_push_until_the_next_pass() {
    let (c, sink, sh) = quiet("slo-no-pass");
    last_pass(&c, NOW);
    fast(&at(&c, 29 * 60), &sh);
    assert!(kinds(&c).is_empty(), "inside 6 passes");
    fast(&at(&c, 30 * 60), &sh);
    fast(&at(&c, 35 * 60), &sh);
    assert_eq!(kinds(&c), ["stuck"]);
    assert_eq!(
        std::fs::read_to_string(&sink).unwrap(),
        "sendmeter stuck: no drive pass for 30 min; tried: nothing (no known heal)\n"
    );
    let shown = card(&c, &sh, NOW + 35 * 60).expect("the owner's card").to_string();
    assert!(shown.contains("no drive pass for 30 min"), "{shown}");
    assert!(shown.contains("`drover waiting sendmeter`") && !shown.contains("drover why"), "{shown}");
    let (code, out) = run(&at(&c, 35 * 60), &Fake::default(), &["waiting", "sendmeter"]);
    assert_eq!(code, 0, "the hint runs: {out}");
    last_pass(&c, NOW + 36 * 60);
    fast(&at(&c, 37 * 60), &sh);
    assert_eq!(kinds(&c), ["stuck", "slo-ok"]);
    assert_eq!(card(&c, &sh, NOW + 37 * 60), None, "a pass ran: the card clears");
    assert_eq!(std::fs::read_to_string(&sink).unwrap().lines().count(), 1);
}

#[test]
fn a_card_left_by_a_missed_promise_clears_on_the_first_pass() {
    let (c, sink, sh) = quiet("slo-old-card");
    let d = json!({"key": format!("promise:sendmeter#7:{}", NOW + 20), "reason": "⚠ missed 16m: drive took no step; tried: nothing (no known heal)"});
    append(&c.state, &Event::new(NOW + 700, "sendmeter", Some(7), "stuck", d)).unwrap();
    assert!(card(&c, &sh, NOW + 700).is_some(), "the card live before the deploy");
    fast(&at(&c, 760), &sh);
    assert_eq!(kinds(&c), ["stuck", "slo-ok"]);
    assert_eq!(card(&c, &sh, NOW + 760), None);
    assert_eq!(std::fs::read_to_string(&sink).unwrap_or_default(), "");
}

fn failed_pass(c: &drover::Ctx, ts: u64) {
    let a = json!([{"step": "pass", "outcome": "error", "code": "ERROR", "words": "failed: no such fleet `sendmeter`"}]);
    let d = json!({"by": "auto", "actions": a, "kick": null});
    append(&c.state, &Event::new(NOW + ts, "sendmeter", None, "drive-pass", d)).unwrap();
}

fn fix_due(c: &drover::Ctx, n: u64) {
    let d = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "pane": format!("sendmeter-impl-{n}"),
        "runtime": "shed", "worktree": "/x", "branch": format!("issue-{n}")});
    append(&c.state, &Event::new(NOW, "sendmeter", Some(n), "spawn", d)).unwrap();
    let v = json!({"verdict": "FAIL", "sha": "a".repeat(40), "body": "FAIL\nsrc/x.rs:3 off by one"});
    append(&c.state, &Event::new(NOW + 10, "sendmeter", Some(n), "verdict", v)).unwrap();
}

fn force(c: &drover::Ctx, r: &dyn drover::runner::Runner, n: u64) -> String {
    let s = Server::new(c, r, Some("127.0.0.1:7777".into()));
    let cookie = pair(&s, c, "", c.now);
    let body = json!({"screen": "cards", "action": "force", "values": {"fleet": "sendmeter", "issue": n}}).to_string();
    let req = format!(
        "POST /action HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&req, c.now)).into_owned()
}

#[test]
fn a_pass_drive_cannot_retry_names_its_error_and_forces_the_held_fix_round() {
    let (c, sink, sh) = quiet("slo-pass-error");
    fix_due(&c, 21);
    failed_pass(&c, 60);
    fast(&at(&c, 120), &sh);
    assert_eq!(kinds(&c), ["stuck"]);
    assert_eq!(
        std::fs::read_to_string(&sink).unwrap(),
        "sendmeter stuck: drive pass failed: no such fleet `sendmeter`; tried: nothing (no known heal)\n"
    );
    let shown = card(&c, &sh, NOW + 120).expect("the owner's card");
    let text = shown.to_string();
    assert!(text.contains("#21: ") && text.contains("`drover waiting sendmeter`"), "{text}");
    let buttons = shown["buttons"].as_array().unwrap();
    let b = buttons.iter().find(|b| b["action"] == "force").expect("[force]");
    assert_eq!((b["label"].as_str(), &b["values"]["issue"]), (Some("Force #21"), &json!(21)));
    let busy = Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "4\n")
        .reply("sysctl -n vm.loadavg", "{ 40.0 40.0 40.0 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "");
    let res = force(&at(&c, 130), &sh.on(&busy), 21);
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    let ev = read(&c.state).unwrap();
    let o: Vec<&Event> = ev.iter().filter(|e| e.kind == "override" && e.issue == Some(21)).collect();
    assert_eq!(o.len(), 1, "{o:?}");
    assert_eq!((o[0].str("code"), o[0].str("by")), (Some("HOST_BUSY"), Some("owner")));
    sh.lane("sendmeter-impl-21", "working", "/x");
    let text = card(&c, &sh, NOW + 140).expect("drive is still failing").to_string();
    assert!(!text.contains("#21: ") && !text.contains("Force #21"), "the lane started: {text}");
    let d = json!({"by": "auto", "actions": [], "kick": null});
    append(&c.state, &Event::new(NOW + 150, "sendmeter", None, "drive-pass", d)).unwrap();
    fast(&at(&c, 180), &sh);
    assert_eq!(card(&c, &sh, NOW + 180), None, "a clean pass clears it");
}

#[test]
fn a_missed_promise_a_hold_took_over_makes_no_stuck_card() {
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
    ev(710, "held", json!({"on": [261]}));
    fast(&at(&c, 720), &sh);
    fast(&at(&c, 1100), &sh);
    assert!(!kinds(&c).contains(&"stuck".to_string()), "{:?}", kinds(&c));
    assert_eq!(std::fs::read_to_string(&sink).unwrap_or_default(), "");
    assert_eq!(card(&c, &sh, NOW + 1100), None);
    let ev = read(&c.state).unwrap();
    assert!(drover::slo::open_stuck(&ev).is_empty());
}
