mod common;
use common::*;
use drover::events::Event;
use serde_json::json;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Instant;

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn large() -> SocketAddr {
    let c = ctx(&tmpdir("wire"), CONFIG);
    let ev = |ts, n, kind, d| serde_json::to_string(&Event::new(ts, "sendmeter", Some(n), kind, d)).unwrap();
    let mut log = vec![];
    for n in 1..=10u64 {
        let agent = format!("sendmeter-impl-{n}");
        let d = json!({"role": "impl", "agent": agent, "pane": agent, "runtime": "shed", "worktree": format!("/wt/{agent}")});
        log.push(ev(now() - 9_000 + n, n, "spawn", d));
    }
    for i in 0..1_600u64 {
        let n = 1 + i % 160;
        let (kind, d) = match i % 4 {
            0 => (
                "verdict",
                json!({"verdict": "FAIL", "round": i / 160, "sha": "a".repeat(40)}),
            ),
            1 => ("needs-human", json!({"reason": format!("round {i} over the limit")})),
            2 => ("merged", json!({"into": "staging", "pr": (1_000 + i).to_string()})),
            _ => (
                "spawn",
                json!({"role": "rev", "agent": format!("sendmeter-rev-{n}-r{i}"), "runtime": "shed"}),
            ),
        };
        log.push(ev(now() - 5_000 + i, n, kind, d));
    }
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    std::fs::write(&c.state, log.join("\n") + "\n").unwrap();
    let r: &'static Fake = Box::leak(Box::new(Fake::default()));
    let c: &'static drover::Ctx = Box::leak(Box::new(c));
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || drover::serve::serve_on(c, r, l, false));
    addr
}

fn get(addr: SocketAddr, path: &str, extra: &str) -> (u128, Vec<u8>) {
    let t = Instant::now();
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n{extra}Connection: close\r\n\r\n").as_bytes())
        .unwrap();
    let mut res = vec![];
    s.read_to_end(&mut res).unwrap();
    (t.elapsed().as_millis(), res)
}

#[test]
fn wire_report() {
    let addr = large();
    get(addr, "/screens/events", "");
    for (name, extra) in [("plain", ""), ("gzip", "Accept-Encoding: gzip\r\n")] {
        let mut runs: Vec<_> = (0..5).map(|_| get(addr, "/screens/events", extra)).collect();
        runs.sort_by_key(|r| r.0);
        let (ms, res) = &runs[2];
        assert!(res.starts_with(b"HTTP/1.1 200"), "{}", String::from_utf8_lossy(res));
        eprintln!("wire /screens/events {name}: {} bytes, median {ms} ms of 5", res.len());
    }
}
