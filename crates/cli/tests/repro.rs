mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::json;

const VERIFY: &str = "\n[fleet.sendmeter.role.verify]\nharness = \"hermes\"\nprofile = \"fleet-verify\"\n";
const BRANCH: &str = "gh api repos/jirathip-dev/sendmeter/branches/issue-12";
const PR_BODY: &str = "gh pr view issue-12 -R jirathip-dev/sendmeter --json body";
const RECIPE: &str = "gh api repos/jirathip-dev/sendmeter/contents/.drover/verify.md";
const BASE_RUN: &str = "Repro: run\n\nBase plus commit 1:\n```\ncargo test meter_total\n```\n```\n\
test meter_total ... FAILED\n```\n";
const HEAD_RUN: &str = "Head:\n```\ncargo test meter_total\n```\n```\ntest meter_total ... ok\n```\n";
const VERIFY_BLOCK: &str = "Repro: verify\n> a pure visual bug: the meter's colour has no unit test path\n";
const SECTION: &str = "Closes #12\n\n## Repro\n\nA pure visual bug: the meter's colour has no\nunit test path.\n\n\
## For humans\n\nx\n";

fn sha() -> String {
    "a".repeat(40)
}

fn setup(name: &str, cfg: &str, marked: bool) -> (drover::Ctx, std::path::PathBuf) {
    let d = tmpdir(name);
    let c = ctx(&d, cfg);
    if marked {
        let (code, out) = run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--repro"]);
        assert_eq!(code, 0, "{out}");
    }
    for (ts, role) in [(1, "impl"), (2, "rev")] {
        let e = json!({"role": role, "stage": role, "check": role == "rev", "agent": format!("sendmeter-{role}-12"),
            "branch": "issue-12"});
        append(&c.state, &Event::new(ts, "sendmeter", Some(12), "spawn", e)).unwrap();
    }
    (c, d)
}

fn verdict(c: &drover::Ctx, d: &std::path::Path, r: &Fake, v: &str, body: &str) -> (i32, String) {
    let file = d.join("verdict.md");
    std::fs::write(&file, body).unwrap();
    let f = file.to_string_lossy();
    run(c, r, &["verdict", "sendmeter", "12", v, "--sha", &sha(), "--file", &f])
}

fn fake(pr_body: &str) -> Fake {
    Fake::default().reply(PR_BODY, pr_body).reply(BRANCH, &sha())
}

fn verdicts(c: &drover::Ctx) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "verdict")
        .collect()
}

fn pass(c: &drover::Ctx, d: &std::path::Path, pr_body: &str, block: &str) -> (i32, String) {
    verdict(
        c,
        d,
        &fake(pr_body),
        "PASS",
        &format!("PASS\n\nAll ACs hold.\n\n{block}"),
    )
}

#[test]
fn a_full_run_block_is_recorded_and_a_missing_or_half_one_is_no_repro() {
    let (c, d) = setup("repro-run", CONFIG, true);
    for block in [
        "",
        BASE_RUN,
        "Repro: run\n```\ncargo test meter_total\n```\n",
        "Repro: maybe\n",
    ] {
        let (code, out) = pass(&c, &d, "", block);
        assert_eq!((code, verdicts(&c).len()), (3, 0), "{block}: {out}");
        assert!(out.contains("REFUSED NO_REPRO"), "{block}: {out}");
    }
    let (_, out) = pass(&c, &d, "", BASE_RUN);
    assert!(out.contains("it has 2 fenced blocks"), "{out}");
    let (code, out) = pass(&c, &d, "", &format!("{BASE_RUN}{HEAD_RUN}\n## Notes\n\nnone\n"));
    assert_eq!(code, 0, "{out}");
    let v = verdicts(&c);
    assert_eq!((v.len(), v[0].str("repro")), (1, Some("run")));
}

const ONE_COMMAND: &str = "Repro: run\n```\ncargo test --locked -p drover --test shed a_remote_host\n```\n\
At base + commit 1 (4c43ddc55), failing:\n```\nshed connect failed (1)\ntest result: FAILED. 0 passed; 1 failed\n```\n\
Same command at head 5c77f77, passing:\n```\ntest a_remote_host ... ok\ntest result: ok. 1 passed; 0 failed\n```\n";

#[test]
fn one_command_fence_with_both_outputs_is_recorded() {
    let (c, d) = setup("repro-one-command", CONFIG, true);
    let (code, out) = pass(&c, &d, "", ONE_COMMAND);
    assert_eq!(code, 0, "{out}");
    let v = verdicts(&c);
    assert_eq!(
        (v.len(), v[0].str("verdict"), v[0].str("repro")),
        (1, Some("PASS"), Some("run"))
    );
}

#[test]
fn a_run_block_needs_two_outputs_and_a_failing_base() {
    let (c, d) = setup("repro-two-outputs", CONFIG, true);
    let cmd = "Repro: run\n```\ncargo test meter_total\n```\n";
    let ok = "```\ntest result: ok. 1 passed; 0 failed\n```\n";
    let cases = [
        (format!("{cmd}{cmd}{ok}"), "it has 3 fenced blocks"),
        (format!("{cmd}{ok}{ok}"), "base plus commit 1 output shows no failure"),
    ];
    for (block, why) in cases {
        let (code, out) = pass(&c, &d, "", &block);
        assert_eq!((code, verdicts(&c).len()), (3, 0), "{block}: {out}");
        assert!(out.contains("REFUSED NO_REPRO") && out.contains(why), "{block}: {out}");
    }
}

#[test]
fn a_verify_block_needs_the_pr_bodys_repro_section_and_its_quote() {
    let (c, d) = setup("repro-verify", CONFIG, true);
    let cases = [
        ("Closes #12\n", VERIFY_BLOCK, "the PR body has no `## Repro` section"),
        (SECTION, "Repro: verify\nno quote here\n", "has no `>` quoted reason"),
        (
            SECTION,
            "Repro: verify\n> it was hard to test\n",
            "quote is not in the PR body's `## Repro` section",
        ),
    ];
    for (body, block, why) in cases {
        let (code, out) = pass(&c, &d, body, block);
        assert_eq!((code, verdicts(&c).len()), (3, 0), "{block}: {out}");
        assert!(out.contains("REFUSED NO_REPRO") && out.contains(why), "{block}: {out}");
    }
    let (code, out) = pass(&c, &d, SECTION, VERIFY_BLOCK);
    assert_eq!(code, 0, "{out}");
    assert_eq!(verdicts(&c)[0].str("repro"), Some("verify"));
}

#[test]
fn an_unmarked_issues_pass_is_never_parsed_for_a_repro_block() {
    let (c, d) = setup("repro-unmarked", CONFIG, false);
    let r = fake("");
    let (code, out) = verdict(&c, &d, &r, "PASS", "PASS\n\nRepro: maybe\n");
    assert_eq!(code, 0, "{out}");
    assert!(!r.called(PR_BODY), "{:?}", r.calls.borrow());
    assert!(verdicts(&c)[0].data["repro"].is_null());
}

#[test]
fn queue_set_turns_the_mark_off_and_on() {
    let (c, d) = setup("repro-set", CONFIG, true);
    let set = |v: &str| run(&c, &Fake::default(), &["queue", "sendmeter", "set", "12", "repro", v]);
    assert_eq!(set("maybe").0, 1);
    assert_eq!(set("off").0, 0);
    assert_eq!(pass(&c, &d, "", "").0, 0, "unmarked: no block needed");
    assert_eq!(set("on").0, 0);
    let (_, ls) = run(&c, &Fake::default(), &["queue", "sendmeter", "ls"]);
    assert!(ls.contains("repro"), "{ls}");
}

fn pr(head: &str) -> String {
    json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": head, "mergeable": "MERGEABLE",
        "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}]})
    .to_string()
}

fn drive_fake() -> Fake {
    fake(SECTION)
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("gh pr view issue-12", &pr(&sha()))
        .reply("gh api repos/jirathip-dev/sendmeter/pulls/100/files", "src/a.rs\n")
        .reply("gh pr merge 100", "")
        .reply("gh pr view 100", r#"{"mergeCommit":{"oid":"m3rg3"}}"#)
        .reply("gh issue", "")
        .reply("git -C", "")
        .reply("which shed", "/opt/shed/bin/shed\n")
}

fn state(c: &drover::Ctx) -> String {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)].shown()
}

#[test]
fn a_repro_verify_pass_goes_to_verify_outside_ui_paths() {
    let (c, d) = setup("repro-to-verify", &(CONFIG.to_string() + VERIFY), true);
    assert_eq!(pass(&c, &d, SECTION, VERIFY_BLOCK).0, 0);
    let sh = FakeShed::new();
    sh.attach(&c);
    let r = drive_fake().reply(RECIPE, "");
    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(sh.called("run", "sendmeter-verify-12-r1"), "{out}");
    assert_eq!(state(&c), "verify(1)");
    let brief = std::fs::read_to_string(c.worktrees.join("sendmeter/verify-12-r1/.brief.md")).unwrap();
    assert!(brief.contains("bug on the PR base"), "{brief}");
}

#[test]
fn a_repro_verify_pass_merges_only_after_a_verify_pass_at_the_head() {
    let (c, d) = setup("repro-merge", &(CONFIG.to_string() + VERIFY), true);
    assert_eq!(pass(&c, &d, SECTION, VERIFY_BLOCK).0, 0);
    let sh = FakeShed::new();
    sh.attach(&c);
    let r = drive_fake();
    run(&c, &r, &["drive"]);
    let (code, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED NO_REPRO"), "{out}");
    assert!(!r.called("gh pr merge"));
    let v = json!({"verdict": "PASS", "sha": sha(), "stage": "verify", "loop": "rev", "body": "PASS"});
    append(&c.state, &Event::new(50, "sendmeter", Some(12), "verdict", v)).unwrap();
    let (code, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(code, 0, "{out}");
    assert!(r.called("gh pr merge 100"));
}

#[test]
fn help_agent_documents_repro_and_no_repro() {
    let c = ctx(&tmpdir("repro-help"), CONFIG);
    let (code, out) = run(&c, &Fake::default(), &["help", "--agent"]);
    assert_eq!(code, 0);
    assert!(out.contains("--repro") && out.contains("NO_REPRO"), "{out}");
}
