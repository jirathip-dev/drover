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
fn idle_21_minutes_with_spawnable_work_heals_then_alerts_once_a_pass_later() {
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
    fast(&at(&c, 27 * 60), &sh);
    fast(&at(&c, 40 * 60), &sh);
    assert_eq!(kinds(&c), ["slo-idle", "self-heal", "stuck"]);
    let pushes = std::fs::read_to_string(&sink).unwrap();
    assert_eq!(pushes.lines().count(), 1, "one push per episode: {pushes}");
    assert!(
        pushes.starts_with("sendmeter stuck: 0 lanes working for 26 min with #12 spawnable")
            && pushes
                .trim_end()
                .ends_with("tried: recounted worktrees 0.0 → 0.0 GB; retried #12's spawn"),
        "{pushes}"
    );
    let shown = card(&c, &sh, NOW + 40 * 60).expect("the owner's card").to_string();
    assert!(
        shown.contains("sendmeter stuck: 0 lanes working") && shown.contains("tried: recounted"),
        "{shown}"
    );
    sh.lane("sendmeter-impl-12", "working", "/x");
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12", "runtime": "shed", "worktree": "/x", "branch": "issue-12"});
    append(&c.state, &Event::new(NOW + 41 * 60, "sendmeter", Some(12), "spawn", d)).unwrap();
    fast(&at(&c, 42 * 60), &sh);
    assert_eq!(kinds(&c), ["slo-idle", "self-heal", "stuck", "slo-ok"]);
    assert_eq!(card(&c, &sh, NOW + 42 * 60), None, "recovered: the card clears");
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
fn idle_with_no_known_cause_alerts_at_once_naming_the_refusal() {
    let (c, sink, sh) = setup("slo-idle-unknown");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/issue-9")).unwrap();
    let d = json!({"code": "HOST_BUSY", "reason": "load 9.1 per cpu"});
    append(&c.state, &Event::new(NOW, "sendmeter", Some(12), "driven", d)).unwrap();
    fast(&c, &sh);
    fast(&at(&c, 21 * 60), &sh);
    fast(&at(&c, 30 * 60), &sh);
    assert_eq!(kinds(&c), ["slo-idle", "stuck"]);
    let pushes = std::fs::read_to_string(&sink).unwrap();
    assert_eq!(
        pushes,
        "sendmeter stuck: 0 lanes working for 21 min with #12 spawnable; #12's last start: HOST_BUSY load 9.1 per cpu; \
         tried: nothing (no known heal)\n"
    );
}

#[test]
fn a_missed_promise_drive_healed_alerts_with_the_issue_reason_and_heal_one_pass_later() {
    let (c, sink, sh) = setup("slo-promise");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/issue-9")).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "rm", "12"]).0, 0);
    let ev = |ts, kind: &str, d: Value| append(&c.state, &Event::new(NOW + ts, "sendmeter", Some(7), kind, d)).unwrap();
    let d = json!({"role": "impl", "agent": "sendmeter-impl-7", "pane": "sendmeter-impl-7", "runtime": "shed", "worktree": "/x", "branch": "issue-7"});
    ev(0, "spawn", d);
    let line = "PASS · verify skipped · merging next pass";
    ev(
        20,
        "promise",
        json!({"kind": "merging", "reason": line, "deadline": NOW + 620, "state": "impl", "head": ""}),
    );
    ev(690, "marked-ready", json!({"pr": 100, "sha": "abc"}));
    ev(
        700,
        "promise-missed",
        json!({"kind": "merging", "promised": line, "age": 680, "why": "now: PASS · CI pending"}),
    );
    fast(&at(&c, 700), &sh);
    assert!(
        kinds(&c).is_empty(),
        "drive's heal (#839) was tried a pass ago: {:?}",
        kinds(&c)
    );
    fast(&at(&c, 800), &sh);
    assert!(kinds(&c).is_empty(), "the heal gets one pass");
    fast(&at(&c, 990), &sh);
    fast(&at(&c, 1100), &sh);
    let pushes = std::fs::read_to_string(&sink).unwrap();
    assert_eq!(
        pushes,
        format!("sendmeter #7 stuck: promise `{line}` missed: now: PASS · CI pending; tried: marked-ready\n")
    );
    assert_eq!(kinds(&c), ["stuck"]);
}
