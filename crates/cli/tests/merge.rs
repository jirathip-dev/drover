//! AC3: every `drover merge` refusal. Each test also asserts `gh pr merge` was
//! never called, so an always-merge gate turns every one of them RED.
mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::json;

const HEAD: &str = "1111111111111111111111111111111111111111";
const OLD: &str = "2222222222222222222222222222222222222222";

fn setup(name: &str, config: &str, verdicts: &[(&str, &str)]) -> (std::path::PathBuf, drover::Ctx) {
    let d = tmpdir(name);
    let c = ctx(&d, config);
    let spawn = json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"});
    append(&c.state, &Event::new(10, "sendmeter", Some(12), "spawn", spawn)).unwrap();
    for (i, (v, sha)) in verdicts.iter().enumerate() {
        let e = Event::new(
            20 + i as u64,
            "sendmeter",
            Some(12),
            "verdict",
            json!({"verdict": v, "sha": sha}),
        );
        append(&c.state, &e).unwrap();
    }
    (d, c)
}

fn green() -> serde_json::Value {
    json!([{"__typename": "CheckRun", "name": "build", "status": "COMPLETED", "conclusion": "SUCCESS"},
           {"__typename": "StatusContext", "context": "ci/legacy", "state": "SUCCESS"}])
}

fn pr(base: &str, checks: serde_json::Value) -> Fake {
    let pr =
        json!({"number": 34, "state": "OPEN", "baseRefName": base, "headRefOid": HEAD, "statusCheckRollup": checks});
    Fake::default()
        .reply("gh pr view issue-12", &pr.to_string())
        .reply("gh pr merge 34", "")
        .reply("gh pr view 34", &json!({"mergeCommit": {"oid": "m3rg3"}}).to_string())
        .reply("gh issue", "")
}

fn assert_refused(c: &drover::Ctx, r: &Fake, code: &str) {
    let (exit, out) = run(c, r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 3, "{out}");
    assert!(out.contains(&format!("REFUSED {code}")), "{out}");
    assert!(!r.called("gh pr merge"), "must not merge: {:?}", r.calls.borrow());
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
    assert!(st.state == "blocked" || st.state == "needs-human", "{}", st.state);
    assert!(read(&c.state).unwrap().iter().all(|e| e.kind != "merged"));
}

#[test]
fn refuses_without_verdict() {
    let (_d, c) = setup("no-verdict", CONFIG, &[]);
    assert_refused(&c, &pr("staging", green()), "NO_VERDICT");
}

#[test]
fn refuses_verdict_for_older_sha() {
    let (_d, c) = setup("old-sha", CONFIG, &[("PASS", OLD)]);
    assert_refused(&c, &pr("staging", green()), "STALE_VERDICT");
}

#[test]
fn refuses_fail_verdict() {
    let (_d, c) = setup("fail", CONFIG, &[("PASS", OLD), ("FAIL", HEAD)]);
    assert_refused(&c, &pr("staging", green()), "VERDICT_FAIL");
}

#[test]
fn refuses_when_ci_not_green() {
    let red = json!([{"__typename": "CheckRun", "name": "test", "status": "COMPLETED", "conclusion": "FAILURE"}]);
    let pending = json!([{"__typename": "CheckRun", "name": "test", "status": "IN_PROGRESS", "conclusion": ""}]);
    // A fresh log each time: the PR read is cached for 60 s per issue.
    let (_d, c) = setup("ci-red", CONFIG, &[("PASS", HEAD)]);
    assert_refused(&c, &pr("staging", red), "CI_NOT_GREEN");
    // #148: pending or not yet reported is refused but stays ready, waiting for CI.
    for checks in [pending, json!([])] {
        let (_d, c) = setup("ci-pending", CONFIG, &[("PASS", HEAD)]);
        let r = pr("staging", checks);
        let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
        assert!(exit == 3 && out.contains("REFUSED CI_NOT_GREEN"), "{out}");
        assert!(!r.called("gh pr merge"));
        assert_eq!(
            fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)].state,
            "ready"
        );
    }
}

#[test]
fn required_checks_gate_only_the_latest_named_run() {
    let cfg = CONFIG.replace(
        "integration = \"staging\"   # never main",
        "integration = \"staging\"\nrequired_checks = [\"quality\"]",
    );
    let run = |n: &str, c: &str, t: &str| json!({"__typename": "CheckRun", "name": n, "status": "COMPLETED", "conclusion": c, "startedAt": t});
    let red = json!([
        run("quality", "SUCCESS", "T1"),
        run("quality", "FAILURE", "T2"),
        run("quality-narrowed", "SUCCESS", "T1")
    ]);
    let (_d, c) = setup("req-red", &cfg, &[("PASS", HEAD)]);
    let (exit, out) = run_cli(&c, &pr("staging", red));
    assert_eq!(exit, 3, "{out}");
    assert!(
        out.contains("REFUSED CI_NOT_GREEN: required check `quality` is failed"),
        "{out}"
    );
    let green = json!([
        run("quality", "CANCELLED", "T1"),
        run("quality", "SUCCESS", "T2"),
        run("quality-narrowed", "FAILURE", "T2")
    ]);
    let (_d, c) = setup("req-green", &cfg, &[("PASS", HEAD)]);
    let (exit, out) = run_cli(&c, &pr("staging", green));
    assert_eq!(exit, 0, "{out}");
}

fn run_cli(c: &drover::Ctx, r: &Fake) -> (i32, String) {
    run(c, r, &["merge", "sendmeter", "12"])
}

#[test]
fn refuses_when_round_limit_exceeded() {
    let (a, b) = (
        "3333333333333333333333333333333333333333",
        "4444444444444444444444444444444444444444",
    );
    let v = [("FAIL", OLD), ("FAIL", a), ("FAIL", b), ("PASS", HEAD)];
    let (_d, c) = setup("rounds", CONFIG, &v);
    assert_refused(&c, &pr("staging", green()), "ROUND_LIMIT");
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
    assert_eq!(st.state, "needs-human");
}

/// #123: three review rounds plus a PASSed `--update` round: merge allowed, rounds stay 3.
#[test]
fn update_round_does_not_count_toward_review_rounds() {
    let (a, b) = (
        "3333333333333333333333333333333333333333",
        "4444444444444444444444444444444444444444",
    );
    let (_d, c) = setup("update-round", CONFIG, &[("FAIL", OLD), ("FAIL", a), ("PASS", b)]);
    let up = json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12", "update": true});
    append(&c.state, &Event::new(30, "sendmeter", Some(12), "spawn", up)).unwrap();
    let rev = json!({"role": "rev", "agent": "sendmeter-rev-12"});
    append(&c.state, &Event::new(31, "sendmeter", Some(12), "spawn", rev)).unwrap();
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
    assert_eq!(st.state, "review(3)+u1");
    let pass = json!({"verdict": "PASS", "sha": HEAD});
    append(&c.state, &Event::new(32, "sendmeter", Some(12), "verdict", pass)).unwrap();
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
    assert_eq!((st.state.as_str(), st.rounds, st.updates), ("ready", 3, 1));
    let r = pr("staging", green());
    let (exit, out) = run_cli(&c, &r);
    assert_eq!(exit, 0, "{out}");
    assert!(r.called("gh pr merge 34"), "{out}");
}

/// #123: an update round whose merge-only check FAILs is a normal review round.
#[test]
fn failed_update_round_counts_as_a_review_round() {
    let (_d, c) = setup("update-fail", CONFIG, &[("PASS", OLD)]);
    let up = json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12", "update": true});
    append(&c.state, &Event::new(30, "sendmeter", Some(12), "spawn", up)).unwrap();
    let fail = json!({"verdict": "FAIL", "sha": HEAD});
    append(&c.state, &Event::new(31, "sendmeter", Some(12), "verdict", fail)).unwrap();
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
    assert_eq!((st.state.as_str(), st.rounds, st.updates), ("fix(2)", 2, 0));
}

#[test]
fn reverdict_on_same_sha_is_one_round() {
    let v = [
        ("FAIL", OLD),
        ("FAIL", OLD),
        ("FAIL", OLD),
        ("PASS", HEAD),
        ("PASS", HEAD),
    ];
    let (_d, c) = setup("same-sha", CONFIG, &v);
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
    assert_eq!((st.state.as_str(), st.rounds), ("ready", 2));
    let (exit, out) = run(&c, &pr("staging", green()), &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
}

#[test]
fn refuses_main_as_integration_target() {
    let cfg = CONFIG.replace("integration = \"staging\"", "integration = \"main\"");
    let (_d, c) = setup("main-cfg", &cfg, &[("PASS", HEAD)]);
    assert_refused(&c, &pr("main", green()), "MAIN_TARGET");
}

#[test]
fn refuses_pr_based_on_main() {
    let (_d, c) = setup("main-base", CONFIG, &[("PASS", HEAD)]);
    assert_refused(&c, &pr("main", green()), "MAIN_TARGET");
}

#[test]
fn refuses_without_open_pr() {
    let (_d, c) = setup("no-pr", CONFIG, &[("PASS", HEAD)]);
    assert_refused(&c, &Fake::default(), "NO_PR");
}

#[test]
fn merges_with_exact_head_pass_and_green_ci() {
    let (_d, c) = setup("happy", CONFIG, &[("FAIL", OLD), ("PASS", HEAD)]);
    let r = pr("staging", green());
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    assert!(r.called(&format!(
        "gh pr merge 34 -R jirathip-dev/sendmeter --merge --match-head-commit {HEAD}"
    )));
    assert!(r.called("gh issue comment 12 -R jirathip-dev/sendmeter --body Merged via `drover merge`"));
    assert!(
        out.contains("m3rg3") && out.contains(HEAD) && out.contains("Merged"),
        "{out}"
    );
    let body = r
        .calls
        .borrow()
        .iter()
        .find(|c| c.starts_with("gh issue comment"))
        .unwrap()
        .clone();
    assert!(body.lines().count() <= 5, "closing comment is ≤ 5 lines: {body}");
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
    assert_eq!(st.state, "merged");
}

/// #677: the closing comment carries the latest why note's Owner line.
#[test]
fn the_closing_comment_carries_the_why_owner_line() {
    let (_d, c) = setup("why-owner", CONFIG, &[("PASS", HEAD)]);
    for (ts, owner) in [(30, "first round"), (31, "retries no longer double the meter")] {
        let d = json!({"sha": HEAD, "round": 1, "owner": owner});
        append(&c.state, &Event::new(ts, "sendmeter", Some(12), "why", d)).unwrap();
    }
    let r = pr("staging", green());
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    let calls = r.calls.borrow();
    let body = calls.iter().find(|c| c.starts_with("gh issue comment")).unwrap();
    assert!(body.ends_with("\nWhy: retries no longer double the meter"), "{body}");
    assert!(body.lines().count() <= 5, "{body}");
}

#[test]
fn merge_twice_says_already_merged_and_logs_nothing() {
    let (_d, c) = setup("twice", CONFIG, &[("PASS", HEAD)]);
    let (exit, out) = run(&c, &pr("staging", green()), &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    let before = read(&c.state).unwrap().len();
    // Second caller: the PR is closed now, so a gate run would refuse NO_PR.
    let r = Fake::default();
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    assert!(out.contains("already merged → m3rg3 (PR #34)"), "{out}");
    assert_eq!(read(&c.state).unwrap().len(), before, "no new event");
    assert!(r.calls.borrow().is_empty(), "no gh calls: {:?}", r.calls.borrow());
}

// --- drover verdict ---

#[test]
fn verdict_pins_exact_branch_head() {
    let (d, c) = setup("verdict", CONFIG, &[]);
    let f = d.join("verdict.md");
    std::fs::write(&f, "PASS\nlooks right\n").unwrap();
    let r = Fake::default().reply(
        "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
        &format!("{HEAD}\n"),
    );
    let fp = f.to_str().unwrap();
    let (exit, out) = run(
        &c,
        &r,
        &["verdict", "sendmeter", "12", "PASS", "--sha", OLD, "--file", fp],
    );
    assert_eq!(exit, 3, "{out}");
    assert!(out.contains("REFUSED STALE_SHA"), "{out}");
    let (exit, out) = run(
        &c,
        &r,
        &["verdict", "sendmeter", "12", "PASS", "--sha", "abc", "--file", fp],
    );
    assert_eq!(exit, 3);
    assert!(out.contains("REFUSED BAD_SHA"), "{out}");
    assert!(read(&c.state).unwrap().iter().all(|e| e.kind != "verdict"));
    let (exit, out) = run(
        &c,
        &r,
        &["verdict", "sendmeter", "12", "PASS", "--sha", HEAD, "--file", fp],
    );
    assert_eq!(exit, 0, "{out}");
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
    assert_eq!((st.state.as_str(), st.rounds), ("ready", 1));
    assert_eq!(
        st.verdicts,
        vec![(HEAD.to_string(), "PASS".to_string(), "rev".to_string())]
    );
}

// --- #6: the evidence comment must land ---

fn merge_fake() -> Fake {
    let pr = json!({"number": 34, "state": "OPEN", "baseRefName": "staging", "headRefOid": HEAD, "statusCheckRollup": green()});
    Fake::default()
        .reply("gh pr view issue-12", &pr.to_string())
        .reply("gh pr merge 34", "")
        .reply("gh pr view 34", &json!({"mergeCommit": {"oid": "m3rg3"}}).to_string())
        .reply("gh issue close", "")
}

#[test]
fn evidence_comment_failing_twice_leaves_issue_open_and_exits_nonzero() {
    let d = tmpdir("evidence-sink");
    let sink = d.join("sink.txt");
    let cfg = format!(
        "{CONFIG}\n[[notify]]\ncmd = \"cat > '{}'\"\non = [\"evidence-missing\"]\n",
        sink.display()
    );
    let (_d, c) = setup("evidence-missing", &cfg, &[("PASS", HEAD)]);
    let r = merge_fake(); // `gh issue comment` is unscripted → fails
    let (exit, _) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_ne!(exit, 0);
    assert!(r.called("gh pr merge 34"), "the merge itself happened");
    assert_eq!(r.count("gh issue comment 12"), 2, "retried once");
    assert!(!r.called("gh issue close"), "issue must stay open");
    let ev = read(&c.state).unwrap();
    assert!(ev.iter().any(|e| e.kind == "merged"));
    let miss = ev
        .iter()
        .find(|e| e.kind == "evidence-missing")
        .expect("evidence-missing logged");
    assert!(miss.str("reason").unwrap().contains("m3rg3"), "{:?}", miss.data);
    let line = std::fs::read_to_string(&sink).expect("sent to notifiers");
    assert!(
        line.starts_with("sendmeter #12 evidence-missing: PR #34 merged @ m3rg3"),
        "{line}"
    );
}

#[test]
fn evidence_comment_retry_that_lands_closes_issue() {
    let (_d, c) = setup("evidence-retry", CONFIG, &[("PASS", HEAD)]);
    let r = merge_fake()
        .once("gh issue comment", 1, "")
        .reply("gh issue comment", "");
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    assert_eq!(r.count("gh issue comment 12"), 2);
    assert!(r.called("gh issue close 12"));
    assert!(read(&c.state).unwrap().iter().all(|e| e.kind != "evidence-missing"));
}

// --- #19: lane teardown + dogfood reinstall line ---

#[test]
fn merge_tears_down_clean_lanes_and_keeps_unpushed_ones() {
    let (_d, c) = setup("teardown", CONFIG, &[("PASS", HEAD)]);
    let wt = |n: &str| {
        let p = c.worktrees.join("sendmeter").join(n);
        std::fs::create_dir_all(&p).unwrap();
        p.to_string_lossy().into_owned()
    };
    let (imp, rev) = (wt("issue-12"), wt("rev-12-r1"));
    for (ts, role, w, ws) in [(11, "impl", &imp, "wA"), (12, "rev", &rev, "wB")] {
        let d = json!({"role": role, "agent": format!("{role}-12"), "worktree": w, "pane": format!("{ws}:p1"), "workspace": ws});
        append(&c.state, &Event::new(ts, "sendmeter", Some(12), "spawn", d)).unwrap();
    }
    // After the clean impl pane is closed only the rev agent (unpushed work) is left.
    let r = pr("staging", green())
        .reply("herdr agent list", &agents(&[("rev-12", "idle", &rev, "wB:p1")]))
        .reply(&format!("git -C {rev} rev-list"), "1\n")
        .reply(&format!("git -C {imp} rev-list"), "0\n")
        .reply("git -C", "")
        .reply("herdr", "{}");
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    assert!(r.called("herdr pane close wA:p1"), "{:?}", r.calls.borrow());
    assert!(r.called("herdr worktree remove --workspace wA"));
    assert!(out.contains(&format!("removed {imp}")), "{out}");
    assert!(
        !r.called("herdr pane close wB:p1"),
        "a lane with unpushed commits keeps its pane"
    );
    assert!(!r.called("herdr worktree remove --workspace wB"));
    assert!(
        out.contains(&format!("keep pane wB:p1 ({rev}): 1 unpushed commit(s)")),
        "{out}"
    );
    assert!(
        !out.contains("cargo install"),
        "only the dogfood fleet prints the reinstall line"
    );
}

#[test]
fn merge_into_fleet_repo_prints_reinstall_command_last() {
    let cfg = CONFIG.replace("jirathip-dev/sendmeter", "jirathip-dev/herdsman");
    let (_d, c) = setup("dogfood", &cfg, &[("PASS", HEAD)]);
    let r = merge_fake().reply("gh issue comment", "");
    let r = Fake {
        replies: r
            .replies
            .into_iter()
            .map(|(p, o)| (p.replace("sendmeter", "drover"), o))
            .collect(),
        ..Fake::default()
    };
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    assert_eq!(
        out.lines().last(),
        Some("cargo build --release --locked -p drover --manifest-path /tmp/does-not-matter/sendmeter/Cargo.toml && sh /tmp/does-not-matter/sendmeter/scripts/install-gate.sh /tmp/does-not-matter/sendmeter/target/release/drover /opt/drover/bin/drover && ln -sf drover /opt/drover/bin/herdsman"),
        "{out}"
    );
    assert!(!r.called("cargo"), "printed, never run");
}

// --- #130: `--owner-override` and `round-extended` ---

/// Four rounds, the last a PASS at the head.
const OVER: [(&str, &str); 4] = [
    ("FAIL", OLD),
    ("FAIL", "3333333333333333333333333333333333333333"),
    ("FAIL", "4444444444444444444444444444444444444444"),
    ("PASS", HEAD),
];

fn over(c: &drover::Ctx, r: &Fake) -> (i32, String) {
    run(c, r, &["merge", "sendmeter", "12", "--owner-override"])
}

#[test]
fn owner_override_merges_past_round_limit_and_says_so() {
    let (_d, c) = setup("override", CONFIG, &OVER);
    let r = pr("staging", green());
    let (exit, out) = over(&c, &r);
    assert_eq!(exit, 0, "{out}");
    assert!(r.called("gh pr merge 34"));
    let all = read(&c.state).unwrap();
    let m = all.iter().find(|e| e.kind == "merged").unwrap();
    assert_eq!((m.str("by"), m.str("override")), (Some("owner"), Some("ROUND_LIMIT")));
    assert!(out.contains("Owner override: ROUND_LIMIT"), "closing comment: {out}");
}

#[test]
fn owner_override_still_refuses_red_ci_and_a_stale_verdict() {
    let red = json!([{"__typename": "CheckRun", "name": "test", "status": "COMPLETED", "conclusion": "FAILURE"}]);
    let (_d, c) = setup("override-red", CONFIG, &OVER);
    let r = pr("staging", red);
    let (exit, out) = over(&c, &r);
    assert!(exit == 3 && out.contains("REFUSED CI_NOT_GREEN"), "{out}");
    assert!(!r.called("gh pr merge"));

    let stale = [
        OVER[0],
        OVER[1],
        OVER[2],
        ("PASS", "5555555555555555555555555555555555555555"),
    ];
    let (_d, c) = setup("override-stale", CONFIG, &stale);
    let r = pr("staging", green());
    let (exit, out) = over(&c, &r);
    assert!(exit == 3 && out.contains("REFUSED STALE_VERDICT"), "{out}");
    assert!(!r.called("gh pr merge"));
}

/// #309: `review_rounds` 9, `rounds` reviewed shas (the last a PASS at the head), the owner's extend after `at`.
fn extended(name: &str, rounds: usize, at: usize) -> (std::path::PathBuf, drover::Ctx) {
    let (d, c) = setup(name, &CONFIG.replace("review_rounds = 3", "review_rounds = 9"), &[]);
    for i in 1..=rounds {
        let (v, sha) = if i == rounds {
            ("PASS", HEAD.to_string())
        } else {
            ("FAIL", format!("{i:040}"))
        };
        append(
            &c.state,
            &Event::new(20, "sendmeter", Some(12), "verdict", json!({"verdict": v, "sha": sha})),
        )
        .unwrap();
        if i == at {
            append(
                &c.state,
                &Event::new(30, "sendmeter", Some(12), "round-extended", json!({"by": "owner"})),
            )
            .unwrap();
        }
    }
    (d, c)
}

/// `[one more round]` logs `round-extended`: round 10 of 9+1 merges without override, round 11 is ROUND_LIMIT again.
#[test]
fn round_extended_allows_exactly_one_more_round() {
    let (_d, c) = extended("extend-10", 10, 9);
    let s = st(&c);
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let (limit, of) = drover::gate::limit(cfg.fleet("sendmeter").unwrap(), "rev", Some(&s));
    assert_eq!(
        format!("round {}{of}", s.stage_rounds["rev"]),
        "round 10 of 9+1 (owner extended)"
    );
    assert_eq!(limit, 10);
    let (exit, out) = run_cli(&c, &pr("staging", green()));
    assert_eq!(exit, 0, "round 10 of 9+1 merges: {out}");

    let (_d, c) = extended("extend-11", 11, 9);
    let (exit, out) = run_cli(&c, &pr("staging", green()));
    assert!(
        out.contains("review rounds ran out (11 of 9+1 (owner extended))"),
        "{out}"
    );
    assert_eq!(exit, 3, "{out}");
    assert_refused(&c, &pr("staging", green()), "ROUND_LIMIT");
}

/// #309 (synergy#2940): a round reviewed past the limit (a new head after a PASS) is needs-human; its extend grants the
/// next round, whose PASS merges.
#[test]
fn an_extend_after_an_overrun_round_grants_the_next_one() {
    let (_d, c) = extended("extend-overrun", 11, 10);
    let (exit, out) = run_cli(&c, &pr("staging", green()));
    assert_eq!(exit, 0, "round 11 of 9+2: {out}");
}

// --- #151: `drover extend` / `drover park` ---

/// Three FAILed rounds of 3, then `needs-human`.
fn needs_human(name: &str) -> (std::path::PathBuf, drover::Ctx) {
    let (d, c) = setup(name, CONFIG, &OVER[..3]);
    let e = Event::new(30, "sendmeter", Some(12), "needs-human", json!({"code": "ROUND_LIMIT"}));
    append(&c.state, &e).unwrap();
    (d, c)
}

fn st(c: &drover::Ctx) -> drover::events::IssueState {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)].clone()
}

#[test]
fn extend_no_spawn_records_one_round_and_the_next_pass_merges_without_override() {
    let (_d, c) = needs_human("cli-extend");
    let r = pr("staging", green());
    let (exit, out) = run(&c, &r, &["extend", "sendmeter", "12", "--no-spawn", "--by", "owner"]);
    assert_eq!(exit, 0, "{out}");
    assert_eq!((st(&c).extra, st(&c).state.as_str()), (1, "fix(3)"));
    let all = read(&c.state).unwrap();
    assert_eq!(
        all.iter().filter(|e| e.kind == "spawn").count(),
        1,
        "--no-spawn spawns nothing"
    );
    assert!(r.calls.borrow().is_empty(), "no herdr/gh call: {:?}", r.calls.borrow());
    let ext = all.iter().rfind(|e| e.kind == "round-extended").unwrap();
    assert_eq!(ext.str("by"), Some("owner"));

    for args in [
        &["extend", "sendmeter", "12", "--no-spawn"][..],
        &["extend", "sendmeter", "12"],
        &["park", "sendmeter", "12"],
    ] {
        let (exit, out) = run(&c, &r, args);
        assert!(exit == 3 && out.contains("REFUSED NOT_NEEDS_HUMAN"), "{args:?}: {out}");
    }
    assert_eq!(st(&c).extra, 1, "a repeat extends nothing");
    assert!(r.calls.borrow().is_empty(), "a refused extend spawns nothing");

    let e = Event::new(
        40,
        "sendmeter",
        Some(12),
        "verdict",
        json!({"verdict": "PASS", "sha": HEAD}),
    );
    append(&c.state, &e).unwrap();
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "round 4 of 3+1 merges with no override: {out}");
    assert!(r.called("gh pr merge 34"));
    assert!(!out.contains("Owner override"), "{out}");
}

#[test]
fn extend_and_park_refuse_outside_needs_human_and_park_clears_it() {
    let (_d, c) = setup("cli-not-nh", CONFIG, &OVER[..1]);
    let r = Fake::default();
    for args in [&["extend", "sendmeter", "12"][..], &["park", "sendmeter", "12"]] {
        let (exit, out) = run(&c, &r, args);
        assert!(
            exit == 3 && out.contains("REFUSED NOT_NEEDS_HUMAN: #12 is fix(1)"),
            "{out}"
        );
    }
    assert!(read(&c.state)
        .unwrap()
        .iter()
        .all(|e| e.kind != "round-extended" && e.kind != "parked"));

    let (_d, c) = needs_human("cli-park");
    let (exit, out) = run(&c, &r, &["park", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    assert_eq!(st(&c).state, "parked");
    let (exit, out) = run(&c, &r, &["park", "sendmeter", "12"]);
    assert!(exit == 3 && out.contains("is parked, not needs-human"), "{out}");
}

/// #712: `unpark` puts a parked issue back in the round it had before its needs-human, logged `by`, and an issue
/// out of the queue goes last; a second unpark refuses.
#[test]
fn unpark_restores_the_round_and_requeues_it_last() {
    let (_d, c) = needs_human("cli-unpark");
    let r = Fake::default();
    assert_eq!(run(&c, &r, &["queue", "sendmeter", "add", "5", "7"]).0, 0);
    assert_eq!(run(&c, &r, &["park", "sendmeter", "12"]).0, 0);
    let (exit, out) = run(&c, &r, &["unpark", "sendmeter", "12", "--by", "alice"]);
    assert_eq!((exit, out.as_str()), (0, "#12: unparked, back to fix(3)\n"));
    assert_eq!(st(&c).state, "fix(3)");
    let all = read(&c.state).unwrap();
    let un = all.iter().rfind(|e| e.kind == "unparked").unwrap();
    assert_eq!((un.str("by"), un.str("state")), (Some("alice"), Some("fix(3)")));
    let q: Vec<u64> = drover::queue::replay(&all, "sendmeter")
        .iter()
        .map(|i| i.issue)
        .collect();
    assert_eq!(q, [5, 7, 12]);
    let (exit, out) = run(&c, &r, &["unpark", "sendmeter", "12"]);
    assert!(
        exit == 3 && out.contains("REFUSED NOT_PARKED: #12 is fix(3), not parked"),
        "{out}"
    );
}

/// #198: the merge commit is `<title> (#PR)`, the PR body's `## For humans` lines, then the facts line.
#[test]
fn merge_message_copies_the_human_summary_and_the_facts_line() {
    let (_d, c) = setup("msg", CONFIG, &[("FAIL", OLD), ("PASS", HEAD)]);
    let view = json!({"number": 34, "title": "#12: Board shows the host", "files": [
        {"path": "src/a.rs", "additions": 20, "deletions": 5},
        {"path": "src/b.rs", "additions": 1, "deletions": 4},
        {"path": "tests/x.rs", "additions": 90, "deletions": 0}],
        "body": "Closes #12\n\n## For humans\n\nThe board's Host tab now shows memory.\nNothing else changes.\n\n## Evidence\nx"});
    let r = pr("staging", green()).once(
        "gh pr view 34 -R jirathip-dev/sendmeter --json number",
        0,
        &view.to_string(),
    );
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    let want = format!(
        "gh pr merge 34 -R jirathip-dev/sendmeter --merge --match-head-commit {HEAD} --subject Board shows the host (#34) --body The board's Host tab now shows memory.\nNothing else changes.\n\nCloses #12 · 2 review rounds · src +12 · PASS 1111111"
    );
    assert!(r.calls.borrow().contains(&want), "{:#?}", r.calls.borrow());
    assert!(read(&c.state).unwrap().iter().all(|e| e.kind != "no-human-summary"));
}

/// #198: no `## For humans` section merges anyway with the facts line only, logged `no-human-summary`.
#[test]
fn merge_without_a_human_summary_gives_the_facts_line_only() {
    let (_d, c) = setup("msg-none", CONFIG, &[("PASS", HEAD)]);
    let view = json!({"number": 34, "title": "[#12] Trim the cap", "body": "Closes #12",
        "files": [{"path": "src/a.rs", "additions": 1, "deletions": 9}]});
    let r = pr("staging", green()).once(
        "gh pr view 34 -R jirathip-dev/sendmeter --json number",
        0,
        &view.to_string(),
    );
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    let want = format!(
        "gh pr merge 34 -R jirathip-dev/sendmeter --merge --match-head-commit {HEAD} --subject Trim the cap (#34) --body Closes #12 · 1 review round · src -8 · PASS 1111111"
    );
    assert!(r.calls.borrow().contains(&want), "{:#?}", r.calls.borrow());
    assert!(read(&c.state).unwrap().iter().any(|e| e.kind == "no-human-summary"));
}

/// #272: a fleet whose local checkout has `ci/cap.toml` (or not); returns (dir, ctx, local, scratch worktree).
fn premerge_setup(name: &str, cap: bool, extra: &str) -> (std::path::PathBuf, drover::Ctx, String, String) {
    let d = tmpdir(name);
    let local = d.join("local");
    std::fs::create_dir_all(local.join("ci")).unwrap();
    if cap {
        std::fs::write(local.join("ci/cap.toml"), "[fleet.x]\nline_cap = 1\n").unwrap();
    }
    let local = local.to_string_lossy().to_string();
    let c = ctx(&d, &(CONFIG.replace("/tmp/does-not-matter/sendmeter", &local) + extra));
    let spawn = json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"});
    append(&c.state, &Event::new(10, "sendmeter", Some(12), "spawn", spawn)).unwrap();
    let v = json!({"verdict": "PASS", "sha": HEAD, "round": 1});
    append(&c.state, &Event::new(20, "sendmeter", Some(12), "verdict", v)).unwrap();
    let wt = d.join("state/premerge-sendmeter").to_string_lossy().to_string();
    (d, c, local, wt)
}

fn premerge_fake(local: &str) -> Fake {
    pr("staging", green())
        .reply(&format!("git -C {local} rev-parse origin/staging"), "base1")
        .reply("git -C ", "")
        .reply("gh api repos/jirathip-dev/sendmeter/branches/issue-12", HEAD)
}

#[test]
fn premerge_cap_failure_refuses_without_merging_and_goes_to_fix() {
    let (_d, c, local, wt) = premerge_setup("pre-cap", true, "");
    let r = premerge_fake(&local).once("sh -c", 1, "src lines (formatted): 14885 / cap 14700\n");
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 3, "{out}");
    let want = "REFUSED STAGING_WOULD_BREAK: /opt/drover/bin/drover cap-check --config ci/cap.toml: src lines (formatted): 14885 / cap 14700";
    assert!(out.contains(want), "{out}");
    assert!(!r.called("gh pr merge"), "{:#?}", r.calls.borrow());
    assert!(r.called(&format!("git -C {wt} merge --no-ff --no-edit {HEAD}")));
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
    assert_eq!((st.state.as_str(), st.rounds), ("fix(1)", 1), "not a new review round");
    let ev = read(&c.state).unwrap();
    let v = ev.iter().rfind(|e| e.kind == "verdict").unwrap();
    assert!(v.str("body").unwrap().contains("14885 / cap 14700"), "{v:?}");
}

#[test]
fn premerge_passing_merges_and_logs_its_time() {
    let (_d, c, local, _) = premerge_setup("pre-ok", true, "");
    let r = premerge_fake(&local).reply("sh -c", "src lines (formatted): 10 / cap 14700\n");
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    assert!(r.index("sh -c").unwrap() < r.index("gh pr merge").unwrap());
    let ev = read(&c.state).unwrap();
    let m = ev.iter().find(|e| e.kind == "merged").unwrap();
    assert!(m.data["premerge_secs"].is_u64(), "{:?}", m.data);
}

/// #510: a passing premerge runs each shadow lesson check on the merged tree and logs which would fail; active and
/// hand-written checks are not run there.
#[test]
fn premerge_logs_the_shadow_lesson_checks() {
    // No `ci/cap.toml` and no `premerge`: the shadow checks alone still run the premerge.
    let (_d, c, local, wt) = premerge_setup("pre-shadow", false, "");
    std::fs::create_dir_all(format!("{wt}/.drover")).unwrap();
    std::fs::create_dir_all(format!("{local}/.drover")).unwrap();
    let rules = "- don't unwrap (enforced by: no-unwrap `! grep -Hn unwrap src`, shadow since 1970-01-01)\n\
                 - don't sleep (enforced by: no-sleep `! grep -Hn sleep src`, shadow since 1970-01-01)\n\
                 - don't panic (enforced by: no-panic `! grep -Hn panic src`, active since 1970-01-01)\n\
                 - don't hand (enforced by: hand `false`)\n";
    std::fs::write(format!("{wt}/.drover/rules.md"), rules).unwrap();
    std::fs::write(format!("{local}/.drover/rules.md"), rules).unwrap();
    let r = premerge_fake(&local)
        .once(
            "sh -c cd \"$0\" && ! grep -Hn unwrap src",
            1,
            "src/a.rs:3: x.unwrap()\n",
        )
        .reply("sh -c", "ok\n");
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    let ev = read(&c.state).unwrap();
    let e = ev
        .iter()
        .find(|e| e.kind == "lesson-shadow")
        .expect("lesson-shadow logged");
    assert_eq!(e.issue, Some(12));
    assert_eq!(
        json!(e.data),
        json!({"ran": ["no-unwrap", "no-sleep"], "hits": ["no-unwrap"]})
    );
    for gone in ["grep -Hn panic", "&& false"] {
        assert!(!r.calls.borrow().iter().any(|l| l.contains(gone)), "{gone} ran");
    }
}

#[test]
fn premerge_rechecks_when_staging_moved() {
    let (_d, c, local, wt) = premerge_setup("pre-moved", true, "");
    let tip = format!("git -C {local} rev-parse origin/staging");
    let r = premerge_fake(&local)
        .once(&tip, 0, "base1")
        .once(&tip, 0, "base2")
        .once(&tip, 0, "base2")
        .once(&tip, 0, "base2")
        .reply("sh -c", "ok\n");
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    assert_eq!(r.count("sh -c"), 2, "{:#?}", r.calls.borrow());
    assert!(r.called(&format!("git -C {local} worktree add -f --detach {wt} base2")));
    assert!(r.index(&format!("git -C {local} worktree add -f --detach {wt} base2")) < r.index("gh pr merge"));
}

#[test]
fn premerge_default_needs_cap_toml() {
    let (_d, c, local, _) = premerge_setup("pre-none", false, "");
    let r = premerge_fake(&local);
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    assert!(!r.called("sh -c") && !r.called("git -C "), "{:#?}", r.calls.borrow());
    let ev = read(&c.state).unwrap();
    let m = ev.iter().find(|e| e.kind == "merged").unwrap();
    assert!(!m.data.contains_key("premerge_secs"));
}

/// #373: the issue's own example, as `[[fleet.<n>.premerge]]` tables.
const NAMED: &str = r#"
[[fleet.sendmeter.premerge]]
name  = "migration-order"
run   = "node scripts/check-migrations.mjs --base origin/staging --ledger"
when  = ["supabase/migrations/**"]
code  = "BACKDATED_MIGRATION"
fix   = "Rename the migration so its version is after {min}; output: {output}"
"#;

#[test]
fn named_premerge_skips_when_its_paths_are_unchanged() {
    let (_d, c, local, wt) = premerge_setup("pre-when-skip", true, NAMED);
    let diff = format!("git -C {wt} diff --name-only base1 HEAD");
    let r = premerge_fake(&local).once(&diff, 0, "src/a.rs\n");
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 0, "{out}");
    assert!(r.called(&diff));
    assert!(!r.called("sh -c"), "{:#?}", r.calls.borrow());
    assert!(r.called("gh pr merge"));
}

#[test]
fn named_premerge_failure_refuses_with_its_code_and_a_fix_round() {
    let (_d, c, local, wt) = premerge_setup("pre-when-fail", true, NAMED);
    let o = "migration 20260101_x.sql is behind the dev ledger\nmin_version=20261104000000\nSUPABASE_ACCESS_TOKEN=sbp9f8e7d6c5b4a\n";
    let r = premerge_fake(&local)
        .once(
            &format!("git -C {wt} diff --name-only base1 HEAD"),
            0,
            "supabase/migrations/20260101_x.sql\n",
        )
        .once("sh -c", 1, o);
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 3, "{out}");
    let want = "REFUSED BACKDATED_MIGRATION: migration 20260101_x.sql is behind the dev ledger";
    assert!(out.contains(want), "{out}");
    assert!(r.called("sh -c cd \"$0\" && node scripts/check-migrations.mjs --base origin/staging --ledger"));
    assert!(!r.called("gh pr merge"), "{:#?}", r.calls.borrow());
    let ev = read(&c.state).unwrap();
    let st = &fold(&ev)[&("sendmeter".to_string(), 12)];
    assert_eq!((st.state.as_str(), st.rounds), ("fix(1)", 1), "not a new review round");
    let p = ev.iter().find(|e| e.kind == "premerge-failed").unwrap();
    assert_eq!(
        (p.str("name"), p.str("code")),
        (Some("migration-order"), Some("BACKDATED_MIGRATION"))
    );
    let v = ev.iter().rfind(|e| e.kind == "verdict").unwrap();
    let body = v.str("body").unwrap();
    assert!(
        body.contains("Rename the migration so its version is after 20261104000000; output:"),
        "{body}"
    );
    assert!(body.contains("is behind the dev ledger"), "{body}");
    let log = drover::events::jsonl(&c.state).unwrap();
    assert!(!log.contains("sbp9f8e7d6c5b4a"), "secret in events: {log}");
    let (_, board) = run(&c, &Fake::default(), &["board", "sendmeter"]);
    assert!(board.contains("migration-order failed: rename needed"), "{board}");
}

#[test]
fn list_form_premerge_still_runs_its_checks() {
    let (_d, c, local, wt) = premerge_setup("pre-list", false, "");
    let cfg = std::fs::read_to_string(&c.config).unwrap();
    let cfg = cfg.replace(
        "integration = \"staging\"",
        "integration = \"staging\"\npremerge = [\"make lint\"]",
    );
    std::fs::write(&c.config, cfg).unwrap();
    let r = premerge_fake(&local).once("sh -c", 2, "lint: error: unused import\n");
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 3, "{out}");
    assert!(
        out.contains("REFUSED STAGING_WOULD_BREAK: make lint: lint: error: unused import"),
        "{out}"
    );
    assert!(r.called("sh -c cd \"$0\" && make lint"));
    assert!(!r.called(&format!("git -C {wt} diff")), "no `when`, no diff");
    assert!(!r.called("gh pr merge"));
    let p = read(&c.state)
        .unwrap()
        .into_iter()
        .find(|e| e.kind == "premerge-failed")
        .unwrap();
    assert_eq!(
        (p.str("name"), p.str("code")),
        (Some("make lint"), Some("STAGING_WOULD_BREAK"))
    );
}
