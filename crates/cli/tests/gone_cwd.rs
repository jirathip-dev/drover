mod common;
use common::*;
use drover::runner::{Out, Runner};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering::SeqCst};
use std::sync::Mutex;

static CWD: Mutex<()> = Mutex::new(());

const SHED: &str = r#"d=$(/bin/pwd -P 2>/dev/null) && test -d "$d" && exit 0
echo "shed: current directory: No such file or directory (os error 2)" >&2
exit 1"#;

struct RealShed<'a>(&'a dyn Runner);

impl Runner for RealShed<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        if prog == "shed" && args.first().is_some_and(|a| a == "run") {
            let o = drover::runner::Real.run("sh", &["-c".to_string(), SHED.into()])?;
            if o.code != 0 {
                return Ok(o);
            }
        }
        self.0.run(prog, args)
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
}

fn acp_ctx(name: &str, sock: &Path) -> drover::Ctx {
    let cfg = CONFIG.replace("harness  =  \"hermes\"    # toggled in tests", "harness = \"claude\"");
    let cfg = format!(
        "{cfg}\n[fleet.sendmeter.runtime]\nkind = \"shed-acp\"\nsocket = \"{}\"\n",
        sock.display()
    );
    let mut c = ctx(&tmpdir(name), &cfg);
    c.exe = env!("CARGO_BIN_EXE_drover").into();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    c
}

fn acp_fake(c: &drover::Ctx) -> Fake {
    let wt = c.worktrees.join("sendmeter/issue-12");
    let agents =
        json!([{"id": "claude-acp", "version": "0.101.0", "distribution": "npx", "cmd": ["echo", "claude-agent-acp"]}]);
    Fake::default()
        .reply(&format!("git -C {} rev-list", wt.display()), "0\n")
        .once("git -C /tmp/does-not-matter/sendmeter rev-parse --verify", 1, "")
        .reply("gh issue view 12", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply("git -C", "")
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("shed help run", "--meta <JSON>")
        .reply("shed agents --json", &agents.to_string())
        .reply("shed run", "")
        .reply("shed send", "")
        .reply("shed cancel", "")
}

fn gone(dir: &Path) -> PathBuf {
    let d = dir.join("gone");
    std::fs::create_dir_all(&d).unwrap();
    std::env::set_current_dir(&d).unwrap();
    std::fs::remove_dir(&d).unwrap();
    d
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<drover::events::Event> {
    let all = drover::events::read(&c.state).unwrap().to_vec();
    all.into_iter().filter(|e| e.kind == kind).collect()
}

fn started(c: &drover::Ctx) -> bool {
    !kinds(c, "spawn").is_empty()
}

#[test]
fn a_kick_and_a_spawn_from_a_deleted_directory_start_their_shed_lane() {
    let _l = CWD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for args in [
        &["kick", "sendmeter", "12", "--why", "stop", "--now"][..],
        &["spawn", "sendmeter", "12", "impl"],
    ] {
        let b = FakeShed::new();
        let c = acp_ctx("gone-cwd", &b.sock);
        let f = acp_fake(&c);
        let cli = b.cli(&f);
        let r = RealShed(&cli);
        gone(c.state.parent().unwrap().parent().unwrap());
        let (code, out) = run(&c, &r, args);
        std::env::set_current_dir("/").unwrap();
        assert_eq!(code, 0, "{args:?}: {out}");
        assert!(started(&c), "{args:?}: {out} {:?}", kinds(&c, "spawn-error"));
    }
}

#[test]
fn a_stop_hook_from_a_cleaned_worktree_kicks_from_a_directory_that_exists() {
    let _l = CWD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tmpdir("gone-hook");
    let mut c = ctx(&dir, CONFIG);
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let exe = dir.join("drover");
    let here = "d=$(/bin/pwd -P 2>/dev/null) && test -d \"$d\" && echo here || echo gone";
    std::fs::write(&exe, format!("#!/bin/sh\n{here}\n")).unwrap();
    std::fs::set_permissions(&exe, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    c.exe = exe.display().to_string();
    std::fs::write(drover::hook::turn_mark(&c.state, "sendmeter:24:rev"), "turn").unwrap();
    gone(&c.worktrees.join("sendmeter"));
    set_env("DROVER_LANE", "sendmeter:24:rev");
    let stop = json!({"hook_event_name": "Stop", "session_id": "s"}).to_string();
    let argv = ["drover", "hook", "--role", "rev"].map(String::from).to_vec();
    let mut out = vec![];
    let code = drover::main_with(argv, &c, &drover::runner::Real, &mut out, &mut stop.as_bytes());
    std::env::remove_var("DROVER_LANE");
    std::env::set_current_dir("/").unwrap();
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&out));
    let log = c.state.with_file_name("kick.log");
    let mut text = String::new();
    for _ in 0..100 {
        text = std::fs::read_to_string(&log).unwrap_or_default();
        if !text.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(text, "here\n");
}

const GONE: &str = "shed: current directory: No such file or directory (os error 2)";

struct Gone<'a>(&'a dyn Runner, AtomicU32, AtomicU32);

impl Runner for Gone<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        if prog == "shed" && args.first().is_some_and(|a| a == "run") {
            self.2.fetch_add(1, SeqCst);
            if self.1.try_update(SeqCst, SeqCst, |n| n.checked_sub(1)).is_ok() {
                return Ok(Out {
                    code: 1,
                    stderr: GONE.into(),
                    ..Out::default()
                });
            }
        }
        self.0.run(prog, args)
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
}

fn kick(c: &drover::Ctx, r: &dyn Runner) {
    let (code, out) = run(c, r, &["kick", "sendmeter", "12", "--why", "stop", "--now"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn a_spawn_that_fails_on_a_gone_current_directory_is_retried_once_and_starts() {
    let b = FakeShed::new();
    let c = acp_ctx("gone-heal", &b.sock);
    let f = acp_fake(&c);
    let cli = b.cli(&f);
    let r = Gone(&cli, AtomicU32::new(1), AtomicU32::new(0));
    kick(&c, &r);
    assert_eq!(r.2.load(SeqCst), 2, "{:?}", f.calls.borrow());
    assert_eq!(kinds(&c, "self-heal").len(), 1);
    assert!(started(&c), "{:?}", kinds(&c, "spawn-error"));
}

#[test]
fn a_gone_current_directory_never_counts_towards_fails_the_same_way() {
    let b = FakeShed::new();
    let c = acp_ctx("gone-stuck", &b.sock);
    let f = acp_fake(&c);
    let cli = b.cli(&f);
    let r = Gone(&cli, AtomicU32::new(2), AtomicU32::new(0));
    kick(&c, &r);
    assert_eq!(r.2.load(SeqCst), 2, "one retry: {:?}", f.calls.borrow());
    assert_eq!(kinds(&c, "self-heal").len(), 1);
    let again = kinds(&c, "spawn-error").pop().expect("the retry's error is logged");
    drover::events::append(&c.state, &again).unwrap();
    let ev = drover::events::read(&c.state).unwrap().to_vec();
    assert_eq!(drover::drive::backoff(&c, &ev, "sendmeter", 12), None);
}
