mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::runner::{Out, Real, Runner};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const NOW: u64 = 1_000_000;
struct Remote {
    f: Fake,
    ssh: PathBuf,
}

fn boat1(prog: &str, args: &[String]) -> bool {
    prog == "ssh" && args[0] != "-G" && args.iter().any(|a| a == "boat1")
}

impl Runner for Remote {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.run_for(prog, args, "", drover::runner::limit(prog, args))
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.run_for(prog, args, input, drover::runner::limit(prog, args))
    }
    fn run_for(&self, prog: &str, args: &[String], input: &str, limit: Duration) -> anyhow::Result<Out> {
        if !boat1(prog, args) {
            return self.f.run_in(prog, args, input);
        }
        self.f.calls.borrow_mut().push(format!("{prog} {}", args.join(" ")));
        Real.run_for(&self.ssh.to_string_lossy(), args, input, limit)
    }
    fn detach(&self, exe: &str, args: &[String], log: &Path) -> anyhow::Result<u32> {
        self.f.detach(exe, args, log)
    }
}
fn fake_ssh(dir: &Path, tail: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("ssh");
    let pids = dir.join("pids");
    let body = format!(
        "#!/bin/sh\ncase \"$*\" in *'-- true') exit 0;; esac\nsleep 600 &\necho $! >> '{}'\n{tail}\n",
        pids.display()
    );
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn alive(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn setup(name: &str) -> (drover::Ctx, FakeShed) {
    let base = CONFIG
        .replace("harness  =  \"hermes\"", "harness = \"claude\"")
        .replace("[limits]\n", "[limits]\nssh_call_secs = 2\nssh_quick_secs = 2\n");
    let toml = format!("{base}\n[hosts.mini]\n\n[hosts.boat1]\nmachine = \"boat1\"\n");
    let c = ctx(&tmpdir(name), &toml);
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "boat1": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    let d = json!({"role": "impl", "agent": "sendmeter-impl-11", "pane": "sendmeter-impl-11", "workspace": "",
                   "worktree": "/Users/me/.herdr/worktrees/sendmeter/issue-11", "branch": "issue-11", "host": "boat1",
                   "machine": "boat1"});
    append(&c.state, &Event::new(NOW - 600, "sendmeter", Some(11), "spawn", d)).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let sh = FakeShed::attached(&c);
    (c, sh)
}

fn local() -> Fake {
    Fake::default()
        .reply(
            "herdr status",
            "client:\n  version: 0.9.5\n\nserver:\n  status: running\n  version: 0.9.5\n",
        )
        .reply("sysctl -n vm.loadavg", "{ 0.4 1 1 }")
        .reply("sysctl -n hw.ncpu", "4")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("git ", "")
        .reply("gh issue view", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply(
            "which shed",
            "/opt/shed/bin/shed
",
        )
}

#[test]
fn an_ssh_that_never_returns_costs_one_deadline_and_drive_still_starts() {
    for (name, tail) in [("deadline-hang", "wait"), ("deadline-held-pipes", "exit 0")] {
        let (c, sh) = setup(name);
        let dir = c.state.parent().unwrap().to_path_buf();
        let r = Remote {
            f: local(),
            ssh: fake_ssh(&dir, tail),
        };
        let t = Instant::now();
        let (code, out) = run(&c, &r, &["tick"]);
        let took = t.elapsed();
        assert_eq!(code, 0, "{name}: {out}");
        assert!(took < Duration::from_secs(2 + 4), "{name}: {took:?}\n{out}");
        assert!(
            sh.called("run", "sendmeter-impl-12"),
            "{name}: drive still starts the main host's work: {out}"
        );
        let calls = r.f.calls.borrow();
        let sent: Vec<_> = calls
            .iter()
            .filter(|l| l.starts_with("ssh ") && l.contains(" boat1 ") && !l.ends_with("-- true"))
            .collect();
        assert_eq!(
            sent.len(),
            2,
            "{name}: one shed connect and its retry off the shared socket, then the host is skipped: {sent:#?}"
        );
        assert!(sent.iter().all(|l| l.contains("'shed' 'connect'")), "{name}: {sent:#?}");
        assert!(!drover::hosts::cached(&c)["boat1"].up, "{name}: boat1 shows offline");
        assert!(
            read(&c.state)
                .unwrap()
                .iter()
                .any(|e| e.kind == "host-offline" && e.str("host") == Some("boat1")),
            "{name}: host-offline logged"
        );
        let pids = std::fs::read_to_string(dir.join("pids")).unwrap();
        for pid in pids.lines() {
            if tail == "wait" {
                let gone = (0..20).any(|_| {
                    std::thread::sleep(Duration::from_millis(100));
                    !alive(pid)
                });
                assert!(gone, "{name}: the ssh's process group is killed with it");
            } else {
                let _ = std::process::Command::new("kill").arg(pid).status();
            }
        }
    }
}
