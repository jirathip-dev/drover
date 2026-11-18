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
        .to_vec()
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
    let (c, d) = setup("repro-verify", &(CONFIG.to_string() + VERIFY), true);
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
    assert!(verdicts(&c)[0].val("repro").is_null());
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

#[test]
fn a_verify_block_on_a_fleet_without_a_verify_role_is_no_repro_and_a_run_block_records() {
    let (c, d) = setup("repro-no-verify-role", CONFIG, true);
    let (code, out) = pass(&c, &d, SECTION, VERIFY_BLOCK);
    assert_eq!((code, verdicts(&c).len()), (3, 0), "{out}");
    assert!(
        out.contains("REFUSED NO_REPRO") && out.contains("has no verify role") && out.contains("`Repro: run`"),
        "{out}"
    );
    let (code, out) = pass(&c, &d, SECTION, &format!("{BASE_RUN}{HEAD_RUN}"));
    assert_eq!(code, 0, "{out}");
    assert_eq!(verdicts(&c)[0].str("repro"), Some("run"));
}

#[test]
fn a_merge_no_repro_on_a_fleet_without_verify_is_needs_human_not_a_rerun() {
    let (c, _d) = setup("repro-no-verify-merge", CONFIG, true);
    let v = json!({"verdict": "PASS", "sha": sha(), "stage": "rev", "loop": "rev", "body": "PASS", "repro": "verify"});
    append(&c.state, &Event::new(3, "sendmeter", Some(12), "verdict", v)).unwrap();
    let sh = FakeShed::new();
    sh.attach(&c);
    let r = drive_fake();
    for _ in 0..3 {
        let (code, out) = run(&c, &r, &["drive"]);
        assert_eq!(code, 0, "{out}");
    }
    assert_eq!(state(&c), "needs-human");
    assert!(
        !sh.called("run", "sendmeter-rev-12-r2"),
        "rev respawned at the same head"
    );
    assert!(!r.called("gh pr merge"));
    let ev = read(&c.state).unwrap();
    let nh = ev.iter().find(|e| e.kind == "needs-human").unwrap();
    assert_eq!(nh.str("code"), Some("NO_REPRO"));
    assert!(
        nh.str("reason").unwrap().contains("has no verify role"),
        "{:?}",
        nh.str("reason")
    );
}

#[test]
fn a_later_run_pass_at_the_head_clears_earlier_verify_passes() {
    let (c, _d) = setup("repro-latest-pass", CONFIG, true);
    for (ts, repro) in [(3, "verify"), (4, "verify"), (5, "run"), (6, "run")] {
        let v = json!({"verdict": "PASS", "sha": sha(), "stage": "rev", "loop": "rev", "body": "PASS", "repro": repro});
        append(&c.state, &Event::new(ts, "sendmeter", Some(12), "verdict", v)).unwrap();
    }
    let sh = FakeShed::new();
    sh.attach(&c);
    let r = drive_fake();
    run(&c, &r, &["drive"]);
    let (code, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(code, 0, "{out}");
    assert!(r.called("gh pr merge 100"));
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
fn a_merge_gate_refusal_at_a_passed_head_is_needs_human_once_and_no_rev_rerun() {
    let (c, _d) = setup("repro-gate-once", &(CONFIG.to_string() + VERIFY), true);
    let v = json!({"verdict": "PASS", "sha": sha(), "stage": "rev", "loop": "rev", "body": "PASS", "repro": "verify"});
    append(&c.state, &Event::new(3, "sendmeter", Some(12), "verdict", v)).unwrap();
    let skip = json!({"sha": sha(), "stage": "verify", "reason": "r"});
    append(&c.state, &Event::new(4, "sendmeter", Some(12), "verify-skipped", skip)).unwrap();
    let sh = FakeShed::new();
    sh.attach(&c);
    let r = drive_fake();
    for _ in 0..3 {
        let (code, out) = run(&c, &r, &["drive"]);
        assert_eq!(code, 0, "{out}");
    }
    let (code, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert!(code == 3 && out.contains("REFUSED NO_REPRO"), "{out}");
    assert_eq!(state(&c), "needs-human");
    assert!(
        !sh.called("run", "sendmeter-rev-12-r2"),
        "rev respawned at the same head"
    );
    assert!(!r.called("gh pr merge"));
    let nh = kinds(&c, "needs-human");
    assert_eq!(nh.len(), 1, "{nh:?}");
    assert_eq!(nh[0].str("code"), Some("NO_REPRO"));
    assert!(
        nh[0].str("reason").unwrap().contains("NO_REPRO"),
        "{:?}",
        nh[0].str("reason")
    );
}

#[test]
fn a_finished_rev_lanes_verdict_is_recorded_before_drive_supersedes_it() {
    let (c, d) = setup("repro-ingest", CONFIG, false);
    let wt = d.join("rev-wt");
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(wt.join("verdict.md"), "PASS\n\nAll ACs hold.\n").unwrap();
    let sp = json!({"role": "rev", "stage": "rev", "check": true, "agent": "sendmeter-rev-12", "branch": "issue-12",
        "worktree": wt.to_string_lossy(), "sha": sha()});
    append(&c.state, &Event::new(3, "sendmeter", Some(12), "spawn", sp)).unwrap();
    let b = json!({"code": "STALE_VERDICT", "reason": "no verdict for the head"});
    append(&c.state, &Event::new(4, "sendmeter", Some(12), "blocked", b)).unwrap();
    let sh = FakeShed::new();
    sh.attach(&c);
    let (code, out) = run(&c, &drive_fake(), &["drive"]);
    assert_eq!(code, 0, "{out}");
    let v = verdicts(&c);
    assert_eq!(v.len(), 1, "{out}");
    assert_eq!(
        (v[0].str("verdict"), v[0].str("sha")),
        (Some("PASS"), Some(sha().as_str()))
    );
    let before = |e: &Event| e.kind == "supersede" && e.seq < v[0].seq;
    assert!(!read(&c.state).unwrap().iter().any(before), "{out}");
}

#[test]
fn a_later_run_pass_at_the_head_does_not_force_the_verify_stage() {
    let (c, _d) = setup("repro-why-latest", &(CONFIG.to_string() + VERIFY), true);
    for (ts, repro) in [(3, "verify"), (4, "run")] {
        let v = json!({"verdict": "PASS", "sha": sha(), "stage": "rev", "loop": "rev", "body": "PASS", "repro": repro});
        append(&c.state, &Event::new(ts, "sendmeter", Some(12), "verdict", v)).unwrap();
    }
    let sh = FakeShed::new();
    sh.attach(&c);
    let r = drive_fake().reply(RECIPE, "");
    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(!sh.called("run", "sendmeter-verify-12-r1"), "{out}");
    let skip = kinds(&c, "verify-skipped");
    assert_eq!(skip.len(), 1, "{out}");
    assert!(!skip[0].str("reason").unwrap().contains("Repro: verify"), "{skip:?}");
}
