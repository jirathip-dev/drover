mod common;
use common::*;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
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

fn shard_dir(lists: &[(&str, &str)]) -> PathBuf {
    let d = tmpdir("test-shards");
    std::fs::create_dir_all(d.join("scripts/test-shards.d")).unwrap();
    std::fs::copy(root().join("scripts/test-shards.sh"), d.join("scripts/test-shards.sh")).unwrap();
    std::fs::create_dir_all(d.join("crates/cli/tests/quick")).unwrap();
    for t in ["slow", "mid", "other", "fresh"] {
        std::fs::write(d.join(format!("crates/cli/tests/{t}.rs")), "").unwrap();
    }
    std::fs::write(d.join("crates/cli/tests/quick/main.rs"), "").unwrap();
    for (name, list) in lists {
        std::fs::write(d.join("scripts/test-shards.d").join(name), list).unwrap();
    }
    d
}

fn placed(d: &Path) -> Vec<(String, String)> {
    let mut all = Vec::new();
    for n in ["1", "2", "3", "4"] {
        let (code, out) = shards(d, &[n]);
        assert_eq!(code, 0, "{out}");
        for t in out
            .replace("--lib --bins", "--test unit")
            .split("--test")
            .map(str::trim)
        {
            if !t.is_empty() {
                all.push((t.to_string(), n.to_string()));
            }
        }
    }
    all.sort();
    all
}

#[test]
fn balance_writes_a_new_list_even_by_nextest_time_and_removes_the_one_before() {
    let d = shard_dir(&[("2000-01-01.txt", "1 unit\n")]);
    std::fs::write(d.join("a.log"), LOG).unwrap();
    std::fs::write(d.join("b.log"), "        PASS [  70.000s] drover::slow a\n").unwrap();

    assert_eq!(shards(&d, &["balance", "a.log", "b.log"]), (0, String::new()));
    let lists: Vec<_> = std::fs::read_dir(d.join("scripts/test-shards.d"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(lists.len(), 1, "{lists:?}");
    assert_ne!(lists[0].file_name().unwrap(), "2000-01-01.txt");
    let list = std::fs::read_to_string(&lists[0]).unwrap();
    assert_eq!(list, "3 fresh\n2 mid\n3 other\n4 quick\n1 slow\n4 unit\n");
    assert_eq!(shards(&d, &["4"]).1, "--test quick --lib --bins ");
    assert_eq!(shards(&d, &["2"]).1, "--test mid ");
}

#[test]
fn every_target_runs_in_exactly_one_shard_whatever_the_newest_list_holds() {
    let d = shard_dir(&[
        ("2000-01-01.txt", "4 slow\n4 mid\n4 other\n4 fresh\n4 quick\n4 unit\n"),
        ("2000-01-02.txt", "2 slow\n3 slow\n9 mid\nx other\n1 gone\n4 unit\n"),
    ]);
    let all = placed(&d);
    let names: Vec<_> = all.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(names, ["fresh", "mid", "other", "quick", "slow", "unit"]);
    let shard = |t: &str| all.iter().find(|(n, _)| n == t).unwrap().1.clone();
    assert_eq!(shard("slow"), "2");
    assert_eq!(shard("unit"), "4");

    for f in ["2000-01-01.txt", "2000-01-02.txt"] {
        std::fs::remove_file(d.join("scripts/test-shards.d").join(f)).unwrap();
    }
    std::fs::write(d.join("crates/cli/tests/new.rs"), "").unwrap();
    let hashed = placed(&d);
    let names: Vec<_> = hashed.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(names, ["fresh", "mid", "new", "other", "quick", "slow", "unit"]);
    for t in ["fresh", "mid", "other", "quick"] {
        assert!(hashed.contains(&(t.to_string(), shard(t))), "{t}: {hashed:?}");
    }
}
