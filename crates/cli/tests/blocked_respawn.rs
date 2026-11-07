//! #639: queue blockers hold every spawn, a `turn blocked` is never a no-push failure, an extra round for a lane that
//! never pushed is no review FAIL, a lessons issue with no PR closes on `done --no-change`, and a GitHub-closed issue
//! reads closed after one `check`. Every test runs as #12's impl lane (`HERDSMAN_LANE` is process-wide).
mod common;
use common::*;
use herdsman::events::{append, fold, read, Event};
use serde_json::json;

const LANE: &str = "sendmeter-impl-12";

fn ev(c: &herdsman::Ctx, ts: u64, n: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(n), kind, d)).unwrap();
}

fn at(c: &herdsman::Ctx, secs: u64) -> herdsman::Ctx {
    herdsman::Ctx {
        now: c.now + secs,
        ..c.clone()
    }
}

fn spawn(c: &herdsman::Ctx, ts: u64) {
    let wt = c.worktrees.join("sendmeter/issue-12");
    let d = json!({"role": "impl", "agent": LANE, "pane": "w9:p1", "workspace": "w9", "worktree": wt,
        "branch": "issue-12"});
    ev(c, ts, 12, "spawn", d);
}

/// #12's impl lane spawned at ts 10, its pane gone (it ended), no PR; `title` = the issue's.
fn lane(name: &str, title: &str) -> (herdsman::Ctx, Fake) {
    std::env::set_var("HERDSMAN_LANE", "sendmeter:12:impl");
    let c = ctx(&tmpdir(name), CONFIG);
    std::fs::create_dir_all(c.worktrees.join("sendmeter/issue-12")).unwrap();
    spawn(&c, 10);
    let r = Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", &json!({"title": title, "body": "b"}).to_string())
        .reply("gh issue close", "")
        .reply("herdr worktree", r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#)
        .reply(
            "herdr pane list",
            r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#,
        )
        .reply("herdr pane", "{}")
        .reply("herdr agent start", "{}")
        .reply("herdr agent prompt", "{}")
        .reply("herdr agent wait", "{}")
        .reply("git -C", "");
    (c, r)
}

fn ok(c: &herdsman::Ctx, r: &Fake, args: &[&str]) -> String {
    let (code, out) = run(c, r, args);
    assert_eq!(code, 0, "{args:?}: {out}");
    out
}

/// `herdsman drive`: (output, `driven` steps logged by this pass).
fn drive(c: &herdsman::Ctx, r: &Fake) -> (String, Vec<String>) {
    let before = read(&c.state).unwrap().len();
    let out = ok(c, r, &["drive"]);
    let new = read(&c.state).unwrap().split_off(before);
    let steps = new.iter().filter(|e| e.kind == "driven");
    (out, steps.map(|e| e.str("step").unwrap().to_string()).collect())
}

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

fn state(c: &herdsman::Ctx) -> String {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)]
        .state
        .clone()
}

fn lane_ends(c: &herdsman::Ctx, ts: u64) {
    let d = json!({"code": "LANE_ENDS", "reason": "lane keeps ending without a push", "agent": LANE});
    ev(c, ts, 12, "needs-human", d);
}

#[test]
fn a_blocked_issue_given_an_extra_round_is_never_respawned_and_waits_for_its_blockers() {
    let (c, r) = lane("blocked-fix", "t");
    ok(&c, &r, &["queue", "sendmeter", "add", "12"]);
    ok(
        &c,
        &r,
        &["queue", "sendmeter", "block", "12", "--on", "11", "--on", "13"],
    );
    // cooee #29's shape: a respawn after the block (its hold ends there), the lane ended without a push, the owner
    // gave it another round.
    let t = c.now;
    spawn(&c, t);
    lane_ends(&c, t);
    ev(&c, t, 12, "round-extended", json!({"by": "owner", "spawn": true}));
    let w = ok(&c, &r, &["waiting", "sendmeter"]);
    assert!(w.contains("blocked by #11, #13"), "{w}");
    assert!(!w.contains("review FAIL"), "{w}");
    let (out, steps) = drive(&c, &r);
    assert!(steps.is_empty(), "{out}");
    assert!(!r.called("herdr agent start"), "{:?}", r.calls.borrow());
    assert!(kinds(&c, "respawn").is_empty());
    let ls = ok(&c, &r, &["queue", "sendmeter", "ls"]);
    assert!(ls.contains("#12    queued  blocked by #11, #13"), "{ls}");
    drive(&at(&c, 7200), &r);
    assert!(!r.called("herdr agent start"), "never while a blocker is open");
    // Both blockers merged: the queue starts it as a first spawn.
    for n in [11, 13] {
        ev(
            &c,
            t,
            n,
            "merged",
            json!({"pr": "40", "merge_sha": "abc", "into": "staging"}),
        );
    }
    let (out, steps) = drive(&at(&c, 7300), &r);
    assert_eq!(steps, ["start"], "{out}");
}

#[test]
fn two_blocked_turns_in_a_row_are_owner_cards_never_lane_ends() {
    let (c, r) = lane("blocked-twice", "t");
    ok(&c, &r, &["turn", "blocked", "--why", "waits on #11"]);
    let (out, steps) = drive(&c, &r);
    assert!(steps.is_empty(), "{out}");
    assert!(
        out.contains("sendmeter #12 needs-human: lane blocked: waits on #11"),
        "{out}"
    );
    // The owner gives it another round; its new lane ends blocked again.
    let c2 = at(&c, 100);
    ev(&c2, c2.now, 12, "round-extended", json!({"by": "owner", "spawn": true}));
    spawn(&c2, c2.now);
    ok(&c2, &r, &["turn", "blocked", "--why", "still waits on #11"]);
    let (out, steps) = drive(&at(&c, 300), &r);
    assert!(steps.is_empty(), "{out}");
    drive(&at(&c, 7200), &r);
    let codes: Vec<_> = kinds(&c, "needs-human")
        .iter()
        .map(|e| e.str("code").map(String::from))
        .collect();
    assert_eq!(codes, [Some("LANE_BLOCKED".into()), Some("LANE_BLOCKED".into())]);
    assert!(kinds(&c, "respawn").is_empty());
    assert!(!r.called("herdr agent start"));
}

#[test]
fn an_extra_round_for_a_lane_that_never_pushed_is_no_review_fail() {
    // The root cause of cooee #29's "review FAIL" with no PR: `extend` on a build lane that never reached review.
    let (c, r) = lane("extend-impl", "t");
    lane_ends(&c, 20);
    ev(&c, 30, 12, "round-extended", json!({"by": "owner", "spawn": true}));
    assert_eq!(state(&c), "impl");
    let w = ok(&c, &r, &["waiting", "sendmeter"]);
    assert!(!w.contains("review FAIL") && !w.contains("fix round"), "{w}");
    assert!(w.contains("lane gone, no push: respawning impl"), "{w}");
}

#[test]
fn a_lessons_issue_done_with_no_change_and_no_pr_closes() {
    // berth #22's shape: drive's queue start spawned the lessons issue as `impl`, not `lesson`.
    let (c, r) = lane("lesson-close", "lessons: sendmeter week of 2026-11-05");
    ok(&c, &r, &["brief", "sendmeter", "12"]);
    let out = ok(&c, &r, &["done", "--no-change", "no repeated class", "sendmeter", "12"]);
    assert!(out.contains("closed sendmeter#12: no repeated class"), "{out}");
    assert!(r.called("gh issue close 12 -R jirathip-dev/sendmeter --comment no repeated class"));
    assert_eq!(state(&c), "closed");
    let (out, steps) = drive(&at(&c, 7200), &r);
    assert!(steps.is_empty(), "{out}");
    assert!(!r.called("herdr agent start"));
    assert!(kinds(&c, "needs-human").is_empty());
}

#[test]
fn an_issue_closed_on_github_reads_closed_after_one_check_and_drive_leaves_it() {
    let (c, r) = lane("gh-closed", "t");
    let r = r.reply(
        "gh issue list -R jirathip-dev/sendmeter --state closed",
        r#"[{"number": 12}]"#,
    );
    let ask = json!({"id": "q1", "agent": LANE, "kind": "owner", "question": "which?", "options": ["a", "b"]});
    ev(&c, 15, 12, "needs-decision", ask);
    lane_ends(&c, 20);
    ok(&c, &r, &["check"]);
    assert_eq!(state(&c), "closed");
    assert_eq!(kinds(&c, "closed-other").len(), 1);
    let asks = ok(&c, &r, &["asks", "sendmeter"]);
    assert!(!asks.contains("q1"), "{asks}");
    ev(&c, 40, 12, "round-extended", json!({"by": "owner", "spawn": true}));
    let (out, steps) = drive(&at(&c, 60), &r);
    assert!(steps.is_empty(), "{out}");
    assert!(!r.called("herdr agent start"));
    ok(&at(&c, 120), &r, &["check"]);
    assert_eq!(kinds(&c, "closed-other").len(), 1, "logged once");
}
