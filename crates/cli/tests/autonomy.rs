mod common;
use common::*;
use drover::events::{append, Event};
use serde_json::{json, Value};

const MON: u64 = 1_794_787_200;
const HEAD: &str = "1111111111111111111111111111111111111111";

fn ev(c: &drover::Ctx, ts: u64, issue: Option<u64>, kind: &str, d: Value) {
    append(&c.state, &Event::new(ts, "sendmeter", issue, kind, d)).unwrap();
}

fn fixture(name: &str) -> drover::Ctx {
    let c = drover::Ctx {
        now: MON + 86_400,
        ..ctx(&tmpdir(name), CONFIG)
    };
    let pass = |r: u64| json!({"verdict": "PASS", "sha": HEAD, "round": r, "by": "lane"});
    ev(
        &c,
        MON - 15 * 86_400,
        None,
        "queue",
        json!({"op": "add", "issues": [1]}),
    );
    ev(
        &c,
        MON - 15 * 86_400 + 10,
        Some(1),
        "spawn",
        json!({"role": "impl", "by": "drive"}),
    );
    ev(&c, MON - 14 * 86_400, Some(1), "verdict", pass(1));
    ev(&c, MON - 14 * 86_400 + 3600, Some(1), "merged", json!({"pr": 1}));
    ev(
        &c,
        MON + 100,
        None,
        "queue",
        json!({"op": "add", "issues": [2, 3, 4, 5, 6]}),
    );
    for (n, by) in [(2, "owner"), (3, "orchestrator-t1")] {
        let id = format!("q{n}");
        ev(
            &c,
            MON + 200,
            Some(n),
            "needs-decision",
            json!({"id": id, "question": "?"}),
        );
        ev(
            &c,
            MON + 800,
            Some(n),
            "decision",
            json!({"id": id, "choice": "yes", "by": by}),
        );
        ev(&c, MON + 900, Some(n), "verdict", pass(1));
        ev(&c, MON + 7200, Some(n), "merged", json!({"pr": n}));
    }
    for r in 1..=2 {
        let fail = json!({"verdict": "FAIL", "sha": HEAD, "round": r, "by": "drive"});
        ev(&c, MON + 1000 + r, Some(4), "verdict", fail);
    }
    ev(&c, MON + 1100, Some(4), "fix-done", json!({"why": "finding is wrong"}));
    ev(&c, MON + 1200, Some(4), "respawn", json!({"role": "impl"}));
    ev(&c, MON + 1300, Some(4), "verdict", pass(3));
    ev(&c, MON + 3700, Some(4), "merged", json!({"pr": 4}));
    ev(&c, MON + 500, Some(5), "closed", json!({}));
    ev(&c, MON + 600, Some(6), "parked", json!({"by": "owner"}));
    c
}

fn stats(c: &drover::Ctx, extra: &[&str]) -> String {
    let (code, out) = run(
        c,
        &Fake::default(),
        &[&["stats", "autonomy", "sendmeter"][..], extra].concat(),
    );
    assert_eq!(code, 0, "{out}");
    out
}

fn total(c: &drover::Ctx, extra: &[&str]) -> Value {
    let j: Value = serde_json::from_str(&stats(c, &[&["--json"][..], extra].concat())).unwrap();
    j["total"].clone()
}

#[test]
fn an_owner_answer_breaks_autonomy_and_an_orchestrator_answer_does_not() {
    let c = fixture("autonomy-table");
    let out = stats(&c, &[]);
    assert!(
        out.contains("sendmeter 2026-11-02: merged 1 · no owner action 1 (100%)"),
        "{out}"
    );
    assert!(
        out.contains("sendmeter 2026-11-16: merged 3 · no owner action 2 (66%)"),
        "{out}"
    );
    let t = out.lines().find(|l| l.starts_with("sendmeter total: ")).expect(&out);
    assert!(t.contains("merged 4 · no owner action 3 (75%)"), "{t}");
    assert!(t.contains("owner actions 0.50/merge (ask answered 1, parked 1)"), "{t}");
    assert!(t.contains("asks 2 (orchestrator 1, owner 1)"), "{t}");
    assert!(
        t.contains("rounds median 1, round 3+ 1 · pushbacks 1 · respawns 1"),
        "{t}"
    );
    assert!(t.contains("owner wait 10m (0%)"), "{t}");
    assert!(t.contains("closed unmerged 1 · parked 1"), "{t}");
}

#[test]
fn since_and_weeks_cut_where_they_say() {
    let c = fixture("autonomy-cut");
    assert_eq!(total(&c, &[])["merged"], 4);
    assert_eq!(total(&c, &["--since", "2026-11-10"])["merged"], 3);
    assert_eq!(total(&c, &["--since", "2026-11-03"])["merged"], 3);
    assert_eq!(total(&c, &["--since", "2026-11-02"])["merged"], 4);
    assert_eq!(total(&c, &["--weeks", "1"])["merged"], 3);
    assert_eq!(total(&c, &["--weeks", "2"])["merged"], 3);
    assert_eq!(total(&c, &["--weeks", "3"])["merged"], 4);
    let out = stats(&c, &["--weeks", "1"]);
    assert!(!out.contains("2026-11-02"), "{out}");
    let (code, out) = run(
        &c,
        &Fake::default(),
        &["stats", "autonomy", "sendmeter", "--since", "11/10"],
    );
    assert_ne!(code, 0, "{out}");
}

#[test]
fn json_matches_the_table() {
    let c = fixture("autonomy-json");
    let j: Value = serde_json::from_str(&stats(&c, &["--json"])).unwrap();
    let out = stats(&c, &[]);
    for r in j["weeks"].as_array().unwrap().iter().chain([&j["total"]]) {
        let want = format!(
            "sendmeter {}: merged {} · no owner action {} ({}%) · owner actions {:.2}/merge",
            r["week"].as_str().unwrap(),
            r["merged"],
            r["autonomous"],
            r["autonomous_pct"],
            r["owner_actions_per_merge"].as_f64().unwrap()
        );
        let line = out.lines().find(|l| l.starts_with(&want)).expect(&want);
        let rounds = format!("rounds median {}, round 3+ {}", r["rounds_median"], r["round3"]);
        assert!(line.contains(&rounds), "{line}");
        let closed = format!("closed unmerged {} · parked {}", r["closed_unmerged"], r["parked"]);
        assert!(line.contains(&closed), "{line}");
    }
    assert_eq!(j["weeks"].as_array().unwrap().len(), 2);
}

#[test]
fn explain_prints_the_owner_action_table() {
    let c = fixture("autonomy-explain");
    let out = stats(&c, &["--explain"]);
    for label in [
        "ask answered",
        "needs-human",
        "run card approved",
        "manual spawn",
        "manual verdict",
        "manual kill",
    ] {
        assert!(out.contains(&format!("{label}: a `")), "{out}");
    }
}

#[test]
fn the_owners_verdict_and_spawn_count_and_a_lanes_do_not() {
    let c = ctx(
        &tmpdir("autonomy-owner"),
        &CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\""),
    );
    let _sh = FakeShed::attached(&c);
    let w = c.worktrees.join("sendmeter").join("rev-12-r1");
    std::fs::create_dir_all(&w).unwrap();
    std::fs::create_dir_all(c.worktrees.join("sendmeter/issue-12")).unwrap();
    let d = json!({"role": "rev", "agent": "sendmeter-rev-12", "worktree": w, "pane": "sendmeter-rev-12"});
    ev(&c, 10, Some(12), "spawn", d);
    let f = c.state.with_file_name("verdict.md");
    std::fs::write(&f, "PASS\n").unwrap();
    let r = Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("gh api repos/jirathip-dev/sendmeter/branches/issue-12", HEAD)
        .reply("gh issue view 12", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply("git -C", "");
    assert_eq!(run(&c, &r, &["profiles", "sync"]).0, 0);
    let args = [
        "verdict",
        "sendmeter",
        "12",
        "PASS",
        "--sha",
        HEAD,
        "--file",
        f.to_str().unwrap(),
    ];
    let spawn = ["spawn", "sendmeter", "12", "impl", "--supersede"];
    let owner = |c: &drover::Ctx, k: &str| total(c, &[])["owner_actions"][k].clone();
    std::env::remove_var("DROVER_ORCHESTRATOR");
    set_env("DROVER_LANE", "sendmeter:12:rev");
    assert_eq!(run(&c, &r, &args).0, 0);
    let (code, out) = run(&c, &r, &spawn);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        (owner(&c, "manual verdict"), owner(&c, "manual spawn")),
        (Value::Null, Value::Null)
    );
    std::env::remove_var("DROVER_LANE");
    assert_eq!(run(&c, &r, &args).0, 0);
    let (code, out) = run(&c, &r, &spawn);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        (owner(&c, "manual verdict"), owner(&c, "manual spawn")),
        (json!(1), json!(1))
    );
}
