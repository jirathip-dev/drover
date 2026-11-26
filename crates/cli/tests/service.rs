mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpStream;

const NOW: u64 = 1_000_000;

#[test]
fn the_service_answers_a_kick_and_streams_the_log() {
    let c = ctx(&tmpdir("service"), CONFIG);
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    append(
        &c.state,
        &Event::new(NOW - 9, "sendmeter", Some(12), "queued", json!({})),
    )
    .unwrap();
    let api = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = api.local_addr().unwrap().to_string();
    let sh = FakeShed::new();
    sh.attach(&c);
    let r: &'static Fake = Box::leak(Box::new(Fake::default()));
    let sc = c.clone();
    std::thread::spawn(move || drover_service::run(sc, r, || NOW, api));
    let mut pid = None;
    for _ in 0..100 {
        pid = drover::service::up(&c);
        if pid.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(pid, Some(u64::from(std::process::id())));
    assert_eq!(
        drover::service::kick(&c, "sendmeter", 12, "push", Some("t1")),
        Some(std::process::id())
    );
    let d = json!({"id": "q1", "question": "Which colour?", "options": ["red"]});
    append(&c.state, &Event::new(NOW, "sendmeter", Some(12), "needs-decision", d)).unwrap();

    let mut s = TcpStream::connect(&addr).unwrap();
    s.write_all(b"GET /api/v1/events?after=0 HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    let (mut got, mut buf) = (String::new(), [0u8; 8192]);
    while !(got.contains("\"service-started\"") && got.contains("Which colour?")) {
        let n = s.read(&mut buf).unwrap();
        assert!(n > 0, "{got}");
        got += &String::from_utf8_lossy(&buf[..n]);
    }
    assert!(
        got.starts_with("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream"),
        "{got}"
    );
}

struct Proc(std::process::Child, std::path::PathBuf);

impl Drop for Proc {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
        for _ in 0..10 {
            let passes = drover::db::leases(&self.1, "drive-").into_iter();
            for l in passes.filter(|l| !l.key.trim_end_matches(".lock").ends_with(|c: char| c.is_ascii_digit())) {
                if l.pid == 0 || l.pid == std::process::id() {
                    continue;
                }
                std::process::Command::new("kill")
                    .args(["-9", &l.pid.to_string()])
                    .status()
                    .ok();
                drover::db::release(&self.1, &l.key, Some(&l));
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

fn service(c: &drover::Ctx, bin: &std::path::Path, api: &str) -> (Proc, u64) {
    let home = c.state.parent().unwrap().parent().unwrap();
    let child = std::process::Command::new(env!("CARGO_BIN_EXE_drover"))
        .args(["service", "--api", api])
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", home)
        .env("DROVER_HOME", home)
        .env("FLEET_CONFIG", &c.config)
        .env("FLEET_STATE", &c.state)
        .env("FLEET_WORKTREES", &c.worktrees)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let p = Proc(child, c.state.clone());
    for _ in 0..200 {
        if let Some(pid) = drover::service::up(c) {
            return (p, pid);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("the service never answered");
}

fn until(what: &str, f: impl Fn() -> bool) {
    for _ in 0..400 {
        if f() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("never: {what}");
}

#[test]
fn a_service_killed_mid_spawn_leaves_no_stale_state_and_a_new_one_resumes() {
    let d = tmpdir("service-chaos");
    let cfg = CONFIG.replace(
        "max_parallel_spawns = 0",
        "max_parallel_spawns = 2\nmin_free_gb = 0\nmin_free_mem_gb = 0",
    );
    let mut c = ctx(&d, &cfg);
    c.now = drover::now();
    c.exe = env!("CARGO_BIN_EXE_drover").into();
    synced(&c);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "1"]).0, 0);
    let (bin, log) = (d.join("bin"), d.join("gh.log"));
    std::fs::create_dir_all(&bin).unwrap();
    let tools = [
        (
            "gh",
            format!(
                "case \"$*\" in *'issue view'*) echo \"$*\" >> '{}'; exec sleep 60;; *) echo '[]';; esac",
                log.display()
            ),
        ),
        ("claude", "exit 0".into()),
        (
            "shed",
            "case \"$*\" in 'help run') echo '--meta <JSON>';; 'agents --json') echo '[{\"id\": \"claude-acp\", \"cmd\": [\"claude-acp\"]}]';; esac".into(),
        ),
    ];
    for (name, body) in tools {
        let p = bin.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    }
    let sh = FakeShed::new();
    sh.attach(&c);
    let pass = |r: &Fake| assert_eq!(run(&c, r, &["drive"]).0, 0);
    let host = || {
        Fake::default()
            .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
            .reply("sysctl -n vm.loadavg", "{ 1.0 1.0 1.0 }\n")
            .reply("sysctl -n hw.ncpu", "10\n")
            .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
            .reply("git -C", "")
    };
    let cfgv = drover::config::Config::load(&c.config).unwrap();
    let live = || drover::drive::markers(&c, &cfgv, &drover::runner::Real, false).0;
    let marker = || lease_on(&c, "spawn-sendmeter-1.pending");

    let (a, pid_a) = service(&c, &bin, "127.0.0.1:0");
    pass(&host());
    until("the pass took the start slot for the service", || {
        marker().is_some_and(|l| u64::from(l.pid) == pid_a)
    });
    until("the spawn reached GitHub", || {
        std::fs::read_to_string(&log).is_ok_and(|s| s.contains("issue view 1"))
    });
    assert_eq!(live(), ["spawn-sendmeter-1.pending", "kick-sendmeter-1.lock"]);

    drop(a);
    assert!(live().is_empty(), "{:?}", live());
    assert_eq!(drover::service::up(&c), None);

    let (_b, pid_b) = service(&c, &bin, "127.0.0.1:0");
    assert_ne!(pid_a, pid_b);
    pass(&host());
    let ev = read(&c.state).unwrap();
    assert!(ev.iter().any(|e| e.kind == "stale-marker" && e.issue == Some(1)));
    until("the pass took the start slot for the service", || {
        marker().is_some_and(|l| u64::from(l.pid) == pid_b)
    });
    until("the spawn was retried", || {
        std::fs::read_to_string(&log).is_ok_and(|s| s.matches("issue view 1").count() == 2)
    });
}
