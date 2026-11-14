mod common;
use common::tmpdir;
use drover::events::Event;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const CONFIG: &str = "[fleet.s]\nrepo = \"a/s\"\nlocal = \"/tmp/x\"\nintegration = \"staging\"\n";

fn run_in(home: &Path, args: &[&str], envs: &[(&str, &str)], stdin: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_drover"))
        .args(args)
        .env("HOME", home)
        .env("DROVER_HOME", home)
        .env_remove("FLEET_CONFIG")
        .env_remove("FLEET_STATE")
        .env_remove("DROVER_LANE")
        .env_remove("FLEET_LANE")
        .envs(envs.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = child.wait_with_output().unwrap();
    (
        o.status.code().unwrap(),
        String::from_utf8(o.stdout).unwrap(),
        String::from_utf8(o.stderr).unwrap(),
    )
}

fn run(home: &Path, args: &[&str]) -> (i32, String, String) {
    run_in(home, args, &[], "")
}

fn seed(home: &Path, name: &str) {
    let (cfg, state) = (home.join(".config").join(name), home.join(".local/state").join(name));
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(cfg.join("fleet.toml"), CONFIG).unwrap();
    let ev = Event::new(1, "s", Some(12), "merged", serde_json::json!({"sha": "abc"}));
    std::fs::write(state.join("events.jsonl"), serde_json::to_string(&ev).unwrap() + "\n").unwrap();
}

fn seed_db(home: &Path, name: &str) {
    seed(home, name);
    let state = home.join(".local/state").join(name).join("events.jsonl");
    drover::db::migrate(&state, &mut std::io::sink()).unwrap();
}

fn snapshot(dir: &Path) -> Vec<(PathBuf, std::time::SystemTime, Vec<u8>)> {
    let md = std::fs::symlink_metadata(dir).unwrap();
    let body = if md.is_file() {
        std::fs::read(dir).unwrap()
    } else {
        vec![]
    };
    let mut v = vec![(dir.to_path_buf(), md.modified().unwrap(), body)];
    if md.is_dir() {
        let mut kids: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
        kids.sort();
        for k in kids {
            v.extend(snapshot(&k));
        }
    }
    v
}

fn moved(h: &Path) -> bool {
    h.join(".config/drover").exists() || h.join(".local/state/drover").exists()
}

#[test]
fn version_and_help_touch_nothing() {
    let h = tmpdir("migrate-pure");
    seed(&h, "herdsman");
    let before = snapshot(&h);
    for args in [&["--version"][..], &["--help"], &["status", "--help"]] {
        let (code, out, err) = run(&h, args);
        assert_eq!(code, 0, "{args:?}: {err}");
        assert!(!out.is_empty() && err.is_empty(), "{args:?}: {out} / {err}");
    }
    assert_eq!(snapshot(&h), before, "home byte- and mtime-identical");
    assert!(!moved(&h));
}

#[test]
fn status_refuses_until_migrated_and_moves_nothing() {
    let h = tmpdir("migrate-fallback");
    seed_db(&h, "herdsman");
    let before = snapshot(&h.join(".config"));
    let (code, out, err) = run(&h, &["status"]);
    let old = [".config/herdsman", ".local/state/herdsman"].map(|d| h.join(d).display().to_string());
    let line = format!(
        "drover: {} is from before the rename: run `drover migrate` first\n",
        old.join(", ")
    );
    assert_eq!((code, out.as_str(), err), (1, "", line));
    assert_eq!(snapshot(&h.join(".config")), before, "nothing moved");
    assert!(!moved(&h));
}

#[test]
fn status_with_neither_home_is_an_empty_board_without_hint() {
    let h = tmpdir("migrate-none");
    let (code, out, err) = run(&h, &["status"]);
    assert_eq!((code, out.trim(), err.as_str()), (0, "no tracked issues", ""));
    assert!(!h.join(".config").exists() && !h.join(".local").exists());
}

#[test]
fn with_both_trees_the_new_one_is_read_and_migrate_moves_nothing() {
    let h = tmpdir("migrate-both");
    seed_db(&h, "herdsman");
    seed_db(&h, "drover");
    std::fs::write(h.join(".config/herdsman/fleet.toml"), "# old\n").unwrap();
    let (code, _, err) = run(&h, &["status"]);
    assert_eq!((code, err.as_str()), (0, ""));
    let before = snapshot(&h);
    let (code, out, err) = run(&h, &["migrate", "--live"]);
    assert_eq!((code, out.as_str()), (1, ""), "{err}");
    let both = [".config/herdsman", ".config/drover"].map(|d| h.join(d).display().to_string());
    let line = format!("both {} and {} exist: nothing moved; keep one", both[0], both[1]);
    assert!(err.contains(&line), "{err}");
    assert_eq!(snapshot(&h), before, "nothing touched");
}

#[test]
fn migrate_moves_both_once() {
    let h = tmpdir("migrate-move");
    seed(&h, "herdsman");
    let (code, out, err) = run(&h, &["migrate", "--live"]);
    assert_eq!(code, 0, "{err}");
    for d in [".config", ".local/state"] {
        let line = format!(
            "moved {} -> {}",
            h.join(d).join("herdsman").display(),
            h.join(d).join("drover").display()
        );
        assert!(out.contains(&line), "{out}");
    }
    for d in [".config", ".local/state"] {
        let link = std::fs::read_link(h.join(d).join("herdsman")).unwrap();
        assert_eq!(link, h.join(d).join("drover"), "the old name is a link to the new tree");
    }
    assert_eq!(
        std::fs::read_to_string(h.join(".config/drover/fleet.toml")).unwrap(),
        CONFIG
    );
    let st = h.join(".local/state/drover");
    assert!(st.join("state.db").exists() && st.join("events.jsonl.migrated").exists());
    let before = snapshot(&h.join(".config"));
    let (code, again, err) = run(&h, &["migrate", "--live"]);
    assert_eq!(code, 0, "{err}");
    assert!(!again.contains("moved") && again.contains("nothing to move"), "{again}");
    assert!(
        again.contains("imported 0 log file(s)") && again.contains("holds 1 events"),
        "{again}"
    );
    assert_eq!(snapshot(&h.join(".config")), before);
    let (code, out, err) = run(&h, &["status"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(out.contains("merged"), "{out}");
}

#[test]
fn migrate_with_nothing_to_move_says_so() {
    let h = tmpdir("migrate-empty");
    let (code, out, err) = run(&h, &["migrate", "--live"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.matches("nothing to move").count(), 3, "{out}");
    assert!(!h.join(".config").exists());
}

#[test]
fn failed_state_move_rolls_the_config_move_back() {
    use std::os::unix::fs::PermissionsExt;
    let h = tmpdir("migrate-rollback");
    seed(&h, "herdsman");
    let parent = h.join(".local/state");
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o555)).unwrap();
    let (code, out, err) = run(&h, &["migrate", "--live"]);
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_ne!(code, 0, "{out}");
    let (old, new) = (parent.join("herdsman"), parent.join("drover"));
    assert!(
        err.contains(&format!("moving {} -> {} failed", old.display(), new.display())),
        "{err}"
    );
    let (cold, cnew) = (h.join(".config/herdsman"), h.join(".config/drover"));
    assert!(
        err.contains(&format!("rolled back {} -> {}", cnew.display(), cold.display())),
        "{err}"
    );
    assert_eq!(
        std::fs::read_to_string(cold.join("fleet.toml")).unwrap(),
        CONFIG,
        "config back in place"
    );
    assert!(!cnew.exists() && !new.exists());
    assert!(old.join("events.jsonl").exists());
}

#[test]
fn symlinked_old_dir_migrates_as_a_link() {
    let h = tmpdir("migrate-link");
    seed(&h, "real");
    std::os::unix::fs::symlink(h.join(".config/real"), h.join(".config/herdsman")).unwrap();
    std::os::unix::fs::symlink(h.join(".local/state/real"), h.join(".local/state/herdsman")).unwrap();
    let (code, _, err) = run(&h, &["migrate", "--live"]);
    assert_eq!(code, 0, "{err}");
    let (code, out, err) = run(&h, &["status"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(out.contains("merged"), "{out}");
    assert_eq!(
        std::fs::read_to_string(h.join(".config/drover/fleet.toml")).unwrap(),
        CONFIG
    );
    assert!(h.join(".config/real/fleet.toml").exists(), "link target untouched");
}

#[test]
fn hook_with_pre_rename_home_prints_only_its_json_decision() {
    let cmd = ["gh", "pr", "merge", "5"].join(" ");
    let input = serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "Bash",
        "cwd": std::env::temp_dir(), "tool_input": {"command": cmd}});
    let lane = [("DROVER_LANE", "s:12:impl")];
    let decide = |h: &Path| run_in(h, &["hook", "--role", "impl"], &lane, &input.to_string());
    let clean = tmpdir("migrate-hook-clean");
    seed(&clean, "drover");
    let (code, want, _) = decide(&clean);
    assert_eq!(code, 0);
    let h = tmpdir("migrate-hook");
    seed(&h, "herdsman");
    let before = snapshot(&h);
    let (code, out, err) = decide(&h);
    assert_eq!((code, err.as_str()), (0, ""), "no stderr at all");
    assert_eq!(out, want, "decision unchanged");
    let v: serde_json::Value = serde_json::from_str(out.trim()).expect("stdout is exactly one JSON value");
    assert!(v.to_string().contains("deny"), "{v}");
    assert_eq!(snapshot(&h), before, "hook never migrates");
}

#[test]
fn the_old_lane_variable_alone_still_arms_the_hook() {
    let cmd = ["gh", "pr", "merge", "5"].join(" ");
    let input = serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "Bash",
        "cwd": std::env::temp_dir(), "tool_input": {"command": cmd}});
    let h = tmpdir("migrate-hook-old-var");
    seed(&h, "drover");
    let decide = |var: &str| {
        run_in(
            &h,
            &["hook", "--role", "impl"],
            &[(var, "s:12:impl")],
            &input.to_string(),
        )
    };
    let (code, new, _) = decide("DROVER_LANE");
    assert!(code == 0 && new.contains("deny"), "{new}");
    assert_eq!(decide("HERDSMAN_LANE"), (0, new, String::new()));
}

#[test]
fn run_as_herdsman_it_works_and_prints_the_rename_notice() {
    let h = tmpdir("migrate-alias");
    seed_db(&h, "drover");
    let alias = h.join("herdsman");
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_drover"), &alias).unwrap();
    let o = Command::new(&alias)
        .arg("status")
        .env("HOME", &h)
        .env("DROVER_HOME", &h)
        .env_remove("FLEET_CONFIG")
        .env_remove("FLEET_STATE")
        .output()
        .unwrap();
    let (out, err) = (
        String::from_utf8(o.stdout).unwrap(),
        String::from_utf8(o.stderr).unwrap(),
    );
    assert!(o.status.success() && out.contains("merged"), "{out} / {err}");
    assert_eq!(err, "herdsman is now drover: the old name works for one more release\n");
}

const EMPTY: [(&str, &str); 4] = [
    ("FLEET_CONFIG", ""),
    ("FLEET_STATE", ""),
    ("FLEET_WORKTREES", ""),
    ("FLEET_HERMES_PROFILES", ""),
];

#[test]
fn empty_path_vars_resolve_to_the_defaults() {
    let h = tmpdir("migrate-empty-vars");
    seed_db(&h, "drover");
    for home in ["", h.to_str().unwrap()] {
        let envs: Vec<_> = EMPTY.iter().copied().chain([("DROVER_HOME", home)]).collect();
        let (code, out, err) = run_in(&h, &["status"], &envs, "");
        assert_eq!((code, err.as_str()), (0, ""), "DROVER_HOME={home:?}");
        assert!(
            out.contains("12") && out.contains("merged"),
            "real history, not an empty board: {out}"
        );
    }
    let old = tmpdir("migrate-empty-vars-old");
    seed_db(&old, "herdsman");
    let envs: Vec<_> = EMPTY.iter().copied().chain([("DROVER_HOME", "")]).collect();
    let (code, out, err) = run_in(&old, &["status"], &envs, "");
    assert_eq!((code, out.as_str()), (1, ""), "{err}");
    assert!(err.contains("run `drover migrate` first"), "{err}");
}

#[test]
fn migrate_refuses_an_empty_drover_home_and_leaves_the_cwd_alone() {
    let (h, cwd) = (tmpdir("migrate-empty-home"), tmpdir("migrate-empty-cwd"));
    seed(&h, "herdsman");
    seed(&cwd, "herdsman");
    let (bh, bc) = (snapshot(&h), snapshot(&cwd));
    let o = Command::new(env!("CARGO_BIN_EXE_drover"))
        .arg("migrate")
        .current_dir(&cwd)
        .env("HOME", &h)
        .env("DROVER_HOME", "")
        .envs(EMPTY)
        .env_remove("DROVER_LANE")
        .env_remove("FLEET_LANE")
        .output()
        .unwrap();
    let err = String::from_utf8(o.stderr).unwrap();
    assert_ne!(o.status.code(), Some(0), "{err}");
    assert!(err.contains("DROVER_HOME is set but empty"), "{err}");
    assert!(o.stdout.is_empty(), "nothing moved or reported");
    assert_eq!(
        (snapshot(&h), snapshot(&cwd)),
        (bh, bc),
        "neither $HOME nor ./.config touched"
    );
}

#[test]
fn the_live_state_dir_is_migrated_only_with_live() {
    let h = tmpdir("migrate-live");
    seed(&h, "drover");
    let st = h.join(".local/state/drover");
    let before = snapshot(&st);
    for args in [&["status"][..], &["migrate"]] {
        let (code, _, err) = run(&h, args);
        assert_ne!(code, 0, "{args:?} refuses");
        assert!(err.contains("migrate --live"), "{args:?}: {err}");
    }
    assert_eq!(snapshot(&st), before, "the log stays, no database made");
    let (code, out, err) = run(&h, &["migrate", "--live"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.contains("imported 1 log file(s)") && out.contains("holds 1 events"),
        "{out}"
    );
    assert!(st.join("state.db").exists() && st.join("events.jsonl.migrated").exists());
    let (code, out, err) = run(&h, &["status"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(out.contains("merged"), "{out}");
}
