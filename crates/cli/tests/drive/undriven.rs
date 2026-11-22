use super::*;

fn failed_at(name: &str, extra: &str) -> (drover::Ctx, PathBuf) {
    let c = ctx(
        &tmpdir(name),
        &CONFIG.replace(
            "integration = \"staging\"",
            &format!("integration = \"staging\"\n{extra}"),
        ),
    );
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "FAIL", "sha": sha("abc"), "body": "FAIL\n1. blocking: src/x.rs:3"}),
    );
    (c, wt)
}

#[test]
fn a_fail_gets_its_fix_round_on_the_next_pass_while_the_in_flight_cap_holds_a_queued_start() {
    let (c, _) = failed_at("drive-fail-cap", "max_in_flight = 1");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "5"]).0, 0);
    let r = fake(NONE)
        .reply("gh pr view issue-12", &pr(&sha("abc"), "green"))
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert!(out.contains("sendmeter #5 start: REFUSED IN_FLIGHT_CAP"), "{out}");
    assert!(steps.contains(&"fix".to_string()), "{steps:?} {out}");
    assert_eq!(state(&c), "fix(1)");
    assert_eq!(started("sendmeter-impl-12"), ["sendmeter-impl-12"]);
}

#[test]
fn a_pass_at_a_red_ci_head_gets_its_fix_round_in_the_same_pass() {
    let c = setup("drive-red-fix");
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
            "error: 1 test failed\n",
        )
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["ci-red", "fix"], "{out}");
    assert_eq!(state(&c), "fix(1)");
    assert_eq!(started("sendmeter-impl-12"), ["sendmeter-impl-12"]);
}

#[test]
fn an_issue_nothing_drives_past_two_passes_raises_one_stall_card_naming_the_skip() {
    let (c, _) = failed_at("drive-undriven", "");
    for ago in [900, 600, 300] {
        let d = json!({"by": "auto", "started": c.now - ago, "ended": c.now - ago, "actions": []});
        append(&c.state, &Event::new(c.now - ago, "sendmeter", None, "drive-pass", d)).unwrap();
    }
    lease_as(&c, "drive-sendmeter-12.lock", 4242, c.now, 3600);
    let r = fake(NONE)
        .reply("gh pr view issue-12", &pr(&sha("abc"), "green"))
        .reply("kill -0 4242", "")
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert!(steps.is_empty(), "{steps:?} {out}");
    drive(&c, &r);
    let all = read(&c.state).unwrap();
    let cards: Vec<_> = all.iter().filter(|e| e.is("stall", "sendmeter", 12)).collect();
    assert_eq!(cards.len(), 1, "{cards:?}");
    let why = cards[0].str("reason").unwrap();
    assert!(why.contains("nothing drives this"), "{why}");
    assert!(why.contains("drive-sendmeter-12.lock"), "{why}");
}
