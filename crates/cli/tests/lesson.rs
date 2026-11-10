//! #419: the lesson role: `lesson digest`, `lesson` (issue, queue, lane), its weekly tick, rules in briefs and CI.
mod common;
use common::*;
use herdsman::events::{append, fold, read, Event};
use serde_json::{json, Value};

const NOW: u64 = 1_000_000;
const HOUR: u64 = 3600;
const OLD: u64 = NOW - 8 * 86400;
const TOKEN: &str = "ghp_a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8";

fn ev(c: &herdsman::Ctx, ts: u64, issue: Option<u64>, kind: &str, d: Value) {
    append(&c.state, &Event::new(ts, "sendmeter", issue, kind, d)).unwrap();
}

/// A FAIL verdict body with one proven blocker `what` and a non-blocking note.
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
        json!({"thread": "t1", "from": "conductor", "text": "on it"}),
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
    // `--days` widens the window.
    let (_, out) = run(&c, &r, &["lesson", "digest", "sendmeter", "--days", "9"]);
    assert!(out.contains("old unwrap") && out.contains("def5678"), "{out}");
}

const OFF: &str = "\n[fleet.quiet]\nrepo = \"acme/quiet\"\nlocal = \"/tmp/does-not-matter/quiet\"\nintegration = \"staging\"\nlessons = \"off\"\n[fleet.quiet.role.impl]\nharness = \"hermes\"\n";

#[test]
fn tick_runs_lesson_once_a_week_and_never_when_off() {
    let c = ctx(&tmpdir("lesson-tick"), &(CONFIG.to_string() + OFF));
    let _sh = FakeShed::attached(&c);
    // #517: a fleet over a week old with 2 FAILs this week.
    ev(&c, OLD, Some(3), "verdict", fail("old unwrap on None"));
    ev(&c, NOW, Some(12), "verdict", fail("unwrap on a missing config"));
    ev(&c, NOW, Some(13), "verdict", fail("unwrap on a missing env"));
    let fake = || {
        Fake::default()
            .reply("herdr status", "server:\n  status: running\n")
            .reply("which shed", "/opt/shed/bin/shed\n")
            .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
            .reply(
                "gh issue create",
                "https://github.com/jirathip-dev/sendmeter/issues/77\n",
            )
    };
    let tick = |now: u64| {
        let r = fake();
        let (code, out) = run(&herdsman::Ctx { now, ..c.clone() }, &r, &["tick"]);
        assert_eq!(code, 0, "{out}");
        let calls: Vec<String> = r
            .calls
            .borrow()
            .iter()
            .filter(|l| l.starts_with("gh issue create"))
            .cloned()
            .collect();
        calls
    };
    let first = tick(NOW);
    assert_eq!(first.len(), 1, "{first:?}");
    assert!(
        first[0].contains("-R jirathip-dev/sendmeter --title lessons: sendmeter week of 1970-01-12"),
        "{first:?}"
    );
    let q = herdsman::queue::replay(&read(&c.state).unwrap(), "sendmeter");
    assert_eq!(q.first().map(|i| i.issue), Some(77));
    assert!(tick(NOW + 6 * 86400).is_empty(), "within the week");
    assert_eq!(tick(NOW + 7 * 86400).len(), 1, "a week on");
}

/// #517: a fleet under a week old, or with under 2 FAIL verdicts this week, skips its weekly run: `lesson-skipped`.
#[test]
fn tick_skips_lesson_for_a_young_or_quiet_fleet() {
    let c = ctx(&tmpdir("lesson-young"), CONFIG);
    let _sh = FakeShed::attached(&c);
    let r = Fake::default()
        .reply("herdr status", "server:\n  status: running\n")
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply(
            "gh issue create",
            "https://github.com/jirathip-dev/sendmeter/issues/77\n",
        );
    let skipped = |c: &herdsman::Ctx| -> Vec<Event> {
        read(&c.state)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == "lesson-skipped")
            .collect()
    };
    // A day old, 2 FAILs: too young.
    ev(&c, NOW - 86400, Some(12), "verdict", fail("unwrap on a missing config"));
    ev(&c, NOW - HOUR, Some(13), "verdict", fail("unwrap on a missing env"));
    let (code, out) = run(&c, &r, &["tick"]);
    assert_eq!(code, 0, "{out}");
    assert!(!r.called("gh issue create"), "{out}");
    let s = skipped(&c);
    assert_eq!(
        (s.len(), s[0].fleet.as_str(), s[0].str("why")),
        (1, "sendmeter", Some("fleet under a week old"))
    );
    // Over a week old, 1 FAIL this week: no repeated class to find either.
    let c = ctx(&tmpdir("lesson-quiet"), CONFIG);
    let _sh = FakeShed::attached(&c);
    ev(&c, OLD, Some(3), "verdict", fail("old unwrap on None"));
    ev(&c, NOW - HOUR, Some(12), "verdict", fail("unwrap on a missing config"));
    let (code, out) = run(&c, &r, &["tick"]);
    assert_eq!(code, 0, "{out}");
    assert!(!r.called("gh issue create"), "{out}");
    assert_eq!(skipped(&c)[0].str("why"), Some("under 2 FAIL verdicts this week"));
}

/// The fake for one spawn on shed, issue `n`'s spec as `title`.
fn spawner(n: u64) -> Fake {
    Fake::default()
        .reply(
            &format!("gh issue view {n}"),
            r#"{"title":"lessons","body":"the template"}"#,
        )
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
    let (code, out) = run(&c, &r, &["lesson", "sendmeter"]);
    assert!(code == 0 || code == 4, "{code} {out}");
    assert!(
        out.contains("filed sendmeter#80: lessons: sendmeter week of 1970-01-12"),
        "{out}"
    );
    let events = read(&c.state).unwrap();
    assert_eq!(
        herdsman::queue::replay(&events, "sendmeter").first().map(|i| i.issue),
        Some(80)
    );
    assert_eq!(sh.last("run")["name"], "sendmeter-lesson-80", "{:?}", sh.methods());
    let spawn = events.iter().find(|e| e.is("spawn", "sendmeter", 80)).expect("a lane");
    // No `[role.lesson]`: impl's harness and profile.
    assert_eq!(
        (spawn.str("role"), spawn.str("profile"), spawn.str("stage")),
        (Some("lesson"), Some("fleet-impl"), Some("impl"))
    );
    let brief = std::fs::read_to_string(c.worktrees.join("sendmeter/issue-80/.brief.md")).unwrap();
    assert!(
        brief.contains("# ROLE: LESSON") && brief.contains("herdsman lesson digest"),
        "{brief}"
    );

    // No repeated class: the lane's `done --no-change` closes the issue with that line.
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

    // An impl lane's `done --no-change` only records the fix round, as before.
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
    for (role, dir) in [("impl", "issue-12"), ("rev", "rev-12-r1")] {
        let wt = c.worktrees.join("sendmeter").join(dir);
        std::fs::create_dir_all(wt.join(".herdsman")).unwrap();
        std::fs::write(wt.join(".herdsman/rules.md"), rules).unwrap();
        // The checkout spawn made: clean.
        let r = spawner(12).once(&format!("git -C {} status --porcelain", wt.display()), 0, "");
        let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", role]);
        assert!(code == 0 || code == 4, "{role}: {code} {out}");
        let brief = std::fs::read_to_string(wt.join(".brief.md")).unwrap();
        let want = format!("## Repo rules (.herdsman/rules.md)\n\n{rules}\n## ");
        assert!(brief.contains(&want), "{role}: {brief}");
    }
    let c = ctx(&tmpdir("lesson-norules"), CONFIG);
    let _sh = FakeShed::attached(&c);
    let (code, out) = run(&c, &spawner(12), &["spawn", "sendmeter", "12", "impl"]);
    assert!(code == 0 || code == 4, "{code} {out}");
    let brief = std::fs::read_to_string(c.worktrees.join("sendmeter/issue-12/.brief.md")).unwrap();
    assert!(!brief.contains("Repo rules"), "{brief}");
}

#[test]
fn a_rules_file_over_30_lines_fails_the_check() {
    let d = tmpdir("lesson-cap");
    let (c, r) = (ctx(&d, CONFIG), Fake::default());
    let cap = d.join("cap.toml");
    std::fs::write(&cap, "[fleet.x]\nline_cap = 100000000\n").unwrap();
    let rules = d.join("rules.md");
    let check = |n: usize| {
        std::fs::write(&rules, "- don't X (enforced by: none)\n".repeat(n)).unwrap();
        run(
            &c,
            &r,
            &[
                "cap-check",
                "--config",
                cap.to_str().unwrap(),
                "--rules",
                rules.to_str().unwrap(),
            ],
        )
    };
    assert_eq!(check(30).0, 0);
    let (code, out) = check(31);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("31 lines, over 30"), "{out}");
    // No rules file: fine.
    let none = d.join("none.md");
    let (code, out) = run(
        &c,
        &r,
        &[
            "cap-check",
            "--config",
            cap.to_str().unwrap(),
            "--rules",
            none.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{out}");
}

/// #510: CI's `lesson-check`: a failing shadow check passes the build and shows would-fail (with file:line) in the job
/// summary; the same check active fails it; a hand-written line is not run.
#[test]
fn a_failing_shadow_check_passes_ci_and_the_summary_shows_would_fail() {
    let d = tmpdir("lesson-shadow-ci");
    std::fs::create_dir_all(d.join(".herdsman")).unwrap();
    std::fs::write(d.join("a.rs"), "fn a() {}\nlet x = cfg.unwrap();\n").unwrap();
    let summary = d.join("summary.md");
    let check = |mark: &str| {
        let rules = format!(
            "- don't unwrap config (enforced by: no-unwrap `! grep -Hn unwrap a.rs`, {mark} since 2026-11-01)\n\
             - don't hand (enforced by: hand `echo hand-ran; false`)\n"
        );
        std::fs::write(d.join(".herdsman/rules.md"), rules).unwrap();
        let _ = std::fs::remove_file(&summary);
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_herdsman"))
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
    // Nothing fails: no summary written.
    std::fs::write(d.join("a.rs"), "fn a() {}\n").unwrap();
    let (code, out, sum) = check("shadow");
    assert_eq!((code, out.as_str(), sum.as_str()), (0, "", ""));
}

const DAY: u64 = 86400;

/// #510: `lesson apply` (and the digest) on fixture histories: promote after 7 days or 10 PRs with a catch, fix or
/// remove on a false positive, retire after 30 days without a catch, the owner's PR-body lines first; a hand-written
/// line is untouched even when the owner names it.
#[test]
fn lesson_apply_promotes_and_retires_by_history_and_never_touches_hand_written_checks() {
    let mut c = ctx(&tmpdir("lesson-apply"), CONFIG);
    c.now = 400 * DAY; // 1971-02-05
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
    // (check, PRs on day, of which caught)
    for (name, day, prs, hits) in [
        ("caught", 390, 3, 1),
        ("quick", 398, 10, 1),
        ("young", 398, 3, 1),
        ("noisy", 390, 3, 2),
        ("idle", 366, 5, 0),
        ("quiet", 390, 4, 0),
        ("caught", 380, 4, 4), // before its shadow date: not counted
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
            "git -C /tmp/does-not-matter/sendmeter show origin/staging:.herdsman/rules.md",
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

/// #733: every brief carries the shared rules pack before the repo's rules, and without them too.
#[test]
fn briefs_carry_the_shared_pack_before_the_repo_rules() {
    let c = ctx(&tmpdir("lesson-shared"), CONFIG);
    let _sh = FakeShed::attached(&c);
    let wt = c.worktrees.join("sendmeter").join("rev-12-r1");
    std::fs::create_dir_all(wt.join(".herdsman")).unwrap();
    std::fs::write(wt.join(".herdsman/rules.md"), "- don't X (enforced by: none)\n").unwrap();
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

/// #733: a fleet without `lessons` is weekly: `check` alerts `lessons-missed` once its last lesson run is over 8 days
/// old (a new fleet: its first event), once per run; `off` and a paused fleet never.
#[test]
fn lessons_missed_fires_once_after_8_days_and_never_when_off() {
    let c = ctx(&tmpdir("lesson-missed"), &(CONFIG.to_string() + OFF));
    let sh = FakeShed::attached(&c);
    let r = Fake::default();
    append(
        &c.state,
        &Event::new(OLD - 2 * 86400, "quiet", Some(1), "queued", json!({})),
    )
    .unwrap();
    ev(&c, NOW - 7 * 86400, None, "lesson-skipped", json!({"why": "quiet"}));
    let check = |now: u64| {
        let (code, out) = run(&herdsman::Ctx { now, ..c.clone() }, &r, &["check"]);
        assert_eq!(code, 0, "{out}");
        out.lines()
            .filter(|l| l.contains("lessons-missed"))
            .map(String::from)
            .collect::<Vec<_>>()
    };
    assert!(check(NOW).is_empty(), "7 days: not yet");
    let missed = check(NOW + 2 * 86400);
    assert_eq!(missed.len(), 1, "{missed:?}");
    assert!(
        missed[0].contains("sendmeter") && missed[0].contains("9 days"),
        "{missed:?}"
    );
    assert!(check(NOW + 3 * 86400).is_empty(), "once per last run");
    // A lesson lane resets it.
    ev(
        &c,
        NOW + 3 * 86400,
        Some(80),
        "spawn",
        json!({"role": "lesson", "agent": "sendmeter-lesson-80"}),
    );
    assert!(check(NOW + 10 * 86400).is_empty(), "7 days after the lane");
    assert_eq!(check(NOW + 12 * 86400).len(), 1);
    // Paused: skipped.
    let c = ctx(
        &tmpdir("lesson-missed-paused"),
        &sh.config(&CONFIG.replace("[fleet.sendmeter]\n", "[fleet.sendmeter]\npaused = true\n")),
    );
    ev(&c, OLD - 86400, Some(1), "queued", json!({}));
    let (code, out) = run(&c, &r, &["check"]);
    assert!(code == 0 && !out.contains("lessons-missed"), "{out}");
}

/// #733: rev's PASS of a lesson PR that only edits prose is recorded as a FAIL with the rule; one that adds a check
/// or retires a rules line passes.
#[test]
fn a_prose_only_lesson_pr_is_a_fail() {
    let head = "b".repeat(40);
    let prose = "diff --git a/roles/impl.md b/roles/impl.md\n--- a/roles/impl.md\n+++ b/roles/impl.md\n@@ -1 +1,2 @@\n x\n+- be careful with unwrap\n";
    let check = "diff --git a/.herdsman/rules.md b/.herdsman/rules.md\n--- a/.herdsman/rules.md\n+++ b/.herdsman/rules.md\n@@ -0,0 +1 @@\n+- don't unwrap (enforced by: no-unwrap `! grep -rn unwrap src`, shadow since 1970-01-12)\n";
    let retire = "diff --git a/.herdsman/rules.md b/.herdsman/rules.md\n--- a/.herdsman/rules.md\n+++ b/.herdsman/rules.md\n@@ -1 +0,0 @@\n-- don't X (enforced by: none)\n";
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
            assert!(herdsman::gate::no_proof(body).is_none(), "{body}");
        }
    }
    // Not a lesson issue: a prose PR passes and its diff is never read.
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
