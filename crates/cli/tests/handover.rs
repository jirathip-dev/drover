mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::json;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CONFIG: &str = "[serve]\nhooks_addr = \"127.0.0.1:0\"\n";

fn get(addr: &str) -> Result<String, String> {
    let mut s = TcpStream::connect(addr).map_err(|e| format!("connect: {e}"))?;
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let req = format!("GET / HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).map_err(|e| format!("write: {e}"))?;
    let mut res = vec![];
    s.read_to_end(&mut res).map_err(|e| format!("read: {e}"))?;
    let res = String::from_utf8_lossy(&res);
    let line = res.lines().next().unwrap_or_default().to_string();
    if line.starts_with("HTTP/1.1 200") {
        Ok(line)
    } else {
        Err(format!("status: {line:?}"))
    }
}

fn live(addr: String, mut last: u64, stop: Arc<AtomicBool>, seen: Arc<Mutex<Vec<u64>>>, opens: Arc<AtomicUsize>) {
    while !stop.load(SeqCst) {
        let Ok(mut s) = TcpStream::connect(&addr) else {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        };
        s.write_all(format!("GET /live?since={last} HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
            .unwrap();
        s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        opens.fetch_add(1, SeqCst);
        let (mut buf, mut b) = (String::new(), [0u8; 65_536]);
        while !stop.load(SeqCst) {
            match s.read(&mut b) {
                Ok(0) => break,
                Ok(n) => buf.push_str(&String::from_utf8_lossy(&b[..n])),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(_) => break,
            }
            while let Some(i) = buf.find("\n\n") {
                let frame: String = buf.drain(..i + 2).collect();
                let id = frame.lines().find_map(|l| l.strip_prefix("id: ")?.parse::<u64>().ok());
                if let Some(n) = id.filter(|_| frame.contains("\nevent: ev\n")) {
                    seen.lock().unwrap().push(n);
                }
                last = last.max(id.unwrap_or(0));
            }
        }
    }
}

fn alive(pid: &str) -> bool {
    std::process::Command::new("kill")
        .args(["-0", pid])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn a_handover_fails_no_request_and_loses_no_live_event() {
    let c = ctx(&tmpdir("ho"), CONFIG);
    append(&c.state, &Event::new(c.now, "drover", None, "note", json!({}))).unwrap();
    let mut p = std::process::Command::new(env!("CARGO_BIN_EXE_drover"))
        .args(["serve", "--addr", "127.0.0.1:0"])
        .env("FLEET_CONFIG", &c.config)
        .env("FLEET_STATE", &c.state)
        .env("FLEET_WORKTREES", &c.worktrees)
        .env("DROVER_HOME", &c.home)
        .env("PATH", "/usr/bin:/bin")
        .env_remove("DROVER_LANE")
        .env_remove("DROVER_SERVE_FDS")
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let log = Arc::new(Mutex::new(String::new()));
    let mut err = BufReader::new(p.stderr.take().unwrap());
    let mut first = String::new();
    err.read_line(&mut first).unwrap();
    let addr = first
        .trim()
        .strip_prefix("drover serve: phone page on http://")
        .unwrap_or_else(|| panic!("{first}"))
        .trim_end_matches('/')
        .to_string();
    let l2 = log.clone();
    std::thread::spawn(move || {
        for l in err.lines().map_while(Result::ok) {
            l2.lock().unwrap().push_str(&(l + "\n"));
        }
    });
    get(&addr).unwrap();

    let stop = Arc::new(AtomicBool::new(false));
    let (fails, gets) = (Arc::new(Mutex::new(vec![])), Arc::new(AtomicUsize::new(0)));
    let poller = {
        let (stop, fails, gets, addr) = (stop.clone(), fails.clone(), gets.clone(), addr.clone());
        std::thread::spawn(move || {
            while !stop.load(SeqCst) {
                let at = Instant::now();
                if let Err(e) = get(&addr) {
                    fails.lock().unwrap().push(e);
                }
                gets.fetch_add(1, SeqCst);
                std::thread::sleep(Duration::from_millis(50).saturating_sub(at.elapsed()));
            }
        })
    };
    let base = read(&c.state).unwrap().last().unwrap().seq;
    let (seen, opens) = (Arc::new(Mutex::new(vec![])), Arc::new(AtomicUsize::new(0)));
    let client = {
        let (stop, seen, opens, addr) = (stop.clone(), seen.clone(), opens.clone(), addr.clone());
        std::thread::spawn(move || live(addr, base, stop, seen, opens))
    };
    std::thread::sleep(Duration::from_millis(1_500));
    let writing = Arc::new(AtomicBool::new(true));
    let writer = {
        let (writing, state) = (writing.clone(), c.state.clone());
        std::thread::spawn(move || {
            while writing.load(SeqCst) {
                append(&state, &Event::new(1, "drover", None, "note", json!({}))).unwrap();
                std::thread::sleep(Duration::from_millis(100));
            }
        })
    };
    std::thread::sleep(Duration::from_millis(500));

    let port = addr.rsplit_once(':').unwrap().1.parse().unwrap();
    let mut ctl = UnixStream::connect(drover::update::serve_sock(&c, port)).unwrap();
    ctl.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    ctl.write_all(b"handover\n").unwrap();
    let mut reply = String::new();
    BufReader::new(ctl).read_line(&mut reply).unwrap();
    let bridge = reply
        .trim()
        .strip_prefix("ok ")
        .unwrap_or_else(|| panic!("{reply}"))
        .to_string();
    let t = Instant::now();
    while alive(&bridge) && t.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(50));
    }
    let gone = !alive(&bridge);
    if !gone {
        std::process::Command::new("kill").arg(&bridge).status().unwrap();
    }
    assert!(
        gone,
        "the bridge drained and exited once the new serve took over: {:?} {}",
        p.try_wait(),
        log.lock().unwrap()
    );
    std::thread::sleep(Duration::from_millis(1_000));
    writing.store(false, SeqCst);
    writer.join().unwrap();
    std::thread::sleep(Duration::from_millis(2_500));
    stop.store(true, SeqCst);
    poller.join().unwrap();
    client.join().unwrap();
    let still = p.try_wait().unwrap();
    let _ = p.kill();
    let _ = p.wait();

    assert!(still.is_none(), "the supervised pid serves on (exec in place)");
    let log = log.lock().unwrap().clone();
    assert_eq!(
        log.matches("drover serve: phone page on").count(),
        2,
        "the bridge and the new serve: {log}"
    );
    assert!(fails.lock().unwrap().is_empty(), "{:?}\n{log}", fails.lock().unwrap());
    assert!(gets.load(SeqCst) > 20, "{}", gets.load(SeqCst));
    let written: Vec<u64> = read(&c.state)
        .unwrap()
        .iter()
        .map(|e| e.seq)
        .filter(|&s| s > base)
        .collect();
    let mut got = seen.lock().unwrap().clone();
    got.retain(|&s| s > base);
    assert!(
        opens.load(SeqCst) >= 3,
        "reconnected to the bridge, then the new serve: {}",
        opens.load(SeqCst)
    );
    assert_eq!(got, written, "each event once, none lost\n{log}");
}

#[test]
fn a_serve_over_its_memory_limit_hands_over_with_no_failed_request() {
    let c = ctx(&tmpdir("ho-mem"), &format!("{CONFIG}[limits]\nserve_host_secs = 1\n"));
    append(&c.state, &Event::new(c.now, "drover", None, "note", json!({}))).unwrap();
    let mut p = std::process::Command::new(env!("CARGO_BIN_EXE_drover"))
        .args(["serve", "--addr", "127.0.0.1:0"])
        .env("FLEET_CONFIG", &c.config)
        .env("FLEET_STATE", &c.state)
        .env("FLEET_WORKTREES", &c.worktrees)
        .env("DROVER_HOME", &c.home)
        .env("PATH", "/usr/bin:/bin")
        .env_remove("DROVER_LANE")
        .env_remove("DROVER_SERVE_FDS")
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let log = Arc::new(Mutex::new(String::new()));
    let mut err = BufReader::new(p.stderr.take().unwrap());
    let mut first = String::new();
    err.read_line(&mut first).unwrap();
    let addr = first
        .trim()
        .strip_prefix("drover serve: phone page on http://")
        .unwrap_or_else(|| panic!("{first}"))
        .trim_end_matches('/')
        .to_string();
    let l2 = log.clone();
    std::thread::spawn(move || {
        for l in err.lines().map_while(Result::ok) {
            l2.lock().unwrap().push_str(&(l + "\n"));
        }
    });
    get(&addr).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let (fails, gets) = (Arc::new(Mutex::new(vec![])), Arc::new(AtomicUsize::new(0)));
    let poller = {
        let (stop, fails, gets, addr) = (stop.clone(), fails.clone(), gets.clone(), addr.clone());
        std::thread::spawn(move || {
            while !stop.load(SeqCst) {
                let at = Instant::now();
                if let Err(e) = get(&addr) {
                    fails.lock().unwrap().push(e);
                }
                gets.fetch_add(1, SeqCst);
                std::thread::sleep(Duration::from_millis(50).saturating_sub(at.elapsed()));
            }
        })
    };
    std::thread::sleep(Duration::from_millis(5_000));
    let limit = |mb: u64| {
        let cfg = format!("{CONFIG}[limits]\nserve_host_secs = 1\nserve_mem_mb = {mb}\n");
        std::fs::write(&c.config, cfg).unwrap();
    };
    limit(1);
    let high = || {
        read(&c.state)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "serve-mem-high")
            .cloned()
            .collect::<Vec<_>>()
    };
    let t = Instant::now();
    while high().is_empty() && t.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(20));
    }
    limit(100_000);
    let t = Instant::now();
    while log.lock().unwrap().matches("drover serve: phone page on").count() < 2
        && t.elapsed() < Duration::from_secs(60)
    {
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(2_000));
    stop.store(true, SeqCst);
    poller.join().unwrap();
    let still = p.try_wait().unwrap();
    let _ = p.kill();
    let _ = p.wait();

    let log = log.lock().unwrap().clone();
    let high = high();
    assert_eq!(high.len(), 1, "logged once: {high:?}\n{log}");
    assert_eq!(high[0].data["handover"], true, "{high:?}");
    assert!(high[0].data["rss_mb"].as_u64().unwrap() >= 1, "{high:?}");
    assert!(still.is_none(), "the supervised pid serves on (exec in place)\n{log}");
    assert_eq!(
        log.matches("drover serve: phone page on").count(),
        2,
        "the bridge and the fresh copy: {log}"
    );
    assert!(fails.lock().unwrap().is_empty(), "{:?}\n{log}", fails.lock().unwrap());
    assert!(gets.load(SeqCst) > 20, "{}", gets.load(SeqCst));
}

#[test]
fn a_serve_that_starts_over_its_memory_limit_logs_once_and_keeps_serving() {
    let c = ctx(
        &tmpdir("ho-mem-start"),
        &format!("{CONFIG}[limits]\nserve_host_secs = 1\nserve_mem_mb = 1\n"),
    );
    let mut p = std::process::Command::new(env!("CARGO_BIN_EXE_drover"))
        .args(["serve", "--addr", "127.0.0.1:0"])
        .env("FLEET_CONFIG", &c.config)
        .env("FLEET_STATE", &c.state)
        .env("FLEET_WORKTREES", &c.worktrees)
        .env("DROVER_HOME", &c.home)
        .env("PATH", "/usr/bin:/bin")
        .env_remove("DROVER_LANE")
        .env_remove("DROVER_SERVE_FDS")
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut err = BufReader::new(p.stderr.take().unwrap());
    let mut first = String::new();
    err.read_line(&mut first).unwrap();
    let addr = first
        .trim()
        .strip_prefix("drover serve: phone page on http://")
        .unwrap()
        .trim_end_matches('/')
        .to_string();
    let log = Arc::new(Mutex::new(String::new()));
    let l2 = log.clone();
    std::thread::spawn(move || {
        err.lines()
            .map_while(Result::ok)
            .for_each(|l| l2.lock().unwrap().push_str(&(l + "\n")))
    });
    std::thread::sleep(Duration::from_millis(5_000));
    get(&addr).unwrap();
    let still = p.try_wait().unwrap();
    let _ = p.kill();
    let _ = p.wait();
    let high: Vec<Event> = read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "serve-mem-high")
        .cloned()
        .collect();
    let log = log.lock().unwrap().clone();
    assert_eq!(high.len(), 1, "logged once: {high:?}");
    assert_eq!(
        high[0].data["handover"], false,
        "a fresh copy would start over it too: {high:?}"
    );
    assert!(still.is_none() && !log.contains("phone page on"), "no handover: {log}");
}
