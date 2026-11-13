//! #734: issues queued `--perf` or `--refactor`: rev's PASS needs a `baseline` first commit and a `Perf:` block
//! (`NO_BASELINE`), or a test diff that only moves or renames tests (`TESTS_CHANGED`).
mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::json;

const BRANCH: &str = "gh api repos/jirathip-dev/sendmeter/branches/issue-12";
const COMMITS: &str = "gh pr view issue-12 -R jirathip-dev/sendmeter --json commits";
const DIFF: &str = "gh pr diff issue-12 -R jirathip-dev/sendmeter";
const PERF: &str = "Perf:\n```\ncargo bench totals\n```\nbefore 41 ms, after 12 ms (3.4x), 5 runs each, spread 2%\n";

/// #12 built and under rev, queued with `flag`.
fn setup(name: &str, flag: &str) -> (drover::Ctx, std::path::PathBuf) {
    let d = tmpdir(name);
    let c = ctx(&d, CONFIG);
    let (code, out) = run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", flag]);
    assert_eq!(code, 0, "{out}");
    for (ts, role) in [(1, "impl"), (2, "rev")] {
        let e = json!({"role": role, "stage": role, "check": role == "rev", "agent": format!("sendmeter-{role}-12"),
            "branch": "issue-12"});
        append(&c.state, &Event::new(ts, "sendmeter", Some(12), "spawn", e)).unwrap();
    }
    (c, d)
}

fn pass(c: &drover::Ctx, d: &std::path::Path, r: &Fake, body: &str) -> (i32, String, usize) {
    let file = d.join("verdict.md");
    std::fs::write(&file, format!("PASS\n\nAll ACs hold.\n\n{body}")).unwrap();
    let sha = "a".repeat(40);
    let (code, out) = run(
        c,
        r,
        &[
            "verdict",
            "sendmeter",
            "12",
            "PASS",
            "--sha",
            &sha,
            "--file",
            &file.to_string_lossy(),
        ],
    );
    let n = read(&c.state).unwrap().iter().filter(|e| e.kind == "verdict").count();
    (code, out, n)
}

#[test]
fn a_perf_pass_needs_a_baseline_commit_and_a_perf_block() {
    let (c, d) = setup("perf", "--perf");
    let (code, out) = run(&c, &Fake::default(), &["queue", "sendmeter", "ls"]);
    assert!(code == 0 && out.contains("perf"), "{out}");
    let sha = "a".repeat(40);
    let fake = |first: &str| Fake::default().reply(BRANCH, &sha).reply(COMMITS, first);
    let (code, out, n) = pass(&c, &d, &fake("speed up totals\n"), PERF);
    assert_eq!((code, n), (3, 0), "{out}");
    assert!(
        out.contains("REFUSED NO_BASELINE: the PR's first commit records no baseline"),
        "{out}"
    );
    let baseline = "baseline: cargo bench totals\nmedian 41 ms over 5 runs\n";
    let (code, out, n) = pass(&c, &d, &fake(baseline), "");
    assert_eq!((code, n), (3, 0), "{out}");
    assert!(out.contains("REFUSED NO_BASELINE: no `Perf:` block"), "{out}");
    let (code, out, n) = pass(&c, &d, &fake(baseline), PERF);
    assert_eq!((code, n), (0, 1), "{out}");
}

const MOVED: &str = "diff --git a/tests/a.rs b/tests/a.rs\n--- a/tests/a.rs\n+++ b/tests/a.rs\n@@ -1,4 +1,1 @@\n \
use x;\n-#[test]\n-fn totals() {\n-    assert_eq!(total(), 3);\n-}\ndiff --git a/tests/b.rs b/tests/b.rs\n--- a/tests/b.rs\n\
+++ b/tests/b.rs\n@@ -1,1 +1,4 @@\n use x;\n+    #[test]\n+    fn totals() {\n+        assert_eq!(total(), 3);\n+    }\n\
diff --git a/tests/old.rs b/tests/new.rs\nsimilarity index 100%\nrename from tests/old.rs\nrename to tests/new.rs\n\
diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,1 +1,1 @@\n-fn total() -> u32 { 1 + 2 }\n\
+fn total() -> u32 {\n+    3\n+}\n";

#[test]
fn a_refactor_pass_needs_tests_only_moved_or_renamed() {
    let (c, d) = setup("refactor", "--refactor");
    let sha = "a".repeat(40);
    let fake = |diff: &str| Fake::default().reply(BRANCH, &sha).reply(DIFF, diff);
    let edited = MOVED.replace("+        assert_eq!(total(), 3);", "+        assert_eq!(total(), 4);");
    let (code, out, n) = pass(&c, &d, &fake(&edited), "");
    assert_eq!((code, n), (3, 0), "{out}");
    let want = "REFUSED TESTS_CHANGED: a `--refactor` PR edits tests beyond moves and renames: tests/a.rs, tests/b.rs";
    assert!(out.contains(want), "{out}");
    let (code, out, n) = pass(&c, &d, &fake(MOVED), "");
    assert_eq!((code, n), (0, 1), "{out}");
}

#[test]
fn an_unmarked_issues_pass_reads_neither_commits_nor_diff() {
    let (c, d) = setup("plain", "--repro");
    let sha = "a".repeat(40);
    let r = Fake::default().reply(BRANCH, &sha);
    let run_block = "Repro: run\n```\ncargo test t\n```\n```\ntest t ... FAILED\n```\n```\ntest t ... ok\n```\n";
    let (code, out, n) = pass(&c, &d, &r, run_block);
    assert_eq!((code, n), (0, 1), "{out}");
    assert!(!r
        .calls
        .borrow()
        .iter()
        .any(|c| c.contains("commits") || c.contains("pr diff")));
}
