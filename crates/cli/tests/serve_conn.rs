mod common;
use common::*;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

fn board() -> SocketAddr {
    let c: &'static drover::Ctx = Box::leak(Box::new(ctx(&tmpdir("conn"), CONFIG)));
    let r: &'static Fake = Box::leak(Box::new(Fake::default()));
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || drover::serve::serve_on(c, r, l, false));
    get(addr, "/screens/lanes");
    addr
}

fn get(addr: SocketAddr, path: &str) -> String {
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n").as_bytes())
        .unwrap();
    let mut res = vec![];
    s.read_to_end(&mut res).unwrap();
    String::from_utf8_lossy(&res).into_owned()
}

fn half(addr: SocketAddr) -> TcpStream {
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(b"GET /screens/lanes HTTP/1.1\r\nHost: x\r\n").unwrap();
    s
}

fn stream(addr: SocketAddr) -> TcpStream {
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(format!("GET /screens/lanes/stream HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
        .unwrap();
    let mut b = [0u8; 12];
    s.read_exact(&mut b).unwrap();
    assert_eq!(&b, b"HTTP/1.1 200");
    s
}

#[test]
fn a_slow_request_does_not_hold_up_a_fast_one() {
    let addr = board();
    let _held = stream(addr);
    let _slow: Vec<_> = (0..100).map(|_| half(addr)).collect();
    let t = Instant::now();
    let res = get(addr, "/screens/lanes");
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
}

#[test]
fn the_65th_request_waits_and_gets_its_answer() {
    let addr = board();
    let all: Vec<_> = (0..80)
        .map(|_| std::thread::spawn(move || get(addr, "/screens/lanes")))
        .collect();
    for h in all {
        let res = h.join().unwrap();
        assert!(
            res.starts_with("HTTP/1.1 200"),
            "{}",
            res.lines().next().unwrap_or_default()
        );
    }
}

#[test]
fn a_client_that_sends_half_a_request_is_dropped_after_5s() {
    let addr = board();
    let mut s = half(addr);
    s.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
    let t = Instant::now();
    let closed = loop {
        match s.read(&mut [0u8; 1]) {
            Ok(0) => break t.elapsed(),
            Err(e) if e.kind() == ErrorKind::ConnectionReset => break t.elapsed(),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                assert!(t.elapsed() < Duration::from_secs(9), "still open after 9 s");
                if s.write_all(b"X-Slow: 1\r\n").is_err() {
                    break t.elapsed();
                }
            }
            other => panic!("{other:?}"),
        }
    };
    assert!(closed >= Duration::from_millis(4_500), "{closed:?}");
    let res = get(addr, "/screens/lanes");
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
}
