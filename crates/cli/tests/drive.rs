//! #92 `drover drive`: each step once per pass, a second pass a no-op; paused, HOST_BUSY and the lock.
mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::rc::Rc;

thread_local! {
    static SHED: Rc<FakeShed> = Rc::new(FakeShed::new());
}

fn sh() -> Rc<FakeShed> {
    SHED.with(Rc::clone)
}

fn ctx(dir: &Path, config: &str) -> drover::Ctx {
    let c = common::ctx(dir, config);
    sh().attach(&c);
    c
}

fn reconfig(c: &drover::Ctx, base: &str) {
    std::fs::write(&c.config, sh().config(base)).unwrap();
}

const NONE: &[(&str, &str, &str)] = &[];

fn started(prefix: &str) -> Vec<String> {
    let runs = sh().all("run").into_iter();
    let names = runs.filter_map(|p| p["name"].as_str().map(String::from));
    names.filter(|n| n.starts_with(prefix)).collect()
}

fn typed(name: &str, text: &str) -> usize {
    sh().sent(name).iter().filter(|t| t.starts_with(text)).count()
}

fn sha(p: &str) -> String {
    format!("{p}{}", "0".repeat(40 - p.len()))
}

fn setup(name: &str) -> drover::Ctx {
    ctx(&tmpdir(name), CONFIG)
}

fn ev(c: &drover::Ctx, ts: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
}

/// A lane of #12 logged as spawned, in a real worktree dir.
fn lane(c: &drover::Ctx, ts: u64, role: &str, agent: &str, dir: &str) -> PathBuf {
    let wt = c.worktrees.join("sendmeter").join(dir);
    std::fs::create_dir_all(&wt).unwrap();
    let d = json!({"role": role, "agent": agent, "pane": agent, "workspace": "", "runtime": "shed", "worktree": wt,
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

/// #285: integration's protection read (cached) as strict, so a behind ready PR is caught up.
fn strict(c: &drover::Ctx) {
    let cache = c.state.with_file_name("cache");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(
        cache.join("strict-sendmeter-0.json"),
        json!({"ts": c.now, "strict": true}).to_string(),
    )
    .unwrap();
}

fn fake<A: AsRef<str>, B: AsRef<str>, C: AsRef<str>>(live: &[(A, B, C)]) -> Fake {
    let sh = sh();
    let names: Vec<&str> = live.iter().map(|l| l.0.as_ref()).collect();
    let lanes: Vec<String> = sh.lock().terms.keys().cloned().collect();
    for gone in lanes.iter().filter(|n| !names.contains(&n.as_str())) {
        sh.exit(gone);
    }
    for (name, status, cwd) in live {
        sh.lane(name.as_ref(), status.as_ref(), cwd.as_ref());
    }
    Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("which shed", "/opt/shed/bin/shed\n")
}

/// `drover drive`: (output, `driven` steps logged by this pass).
fn drive(c: &drover::Ctx, r: &Fake) -> (String, Vec<String>) {
    let before = read(&c.state).unwrap().len();
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    let new = read(&c.state).unwrap().to_vec().split_off(before);
    (
        out,
        new.iter()
            .filter(|e| e.kind == "driven")
            .map(|e| e.str("step").unwrap().to_string())
            .collect(),
    )
}

/// The second pass changes nothing: no output, no event but its own `drive-pass` record (#166).
fn idle(c: &drover::Ctx, r: &Fake) {
    let acts = || {
        read(&c.state)
            .unwrap()
            .iter()
            .filter(|e| e.kind != "drive-pass")
            .count()
    };
    let before = acts();
    assert_eq!(drive(c, r).0, "");
    assert_eq!(acts(), before);
}

fn state(c: &drover::Ctx) -> String {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)]
        .state
        .clone()
}

#[test]
fn start_spawns_the_next_queued_issue() {
    let c = setup("drive-start");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let r = fake(NONE).reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["start"], "{out}");
    assert!(out.starts_with("sendmeter #12 start: "), "{out}");
    assert!(!started("sendmeter-impl-12").is_empty());
    assert_eq!(state(&c), "impl");
    let wt = c.worktrees.join("sendmeter/issue-12");
    idle(&c, &fake(&[("sendmeter-impl-12", "working", &wt.to_string_lossy())]));
}

#[test]
fn review_spawns_rev_at_the_pushed_head() {
    let c = setup("drive-review");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("abc");
    let r = fake(&[("sendmeter-impl-12", "idle", &wt.to_string_lossy())])
        .once("git -C /tmp/does-not-matter/sendmeter rev-parse --verify", 1, "")
        .reply("gh pr view issue-12", &pr(&head, "pending"))
        .reply(&format!("git -C {} log", wt.display()), &format!("{head} 20"))
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["review"], "{out}");
    let rev = c.worktrees.join("sendmeter/rev-12-r1");
    let add = format!(
        "git -C /tmp/does-not-matter/sendmeter worktree add -b rev-12-r1 {} origin/issue-12",
        rev.display()
    );
    assert!(r.called(&add), "{:?}", r.calls.borrow());
    assert!(!started("sendmeter-rev-12-r1").is_empty());
    assert_eq!(state(&c), "review(1)");
    // The reviewer's pane is live (idle, no verdict yet): nothing to do.
    let r = fake(&[
        ("sendmeter-impl-12", "idle", &wt.to_string_lossy()),
        ("sendmeter-rev-12-r1", "idle", &rev.to_string_lossy()),
    ])
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
    let r = fake(NONE)
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
        json!({"role": "rev", "agent": "sendmeter-rev-12-r1", "pane": "sendmeter-rev-12-r1", "workspace": "", "runtime": "shed",
        "worktree": rev, "branch": "rev-12-r1", "sha": sha("0d")}),
    );
    std::fs::write(rev.join("verdict.md"), "PASS\nall good\n").unwrap();
    let head = sha("bee");
    let r = fake(NONE)
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
        .to_vec()
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
    let r = fake(NONE)
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
    let r = fake(NONE)
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
    let r = fake(NONE)
        .reply("gh pr view issue-12", &pr(&sha("abc"), "green"))
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["fix"], "{out}");
    let brief = std::fs::read_to_string(wt.join(".brief.md")).unwrap();
    assert!(brief.contains(body), "{brief}");
    assert!(brief.contains("If a finding is wrong or out of scope, say so in your report with file:line proof"));
    assert_eq!(state(&c), "fix(1)");
    idle(&c, &fake(&[("sendmeter-impl-12", "working", &wt.to_string_lossy())]));
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
    let r = fake(NONE)
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
    assert!(started("").is_empty(), "no fix lane after the limit");
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
    let r = fake(NONE)
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
    let r = fake(NONE)
        .reply(&format!("git -C {} rev-list", wt.display()), "0\n")
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["clean"], "{out}");
    let rm = format!("git -C /tmp/does-not-matter/sendmeter worktree remove {}", wt.display());
    assert!(r.called(&rm), "{:?}", r.calls.borrow());
    std::fs::remove_dir_all(&wt).unwrap();
    idle(&c, &r);
}

/// #605: drive's pass over #12's impl lane, its pane listed as `status`, `s` seconds after `c.now`.
fn idle_lane(name: &str, merged: bool, status: &str) -> (drover::Ctx, PathBuf, impl Fn(u64) -> Fake) {
    let c = setup(name);
    // #666: an open issue's lane spawned a minute ago, so no pass here finds it stalled.
    let t = if merged { 10 } else { c.now - 60 };
    let wt = lane(&c, t, "impl", "sendmeter-impl-12", "issue-12");
    if merged {
        ev(&c, 30, "merged", json!({"pr": "100"}));
    }
    let w = wt.display().to_string();
    let list = [("sendmeter-impl-12", status.to_string(), w.clone())];
    // One unpushed commit until the wip push.
    let r = move |_| {
        fake(&list)
            .once(&format!("git -C {w} rev-list"), 0, "1\n")
            .reply(&format!("git -C {w} rev-list"), "0\n")
            .reply("date -r", "20261107-120000\n")
            .reply("git -C", "")
    };
    (c, wt, r)
}

struct Stamped(Fake, std::sync::Mutex<Vec<(String, usize)>>);
impl drover::runner::Runner for Stamped {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        let line = format!("{prog} {}", args.join(" "));
        self.1.lock().unwrap().push((line, sh().methods().len()));
        self.0.run(prog, args)
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
}
impl Stamped {
    fn at(&self, prefix: &str) -> Option<usize> {
        let calls = self.1.lock().unwrap();
        calls.iter().find(|c| c.0.starts_with(prefix)).map(|c| c.1)
    }
}

fn at(c: &drover::Ctx, s: u64) -> drover::Ctx {
    drover::Ctx {
        now: c.now + s,
        ..c.clone()
    }
}

#[test]
fn a_merged_issues_idle_lane_pane_closes_after_the_limit_with_its_work_on_wip() {
    let (c, wt, r) = idle_lane("drive-merged-idle", true, "idle");
    let r0 = r(0);
    let (code, out) = run(&c, &r0, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        sh().all("kill").is_empty(),
        "idle under idle_merged_minutes (15): {out}"
    );
    assert!(read(&c.state).unwrap().iter().all(|e| e.kind != "lane-closed"));
    let r1 = Stamped(r(900), Default::default());
    let (code, out) = run(&at(&c, 900), &r1, &["drive"]);
    assert_eq!(code, 0, "{out}");
    let w = wt.display();
    let push = format!("git -C {w} push origin HEAD:refs/heads/wip/sendmeter-impl-12-20261107-120000-");
    let close = sh().methods().iter().position(|m| m == "kill").expect("pane closed");
    assert!(r1.at(&push).expect("wip pushed") <= close, "{:?}", sh().methods());
    let all = read(&c.state).unwrap();
    let e = all.iter().find(|e| e.kind == "lane-closed").expect("lane-closed");
    assert_eq!(
        (e.str("agent"), e.str("why")),
        (Some("sendmeter-impl-12"), Some("merged-idle"))
    );
    assert!(
        e.str("wip")
            .unwrap()
            .starts_with("wip/sendmeter-impl-12-20261107-120000-"),
        "{e:?}"
    );
    // Then the worktree goes the clean way.
    let rm = format!("git -C /tmp/does-not-matter/sendmeter worktree remove {w}");
    assert!(close < r1.at(&rm).expect("removed"));
}

#[test]
fn a_working_lane_of_a_merged_issue_is_never_closed() {
    let (c, _, r) = idle_lane("drive-merged-working", true, "working");
    for s in [0, 900, 3600] {
        let r = r(s);
        let (code, out) = run(&at(&c, s), &r, &["drive"]);
        assert_eq!(code, 0, "{out}");
        assert!(
            sh().all("kill").is_empty() && !r.called("git -C /tmp/does-not-matter/sendmeter worktree remove"),
            "{out}"
        );
    }
    assert!(read(&c.state).unwrap().iter().all(|e| e.kind != "lane-closed"));
}

#[test]
fn an_open_issues_idle_lane_is_kept() {
    let (c, _, r) = idle_lane("drive-open-idle", false, "idle");
    // #666: past `stall_minutes` it is superseded instead (tests/stalled_lanes.rs).
    for s in [0, 900, 1500] {
        let r = r(s);
        let (code, out) = run(&at(&c, s), &r, &["drive"]);
        assert_eq!(code, 0, "{out}");
        assert!(!sh().called("kill", "sendmeter-impl-12"), "{:?}", r.calls.borrow());
    }
    assert!(read(&c.state).unwrap().iter().all(|e| e.kind != "lane-closed"));
}

#[test]
fn a_secret_in_a_merged_lanes_wip_keeps_its_pane_reported_once_and_clean_dry_run_names_the_close() {
    let c = setup("drive-merged-secret");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(&c, 30, "merged", json!({"pr": "100"}));
    let w = wt.display().to_string();
    let list = [("sendmeter-impl-12", "done", &w)];
    let r = || {
        fake(&list)
            .reply(&format!("git -C {w} rev-list"), "1\n")
            .reply(
                &format!("git -C {w} log -p"),
                concat!("+TOKEN = \"gh", "p_abcdefghijklmnopqrstu1234\"\n"),
            )
            .reply("git -C", "")
    };
    run(&c, &r(), &["drive"]);
    let (code, dry) = run(&at(&c, 900), &r(), &["clean", "sendmeter"]);
    assert_eq!(code, 0, "{dry}");
    assert!(
        dry.contains("would close sendmeter-impl-12's idle pane sendmeter-impl-12"),
        "{dry}"
    );
    for s in [900, 1200] {
        let r = r();
        let (code, out) = run(&at(&c, s), &r, &["drive"]);
        assert_eq!(code, 0, "{out}");
        assert!(
            sh().all("kill").is_empty() && !r.called(&format!("git -C {w} push")),
            "{out}"
        );
        let n = s == 900;
        assert_eq!(r.called(&format!("git -C {w} add -A")), n, "tried once, never again");
    }
    let kept: Vec<_> = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == "lane-kept-dirty")
        .collect();
    assert_eq!(kept.len(), 1);
    assert!(kept[0].str("why").unwrap().contains("SECRET_IN_WIP"), "{:?}", kept[0]);
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
fn a_pr_merged_outside_drover_is_logged_merged_and_its_lanes_cleaned() {
    let c = setup("drive-external-merge");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(
        &c,
        20,
        "verdict",
        json!({"verdict": "PASS", "sha": sha("a1"), "body": "PASS"}),
    );
    assert_eq!(state(&c), "ready");
    let r = fake(NONE)
        .reply("gh pr view issue-12", &gone_pr("MERGED"))
        .reply(&format!("git -C {} rev-list", wt.display()), "0\n")
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert!(out.contains("sendmeter #12 merged outside drover (PR #100)"), "{out}");
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
    let rm = format!("git -C /tmp/does-not-matter/sendmeter worktree remove {}", wt.display());
    assert!(r.called(&rm), "{:?}", r.calls.borrow());
    std::fs::remove_dir_all(&wt).unwrap();
    assert!(!run(&c, &r, &["waiting", "sendmeter"]).1.contains("#12"));
    // The repair is once: a later pass logs nothing more.
    idle(&c, &r);
    assert_eq!(read(&c.state).unwrap().iter().filter(|e| e.kind == "merged").count(), 1);
}

#[test]
fn a_pr_closed_outside_drover_closes_or_blocks_the_issue_once() {
    for (issues, want) in [(r#"[{"number":12}]"#, "closed"), ("[]", "blocked")] {
        let c = setup(&format!("drive-external-{want}"));
        lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
        ev(
            &c,
            20,
            "verdict",
            json!({"verdict": "PASS", "sha": sha("a1"), "body": "PASS"}),
        );
        let r = fake(NONE)
            .reply("gh pr view issue-12", &gone_pr("CLOSED"))
            .reply("gh issue list", issues)
            .reply("git -C", "");
        let (out, _) = drive(&c, &r);
        assert!(
            out.contains(&format!("sendmeter #12 {want} outside drover (PR #100)")),
            "{out}"
        );
        assert_eq!(state(&c), want);
        let e = read(&c.state)
            .unwrap()
            .to_vec()
            .into_iter()
            .rfind(|e| e.is(want, "sendmeter", 12))
            .unwrap();
        if want == "blocked" {
            assert_eq!(
                (e.str("code"), e.str("reason")),
                (Some("PR_CLOSED"), Some("PR closed outside drover"))
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
    let r = fake(NONE).reply("git -C", "");
    idle(&c, &r);
    assert!(sh().methods().iter().all(|m| m == "events"), "{:?}", sh().methods());
    assert_eq!(
        drover::drive::last_pass(&c, "sendmeter"),
        None,
        "a paused fleet had no pass"
    );

    let c = setup("drive-busy");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let busy = Fake::default().once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "4\n");
    let busy = Fake {
        replies: fake(NONE).reply("git -C", "").replies,
        ..busy
    };
    let (out, steps) = drive(&c, &busy);
    assert_eq!(steps, ["start"]);
    assert!(out.contains("REFUSED HOST_BUSY"), "{out}");
    assert!(!busy.called("git -C /tmp/does-not-matter/sendmeter worktree") && started("").is_empty());
    // Retried next pass, logged and printed once per hour.
    let busy = Fake {
        replies: fake(NONE).reply("git -C", "").replies,
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
    lease_as(&c, "drive.lock", 4242, c.now, 3600);
    let r = fake(NONE).reply("git -C", "").reply("kill -0 4242", "");
    let (out, steps) = drive(&c, &r);
    assert!(out.contains("another pass holds") && steps.is_empty(), "{out}");
    assert!(sh().methods().is_empty(), "{:?}", sh().methods());
    // #344: expired an hour ago, a live holder's lease is taken over.
    lease_as(&c, "drive.lock", 4242, c.now - 7200, 3600);
    assert_eq!(drive(&c, &r).1, ["start"]);
    assert!(lease_on(&c, "drive.lock").is_none(), "released after the pass");
    assert_eq!(drover::drive::last_pass(&c, "sendmeter"), Some(c.now));
}

/// #168: a lock with a dead pid is taken over at once, a live pid's is respected; the pass's pid is written.
#[test]
fn a_dead_pids_lock_is_taken_over_at_once_and_a_live_ones_respected() {
    let c = setup("drive-pid");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    lease_as(&c, "drive.lock", 4242, c.now, 3600);
    let live = fake(NONE).reply("git -C", "").reply("kill -0 4242", "");
    let (out, steps) = drive(&c, &live);
    assert!(out.contains("another pass holds") && steps.is_empty(), "{out}");
    assert_eq!(
        lease_on(&c, "drive.lock").unwrap().pid,
        4242,
        "a live pass's lease stays"
    );
    let dead = fake(NONE).reply("git -C", "").once("kill -0 4242", 1, "");
    let gone = || assert!(lease_on(&c, "drive.lock").is_none(), "released after the pass");
    assert_eq!(drive(&c, &dead).1, ["start"]);
    gone();
    lease_as(&c, "drive.lock", 4242, c.now, 3600);
    std::fs::remove_file(&sh().sock).unwrap();
    let bad = Fake::default().once("kill -0 4242", 1, "");
    assert!(run(&c, &bad, &["drive"]).0 != 0);
    gone();
}

#[test]
fn drive_now_on_the_board_is_paired_only_and_toasts_its_line() {
    let c = setup("drive-web");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let (f, shed) = (fake(NONE).reply("git -C", ""), sh());
    let r = shed.on(&f);
    let s = drover::serve::Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    let get = |s: &drover::serve::Server| {
        let res = s.respond("GET / HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n\r\n", c.now);
        with_queue(&String::from_utf8_lossy(&res), s, "127.0.0.1:7777", c.now) // #268: the board's Queue tab
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
    assert!(started("").is_empty());
    let res = post("127.0.0.1:7777", &cookie);
    assert!(
        res.starts_with("HTTP/1.1 303") && res.contains("sendmeter%3A%20started%20%2312"),
        "{res}"
    );
    assert!(!started("sendmeter-impl-12").is_empty());
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
        drover::drive::summary("sendmeter", out),
        "sendmeter: clean · merged #2752, #2781 · reviewed #9 · started #2755 · 4 skipped (HOST_BUSY, SERIAL_BUSY, error)"
    );
    assert_eq!(drover::drive::summary("sendmeter", ""), "sendmeter: nothing due");
}

struct Slow(Fake);
impl drover::runner::Runner for Slow {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        if prog == "git" && args.join(" ").contains(" worktree add ") {
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
        &drover::serve::Server::new(&c, &Fake::default(), Some("127.0.0.1:7777".into())),
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
    std::thread::spawn(move || drover::serve::serve_on(&c2, &Slow(fake(NONE).reply("git -C", "")), l, false));
    let send = |req: String| {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        s.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    };
    let get = |path: &str, extra: &str| send(format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n{extra}\r\n"));
    let page = get("/api/v1/state", &cookie); // #268: the Queue tab's forms send `State.mtime`
    let i = page.find("\"mtime\":\"").unwrap() + 9;
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
    let mut toast = || {
        let (mut seen, mut b) = (String::new(), [0u8; 65_536]);
        while !seen.contains("event: toast") {
            let n = live.read(&mut b).unwrap();
            assert!(n > 0, "the stream closed: {seen}");
            seen += &String::from_utf8_lossy(&b[..n]);
        }
        seen
    };
    let seen = toast();
    assert!(
        seen.contains("event: toast\ndata: \"sendmeter: started #12\""),
        "{seen}"
    );
    assert!(post().0.contains("drive%20started"), "the flag is cleared");
    toast(); // #535: that pass ends before the test does, or it writes its dir back after the sweep
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
        drover::gh::forget_pr(&c, "sendmeter", 12);
        let r = fake(NONE)
            .reply("gh pr view issue-12", &stale_pr(&sha(p), "CONFLICTING", "DIRTY"))
            .reply("git -C", "");
        let runs = started("").len();
        let s = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
        assert_eq!((s.rounds, s.updates), (3, u32::try_from(k - 1).unwrap()));
        let (_, out) = run(&c, &r, &["waiting", "sendmeter"]);
        assert!(
            out.contains("PASS · conflicts with staging · catching up next pass (≤ 5 min)"),
            "{out}"
        );
        assert!(
            out.contains("drover spawn sendmeter 12 impl --supersede --update"),
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
            assert_eq!(started("").len(), runs, "no fourth update lane");
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
    let r = fake(NONE)
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
        drover::gh::forget_pr(&c, "sendmeter", 12);
        fake(NONE)
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
        out.contains("drover spawn sendmeter 12 rev") && !out.contains("--update"),
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
fn fanout(name: &str, extra: &str, ns: &[&str]) -> drover::Ctx {
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

fn fan_fake<A: AsRef<str>, B: AsRef<str>, C: AsRef<str>>(live: &[(A, B, C)]) -> Fake {
    fake(live)
        .reply("git -C", "")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
}

fn working(c: &drover::Ctx, ns: &[u64]) -> Vec<(String, &'static str, String)> {
    let wt = |n: u64| {
        c.worktrees
            .join(format!("sendmeter/issue-{n}"))
            .to_string_lossy()
            .to_string()
    };
    ns.iter()
        .map(|n| (format!("sendmeter-impl-{n}"), "working", wt(*n)))
        .collect()
}

fn state_of(c: &drover::Ctx, n: u64) -> String {
    fold(&read(&c.state).unwrap())
        .get(&("sendmeter".to_string(), n))
        .map_or("queued".into(), |s| s.state.clone())
}

#[test]
fn one_pass_starts_every_free_item_in_order_with_a_fresh_host_sample_each() {
    let c = fanout("drive-fan-all", "", &["1", "2", "3"]);
    let r = fan_fake(NONE);
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["start", "start", "start"], "{out}");
    let runs = started("sendmeter-impl-");
    let at = |n: u64| runs.iter().position(|a| *a == format!("sendmeter-impl-{n}")).unwrap();
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
    assert!(started("sendmeter-impl-1").is_empty());
    assert_eq!((state_of(&c, 2), state_of(&c, 3)), ("impl".into(), "impl".into()));
    assert_eq!(
        run(&c, &Fake::default(), &["next", "sendmeter"]),
        (0, "1\n".into()),
        "1 stays first"
    );
    let status = run(&c, &fake(&working(&c, &[2, 3, 9])), &["status", "sendmeter"]).1;
    assert!(
        status.contains("skipped this pass: waiting for a db slot (#9 holds 1/1)"),
        "{status}"
    );
    let board = run(&c, &Fake::default(), &["board", "sendmeter"]).1;
    assert!(board.contains(" #1     queued      skip SERIAL_BUSY "), "{board}");
    // The second pass: #1 still refused (logged once an hour), nothing else free: a no-op.
    let r = fan_fake(&working(&c, &[2, 3, 9]));
    idle(&c, &r);
}

/// #572: a `SERIAL_BUSY` skip shows the group's holders now: a merged one drops out, a free slot says so.
#[test]
fn a_serial_busy_skip_shows_the_live_holders() {
    let c = fanout("drive-skip-live", SERIAL, &["9", "1", "4", "--serial"]);
    let at = |ts: u64, n: u64, kind: &str, d: serde_json::Value| {
        append(&c.state, &Event::new(ts, "sendmeter", Some(n), kind, d)).unwrap();
    };
    let spawn = |ts: u64, n: u64| {
        let d = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "branch": format!("issue-{n}")});
        at(ts, n, "spawn", d);
    };
    spawn(c.now - 600, 9);
    let why = "REFUSED SERIAL_BUSY: db: #9 holds 1/1; retry after they merge";
    at(
        c.now - 300,
        1,
        "driven",
        json!({"step": "start", "code": "SERIAL_BUSY", "result": why}),
    );
    let status = || run(&c, &Fake::default(), &["status", "sendmeter"]).1;
    let board = || run(&c, &Fake::default(), &["board", "sendmeter"]).1;
    assert!(
        status().contains("skipped this pass: waiting for a db slot (#9 holds 1/1)"),
        "{}",
        status()
    );
    at(c.now - 200, 9, "merged", json!({}));
    let s = status();
    assert!(s.contains("skipped this pass: db slot free; starts next pass"), "{s}");
    assert!(board().contains(" #1     queued      db slot free "), "{}", board());
    spawn(c.now - 100, 4);
    let s = status();
    assert!(
        s.contains("skipped this pass: waiting for a db slot (#4 holds 1/1)"),
        "{s}"
    );
    assert!(!s.contains("#9 hold"), "{s}");
    assert!(board().contains(" #1     queued      skip SERIAL_BUSY "), "{}", board());
}

/// #572: another code's skip shows its age; one older than the issue's last state change is gone.
#[test]
fn an_old_skip_shows_its_age_until_the_issue_moves() {
    let c = fanout("drive-skip-age", "", &["12"]);
    let why = "REFUSED CAP: 2 impl lane(s) open in sendmeter, max_impl = 2";
    ev(
        &c,
        c.now - 720,
        "driven",
        json!({"step": "start", "code": "CAP", "result": why}),
    );
    let status = || run(&c, &Fake::default(), &["status", "sendmeter"]).1;
    let s = status();
    assert!(
        s.contains("skipped this pass: CAP (2 impl lane(s) open in sendmeter, max_impl = 2) · 12m ago"),
        "{s}"
    );
    // Its lane started and was killed since: back in the queue, the old reason is not current.
    ev(
        &c,
        c.now - 600,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"}),
    );
    ev(
        &c,
        c.now - 500,
        "lane-killed",
        json!({"role": "impl", "agent": "sendmeter-impl-12"}),
    );
    let s = status();
    assert!(s.contains(" queued "), "{s}");
    assert!(!s.contains("skipped this pass"), "{s}");
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
    let r = fan_fake(NONE)
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
        replies: fan_fake(NONE).replies,
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
    let r = fan_fake(NONE);
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["start", "start"], "{out}");
    assert!(out.contains("sendmeter #2 start: REFUSED CAP"), "{out}");
    assert!(!out.contains("#3"), "{out}");
    assert_eq!((state_of(&c, 1), state_of(&c, 3)), ("impl".into(), "queued".into()));
    idle(&c, &fan_fake(&working(&c, &[1])));
}

/// #149: unmarked #12 at PASS + green whose PR changes `files`: one drive pass, then the `serial-joined` events (#294).
fn serial_auto(name: &str, files: &str) -> (drover::Ctx, Fake, String, Vec<Event>) {
    let cfg = CONFIG.replace(
        "integration = \"staging\"",
        &format!("integration = \"staging\"\ndriver = \"drover\"\n{SERIAL}"),
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
    let r = fake(NONE)
        .reply("gh pr view issue-12", &pr(&head, "green"))
        .reply("gh api repos/jirathip-dev/sendmeter/pulls/100/files", files)
        .reply("gh pr merge 100", "")
        .reply("gh pr view 100", r#"{"mergeCommit":{"oid":"m3rg3"}}"#)
        .reply("gh issue", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["merge"], "{out}");
    assert!(!out.contains("SERIAL_UNMARKED"), "{out}");
    assert_eq!(state(&c), "merged");
    let auto = read(&c.state)
        .unwrap()
        .to_vec()
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
    let r = fake(NONE)
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
    let r = fake(NONE)
        .reply("gh pr view issue-12", &labelled_pr(&head, &["merge-ready"]))
        .reply("gh pr edit", "")
        .reply("git -C", "");
    idle(&c, &r);
    assert!(!r.called("gh pr edit"));
    // The head moved past the PASS (a fresh PR read, as after 60 s): the label comes off, and is never added back.
    drover::gh::forget_pr(&c, "sendmeter", 12);
    let r = fake(NONE)
        .reply("gh pr view issue-12", &labelled_pr(&moved, &["merge-ready"]))
        .reply("gh pr edit", "");
    let (out, _) = drive(&c, &r);
    assert!(
        r.called("gh pr edit 100 -R jirathip-dev/sendmeter --remove-label merge-ready"),
        "{out}"
    );
    assert_eq!((kinds("labelled"), kinds("unlabelled")), (1, 1));
    drover::gh::forget_pr(&c, "sendmeter", 12);
    let r = fake(NONE)
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
    let r = fake(NONE).reply("gh pr view issue-12", &labelled_pr(&head, &[]));
    // Prime the PR cache the way a pass does; `status` reads only the cache.
    run(&c, &r, &["pr", "sendmeter", "12"]);
    let (code, out) = run(&c, &r, &["status", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("PR lacks label `merge-ready`"), "{out}");
}

/// #166: the `drive-pass` events of `c` (newest last).
fn passes(c: &drover::Ctx) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
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
            replies: fake(NONE).reply("git -C", "").replies,
            ..Default::default()
        }
        .once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "4\n")
    };
    drive(&c, &busy());
    let mut out = vec![];
    let cfg = drover::config::Config::load(&c.config).unwrap();
    // The second pass is a manual press and prints nothing (a repeat), but its record still lists the refusal.
    let (f, shed) = (busy(), sh());
    drover::drive::drive(&c, &cfg, &shed.on(&f), &mut out, Some("sendmeter"), true, false).unwrap();
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
    let (tone, line) = drover::drive::result(&p[1]);
    assert_eq!(tone, "a", "everything held back: amber");
    assert!(
        line.ends_with(" · started 0 · merged 0 · 1 waiting: Mac busy (memory pressure critical)"),
        "{line}"
    );
}

/// The other pass: it ends (its record, then the lock goes) on the press's 3rd one-second wait.
struct Other<'a>(&'a Fake, &'a drover::Ctx, std::sync::atomic::AtomicUsize);
impl drover::runner::Runner for Other<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
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
            assert!(drover::db::release(&self.1.state, "drive.lock", None));
        }
    }
}

#[test]
fn a_manual_press_that_finds_a_pass_running_waits_for_it_and_shows_its_actions() {
    let c = setup("drive-wait");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    lease_as(&c, "drive.lock", 4242, c.now, 3600);
    let cfg = drover::config::Config::load(&c.config).unwrap();
    assert!(drover::drive::running(&c, "sendmeter"));
    let (f, mut out) = (fake(NONE).reply("git -C", "").reply("kill -0 4242", ""), vec![]);
    let (r, shed) = (Other(&f, &c, 0.into()), sh());
    drover::drive::drive(&c, &cfg, &shed.on(&r), &mut out, Some("sendmeter"), true, false).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert_eq!(
        out,
        "sendmeter #12 start: started\nsendmeter #13 start: REFUSED CAP: lane limit or usage cap reached\n"
    );
    assert_eq!(r.2.into_inner(), 3, "waited 3 s, then ran no pass of its own");
    assert!(shed.methods().is_empty(), "never two passes: {:?}", shed.methods());
    assert_eq!(
        drover::drive::summary("sendmeter", &out),
        "sendmeter: started #12 · 1 skipped (CAP)"
    );
    assert_eq!(passes(&c).len(), 1);

    // Bounded: a pass still running after 120 s leaves the press with a pointer to the line.
    lease_as(&c, "drive.lock", 4242, c.now, 3600);
    let mut out = vec![];
    drover::drive::drive(&c, &cfg, &shed.on(&f), &mut out, Some("sendmeter"), true, false).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "drive: still driving · result will appear here\n"
    );
    assert_eq!(f.count("sleep 1"), 120);
}

#[test]
fn the_result_line_is_red_on_an_error_green_on_a_start_or_merge_amber_when_all_held_back() {
    let line = |acts: serde_json::Value| {
        drover::drive::result(&Event::new(
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
    // #666: idle past the handoff's 2 min, short of `stall_minutes`.
    let wt = lane(&c, c.now - 300, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("bee");
    let r = |ci: &str| {
        fake(&[("sendmeter-impl-12", "idle", &wt.to_string_lossy())])
            .reply("gh pr view issue-12", &pr(&head, ci))
            .reply(&format!("git -C {} log", wt.display()), &format!("{} 20", sha("a1")))
            .reply("git -C", "")
    };
    // The worktree is elsewhere and no handoff was logged; CI still runs at the PR head: nothing yet.
    idle(&c, &r("pending"));
    drover::gh::forget_pr(&c, "sendmeter", 12);
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
    let r = fake(NONE)
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
        replies: fake(&[("sendmeter-impl-12", "idle", &wt.to_string_lossy())])
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
    assert!(!started("sendmeter-rev-12-r1").is_empty(), "{out}");
    assert!(out.contains("alpha #5 start: REFUSED HOST_BUSY"), "{out}");
    assert_eq!(state(&c), "review(1)");
}

/// #212: `drover drive --fast` (output, `driven` steps logged by it).
fn fast(c: &drover::Ctx, r: &Fake) -> (String, Vec<String>) {
    let before = read(&c.state).unwrap().len();
    let (code, out) = run(c, r, &["drive", "--fast"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    let new = read(&c.state).unwrap().to_vec().split_off(before);
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
    lease_as(&c, "drive.lock", 4242, c.now, 3600);
    let head = sha("abc");
    let r = fake(NONE)
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
    lease_as(&c, "drive-sendmeter-12.lock", 4242, c.now, 3600);
    assert_eq!(fast(&c, &r).1, Vec::<String>::new());
    assert!(drover::db::release(&c.state, "drive-sendmeter-12.lock", None));
    let (out, steps) = fast(&c, &r);
    assert_eq!(steps, ["verdict", "merge"], "{out}");
    assert_eq!(state(&c), "merged");
    assert!(started("").is_empty() && !r.called("git -C /tmp/does-not-matter/sendmeter fetch"));
    assert_eq!(lease_on(&c, "drive.lock").unwrap().pid, 4242, "the slow lock stays");
    assert_eq!(drover::drive::last_pass(&c, "sendmeter"), None, "no slow pass ran");
    assert!(
        lease_on(&c, "drive-sendmeter-12.lock").is_none(),
        "the issue lock is released"
    );
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
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let f = cfg.fleet("sendmeter").unwrap();
    let check =
        |n: &str, st: &str, con: &str| json!({"__typename": "CheckRun", "name": n, "status": st, "conclusion": con});
    let view = |checks: Vec<serde_json::Value>| {
        json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": head, "statusCheckRollup": checks})
            .to_string()
    };
    let card = || {
        let ws = drover::waiting::scan(&c, &cfg, &Fake::default(), &read(&c.state).unwrap(), &[], false);
        ws.into_iter().find(|w| w.1.issue == 12).unwrap().1
    };
    // Cached ten minutes ago: three of five green.
    let mut five: Vec<_> = (0..3)
        .map(|i| check(&format!("c{i}"), "COMPLETED", "SUCCESS"))
        .collect();
    five.extend((3..5).map(|i| check(&format!("c{i}"), "IN_PROGRESS", "")));
    let old = Fake::default().reply("gh pr view issue-12", &view(five.clone()));
    drover::gh::pr(&c, &old, f, ("sendmeter", 12), "issue-12", c.now - 600).unwrap();
    assert_eq!(card().what, "PASS · CI running 3/5 · checked 10m ago");
    // The fast step reads it afresh: one red check.
    five[4] = check("lint", "COMPLETED", "FAILURE");
    let r = fake(NONE).reply("gh pr view issue-12", &view(five)).reply("git -C", "");
    fast(&c, &r);
    assert_eq!(drover::gh::cached_at(&c, "sendmeter", 12), Some(c.now));
    assert_eq!((card().who, card().what.as_str()), ("ci", "PASS · CI red: lint"));
    assert!(!r.called("gh run view"), "a red CI's fix round is the slow step's");
    // All green: merging within a minute.
    let green: Vec<_> = (0..5)
        .map(|i| check(&format!("c{i}"), "COMPLETED", "SUCCESS"))
        .collect();
    let now = Fake::default().reply("gh pr view issue-12", &view(green));
    drover::gh::pr(&c, &now, f, ("sendmeter", 12), "issue-12", c.now + 60).unwrap();
    assert_eq!(card().what, "PASS · CI green · merging within 1 min");
}

/// #212: a review start the host refused says so on the card, in plain words.
#[test]
fn a_refused_review_start_shows_waiting_for_a_reviewer_slot() {
    let c = setup("drive-rev-slot");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("abc");
    let r = Fake {
        replies: fake(&[("sendmeter-impl-12", "idle", &wt.to_string_lossy())])
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
    let r = fake(NONE)
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
    let r = fake(NONE)
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
    let r = fake(NONE)
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

/// #373: an auto-update re-runs the premerge checks on the updated head before it pushes; a failure is a fix round.
#[test]
fn auto_update_rechecks_premerge_before_its_push() {
    for fails in [false, true] {
        let c = setup(if fails {
            "drive-update-premerge-red"
        } else {
            "drive-update-premerge"
        });
        let cfg = CONFIG.replace(
            "integration = \"staging\"",
            "integration = \"staging\"\npremerge = [\"make lint\"]",
        );
        reconfig(&c, &cfg);
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
        let check = format!("sh -c cd \"$0\" && make lint {}", wt.display());
        let push = format!("git -C {} push -q origin HEAD:issue-12", wt.display());
        let r = fake(NONE)
            .once(&check, i32::from(fails), "lint: error: unused import\n")
            .reply("gh pr view issue-12", &pr(&head, "pending"))
            .reply(
                "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
                &format!("{head}\n"),
            )
            .reply("git -C /tmp/does-not-matter/sendmeter rev-parse", &format!("{head}\n"))
            .reply(
                &format!("git -C {} rev-parse HEAD", wt.display()),
                &format!("{}\n", sha("b2")),
            )
            .reply("git -C", "");
        let (out, _) = drive(&c, &r);
        assert!(r.called(&check), "{out}");
        assert!(!r.called("gh pr merge"), "{out}");
        if !fails {
            assert!(r.index(&check) < r.index(&push), "{out}");
            continue;
        }
        assert!(!r.called(&push), "{out}");
        assert!(
            out.contains("REFUSED STAGING_WOULD_BREAK: make lint: lint: error: unused import"),
            "{out}"
        );
        let ev = read(&c.state).unwrap();
        assert!(ev.iter().any(|e| e.kind == "premerge-failed"), "{out}");
        let v = ev.iter().rfind(|e| e.kind == "verdict").unwrap();
        assert_eq!((v.str("verdict"), v.str("sha")), (Some("FAIL"), Some(head.as_str())));
        assert!(state(&c).starts_with("fix(1)"), "{}", state(&c));
    }
}

#[test]
fn an_exited_reviewer_without_a_verdict_gets_a_new_review_round() {
    let c = setup("drive-rev-lost");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("b4e191c");
    let rev = c.worktrees.join("sendmeter/rev-12-r1");
    std::fs::create_dir_all(&rev).unwrap();
    let d = json!({"role": "rev", "agent": "sendmeter-rev-12-r1", "pane": "sendmeter-rev-12-r1", "workspace": "", "runtime": "shed",
        "worktree": rev, "branch": "rev-12-r1", "sha": head});
    ev(&c, 20, "spawn", d);
    let r = fake(NONE)
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
    assert!(!started("sendmeter-rev-12-r1").is_empty(), "{out}");
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
        let r = fake(NONE)
            .reply("gh pr view issue-12", &stale_pr(&head, mergeable, "DIRTY"))
            .reply("git -C", "");
        let (out, steps) = drive(&c, &r);
        assert_eq!(steps, [step], "{out}");
    }
    let c = setup("drive-blocked-no-pr");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(&c, 40, "blocked", json!({"code": "NO_PR", "reason": "no open PR"}));
    let r = fake(NONE).reply("git -C", "");
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

struct Spy<'a>(Fake, &'a drover::Ctx, std::sync::Mutex<Vec<bool>>);
impl drover::runner::Runner for Spy<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        let logged = read(&self.1.state).unwrap().iter().any(|e| e.kind == "round-extended");
        self.2.lock().unwrap().push(logged);
        self.0.run(prog, args)
    }
}

const LOW_MEM: &str = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free: 16384.\n\
    Pages inactive: 16384.\nSwapouts: 100.\n";

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
        fake(NONE)
            .reply("gh pr view issue-12", &pr(&sha("3"), "green"))
            .reply("git -C", "")
    };
    let low = Fake::default().reply("vm_stat", LOW_MEM);
    let low = Fake {
        replies: [low.replies, base().replies].concat(),
        ..Default::default()
    };
    let spy = Spy(low, &c, Default::default());
    let (code, out) = run(&c, &spy, &["extend", "sendmeter", "12"]);
    assert!(code == 0 && out.contains("waiting for host (MEM_LOW)"), "{out}");
    let seen = spy.2.lock().unwrap().clone();
    assert!(
        !seen.is_empty() && seen.iter().all(|&l| l),
        "a command ran before the decision: {seen:?}"
    );
    let all = read(&c.state).unwrap().to_vec();
    assert!(!all.iter().any(|e| e.kind == "extend-failed"), "{all:?}");
    let p = drover::pending::of(&all, "sendmeter", 12).expect("pending");
    assert_eq!(
        (p.action.as_str(), p.reason.as_deref(), p.failed),
        ("extend", Some("MEM_LOW"), false)
    );
    assert_eq!(state(&c), "fix(3)");

    let r = base();
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["fix"], "{out}");
    assert!(!started("").is_empty(), "{:?}", r.calls.borrow());
    assert_eq!(drover::pending::of(&read(&c.state).unwrap(), "sendmeter", 12), None);
}

#[test]
fn an_extend_during_a_pass_spawns_at_once() {
    let c = setup("drive-extend-lock");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    // #639: a fix round follows a FAIL (an extra round for a lane never reviewed goes back to impl).
    ev(
        &c,
        12,
        "verdict",
        json!({"verdict": "FAIL", "sha": sha("3"), "body": "FAIL"}),
    );
    ev(&c, 14, "needs-human", json!({"code": "UPDATE_LIMIT"}));
    lease_as(&c, "drive.lock", 4242, c.now, 3600);
    let r = fake(NONE)
        .reply("gh pr view issue-12", &pr(&sha("3"), "green"))
        .reply("kill -0 4242", "")
        .reply("git -C", "");
    let (code, out) = run(&c, &r, &["extend", "sendmeter", "12"]);
    assert!(code == 0 && out.ends_with("#12: extend\n"), "{out}");
    assert!(!started("sendmeter-impl-12").is_empty(), "{:?}", r.calls.borrow());
    assert!(lease_on(&c, "drive.lock").is_some(), "the pass's lock is left alone");
    let all = read(&c.state).unwrap();
    assert_eq!(all.iter().filter(|e| e.kind == "round-extended").count(), 1);
    assert_eq!(drover::pending::of(&all, "sendmeter", 12), None);
}

/// #217 review round 2: `extend --no-spawn` leaves the live lane alone; no drive pass supersedes it for the round.
#[test]
fn an_extend_no_spawn_is_never_a_due_fix_over_a_live_lane() {
    let c = setup("drive-extend-no-spawn");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(&c, 14, "needs-human", json!({"code": "UPDATE_LIMIT"}));
    let live = [("sendmeter-impl-12", "idle", wt.to_string_lossy())];
    let r = fake(&live)
        .reply("gh pr view issue-12", &pr(&sha("3"), "green"))
        .reply("git -C", "");
    let (code, out) = run(&c, &r, &["extend", "sendmeter", "12", "--no-spawn"]);
    assert_eq!(code, 0, "{out}");
    let (out, steps) = drive(&c, &r);
    assert!(!steps.iter().any(|s| s == "fix"), "{out}");
    assert!(
        started("sendmeter-impl-12").is_empty() && sh().all("kill").is_empty(),
        "{:?}",
        r.calls.borrow()
    );
}

// --- #319: a fix round that ends with no new commit still moves on ---

/// #12 FAILed at `head` with `body` (`stage`), then its fix lane (spawned at 40) went idle without a new commit.
fn no_change(name: &str, stage: &str, body: &str, ci: &str) -> (drover::Ctx, Fake, String) {
    no_change_in(CONFIG, name, stage, body, ci)
}

fn no_change_in(cfg: &str, name: &str, stage: &str, body: &str, ci: &str) -> (drover::Ctx, Fake, String) {
    let c = ctx(&tmpdir(name), cfg);
    let head = sha("abc");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(&c, 20, "pushed", json!({"sha": head}));
    let d = json!({"verdict": "FAIL", "sha": head, "body": body, "stage": stage, "loop": "rev"});
    ev(&c, 30, "verdict", d);
    // #666: the fix lane spawned 15 min ago: idle, not stalled yet.
    let wt = lane(&c, c.now - 900, "impl", "sendmeter-impl-12", "issue-12");
    assert_eq!(state(&c), "fix(1)");
    let r = fake(&[("sendmeter-impl-12", "idle", &wt.to_string_lossy())])
        .reply("gh pr view issue-12", &pr(&head, ci))
        .reply("gh pr merge 100", "")
        .reply("gh pr view 100", r#"{"mergeCommit":{"oid":"m3rg3"}}"#)
        .reply("gh issue", "")
        .reply("git -C", "");
    (c, r, head)
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
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
        assert!(started("sendmeter-rev").is_empty(), "{out}");
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
    let earlier = drover::Ctx {
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
    assert!(!started("sendmeter-rev-12-r2").is_empty(), "{out}");
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
    let (code, out) = run(&at(&c, 1800), &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    let s = &kinds(&c, "stall")[0];
    let want = "fix lane idle, head unchanged, CI failed: run drover done sendmeter 12 --no-change \"<why>\" or push";
    assert!(s.str("reason").unwrap().ends_with(want), "{:?}", s.data);
}

/// #445: the required `quality` runs only once a PASS adds the merge label, so at a FAILed head it never reports. A
/// `done --no-change` there is no stall asking for it again, `waiting` names the review, and drive spawns it.
#[test]
fn a_no_change_round_is_reviewed_while_a_check_that_waits_for_the_label_is_absent() {
    let cfg = CONFIG.replace(
        "integration = \"staging\"",
        "integration = \"staging\"\nmerge_label = \"merge-ready\"\nrequired_checks = [\"quality\"]",
    );
    let body = "FAIL (form-only)\n1. blocking [form]: the PR body lacks For humans";
    let (c, r, head) = no_change_in(&cfg, "drive-noop-label", "rev", body, "pending");
    let earlier = drover::Ctx {
        now: c.now - 3 * 3600,
        ..c.clone()
    };
    let (code, out) = run(
        &earlier,
        &r,
        &["done", "sendmeter", "12", "--no-change", "PR body rewritten"],
    );
    assert_eq!(code, 0, "{out}");
    // `waiting` reads the PR, as the tick does before `check`.
    let (_, out) = run(&c, &r, &["waiting", "sendmeter"]);
    let want = format!(
        "sendmeter #12 drive: no change at {:.7}, waiting for drive (next pass ≤ 5 min) to spawn review",
        head
    );
    assert!(out.contains(&want), "{out}");
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    let stalls: Vec<_> = kinds(&c, "stall")
        .iter()
        .map(|e| e.str("reason").unwrap().to_string())
        .collect();
    assert!(stalls.iter().all(|s| !s.contains("--no-change")), "{stalls:?}");
    assert!(
        stalls
            .iter()
            .any(|s| s.ends_with("fix-done recorded, waiting for drive (next pass ≤ 5 min) to spawn review")),
        "{stalls:?}"
    );
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["review"], "{out}");
    assert!(!started("sendmeter-rev-12-r2").is_empty(), "{out}");
    let e = &kinds(&c, "fix-no-change")[0];
    assert_eq!((e.str("sha"), e.str("ci")), (Some(head.as_str()), Some("pending")));
    assert!(state(&c).starts_with("review"), "{}", state(&c));
}

/// #388: a PASSed head with no checks reads `CI running` inside `ci_start_minutes`, `CI missing` after it; a pending
/// check is unchanged.
#[test]
fn no_checks_after_the_grace_is_ci_missing_not_running() {
    let c = setup("drive-ci-missing");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("abc");
    ev(
        &c,
        c.now - 300,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
    );
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let f = cfg.fleet("sendmeter").unwrap();
    let view = |checks: serde_json::Value| {
        json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": head, "statusCheckRollup": checks})
            .to_string()
    };
    let card = |c: &drover::Ctx| {
        let ws = drover::waiting::scan(c, &cfg, &Fake::default(), &read(&c.state).unwrap(), &[], false);
        ws.into_iter().find(|w| w.1.issue == 12).unwrap().1
    };
    let empty = Fake::default().reply("gh pr view issue-12", &view(json!([])));
    drover::gh::pr(&c, &empty, f, ("sendmeter", 12), "issue-12", c.now).unwrap();
    assert_eq!(
        card(&c).what,
        "PASS · CI running 0/0",
        "5 min after the PASS: still starting"
    );
    let mut later = c.clone();
    later.now = c.now + 300;
    drover::gh::pr(&c, &empty, f, ("sendmeter", 12), "issue-12", later.now).unwrap();
    assert_eq!(
        (card(&later).who, card(&later).what.as_str()),
        ("ci", "PASS · CI missing (no checks 10m after push)")
    );
    let pending = json!([{"__typename": "CheckRun", "name": "ci", "status": "IN_PROGRESS"}]);
    let r = Fake::default().reply("gh pr view issue-12", &view(pending));
    later.now += 60;
    drover::gh::pr(&c, &r, f, ("sendmeter", 12), "issue-12", later.now).unwrap();
    assert_eq!(
        card(&later).what,
        "PASS · CI running 0/1",
        "a pending check is unchanged"
    );
}

/// #388: a failed run with no jobs at the head is `CI broken: <file>`, and drive sends the lane a fix round.
#[test]
fn a_workflow_that_never_started_is_ci_broken_and_gets_a_fix_round() {
    let c = setup("drive-ci-broken");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("abc");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
    );
    let view = json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": head,
        "statusCheckRollup": []});
    let runs = format!("gh api repos/jirathip-dev/sendmeter/actions/runs?head_sha={head}");
    let r = fake(NONE)
        .reply("gh pr view issue-12", &view.to_string())
        .reply(&runs, "554 lint.yml\n555 ci.yml\n")
        .reply("gh api repos/jirathip-dev/sendmeter/actions/runs/554/jobs", "2\n")
        .reply("gh api repos/jirathip-dev/sendmeter/actions/runs/555/jobs", "0\n")
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
    let body = v.str("body").unwrap();
    assert!(body.contains("CI broken: ci.yml did not start"), "{body}");
    assert!(state(&c).starts_with("fix"), "{}", state(&c));
}

fn held(name: &str, live: &[(&str, &str, &str)]) -> (drover::Ctx, Fake) {
    let c = setup(name);
    lane(&c, c.now - 600, "impl", "sendmeter-impl-12", "issue-12");
    ev(&c, c.now - 500, "held", json!({"agent": "sendmeter-impl-12", "on": 11}));
    append(
        &c.state,
        &Event::new(c.now - 60, "sendmeter", Some(11), "merged", json!({})),
    )
    .unwrap();
    (c, fake(live))
}

/// #425: the blocker merged: drive types the wake into the held lane's pane once and logs `released`.
#[test]
fn a_held_lane_is_prompted_once_when_its_blocker_merges() {
    let (c, r) = held("drive-held-wake", &[("sendmeter-impl-12", "idle", "/x")]);
    let (out, _) = drive(&c, &r);
    let woken = || typed("sendmeter-impl-12", "#11 merged; rebase on staging and continue");
    assert_eq!(woken(), 1, "{out} {:?}", sh().methods());
    let rel = &kinds(&c, "released")[0];
    assert_eq!(
        (&rel.data["on"], rel.data["respawn"].as_bool()),
        (&json!([11]), Some(false))
    );
    drive(&c, &r);
    assert_eq!(woken(), 1, "woken once");
    assert_eq!(kinds(&c, "released").len(), 1);
}

/// #425: the held lane's pane is gone: drive respawns it `--supersede` instead, then logs `released`.
#[test]
fn a_held_lane_whose_pane_is_gone_is_respawned_when_its_blocker_merges() {
    let (c, r) = held("drive-held-respawn", NONE);
    let r = r.reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["respawn"], "{out}");
    assert!(!started("sendmeter-impl-12").is_empty(), "{out} {:?}", r.calls.borrow());
    assert_eq!(typed("sendmeter-impl-12", "#11"), 0);
    let rel = &kinds(&c, "released")[0];
    assert_eq!(rel.data["respawn"].as_bool(), Some(true));
    assert_eq!(read(&c.state).unwrap().iter().filter(|e| e.kind == "spawn").count(), 2);
}

/// #505: a lane held on two issues is woken only once both have merged.
#[test]
fn a_lane_held_on_two_blockers_waits_for_both() {
    let (c, r) = held("drive-held-two", &[("sendmeter-impl-12", "idle", "/x")]);
    ev(
        &c,
        c.now - 400,
        "held",
        json!({"agent": "sendmeter-impl-12", "on": [11, 13]}),
    );
    drive(&c, &r);
    assert!(sh().sent("sendmeter-impl-12").is_empty(), "{:?}", sh().methods());
    assert!(kinds(&c, "released").is_empty());
    let merged = Event::new(c.now - 30, "sendmeter", Some(13), "merged", json!({}));
    append(&c.state, &merged).unwrap();
    drive(&c, &r);
    let woken = typed("sendmeter-impl-12", "#11, #13 merged; rebase on staging and continue");
    assert_eq!(woken, 1, "{:?}", sh().methods());
    assert_eq!(kinds(&c, "released")[0].data["on"], json!([11, 13]));
}

// --- #444: a fix round whose lane ended without pushing is driven again ---

/// #363's log: FAIL at the head, supersede, a spawn whose prompt never landed (exit 4), its pane closed `done`.
fn ended(name: &str) -> (drover::Ctx, Fake) {
    let c = setup(name);
    let head = sha("abc");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(&c, 20, "pushed", json!({"sha": head}));
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "FAIL", "sha": head, "body": "FAIL\nform"}),
    );
    ev(&c, 31, "supersede", json!({"agent": "sendmeter-impl-12"}));
    let wt = lane(&c, 32, "impl", "sendmeter-impl-12", "issue-12");
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12", "runtime": "shed",
        "worktree": wt, "branch": "issue-12", "confirmed": false});
    ev(&c, 32, "spawn", d);
    ev(
        &c,
        40,
        "lane-closed",
        json!({"agent": "sendmeter-impl-12", "reason": "done", "key": "k"}),
    );
    let r = fake(NONE)
        .reply("gh pr view issue-12", &pr(&head, "green"))
        .reply("git -C", "");
    (c, r)
}

#[test]
fn the_363_sequence_replayed_respawns_impl_once_per_30_min() {
    let (c, r) = ended("drive-ended");
    assert_eq!(state(&c), "fix(1)");
    let (code, out) = run(&c, &r, &["waiting", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("#12 drive: lane gone, no push: respawning impl")
            && out.contains("→ drover spawn sendmeter 12 impl --supersede"),
        "{out}"
    );
    let (_, st) = run(&c, &r, &["status", "sendmeter"]);
    assert!(
        st.contains("lane gone, no push") && !st.contains("idle without pushing"),
        "{st}"
    );
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["respawn"], "{out}");
    assert!(!started("sendmeter-impl-12").is_empty(), "{out}");
    let e = &kinds(&c, "respawn")[0];
    assert_eq!((e.str("role"), e.str("why")), (Some("impl"), Some("lane gone")));
    // The new lane is gone too: no second respawn within 30 min.
    sh().exit("sendmeter-impl-12");
    idle(&c, &r);
}

#[test]
fn two_respawns_without_a_push_go_to_needs_human() {
    let (c, r) = ended("drive-ended-twice");
    let at = |s: u64| drover::Ctx {
        now: c.now + s,
        ..c.clone()
    };
    let ends = || sh().exit("sendmeter-impl-12");
    assert_eq!(drive(&c, &r).1, ["respawn"]);
    ends();
    assert_eq!(drive(&at(1801), &r).1, ["respawn"]);
    ends();
    let (code, out) = run(&at(3602), &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("needs-human: lane keeps ending without a push"), "{out}");
    assert_eq!(state(&c), "needs-human");
    assert_eq!(kinds(&c, "respawn").len(), 2);
    let nh = &kinds(&c, "needs-human")[0];
    assert_eq!(nh.str("reason"), Some("lane keeps ending without a push"));
    idle(&at(7200), &r);
    assert_eq!(kinds(&c, "needs-human").len(), 1, "it never loops");
}

#[test]
fn a_spawn_whose_prompt_never_landed_is_retried_by_the_next_pass() {
    let c = setup("drive-exit4");
    let head = sha("abc");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "FAIL", "sha": head, "body": "FAIL"}),
    );
    let wt = c.worktrees.join("sendmeter/issue-12");
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12", "runtime": "shed",
        "worktree": wt, "branch": "issue-12", "confirmed": false});
    ev(&c, 30, "spawn", d);
    // The pane is still there (the agent sits `done`, never prompted).
    let list = [("sendmeter-impl-12", "done", wt.to_string_lossy())];
    let r = fake(&list)
        .reply("gh pr view issue-12", &pr(&head, "green"))
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["respawn"], "{out}");
    assert_eq!(kinds(&c, "respawn")[0].str("why"), Some("prompt never landed"));
    let done = kinds(&c, "lane-closed");
    assert!(done.iter().all(|e| e.str("reason") != Some("done")), "{done:?}");
}

const PROOFLESS: &str = "FAIL\n\n- blocking [code] src/x.rs:9: off by one\n";
const PROVEN: &str = "FAIL\n\n- blocking [code] src/x.rs:9: off by one\n  Proof: spec\n  > AC 1: totals add up\n";

/// #491: a rev lane at `head` whose verdict.md is `body`, and a Fake on which that reviewer is gone or idle.
fn proofless(name: &str, body: &str, live: bool) -> (drover::Ctx, PathBuf, Fake) {
    let c = setup(name);
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let rev = lane(&c, 20, "rev", "sendmeter-rev-12-r1", "rev-12-r1");
    std::fs::write(rev.join("verdict.md"), body).unwrap();
    let head = sha("abc");
    let list = [("sendmeter-rev-12-r1", "idle", rev.to_string_lossy().to_string())];
    let r = fake(if live { &list[..] } else { &[] })
        .reply("gh pr view issue-12", &pr(&head, "green"))
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply(&format!("git -C {} log", rev.display()), &format!("{head} 20"))
        .reply("git -C", "");
    (c, rev, r)
}

#[test]
fn a_proofless_fail_from_an_ended_reviewer_gets_one_fresh_review_of_the_same_head() {
    let (c, rev, r) = proofless("drive-noproof-ended", PROOFLESS, false);
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["verdict", "review"], "{out}");
    assert!(out.contains("REFUSED NO_PROOF"), "{out}");
    assert_eq!(started("sendmeter-rev-12-r1").len(), 1, "{out}");
    let all = read(&c.state).unwrap();
    let d = all.iter().rfind(|e| e.kind == "driven").unwrap();
    assert_eq!(d.str("cmd"), Some("drover spawn sendmeter 12 rev --supersede"), "{out}");
    let lost = all.iter().rfind(|e| e.kind == "lane-lost").unwrap();
    assert_eq!(lost.str("why"), Some("verdict refused NO_PROOF: re-review"));
    assert_eq!(
        (kinds(&c, "verdict").len(), state(&c)),
        (0, "review(1)".into()),
        "{out}"
    );
    assert!(!rev.join("verdict.md").exists(), "the fresh review starts clean");
}

fn told() -> usize {
    typed(
        "sendmeter-rev-12-r1",
        "drover verdict refused your verdict.md: REFUSED NO_PROOF",
    )
}

#[test]
fn an_idle_reviewer_is_told_once_then_the_unchanged_file_is_not_retried_but_re_reviewed() {
    let (mut c, _, r) = proofless("drive-noproof-told", PROOFLESS, true);
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["verdict"], "{out}");
    assert_eq!(told(), 1, "{:?}", r.calls.borrow());
    assert!(started("").is_empty(), "{out}");
    // Past drive's 1 h quiet for a repeated refusal: the same file at the same head is not tried again, nor the
    // reviewer told twice; idle again, it has ended.
    c.now += 7200;
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["review"], "{out}");
    assert_eq!(told(), 1);
    assert_eq!(started("sendmeter-rev-12-r1").len(), 1, "{out}");
    assert_eq!(
        (kinds(&c, "verdict").len(), state(&c)),
        (0, "review(1)".into()),
        "{out}"
    );
}

#[test]
fn a_told_reviewers_corrected_fail_is_recorded() {
    let (c, rev, r) = proofless("drive-noproof-fixed", PROOFLESS, true);
    let (out, _) = drive(&c, &r);
    assert_eq!(told(), 1, "{out}");
    std::fs::write(rev.join("verdict.md"), PROVEN).unwrap();
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["verdict"], "{out}");
    let v = kinds(&c, "verdict");
    assert_eq!((v.len(), v[0].str("verdict")), (1, Some("FAIL")), "{out}");
    assert_eq!(state(&c), "fix(1)", "{out}");
    assert_eq!(told(), 1);
    assert!(started("sendmeter-rev").is_empty(), "{out}");
}

/// #607: a `--repro` PASS whose `Repro: run` has the command but no output blocks.
const NO_OUTPUTS: &str = "PASS\n\nRepro: run\n```\ncargo test meter_total\n```\n";

#[test]
fn a_no_repro_pass_tells_a_live_reviewer_once_and_waiting_names_the_refusal() {
    let (mut c, _, r) = proofless("drive-norepro-told", NO_OUTPUTS, true);
    let (code, out) = run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--repro"]);
    assert_eq!(code, 0, "{out}");
    let told = || {
        let refused = "drover verdict refused your verdict.md: REFUSED NO_REPRO: `Repro: run` needs";
        typed("sendmeter-rev-12-r1", refused)
    };
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["verdict"], "{out}");
    assert!(out.contains("REFUSED NO_REPRO"), "{out}");
    assert_eq!(told(), 1, "{:?}", r.calls.borrow());
    let refused = kinds(&c, "repro-refused");
    assert_eq!((refused.len(), refused[0].data["prompted"].as_bool()), (1, Some(true)));
    // The same file at the same head is not tried again, nor its reviewer told twice; `waiting` says why.
    c.now += 7200;
    let (out, _) = drive(&c, &r);
    assert_eq!(
        (told(), kinds(&c, "repro-refused").len(), kinds(&c, "verdict").len()),
        (1, 1, 0),
        "{out}"
    );
    assert!(started("sendmeter-rev").is_empty(), "{out}");
    let (_, out) = run(&c, &r, &["waiting", "sendmeter"]);
    assert!(out.contains("verdict refused NO_REPRO: `Repro: run` needs"), "{out}");
    assert!(!out.contains("recording within 1 min"), "{out}");
}

// --- #517: a lesson lane that ends with no push and no `done` ---

/// A lesson lane of #12 (no PR) and a Fake that lists it `idle` (`live`) or not at all.
fn lesson(name: &str, live: bool) -> (drover::Ctx, Fake) {
    let c = setup(name);
    let wt = lane(&c, 10, "lesson", "sendmeter-lesson-12", "issue-12");
    let list = [("sendmeter-lesson-12", "idle", wt.to_string_lossy().to_string())];
    let r = fake(if live { &list[..] } else { &[] })
        .reply("git -C", "")
        .reply("gh issue close", "");
    (c, r)
}

const CLOSE: &str = "gh issue close 12 -R jirathip-dev/sendmeter --comment no repeated class";

#[test]
fn a_lesson_lane_ending_without_a_push_is_prompted_once_then_closed_never_lane_ends() {
    let (c, r) = lesson("drive-lesson-ended", true);
    let at = |s: u64| drover::Ctx {
        now: c.now + s,
        ..c.clone()
    };
    let (_, out) = run(&c, &r, &["waiting", "sendmeter"]);
    assert!(
        out.contains("#12 drive: lesson lane ended without a push: prompting it once"),
        "{out}"
    );
    let (out, _) = drive(&c, &r);
    assert!(out.contains("sendmeter #12 lesson lane prompted: sent"), "{out}");
    let prompt = || {
        let line = "no repeated class? end with `drover done --no-change \"no repeated class\" sendmeter 12`";
        typed("sendmeter-lesson-12", line)
    };
    assert_eq!(prompt(), 1, "{:?}", sh().methods());
    // Unanswered for under 30 min: nothing more, the prompt is never sent twice.
    idle(&at(600), &r);
    assert_eq!(prompt(), 1);
    assert!(!r.called("gh issue close"));
    // 30 min on: closed as "no repeated class", never respawned or parked `LANE_ENDS`.
    let (out, _) = drive(&at(1801), &r);
    assert!(out.contains("closed sendmeter#12: no repeated class"), "{out}");
    assert!(r.called(CLOSE), "{:?}", r.calls.borrow());
    for s in [3602, 7200] {
        drive(&at(s), &r);
    }
    assert_eq!(state(&c), "closed");
    assert!(kinds(&c, "needs-human").is_empty() && kinds(&c, "respawn").is_empty());
    assert_eq!((prompt(), r.count(CLOSE)), (1, 1));
}

#[test]
fn a_gone_lesson_lane_is_closed_not_respawned() {
    let (c, r) = lesson("drive-lesson-gone", false);
    let (out, steps) = drive(&c, &r);
    assert!(
        steps.is_empty() && out.contains("closed sendmeter#12: no repeated class"),
        "{out}"
    );
    assert!(started("").is_empty() && sh().all("send").is_empty(), "{out}");
    assert_eq!(state(&c), "closed");
    assert!(kinds(&c, "needs-human").is_empty() && kinds(&c, "respawn").is_empty());
}

/// The install repair: a lesson issue parked `LANE_ENDS` before #517 is closed as "no repeated class".
#[test]
fn a_lesson_issue_parked_lane_ends_is_closed_on_install() {
    let (c, r) = lesson("drive-lesson-repair", false);
    let d = json!({"code": "LANE_ENDS", "reason": "lane keeps ending without a push", "agent": "sendmeter-lesson-12"});
    ev(&c, 20, "needs-human", d);
    assert_eq!(state(&c), "needs-human");
    let (out, _) = drive(&c, &r);
    assert!(out.contains("closed sendmeter#12: no repeated class"), "{out}");
    assert!(r.called(CLOSE), "{:?}", r.calls.borrow());
    assert_eq!(state(&c), "closed");
}

fn old_reviewer(name: &str, status: &str) -> (drover::Ctx, Fake) {
    let c = setup(name);
    let rev = lane(&c, 5, "rev", "sendmeter-rev-12-r1", "rev-12-r1");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("abc");
    let r = fake(&[
        ("sendmeter-impl-12", "idle", &wt.to_string_lossy()),
        ("sendmeter-rev-12-r1", status, &rev.to_string_lossy()),
    ])
    .reply("gh pr view issue-12", &pr(&head, "green"))
    .reply(&format!("git -C {} log", wt.display()), &format!("{head} 20"))
    .reply("git -C", "");
    (c, r)
}

#[test]
fn a_done_reviewer_in_the_way_is_superseded_and_the_review_spawns() {
    let (c, r) = old_reviewer("drive-old-rev", "done");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["review", "review"], "{out}");
    let all = read(&c.state).unwrap();
    let sup = all.iter().find(|e| e.kind == "supersede").expect(&out);
    assert_eq!(sup.str("agent"), Some("sendmeter-rev-12-r1"));
    assert!(sh().called("kill", "sendmeter-rev-12-r1"), "{:?}", r.calls.borrow());
    assert!(!started("sendmeter-rev-12-r1").is_empty());
    assert_eq!(state(&c), "review(1)");
}

#[test]
fn a_working_reviewer_in_the_way_still_waits() {
    let (c, r) = old_reviewer("drive-busy-rev", "working");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["review"], "{out}");
    assert!(out.contains("REFUSED LANE_LIVE: live agent(s)"), "{out}");
    assert!(!read(&c.state).unwrap().iter().any(|e| e.kind == "supersede"));
    assert!(started("").is_empty());
    assert_eq!(state(&c), "impl");
}

#[test]
fn the_replayed_cooee_1_log_recovers_in_one_pass() {
    let (c, r) = old_reviewer("drive-cooee-1", "done");
    let old = "REFUSED LANE_LIVE: finished (done) lane(s) [\"sendmeter-rev-12-r1\"] in /wt/rev-12-r1; `--supersede` \
        replaces it";
    for ts in [20, 600, 1200] {
        let d = json!({"step": "review", "cmd": "drover spawn sendmeter 12 rev", "result": old, "code": "LANE_LIVE"});
        ev(&c, ts, "driven", d);
        ev(
            &c,
            ts,
            "kick",
            json!({"why": "drive", "result": "sendmeter: 1 skipped (LANE_LIVE)"}),
        );
    }
    assert_eq!(run(&c, &r, &["pr", "sendmeter", "12"]).0, 0); // the cached PR status reads
    let (code, status) = run(&c, &r, &["status", "sendmeter"]);
    assert_eq!(code, 0, "{status}");
    assert!(
        status.contains("an old reviewer (done) is in the way: replacing it"),
        "{status} {:?}",
        r.calls.borrow()
    );
    let (out, _) = drive(&c, &r);
    assert!(!started("sendmeter-rev-12-r1").is_empty(), "{out}");
    assert_eq!(state(&c), "review(1)");
}

/// #558: one process's passes follow fleet.toml, each read afresh: a fleet appended between two passes has its first
/// queued issue started by the next one (`waiting` says drive hasn't passed it yet till then); removed, no more passes.
#[test]
fn passes_follow_a_fleet_added_then_removed_in_fleet_toml() {
    let c = setup("drive-fleets");
    let r = fake(NONE).reply("git -C", "");
    drive(&c, &r);
    let passes = || {
        let e = read(&c.state).unwrap();
        e.iter()
            .filter(|e| e.kind == "drive-pass" && e.fleet == "drover")
            .count()
    };
    let added = CONFIG
        .split("\n[limits]")
        .next()
        .unwrap()
        .replace("sendmeter", "drover");
    reconfig(&c, &format!("{CONFIG}\n{added}"));
    assert_eq!(run(&c, &Fake::default(), &["queue", "drover", "add", "12"]).0, 0);
    let (_, out) = run(&c, &r, &["waiting", "drover"]);
    assert!(
        out.contains("drover #12 drive: next in the queue: drive starts it")
            && out.contains("→ drive hasn't passed this fleet yet: drover drive drover"),
        "{out}"
    );
    let (out, _) = drive(&c, &r);
    assert!(out.contains("drover #12 start: "), "{out}");
    assert!(!started("drover-impl-12").is_empty(), "{out}");
    assert_eq!(passes(), 1);
    reconfig(&c, CONFIG);
    drive(&c, &r);
    assert_eq!(passes(), 1, "a removed fleet gets no pass");
}

/// #665: #12 held until #11 merges, its lane ended (PASS, no active lane). #11 merges and #12's PR conflicts:
/// the next pass releases the hold and spawns the update round, and `waiting` stops saying "held".
#[test]
fn a_hold_whose_blocker_merged_is_released_and_its_conflicting_pr_updated() {
    let c = setup("drive-held-merged-conflict");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(&c, 20, "held", json!({"agent": "sendmeter-impl-12", "on": [11]}));
    let head = sha("a1");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
    );
    let merged = Event::new(c.now - 1800, "sendmeter", Some(11), "merged", json!({}));
    append(&c.state, &merged).unwrap();
    let r = fake(NONE)
        .reply("gh pr view issue-12", &stale_pr(&head, "CONFLICTING", "DIRTY"))
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["conflict"], "{out}");
    assert_eq!(state(&c), "fix(1)+u1");
    let (_, out) = run(&c, &r, &["waiting", "sendmeter"]);
    assert!(!out.contains("held"), "{out}");
}

/// #665: a pass ran after #11 merged and #12 still says held: the release logs `hold-stale` once, both numbers.
#[test]
fn a_hold_outliving_a_pass_past_its_blockers_merge_is_logged_hold_stale_once() {
    let c = setup("drive-hold-stale");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(&c, 20, "held", json!({"agent": "sendmeter-impl-12", "on": [11]}));
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": sha("a1"), "body": "PASS"}),
    );
    let merged = Event::new(c.now - 1800, "sendmeter", Some(11), "merged", json!({}));
    append(&c.state, &merged).unwrap();
    let pass = Event::new(c.now - 1500, "sendmeter", None, "drive-pass", json!({}));
    append(&c.state, &pass).unwrap();
    let r = fake(NONE)
        .reply("gh pr view issue-12", &stale_pr(&sha("a1"), "CONFLICTING", "DIRTY"))
        .reply("git -C", "");
    drive(&c, &r);
    drive(&c, &r);
    let stale = kinds(&c, "hold-stale");
    assert_eq!(stale.len(), 1, "logged once");
    assert_eq!((stale[0].issue, &stale[0].data["on"]), (Some(12), &json!([11])));
}

/// #712: a parked issue is skipped; `unpark` (a paired device's [unpark], never an unpaired one) puts it back in the
/// state it had before its needs-human, in its queue place, and the next pass starts it.
#[test]
fn an_unparked_issue_goes_back_to_its_state_and_the_next_pass_starts_it() {
    let c = setup("drive-unpark");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    ev(&c, 10, "needs-human", json!({"code": "START_FAILED"}));
    let (code, out) = run(&c, &Fake::default(), &["unpark", "sendmeter", "12"]);
    assert!(
        code == 3 && out.contains("REFUSED NOT_PARKED: #12 is needs-human, not parked"),
        "{out}"
    );
    let park = ["park", "sendmeter", "12", "--why", "host mini is down"];
    assert_eq!(run(&c, &Fake::default(), &park).0, 0);
    let (code, out) = run(&c, &Fake::default(), &["queue", "sendmeter", "ls"]);
    assert!(code == 0 && out.contains("parked: host mini is down"), "{out}");
    let r = fake(NONE).reply("git -C", "");
    assert!(drive(&c, &r).1.is_empty());
    assert!(started("").is_empty(), "a parked issue is skipped");

    let s = drover::serve::Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    let page = with_queue(
        &String::from_utf8_lossy(&s.respond("GET / HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n\r\n", c.now)),
        &s,
        "127.0.0.1:7777",
        c.now,
    );
    assert!(
        page.contains("<span class=r>parked: host mini is down</span>"),
        "{page}"
    );
    assert!(page.contains("<input type=hidden name=op value=unpark>"), "{page}");
    let i = page.find("name=mtime value=").unwrap() + 17;
    let mtime: String = page[i..].chars().take_while(char::is_ascii_digit).collect();
    let body = format!("op=unpark&fleet=sendmeter&issue=12&mtime={mtime}");
    let post = |extra: &str| {
        let req = format!(
            "POST / HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n{extra}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        String::from_utf8_lossy(&s.respond(&req, c.now)).into_owned()
    };
    assert!(post("").starts_with("HTTP/1.1 401"), "an unpaired device cannot unpark");
    assert_eq!(state(&c), "parked");
    let cookie = pair(&s, &c, "", c.now);
    let res = post(&cookie);
    assert!(res.starts_with("HTTP/1.1 303") && res.contains("unparked"), "{res}");
    assert_eq!(state(&c), "queued");
    let all = read(&c.state).unwrap();
    let kinds: Vec<(&str, Option<&str>)> = (all.iter())
        .filter(|e| ["parked", "unparked"].contains(&e.kind.as_str()))
        .map(|e| (e.kind.as_str(), e.str("by")))
        .collect();
    assert_eq!(kinds, [("parked", Some("owner")), ("unparked", Some("owner"))]);
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["start"], "{out}");
    assert!(!started("sendmeter-impl-12").is_empty());
}

#[test]
fn a_conflicting_pass_with_an_idle_impl_lane_gets_its_update_in_one_pass() {
    let c = setup("drive-conflict-idle");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(
        &c,
        20,
        "verdict",
        json!({"verdict": "PASS", "sha": sha("a1"), "body": "PASS"}),
    );
    ev(
        &c,
        21,
        "blocked",
        json!({"code": "NOT_MERGEABLE", "reason": "a100000 conflicts with origin/staging"}),
    );
    ev(
        &c,
        22,
        "lane-state",
        json!({"agent": "sendmeter-impl-12", "state": "hibernated"}),
    );
    let r = fake(&[("sendmeter-impl-12", "hibernated", &*wt.to_string_lossy())])
        .reply("gh pr view issue-12", &stale_pr(&sha("a1"), "CONFLICTING", "DIRTY"))
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["conflict"], "{out}");
    assert_eq!(state(&c), "fix(1)+u1", "{out}");
    let (_, out) = run(&c, &r, &["waiting", "sendmeter"]);
    assert!(!out.contains("catching up next pass"), "{out}");
}

#[test]
fn a_refused_update_round_shows_its_reason_not_catching_up() {
    let c = setup("drive-conflict-held");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(
        &c,
        20,
        "verdict",
        json!({"verdict": "PASS", "sha": sha("a1"), "body": "PASS"}),
    );
    let busy = Fake::default().once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "4\n");
    let busy = Fake {
        replies: fake(NONE)
            .reply("gh pr view issue-12", &stale_pr(&sha("a1"), "CONFLICTING", "DIRTY"))
            .reply("git -C", "")
            .replies,
        ..busy
    };
    let (out, steps) = drive(&c, &busy);
    assert_eq!(steps, ["conflict"], "{out}");
    assert!(out.contains("REFUSED HOST_BUSY"), "{out}");
    let (_, out) = run(&c, &busy, &["waiting", "sendmeter"]);
    assert!(out.contains("PASS · conflicts with staging · held: HOST_BUSY"), "{out}");
    assert!(!out.contains("catching up next pass"), "{out}");
}

fn draft_pr(head: &str, ci: &str) -> String {
    let mut v: serde_json::Value = serde_json::from_str(&pr(head, ci)).unwrap();
    v["isDraft"] = json!(true);
    v.to_string()
}

#[test]
fn a_draft_pr_with_a_pass_is_marked_ready_once_then_merged() {
    let c = setup("drive-839-draft");
    lane(&c, 10, "impl", "sendmeter-impl-12", "gone-12");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/gone-12")).unwrap();
    let head = sha("abc");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
    );
    let ready = "gh pr ready 100 -R jirathip-dev/sendmeter";
    let kinds = |k: &str| read(&c.state).unwrap().iter().filter(|e| e.kind == k).count();
    let r = fake(NONE)
        .reply("gh pr view issue-12", &draft_pr(&head, "green"))
        .reply("gh pr ready 100", "")
        .reply("gh pr merge 100", "")
        .reply("gh pr view 100", r#"{"mergeCommit":{"oid":"m3rg3"}}"#)
        .reply("gh issue", "");
    let (out, _) = drive(&c, &r);
    assert_eq!(r.count(ready), 1, "{out}");
    assert!(!r.called("gh pr merge"), "a draft never merges: {out}");
    assert_eq!(kinds("marked-ready"), 1);
    drover::gh::forget_pr(&c, "sendmeter", 12);
    let r = fake(NONE)
        .reply("gh pr view issue-12", &pr(&head, "green"))
        .reply("gh pr ready 100", "")
        .reply("gh pr merge 100", "")
        .reply("gh pr view 100", r#"{"mergeCommit":{"oid":"m3rg3"}}"#)
        .reply("gh issue", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["merge"], "{out}");
    assert!(!r.called(ready), "marked ready once only: {out}");
    assert_eq!(kinds("marked-ready"), 1);
    assert_eq!(state(&c), "merged");
}

#[test]
fn a_draft_pr_that_cannot_be_marked_ready_says_so_in_status() {
    let c = setup("drive-839-draft-stuck");
    lane(&c, 10, "impl", "sendmeter-impl-12", "gone-12");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/gone-12")).unwrap();
    let head = sha("abc");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
    );
    let r = fake(NONE)
        .reply("gh pr view issue-12", &draft_pr(&head, "pending"))
        .once_err("gh pr ready 100", 1, "GraphQL: Resource not accessible by integration");
    let (out, _) = drive(&c, &r);
    assert!(r.called("gh pr ready 100"), "{out}");
    let (code, out) = run(&c, &r, &["status", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("PR is a draft"), "{out}");
    assert!(!out.contains("merging"), "{out}");
}
