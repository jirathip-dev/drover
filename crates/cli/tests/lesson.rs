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
    let fake = || {
        Fake::default()
            .reply("herdr status", "server:\n  status: running\n")
            .reply("herdr agent list", NO_AGENTS)
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

/// The fake for one spawn, issue `n`'s spec as `title`.
fn spawner(n: u64) -> Fake {
    Fake::default()
        .reply("herdr agent list", NO_AGENTS)
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
        .reply("herdr worktree", r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#)
        .reply(
            "herdr workspace create",
            r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#,
        )
        .reply(
            "herdr pane list",
            r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#,
        )
        .reply("herdr pane run", "{}")
        .reply("herdr agent start", "{}")
        .reply("herdr pane wait-output", "{}")
        .reply("herdr agent prompt", "{}")
        .reply("herdr pane read", "Read the brief")
}

#[test]
fn lesson_files_queues_and_spawns_a_lesson_lane_that_may_close_with_no_change() {
    let c = ctx(&tmpdir("lesson-run"), CONFIG);
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
    let rules = "- don't unwrap config reads (enforced by: clippy unwrap_used)\n";
    for (role, dir) in [("impl", "issue-12"), ("rev", "rev-12-r1")] {
        let wt = c.worktrees.join("sendmeter").join(dir);
        std::fs::create_dir_all(wt.join(".herdsman")).unwrap();
        std::fs::write(wt.join(".herdsman/rules.md"), rules).unwrap();
        // The checkout herdr made: clean.
        let r = spawner(12).once(&format!("git -C {} status --porcelain", wt.display()), 0, "");
        let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", role]);
        assert!(code == 0 || code == 4, "{role}: {code} {out}");
        let brief = std::fs::read_to_string(wt.join(".brief.md")).unwrap();
        let want = format!("## Repo rules (.herdsman/rules.md)\n\n{rules}\n## ");
        assert!(brief.contains(&want), "{role}: {brief}");
    }
    let c = ctx(&tmpdir("lesson-norules"), CONFIG);
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
