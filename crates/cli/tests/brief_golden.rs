//! #34: the lane briefs (impl, impl fix round, #102 impl update round, rev, design) are byte-identical to staging's.
//! Goldens in tests/golden/briefs/ were written by staging's build; `BLESS=1` rewrites them.
//! #82: the fixture fleet is renamed `api` (`acme/api`), so the goldens carry neutral names.
mod common;
use common::*;
use herdsman::events::{append, Event};
use serde_json::json;
use std::path::Path;

const DESIGN: &str = "\n[fleet.api.role.design]\nharness = \"hermes\"\nprofile = \"fleet-design\"\n";

fn fake() -> Fake {
    Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply(
            "gh issue view 12",
            r#"{"title":"Fix the meter","body":"AC: it {works} \\ `ok`"}"#,
        )
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

/// Spawn one lane and return its brief, with the temp dir replaced by `<TMP>`.
fn brief(name: &str, issue: &str, role: &str, fail: bool, update: bool) -> String {
    let d = tmpdir(&format!("brief-{name}"));
    let c = ctx(
        &d,
        &(CONFIG
            .replace("jirathip-dev/sendmeter", "acme/api")
            .replace("sendmeter", "api")
            + DESIGN),
    );
    if fail {
        let body = "FAIL\n\n1. **blocking**: src/x.rs:3 off by {one}.";
        let data = json!({"verdict": "FAIL", "sha": "a".repeat(40), "body": body});
        append(&c.state, &Event::new(10, "api", Some(12), "verdict", data)).unwrap();
    }
    // #420: a `-repro` case's issue is queued `--repro`.
    if name.ends_with("repro") {
        assert_eq!(run(&c, &fake(), &["queue", "api", "add", issue, "--repro"]).0, 0);
    }
    let args = ["spawn", "api", issue, role, "--update"];
    let (code, out) = run(&c, &fake(), &args[..4 + usize::from(update)]);
    assert!(code == 0 || code == 4, "{name}: {code} {out}");
    let path = match role {
        "rev" => c.worktrees.join("api/rev-12-r1/.brief.md"),
        "design" => c.worktrees.join("api/design-12/.brief.md"),
        _ => c.worktrees.join("api/issue-12/.brief.md"),
    };
    std::fs::read_to_string(path)
        .unwrap()
        .replace(d.to_str().unwrap(), "<TMP>")
}

#[test]
fn lane_briefs_match_staging() {
    let cases = [
        ("impl", "12", "impl", false, false),
        ("impl-fix", "12", "impl", true, false),
        ("impl-update", "12", "impl", false, true),
        ("rev", "12", "rev", false, false),
        ("design", "12", "design", false, false),
        ("impl-repro", "12", "impl", false, false),
        ("rev-repro", "12", "rev", false, false),
    ];
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/briefs");
    for (name, issue, role, fail, update) in cases {
        let got = brief(name, issue, role, fail, update);
        let file = dir.join(format!("{name}.md"));
        if std::env::var_os("BLESS").is_some() {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&file, &got).unwrap();
        }
        assert_eq!(
            got,
            std::fs::read_to_string(&file).unwrap(),
            "{name} brief differs from staging's"
        );
    }
}

/// #360: the rev and impl briefs carry the test anti-patterns list; rev asks for the verdict's anti-patterns line.
#[test]
fn briefs_carry_the_test_antipatterns() {
    let (rev, imp) = (
        brief("ap-rev", "12", "rev", false, false),
        brief("ap-impl", "12", "impl", false, false),
    );
    for w in [
        "re-implement production logic instead of exercising it",
        "assert only mock calls or values their own mocks supplied",
        "a fixture that bypasses the real server path",
        "pixel or interaction coverage from static markup",
        "persistence at a path the code never writes",
        "a capability flag instead of delivery or acknowledgement",
        "without hitting the real rejection path",
        "real time or networks instead of fake clocks",
        "Production code added only for tests",
    ] {
        assert!(rev.contains(w) && imp.contains(w), "missing {w}");
    }
    assert!(rev.contains("## Tests that don't count"), "{rev}");
    assert!(
        imp.contains("## Write tests that would fail without your change"),
        "{imp}"
    );
    assert!(rev.contains("`Tests checked against anti-patterns: yes`"), "{rev}");
}

/// #420: a `--repro` issue's impl and rev briefs carry the repro steps; an unmarked issue's don't.
#[test]
fn only_a_repro_issues_briefs_carry_the_repro_steps() {
    let (imp, rev) = (
        brief("r-impl-repro", "12", "impl", false, false),
        brief("r-rev-repro", "12", "rev", false, false),
    );
    assert!(imp.contains("commit 1 holds only the regression test"), "{imp}");
    assert!(rev.contains("`Repro: run`") && rev.contains("`NO_REPRO`"), "{rev}");
    for b in [
        brief("r-impl", "12", "impl", false, false),
        brief("r-rev", "12", "rev", false, false),
    ] {
        assert!(!b.contains("--repro") && !b.contains("Repro:"), "{b}");
    }
}
