mod common;
use common::*;
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn gated(changed: &str) -> String {
    let mut c = Command::new("bash")
        .arg(root().join("scripts/ci-changes.sh"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(changed.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success());
    String::from_utf8(o.stdout).unwrap()
}

#[test]
fn release_tests_run_on_a_pr_only_for_the_paths_they_cover() {
    for path in [
        "crates/core/src/drive.rs",
        "crates/cli/tests/perf.rs",
        "crates/cli/tests/screen_hub.rs",
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "assets/sw.js",
        "demo/events.jsonl",
        "web/dist/board.js",
    ] {
        let out = gated(&format!("docs/ci.md\n{path}\n"));
        assert!(out.contains("release=true\n"), "{path}: {out}");
    }
    for path in [
        "docs/ci.md",
        "roles/impl.md",
        ".github/workflows/ci.yml",
        "README.md",
        "scripts/ui-audit.sh",
        "templates/ci/ci.yml",
        "x/crates/a.rs",
    ] {
        assert_eq!(gated(&format!("{path}\n")), "webkit=false\nrelease=false\n", "{path}");
    }
    assert_eq!(gated("web/src/board.ts\n"), "webkit=true\nrelease=false\n");
}

fn ci() -> Value {
    let o = Command::new("ruby")
        .args([
            "-rjson",
            "-ryaml",
            "-e",
            "puts JSON.generate(YAML.safe_load(File.read(ARGV[0])))",
        ])
        .arg(root().join(".github/workflows/ci.yml"))
        .output()
        .expect("ruby on PATH");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    serde_json::from_slice(&o.stdout).unwrap()
}

fn runs(job: &Value) -> String {
    job["steps"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["run"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_rust_check_waits_on_lint_and_release_tests_which_every_push_runs() {
    let w = ci();
    let jobs = &w["jobs"];
    let release = &jobs["release-tests"];
    assert_eq!(release["needs"], "changes");
    assert_eq!(
        release["if"],
        "github.event_name == 'push' || needs.changes.outputs.release == 'true'"
    );
    assert_eq!(
        jobs["changes"]["outputs"]["release"],
        "${{ steps.diff.outputs.release }}"
    );
    assert!(runs(&jobs["changes"]).contains("scripts/ci-changes.sh < changed.txt"));
    let r = runs(release);
    for step in [
        "cargo build --release --locked",
        "--test perf",
        "--test screen_hub",
        "--test soak",
        "./target/release/drover serve --export site --demo",
        "site/",
    ] {
        assert!(r.contains(step), "release-tests lacks {step}");
    }
    let lint = runs(&jobs["lint"]);
    assert!(!lint.contains("target/release"), "{lint}");
    for step in [
        "cargo fmt --check",
        "cargo clippy",
        "scripts/test-shards.sh check",
        "cargo deny --locked check",
        "drover cap-check",
        "drover arch-check",
        "drover lesson-check",
        "docs/ demo/",
    ] {
        assert!(lint.contains(step), "lint lacks {step}");
    }
    let rust = &jobs["rust"];
    assert_eq!(rust["needs"], serde_json::json!(["changes", "lint", "release-tests"]));
    assert_eq!(rust["if"], "always()");
}

fn shards(dir: &Path, args: &[&str]) -> (i32, String) {
    let o = Command::new("bash")
        .arg(dir.join("scripts/test-shards.sh"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    (o.status.code().unwrap_or(-1), out)
}

const LOG: &str = "test (shard 1/4)\tUNKNOWN STEP\t2026Z ^[[32;1m        PASS^[[0m [  90.000s] (  1/9) ^[[35;1mdrover::slow^[[0m ^[[34;1ma^[[0m\n\
\x1b[32;1m        PASS\x1b[0m [  30.000s] (  2/9) drover::mid a\n\
        PASS [  30.000s] (3/9) drover::mid b\n\
        PASS [  50.000s] drover::other a\n\
        PASS [  20.000s] drover-core config::tests::a\n\
        PASS [  20.000s] drover::bin/drover tests::b\n\
        FAIL [ 500.000s] drover::quick a\n\
        SLOW [> 60.000s] drover::quick b\n\
        PASS [  10.000s] drover::quick c\n";

#[test]
fn balance_writes_shards_even_by_nextest_time_and_check_guards_the_partition() {
    let d = tmpdir("test-shards");
    std::fs::create_dir_all(d.join("scripts")).unwrap();
    std::fs::copy(root().join("scripts/test-shards.sh"), d.join("scripts/test-shards.sh")).unwrap();
    std::fs::create_dir_all(d.join("crates/cli/tests/quick")).unwrap();
    for t in ["slow", "mid", "other", "fresh"] {
        std::fs::write(d.join(format!("crates/cli/tests/{t}.rs")), "").unwrap();
    }
    std::fs::write(d.join("crates/cli/tests/quick/main.rs"), "").unwrap();
    std::fs::write(d.join("scripts/test-shards.txt"), "1 unit\n").unwrap();
    std::fs::write(d.join("a.log"), LOG).unwrap();
    std::fs::write(d.join("b.log"), "        PASS [  70.000s] drover::slow a\n").unwrap();

    assert_eq!(shards(&d, &["balance", "a.log", "b.log"]), (0, String::new()));
    let list = std::fs::read_to_string(d.join("scripts/test-shards.txt")).unwrap();
    assert_eq!(list, "3 fresh\n2 mid\n3 other\n4 quick\n1 slow\n4 unit\n");
    assert_eq!(shards(&d, &["4"]).1, "--test quick --lib --bins ");
    assert_eq!(shards(&d, &["2"]).1, "--test mid ");

    std::fs::write(d.join("crates/cli/tests/new.rs"), "").unwrap();
    std::fs::write(d.join("scripts/test-shards.txt"), format!("{list}2 slow\n")).unwrap();
    let (code, out) = shards(&d, &["check"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("in two shards: slow"), "{out}");
    assert!(out.contains("in no shard: new"), "{out}");
    std::fs::remove_file(d.join("crates/cli/tests/fresh.rs")).unwrap();
    assert!(shards(&d, &["1"]).1.contains("no such test target: fresh"));
}
