use super::*;

#[test]
fn red_ci_after_pass_is_a_fail_with_the_log_tail() {
    let off = CONFIG.replace(
        "integration = \"staging\"",
        "integration = \"staging\"\nci_flaky_retries = 0",
    );
    let c = ctx(&tmpdir("drive-red"), &off);
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
    assert_eq!(steps, ["ci-red", "fix"], "{out}");
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
    assert!(!r.called("gh run rerun"), "ci_flaky_retries = 0 never re-runs");
    assert!(all.iter().all(|e| e.kind != "ci-rerun"));
}

fn red(head: &str, completed: &str) -> String {
    let check = json!({"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "FAILURE",
        "completedAt": completed, "detailsUrl": "https://github.com/jirathip-dev/sendmeter/actions/runs/777/job/1"});
    json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": head, "statusCheckRollup": [check]})
        .to_string()
}

fn flaky(name: &str) -> (drover::Ctx, String) {
    let c = setup(name);
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
        .reply("gh pr view issue-12", &red(&head, "1970-01-12T13:40:00Z"))
        .reply("gh run rerun", "");
    let (out, steps) = drive(&c, &r);
    assert!(steps.is_empty(), "{out}");
    assert_eq!(
        r.count("gh run rerun 777 --failed -R jirathip-dev/sendmeter"),
        1,
        "{out}"
    );
    assert!(out.contains("ci-rerun: ci failed at abc0000"), "{out}");
    let all = read(&c.state).unwrap();
    let reruns: Vec<&Event> = all.iter().filter(|e| e.kind == "ci-rerun").collect();
    assert_eq!((reruns.len(), reruns[0].str("why")), (1, Some("failed")));
    assert_eq!(state(&c), "ready");
    let (out, steps) = drive(&at(&c, 120), &r);
    assert!(
        steps.is_empty(),
        "the red from before the re-run is not its result: {out}"
    );
    assert_eq!(r.count("gh run rerun"), 1);
    (c, head)
}

#[test]
fn a_failed_check_is_rerun_once_and_merges_when_the_rerun_is_green() {
    let (c, head) = flaky("drive-flaky-green");
    let r = fake(NONE)
        .reply("gh pr view issue-12", &pr(&head, "green"))
        .reply("gh pr merge 100", "")
        .reply("gh pr view 100", r#"{"mergeCommit":{"oid":"m3rg3"}}"#)
        .reply("gh issue", "");
    let (out, steps) = drive(&at(&c, 400), &r);
    assert_eq!(steps, ["merge"], "{out}");
    assert!(!r.called("gh run rerun"));
    assert_eq!(state(&c), "merged");
    let all = read(&c.state).unwrap();
    let f = all.iter().find(|e| e.kind == "ci-flaky?").expect("ci-flaky?");
    assert_eq!((f.str("sha"), f.val("checks")), (Some(head.as_str()), &json!(["ci"])));
}

#[test]
fn a_check_red_again_after_its_rerun_is_ci_red_naming_it_with_no_second_rerun() {
    let (c, head) = flaky("drive-flaky-red");
    let r = fake(NONE)
        .reply("gh pr view issue-12", &red(&head, "1970-01-12T13:50:00Z"))
        .reply("gh run rerun", "")
        .reply(
            "gh run view 777 -R jirathip-dev/sendmeter --log-failed",
            "error: 1 test failed\n",
        )
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply("git -C", "");
    let (out, steps) = drive(&at(&c, 400), &r);
    assert_eq!(steps.first().map(String::as_str), Some("ci-red"), "{out}");
    assert!(!r.called("gh run rerun"), "one re-run a head");
    let all = read(&c.state).unwrap();
    let v = all.iter().rev().find(|e| e.kind == "verdict").unwrap();
    assert_eq!(v.str("verdict"), Some("FAIL"));
    let body = v.str("body").unwrap();
    assert!(body.contains("failed again after its re-run: `ci`"), "{body}");
    assert!(all.iter().all(|e| e.kind != "ci-flaky?"));
}
