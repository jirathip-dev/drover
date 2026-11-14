mod common;
use common::*;
use drover::config::Config;
use drover::events::{append, fold, read, Event};
use serde_json::{json, Value};

const HEAD: &str = "1111111111111111111111111111111111111111";
const NEW: &str = "3333333333333333333333333333333333333333";
const OLD: &str = "2222222222222222222222222222222222222222";
const REPO: &str = "jirathip-dev/sendmeter";
const DIR: &str = "/tmp/does-not-matter/sendmeter";

fn setup(name: &str, land: bool, verdict: &str) -> (std::path::PathBuf, drover::Ctx) {
    let d = tmpdir(name);
    let text = if land {
        let extra = format!(
            "merge_label = \"merge-ready\"\n\n[fleet.sendmeter.land]\nmode = \"fast-first\"\nfast = [\"unit\"]\n\
             full_first_paths = [\"db/**\"]\n\n[[fleet.sendmeter.notify]]\ncmd = \"cat >> '{}'\"\non = [\"merged\"]\n\n\
             [fleet.sendmeter.role.impl]",
            d.join("alerts.txt").display()
        );
        CONFIG.replacen("[fleet.sendmeter.role.impl]", &extra, 1)
    } else {
        CONFIG.to_string()
    };
    let c = ctx(&d, &text);
    let spawn = json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"});
    append(&c.state, &Event::new(10, "sendmeter", Some(12), "spawn", spawn)).unwrap();
    let v = json!({"verdict": "PASS", "sha": verdict});
    append(&c.state, &Event::new(20, "sendmeter", Some(12), "verdict", v)).unwrap();
    (d, c)
}

fn check(name: &str, status: &str, conclusion: &str) -> Value {
    json!({"__typename": "CheckRun", "name": name, "status": status, "conclusion": conclusion,
        "startedAt": "2026-11-04T10:00:00Z", "completedAt": "2026-11-04T10:03:00Z",
        "detailsUrl": "https://github.com/jirathip-dev/sendmeter/actions/runs/99/job/1"})
}

fn fast_green() -> Value {
    json!([check("unit", "COMPLETED", "SUCCESS"), check("e2e", "IN_PROGRESS", "")])
}

fn pr(head: &str, checks: Value, files: &str) -> Fake {
    let pr = json!({"number": 34, "state": "OPEN", "baseRefName": "staging", "headRefOid": head,
        "statusCheckRollup": checks});
    Fake::default()
        .reply("gh pr view issue-12", &pr.to_string())
        .reply("gh api repos/jirathip-dev/sendmeter/pulls/34/files", files)
        .reply("gh pr merge 34", "")
        .reply("gh pr view 34", &json!({"mergeCommit": {"oid": "m3rg3"}}).to_string())
        .reply("gh issue", "")
}

fn state(c: &drover::Ctx, n: u64) -> String {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), n)]
        .state
        .clone()
}

fn kinds(c: &drover::Ctx) -> Vec<String> {
    read(&c.state).unwrap().to_vec().into_iter().map(|e| e.kind).collect()
}

fn event(c: &drover::Ctx, kind: &str) -> Option<Event> {
    read(&c.state).unwrap().to_vec().into_iter().rfind(|e| e.kind == kind)
}

#[test]
fn a_pass_lands_once_its_fast_checks_are_green() {
    let (_d, c) = setup("land-fast", true, HEAD);
    let r = pr(HEAD, fast_green(), "src/a.rs\n").reply("git", "");
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    assert!(r.called(&format!("git -C {DIR} merge-base --is-ancestor origin/staging {HEAD}")));
    assert!(r.called(&format!("gh pr merge 34 -R {REPO} --merge --match-head-commit {HEAD}")));
    let l = event(&c, "landed").expect("landed");
    assert_eq!((l.str("mode"), l.str("sha")), (Some("fast-first"), Some("m3rg3")));
    assert_eq!(l.val("checks_secs"), 180);
    assert_eq!(state(&c, 12), "merged");
}

#[test]
fn a_pr_touching_full_first_paths_waits_for_the_full_suite() {
    let (_d, c) = setup("land-full-path", true, HEAD);
    let r = pr(HEAD, fast_green(), "src/a.rs\ndb/migrations/1.sql\n").reply("git", "");
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 3, "{out}");
    assert!(out.contains("REFUSED CI_NOT_GREEN") && out.contains("e2e"), "{out}");
    assert!(!r.called("gh pr merge"));
    assert_eq!(state(&c, 12), "ready", "a running full suite is no block");
}

#[test]
fn the_pass_must_be_at_the_exact_head_and_fast_red_is_no_landing() {
    let (_d, c) = setup("land-stale", true, OLD);
    let r = pr(HEAD, fast_green(), "src/a.rs\n").reply("git", "");
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 3, "{out}");
    assert!(out.contains("REFUSED STALE_VERDICT"), "{out}");
    let (_d, c) = setup("land-fast-red", true, HEAD);
    let red = json!([
        check("unit", "COMPLETED", "FAILURE"),
        check("e2e", "COMPLETED", "SUCCESS")
    ]);
    let r = pr(HEAD, red, "src/a.rs\n").reply("git", "");
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 3, "{out}");
    assert!(out.contains("REFUSED CI_NOT_GREEN") && out.contains("unit"), "{out}");
    assert!(!r.called("gh pr merge"));
}

#[test]
fn default_config_still_needs_the_full_suite() {
    let (_d, c) = setup("land-default", false, HEAD);
    let r = pr(HEAD, fast_green(), "src/a.rs\n").reply("git", "");
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 3, "{out}");
    assert!(out.contains("REFUSED CI_NOT_GREEN"), "{out}");
    assert!(
        !r.called("gh api repos/jirathip-dev/sendmeter/pulls/") && !r.called("git -C"),
        "{:?}",
        r.calls.borrow()
    );
    assert!(!kinds(&c).contains(&"landed".to_string()));
}

#[test]
fn integration_moved_catches_up_and_lands_on_the_fast_checks_again() {
    let (d, c) = setup("land-swap", true, HEAD);
    let r = pr(HEAD, fast_green(), "src/a.rs\n")
        .once("git -C /tmp/does-not-matter/sendmeter merge-base --is-ancestor", 1, "")
        .reply("git", "");
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 3, "{out}");
    assert!(out.contains("REFUSED BEHIND"), "{out}");
    assert!(!r.called("gh pr merge"));
    assert_eq!(state(&c, 12), "ready");
    let wt = d.join("state/update-sendmeter");
    let r = Fake::default()
        .once(&format!("git -C {DIR} rev-parse origin/issue-12"), 0, HEAD)
        .once(&format!("git -C {} rev-parse HEAD", wt.display()), 0, NEW)
        .reply("gh api repos/jirathip-dev/sendmeter/pulls/34/files", "src/a.rs\n")
        .reply("git", "");
    let cfg = Config::load(&c.config).unwrap();
    drover::drive::auto_update(&c, &cfg, &r, &mut Vec::new(), "sendmeter").unwrap();
    assert!(r.called(&format!("git -C {} push -q origin HEAD:issue-12", wt.display())));
    assert!(!r.called("gh pr edit"), "{:?}", r.calls.borrow());
    let u = event(&c, "auto-updated").expect("auto-updated");
    assert_eq!((u.str("from"), u.str("to")), (Some(HEAD), Some(NEW)));
    let c = drover::Ctx { now: c.now + 120, ..c };
    let r = pr(NEW, fast_green(), "src/a.rs\n")
        .reply(
            &format!("git -C {DIR} rev-list --parents -n1 {NEW}"),
            &format!("{NEW} {HEAD} b4se"),
        )
        .reply(&format!("git -C {DIR} merge-tree"), "tree")
        .reply(&format!("git -C {DIR} rev-parse {NEW}^{{tree}}"), "tree")
        .reply("git", "");
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    assert!(r.called(&format!("gh pr merge 34 -R {REPO} --merge --match-head-commit {NEW}")));
}

fn landed(c: &drover::Ctx, rows: &[(u64, &str, &str)]) {
    for (i, (n, mode, sha)) in rows.iter().enumerate() {
        let ts = 100 + i as u64;
        let d = json!({"pr": format!("{}", 30 + n), "merge_sha": sha});
        append(&c.state, &Event::new(ts, "sendmeter", Some(*n), "merged", d)).unwrap();
        let d = json!({"mode": mode, "sha": sha, "pr": format!("{}", 30 + n)});
        append(&c.state, &Event::new(ts, "sendmeter", Some(*n), "landed", d)).unwrap();
    }
}

fn runs(checks: Value) -> String {
    checks.to_string()
}

fn watch(c: &drover::Ctx, r: &Fake) -> String {
    let cfg = Config::load(&c.config).unwrap();
    let mut out = Vec::new();
    drover::land::watch(c, &cfg, r, &mut out, "sendmeter").unwrap();
    String::from_utf8(out).unwrap()
}

fn integration(tip: &str, suites: &[(&str, Value)]) -> Fake {
    let mut r = Fake::default().reply(&format!("gh api repos/{REPO}/branches/staging"), tip);
    for (sha, checks) in suites {
        r = r.reply(
            &format!("gh api repos/{REPO}/commits/{sha}/check-runs"),
            &runs(checks.clone()),
        );
    }
    r
}

fn red() -> Value {
    json!([
        check("unit", "COMPLETED", "SUCCESS"),
        check("e2e", "COMPLETED", "FAILURE")
    ])
}

fn green() -> Value {
    json!([
        check("unit", "COMPLETED", "SUCCESS"),
        check("e2e", "COMPLETED", "SUCCESS")
    ])
}

#[test]
fn a_single_culprit_is_reverted_and_its_issue_reopened_with_the_log() {
    let (d, c) = setup("land-revert", true, HEAD);
    landed(&c, &[(12, "fast-first", "m1")]);
    let r = integration("m1", &[("m1", red())])
        .reply(
            &format!("gh pr create -R {REPO}"),
            "https://github.com/jirathip-dev/sendmeter/pull/77\n",
        )
        .reply("git", "");
    let out = watch(&c, &r);
    assert!(out.contains("revert PR #77"), "{out}");
    assert!(r.called(&format!(
        "git -C {}/state/revert-sendmeter revert -m 1 --no-edit m1",
        d.display()
    )));
    let p = event(&c, "land-paused").expect("paused");
    assert_eq!((p.str("red_sha"), p.str("job")), (Some("m1"), Some("e2e")));
    assert_eq!(event(&c, "revert-opened").unwrap().str("pr"), Some("77"));
    let c = drover::Ctx { now: c.now + 60, ..c };
    let rv =
        json!({"state": "OPEN", "headRefOid": "r3v", "statusCheckRollup": [check("unit", "COMPLETED", "SUCCESS")]});
    let r = integration("m1", &[("m1", red())])
        .reply("gh pr view 77", &rv.to_string())
        .reply("gh pr merge 77", "")
        .reply("gh run view 99", "e2e\tboom: assertion failed\n")
        .reply("gh issue reopen", "");
    watch(&c, &r);
    assert!(r.called(&format!("gh pr merge 77 -R {REPO} --merge --match-head-commit r3v")));
    assert!(r.called(&format!("gh issue reopen 12 -R {REPO}")));
    assert!(!r.called("gh pr create"), "one revert per culprit");
    let rv = event(&c, "reverted").expect("reverted");
    assert_eq!(
        (rv.str("pr"), rv.str("culprit"), rv.str("red_job")),
        (Some("77"), Some("m1"), Some("e2e"))
    );
    assert!(rv.ts - p.ts <= 600, "the revert lands within 10 min of the red result");
    assert_eq!(state(&c, 12), "fix(2)");
    let v = event(&c, "verdict").unwrap();
    assert!(v.str("body").unwrap().contains("boom: assertion failed"), "{v:?}");
    let ev = read(&c.state).unwrap();
    let s = &fold(&ev)[&("sendmeter".to_string(), 12)];
    let merged = json!({"number": 34, "state": "MERGED", "headRefOid": HEAD});
    let cfg = Config::load(&c.config).unwrap();
    let w = drover::waiting::waiting_on(
        &drover::waiting::Log::new(&ev),
        &ev,
        s,
        None,
        &merged,
        &cfg.fleet["sendmeter"],
        None,
    );
    assert!(
        w.next_cmd.as_deref().is_some_and(|c| c.contains("spawn sendmeter 12")),
        "{w:?}"
    );
    assert_eq!(
        drover::land::note(&ev, "sendmeter", 12).as_deref(),
        Some("reverted: e2e red")
    );
    let alerts = std::fs::read_to_string(d.join("alerts.txt")).unwrap();
    assert!(
        alerts.contains("land-paused") && alerts.contains("reverted"),
        "{alerts}"
    );
}

#[test]
fn several_landings_bisect_to_the_first_red_one() {
    let (_d, c) = setup("land-bisect", true, HEAD);
    landed(
        &c,
        &[
            (12, "fast-first", "m1"),
            (13, "fast-first", "m2"),
            (14, "fast-first", "m3"),
        ],
    );
    let r = integration("m3", &[("m1", green()), ("m2", json!([])), ("m3", red())])
        .reply("gh run view 99", "{\"workflowName\": \"CI\"}")
        .reply("gh workflow run", "")
        .reply("git", "");
    watch(&c, &r);
    assert!(r.called(&format!(
        "git -C {DIR} push -q -f origin m2:refs/heads/drover-bisect-m2"
    )));
    assert!(r.called(&format!("gh workflow run CI -R {REPO} --ref drover-bisect-m2")));
    assert!(!r.called("gh pr create"));
    assert_eq!(event(&c, "land-bisect").unwrap().issue, Some(13));
    let r = integration("m3", &[("m1", green()), ("m2", red()), ("m3", red())])
        .reply(
            &format!("gh pr create -R {REPO}"),
            "https://github.com/jirathip-dev/sendmeter/pull/78\n",
        )
        .reply("git", "");
    watch(&c, &r);
    let o = event(&c, "revert-opened").expect("revert");
    assert_eq!((o.issue, o.str("culprit")), (Some(13), Some("m2")));
    assert_eq!(kinds(&c).iter().filter(|k| *k == "land-paused").count(), 1);
}

#[test]
fn red_pauses_fast_first_landings_and_green_resumes_them() {
    let (_d, c) = setup("land-pause", true, HEAD);
    landed(&c, &[(13, "fast-first", "m1")]);
    let infra = json!([
        check("unit", "COMPLETED", "SUCCESS"),
        check("e2e", "COMPLETED", "CANCELLED")
    ]);
    let r = integration("m1", &[("m1", infra.clone())]).reply("gh run rerun", "");
    watch(&c, &r);
    assert!(r.called(&format!("gh run rerun 99 -R {REPO} --failed")));
    assert!(event(&c, "land-paused").is_none());
    let r = integration("m1", &[("m1", infra)])
        .reply(
            &format!("gh pr create -R {REPO}"),
            "https://github.com/jirathip-dev/sendmeter/pull/79\n",
        )
        .reply("git", "");
    watch(&c, &r);
    assert!(!r.called("gh run rerun"), "re-run once only");
    let ev = read(&c.state).unwrap();
    assert!(drover::land::paused(&ev, "sendmeter").is_some());
    let r = pr(HEAD, fast_green(), "src/a.rs\n").reply("git", "");
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 3, "{out}");
    assert!(out.contains("REFUSED CI_NOT_GREEN"), "{out}");
    let r = integration("r3v", &[("r3v", green())]).reply("gh pr view 79", "{}");
    let out = watch(&c, &r);
    assert!(out.contains("landing resumed"), "{out}");
    let ev = read(&c.state).unwrap();
    assert!(drover::land::paused(&ev, "sendmeter").is_none());
    assert_eq!(event(&c, "land-resumed").unwrap().str("sha"), Some("r3v"));
    let r = Fake::default().reply("gh pr view 79", "{}");
    watch(&c, &r);
    assert!(
        r.calls.borrow().iter().all(|c| c.starts_with("gh pr view 79")),
        "{:?}",
        r.calls.borrow()
    );
}

#[test]
fn a_full_first_landing_is_never_reverted_only_fixed() {
    let (_d, c) = setup("land-full-fix", true, HEAD);
    landed(&c, &[(12, "full-first", "m1")]);
    let r = integration("m1", &[("m1", red())])
        .reply("gh run view 99", "e2e\tmigration broke\n")
        .reply("gh issue reopen", "")
        .reply("git", "");
    watch(&c, &r);
    assert!(
        !r.called("gh pr create") && !r.called("git -C"),
        "{:?}",
        r.calls.borrow()
    );
    assert!(r.called(&format!("gh issue reopen 12 -R {REPO}")));
    assert!(event(&c, "reverted").is_none());
    assert_eq!(event(&c, "land-fix").unwrap().str("culprit"), Some("m1"));
    assert!(state(&c, 12).starts_with("fix("));
    let r = integration("m1", &[("m1", red())]);
    watch(&c, &r);
    assert!(!r.called("gh issue reopen"));
    assert_eq!(kinds(&c).iter().filter(|k| *k == "land-fix").count(), 1);
}

#[test]
fn stats_land_reports_both_modes() {
    let (_d, c) = setup("land-stats", true, HEAD);
    landed(&c, &[(12, "fast-first", "m1"), (13, "full-first", "m2")]);
    let (exit, out) = run(&c, &Fake::default(), &["stats", "land", "sendmeter"]);
    assert_eq!(exit, 0, "{out}");
    assert!(out.contains("sendmeter fast-first: 1 landed"), "{out}");
    assert!(out.contains("sendmeter full-first: 1 landed"), "{out}");
    assert!(out.contains("integration red (landings paused): 0 min"), "{out}");
    let c = drover::Ctx {
        now: 100 + 2 * 86_400,
        ..c
    };
    let (_, out) = run(&c, &Fake::default(), &["stats", "land", "sendmeter"]);
    assert!(out.contains("1 landed · 0.5/day"), "{out}");
}

#[test]
fn a_revert_that_does_not_apply_is_logged_and_alerted_once() {
    let (d, c) = setup("land-revert-failed", true, HEAD);
    landed(&c, &[(12, "fast-first", "m1")]);
    let r = integration("m1", &[("m1", red())]).reply(&format!("git -C {DIR}"), "");
    let out = watch(&c, &r);
    assert!(out.contains("revert of m1 failed"), "{out}");
    assert!(!r.called("gh pr create"));
    let e = event(&c, "revert-failed").expect("revert-failed");
    assert_eq!(e.str("culprit"), Some("m1"));
    let alerts = std::fs::read_to_string(d.join("alerts.txt")).unwrap();
    assert!(alerts.contains("revert-failed"), "{alerts}");
    watch(&c, &r);
    assert_eq!(kinds(&c).iter().filter(|k| *k == "revert-failed").count(), 1);
}

#[test]
fn a_cancelled_intermediate_run_is_no_verdict_until_its_own_run_repeats_it() {
    let (_d, c) = setup("land-bisect-cancelled", true, HEAD);
    landed(
        &c,
        &[
            (12, "fast-first", "m1"),
            (13, "fast-first", "m2"),
            (14, "fast-first", "m3"),
        ],
    );
    let mut old = check("e2e", "COMPLETED", "CANCELLED");
    old["startedAt"] = json!("1970-01-01T00:00:10Z");
    let cancelled = json!([check("unit", "COMPLETED", "SUCCESS"), old.clone()]);
    let r = integration("m3", &[("m1", green()), ("m2", cancelled), ("m3", red())])
        .reply("gh run view 99", "{\"workflowName\": \"CI\"}")
        .reply("gh workflow run", "")
        .reply("git", "");
    watch(&c, &r);
    assert!(r.called(&format!("gh workflow run CI -R {REPO} --ref drover-bisect-m2")));
    assert!(!r.called("gh pr create"));
    let r = integration("m3", &[("m1", green()), ("m2", json!([old])), ("m3", red())]).reply("git", "");
    watch(&c, &r);
    assert!(!r.called("gh workflow run") && !r.called("gh pr create"));
    let r = integration("m3", &[("m1", green()), ("m2", red()), ("m3", red())])
        .reply(
            &format!("gh pr create -R {REPO}"),
            "https://github.com/jirathip-dev/sendmeter/pull/80\n",
        )
        .reply("git", "");
    watch(&c, &r);
    let o = event(&c, "revert-opened").expect("revert");
    assert_eq!((o.issue, o.str("culprit")), (Some(13), Some("m2")));
}

#[test]
fn a_bisect_run_that_does_not_start_is_alerted_once_and_retried() {
    let (d, c) = setup("land-bisect-fail", true, HEAD);
    landed(&c, &[(12, "fast-first", "m1"), (13, "fast-first", "m2")]);
    let fake = || {
        integration("m2", &[("m1", json!([])), ("m2", red())])
            .reply("gh run view 99", "{\"workflowName\": \"CI\"}")
            .reply("git", "")
    };
    watch(&c, &fake());
    let e = event(&c, "revert-failed").expect("revert-failed");
    assert_eq!((e.issue, e.str("sha")), (Some(12), Some("m1")));
    assert!(event(&c, "land-bisect").is_none());
    let alerts = std::fs::read_to_string(d.join("alerts.txt")).unwrap();
    assert!(alerts.contains("revert-failed"), "{alerts}");
    let r = fake();
    watch(&c, &r);
    assert!(r.called(&format!("gh workflow run CI -R {REPO} --ref drover-bisect-m1")));
    assert_eq!(kinds(&c).iter().filter(|k| *k == "revert-failed").count(), 1);
}

#[test]
fn a_revert_whose_fast_checks_go_red_is_alerted_once() {
    let (d, c) = setup("land-revert-red", true, HEAD);
    landed(&c, &[(12, "fast-first", "m1")]);
    let o = json!({"culprit": "m1", "pr": "77", "red_sha": "m1", "red_job": "e2e", "run": "99"});
    append(&c.state, &Event::new(200, "sendmeter", Some(12), "revert-opened", o)).unwrap();
    let rv = json!({"state": "OPEN", "headRefOid": "r3v",
        "statusCheckRollup": [check("unit", "COMPLETED", "FAILURE")]});
    let fake = || integration("m1", &[("m1", red())]).reply("gh pr view 77", &rv.to_string());
    let out = watch(&c, &fake());
    assert!(out.contains("revert PR #77: fast checks red"), "{out}");
    assert_eq!(event(&c, "revert-failed").unwrap().str("pr"), Some("77"));
    let alerts = std::fs::read_to_string(d.join("alerts.txt")).unwrap();
    assert!(alerts.contains("revert-failed"), "{alerts}");
    let r = fake();
    watch(&c, &r);
    assert!(!r.called("gh pr merge"));
    assert_eq!(kinds(&c).iter().filter(|k| *k == "revert-failed").count(), 1);
}

#[test]
fn the_only_fast_first_landing_is_the_culprit_beside_full_first_ones() {
    let (_d, c) = setup("land-one-fast", true, HEAD);
    landed(&c, &[(12, "full-first", "m1"), (13, "fast-first", "m2")]);
    let r = integration("m2", &[("m2", red())])
        .reply(
            &format!("gh pr create -R {REPO}"),
            "https://github.com/jirathip-dev/sendmeter/pull/81\n",
        )
        .reply("git", "");
    watch(&c, &r);
    assert!(!r.called("gh workflow run") && !r.called(&format!("gh api repos/{REPO}/commits/m1")));
    let o = event(&c, "revert-opened").expect("revert");
    assert_eq!((o.issue, o.str("culprit")), (Some(13), Some("m2")));
}

#[test]
fn doctor_warns_on_an_unknown_mode() {
    let (d, _) = setup("land-doctor-mode", true, HEAD);
    let text = std::fs::read_to_string(d.join("config/fleet.toml")).unwrap();
    let c = ctx(&d, &text.replace("mode = \"fast-first\"", "mode = \"fast_first\""));
    let (_, out) = run(&c, &Fake::default(), &["doctor"]);
    assert!(out.contains("! land sendmeter — mode `fast_first` is unknown"), "{out}");
    let (_, c) = setup("land-doctor-mode-ok", true, HEAD);
    assert!(!run(&c, &Fake::default(), &["doctor"]).1.contains("is unknown"));
}
