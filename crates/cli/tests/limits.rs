mod common;
use common::*;
use serde_json::json;
use std::path::Path;
use std::process::Command;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn remote_plan(name: &str, limits: &str) -> String {
    let conf = CONFIG.replace("[limits]\n", &format!("[limits]\n{limits}"));
    let c = ctx(
        &tmpdir(name),
        &format!("{conf}\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n"),
    );
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": 1, "seen": 1_000_000, "up": true},
        "bazzite": {"tags": ["os:linux"], "home": "/home/me", "probed": 1, "seen": 1_000_000, "up": true,
                    "load": 0.1, "mem": 12.0, "disk": 200.0},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    let r = Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("gh issue view 12", r#"{"title":"Fix the meter","body":"AC: it works"}"#);
    let args = ["spawn", "sendmeter", "12", "impl", "--host", "bazzite", "--dry-run"];
    let (code, out) = run(&c, &r, &args);
    assert_eq!(code, 0, "{out}");
    out
}

#[test]
fn ssh_connect_secs_sets_the_connect_timeout_a_spawn_plan_prints() {
    let plain = remote_plan("limits-plain", "");
    assert!(plain.contains("ssh -oBatchMode=yes -oConnectTimeout=10 "), "{plain}");
    let set = remote_plan("limits-set", "ssh_connect_secs = 42\n");
    assert!(set.contains("ssh -oBatchMode=yes -oConnectTimeout=42 "), "{set}");
    assert!(!set.contains("-oConnectTimeout=10"), "{set}");
}

#[test]
fn config_show_lists_every_limit_and_docs_table_matches() {
    let c = ctx(
        &tmpdir("limits-show"),
        &CONFIG.replace("[limits]\n", "[limits]\ndb_busy_secs = 7\n"),
    );
    let (code, out) = run(&c, &Fake::default(), &["config", "show"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("db_busy_secs = 7  # the SQLite busy timeout\n"), "{out}");
    let fields = toml::Table::try_from(drover::config::Limits::default()).unwrap();
    let shown: Vec<&str> = out.lines().filter_map(|l| l.split_once(" = ").map(|p| p.0)).collect();
    for k in fields.keys() {
        assert!(shown.contains(&k.as_str()), "`config show` lacks {k}: {out}");
    }
    assert_eq!(shown.len(), drover::config::LIMIT_DOCS.len());
    let docs = std::fs::read_to_string(root().join("docs/config.md")).unwrap();
    let c = ctx(&tmpdir("limits-defaults"), "");
    let (_, defaults) = run(&c, &Fake::default(), &["config", "show"]);
    for l in defaults.lines() {
        let (k, rest) = l.split_once(" = ").unwrap();
        let (v, doc) = rest.split_once("  # ").unwrap();
        let v = if k == "deny_commands" {
            "assets/deny_commands.toml"
        } else {
            v
        };
        let row = format!("| `{k}` | `{v}` | {doc} |");
        assert!(docs.contains(&row), "docs/config.md lacks the row {row}");
    }
}

fn timing_check(files: &[(&str, &str)]) -> (Option<i32>, String) {
    let d = tmpdir("timing-check");
    for (p, text) in files {
        let p = d.join("crates/core/src").join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    let o = Command::new("bash")
        .arg(root().join("scripts/timing-check.sh"))
        .arg(&d)
        .output()
        .unwrap();
    (o.status.code(), String::from_utf8_lossy(&o.stdout).into())
}

const TESTS: &str = "pub fn f() {}\n\n#[cfg(test)]\nmod tests {\n    fn t() {\n        let _ = \
                     std::time::Duration::from_millis(20);\n    }\n}\n";

#[test]
fn timing_check_fails_a_hard_coded_duration_in_core_but_not_config_or_tests() {
    let config = "pub fn d() -> std::time::Duration { std::time::Duration::from_secs(60) }\n";
    let after = "#[cfg(test)]\nuse std::time::Duration;\n\npub fn g() { let _ = Duration::from_secs(5); }\n";
    let ok = [
        ("config.rs", config),
        ("a.rs", TESTS),
        ("b.rs", "let t = Duration::from_secs(secs);\n"),
    ];
    assert_eq!(timing_check(&ok), (Some(0), String::new()));
    let (code, out) = timing_check(&[("config.rs", config), ("hosts.rs", "let t = Duration::from_secs(5);\n")]);
    assert_eq!(code, Some(1), "{out}");
    assert!(out.contains("hosts.rs:1: let t = Duration::from_secs(5);"), "{out}");
    let (code, out) = timing_check(&[("x/y.rs", after)]);
    assert_eq!(code, Some(1), "{out}");
    assert!(out.contains("x/y.rs:4:"), "{out}");
    let (code, out) = timing_check(&[(
        "a.rs",
        &TESTS.replace("pub fn f() {}", "fn f() { sleep(Duration::from_mins(1)) }"),
    )]);
    assert_eq!(code, Some(1), "{out}");
}

#[test]
fn timing_check_passes_this_tree() {
    let o = Command::new("bash")
        .arg(root().join("scripts/timing-check.sh"))
        .arg(root())
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
}
