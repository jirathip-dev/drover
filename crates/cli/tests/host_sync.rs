mod common;
use common::*;
use drover::events::{read, Event};
use serde_json::json;
use std::path::Path;

const NOW: u64 = 1_000_000;

const DROVER: &str = "[fleet.drover]\nrepo = \"jirathip-dev/herdsman\"\nlocal = \"/x/drover\"\nintegration = \"staging\"\n\n[fleet.drover.role.impl]\nharness = \"hermes\"\n";

fn config(extra: &str) -> String {
    let base = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
    format!("{base}\n{DROVER}\n[hosts.mini]\ntags = [\"fast\"]\n{extra}")
}

fn setup(name: &str) -> drover::Ctx {
    let board = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let serve = format!("\n[serve]\naddr = \"{}\"\n", board.local_addr().unwrap());
    std::mem::forget(board);
    let c = ctx(&tmpdir(name), &config(&serve));
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos", "arch:arm64", "xcode"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    c
}

const NEW: &str = "1111111111111111111111111111111111111111";
const OLD: &str = "0000000000000000000000000000000000000000";

fn ver(sha: &str) -> String {
    format!("drover 0.1.0 ({sha})")
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == kind)
        .cloned()
        .collect()
}

fn rollout(c: &drover::Ctx) -> Fake {
    let tag = drover::release::tag("main", NEW);
    let sums = format!(
        "{}  drover-x86_64-unknown-linux-gnu\n{}  drover-aarch64-apple-darwin\n",
        "b".repeat(64),
        "a".repeat(64)
    );
    Fake::default()
        .reply("git -C /x/drover rev-parse origin/main", &format!("{NEW}\n"))
        .reply("git -C /x/drover fetch", "")
        .reply(&format!("gh release download {tag} "), &sums)
        .reply(&format!("sh -c ( n='{}.new'", c.exe), &ver(NEW))
        .reply(&format!("{}.new doctor --config-only", c.exe), "")
        .reply(&format!("{} profiles sync", c.exe), "")
        .reply("launchctl print gui/", "pid = 42\n")
        .reply("systemctl --user show -p MainPID", "LoadState=loaded\nMainPID=42\n")
        .reply("launchctl", "")
        .reply("systemctl", "")
        .reply("codesign -v -R", "")
}

#[test]
fn a_linux_only_release_leaves_the_mac_on_its_build() {
    let mut c = setup("rollout-linux-only");
    let dir = c.state.parent().unwrap().parent().unwrap().canonicalize().unwrap();
    c.exe = dir.join("bin/drover").display().to_string();
    version_bin(Path::new(&c.exe), &ver(OLD));
    let tag = drover::release::tag("main", NEW);
    let linux = format!("{}  drover-x86_64-unknown-linux-gnu\n", "b".repeat(64));
    let f = rollout(&c).once(&format!("gh release download {tag} "), 0, &linux);
    let (code, out) = run(&c, &Disk(&f), &["self-update"]);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("✗ mini: 1111111 not staged: no release asset for os:macos,arch:arm64"),
        "{out}"
    );
    assert!(
        std::fs::read_to_string(&c.exe).unwrap().contains(OLD),
        "the Mac keeps its build"
    );
    assert!(!f.called(&format!("{} profiles sync", c.exe)));
    let failed = kinds(&c, "self-update-failed");
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert_eq!(failed[0].str("host"), Some("mini"));
}

#[test]
fn an_unsigned_mac_binary_at_the_release_sha_installs_the_signed_build_then_is_up_to_date() {
    let mut c = setup("rollout-unsigned");
    let dir = c.state.parent().unwrap().parent().unwrap().canonicalize().unwrap();
    c.exe = dir.join("bin/drover").display().to_string();
    version_bin(Path::new(&c.exe), &ver(NEW));
    let unsigned = || rollout(&c).once("codesign -v -R", 1, "");

    let (code, out) = run(&c, &Disk(&unsigned()), &["self-update", "--dry-run"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("up to date"), "{out}");
    let mini = out.lines().find(|l| l.trim_start().starts_with("mini ")).unwrap();
    assert!(
        mini.contains("1111111 unsigned") && mini.ends_with("would stage, then swap, last"),
        "{out}"
    );

    let f = unsigned();
    let (code, out) = run(&c, &Disk(&f), &["self-update"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.starts_with(&format!("✓ installed 1111111 at {}", c.exe)), "{out}");
    assert!(f.called(&format!("sh -c ( n='{}.new'", c.exe)), "the release is staged");
    assert!(f.called(&format!("{} profiles sync", c.exe)));

    let f = rollout(&c);
    let (code, out) = run(&c, &Disk(&f), &["self-update"]);
    assert_eq!((code, out.as_str()), (0, "= up to date @ 1111111\n"));
    assert!(
        !f.called(&format!("sh -c ( n='{}.new'", c.exe)),
        "a signed binary is left alone"
    );
}
