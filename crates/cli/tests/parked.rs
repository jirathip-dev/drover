mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::serve::Server;
use serde_json::json;

const NOW: u64 = 1_000_000;
const SAFE: &str = "sendmeter-impl-12";
const RISKY: &str = "sendmeter-impl-14";

fn fixture(name: &str, paused: bool) -> (drover::Ctx, OnShed) {
    let cfg = CONFIG.replace(
        "integration = \"staging\"   # never main",
        &format!("integration = \"staging\"\npaused = {paused}"),
    );
    let c = ctx(&tmpdir(name), &cfg);
    for (n, agent) in [(12, SAFE), (14, RISKY)] {
        let d = json!({"role": "impl", "agent": agent, "harness": "claude", "pane": format!("w1:{n}"), "worktree": format!("/wt/{agent}")});
        append(&c.state, &Event::new(NOW - 3 * 3600, "sendmeter", Some(n), "spawn", d)).unwrap();
    }
    let cache = c.state.parent().unwrap().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let pr = json!({"number": 42, "state": "OPEN", "headRefOid": "6a258db".to_string() + &"0".repeat(33), "statusCheckRollup": []});
    std::fs::write(
        cache.join("pr-sendmeter-12.json"),
        json!({"ts": 1, "branch": "issue-12", "pr": pr}).to_string(),
    )
    .unwrap();
    let r = Fake::default()
        .reply(&format!("git -C /wt/{SAFE} status"), "?? .brief.md\n")
        .reply(&format!("git -C /wt/{SAFE} rev-list"), "0\n")
        .reply(&format!("git -C /wt/{SAFE} rev-parse"), "6a258db\n")
        .reply(
            &format!("git -C /wt/{SAFE} log"),
            &format!("6a258db{} {}", "0".repeat(33), NOW - 7200),
        )
        .reply(&format!("git -C /wt/{RISKY} status"), " M src/a.rs\n");
    let r = OnShed::new(r);
    look(&c, &r);
    (c, r)
}

fn v1(s: &Server, path: &str) -> serde_json::Value {
    serde_json::from_str(&api(s, "", "", NOW, path)).unwrap()
}

fn lane(s: &Server, agent: &str) -> Option<serde_json::Value> {
    let l = v1(s, "lanes");
    let all = l["sections"].as_array().unwrap().iter();
    let all = all.flat_map(|sec| {
        sec["repos"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|r| r["lanes"].as_array().unwrap().iter())
    });
    all.into_iter().find(|x| x["name"] == agent).cloned()
}

fn row(s: &Server, n: u64) -> Option<serde_json::Value> {
    let q = v1(s, "queue");
    q["fleets"][0]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["issue"] == n)
        .cloned()
}

fn sections(s: &Server) -> Vec<String> {
    let l = v1(s, "lanes");
    l["sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn gone_pane_with_safe_work_is_parked_dirty_is_dead() {
    let (c, r) = fixture("parked-sheet", true);
    let s = Server::new(&c, &r, None);
    let safe = lane(&s, SAFE).expect("parked row");
    assert_eq!(safe["gone"], true, "no pane: stop/kill hidden: {safe}");
    assert_eq!(safe["shown"], "parked · work on PR #42", "{safe}");
    assert_eq!(
        (&safe["why"], &safe["why_tone"], &safe["tone"], &safe["dead"]),
        (
            &json!("parked: fleet paused (resume to review PR #42)"),
            &json!("m"),
            &json!("m"),
            &json!(false)
        ),
        "neutral, no red: {safe}"
    );
    let risky = lane(&s, RISKY).expect("dead row");
    assert_eq!(
        (&risky["state"], &risky["tone"], &risky["dead"]),
        (&json!("dead"), &json!("r"), &json!(true)),
        "{risky}"
    );
    assert_eq!(sections(&s), ["parked", "dead"]);
    let tones = |n: u64| row(&s, n).map(|r| (r["state"].clone(), r["tone"].clone()));
    assert_eq!(tones(12), Some((json!("parked"), json!("m"))), "queue rows");
    assert_eq!(tones(14), Some((json!("dead"), json!("r"))), "queue rows");
    let herd = v1(&s, "herd");
    let state = |n: &str| {
        herd["fleets"][0]["lanes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["name"] == n)
            .map(|l| (l["state"].clone(), l["shown"].clone()))
    };
    assert_eq!(state(SAFE), Some((json!("parked"), json!("parked · work on PR #42"))));
    assert_eq!(state(RISKY).map(|s| s.0), Some(json!("dead")));
    let (code, out) = run(&c, &r, &["status", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    let row = out
        .lines()
        .find(|l| l.contains("#12") || l.contains(" 12 "))
        .unwrap_or_default();
    assert!(row.ends_with("parked: fleet paused (resume to review PR #42)"), "{out}");
}

#[test]
fn parked_lane_alerts_once_and_never_stalls() {
    let (mut c, r) = fixture("parked-check", true);
    for at in [NOW, NOW + 3600, NOW + 7200] {
        c.now = at;
        let (code, out) = run(&c, &r, &["check"]);
        assert_eq!(code, 0, "{out}");
    }
    let n = |kind: &str, issue: u64| {
        read(&c.state)
            .unwrap()
            .iter()
            .filter(|e| e.kind == kind && e.issue == Some(issue))
            .count()
    };
    assert_eq!(
        (n("lane-dead", 12), n("stall", 12)),
        (1, 0),
        "parked: one lane-dead, no stall"
    );
    assert_eq!(
        (n("lane-dead", 14), n("stall", 14)),
        (1, 0),
        "dead (dirty) keeps its lane-dead; #1053: a gone lane is dead, never stalled"
    );
}

#[test]
fn unpaused_fleet_resumes_a_parked_impl_to_review() {
    let (c, r) = fixture("parked-resume", false);
    let events = read(&c.state).unwrap();
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let waits = drover::waiting::scan(&c, &cfg, &r, &events, &[], false, None);
    let w = &waits.iter().find(|(s, _)| s.issue == 12).unwrap().1;
    assert_eq!(
        (w.who, w.what.as_str()),
        (
            "drive",
            "pushed 6a258db, waiting for drive (next pass ≤ 5 min) to spawn review"
        )
    );
    assert_eq!(w.next_cmd.as_deref(), Some("drover spawn sendmeter 12 rev"));
}

#[test]
fn cleaned_lane_of_a_closed_issue_is_not_dead() {
    let (c, r) = fixture("closed-not-dead", false);
    let check = |c: &drover::Ctx| {
        let (code, out) = run(c, &r, &["check"]);
        assert_eq!(code, 0, "{out}");
        read(&c.state).unwrap()
    };
    let hits = |ev: &[Event], n: u64| {
        ev.iter()
            .any(|e| e.issue == Some(n) && matches!(e.kind.as_str(), "stall" | "lane-dead"))
    };
    let ev = check(&c);
    assert!(hits(&ev, 14), "a vanished lane with no clean is dead");
    let l = lane(&Server::new(&c, &r, None), RISKY).expect("dead row");
    assert_eq!(l["state"], "dead", "{l}");

    let c = fixture("closed-cleaned", false).0;
    let d = json!({"worktree": format!("/wt/{RISKY}"), "pane": "w1:14", "why": "issue closed"});
    append(&c.state, &Event::new(NOW - 3600, "sendmeter", Some(14), "clean", d)).unwrap();
    let st = drover::events::fold(&read(&c.state).unwrap());
    assert_eq!(st[&("sendmeter".to_string(), 14)].state, "closed");
    assert!(!hits(&check(&c), 14), "no stall or lane-dead for a closed issue");
    assert!(lane(&Server::new(&c, &r, None), RISKY).is_none(), "no Dead row");
    let (code, waiting) = run(&c, &r, &["waiting"]);
    assert!(code == 0 && !waiting.contains("#14"), "{waiting}");
}

#[test]
fn a_fix_round_held_for_a_spawn_slot_is_not_parked() {
    let (c, r) = fixture("parked-slot", false);
    let head = "6a258db".to_string() + &"0".repeat(33);
    let ev = |ts, kind, d| append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
    ev(
        NOW - 600,
        "verdict",
        json!({"verdict": "FAIL", "sha": head, "body": "FAIL\nsrc/x.rs:3 off"}),
    );
    let s = Server::new(&c, &r, None);
    let shown = |s: &Server| {
        let tab = v1(s, "herd");
        let lanes = tab["fleets"][0]["lanes"].as_array().unwrap().clone();
        lanes.into_iter().find(|l| l["name"] == SAFE).unwrap()["shown"].clone()
    };
    assert_eq!(shown(&s), json!("parked · work on PR #42"), "not yet tried");
    let d = json!({"step": "fix", "result": "REFUSED SPAWN_PENDING: 6 spawns starting", "code": "SPAWN_PENDING"});
    ev(NOW - 300, "driven", d);
    let s = Server::new(&c, &r, None);
    assert_eq!(shown(&s), json!("idle · waiting for a spawn slot (10m)"));
}

#[test]
fn a_held_lane_reads_held_by_its_blocker_and_a_parked_pr_number_drops_off() {
    let (c, r) = fixture("held-not-dead", false);
    let held = Event::new(NOW - 3000, "sendmeter", Some(14), "held", json!({"on": [456]}));
    append(&c.state, &held).unwrap();
    let q = json!({"op": "add", "issues": [828]});
    append(&c.state, &Event::new(NOW - 2000, "sendmeter", None, "queue", q)).unwrap();
    let parked = json!({"why": "a PR number", "by": "owner"});
    append(
        &c.state,
        &Event::new(NOW - 1000, "sendmeter", Some(828), "parked", parked),
    )
    .unwrap();
    let spec =
        json!({"title": "t", "body": "Closes #817", "url": "https://github.com/jirathip-dev/sendmeter/pull/828"});
    let cache = c.state.parent().unwrap().join("cache");
    std::fs::write(cache.join("issue-sendmeter-828.json"), spec.to_string()).unwrap();
    let s = Server::new(&c, &r, None);
    let risky = lane(&s, RISKY).expect("held row");
    assert!(
        risky.to_string().contains("held by #456") && risky["dead"] == false,
        "{risky}"
    );
    assert!(!sections(&s).contains(&"dead".to_string()), "no dead section");
    let q = row(&s, 14).expect("queue row");
    assert_eq!(
        (&q["state"], &q["tone"]),
        (&json!("held by #456"), &json!("a")),
        "queue row: {q}"
    );
    assert!(row(&s, 828).is_none(), "no row for a PR number");
}
