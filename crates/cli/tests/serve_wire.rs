mod common;
use common::*;
use drover::events::Event;
use flate2::write::GzDecoder;
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

fn large() -> (&'static drover::Ctx, &'static Fake, SocketAddr) {
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
    (c, r, addr)
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
    let (_, _, addr) = large();
    get(addr, "/screens/events", "");
    for (name, extra) in [("plain", ""), ("gzip", "Accept-Encoding: gzip\r\n")] {
        let mut runs: Vec<_> = (0..5).map(|_| get(addr, "/screens/events", extra)).collect();
        runs.sort_by_key(|r| r.0);
        let (ms, res) = &runs[2];
        assert!(res.starts_with(b"HTTP/1.1 200"), "{}", String::from_utf8_lossy(res));
        eprintln!("wire /screens/events {name}: {} bytes, median {ms} ms of 5", res.len());
    }
}

fn split(res: &[u8]) -> (String, &[u8]) {
    let i = res.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    (String::from_utf8_lossy(&res[..i]).into_owned(), &res[i..])
}

fn gunzip(b: &[u8]) -> Vec<u8> {
    let mut d = GzDecoder::new(vec![]);
    d.write_all(b).unwrap();
    d.finish().unwrap()
}

#[test]
fn gzip_shrinks_the_events_screen_over_five_times_and_decodes_to_the_plain_body() {
    let (_, _, addr) = large();
    let (_, plain) = get(addr, "/screens/events", "");
    let (_, packed) = get(addr, "/screens/events", "Accept-Encoding: gzip, deflate, br\r\n");
    let ((ph, pb), (zh, zb)) = (split(&plain), split(&packed));
    assert!(!ph.contains("Content-Encoding"), "{ph}");
    assert!(zh.contains("\r\nContent-Encoding: gzip\r\n"), "{zh}");
    assert!(zh.contains(&format!("\r\nContent-Length: {}\r\n", zb.len())), "{zh}");
    assert!(
        pb.len() > 10_000 && zb.len() * 5 <= pb.len(),
        "{} vs {}",
        pb.len(),
        zb.len()
    );
    let rows = |b: &[u8]| {
        serde_json::from_slice::<serde_json::Value>(b)
            .unwrap()
            .to_string()
            .matches("\"type\":\"row\"")
            .count()
    };
    assert!(rows(pb) >= 100);
    assert_eq!(rows(&gunzip(zb)), rows(pb));
    let (_, refused) = get(addr, "/screens/events", "Accept-Encoding: gzip;q=0\r\n");
    assert!(!split(&refused).0.contains("Content-Encoding"));
}

#[test]
fn without_gzip_a_response_is_the_bytes_the_server_built() {
    let (c, r, addr) = large();
    let s = drover::serve::Server::new(c, r, Some(addr.to_string()));
    let page = String::from_utf8(get(addr, "/", "").1).unwrap();
    let v = page.split("/a/").nth(1).unwrap().split('/').next().unwrap();
    for path in [format!("/a/{v}/board.js"), "/sw.js".into(), "/nowhere".into()] {
        let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
        let (_, wire) = get(addr, &path, "");
        assert_eq!(
            String::from_utf8_lossy(&wire),
            String::from_utf8_lossy(&s.respond(&req, now())),
            "{path}"
        );
        assert!(split(&wire).0.contains("\r\nConnection: close\r\n"));
    }
    let js = format!("/a/{v}/board.js");
    let (a, b) = (
        get(addr, &js, "Accept-Encoding: gzip\r\n").1,
        get(addr, &js, "Accept-Encoding: gzip\r\n").1,
    );
    assert_eq!(a, b);
    assert!(split(&a).0.contains("Content-Encoding: gzip"));
    assert_eq!(gunzip(split(&a).1), split(&get(addr, &js, "").1).1);
}

fn first_message(addr: SocketAddr, path: &str) -> (u128, String, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nAccept-Encoding: gzip\r\n\r\n").as_bytes())
        .unwrap();
    let (mut raw, mut b, mut t) = (vec![], vec![0u8; 65_536], None);
    let mut d = GzDecoder::new(vec![]);
    let end = Instant::now() + std::time::Duration::from_secs(10);
    while Instant::now() < end {
        let n = s.read(&mut b).unwrap();
        assert!(n > 0, "stream ended");
        raw.extend_from_slice(&b[..n]);
        let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let t = *t.get_or_insert_with(Instant::now);
        d.write_all(&raw[i + 4..]).unwrap();
        raw.truncate(i + 4);
        d.flush().unwrap();
        let text = String::from_utf8_lossy(d.get_ref()).into_owned();
        if text.contains("\n\n") {
            return (
                t.elapsed().as_millis(),
                String::from_utf8_lossy(&raw).into_owned(),
                text,
            );
        }
    }
    panic!("no whole message in 10 s");
}

#[test]
fn a_gzipped_stream_message_decodes_whole_while_the_stream_stays_open() {
    let (_, _, addr) = large();
    let (ms, head, text) = first_message(addr, "/screens/events/stream");
    assert!(
        head.contains("text/event-stream") && head.contains("Content-Encoding: gzip"),
        "{head}"
    );
    assert!(text.starts_with("id: ") && text.contains("\"op\":\"screen\""), "{text}");
    assert!(ms < 100, "{ms} ms after the head");
    let (ms, head, text) = first_message(addr, "/live");
    assert!(head.contains("Content-Encoding: gzip"), "{head}");
    assert!(text.starts_with(": ping\n\n"), "{text}");
    assert!(ms < 100, "{ms} ms after the head");
}

fn response(s: &mut TcpStream) -> String {
    let mut res = vec![];
    let mut b = [0u8; 1];
    while !res.ends_with(b"\r\n\r\n") {
        assert_eq!(s.read(&mut b).unwrap(), 1, "closed mid-head");
        res.push(b[0]);
    }
    let head = String::from_utf8(res).unwrap();
    let n: usize = head
        .split("Content-Length: ")
        .nth(1)
        .unwrap()
        .split("\r\n")
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let mut body = vec![0u8; n];
    s.read_exact(&mut body).unwrap();
    head
}

#[test]
fn two_plain_http11_requests_over_one_connection_both_succeed_then_close_ends_it() {
    let (_, _, addr) = large();
    let mut s = TcpStream::connect(addr).unwrap();
    let req = |c: &str| format!("GET /screens/lanes HTTP/1.1\r\nHost: {addr}\r\n{c}\r\n");
    for _ in 0..2 {
        s.write_all(req("").as_bytes()).unwrap();
        let head = response(&mut s);
        assert!(
            head.starts_with("HTTP/1.1 200") && head.contains("Connection: keep-alive"),
            "{head}"
        );
    }
    s.write_all(req("Connection: close\r\n").as_bytes()).unwrap();
    assert!(response(&mut s).contains("Connection: close"));
    assert_eq!(s.read(&mut [0u8; 1]).unwrap(), 0);
}
