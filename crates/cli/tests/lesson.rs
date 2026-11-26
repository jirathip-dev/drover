mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::{json, Value};

const NOW: u64 = 1_000_000;
const HOUR: u64 = 3600;
const OLD: u64 = NOW - 8 * 86400;
const TOKEN: &str = "ghp_a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8";

fn ev(c: &drover::Ctx, ts: u64, issue: Option<u64>, kind: &str, d: Value) {
    append(&c.state, &Event::new(ts, "sendmeter", issue, kind, d)).unwrap();
}

fn fail(what: &str) -> Value {
    let body = format!(
        "FAIL\n\n- blocking [correctness] src/x.rs:3: {what}\n  Proof: spec\n  > AC1: it works\n  src/x.rs:3 breaks it\n- non-blocking: a nit about naming\n"
    );
    json!({"verdict": "FAIL", "sha": "a".repeat(40), "body": body})
}

#[test]
fn digest_shows_the_last_week_and_redacts_secrets() {
    let c = ctx(&tmpdir("lesson-digest"), CONFIG);
    ev(&c, OLD, Some(3), "verdict", fail("old unwrap on None"));
    ev(&c, NOW - HOUR, Some(12), "verdict", fail("unwrap on a missing config"));
    ev(&c, OLD, None, "thread-new", json!({"thread": "t1", "title": "chat"}));
    ev(
        &c,
        OLD,
        None,
        "thread-msg",
        json!({"thread": "t1", "from": "owner", "text": "old: stop that"}),
    );
    let said = format!("don't paste tokens like {TOKEN} into briefs");
    ev(
        &c,
        NOW - HOUR,
        None,
        "thread-msg",
        json!({"thread": "t1", "from": "owner", "text": said}),
    );
    ev(
        &c,
        NOW - HOUR,
        None,
        "thread-msg",
        json!({"thread": "t1", "from": "orchestrator", "text": "on it"}),
    );
    ev(&c, NOW - HOUR, Some(12), "parked", json!({"by": "owner"}));
    ev(&c, OLD, Some(3), "round-extended", json!({"by": "owner"}));
    let ask = json!({"id": "a1", "question": "which?", "options": ["keep", "drop"], "recommend": 1});
    ev(&c, NOW - 2 * HOUR, Some(12), "needs-decision", ask);
    ev(
        &c,
        NOW - HOUR,
        Some(12),
        "decision",
        json!({"id": "a1", "choice": "drop", "by": "owner"}),
    );
    let ask = json!({"id": "a2", "question": "go?", "options": ["yes", "no"], "recommend": 1});
    ev(&c, NOW - 2 * HOUR, Some(12), "needs-decision", ask);
    ev(
        &c,
        NOW - HOUR,
        Some(12),
        "decision",
        json!({"id": "a2", "choice": "yes", "by": "owner"}),
    );
    let log = format!(
        "{} abc1234 Revert \"drive: skip stalls\"\n{OLD} def5678 Revert \"old thing\"\n",
        NOW - HOUR
    );
    let r = Fake::default().reply(
        "git -C /tmp/does-not-matter/sendmeter log origin/staging -i --grep=^revert",
        &log,
    );
    let (code, out) = run(&c, &r, &["lesson", "digest", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    for want in [
        "#12 round 1:\n- blocking [correctness] src/x.rs:3: unwrap on a missing config\n  Proof: spec\n  > AC1: it works",
        "- t1: don't paste tokens like ••• into briefs",
        "parked",
        "decision",
        "drop",
        "abc1234 Revert \"drive: skip stalls\"",
    ] {
        assert!(out.contains(want), "missing {want:?}:\n{out}");
    }
    for gone in [
        TOKEN,
        "old unwrap",
        "old: stop that",
        "round-extended",
        "def5678",
        "on it",
        "a nit",
        "go?",
    ] {
        assert!(!out.contains(gone), "{gone:?} shown:\n{out}");
    }
    let (_, out) = run(&c, &r, &["lesson", "digest", "sendmeter", "--days", "9"]);
    assert!(out.contains("old unwrap") && out.contains("def5678"), "{out}");
}

const OFF: &str = "\n[fleet.quiet]\nrepo = \"acme/quiet\"\nlocal = \"/tmp/does-not-matter/quiet\"\nintegration = \"staging\"\nlessons = \"off\"\n[fleet.quiet.role.impl]\nharness = \"claude\"\n";

#[test]
fn the_lesson_job_files_only_when_the_digest_holds_two_fails_and_never_when_off() {
    let c = ctx(&tmpdir("lesson-tick"), CONFIG);
    ev(&c, OLD, Some(3), "verdict", fail("old unwrap on None"));
    ev(&c, NOW, Some(12), "verdict", fail("unwrap on a missing config"));
    let r = Fake::default().reply("git -C", "");
    let digest = ["lesson", "digest", "sendmeter", "--days", "7", "--min-fails", "2"];
    let (code, out) = run(&c, &r, &digest);
    assert_eq!(code, 0, "one FAIL in the week: the check passes\n{out}");
    ev(&c, NOW, Some(13), "verdict", fail("unwrap on a missing env"));
    let (code, out) = run(&c, &r, &digest);
    assert_eq!(code, 1, "two FAILs: the check fails and the job files\n{out}");
    assert!(out.contains("unwrap on a missing env"), "{out}");
    let (code, _) = run(&c, &r, &digest[..5]);
    assert_eq!(code, 0, "no --min-fails: a plain read");

    let cfg = drover::config::Config::parse(&(CONFIG.to_string() + OFF)).unwrap();
    let lesson = &cfg.fleet["sendmeter"].jobs["lesson"];
    assert_eq!(lesson.every, 7 * 86400);
    assert!(lesson
        .check
        .as_deref()
        .unwrap_or_default()
        .contains("lesson digest {fleet} --days 7 --min-fails 2"));
    assert_eq!(cfg.fleet["quiet"].jobs["lesson"].every, 0, "lessons = off");
}

fn spawner(n: u64) -> Fake {
    Fake::default()
        .reply(
            &format!("gh issue view {n}"),
            r#"{"title":"lessons","body":"the template"}"#,
        )
        .reply("gh issue list", "[]")
        .reply("sh -c", "")
        .reply(
            "gh issue create",
            &format!("https://github.com/jirathip-dev/sendmeter/issues/{n}\n"),
        )
        .reply("gh issue close", "")
        .reply("git -C", "0\n")
        .reply("which shed", "/opt/shed/bin/shed\n")
}

#[test]
fn lesson_files_queues_and_spawns_a_lesson_lane_that_may_close_with_no_change() {
    let c = ctx(&tmpdir("lesson-run"), CONFIG);
    let sh = FakeShed::attached(&c);
    let r = spawner(80);
    let (code, out) = run(&c, &r, &["job", "sendmeter", "lesson"]);
    assert!(code == 0 || code == 4, "{code} {out}");
    assert!(
        out.contains("filed sendmeter#80: lessons: sendmeter week of 1970-01-12"),
        "{out}"
    );
    let events = read(&c.state).unwrap();
    assert_eq!(
        drover::queue::replay(&events, "sendmeter").first().map(|i| i.issue),
        Some(80)
    );
    assert_eq!(sh.last("send")["name"], "sendmeter-lesson-80", "{:?}", sh.methods());
    let spawn = events.iter().find(|e| e.is("spawn", "sendmeter", 80)).expect("a lane");
    assert_eq!(
        (spawn.str("role"), spawn.str("profile"), spawn.str("stage")),
        (Some("lesson"), Some("fleet-impl"), Some("impl"))
    );
    let brief = std::fs::read_to_string(c.worktrees.join("sendmeter/issue-80/.brief.md")).unwrap();
    assert!(
        brief.contains("# ROLE: LESSON") && brief.contains("drover lesson digest"),
        "{brief}"
    );

    let (code, out) = run(&c, &r, &["done", "sendmeter", "80", "--no-change", "no repeated class"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        r.called("gh issue close 80 -R jirathip-dev/sendmeter --comment no repeated class"),
        "{:?}",
        r.calls.borrow()
    );
    assert_eq!(
        fold(&read(&c.state).unwrap())[&("sendmeter".into(), 80)].state,
        "closed"
    );

    let r = spawner(81);
    run(&c, &r, &["spawn", "sendmeter", "81", "impl"]);
    let (code, out) = run(&c, &r, &["done", "sendmeter", "81", "--no-change", "nothing to fix"]);
    assert_eq!(code, 0, "{out}");
    assert!(!r.called("gh issue close"), "{out}");
    assert_ne!(
        fold(&read(&c.state).unwrap())[&("sendmeter".into(), 81)].state,
        "closed"
    );
}

#[test]
fn rules_file_is_in_impl_and_rev_briefs_and_missing_is_fine() {
    let c = ctx(&tmpdir("lesson-rules"), CONFIG);
    let _sh = FakeShed::attached(&c);
    let rules = "- don't unwrap config reads (enforced by: clippy unwrap_used)\n";
    let docs = "- `docs/setup.md`: install\n";
    for (role, dir) in [("impl", "issue-12"), ("rev", "rev-12-r1")] {
        let wt = c.worktrees.join("sendmeter").join(dir);
        std::fs::create_dir_all(wt.join(".drover")).unwrap();
        std::fs::write(wt.join(".drover/rules.md"), rules).unwrap();
        std::fs::write(wt.join(".drover/docs.md"), docs).unwrap();
        let r = spawner(12)
            .once(&format!("git -C {} status --porcelain", wt.display()), 0, "")
            .once(
                "git -C /tmp/does-not-matter/sendmeter show origin/staging:.drover/rules.md",
                0,
                rules,
            );
        let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", role]);
        assert!(code == 0 || code == 4, "{role}: {code} {out}");
        let brief = std::fs::read_to_string(wt.join(".brief.md")).unwrap();
        let want = format!("## Repo rules (.drover/rules.md)\n\n{rules}\n## ");
        assert!(brief.contains(&want), "{role}: {brief}");
        assert!(
            brief.contains("## Docs you may trust (.drover/docs.md)\n\n"),
            "{role}: {brief}"
        );
        assert!(
            brief.contains(&format!("unless this brief names it.\n\n{docs}\n## ")),
            "{role}: {brief}"
        );
    }
    let c = ctx(&tmpdir("lesson-norules"), CONFIG);
    let _sh = FakeShed::attached(&c);
    let (code, out) = run(&c, &spawner(12), &["spawn", "sendmeter", "12", "impl"]);
    assert!(code == 0 || code == 4, "{code} {out}");
    let brief = std::fs::read_to_string(c.worktrees.join("sendmeter/issue-12/.brief.md")).unwrap();
    assert!(
        !brief.contains("Repo rules") && !brief.contains("Docs you may trust"),
        "{brief}"
    );
}

#[test]
fn lesson_check_fails_a_rule_naming_no_enforcer_and_a_file_over_30_lines_with_no_fleet_config() {
    let d = tmpdir("lesson-format");
    std::fs::create_dir_all(d.join(".drover")).unwrap();
    let check = |rules: String| {
        std::fs::write(d.join(".drover/rules.md"), rules).unwrap();
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_drover"))
            .arg("lesson-check")
            .env("HOME", d.join("home"))
            .env("FLEET_CONFIG", d.join("none.toml"))
            .current_dir(&d)
            .output()
            .unwrap();
        (
            o.status.code().unwrap(),
            String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr),
        )
    };
    let ok = "- don't X (enforced by: none)\n";
    assert_eq!(check(ok.repeat(30)), (0, String::new()));
    let (code, out) = check(ok.repeat(31));
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("31 lines, over 30"), "{out}");
    for bare in ["- don't Y\n", "- don't Y (enforced by: )\n"] {
        let (code, out) = check(format!("# Rules\n{ok}{bare}"));
        assert_eq!(code, 1, "{bare}: {out}");
        assert!(out.contains("line 3 names no enforcer"), "{out}");
        assert!(!out.contains("line 2") && !out.contains("line 1 "), "{out}");
    }
}

#[test]
fn a_failing_shadow_check_passes_ci_and_the_summary_shows_would_fail() {
    let d = tmpdir("lesson-shadow-ci");
    std::fs::create_dir_all(d.join(".drover")).unwrap();
    std::fs::write(d.join("a.rs"), "fn a() {}\nlet x = cfg.unwrap();\n").unwrap();
    let summary = d.join("summary.md");
    let check = |mark: &str| {
        let rules = format!(
            "- don't unwrap config (enforced by: no-unwrap `! grep -Hn unwrap a.rs`, {mark} since 2026-11-01)\n\
             - don't hand (enforced by: hand `echo hand-ran; false`)\n"
        );
        std::fs::write(d.join(".drover/rules.md"), rules).unwrap();
        let _ = std::fs::remove_file(&summary);
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_drover"))
            .arg("lesson-check")
            .env("GITHUB_STEP_SUMMARY", &summary)
            .current_dir(&d)
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&o.stdout).into_owned();
        (
            o.status.code().unwrap(),
            out,
            std::fs::read_to_string(&summary).unwrap_or_default(),
        )
    };
    let (code, out, sum) = check("shadow");
    assert_eq!(code, 0, "{out}");
    assert!(
        sum.contains("would-fail (shadow) no-unwrap") && sum.contains("a.rs:2:let x = cfg.unwrap();"),
        "{sum}"
    );
    assert!(!out.contains("hand") && !sum.contains("hand"), "{out}");
    let (code, out, _) = check("active");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("FAIL no-unwrap") && out.contains("a.rs:2:"), "{out}");
    std::fs::write(d.join("a.rs"), "fn a() {}\n").unwrap();
    let (code, out, sum) = check("shadow");
    assert_eq!((code, out.as_str(), sum.as_str()), (0, "", ""));
}

const DAY: u64 = 86400;

#[test]
fn lesson_apply_promotes_and_retires_by_history_and_never_touches_hand_written_checks() {
    let mut c = ctx(&tmpdir("lesson-apply"), CONFIG);
    c.now = 400 * DAY;
    let line = |name: &str, mark: &str| format!("- don't {name} (enforced by: {name} `false`, {mark})\n");
    let hand = "- don't hand (enforced by: hand `false`)\n- don't nap (enforced by: none)\n";
    let rules = [
        line("caught", "shadow since 1971-01-20"),
        line("quick", "shadow since 1971-02-03"),
        line("young", "shadow since 1971-02-03"),
        line("noisy", "shadow since 1971-01-20"),
        line("idle", "shadow since 1971-01-01"),
        line("quiet", "shadow since 1971-01-25"),
        line("unrun", "shadow since 1971-01-01"),
        line("owned", "shadow since 1971-02-04"),
        line("oldactive", "active since 1970-12-01"),
        line("steady", "active since 1970-12-01"),
        hand.into(),
    ]
    .concat();
    for (name, day, prs, hits) in [
        ("caught", 390, 3, 1),
        ("quick", 398, 10, 1),
        ("young", 398, 3, 1),
        ("noisy", 390, 3, 2),
        ("idle", 366, 5, 0),
        ("quiet", 390, 4, 0),
        ("caught", 380, 4, 4),
    ] {
        for i in 0..prs {
            let hit: Vec<&str> = if i < hits { vec![name] } else { vec![] };
            ev(
                &c,
                day * DAY + i,
                Some(100 + i),
                "lesson-shadow",
                json!({"ran": [name], "hits": hit}),
            );
        }
    }
    let fp = json!({"verdict": "FAIL", "body": "FAIL\n\nfalse positive: noisy\n"});
    ev(&c, 395 * DAY, Some(7), "verdict", fp);
    let r = Fake::default()
        .reply(
            "gh pr list -R jirathip-dev/sendmeter --state all --search lessons: in:title --json body",
            "Closes #90\npromote owned\nretire oldactive\nretire hand\n",
        )
        .reply(
            "git -C /tmp/does-not-matter/sendmeter show origin/staging:.drover/rules.md",
            &rules,
        );
    let (code, out) = run(&c, &r, &["lesson", "digest", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    for want in [
        "- caught: shadow since 1971-01-20, 3 PRs, 1 caught, 0 false positives → promote",
        "- quick: shadow since 1971-02-03, 10 PRs, 1 caught, 0 false positives → promote",
        "- young: shadow since 1971-02-03, 3 PRs, 1 caught, 0 false positives → keep shadow",
        "- noisy: shadow since 1971-01-20, 3 PRs, 2 caught, 1 false positives → fix or remove",
        "- idle: shadow since 1971-01-01, 5 PRs, 0 caught, 0 false positives → retire",
        "- quiet: shadow since 1971-01-25, 4 PRs, 0 caught, 0 false positives → keep shadow",
        "- unrun: shadow since 1971-01-01, 0 PRs, 0 caught, 0 false positives → keep shadow",
        "- owned: shadow since 1971-02-04, 0 PRs, 0 caught, 0 false positives → promote",
        "- oldactive: active since 1970-12-01, 0 PRs, 0 caught, 0 false positives → retire",
        "- steady: active since 1970-12-01, 0 PRs, 0 caught, 0 false positives → keep active",
    ] {
        assert!(out.contains(want), "missing {want:?}:\n{out}");
    }
    assert!(!out.contains("- hand:") && !out.contains("- nap:"), "{out}");

    let file = c.state.with_file_name("rules.md");
    std::fs::write(&file, &rules).unwrap();
    let (code, out) = run(
        &c,
        &r,
        &["lesson", "apply", "sendmeter", "--rules", file.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{out}");
    let want = [
        line("caught", "active since 1971-02-05"),
        line("quick", "active since 1971-02-05"),
        line("young", "shadow since 1971-02-03"),
        line("noisy", "shadow since 1971-01-20"),
        line("quiet", "shadow since 1971-01-25"),
        line("unrun", "shadow since 1971-01-01"),
        line("owned", "active since 1971-02-05"),
        line("steady", "active since 1970-12-01"),
        hand.into(),
    ]
    .concat();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), want);
    assert!(
        out.contains("noisy: shadow since 1971-01-20, 3 PRs, 2 caught, 1 false positives → fix or remove"),
        "{out}"
    );
}

#[test]
fn briefs_carry_the_shared_pack_before_the_repo_rules() {
    let c = ctx(&tmpdir("lesson-shared"), CONFIG);
    let _sh = FakeShed::attached(&c);
    let wt = c.worktrees.join("sendmeter").join("rev-12-r1");
    std::fs::create_dir_all(wt.join(".drover")).unwrap();
    std::fs::write(wt.join(".drover/rules.md"), "- don't X (enforced by: none)\n").unwrap();
    let r = spawner(12).once(&format!("git -C {} status --porcelain", wt.display()), 0, "");
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "rev"]);
    assert!(code == 0 || code == 4, "{code} {out}");
    let brief = std::fs::read_to_string(wt.join(".brief.md")).unwrap();
    let (shared, repo) = (
        brief.find("## Shared rules (rules/shared.md)\n\n- Subtract before you add"),
        brief.find("## Repo rules"),
    );
    assert!(shared.is_some() && shared < repo, "{brief}");
    let c = ctx(&tmpdir("lesson-shared-none"), CONFIG);
    let _sh = FakeShed::attached(&c);
    let (code, out) = run(&c, &spawner(12), &["spawn", "sendmeter", "12", "impl"]);
    assert!(code == 0 || code == 4, "{code} {out}");
    let brief = std::fs::read_to_string(c.worktrees.join("sendmeter/issue-12/.brief.md")).unwrap();
    assert!(
        brief.contains("## Shared rules") && !brief.contains("Repo rules"),
        "{brief}"
    );
}

#[test]
fn a_prose_only_lesson_pr_is_a_fail() {
    let head = "b".repeat(40);
    let prose = "diff --git a/roles/impl.md b/roles/impl.md\n--- a/roles/impl.md\n+++ b/roles/impl.md\n@@ -1 +1,2 @@\n x\n+- be careful with unwrap\n";
    let check = "diff --git a/.drover/rules.md b/.drover/rules.md\n--- a/.drover/rules.md\n+++ b/.drover/rules.md\n@@ -0,0 +1 @@\n+- don't unwrap (enforced by: no-unwrap `! grep -rn unwrap src`, shadow since 1970-01-12)\n";
    let retire = "diff --git a/.drover/rules.md b/.drover/rules.md\n--- a/.drover/rules.md\n+++ b/.drover/rules.md\n@@ -1 +0,0 @@\n-- don't X (enforced by: none)\n";
    let code_pr = "diff --git a/src/x.rs b/src/x.rs\n+fn typed() {}\n";
    for (diff, want) in [(prose, "FAIL"), (check, "PASS"), (retire, "PASS"), (code_pr, "PASS")] {
        let c = ctx(&tmpdir("lesson-prose"), CONFIG);
        let _sh = FakeShed::attached(&c);
        ev(
            &c,
            NOW - 2 * HOUR,
            Some(12),
            "spawn",
            json!({"role": "lesson", "agent": "sendmeter-lesson-12"}),
        );
        let f = c.state.with_file_name("verdict.md");
        std::fs::write(&f, "PASS\n\nlooks fine\n").unwrap();
        let r = Fake::default()
            .reply("gh api repos/jirathip-dev/sendmeter/branches/issue-12", &head)
            .reply("gh pr diff issue-12 -R jirathip-dev/sendmeter", diff);
        let args = [
            "verdict",
            "sendmeter",
            "12",
            "PASS",
            "--sha",
            &head,
            "--file",
            f.to_str().unwrap(),
        ];
        let (code, out) = run(&c, &r, &args);
        assert_eq!(code, 0, "{out}");
        let events = read(&c.state).unwrap();
        let v = events.iter().rfind(|e| e.is("verdict", "sendmeter", 12)).unwrap();
        assert_eq!(v.str("verdict"), Some(want), "{diff}: {out}");
        if want == "FAIL" {
            let body = v.str("body").unwrap();
            assert!(
                body.starts_with("FAIL\n\n- blocking [lesson]") && body.contains("A PR that only edits prose"),
                "{body}"
            );
            assert!(drover::gate::no_proof(body).is_none(), "{body}");
        }
    }
    let c = ctx(&tmpdir("lesson-prose-impl"), CONFIG);
    let _sh = FakeShed::attached(&c);
    ev(
        &c,
        NOW - 2 * HOUR,
        Some(12),
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12"}),
    );
    let f = c.state.with_file_name("verdict.md");
    std::fs::write(&f, "PASS\n").unwrap();
    let r = Fake::default().reply("gh api repos/jirathip-dev/sendmeter/branches/issue-12", &head);
    let (code, out) = run(
        &c,
        &r,
        &[
            "verdict",
            "sendmeter",
            "12",
            "PASS",
            "--sha",
            &head,
            "--file",
            f.to_str().unwrap(),
        ],
    );
    assert!(code == 0 && out.contains("recorded PASS"), "{out}");
    assert!(!r.called("gh pr diff"), "{out}");
}

#[test]
fn a_pr_that_deletes_every_rule_is_reviewed_by_the_integration_rules() {
    let c = ctx(&tmpdir("lesson-own-rules"), CONFIG);
    let _sh = FakeShed::attached(&c);
    let wt = c.worktrees.join("sendmeter").join("rev-12-r1");
    std::fs::create_dir_all(wt.join(".drover")).unwrap();
    std::fs::write(wt.join(".drover/rules.md"), "").unwrap();
    let rules = "- don't unwrap config reads (enforced by: clippy unwrap_used)\n";
    let diff = "diff --git a/.drover/rules.md b/.drover/rules.md\n--- a/.drover/rules.md\n+++ b/.drover/rules.md\n@@ -1 +0,0 @@\n-- don't unwrap config reads (enforced by: clippy unwrap_used)\n";
    let r = spawner(12)
        .once(&format!("git -C {} status --porcelain", wt.display()), 0, "")
        .once(
            "git -C /tmp/does-not-matter/sendmeter show origin/staging:.drover/rules.md",
            0,
            rules,
        )
        .once(
            &format!("git -C {} diff origin/staging...HEAD --", wt.display()),
            0,
            diff,
        );
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "rev"]);
    assert!(code == 0 || code == 4, "{code} {out}");
    let brief = std::fs::read_to_string(wt.join(".brief.md")).unwrap();
    let want = format!("## Repo rules (.drover/rules.md)\n\n{rules}\n## ");
    assert!(brief.contains(&want), "{brief}");
    assert!(
        brief.contains("This PR changes `.drover/rules.md`") && brief.contains(diff),
        "{brief}"
    );
}

#[test]
fn a_lesson_pr_is_reviewed_by_its_own_rules_as_before() {
    let c = ctx(&tmpdir("lesson-pr-rules"), CONFIG);
    let _sh = FakeShed::attached(&c);
    let wt = c.worktrees.join("sendmeter").join("rev-12-r1");
    std::fs::create_dir_all(wt.join(".drover")).unwrap();
    let rules = "- don't X (enforced by: lesson x `true`, shadow since 1970-01-12)\n";
    std::fs::write(wt.join(".drover/rules.md"), rules).unwrap();
    let filed = json!({"job": "lesson", "role": "lesson", "title": "lessons: sendmeter week of 1970-01-12"});
    ev(&c, NOW, Some(12), "job-filed", filed);
    let r = Fake::default()
        .reply(
            "gh issue view 12",
            r#"{"title":"lessons: sendmeter week of 1970-01-12","body":"b"}"#,
        )
        .reply("git -C", "0\n")
        .reply("which shed", "/opt/shed/bin/shed\n")
        .once(&format!("git -C {} status --porcelain", wt.display()), 0, "");
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "rev"]);
    assert!(code == 0 || code == 4, "{code} {out}");
    let brief = std::fs::read_to_string(wt.join(".brief.md")).unwrap();
    let want = format!("## Repo rules (.drover/rules.md)\n\n{rules}\n## ");
    assert!(brief.contains(&want) && !brief.contains("This PR changes"), "{brief}");
    assert!(
        !r.called("git -C /tmp/does-not-matter/sendmeter show"),
        "{:?}",
        r.calls.borrow()
    );
}
