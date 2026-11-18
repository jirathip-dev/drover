mod common;
use common::*;
use std::path::Path;

const RETIRED: &str = include_str!("upgrade/441-accounts-env.toml");

fn fixtures() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/upgrade");
    let mut all: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().into(),
                std::fs::read_to_string(&p).unwrap(),
            )
        })
        .collect();
    all.sort();
    all
}

#[test]
fn parity_r2_a_fleet_toml_from_before_each_retired_key_change_loads() {
    let all = fixtures();
    assert!(all.len() >= 5, "{all:?}");
    for (name, text) in all {
        let c = ctx(&tmpdir("upgrade"), &text);
        let (code, out) = run(&c, &Fake::default(), &["doctor", "--config-only"]);
        assert_eq!(code, 0, "{name}: {out}");
        assert!(out.starts_with("✓ config — ") && !out.contains('✗'), "{name}: {out}");
    }
    let c = ctx(&tmpdir("upgrade-441"), RETIRED);
    let (_, out) = run(&c, &Fake::default(), &["doctor", "--config-only"]);
    let warn = "! config — [accounts] is removed (#923): lanes run on the host's own CLI login; delete [accounts], fleet.drover.role.impl.account";
    assert!(out.lines().any(|l| l == warn), "{out}");
}

#[test]
fn config_only_fails_on_a_config_this_build_cannot_load() {
    let c = ctx(
        &tmpdir("upgrade-bad"),
        &format!("{RETIRED}\n[limits]\neffort = \"bogus\"\n"),
    );
    let (code, out) = run(&c, &Fake::default(), &["doctor", "--config-only"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.starts_with("✗ config — ") && out.contains("limits.effort"), "{out}");
}

#[test]
fn check_alerts_a_retired_key_once_and_the_board_shows_it() {
    let c = ctx(&tmpdir("retired-check"), RETIRED);
    let r = Fake::default();
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("[accounts] is removed (#923): lanes run on the host's own CLI login; delete [accounts], fleet.drover.role.impl.account"), "{out}");
    let log = drover::events::read(&c.state).unwrap();
    assert_eq!(log.iter().filter(|e| e.kind == "config-warning").count(), 1);
    let (_, out) = run(&c, &r, &["check"]);
    assert!(!out.contains("[accounts]"), "once per key: {out}");
    let (code, out) = run(&c, &r, &["board"]);
    assert_eq!(code, 0, "{out}");
    let first = out.lines().next().unwrap_or_default();
    assert_eq!(first, "! [accounts] is retired → drover doctor");
    assert!(first.chars().count() <= 60);
}
