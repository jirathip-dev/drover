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

fn live(addr: &str, mut last: u64, stop: &AtomicBool, seen: &Mutex<Vec<u64>>, opens: &AtomicUsize) {
    while !stop.load(SeqCst) {
        let Ok(mut s) = TcpStream::connect(addr) else {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        };
        s.write_all(format!("GET /live?since={last} HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
            .unwrap();
        s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        opens.fetch_add(1, SeqCst);
        let (mut buf, mut b) = (String::new(), vec![0u8; 65_536]);
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
        std::thread::spawn(move || live(&addr, base, &stop, &seen, &opens))
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
    assert_eq!(high[0].data["handover"], true, "{high:?}");
    assert!(high[0].data["rss_mb"].as_u64().unwrap() >= 1, "{high:?}");
    assert!(
        high[1..].iter().all(|e| e.data["handover"] == false),
        "a fresh copy over it never hands over: {high:?}"
    );
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
            .for_each(|l| l2.lock().unwrap().push_str(&(l + "\n")));
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

fn serve(c: &drover::Ctx, addr: &str) -> std::process::Child {
    std::process::Command::new(env!("CARGO_BIN_EXE_drover"))
        .args(["serve", "--addr", addr])
        .env("FLEET_CONFIG", &c.config)
        .env("FLEET_STATE", &c.state)
        .env("FLEET_WORKTREES", &c.worktrees)
        .env("DROVER_HOME", &c.home)
        .env("PATH", "/usr/bin:/bin")
        .env_remove("DROVER_LANE")
        .env_remove("DROVER_SERVE_FDS")
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap()
}

fn lines(p: &mut std::process::Child) -> Arc<Mutex<String>> {
    let (log, err) = (Arc::new(Mutex::new(String::new())), p.stderr.take().unwrap());
    let l2 = log.clone();
    std::thread::spawn(move || {
        for l in BufReader::new(err).lines().map_while(Result::ok) {
            l2.lock().unwrap().push_str(&(l + "\n"));
        }
    });
    log
}

fn exited(p: &mut std::process::Child, secs: u64) -> Option<std::process::ExitStatus> {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(secs) {
        if let Some(s) = p.try_wait().unwrap() {
            return Some(s);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = p.kill();
    None
}

fn addr_of(log: &Mutex<String>) -> String {
    let t = Instant::now();
    loop {
        let l = log.lock().unwrap().clone();
        if let Some(a) = l
            .lines()
            .find_map(|l| l.strip_prefix("drover serve: phone page on http://"))
        {
            return a.trim_end_matches('/').to_string();
        }
        assert!(t.elapsed() < Duration::from_secs(30), "{l}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_second_serve_on_a_port_its_own_build_holds_exits_0_once() {
    let c = ctx(&tmpdir("ho-held"), CONFIG);
    let mut first = serve(&c, "127.0.0.1:0");
    let addr = addr_of(&lines(&mut first));
    get(&addr).unwrap();
    let mut second = serve(&c, &addr);
    let log = lines(&mut second);
    let code = exited(&mut second, 30);
    let still = first.try_wait().unwrap();
    let _ = (first.kill(), first.wait());
    let log = log.lock().unwrap().clone();
    assert_eq!(code.and_then(|s| s.code()), Some(0), "{log}");
    assert!(
        log.contains(&format!("{addr} already serves {:.7}", drover::SHA)),
        "{log}"
    );
    assert!(still.is_none(), "the serve holding the port is left alone");
    let starts = std::fs::read_to_string(c.state.with_file_name("serve-starts")).unwrap();
    assert_eq!(starts.lines().count(), 1, "no failed start: {starts}");
}

#[test]
fn a_serve_on_a_port_an_older_build_holds_has_it_hand_over_to_this_binary() {
    let c = ctx(&tmpdir("ho-old"), CONFIG);
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let sock = drover::update::serve_sock(&c, l.local_addr().unwrap().port());
    let ctl = std::os::unix::net::UnixListener::bind(&sock).unwrap();
    let env = |k: &str, v: &std::path::Path| format!("{k}={}", v.display()).into();
    let argv: Vec<std::ffi::OsString> = vec![
        "/usr/bin/env".into(),
        "-u".into(),
        "DROVER_LANE".into(),
        env("FLEET_CONFIG", &c.config),
        env("FLEET_STATE", &c.state),
        env("FLEET_WORKTREES", &c.worktrees),
        env("DROVER_HOME", &c.home),
        "PATH=/usr/bin:/bin".into(),
        env!("CARGO_BIN_EXE_drover").into(),
        "serve".into(),
        "--addr".into(),
        addr.clone().into(),
    ];
    let old = std::thread::spawn(move || {
        let (mut probe, _) = l.accept().unwrap();
        let mut req = String::new();
        let mut r = BufReader::new(&probe);
        while !req.ends_with("\r\n\r\n") && r.read_line(&mut req).unwrap() > 0 {}
        assert!(req.starts_with("GET /api/v1/health "), "{req}");
        let body = json!({"ok": true, "serve": {"sha": "0ld0ld0ld"}}).to_string();
        let res = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        probe.write_all(res.as_bytes()).unwrap();
        drop(probe);
        let (mut asker, _) = ctl.accept().unwrap();
        let mut line = String::new();
        BufReader::new(&asker).read_line(&mut line).unwrap();
        assert_eq!(line, "handover\n");
        drover::runner::hand_fds(&argv, &[std::os::fd::AsRawFd::as_raw_fd(&l)], true).unwrap();
        let (mut bridge, _) = ctl.accept().unwrap();
        let mut ready = String::new();
        BufReader::new(&bridge).read_line(&mut ready).unwrap();
        let pid = ready.trim().strip_prefix("ready ").unwrap().to_string();
        writeln!(bridge, "ok").unwrap();
        writeln!(asker, "ok {pid}").unwrap();
        pid
    });
    let mut new = serve(&c, &addr);
    let log = lines(&mut new);
    let code = exited(&mut new, 60);
    let bridge = old.join().unwrap();
    let health = drover::update::serving(&addr, 20);
    let mut again = serve(&c, &addr);
    let again_log = lines(&mut again);
    let again_code = exited(&mut again, 30);
    std::process::Command::new("kill").arg(&bridge).status().unwrap();
    let log = log.lock().unwrap().clone();
    assert_eq!(code.and_then(|s| s.code()), Some(0), "{log}");
    assert!(
        log.contains(&format!("{addr} ran 0ld0ld0; handed over (bridge pid {bridge})")),
        "{log}"
    );
    assert_eq!(health.as_deref(), Some(drover::SHA), "the port serves this build now");
    let again_log = again_log.lock().unwrap().clone();
    assert_eq!(
        again_code.and_then(|s| s.code()),
        Some(0),
        "no respawn loop: {again_log}"
    );
    assert!(again_log.contains("already serves"), "{again_log}");
}

#[test]
fn a_serve_started_by_a_handover_hands_over_again_past_its_memory_limit() {
    let cfg = |mb: u64| format!("{CONFIG}[limits]\nserve_host_secs = 1\nserve_mem_mb = {mb}\n");
    let c = ctx(&tmpdir("ho-mem-again"), &cfg(100_000));
    append(&c.state, &Event::new(c.now, "drover", None, "note", json!({}))).unwrap();
    let mut p = serve(&c, "127.0.0.1:0");
    let log = lines(&mut p);
    let addr = addr_of(&log);
    let port = addr.rsplit_once(':').unwrap().1.parse().unwrap();
    let sock = drover::update::serve_sock(&c, port);
    let t = Instant::now();
    while !sock.exists() && t.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(50));
    }
    let Ok(pid) = drover::update::ask_handover(&sock, 60) else {
        let _ = (p.kill(), p.wait());
        panic!("no handover: {}", log.lock().unwrap());
    };
    let t = Instant::now();
    while alive(&pid) && t.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(50));
    }
    let starts = || std::fs::read_to_string(c.state.with_file_name("serve-starts")).unwrap_or_default();
    let t = Instant::now();
    while starts().lines().count() < 3 && t.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(3_000));
    std::fs::write(&c.config, cfg(1)).unwrap();
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
    std::fs::write(&c.config, cfg(100_000)).unwrap();
    let t = Instant::now();
    while log.lock().unwrap().matches("phone page on").count() < 4 && t.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(1_000));
    let still = p.try_wait().unwrap();
    let _ = (p.kill(), p.wait());
    let (log, high, starts) = (log.lock().unwrap().clone(), high(), starts());
    assert!(!alive(&pid), "the first handover finished: {log}");
    assert!(
        starts.ends_with("handover\n"),
        "the serve on the port came from a handover: {starts}"
    );
    assert_eq!(
        high.first().map(|e| e.data["handover"].clone()),
        Some(json!(true)),
        "{high:?}"
    );
    assert_eq!(
        log.matches("phone page on").count(),
        4,
        "two bridges, two exec'd copies: {log}"
    );
    assert!(still.is_none(), "the supervised pid serves on\n{log}");
}
