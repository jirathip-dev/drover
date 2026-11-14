mod common;
use common::*;
use drover::events::{append, Event};
use drover::runner::{Out, Real, Runner};
use serde_json::json;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BUDGET_MB: u64 = 300;
const HOOKS: &str = "[serve]\nhooks_addr = \"127.0.0.1:0\"\n";

struct Leaky(Mutex<Vec<Vec<u8>>>);
impl Runner for Leaky {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.0.lock().unwrap().push(vec![7; 1 << 20]);
        Real.run(prog, args)
    }
}

#[test]
#[ignore = "the soak's served copy, started by the soak tests"]
fn soak_child() {
    let Ok(mode) = std::env::var("SOAK_CHILD") else {
        return;
    };
    let ctx = drover::Ctx::from_env();
    let leaky = Leaky(Mutex::default());
    let r: &dyn Runner = if mode == "leak" { &leaky } else { &Real };
    drover::hooks::board(&ctx, r, "127.0.0.1:0", false, true).unwrap();
}

fn fixture(c: &drover::Ctx) {
    let now = drover::now() - 86_400;
    let ev = |i: u64, f: &str, n: u64, kind: &str, d| {
        serde_json::to_string(&Event::new(now + i, f, Some(n), kind, d)).unwrap()
    };
    let body = "a finding, its proof and the file it names\n".repeat(30);
    let mut log = vec![];
    for i in 0..30_000u64 {
        let (f, n) = (["sendmeter", "berth"][(i % 2) as usize], 1 + i % 300);
        let agent = format!("{f}-impl-{n}");
        let line = match i % 10 {
            0 => ev(
                i,
                f,
                n,
                "spawn",
                json!({"role": "impl", "agent": agent, "pane": agent, "worktree": format!("/wt/{agent}")}),
            ),
            1 => ev(
                i,
                f,
                n,
                "verdict",
                json!({"verdict": "FAIL", "sha": "a".repeat(40), "body": body}),
            ),
            2 => ev(
                i,
                f,
                n,
                "needs-decision",
                json!({"id": format!("{f}-q{i}"), "agent": agent, "question": "which way?", "options": ["yes", "no"], "recommend": 1}),
            ),
            3 => ev(
                i,
                f,
                n,
                "decision",
                json!({"id": format!("{f}-q{}", i - 1), "choice": "yes", "by": "owner"}),
            ),
            4 => ev(
                i,
                f,
                n,
                "thread-msg",
                json!({"thread": format!("{f}-t{n}"), "from": "orchestrator", "text": body[..600].to_string()}),
            ),
            5 => ev(i, f, n, "pushed", json!({"sha": "b".repeat(40)})),
            _ => ev(
                i,
                f,
                n,
                "driven",
                json!({"step": "review", "code": "EXIT", "result": "exit 1: boom"}),
            ),
        };
        log.push(line);
    }
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    std::fs::write(&c.state, log.join("\n") + "\n").unwrap();
}

struct Served(Child);
impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn envs(cmd: &mut Command, c: &drover::Ctx) {
    cmd.env("FLEET_CONFIG", &c.config)
        .env("FLEET_STATE", &c.state)
        .env("FLEET_WORKTREES", &c.worktrees)
        .env("DROVER_HOME", &c.home)
        .env("PATH", "/usr/bin:/bin")
        .env_remove("DROVER_LANE")
        .env_remove("DROVER_SERVE_FDS");
}

fn rss_mb(pid: u32) -> Option<u64> {
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u64>()
        .ok()
        .map(|kb| kb >> 10)
}

fn get(addr: &str, path: &str, cookie: &str) -> Result<(), String> {
    let mut s = TcpStream::connect(addr).map_err(|e| format!("{path}: {e}"))?;
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n{cookie}Connection: close\r\n\r\n");
    s.write_all(req.as_bytes()).map_err(|e| format!("{path}: {e}"))?;
    let mut res = vec![];
    s.read_to_end(&mut res).map_err(|e| format!("{path}: {e}"))?;
    let line = String::from_utf8_lossy(&res)
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    line.starts_with("HTTP/1.1 200")
        .then_some(())
        .ok_or(format!("{path}: {line}"))
}

fn median(v: &[u64]) -> u64 {
    let mut v = v.to_vec();
    v.sort_unstable();
    v[v.len() / 2]
}

fn soak(name: &str, mode: &str, config: &str) -> Result<Vec<u64>, String> {
    let secs: u64 = std::env::var("DROVER_SOAK_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let c = ctx(&tmpdir(name), config);
    fixture(&c);
    let fake = Fake::default();
    let s = drover::serve::Server::new(&c, &fake, Some("127.0.0.1:7777".into()));
    let cookies: Vec<String> = (0..8)
        .map(|i| pair(&s, &c, &format!("soak {i}"), drover::now()))
        .collect();
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["soak_child", "--exact", "--ignored", "--nocapture"])
        .env("SOAK_CHILD", mode)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    envs(&mut cmd, &c);
    let mut child = Served(cmd.spawn().unwrap());
    let pid = child.0.id();
    let mut err = BufReader::new(child.0.stderr.take().unwrap());
    let mut first = String::new();
    err.read_line(&mut first).unwrap();
    let addr = first
        .trim()
        .strip_prefix("drover serve: phone page on http://")
        .unwrap_or_else(|| panic!("{first}"))
        .trim_end_matches('/')
        .to_string();
    std::thread::spawn(move || err.lines().map_while(Result::ok).for_each(|l| eprintln!("serve: {l}")));
    get(&addr, "/", "")?;
    let stop = Arc::new(AtomicBool::new(false));
    let streams: Vec<_> = cookies
        .iter()
        .map(|cookie| {
            let (addr, cookie, stop) = (addr.clone(), cookie.clone(), stop.clone());
            std::thread::spawn(move || {
                let mut s = TcpStream::connect(&addr).unwrap();
                s.write_all(format!("GET /live HTTP/1.1\r\nHost: {addr}\r\n{cookie}\r\n").as_bytes())
                    .unwrap();
                s.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
                let (mut b, mut got) = ([0u8; 65_536], 0);
                while !stop.load(SeqCst) {
                    match s.read(&mut b) {
                        Ok(0) => break,
                        Ok(n) => got += n,
                        Err(_) => {}
                    }
                }
                got
            })
        })
        .collect();
    let (t0, mut samples, mut next_drive) = (Instant::now(), vec![], Instant::now());
    while t0.elapsed() < Duration::from_secs(secs) {
        let tick = Instant::now();
        for p in ["/", "/herd.json", "/api/v1/state"] {
            get(&addr, p, &cookies[0])?;
        }
        let d = json!({"step": "review", "code": "EXIT", "result": "exit 1: boom"});
        append(&c.state, &Event::new(drover::now(), "sendmeter", Some(1), "driven", d)).unwrap();
        if tick >= next_drive {
            let mut drive = Command::new(env!("CARGO_BIN_EXE_drover"));
            envs(drive.arg("drive").stdout(Stdio::null()).stderr(Stdio::null()), &c);
            drive.status().unwrap();
            next_drive = tick + Duration::from_secs(10);
        }
        let mb = rss_mb(pid).ok_or("serve is gone")?;
        samples.push(mb);
        if mb > BUDGET_MB {
            return Err(format!("RSS {mb} MB over the {BUDGET_MB} MB budget: {samples:?}"));
        }
        std::thread::sleep(Duration::from_secs(1).saturating_sub(tick.elapsed()));
    }
    stop.store(true, SeqCst);
    let streamed: Vec<usize> = streams.into_iter().map(|t| t.join().unwrap()).collect();
    assert!(
        streamed.iter().all(|n| *n > 0),
        "a /live client heard nothing: {streamed:?}"
    );
    let q = samples.len() / 4;
    let (mid, end) = (median(&samples[q..2 * q]), median(&samples[3 * q..]));
    eprintln!(
        "soak {mode}: RSS start {} MB, second quarter {mid} MB, last quarter {end} MB: {samples:?}",
        samples[0]
    );
    if let Ok(p) = std::env::var("GITHUB_STEP_SUMMARY") {
        let line = format!(
            "- soak ({mode}, {secs} s): RSS {} → {end} MB (budget {BUDGET_MB})\n",
            samples[0]
        );
        let _ = std::fs::OpenOptions::new()
            .append(true)
            .open(p)
            .map(|mut f| f.write_all(line.as_bytes()));
    }
    if end > mid + (mid / 5).max(16) {
        return Err(format!("RSS grew from {mid} MB to {end} MB: {samples:?}"));
    }
    Ok(samples)
}

#[test]
#[ignore = "a minute of load: CI's soak step runs it in release"]
fn serve_holds_its_budget_and_stops_growing_under_load() {
    let samples = soak("soak", "plain", HOOKS).unwrap();
    let end = median(&samples[samples.len() * 3 / 4..]);
    assert!(
        end <= 100,
        "the soak's steady RSS is {end} MB, over the 100 MB target: {samples:?}"
    );
}

#[test]
#[ignore = "a minute of load: CI's soak step runs it in release"]
fn the_soak_fails_on_a_planted_leak() {
    let err = soak(
        "soak-leak",
        "leak",
        &format!("{HOOKS}[limits]\nserve_mem_mb = 100000\n"),
    )
    .unwrap_err();
    assert!(err.contains("grew") || err.contains("budget"), "{err}");
}
