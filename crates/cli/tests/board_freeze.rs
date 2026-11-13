//! #552: the old board takes bug fixes only. `scripts/board-freeze.sh` fails a PR whose changed files touch the
//! board's UI unless its issue's spec has a `board-fix` line; /api/v1 (api.rs, serve/v1.rs) stays open.
mod common;
use common::*;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const FEATURE: &str = "## Issue #9: a new tab\n\nAdd a tab; it mentions a `board-fix` mid-line only.\n";
const FIX: &str = "## Issue #9: the tab flickers\n\nboard-fix: the tab redraws twice\n";
const AMENDED: &str =
    "## Issue #9: the tab flickers\n\n## Amendments\n- **Amendment 1**: board-fix, it redraws twice\n";

fn freeze(spec: &str, files: &str) -> (Option<i32>, String) {
    let d = tmpdir("board-freeze");
    std::fs::write(d.join("spec.md"), spec).unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut c = Command::new("bash")
        .arg(root.join("scripts/board-freeze.sh"))
        .arg(d.join("spec.md"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(files.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    (o.status.code(), String::from_utf8_lossy(&o.stdout).into())
}

#[test]
fn a_board_ui_change_needs_board_fix_in_the_issue() {
    let pr = "README.md\nassets/serve.js\nweb/src/queue.ts\ncrates/server/src/serve/chat.rs\n";
    let (code, out) = freeze(FEATURE, pr);
    assert_eq!(code, Some(1), "{out}");
    assert!(
        out.contains(
            "the old board takes bug fixes only (board-fix in the issue); new UI goes to cooee or Drover's screens (served by drover)"
        ),
        "{out}"
    );
    for f in [
        "  assets/serve.js\n",
        "  web/src/queue.ts\n",
        "  crates/server/src/serve/chat.rs\n",
    ] {
        assert!(out.contains(f), "names {f}: {out}");
    }
    assert!(!out.contains("README.md"), "{out}");
    for f in [
        "assets/serve.css",
        "assets/herd.js",
        "web/dist/app.js",
        "crates/server/src/serve.rs",
        "assets/themes/a.css",
    ] {
        assert_eq!(freeze("", &format!("{f}\n")).0, Some(1), "{f} is frozen");
    }

    assert_eq!(freeze(FIX, pr), (Some(0), String::new()));
    assert_eq!(freeze(AMENDED, pr), (Some(0), String::new()));
}

#[test]
fn api_v1_stays_open_without_board_fix() {
    let pr = "crates/server/src/api.rs\ncrates/server/src/serve/v1.rs\ncrates/core/src/gh.rs\n";
    assert_eq!(freeze(FEATURE, pr), (Some(0), String::new()));
    assert_eq!(freeze("", pr), (Some(0), String::new()));
}

#[test]
fn ci_runs_the_freeze_on_every_staging_pr() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let wf = std::fs::read_to_string(root.join(".github/workflows/board-freeze.yml")).unwrap();
    assert!(wf.contains("branches: [staging]") && wf.contains("synchronize"), "{wf}");
    assert!(wf.contains("| scripts/board-freeze.sh spec.md"), "{wf}");
}

/// #574: the body `drover amend` sent with `gh issue edit` (the call's `--body` value).
fn edited(r: &Fake) -> String {
    let calls = r.calls.borrow();
    let edit = calls
        .iter()
        .rfind(|c| c.starts_with("gh issue edit 60 "))
        .expect("an edit");
    edit.split_once(" --body ").unwrap().1.to_string()
}

/// #574: `drover amend` appends an owner's decision under the body's `## Amendments` (made once), logs `amend`,
/// re-runs the open PR's failed board-freeze (not other checks), and its `board-fix` line passes board-freeze.
#[test]
fn amend_writes_an_owner_decision_into_the_spec_and_reruns_board_freeze() {
    use drover::events::{append, read, Event};
    use serde_json::json;
    let c = ctx(&tmpdir("amend"), CONFIG);
    let ev = |n: u64, kind: &str, d: serde_json::Value| append(&c.state, &Event::new(1, "sendmeter", Some(n), kind, d));
    for (id, n, by) in [
        ("q1", 60, Some("owner")),
        ("q2", 60, Some("default")),
        ("q3", 60, None),
        ("q4", 61, Some("owner")),
        ("q5", 60, Some("orchestrator-t1")),
    ] {
        ev(n, "needs-decision", json!({"id": id, "question": "?"})).unwrap();
        if let Some(by) = by {
            ev(n, "decision", json!({"id": id, "choice": "yes", "by": by})).unwrap();
        }
    }
    for id in ["q3", "q4", "q5", "q9"] {
        let r = Fake::default();
        let (code, out) = run(&c, &r, &["amend", "sendmeter", "60", "--basis", id, "board-fix: x"]);
        assert_eq!(code, 3, "{id}: {out}");
        assert!(out.contains("REFUSED NO_DECISION"), "{id}: {out}");
        assert!(r.calls.borrow().is_empty(), "{id}: no gh call: {:?}", r.calls.borrow());
    }
    let checks = json!({"state": "OPEN", "statusCheckRollup": [
        {"__typename": "CheckRun", "name": "board-freeze", "status": "COMPLETED", "conclusion": "FAILURE",
         "detailsUrl": "https://github.com/o/r/actions/runs/77/job/5"},
        {"__typename": "CheckRun", "name": "rust", "status": "COMPLETED", "conclusion": "FAILURE",
         "detailsUrl": "https://github.com/o/r/actions/runs/88/job/6"}]});
    let r = Fake::default()
        .reply("gh issue view 60", &json!({"body": "Tidy the tab.\n"}).to_string())
        .reply("gh issue edit 60", "")
        .reply("gh pr view issue-60", &checks.to_string())
        .reply("gh run rerun 77", "");
    let text = "board-fix: the tab redraws twice";
    let (code, out) = run(&c, &r, &["amend", "sendmeter", "60", "--basis", "q1", text]);
    assert_eq!(code, 0, "{out}");
    let first = edited(&r);
    assert_eq!(
        first,
        format!("Tidy the tab.\n\n## Amendments\n- **Amendment (q1)**: {text}\n")
    );
    assert!(
        r.called("gh run rerun 77 -R jirathip-dev/sendmeter --failed"),
        "{:?}",
        r.calls.borrow()
    );
    assert!(!r.called("gh run rerun 88"), "only spec-reading checks re-run");
    let amends: Vec<_> = read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "amend")
        .collect();
    assert_eq!(
        (amends[0].issue, amends[0].str("id"), amends[0].str("text")),
        (Some(60), Some("q1"), Some(text))
    );
    assert_eq!(
        freeze(&first, "web/src/queue.ts\n").0,
        Some(0),
        "the amend line passes board-freeze"
    );
    assert_eq!(
        freeze("Tidy the tab.\n", "web/src/queue.ts\n").0,
        Some(1),
        "without it, red"
    );

    let r = Fake::default()
        .reply("gh issue view 60", &json!({"body": first}).to_string())
        .reply("gh issue edit 60", "")
        .reply("gh pr view issue-60", "{}");
    let (code, out) = run(
        &c,
        &r,
        &["amend", "sendmeter", "60", "--basis", "q2", "scope: no new tab"],
    );
    assert_eq!(code, 0, "{out}");
    let second = edited(&r);
    assert_eq!(second, format!("{first}- **Amendment (q2)**: scope: no new tab\n"));
    assert_eq!(second.matches("## Amendments").count(), 1, "{second}");
    assert!(!r.called("gh run rerun"), "no PR, nothing to re-run");
}
