mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::json;
use std::path::PathBuf;

const REV: &str = "sendmeter-rev-12-r1";

fn sha() -> String {
    format!("b4e191c{}", "0".repeat(33))
}

fn ev(c: &drover::Ctx, ts: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
}

fn setup(name: &str) -> (drover::Ctx, FakeShed, PathBuf) {
    let c = ctx(&tmpdir(name), CONFIG);
    let sh = FakeShed::attached(&c);
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    ev(
        &c,
        10,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12", "workspace": "",
               "runtime": "shed", "worktree": wt, "branch": "issue-12"}),
    );
    let rev = c.worktrees.join("sendmeter/rev-12-r1");
    std::fs::create_dir_all(&rev).unwrap();
    ev(
        &c,
        20,
        "spawn",
        json!({"role": "rev", "agent": REV, "pane": REV, "workspace": "", "runtime": "shed",
               "worktree": rev, "branch": "rev-12-r1", "sha": sha()}),
    );
    (c, sh, rev)
}

fn fake() -> Fake {
    let pr = json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": sha(),
        "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}]});
    Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("gh pr view issue-12", &pr.to_string())
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{}\n", sha()),
        )
        .reply("git -C", "")
}

fn at(c: &drover::Ctx, pass: u64) -> drover::Ctx {
    drover::Ctx {
        now: c.now + pass * 300,
        ..c.clone()
    }
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == kind)
        .cloned()
        .collect()
}

fn rev_spawns(c: &drover::Ctx) -> Vec<Event> {
    kinds(c, "spawn")
        .into_iter()
        .filter(|e| e.str("role") == Some("rev"))
        .collect()
}

fn state(c: &drover::Ctx) -> String {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)]
        .state
        .clone()
}

fn pass(c: &drover::Ctx, r: &Fake) -> String {
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    out
}

#[test]
fn a_reviewer_that_ended_its_turn_without_verdict_md_is_reviewed_again_within_three_passes() {
    let (c, sh, rev) = setup("rev-no-verdict-heal");
    sh.lane(REV, "idle", &rev.display().to_string());
    let round = state(&c);
    let mut log = String::new();
    for p in 1..=3 {
        log += &pass(&at(&c, p), &fake());
        if rev_spawns(&c).len() > 1 {
            break;
        }
    }
    let spawns = rev_spawns(&c);
    assert_eq!(spawns.len(), 2, "the review is restarted: {log}");
    assert_eq!(state(&c), round, "same round: {log}");
    let heals = kinds(&c, "self-heal");
    assert_eq!(heals.len(), 1, "logged as a heal: {log}");
    assert!(
        heals[0].str("why").unwrap_or_default().contains("verdict.md"),
        "{heals:?}"
    );

    let wt = spawns[1].str("worktree").unwrap().to_string();
    let agent = spawns[1].str("agent").unwrap().to_string();
    sh.lane(&agent, "idle", &wt);
    std::fs::write(
        format!("{wt}/verdict.md"),
        format!("PASS\n\nReviewed head: {}\n", sha()),
    )
    .unwrap();
    let out = pass(&at(&c, 4), &fake());
    let v = kinds(&c, "verdict");
    assert_eq!(v.len(), 1, "the restarted lane's verdict is picked up: {out}");
    assert_eq!(v[0].str("verdict"), Some("PASS"));
}

#[test]
fn a_working_reviewer_without_verdict_md_is_no_failed_pickup() {
    let (c, sh, rev) = setup("rev-no-verdict-working");
    sh.lane(REV, "working", &rev.display().to_string());
    let mut log = String::new();
    for p in 1..=4 {
        log += &pass(&at(&c, p), &fake());
    }
    assert!(kinds(&c, "verdict-pickup-failed").is_empty(), "{log}");
    assert!(!log.contains("verdict pickup failed"), "{log}");
    assert_eq!(rev_spawns(&c).len(), 1, "{log}");
    let (_, waiting) = run(&at(&c, 4), &fake(), &["waiting", "sendmeter"]);
    assert!(!waiting.contains("verdict pickup failed"), "{waiting}");
}

#[test]
fn a_second_reviewer_ending_without_verdict_md_is_one_owner_card_not_a_third_review() {
    let (c, sh, _) = setup("rev-no-verdict-card");
    let mut log = String::new();
    for p in 1..=3 {
        log += &pass(&at(&c, p), &fake());
        if rev_spawns(&c).len() > 1 {
            break;
        }
    }
    let spawns = rev_spawns(&c);
    assert_eq!(spawns.len(), 2, "{log}");
    let agent = spawns[1].str("agent").unwrap().to_string();
    let wt = spawns[1].str("worktree").unwrap().to_string();
    sh.exit(&agent);
    for p in 4..=8 {
        log += &pass(&at(&c, p), &fake());
    }
    assert_eq!(rev_spawns(&c).len(), 2, "no third review: {log}");
    let cards = kinds(&c, "needs-human");
    assert_eq!(cards.len(), 1, "one owner card: {log}");
    let reason = cards[0].str("reason").unwrap_or_default();
    assert!(reason.contains(&format!("{wt}/verdict.md")), "{reason}");
}

#[test]
fn after_a_supersede_waiting_shows_the_new_lane_not_the_old_ones_miss() {
    let (c, sh, rev) = setup("rev-no-verdict-supersede");
    ev(
        &c,
        30,
        "promise",
        json!({"kind": "next-pass", "reason": "verdict pickup failed", "deadline": 600, "state": state(&c), "head": sha()}),
    );
    ev(
        &c,
        40,
        "verdict-pickup-failed",
        json!({"agent": REV, "why": format!("cannot read verdict file {}/verdict.md", rev.display())}),
    );
    ev(
        &c,
        700,
        "promise-missed",
        json!({"kind": "next-pass", "promised": "verdict pickup failed", "age": 670, "why": "verdict pickup failed"}),
    );
    let next = c.worktrees.join("sendmeter/rev-12-r1b");
    std::fs::create_dir_all(&next).unwrap();
    let agent = "sendmeter-rev-12-r1b";
    ev(
        &c,
        c.now - 120,
        "spawn",
        json!({"role": "rev", "agent": agent, "pane": agent, "workspace": "", "runtime": "shed",
               "worktree": next, "branch": "rev-12-r1", "sha": sha()}),
    );
    sh.lane(agent, "idle", &next.display().to_string());
    let (_, waiting) = run(&c, &fake(), &["waiting", "sendmeter"]);
    assert!(!waiting.contains("missed"), "{waiting}");
    assert!(!waiting.contains("verdict pickup failed"), "{waiting}");
    assert!(waiting.contains("(2m)"), "the new lane's age: {waiting}");
}
