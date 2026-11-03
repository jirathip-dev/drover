//! #92 `herdsman drive`: each step once per pass, a second pass a no-op; paused, HOST_BUSY and the lock.
mod common;
use common::*;
use herdsman::events::{append, fold, read, Event};
use serde_json::json;
use std::path::PathBuf;

const WS: &str = r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#;
const PANES: &str = r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#;

fn sha(p: &str) -> String {
    format!("{p}{}", "0".repeat(40 - p.len()))
}

fn setup(name: &str) -> herdsman::Ctx {
    ctx(&tmpdir(name), CONFIG)
}

fn ev(c: &herdsman::Ctx, ts: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
}

/// A lane of #12 logged as spawned, in a real worktree dir.
fn lane(c: &herdsman::Ctx, ts: u64, role: &str, agent: &str, dir: &str) -> PathBuf {
    let wt = c.worktrees.join("sendmeter").join(dir);
    std::fs::create_dir_all(&wt).unwrap();
    let d = json!({"role": role, "agent": agent, "pane": format!("w9:{agent}"), "workspace": "w9", "worktree": wt,
        "branch": "issue-12"});
    ev(c, ts, "spawn", d);
    wt
}

fn pr(head: &str, ci: &str) -> String {
    let check = match ci {
        "green" => json!({"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}),
        "red" => json!({"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "FAILURE",
            "detailsUrl": "https://github.com/jirathip-dev/sendmeter/actions/runs/777/job/1"}),
        _ => json!({"__typename": "CheckRun", "name": "ci", "status": "IN_PROGRESS"}),
    };
    json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": head, "statusCheckRollup": [check]})
        .to_string()
}

/// A Fake on which spawns succeed and the host is calm; `agents` = the herdr agent list.
/// #285: integration's protection read (cached) as strict, so a behind ready PR is caught up.
fn strict(c: &herdsman::Ctx) {
    let cache = c.state.with_file_name("cache");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(
        cache.join("strict-sendmeter-0.json"),
        json!({"ts": c.now, "strict": true}).to_string(),
    )
    .unwrap();
}

fn fake(agents: &str) -> Fake {
    Fake::default()
        .reply("herdr agent list", agents)
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("herdr worktree", WS)
        .reply("herdr pane list", PANES)
        .reply("herdr pane run", "{}")
        .reply("herdr agent start", "{}")
        .reply("herdr pane", "{}")
        .reply("herdr agent prompt", "{}")
        .reply("herdr agent wait", "{}")
}

/// `herdsman drive`: (output, `driven` steps logged by this pass).
fn drive(c: &herdsman::Ctx, r: &Fake) -> (String, Vec<String>) {
    let before = read(&c.state).unwrap().len();
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    let new = read(&c.state).unwrap().split_off(before);
    (
        out,
        new.iter()
            .filter(|e| e.kind == "driven")
            .map(|e| e.str("step").unwrap().to_string())
            .collect(),
    )
}

/// The second pass changes nothing: no output, no event but its own `drive-pass` record (#166).
fn idle(c: &herdsman::Ctx, r: &Fake) {
    let acts = || {
        read(&c.state)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind != "drive-pass")
            .count()
    };
    let before = acts();
    assert_eq!(drive(c, r).0, "");
    assert_eq!(acts(), before);
}

fn state(c: &herdsman::Ctx) -> String {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)]
        .state
        .clone()
}

#[test]
fn start_spawns_the_next_queued_issue() {
    let c = setup("drive-start");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let r = fake(NO_AGENTS).reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["start"], "{out}");
    assert!(out.starts_with("sendmeter #12 start: "), "{out}");
    assert!(r.called("herdr agent start sendmeter-impl-12"));
    assert_eq!(state(&c), "impl");
    let wt = c.worktrees.join("sendmeter/issue-12");
    idle(
        &c,
        &fake(&agents(&[(
            "sendmeter-impl-12",
            "working",
            &wt.to_string_lossy(),
            "w9:p1",
        )])),
    );
}

#[test]
fn review_spawns_rev_at_the_pushed_head() {
    let c = setup("drive-review");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("abc");
    let r = fake(&agents(&[(
        "sendmeter-impl-12",
        "idle",
        &wt.to_string_lossy(),
        "w9:p1",
    )]))
    .reply("gh pr view issue-12", &pr(&head, "pending"))
    .reply(&format!("git -C {} log", wt.display()), &format!("{head} 20"))
    .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["review"], "{out}");
    assert!(r.called(
        "herdr worktree create --cwd /tmp/does-not-matter/sendmeter --branch rev-12-r1 --base origin/issue-12"
    ));
    assert_eq!(state(&c), "review(1)");
    // The reviewer's pane is live (idle, no verdict yet): nothing to do.
    let rev = c.worktrees.join("sendmeter/rev-12-r1");
    let r = fake(&agents(&[
        ("sendmeter-impl-12", "idle", &wt.to_string_lossy(), "w9:p1"),
        ("sendmeter-rev-12-r1", "idle", &rev.to_string_lossy(), "w9:p2"),
    ]))
    .reply("gh pr view issue-12", &pr(&head, "pending"))
    .reply(&format!("git -C {} log", wt.display()), &format!("{head} 20"))
    .reply("git -C", "");
    idle(&c, &r);
}

#[test]
fn a_written_verdict_is_recorded_at_the_head_then_pending_ci_waits() {
    let c = setup("drive-verdict");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let rev = lane(&c, 20, "rev", "sendmeter-rev-12-r1", "rev-12-r1");
    std::fs::write(rev.join("verdict.md"), "PASS\nall good\n").unwrap();
    let head = sha("abc");
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&head, "pending"))
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply(&format!("git -C {} log", rev.display()), &format!("{head} 20"))
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["verdict"], "{out}");
    let all = read(&c.state).unwrap();
    let v = all.iter().find(|e| e.kind == "verdict").unwrap();
    assert_eq!((v.str("verdict"), v.str("sha")), (Some("PASS"), Some(head.as_str())));
    // PASS with CI pending: drive waits.
    idle(&c, &r);
    assert!(!r.called("gh pr merge"));
}

#[test]
fn a_head_moved_during_review_gets_a_fresh_reviewer_not_the_old_verdict() {
    let c = setup("drive-moved");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let rev = c.worktrees.join("sendmeter/rev-12-r1");
    std::fs::create_dir_all(&rev).unwrap();
    ev(
        &c,
        20,
        "spawn",
        json!({"role": "rev", "agent": "sendmeter-rev-12-r1", "pane": "w9:r", "workspace": "w9",
        "worktree": rev, "branch": "rev-12-r1", "sha": sha("0d")}),
    );
    std::fs::write(rev.join("verdict.md"), "PASS\nall good\n").unwrap();
    let head = sha("bee");
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&head, "pending"))
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply(
            &format!("git -C {} rev-parse HEAD", rev.display()),
            &format!("{head}\n"),
        )
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["review"], "{out}");
    let all = read(&c.state).unwrap();
    assert!(
        !all.iter().any(|e| e.kind == "verdict"),
        "the old head's verdict is not recorded for the new head"
    );
    assert!(r.called(&format!("git -C {} reset --hard origin/issue-12", rev.display())));
    assert!(!rev.join("verdict.md").exists(), "the stale verdict is gone");
    let spawn = all.iter().rev().find(|e| e.kind == "spawn").unwrap();
    assert_eq!(
        (spawn.str("role"), spawn.str("sha")),
        (Some("rev"), Some(head.as_str())),
        "logged at the new head"
    );
    // The fresh reviewer's verdict is recorded at the head it was spawned at.
    std::fs::write(rev.join("verdict.md"), "PASS\n").unwrap();
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["verdict"], "{out}");
    let v = read(&c.state)
        .unwrap()
        .into_iter()
        .find(|e| e.kind == "verdict")
        .expect(&out);
    assert_eq!(v.str("sha"), Some(head.as_str()));
}

#[test]
fn pass_and_green_ci_merges() {
    let c = setup("drive-merge");
    lane(&c, 10, "impl", "sendmeter-impl-12", "gone-12");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/gone-12")).unwrap();
    let head = sha("abc");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
    );
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&head, "green"))
        .reply("gh pr merge 100", "")
        .reply("gh pr view 100", r#"{"mergeCommit":{"oid":"m3rg3"}}"#)
        .reply("gh issue", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["merge"], "{out}");
    assert!(r.called(&format!(
        "gh pr merge 100 -R jirathip-dev/sendmeter --merge --match-head-commit {head}"
    )));
    assert_eq!(state(&c), "merged");
    idle(&c, &r);
}

#[test]
fn a_blocked_ci_not_green_item_merges_once_ci_turns_green() {
    let c = setup("drive-blocked-retry");
    lane(&c, 10, "impl", "sendmeter-impl-12", "gone-12");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/gone-12")).unwrap();
    let head = sha("abc");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
    );
    ev(
        &c,
        40,
        "blocked",
        json!({"code": "CI_NOT_GREEN", "reason": "no checks reported"}),
    );
    assert_eq!(state(&c), "blocked");
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&head, "green"))
        .reply("gh pr merge 100", "")
        .reply("gh pr view 100", r#"{"mergeCommit":{"oid":"m3rg3"}}"#)
        .reply("gh issue", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["merge"], "{out}");
    assert_eq!(state(&c), "merged");
}

#[test]
fn a_fail_starts_a_fix_round_with_the_findings_verbatim() {
    let c = setup("drive-fix");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let body = "FAIL\n1. blocking: src/x.rs:3 off by one";
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "FAIL", "sha": sha("abc"), "body": body}),
    );
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&sha("abc"), "green"))
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["fix"], "{out}");
    let brief = std::fs::read_to_string(wt.join(".brief.md")).unwrap();
    assert!(brief.contains(body), "{brief}");
    assert!(brief.contains("If a finding is wrong or out of scope, say so in your report with file:line proof"));
    assert_eq!(state(&c), "fix(1)");
    idle(
        &c,
        &fake(&agents(&[(
            "sendmeter-impl-12",
            "working",
            &wt.to_string_lossy(),
            "w9:p1",
        )])),
    );
}

#[test]
fn the_third_fail_stops_at_needs_human_with_the_findings() {
    let c = setup("drive-limit");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(
        &c,
        11,
        "verdict",
        json!({"verdict": "FAIL", "sha": sha("1"), "body": "FAIL"}),
    );
    ev(
        &c,
        12,
        "verdict",
        json!({"verdict": "FAIL", "sha": sha("2"), "body": "FAIL"}),
    );
    let rev = lane(&c, 20, "rev", "sendmeter-rev-12-r3", "rev-12-r3");
    std::fs::write(rev.join("verdict.md"), "FAIL\nsrc/y.rs:9 still wrong\n").unwrap();
    let head = sha("3");
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&head, "green"))
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply(&format!("git -C {} log", rev.display()), &format!("{head} 20"))
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["verdict"], "{out}");
    assert_eq!(state(&c), "needs-human");
    let all = read(&c.state).unwrap();
    let nh = all.iter().find(|e| e.kind == "needs-human").unwrap();
    assert!(nh.str("findings").unwrap().contains("src/y.rs:9 still wrong"));
    idle(&c, &r);
    assert!(!r.called("herdr agent start"), "no fix lane after the limit");
}

#[test]
fn red_ci_after_pass_is_a_fail_with_the_log_tail() {
    let c = setup("drive-red");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("abc");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
    );
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&head, "red"))
        .reply(
            "gh run view 777 -R jirathip-dev/sendmeter --log-failed",
            "test x ... FAILED\nGH_TOKEN=ghp_abcdef123456\nerror: 1 test failed\n",
        )
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["ci-red"], "{out}");
    let all = read(&c.state).unwrap();
    let v = all.iter().rev().find(|e| e.kind == "verdict").unwrap();
    assert_eq!(v.str("verdict"), Some("FAIL"));
    assert!(
        v.str("body").unwrap().contains("error: 1 test failed"),
        "{:?}",
        v.str("body")
    );
    assert!(
        !v.str("body").unwrap().contains("ghp_abcdef"),
        "the log tail is masked: {:?}",
        v.str("body")
    );
    let st = &fold(&all)[&("sendmeter".to_string(), 12)];
    assert_eq!((st.state.as_str(), st.rounds), ("fix(1)", 1), "same head: no new round");
}

#[test]
fn clean_reclaims_merged_lanes() {
    let c = setup("drive-clean");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(&c, 30, "merged", json!({"pr": "100"}));
    let r = fake(NO_AGENTS)
        .reply(&format!("git -C {} rev-list", wt.display()), "0\n")
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["clean"], "{out}");
    assert!(r.called("herdr worktree remove --workspace w9"));
    std::fs::remove_dir_all(&wt).unwrap(); // herdr removed it
    idle(&c, &r);
}

/// #285: PR #100 of #12 as GitHub shows it after it left the open set (`MERGED` / `CLOSED`).
fn gone_pr(state: &str) -> String {
    let mut v: serde_json::Value = serde_json::from_str(&pr(&sha("a1"), "green")).unwrap();
    v["state"] = json!(state);
    if state == "MERGED" {
        (v["mergeCommit"], v["mergedAt"]) = (json!({"oid": "3c05c00a"}), json!("2026-11-03T06:37:00Z"));
    }
    v.to_string()
}

#[test]
fn a_pr_merged_outside_herdsman_is_logged_merged_and_its_lanes_cleaned() {
    let c = setup("drive-external-merge");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(
        &c,
        20,
        "verdict",
        json!({"verdict": "PASS", "sha": sha("a1"), "body": "PASS"}),
    );
    assert_eq!(state(&c), "ready");
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &gone_pr("MERGED"))
        .reply(&format!("git -C {} rev-list", wt.display()), "0\n")
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert!(out.contains("sendmeter #12 merged outside herdsman (PR #100)"), "{out}");
    let ev = read(&c.state).unwrap();
    let m = ev.iter().find(|e| e.is("merged", "sendmeter", 12)).expect("merged");
    assert_eq!(
        (m.str("by"), m.str("merge_sha"), m.str("pr"), m.str("into")),
        (Some("external"), Some("3c05c00a"), Some("100"), Some("staging"))
    );
    assert_eq!(m.str("merged_at"), Some("2026-11-03T06:37:00Z"));
    assert_eq!(state(&c), "merged");
    assert!(!r.called("gh pr merge") && !r.called("gh issue comment") && !r.called("gh issue close"));
    // Its lanes go the normal clean way in the same slow pass; the card is gone.
    assert_eq!(steps, ["clean"], "{out}");
    assert!(r.called("herdr worktree remove --workspace w9"));
    std::fs::remove_dir_all(&wt).unwrap(); // herdr removed it
    assert!(!run(&c, &r, &["waiting", "sendmeter"]).1.contains("#12"));
    // The repair is once: a later pass logs nothing more.
    idle(&c, &r);
    assert_eq!(read(&c.state).unwrap().iter().filter(|e| e.kind == "merged").count(), 1);
}

#[test]
fn a_pr_closed_outside_herdsman_closes_or_blocks_the_issue_once() {
    for (issues, want) in [(r#"[{"number":12}]"#, "closed"), ("[]", "blocked")] {
        let c = setup(&format!("drive-external-{want}"));
        lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
        ev(
            &c,
            20,
            "verdict",
            json!({"verdict": "PASS", "sha": sha("a1"), "body": "PASS"}),
        );
        let r = fake(NO_AGENTS)
            .reply("gh pr view issue-12", &gone_pr("CLOSED"))
            .reply("gh issue list", issues)
            .reply("git -C", "");
        let (out, _) = drive(&c, &r);
        assert!(
            out.contains(&format!("sendmeter #12 {want} outside herdsman (PR #100)")),
            "{out}"
        );
        assert_eq!(state(&c), want);
        let e = read(&c.state)
            .unwrap()
            .into_iter()
            .rfind(|e| e.is(want, "sendmeter", 12))
            .unwrap();
        if want == "blocked" {
            assert_eq!(
                (e.str("code"), e.str("reason")),
                (Some("PR_CLOSED"), Some("PR closed outside herdsman"))
            );
        } else {
            assert_eq!(e.str("by"), Some("external"));
        }
        drive(&c, &r);
        assert_eq!(read(&c.state).unwrap().iter().filter(|e| e.kind == want).count(), 1);
        assert!(!r.called("gh issue comment"));
    }
}

#[test]
fn paused_fleets_and_host_busy_are_no_ops() {
    let c = setup("drive-paused");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    assert_eq!(run(&c, &Fake::default(), &["pause", "sendmeter"]).0, 0);
    let r = fake(NO_AGENTS).reply("git -C", "");
    idle(&c, &r);
    assert!(!r.called("herdr"), "{:?}", r.calls.borrow());
    assert_eq!(
        herdsman::drive::last_pass(&c, "sendmeter"),
        None,
        "a paused fleet had no pass"
    );

    let c = setup("drive-busy");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let busy = Fake::default().once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "4\n");
    let busy = Fake {
        replies: fake(NO_AGENTS).reply("git -C", "").replies,
        ..busy
    };
    let (out, steps) = drive(&c, &busy);
    assert_eq!(steps, ["start"]);
    assert!(out.contains("REFUSED HOST_BUSY"), "{out}");
    assert!(!busy.called("herdr worktree"));
    // Retried next pass, logged and printed once per hour.
    let busy = Fake {
        replies: fake(NO_AGENTS).reply("git -C", "").replies,
        ..Default::default()
    }
    .once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "4\n");
    let (out, steps) = drive(&c, &busy);
    assert!(out.is_empty() && steps.is_empty(), "{out}");
    assert_eq!(
        busy.count("sysctl -n kern.memorystatus_vm_pressure_level"),
        1,
        "retried"
    );
    assert_eq!(state(&c), "queued");
}

#[test]
fn the_lock_keeps_passes_apart_and_a_stale_one_is_taken_over() {
    let c = setup("drive-lock");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let lock = c.state.parent().unwrap().join("drive.lock");
    std::fs::write(&lock, "").unwrap();
    let r = fake(NO_AGENTS).reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert!(out.contains("another pass holds") && steps.is_empty(), "{out}");
    assert!(!r.called("herdr"));
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    std::fs::File::options()
        .write(true)
        .open(&lock)
        .unwrap()
        .set_modified(old)
        .unwrap();
    assert_eq!(drive(&c, &r).1, ["start"]);
    assert!(!lock.exists(), "released after the pass");
    assert_eq!(herdsman::drive::last_pass(&c, "sendmeter"), Some(c.now));
}

/// #168: a lock with a dead pid is taken over at once, a live pid's is respected; the pass's pid is written.
#[test]
fn a_dead_pids_lock_is_taken_over_at_once_and_a_live_ones_respected() {
    let c = setup("drive-pid");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let lock = c.state.parent().unwrap().join("drive.lock");
    std::fs::write(&lock, "4242").unwrap();
    let live = fake(NO_AGENTS).reply("git -C", "").reply("kill -0 4242", "");
    let (out, steps) = drive(&c, &live);
    assert!(out.contains("another pass holds") && steps.is_empty(), "{out}");
    assert_eq!(
        std::fs::read_to_string(&lock).unwrap(),
        "4242",
        "a live pass's lock stays"
    );
    let dead = fake(NO_AGENTS).reply("git -C", "").once("kill -0 4242", 1, "");
    let gone = || assert!(!lock.exists(), "released after the pass");
    assert_eq!(drive(&c, &dead).1, ["start"]);
    gone();
    // The guard releases the lock when the pass fails, too.
    std::fs::write(&lock, "4242").unwrap();
    let bad = Fake::default().once("kill -0 4242", 1, "");
    assert!(run(&c, &bad, &["drive"]).0 != 0);
    gone();
}

#[test]
fn drive_now_on_the_board_is_paired_only_and_toasts_its_line() {
    let c = setup("drive-web");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let r = fake(NO_AGENTS).reply("git -C", "");
    let s = herdsman::serve::Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    let get = |s: &herdsman::serve::Server| {
        let res = s.respond("GET / HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n\r\n", c.now);
        String::from_utf8_lossy(&res).into_owned()
    };
    let page = get(&s);
    assert!(
        page.contains("<button>Drive now</button>")
            && page.contains("<details class=\"dp a\"><summary><i>●</i><span>drive · no pass yet</span>")
    );
    let i = page.find("name=mtime value=").unwrap() + 17;
    let mtime: String = page[i..].chars().take_while(char::is_ascii_digit).collect();
    let body = format!("op=drive&fleet=sendmeter&mtime={mtime}");
    let post = |host: &str, extra: &str| {
        let req = format!(
            "POST / HTTP/1.1\r\nHost: {host}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        String::from_utf8_lossy(&s.respond(&req, c.now)).into_owned()
    };
    assert!(post("127.0.0.1:7777", "").starts_with("HTTP/1.1 401"));
    let cookie = pair(&s, &c, "", c.now);
    assert!(post("evil.example", &cookie).starts_with("HTTP/1.1 403"));
    assert!(!r.called("herdr agent start"));
    let res = post("127.0.0.1:7777", &cookie);
    assert!(
        res.starts_with("HTTP/1.1 303") && res.contains("sendmeter%3A%20started%20%2312"),
        "{res}"
    );
    assert!(r.called("herdr agent start sendmeter-impl-12"));
    assert!(
        get(&s).contains("<details class=\"dp g\"><summary><i>●</i><span>drive · 0s ago · started 1 · merged 0</span>")
    );
}

/// #140: a drive pass's lines as one toast, per action, then the refusals by code.
#[test]
fn the_drive_summary_groups_per_action() {
    let out = "sendmeter #2752 merge: merged #2752 into staging\nsendmeter #2755 start: spawned sendmeter-impl-2755\n\
        sendmeter #2781 merge: merged\nsendmeter #9 review: spawned\nsendmeter #2756 start: REFUSED SERIAL_BUSY: x\n\
        sendmeter #2757 start: REFUSED HOST_BUSY: y\nsendmeter #3 fix: error: boom\nsendmeter #4 start: REFUSED HOST_BUSY: z\n\
        sendmeter clean: removed 1\n";
    assert_eq!(
        herdsman::drive::summary("sendmeter", out),
        "sendmeter: clean · merged #2752, #2781 · reviewed #9 · started #2755 · 4 skipped (HOST_BUSY, SERIAL_BUSY, error)"
    );
    assert_eq!(herdsman::drive::summary("sendmeter", ""), "sendmeter: nothing due");
}

/// Sleeps 3 s on each agent start: a slow drive pass.
struct Slow(Fake);
impl herdsman::runner::Runner for Slow {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<herdsman::runner::Out> {
        if prog == "herdr" && args.join(" ").starts_with("agent start") {
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
        self.0.run(prog, args)
    }
}

/// #140: `[Drive now]` replies at once and runs on the job thread; a second tap is DRIVE_RUNNING, and the
/// board (a thread per connection) answers a GET meanwhile; the result reaches `/live` as a toast.
#[test]
fn drive_now_runs_in_the_background_and_the_board_keeps_answering() {
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};
    let c = setup("drive-bg");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let cookie = pair(
        &herdsman::serve::Server::new(&c, &Fake::default(), Some("127.0.0.1:7777".into())),
        &c,
        "",
        // #288: a pairing lasts a year after last use; `serve_on` reads the real clock.
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    );
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || herdsman::serve::serve_on(&c2, &Slow(fake(NO_AGENTS).reply("git -C", "")), l, false));
    let send = |req: String| {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        s.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    };
    let get = |path: &str, extra: &str| send(format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n{extra}\r\n"));
    let page = get("/", &cookie);
    let i = page.find("name=mtime value=").unwrap() + 17;
    let mtime: String = page[i..].chars().take_while(char::is_ascii_digit).collect();
    let body = format!("op=drive&fleet=sendmeter&mtime={mtime}");
    let post = || {
        let t = Instant::now();
        let req = format!(
            "POST / HTTP/1.1\r\nHost: {addr}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        (send(req), t.elapsed())
    };
    let mut live = std::net::TcpStream::connect(addr).unwrap();
    live.write_all(format!("GET /live HTTP/1.1\r\nHost: {addr}\r\n{cookie}\r\n").as_bytes())
        .unwrap();
    let (res, took) = post();
    assert!(res.contains("drive%20started%20for%20sendmeter"), "{res}");
    assert!(took < Duration::from_millis(200), "the POST took {took:?}");
    let (res, _) = post();
    assert!(res.contains("REFUSED%20DRIVE%5FRUNNING"), "{res}");
    let t = Instant::now();
    assert!(get("/", "").starts_with("HTTP/1.1 200"));
    assert!(t.elapsed() < Duration::from_secs(1), "GET / waited {:?}", t.elapsed());
    // The pass ends: its summary reaches the board as a toast, and a new tap starts another pass.
    live.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let (mut seen, mut b) = (String::new(), [0u8; 65_536]);
    while !seen.contains("event: toast") {
        let n = live.read(&mut b).unwrap();
        assert!(n > 0, "the stream closed: {seen}");
        seen += &String::from_utf8_lossy(&b[..n]);
    }
    assert!(
        seen.contains("event: toast\ndata: \"sendmeter: started #12\""),
        "{seen}"
    );
    assert!(post().0.contains("drive%20started"), "the flag is cleared");
}

/// A PR view whose `mergeable` / `mergeStateStatus` are set (CI green).
fn stale_pr(head: &str, mergeable: &str, status: &str) -> String {
    let mut v: serde_json::Value = serde_json::from_str(&pr(head, "green")).unwrap();
    v["mergeable"] = json!(mergeable);
    v["mergeStateStatus"] = json!(status);
    v.to_string()
}

#[test]
fn a_conflicting_pr_after_pass_gets_update_rounds_then_needs_human() {
    let c = setup("drive-conflict");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    for (i, (v, p)) in [("FAIL", "a1"), ("FAIL", "b2")].into_iter().enumerate() {
        ev(
            &c,
            11 + i as u64,
            "verdict",
            json!({"verdict": v, "sha": sha(p), "body": v}),
        );
    }
    // #123: round 3 PASSes, then three update rounds PASS; none of them is a review round.
    for (k, p) in [(1, "c3"), (2, "d4"), (3, "e5"), (4, "f6")] {
        ev(
            &c,
            20 + k,
            "verdict",
            json!({"verdict": "PASS", "sha": sha(p), "body": "PASS"}),
        );
        herdsman::gh::forget_pr(&c, "sendmeter", 12);
        let r = fake(NO_AGENTS)
            .reply("gh pr view issue-12", &stale_pr(&sha(p), "CONFLICTING", "DIRTY"))
            .reply("git -C", "");
        let s = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
        assert_eq!((s.rounds, s.updates), (3, u32::try_from(k - 1).unwrap()));
        let (_, out) = run(&c, &r, &["waiting", "sendmeter"]);
        assert!(
            out.contains("PASS · conflicts with staging · catching up next pass (≤ 5 min)"),
            "{out}"
        );
        assert!(
            out.contains("herdsman spawn sendmeter 12 impl --supersede --update"),
            "{out}"
        );
        let (out, steps) = drive(&c, &r);
        assert_eq!(steps, ["conflict"], "{out}");
        if k < 4 {
            assert_eq!(state(&c), format!("fix(3)+u{k}"));
            let brief = std::fs::read_to_string(wt.join(".brief.md")).unwrap();
            assert!(brief.contains("Merge `origin/staging` into this branch"), "{brief}");
            assert!(!brief.contains("Review findings to fix"), "{brief}");
            lane(&c, 30 + k, "rev", "sendmeter-rev-12", "issue-12-rev");
            assert_eq!(state(&c), format!("review(3)+u{k}"));
        } else {
            assert!(out.contains("REFUSED UPDATE_LIMIT"), "{out}");
            assert_eq!(state(&c), "needs-human");
            assert_eq!(r.count("herdr agent start"), 0, "no fourth update lane");
            idle(&c, &r);
        }
    }
}

#[test]
fn a_behind_pr_after_pass_is_updated_the_same_way() {
    let c = setup("drive-behind");
    strict(&c);
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(
        &c,
        20,
        "verdict",
        json!({"verdict": "PASS", "sha": sha("a1"), "body": "PASS"}),
    );
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &stale_pr(&sha("a1"), "MERGEABLE", "BEHIND"))
        .reply("git -C", "");
    let (_, out) = run(&c, &r, &["waiting", "sendmeter"]);
    assert!(
        out.contains("PASS · behind staging · catching up next pass (≤ 5 min)"),
        "{out}"
    );
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["conflict"], "{out}");
    assert_eq!(state(&c), "fix(1)+u1");
}

#[test]
fn a_pushed_update_or_fix_head_still_conflicting_gets_a_review_not_another_update() {
    let c = setup("drive-conflict-stale");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(
        &c,
        20,
        "verdict",
        json!({"verdict": "PASS", "sha": sha("a1"), "body": "PASS"}),
    );
    let pr_at = |head: &str| {
        herdsman::gh::forget_pr(&c, "sendmeter", 12);
        fake(NO_AGENTS)
            .reply("gh pr view issue-12", &stale_pr(&sha(head), "CONFLICTING", "DIRTY"))
            .reply("git -C", "")
    };
    assert_eq!(drive(&c, &pr_at("a1")).1, ["conflict"]);
    assert_eq!(state(&c), "fix(1)+u1");
    // The update lane pushed b2; GitHub has not recomputed `mergeable` yet.
    let (_, out) = run(&c, &pr_at("b2"), &["waiting", "sendmeter"]);
    assert!(
        out.contains("waiting for drive (next pass ≤ 5 min) to spawn review"),
        "{out}"
    );
    assert!(
        out.contains("herdsman spawn sendmeter 12 rev") && !out.contains("--update"),
        "{out}"
    );
    // A FAIL's fix round pushing a conflicting head is reviewed, too.
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "FAIL", "sha": sha("b2"), "body": "FAIL"}),
    );
    ev(
        &c,
        31,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"}),
    );
    let (_, out) = run(&c, &pr_at("c3"), &["waiting", "sendmeter"]);
    assert!(
        out.contains("waiting for drive (next pass ≤ 5 min) to spawn review") && !out.contains("--update"),
        "{out}"
    );
    let (code, out) = run(&c, &Fake::default(), &["spawn", "sendmeter", "12", "rev", "--update"]);
    assert!(code == 3 && out.contains("REFUSED BAD_VALUE"), "{out}");
}

const SERIAL: &str = "serial_paths = [\"supabase/migrations/**\"]";

/// #100: a driven fleet with `extra` config lines, issues `ns` queued.
fn fanout(name: &str, extra: &str, ns: &[&str]) -> herdsman::Ctx {
    let c = ctx(
        &tmpdir(name),
        &CONFIG.replace(
            "integration = \"staging\"",
            &format!("integration = \"staging\"\n{extra}"),
        ),
    );
    let add = [&["queue", "sendmeter", "add"][..], ns].concat();
    assert_eq!(run(&c, &Fake::default(), &add).0, 0);
    c
}

fn fan_fake(agents: &str) -> Fake {
    fake(agents)
        .reply("git -C", "")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
}

/// The herdr agent list with issues `ns`' impl lanes working.
fn working(c: &herdsman::Ctx, ns: &[u64]) -> String {
    let wt = |n: u64| {
        c.worktrees
            .join(format!("sendmeter/issue-{n}"))
            .to_string_lossy()
            .to_string()
    };
    let rows: Vec<(String, String)> = ns.iter().map(|n| (format!("sendmeter-impl-{n}"), wt(*n))).collect();
    let rows: Vec<_> = rows
        .iter()
        .map(|(a, w)| (a.as_str(), "working", w.as_str(), "w9:p1"))
        .collect();
    agents(&rows)
}

fn state_of(c: &herdsman::Ctx, n: u64) -> String {
    fold(&read(&c.state).unwrap())
        .get(&("sendmeter".to_string(), n))
        .map_or("queued".into(), |s| s.state.clone())
}

#[test]
fn one_pass_starts_every_free_item_in_order_with_a_fresh_host_sample_each() {
    let c = fanout("drive-fan-all", "", &["1", "2", "3"]);
    let r = fan_fake(NO_AGENTS);
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["start", "start", "start"], "{out}");
    let at = |n: u64| r.index(&format!("herdr agent start sendmeter-impl-{n}")).unwrap();
    assert!(at(1) < at(2) && at(2) < at(3), "{:?}", r.calls.borrow());
    assert_eq!(
        r.count("sysctl -n kern.memorystatus_vm_pressure_level"),
        3,
        "one sample per start"
    );
    assert!((1..=3).all(|n| state_of(&c, n) == "impl"));
    // Nothing free on the second pass: a no-op.
    idle(&c, &fake(&working(&c, &[1, 2, 3])));
}

#[test]
fn a_serial_busy_item_is_skipped_and_keeps_its_place() {
    let c = fanout("drive-fan-serial", SERIAL, &["9", "1", "--serial"]);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "2", "3"]).0, 0);
    let wt = c.worktrees.join("sendmeter/issue-9");
    std::fs::create_dir_all(&wt).unwrap();
    let d = json!({"role": "impl", "agent": "sendmeter-impl-9", "worktree": wt, "branch": "issue-9"});
    append(&c.state, &Event::new(5, "sendmeter", Some(9), "spawn", d)).unwrap();
    let r = fan_fake(&working(&c, &[9]));
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["start", "start", "start"], "{out}");
    assert!(out.contains("sendmeter #1 start: REFUSED SERIAL_BUSY"), "{out}");
    assert!(!r.called("herdr agent start sendmeter-impl-1"));
    assert_eq!((state_of(&c, 2), state_of(&c, 3)), ("impl".into(), "impl".into()));
    assert_eq!(
        run(&c, &Fake::default(), &["next", "sendmeter"]),
        (0, "1\n".into()),
        "1 stays first"
    );
    let status = run(&c, &fake(&working(&c, &[2, 3, 9])), &["status", "sendmeter"]).1;
    assert!(
        status.contains("skipped this pass: SERIAL_BUSY (db: #9 holds 1/1"),
        "{status}"
    );
    let board = run(&c, &Fake::default(), &["board", "sendmeter"]).1;
    assert!(board.contains(" #1     queued      skip SERIAL_BUSY "), "{board}");
    // The second pass: #1 still refused (logged once an hour), nothing else free: a no-op.
    let r = fan_fake(&working(&c, &[2, 3, 9]));
    idle(&c, &r);
}

/// #103: DISK_LOW stops the pass like HOST_BUSY: #3 is not tried.
#[test]
fn disk_low_stops_the_pass() {
    let c = fanout("drive-fan-disk", "", &["1", "2", "3"]);
    let df = |gb: u64| {
        format!(
            "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/d 1 1 {} 1% /\n",
            gb << 20
        )
    };
    // #337: the step's look reads the disk first and, the pass having logged, last (the Host tab's row).
    let r = fan_fake(NO_AGENTS)
        .once("df -k /", 0, &df(50))
        .once("df -k /", 0, &df(50))
        .once("df -k /", 0, &df(5));
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["start", "start"], "{out}");
    assert!(out.contains("sendmeter #2 start: REFUSED DISK_LOW"), "{out}");
    assert_eq!(r.count("df -k /"), 4, "#3 not tried");
    assert_eq!(state_of(&c, 3), "queued");
}

#[test]
fn host_busy_stops_the_pass_and_cap_stops_starting() {
    let c = fanout("drive-fan-busy", "", &["1", "2", "3"]);
    let busy = Fake {
        replies: fan_fake(NO_AGENTS).replies,
        ..Default::default()
    }
    .once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "1\n")
    .once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "4\n");
    let (out, steps) = drive(&c, &busy);
    assert_eq!(steps, ["start", "start"], "{out}");
    assert!(out.contains("sendmeter #2 start: REFUSED HOST_BUSY"), "{out}");
    assert_eq!(
        busy.count("sysctl -n kern.memorystatus_vm_pressure_level"),
        2,
        "#3 not tried"
    );
    assert_eq!(state_of(&c, 3), "queued");

    let c = fanout("drive-fan-cap", "max_impl = 1", &["1", "2", "3"]);
    let r = fan_fake(NO_AGENTS);
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["start", "start"], "{out}");
    assert!(out.contains("sendmeter #2 start: REFUSED CAP"), "{out}");
    assert!(!out.contains("#3"), "{out}");
    assert_eq!((state_of(&c, 1), state_of(&c, 3)), ("impl".into(), "queued".into()));
    idle(&c, &fan_fake(&working(&c, &[1])));
}

/// #149: unmarked #12 at PASS + green whose PR changes `files`: one drive pass, then the `serial-joined` events (#294).
fn serial_auto(name: &str, files: &str) -> (herdsman::Ctx, Fake, String, Vec<Event>) {
    let cfg = CONFIG.replace(
        "integration = \"staging\"",
        &format!("integration = \"staging\"\ndriver = \"herdsman\"\n{SERIAL}"),
    );
    let c = ctx(&tmpdir(name), &cfg);
    lane(&c, 10, "impl", "sendmeter-impl-12", "gone-12");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/gone-12")).unwrap();
    let head = sha("abc");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
    );
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&head, "green"))
        .reply("gh pr diff 100", files)
        .reply("gh pr merge 100", "")
        .reply("gh pr view 100", r#"{"mergeCommit":{"oid":"m3rg3"}}"#)
        .reply("gh issue", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["merge"], "{out}");
    assert!(!out.contains("SERIAL_UNMARKED"), "{out}");
    assert_eq!(state(&c), "merged");
    let auto = read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "serial-joined");
    (c, r, out, auto.collect())
}

#[test]
fn an_unmarked_pr_touching_a_serial_path_is_auto_marked_and_merges() {
    let (c, r, out, auto) = serial_auto("drive-serial-auto", "src/a.rs\nsupabase/migrations/1_x.sql\n");
    assert!(
        out.contains("sendmeter #12 serial-joined db: supabase/migrations/1_x.sql"),
        "{out}"
    );
    assert_eq!(auto.len(), 1);
    assert_eq!(
        (auto[0].issue, auto[0].str("path")),
        (Some(12), Some("supabase/migrations/1_x.sql"))
    );
    idle(&c, &r);
}

#[test]
fn a_pr_off_serial_paths_is_not_marked() {
    let (_, _, out, auto) = serial_auto("drive-serial-none", "src/a.rs\n");
    assert!(auto.is_empty(), "{out}");
}

/// A PR view (CI: the full run not reported yet) carrying `labels`.
fn labelled_pr(head: &str, labels: &[&str]) -> String {
    let mut v: serde_json::Value = serde_json::from_str(&pr(head, "pending")).unwrap();
    v["labels"] = json!(labels.iter().map(|l| json!({"name": l})).collect::<Vec<_>>());
    v.to_string()
}

#[test]
fn a_pass_at_the_head_gets_the_merge_label_once_and_a_moved_head_loses_it() {
    let dir = tmpdir("drive-label");
    let cfg = CONFIG.replace(
        "integration = \"staging\"",
        "integration = \"staging\"\nmerge_label = \"merge-ready\"",
    );
    let c = ctx(&dir, &cfg);
    lane(&c, 10, "impl", "sendmeter-impl-12", "gone-12");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/gone-12")).unwrap();
    let (head, moved) = (sha("abc"), sha("def"));
    let file = dir.join("verdict.md");
    std::fs::write(&file, "PASS\nall good\n").unwrap();
    let add = "gh pr edit 100 -R jirathip-dev/sendmeter --add-label merge-ready";
    let kinds = |k: &str| read(&c.state).unwrap().iter().filter(|e| e.kind == k).count();
    // PASS at the head: labelled once, by the verdict itself.
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &labelled_pr(&head, &[]))
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply("gh pr edit", "");
    let f = file.to_string_lossy();
    let (code, out) = run(
        &c,
        &r,
        &["verdict", "sendmeter", "12", "PASS", "--sha", &head, "--file", &f],
    );
    assert_eq!(code, 0, "{out}");
    assert!(r.called(add), "{out}");
    assert_eq!(kinds("labelled"), 1);
    assert_eq!(state(&c), "ready", "labelled is not activity");
    // The label shows on the PR: the next pass doesn't re-add it.
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &labelled_pr(&head, &["merge-ready"]))
        .reply("gh pr edit", "")
        .reply("git -C", "");
    idle(&c, &r);
    assert!(!r.called("gh pr edit"));
    // The head moved past the PASS (a fresh PR read, as after 60 s): the label comes off, and is never added back.
    herdsman::gh::forget_pr(&c, "sendmeter", 12);
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &labelled_pr(&moved, &["merge-ready"]))
        .reply("gh pr edit", "");
    let (out, _) = drive(&c, &r);
    assert!(
        r.called("gh pr edit 100 -R jirathip-dev/sendmeter --remove-label merge-ready"),
        "{out}"
    );
    assert_eq!((kinds("labelled"), kinds("unlabelled")), (1, 1));
    herdsman::gh::forget_pr(&c, "sendmeter", 12);
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &labelled_pr(&moved, &[]))
        .reply("gh pr edit", "");
    drive(&c, &r);
    assert!(!r.called("gh pr edit"), "no label for the old PASS");
    assert_eq!(kinds("labelled"), 1);
}

#[test]
fn status_says_a_ready_pr_lacks_the_merge_label() {
    let c = ctx(
        &tmpdir("drive-label-status"),
        &CONFIG.replace(
            "integration = \"staging\"",
            "integration = \"staging\"\nmerge_label = \"merge-ready\"",
        ),
    );
    lane(&c, 10, "impl", "sendmeter-impl-12", "gone-12");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/gone-12")).unwrap();
    let head = sha("abc");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
    );
    let r = fake(NO_AGENTS).reply("gh pr view issue-12", &labelled_pr(&head, &[]));
    // Prime the PR cache the way a pass does; `status` reads only the cache.
    run(&c, &r, &["pr", "sendmeter", "12"]);
    let (code, out) = run(&c, &r, &["status", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("PR lacks label `merge-ready`"), "{out}");
}

/// #166: the `drive-pass` events of `c` (newest last).
fn passes(c: &herdsman::Ctx) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "drive-pass")
        .collect()
}

#[test]
fn every_pass_manual_or_automatic_writes_a_drive_pass_with_each_action_a_silent_repeat_too() {
    let c = setup("drive-pass");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let busy = || {
        Fake {
            replies: fake(NO_AGENTS).reply("git -C", "").replies,
            ..Default::default()
        }
        .once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "4\n")
    };
    drive(&c, &busy());
    let mut out = vec![];
    let cfg = herdsman::config::Config::load(&c.config).unwrap();
    // The second pass is a manual press and prints nothing (a repeat), but its record still lists the refusal.
    herdsman::drive::drive(&c, &cfg, &busy(), &mut out, Some("sendmeter"), true, false).unwrap();
    let p = passes(&c);
    assert_eq!(p.len(), 2, "{p:?}");
    assert_eq!((p[0].str("by"), p[1].str("by")), (Some("auto"), Some("manual")));
    for e in &p {
        assert_eq!(e.fleet, "sendmeter");
        assert_eq!(e.data["started"], c.now);
        assert!(e.data["ended"].as_u64().unwrap() >= c.now);
        let a = &e.data["actions"][0];
        assert_eq!((a["issue"].as_u64(), a["step"].as_str()), (Some(12), Some("start")));
        assert_eq!(
            (a["outcome"].as_str(), a["code"].as_str()),
            (Some("waiting"), Some("HOST_BUSY"))
        );
        assert!(
            a["words"].as_str().unwrap().starts_with("Mac busy (memory pressure"),
            "{a}"
        );
    }
    let (tone, line) = herdsman::drive::result(&p[1]);
    assert_eq!(tone, "a", "everything held back: amber");
    assert!(
        line.ends_with(" · started 0 · merged 0 · 1 waiting: Mac busy (memory pressure critical)"),
        "{line}"
    );
}

/// The other pass: it ends (its record, then the lock goes) on the press's 3rd one-second wait.
struct Other<'a>(&'a Fake, &'a herdsman::Ctx, std::sync::atomic::AtomicUsize);
impl herdsman::runner::Runner for Other<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<herdsman::runner::Out> {
        self.0.run(prog, args)
    }
    fn sleep(&self, _: u64) {
        if self.2.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 2 {
            let a = json!([{"issue": 12, "step": "start", "outcome": "started", "code": null, "words": "started"},
                {"issue": 13, "step": "start", "outcome": "waiting", "code": "CAP", "words": "lane limit or usage cap reached"}]);
            let d =
                json!({"by": "auto", "started": self.1.now - 3, "ended": self.1.now + 2, "at": "07:41", "actions": a});
            append(
                &self.1.state,
                &Event::new(self.1.now - 3, "sendmeter", None, "drive-pass", d),
            )
            .unwrap();
            std::fs::remove_file(self.1.state.parent().unwrap().join("drive.lock")).unwrap();
        }
    }
}

#[test]
fn a_manual_press_that_finds_a_pass_running_waits_for_it_and_shows_its_actions() {
    let c = setup("drive-wait");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let lock = c.state.parent().unwrap().join("drive.lock");
    std::fs::write(&lock, "").unwrap();
    let cfg = herdsman::config::Config::load(&c.config).unwrap();
    assert!(herdsman::drive::running(&c, "sendmeter"));
    let (f, mut out) = (fake(NO_AGENTS).reply("git -C", ""), vec![]);
    let r = Other(&f, &c, 0.into());
    herdsman::drive::drive(&c, &cfg, &r, &mut out, Some("sendmeter"), true, false).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert_eq!(
        out,
        "sendmeter #12 start: started\nsendmeter #13 start: REFUSED CAP: lane limit or usage cap reached\n"
    );
    assert_eq!(r.2.into_inner(), 3, "waited 3 s, then ran no pass of its own");
    assert!(!f.called("herdr"), "never two passes");
    assert_eq!(
        herdsman::drive::summary("sendmeter", &out),
        "sendmeter: started #12 · 1 skipped (CAP)"
    );
    assert_eq!(passes(&c).len(), 1);

    // Bounded: a pass still running after 120 s leaves the press with a pointer to the line.
    std::fs::write(&lock, "").unwrap();
    let mut out = vec![];
    herdsman::drive::drive(&c, &cfg, &f, &mut out, Some("sendmeter"), true, false).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "drive: still driving · result will appear here\n"
    );
    assert_eq!(f.count("sleep 1"), 120);
}

#[test]
fn the_result_line_is_red_on_an_error_green_on_a_start_or_merge_amber_when_all_held_back() {
    let line = |acts: serde_json::Value| {
        herdsman::drive::result(&Event::new(
            1,
            "f",
            None,
            "drive-pass",
            json!({"at": "07:41", "actions": acts}),
        ))
    };
    let a = |o: &str, w: &str| json!({"issue": 1, "step": "start", "outcome": o, "words": w});
    let busy = a("waiting", "Mac busy (load/cpu 4.60 > 2.5)");
    assert_eq!(
        line(json!([a("started", "started"), a("merged", "merged"), busy])),
        (
            "g",
            "07:41 · started 1 · merged 1 · 1 waiting: Mac busy (load/cpu 4.60 > 2.5)".into()
        )
    );
    let all = json!([busy, a("waiting", "waiting for CI"), busy, busy]);
    assert_eq!(
        line(all),
        (
            "a",
            "07:41 · started 0 · merged 0 · 4 waiting: Mac busy (load/cpu 4.60 > 2.5)".into()
        )
    );
    assert_eq!(
        line(json!([a("started", "started"), a("error", "failed: exit 1")])),
        ("r", "07:41 · started 1 · merged 0 · 1 failed".into())
    );
    assert_eq!(line(json!([])), ("m", "07:41 · started 0 · merged 0".into()));
}

// --- #182: green PRs get reviewed and merged without waiting on a handoff or the next pass ---

#[test]
fn a_pr_head_moved_without_a_handoff_is_handed_off_by_the_pr_once_ci_is_done() {
    let c = setup("drive-handoff");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("bee");
    let r = |ci: &str| {
        fake(&agents(&[(
            "sendmeter-impl-12",
            "idle",
            &wt.to_string_lossy(),
            "w9:p1",
        )]))
        .reply("gh pr view issue-12", &pr(&head, ci))
        .reply(&format!("git -C {} log", wt.display()), &format!("{} 20", sha("a1")))
        .reply("git -C", "")
    };
    // The worktree is elsewhere and no handoff was logged; CI still runs at the PR head: nothing yet.
    idle(&c, &r("pending"));
    herdsman::gh::forget_pr(&c, "sendmeter", 12);
    let (out, steps) = drive(&c, &r("green"));
    assert_eq!(steps, ["review"], "{out}");
    assert!(out.contains("sendmeter #12 pushed: bee0000 via pr"), "{out}");
    let all = read(&c.state).unwrap();
    let p = all.iter().find(|e| e.kind == "pushed").unwrap();
    assert_eq!((p.str("sha"), p.str("via")), (Some(head.as_str()), Some("pr")));
    assert_eq!(state(&c), "review(1)");
    assert_eq!(all.iter().filter(|e| e.kind == "pushed").count(), 1);
}

#[test]
fn a_pass_recorded_with_green_ci_merges_in_the_same_pass() {
    let c = setup("drive-verdict-merge");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let rev = lane(&c, 20, "rev", "sendmeter-rev-12-r1", "rev-12-r1");
    std::fs::write(rev.join("verdict.md"), "PASS\nall good\n").unwrap();
    let head = sha("abc");
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&head, "green"))
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply(&format!("git -C {} log", rev.display()), &format!("{head} 20"))
        .reply("git -C", "")
        .reply("gh pr merge 100", "")
        .reply("gh pr view 100", r#"{"mergeCommit":{"oid":"m3rg3"}}"#)
        .reply("gh issue", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["verdict", "merge"], "{out}");
    assert!(r.called(&format!(
        "gh pr merge 100 -R jirathip-dev/sendmeter --merge --match-head-commit {head}"
    )));
    assert_eq!(state(&c), "merged");
}

#[test]
fn under_limited_admission_a_review_goes_before_any_fleets_new_start() {
    let alpha = "[fleet.alpha]\nrepo = \"jirathip-dev/alpha\"\nlocal = \"/tmp/does-not-matter/alpha\"\n\
        integration = \"staging\"\n\n[fleet.alpha.role.impl]\nharness = \"hermes\"\nprofile = \"fleet-impl\"\n\n[limits]";
    let c = ctx(&tmpdir("drive-rev-first"), &CONFIG.replace("[limits]", alpha));
    assert_eq!(run(&c, &Fake::default(), &["queue", "alpha", "add", "5"]).0, 0);
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("abc");
    // The host admits one spawn, then is busy.
    let r = Fake {
        replies: fake(&agents(&[(
            "sendmeter-impl-12",
            "idle",
            &wt.to_string_lossy(),
            "w9:p1",
        )]))
        .reply("gh pr view issue-12", &pr(&head, "pending"))
        .reply(&format!("git -C {} log", wt.display()), &format!("{head} 20"))
        .reply("git -C", "")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .replies,
        ..Default::default()
    }
    .once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "1\n")
    .once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "4\n");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["review", "start"], "{out}");
    assert!(r.called("herdr agent start sendmeter-rev-12-r1"), "{out}");
    assert!(out.contains("alpha #5 start: REFUSED HOST_BUSY"), "{out}");
    assert_eq!(state(&c), "review(1)");
}

/// #212: `herdsman drive --fast` (output, `driven` steps logged by it).
fn fast(c: &herdsman::Ctx, r: &Fake) -> (String, Vec<String>) {
    let before = read(&c.state).unwrap().len();
    let (code, out) = run(c, r, &["drive", "--fast"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    let new = read(&c.state).unwrap().split_off(before);
    let steps = new.iter().filter(|e| e.kind == "driven");
    (out, steps.map(|e| e.str("step").unwrap().to_string()).collect())
}

/// #212: the fast step merges a PASS + green issue past a live slow pass's lock, and starts no lane.
#[test]
fn the_fast_step_merges_past_a_held_slow_lock_and_starts_no_lane() {
    let c = setup("drive-fast-merge");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "13"]).0, 0);
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let rev = lane(&c, 20, "rev", "sendmeter-rev-12-r1", "rev-12-r1");
    std::fs::write(rev.join("verdict.md"), "PASS\nall good\n").unwrap();
    let lock = c.state.parent().unwrap().join("drive.lock");
    std::fs::write(&lock, "4242").unwrap();
    let head = sha("abc");
    let r = fake(NO_AGENTS)
        .reply("kill -0 4242", "")
        .reply("gh pr view issue-12", &pr(&head, "green"))
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply(&format!("git -C {} log", rev.display()), &format!("{head} 20"))
        .reply("git -C", "")
        .reply("gh pr merge 100", "")
        .reply("gh pr view 100", r#"{"mergeCommit":{"oid":"m3rg3"}}"#)
        .reply("gh issue", "");
    // An issue whose own lock a live pass holds is skipped.
    let issue = c.state.parent().unwrap().join("drive-sendmeter-12.lock");
    std::fs::write(&issue, "4242").unwrap();
    assert_eq!(fast(&c, &r).1, Vec::<String>::new());
    std::fs::remove_file(&issue).unwrap();
    let (out, steps) = fast(&c, &r);
    assert_eq!(steps, ["verdict", "merge"], "{out}");
    assert_eq!(state(&c), "merged");
    assert!(!r.called("herdr agent start") && !r.called("git -C /tmp/does-not-matter/sendmeter fetch"));
    assert_eq!(std::fs::read_to_string(&lock).unwrap(), "4242", "the slow lock stays");
    assert_eq!(herdsman::drive::last_pass(&c, "sendmeter"), None, "no slow pass ran");
    assert!(!issue.exists(), "the issue lock is released");
}

/// #212: a ready PASS's card from the cached PR the fast step refreshes: stale says when it was checked, then fresh.
#[test]
fn the_fast_step_refreshes_the_card_cache_and_the_wording_reads_it() {
    let c = setup("drive-fast-cache");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("abc");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
    );
    let cfg = herdsman::config::Config::load(&c.config).unwrap();
    let f = cfg.fleet("sendmeter").unwrap();
    let check =
        |n: &str, st: &str, con: &str| json!({"__typename": "CheckRun", "name": n, "status": st, "conclusion": con});
    let view = |checks: Vec<serde_json::Value>| {
        json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": head, "statusCheckRollup": checks})
            .to_string()
    };
    let card = || {
        let ws = herdsman::waiting::scan(&c, &cfg, &Fake::default(), &read(&c.state).unwrap(), &[], false);
        ws.into_iter().find(|w| w.1.issue == 12).unwrap().1
    };
    // Cached ten minutes ago: three of five green.
    let mut five: Vec<_> = (0..3)
        .map(|i| check(&format!("c{i}"), "COMPLETED", "SUCCESS"))
        .collect();
    five.extend((3..5).map(|i| check(&format!("c{i}"), "IN_PROGRESS", "")));
    let old = Fake::default().reply("gh pr view issue-12", &view(five.clone()));
    herdsman::gh::pr(&c, &old, f, ("sendmeter", 12), "issue-12", c.now - 600).unwrap();
    assert_eq!(card().what, "PASS · CI running 3/5 · checked 10m ago");
    // The fast step reads it afresh: one red check.
    five[4] = check("lint", "COMPLETED", "FAILURE");
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &view(five))
        .reply("git -C", "");
    fast(&c, &r);
    assert_eq!(herdsman::gh::cached_at(&c, "sendmeter", 12), Some(c.now));
    assert_eq!((card().who, card().what.as_str()), ("ci", "PASS · CI red: lint"));
    assert!(!r.called("gh run view"), "a red CI's fix round is the slow step's");
    // All green: merging within a minute.
    let green: Vec<_> = (0..5)
        .map(|i| check(&format!("c{i}"), "COMPLETED", "SUCCESS"))
        .collect();
    let now = Fake::default().reply("gh pr view issue-12", &view(green));
    herdsman::gh::pr(&c, &now, f, ("sendmeter", 12), "issue-12", c.now + 60).unwrap();
    assert_eq!(card().what, "PASS · CI green · merging within 1 min");
}

/// #212: a review start the host refused says so on the card, in plain words.
#[test]
fn a_refused_review_start_shows_waiting_for_a_reviewer_slot() {
    let c = setup("drive-rev-slot");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("abc");
    let r = Fake {
        replies: fake(&agents(&[(
            "sendmeter-impl-12",
            "idle",
            &wt.to_string_lossy(),
            "w9:p1",
        )]))
        .reply("gh pr view issue-12", &pr(&head, "pending"))
        .reply(&format!("git -C {} log", wt.display()), &format!("{head} 20"))
        .reply("git -C", "")
        .replies,
        ..Default::default()
    }
    .once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "4\n");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["review"], "{out}");
    let (_, out) = run(&c, &r, &["waiting", "sendmeter"]);
    assert!(out.contains("waiting for a reviewer slot: Mac busy ("), "{out}");
}

// --- #214: conflicts get their update lane, catch-ups every pass, a lost reviewer is replaced, no silent block ---

#[test]
fn a_ready_conflicting_pr_gets_its_update_lane_and_no_merge_attempt() {
    let c = setup("drive-conflict-first");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let rev = lane(&c, 20, "rev", "sendmeter-rev-12-r1", "rev-12-r1");
    std::fs::write(rev.join("verdict.md"), "PASS\nall good\n").unwrap();
    let head = sha("abc");
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &stale_pr(&head, "CONFLICTING", "DIRTY"))
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply(&format!("git -C {} log", rev.display()), &format!("{head} 20"))
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["verdict", "conflict"], "{out}");
    assert!(!r.called("gh pr merge"), "{out}");
    assert_eq!(state(&c), "fix(1)+u1");
}

#[test]
fn a_merge_that_fails_on_a_conflict_gets_the_update_lane() {
    let c = setup("drive-merge-conflict");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("abc");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
    );
    // GitHub's cached `mergeable` lags: the merge itself finds the conflict.
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &stale_pr(&head, "MERGEABLE", "CLEAN"))
        .once_err(
            "gh pr merge 100",
            1,
            "Pull request #100 is not mergeable: the merge commit cannot be cleanly created.",
        )
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["merge", "conflict"], "{out}");
    assert_eq!(state(&c), "fix(1)+u1");
}

#[test]
fn auto_update_runs_in_a_pass_with_no_merge() {
    let c = setup("drive-catch-up");
    strict(&c);
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("a1");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
    );
    let wt = c.state.with_file_name("update-sendmeter");
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&head, "pending"))
        .reply("git -C /tmp/does-not-matter/sendmeter rev-parse", &format!("{head}\n"))
        .reply(
            &format!("git -C {} rev-parse HEAD", wt.display()),
            &format!("{}\n", sha("b2")),
        )
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert!(steps.is_empty(), "{out}");
    assert!(!r.called("gh pr merge"), "{out}");
    assert!(out.contains("sendmeter #12 auto-updated: a100000 → b200000"), "{out}");
    assert!(r.called(&format!("git -C {} push -q origin HEAD:issue-12", wt.display())));
}

#[test]
fn an_exited_reviewer_without_a_verdict_gets_a_new_review_round() {
    let c = setup("drive-rev-lost");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("b4e191c");
    let rev = c.worktrees.join("sendmeter/rev-12-r1");
    std::fs::create_dir_all(&rev).unwrap();
    let d = json!({"role": "rev", "agent": "sendmeter-rev-12-r1", "pane": "w9:r", "workspace": "w9",
        "worktree": rev, "branch": "rev-12-r1", "sha": head});
    ev(&c, 20, "spawn", d);
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&head, "green"))
        .reply("git -C", "");
    let (_, out) = run(&c, &r, &["waiting", "sendmeter"]);
    assert!(
        out.contains("reviewer exited without a verdict: reviewing again"),
        "{out}"
    );
    assert!(!out.contains("head moved"), "{out}");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["review"], "{out}");
    assert!(r.called("herdr agent start sendmeter-rev-12-r1"), "{out}");
    let all = read(&c.state).unwrap();
    assert!(all.iter().any(|e| e.kind == "lane-lost"), "{out}");
}

#[test]
fn blocked_with_an_open_pr_takes_the_next_step_and_without_one_logs_no_action() {
    for (name, mergeable, step) in [
        ("drive-blocked-conflict", "CONFLICTING", "conflict"),
        ("drive-blocked-review", "MERGEABLE", "review"),
    ] {
        let c = setup(name);
        lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
        let head = sha("abc");
        ev(
            &c,
            30,
            "verdict",
            json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
        );
        ev(&c, 40, "blocked", json!({"code": "BASE_MISMATCH", "reason": "?"}));
        let r = fake(NO_AGENTS)
            .reply("gh pr view issue-12", &stale_pr(&head, mergeable, "DIRTY"))
            .reply("git -C", "");
        let (out, steps) = drive(&c, &r);
        assert_eq!(steps, [step], "{out}");
    }
    let c = setup("drive-blocked-no-pr");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(&c, 40, "blocked", json!({"code": "NO_PR", "reason": "no open PR"}));
    let r = fake(NO_AGENTS).reply("git -C", "");
    let (out, _) = drive(&c, &r);
    assert!(
        out.contains("sendmeter #12 no-action: nothing to do (blocked)"),
        "{out}"
    );
    idle(&c, &r);
    assert_eq!(
        read(&c.state).unwrap().iter().filter(|e| e.kind == "no-action").count(),
        1
    );
}

/// Notes, at each command run, whether the log already holds `round-extended` and drive.lock is held.
struct Spy<'a>(Fake, &'a herdsman::Ctx, std::sync::Mutex<Vec<bool>>);
impl herdsman::runner::Runner for Spy<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<herdsman::runner::Out> {
        let held = self.1.state.parent().unwrap().join("drive.lock").exists();
        let logged = held && read(&self.1.state).unwrap().iter().any(|e| e.kind == "round-extended");
        self.2.lock().unwrap().push(logged);
        self.0.run(prog, args)
    }
}

/// #217: `[one more round]` logs its decision before the spawn runs; a spawn refused HOST_BUSY logs `extend-failed`
/// with the reason, and the next drive pass starts the recorded round as its fix step. The spawn runs under
/// drive.lock, so no pass starts the same round meanwhile.
#[test]
fn extend_logs_first_and_a_failed_spawn_is_the_next_passs_fix() {
    let c = setup("drive-extend");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    for (ts, p) in [(11, "1"), (12, "2"), (13, "3")] {
        ev(
            &c,
            ts,
            "verdict",
            json!({"verdict": "FAIL", "sha": sha(p), "body": "FAIL"}),
        );
    }
    ev(&c, 14, "needs-human", json!({"code": "ROUND_LIMIT"}));
    let base = || {
        fake(NO_AGENTS)
            .reply("gh pr view issue-12", &pr(&sha("3"), "green"))
            .reply("git -C", "")
    };
    let busy = Fake {
        replies: base().replies,
        ..Default::default()
    }
    .once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "4\n");
    let spy = Spy(busy, &c, Default::default());
    let (code, out) = run(&c, &spy, &["extend", "sendmeter", "12"]);
    assert!(code == 3 && out.contains("REFUSED HOST_BUSY"), "{out}");
    let seen = spy.2.lock().unwrap().clone();
    assert!(
        !seen.is_empty() && seen.iter().all(|&l| l),
        "a command ran before the decision or outside drive.lock: {seen:?}"
    );
    let all = read(&c.state).unwrap();
    let failed = all
        .iter()
        .find(|e| e.kind == "extend-failed")
        .expect("no extend-failed");
    assert!(
        failed.str("reason").unwrap().starts_with("REFUSED HOST_BUSY"),
        "{failed:?}"
    );
    assert_eq!(state(&c), "fix(3)");

    let r = base();
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["fix"], "{out}");
    assert!(r.called("herdr agent start"), "{:?}", r.calls.borrow());
}

/// #217 review finding 1: an extend while a drive pass holds drive.lock records the round and spawns nothing (the
/// pass could start it too); the next pass starts it.
#[test]
fn an_extend_during_a_pass_leaves_the_spawn_to_the_next_pass() {
    let c = setup("drive-extend-lock");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(&c, 14, "needs-human", json!({"code": "UPDATE_LIMIT"}));
    let lock = c.state.parent().unwrap().join("drive.lock");
    std::fs::write(&lock, "").unwrap();
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&sha("3"), "green"))
        .reply("git -C", "");
    let (code, out) = run(&c, &r, &["extend", "sendmeter", "12"]);
    assert!(code == 0 && out.contains("the next pass starts the round"), "{out}");
    assert!(!r.called("herdr agent start"), "{:?}", r.calls.borrow());
    assert!(lock.exists(), "the pass's lock is left alone");
    assert_eq!(
        read(&c.state)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "round-extended")
            .count(),
        1
    );

    std::fs::remove_file(&lock).unwrap();
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["fix"], "{out}");
    assert!(r.called("herdr agent start"), "{:?}", r.calls.borrow());
}

/// #217 review round 2: `extend --no-spawn` leaves the live lane alone; no drive pass supersedes it for the round.
#[test]
fn an_extend_no_spawn_is_never_a_due_fix_over_a_live_lane() {
    let c = setup("drive-extend-no-spawn");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(&c, 14, "needs-human", json!({"code": "UPDATE_LIMIT"}));
    let live = agents(&[("sendmeter-impl-12", "idle", &wt.to_string_lossy(), "w9:p1")]);
    let r = fake(&live)
        .reply("gh pr view issue-12", &pr(&sha("3"), "green"))
        .reply("git -C", "");
    let (code, out) = run(&c, &r, &["extend", "sendmeter", "12", "--no-spawn"]);
    assert_eq!(code, 0, "{out}");
    let (out, steps) = drive(&c, &r);
    assert!(!steps.iter().any(|s| s == "fix"), "{out}");
    assert!(
        !r.called("herdr agent start sendmeter-impl-12") && !r.called("herdr pane close"),
        "{:?}",
        r.calls.borrow()
    );
}

// --- #319: a fix round that ends with no new commit still moves on ---

/// #12 FAILed at `head` with `body` (`stage`), then its fix lane (spawned at 40) went idle without a new commit.
fn no_change(name: &str, stage: &str, body: &str, ci: &str) -> (herdsman::Ctx, Fake, String) {
    let c = setup(name);
    let head = sha("abc");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(&c, 20, "pushed", json!({"sha": head}));
    let d = json!({"verdict": "FAIL", "sha": head, "body": body, "stage": stage, "loop": "rev"});
    ev(&c, 30, "verdict", d);
    let wt = lane(&c, 40, "impl", "sendmeter-impl-12", "issue-12");
    assert_eq!(state(&c), "fix(1)");
    let r = fake(&agents(&[(
        "sendmeter-impl-12",
        "idle",
        &wt.to_string_lossy(),
        "w9:p1",
    )]))
    .reply("gh pr view issue-12", &pr(&head, ci))
    .reply("gh pr merge 100", "")
    .reply("gh pr view 100", r#"{"mergeCommit":{"oid":"m3rg3"}}"#)
    .reply("gh issue", "")
    .reply("git -C", "");
    (c, r, head)
}

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

#[test]
fn a_ci_only_fail_green_again_at_the_same_head_merges_without_a_review() {
    for (stage, body) in [
        ("gates", "FAIL\n\nCI is red at abc: ci."),
        ("rev", "FAIL\n1. blocking [ci]: the runner ran out of disk"),
    ] {
        let (c, r, head) = no_change(&format!("drive-noop-ci-{stage}"), stage, body, "green");
        let (out, steps) = drive(&c, &r);
        assert_eq!(steps, ["merge"], "{out}");
        assert!(!r.called("herdr agent start sendmeter-rev"), "{out}");
        let e = &kinds(&c, "fix-no-change")[0];
        assert_eq!((e.str("sha"), e.str("ci")), (Some(head.as_str()), Some("green")));
        assert_eq!(state(&c), "merged");
    }
}

#[test]
fn a_code_fail_with_no_change_is_reviewed_again_at_the_same_head() {
    let body = "FAIL\n1. blocking: src/x.rs:3 off by one";
    let (c, r, head) = no_change("drive-noop-code", "rev", body, "green");
    // Green CI alone does not answer a code finding: the lane must say why nothing changed.
    idle(&c, &r);
    let earlier = herdsman::Ctx {
        now: c.now - 600,
        ..c.clone()
    };
    let (code, out) = run(
        &earlier,
        &r,
        &["done", "sendmeter", "12", "--no-change", "the finding is wrong"],
    );
    assert_eq!(code, 0, "{out}");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["review"], "{out}");
    assert!(r.called("herdr agent start sendmeter-rev-12-r2"), "{out}");
    let e = &kinds(&c, "fix-no-change")[0];
    assert_eq!(
        (e.str("sha"), e.str("why")),
        (Some(head.as_str()), Some("the finding is wrong"))
    );
    assert!(state(&c).starts_with("review"), "{}", state(&c));
}

#[test]
fn an_unchanged_head_with_red_ci_stays_and_the_stall_says_what_is_missing() {
    let (c, r, _) = no_change("drive-noop-red", "gates", "FAIL\n\nCI is red", "red");
    idle(&c, &r);
    assert!(kinds(&c, "fix-no-change").is_empty());
    assert_eq!(state(&c), "fix(1)");
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    let s = &kinds(&c, "stall")[0];
    let want = "fix lane idle, head unchanged, CI failed: run herdsman done sendmeter 12 --no-change \"<why>\" or push";
    assert!(s.str("reason").unwrap().ends_with(want), "{:?}", s.data);
}
