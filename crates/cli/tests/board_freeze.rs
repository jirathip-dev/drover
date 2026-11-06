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
            "the old board takes bug fixes only (board-fix in the issue); new UI goes to cooee or the drover plugins"
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
