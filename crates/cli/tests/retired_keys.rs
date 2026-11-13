//! #539: a retired config key is a warning, never a load error. Each `upgrade/*.toml` is a fleet.toml from before a
//! retired-key change; this build loads it. `check` and the board warn; the install gate refuses a binary that can't.
mod common;
use common::*;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

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
fn a_fleet_toml_from_before_each_retired_key_change_loads() {
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
    let warn = "! config — [accounts].accounts_env is retired (#441): drop the line (#683: accounts sign in with their own login)";
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
    assert!(out.contains("[accounts].accounts_env is retired (#441)"), "{out}");
    let log = drover::events::read(&c.state).unwrap();
    assert_eq!(log.iter().filter(|e| e.kind == "config-warning").count(), 1);
    let (_, out) = run(&c, &r, &["check"]);
    assert!(!out.contains("accounts_env"), "once per key: {out}");
    let (code, out) = run(&c, &r, &["board"]);
    assert_eq!(code, 0, "{out}");
    let first = out.lines().next().unwrap_or_default();
    assert_eq!(first, "! [accounts].accounts_env is retired → drover doctor");
    assert!(first.chars().count() <= 60);
}

#[test]
fn install_gate_keeps_the_old_binary_when_the_new_one_cannot_load_the_config() {
    let d = tmpdir("gate");
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/install-gate.sh");
    let new = env!("CARGO_BIN_EXE_drover");
    let dest = d.join("bin/drover");
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    std::fs::write(&dest, "old").unwrap();
    let fake = d.join("fake");
    std::fs::create_dir_all(&fake).unwrap();
    let log = d.join("codesign.log");
    let sh = format!(
        "#!/bin/sh\n[ -n \"$HANG\" ] && exec sleep 30\necho \"$@\" >> '{}'\n",
        log.display()
    );
    let sec = "#!/bin/sh\n[ -n \"$NOID\" ] && exit 44\necho 'SHA-256 hash: 00'\necho 'SHA-1 hash: AC01B47A'\n";
    for (name, body) in [("codesign", sh.as_str()), ("security", sec)] {
        std::fs::write(fake.join(name), body).unwrap();
        std::fs::set_permissions(fake.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = format!("{}:{}", fake.display(), std::env::var("PATH").unwrap());
    let gate = |config: &str, env: &str| {
        let cfg = d.join("fleet.toml");
        std::fs::write(&cfg, config).unwrap();
        Command::new("sh")
            .arg(&script)
            .args([Path::new(new), &dest])
            .env("FLEET_CONFIG", &cfg)
            .env("FLEET_STATE", d.join("state/events.jsonl"))
            .env("PATH", &path)
            .env("HOME", &d)
            .env("DROVER_SIGN_TIMEOUT", "1")
            .env(env, "1")
            .output()
            .unwrap()
    };
    let o = gate("[limits]\neffort = \"bogus\"\n", "X");
    let out = String::from_utf8_lossy(&o.stdout);
    assert_eq!(o.status.code(), Some(1), "{out}");
    assert!(out.contains("kept ") && out.contains("✗ config — "), "{out}");
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old");
    assert!(!log.exists(), "a refused build is never signed");

    for (env, fix) in [
        ("NOID", "fix: sh scripts/signing-identity.sh once"),
        ("HANG", "fix: security set-key-partition-list"),
    ] {
        let o = gate(RETIRED, env);
        let out = String::from_utf8_lossy(&o.stdout);
        assert_eq!(o.status.code(), Some(1), "{env}: {out}");
        assert!(out.contains("not signed; kept ") && out.contains(fix), "{env}: {out}");
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "old");
        assert!(!dest.with_file_name("drover.new").exists());
    }

    let o = gate(RETIRED, "X");
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stdout));
    assert_eq!(std::fs::read(&dest).unwrap(), std::fs::read(new).unwrap());
    assert!(!dest.with_file_name("drover.new").exists());
    let signed = format!(
        "--force --sign AC01B47A --keychain {}/Library/Keychains/login.keychain-db --identifier drover {}.new\n",
        d.display(),
        dest.display()
    );
    assert_eq!(std::fs::read_to_string(&log).unwrap(), signed);
}
