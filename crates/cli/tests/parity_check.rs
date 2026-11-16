mod common;
use common::*;
use std::path::Path;
use std::process::Command;

const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/parity-check.sh");
const HEAD: &str = "| row | feature | test | status |\n|---|---|---|---|\n";

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn commit(dir: &Path, rows: &str, tests: &str) -> String {
    std::fs::create_dir_all(dir.join("docs")).unwrap();
    std::fs::create_dir_all(dir.join("crates/cli/tests")).unwrap();
    std::fs::write(dir.join("docs/parity.md"), format!("# Parity\n\n{HEAD}{rows}")).unwrap();
    std::fs::write(dir.join("crates/cli/tests/queue.rs"), tests).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "x", "--allow-empty"]);
    git(dir, &["rev-parse", "HEAD"])
}

fn check(dir: &Path, base: &str) -> (i32, String) {
    let o = Command::new("bash")
        .arg(SCRIPT)
        .arg(base)
        .current_dir(dir)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    (o.status.code().unwrap_or(-1), out)
}

#[test]
fn deleting_parity_38_fails_unless_its_row_is_dropped_with_an_owner_decision() {
    let d = tmpdir("parity-check");
    git(&d, &["init", "-q"]);
    let kept = "| 38 | Blocked chains as a tree | parity_38 | kept |\n| 39 | +N more | parity_38 | open #1089 |\n";
    let base = commit(&d, kept, "#[test]\nfn parity_38_chains_fold() {}\n");
    assert_eq!(check(&d, &base), (0, "parity: 2 rows, 1 tests, all listed\n".into()));

    commit(&d, kept, "");
    let (code, out) = check(&d, &base);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("row 38 is kept but its test parity_38 is gone"), "{out}");

    let open = "| 38 | Blocked chains as a tree | parity_38 | open |\n| 39 | +N more | parity_38 | open #1089 |\n";
    commit(&d, open, "");
    let (code, out) = check(&d, &base);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("parity_38 is deleted, but docs/parity.md has no `dropped q<N>` row"),
        "{out}"
    );
    assert_eq!(check(&d, "").0, 0, "without a base only the tree is checked");

    let no_q =
        "| 38 | Blocked chains as a tree | parity_38 | dropped |\n| 39 | +N more | parity_38 | dropped q1726 |\n";
    commit(&d, no_q, "");
    let (code, out) = check(&d, &base);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("row 38: status \"dropped\" is none of"), "{out}");

    let dropped =
        "| 38 | Blocked chains as a tree | parity_38 | dropped q1726 |\n| 39 | +N more | parity_38 | dropped q1726 |\n";
    commit(&d, dropped, "");
    assert_eq!(check(&d, &base), (0, "parity: 2 rows, 0 tests, all listed\n".into()));

    commit(&d, "| 39 | +N more | parity_38 | dropped q1726 |\n", "");
    let (code, out) = check(&d, &base);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("row 38 is gone from docs/parity.md"), "{out}");
}

#[test]
fn a_parity_test_in_no_row_fails() {
    let d = tmpdir("parity-unlisted");
    git(&d, &["init", "-q"]);
    commit(&d, "| 38 | chains | — | open |\n", "fn parity_r25_no_flash() {}\n");
    let (code, out) = check(&d, "");
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("parity_r25 is in no row of docs/parity.md"), "{out}");
}

#[test]
fn this_repos_parity_list_names_every_parity_test() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (code, out) = check(&root, "");
    assert_eq!(code, 0, "{out}");
}
