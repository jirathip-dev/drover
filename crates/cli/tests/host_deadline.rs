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
            1,
            "{name}: one shed connect killed at its deadline, then the host is skipped: {sent:#?}"
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

const SSH2: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C -oControlPersist=10m boat2 -- ";

struct Two {
    f: Fake,
    ssh: PathBuf,
    boat2: FakeShed,
}

impl Runner for Two {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.run_for(prog, args, "", drover::runner::limit(prog, args))
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.run_for(prog, args, input, drover::runner::limit(prog, args))
    }
    fn run_for(&self, prog: &str, args: &[String], input: &str, limit: Duration) -> anyhow::Result<Out> {
        if !boat1(prog, args) {
            let on = OverSsh {
                f: &self.f,
                hosts: vec![("boat2", &self.boat2)],
            };
            return on.run_in(prog, args, input);
        }
        self.f.calls.borrow_mut().push(format!("{prog} {}", args.join(" ")));
        Real.run_for(&self.ssh.to_string_lossy(), args, input, limit)
    }
    fn sleep(&self, secs: u64) {
        self.f.sleep(secs);
        std::thread::sleep(Duration::from_secs(secs));
    }
    fn detach(&self, exe: &str, args: &[String], log: &Path) -> anyhow::Result<u32> {
        self.f.detach(exe, args, log)
    }
}

fn hung_connect(dir: &Path, child: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("ssh");
    let (pids, seen) = (dir.join("pids"), dir.join("connects"));
    let body = format!(
        "#!/bin/sh\ncase \"$*\" in *\"'sh' '-c'\"*) exit 0;; *\"'shed' 'connect'\") ;; *) exit 0;; esac\n\
         [ -f '{seen}' ] || {{ : > '{seen}'; exit 1; }}\n{child} &\necho $! >> '{pids}'\nwait\n",
        seen = seen.display(),
        pids = pids.display()
    );
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn boat2_side() -> Fake {
    local()
        .reply(&format!("{SSH2}'cat' '/proc/loadavg'"), "0.4 1 1 1/9 42")
        .reply(&format!("{SSH2}'nproc'"), "4")
        .reply(
            &format!("{SSH2}'/opt/drover/bin/drover' 'profiles' 'sync' '--profile'"),
            "",
        )
        .reply(&format!("{SSH2}'git' '--no-replace-objects' 'ls-remote'"), "abc\tHEAD\n")
        .reply(&format!("{SSH2}'sh' '-lc'"), "")
        .reply(
            &format!("{SSH2}'/opt/drover/bin/drover' '--version'"),
            &format!("drover {}\n", drover::VERSION),
        )
        .reply(&format!("{SSH2}'sh' '-c' 'git clone --single-branch"), "")
        .reply(&format!("{SSH2}'git' '--no-replace-objects' '-C'"), "")
        .reply(&format!("{SSH2}'sh' '-c' 'mkdir"), "")
        .reply(&format!("{SSH2}'sh' '-c' 'umask 077"), "")
        .reply(&format!("{SSH2}'rm' '-f'"), "")
}

fn oldest_child(mark: &Path, stop: &std::sync::atomic::AtomicBool) -> Duration {
    let (mut worst, mut seen): (Duration, Option<(String, Instant)>) = (Duration::ZERO, None);
    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
        let m: serde_json::Value = std::fs::read_to_string(mark)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let child = m["child"].as_str().unwrap_or_default().to_string();
        let key = child.clone();
        match &seen {
            Some((k, t)) if *k == key => worst = worst.max(t.elapsed()),
            _ => seen = (!child.is_empty()).then(|| (key, Instant::now())),
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    worst
}

#[test]
fn a_hung_shed_connect_on_one_remote_host_costs_one_deadline_and_another_remote_host_still_spawns() {
    let detached = "perl -e 'setpgrp(0, 0); sleep 600'";
    for (name, child) in [("connect-hang", "sleep 600"), ("connect-held-pipe", detached)] {
        let base = CONFIG
            .replace("harness  =  \"hermes\"", "harness = \"claude\"")
            .replace("[limits]\n", "[limits]\nssh_call_secs = 2\nssh_quick_secs = 2\n");
        let toml = format!(
            "{base}\n[hosts.mini]\n\n[hosts.boat1]\nmachine = \"boat1\"\n\n[hosts.boat2]\nmachine = \"boat2\"\n"
        );
        let c = ctx(&tmpdir(name), &toml);
        let dir = c.state.parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&dir).unwrap();
        let cache = json!({
            "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
            "boat1": {"tags": ["os:linux", "docker"], "home": "/home/me", "probed": NOW, "seen": NOW, "up": true},
            "boat2": {"tags": ["os:linux", "docker"], "home": "/home/me", "probed": NOW, "seen": NOW, "up": true,
                      "load": 0.1, "mem": 12.0, "disk": 200.0},
        });
        std::fs::write(dir.join("hosts.json"), cache.to_string()).unwrap();
        let d = json!({"role": "impl", "agent": "sendmeter-impl-11", "pane": "sendmeter-impl-11", "workspace": "",
                       "worktree": "/home/me/.local/share/drover/work/sendmeter/issue-11", "branch": "issue-11",
                       "host": "boat1", "machine": "boat1"});
        append(&c.state, &Event::new(NOW - 600, "sendmeter", Some(11), "spawn", d)).unwrap();
        assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
        let add = ["queue", "sendmeter", "add", "12", "--needs", "docker"];
        assert_eq!(run(&c, &Fake::default(), &add).0, 0);
        let _main = FakeShed::attached(&c);
        let r = Two {
            f: boat2_side(),
            ssh: hung_connect(&dir, child),
            boat2: FakeShed::new(),
        };
        let stop = std::sync::atomic::AtomicBool::new(false);
        let mark = c.state.with_file_name("tick.step");
        let t = Instant::now();
        let (worst, (code, out)) = std::thread::scope(|s| {
            let w = s.spawn(|| oldest_child(&mark, &stop));
            let res = run(&c, &r, &["tick"]);
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            (w.join().unwrap(), res)
        });
        let took = t.elapsed();
        let pids = std::fs::read_to_string(dir.join("pids")).unwrap_or_default();
        for pid in pids.lines() {
            let _ = std::process::Command::new("kill").arg(pid).status();
        }
        assert_eq!(code, 0, "{name}: {out}");
        assert!(
            took < Duration::from_secs(2 + 6),
            "{name}: the pass took {took:?}\n{out}"
        );
        assert!(
            worst < Duration::from_secs(2 + 2),
            "{name}: tick.step held one child {worst:?}"
        );
        assert!(!pids.is_empty(), "{name}: boat1's shed connect was reached");
        assert!(
            r.boat2.called("run", "sendmeter-impl-12"),
            "{name}: the lane starts on boat2: {:?}\n{out}",
            r.boat2.methods()
        );
        let spawned = read(&c.state)
            .unwrap()
            .to_vec()
            .into_iter()
            .rfind(|e| e.is("spawn", "sendmeter", 12));
        assert_eq!(
            spawned.and_then(|e| e.str("host").map(String::from)).as_deref(),
            Some("boat2"),
            "{name}"
        );
        assert!(
            read(&c.state)
                .unwrap()
                .iter()
                .any(|e| e.kind == "host-offline" && e.str("host") == Some("boat1")),
            "{name}: boat1 logged offline: {out}"
        );
    }
}
