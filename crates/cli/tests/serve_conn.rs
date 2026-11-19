mod common;
use common::*;
use drover::runner::Out;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Condvar, Mutex};
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

#[derive(Default)]
struct Gate(Mutex<(usize, bool)>, Condvar);

impl drover::runner::Runner for Gate {
    fn run(&self, _: &str, _: &[String]) -> anyhow::Result<Out> {
        Ok(Out::default())
    }
    fn stream(&self, _: &str, _: &[String], _: &str) -> anyhow::Result<Box<dyn Read + Send>> {
        let mut g = self.0.lock().unwrap();
        g.0 += 1;
        self.1.notify_all();
        drop(self.1.wait_while(g, |g| !g.1).unwrap());
        Ok(Box::new(std::io::empty()))
    }
}

impl Gate {
    fn inside(&self) -> usize {
        self.0.lock().unwrap().0
    }
    fn wait_for(&self, n: usize) {
        let g = self.0.lock().unwrap();
        let (g, t) = self
            .1
            .wait_timeout_while(g, Duration::from_secs(20), |g| g.0 < n)
            .unwrap();
        assert!(!t.timed_out(), "{} of {n} handlers busy", g.0);
    }
    fn open(&self) {
        self.0.lock().unwrap().1 = true;
        self.1.notify_all();
    }
}

#[test]
fn the_65th_request_waits_until_one_of_64_busy_handlers_is_freed() {
    let c: &'static drover::Ctx = Box::leak(Box::new(ctx(&tmpdir("conn-cap"), CONFIG)));
    let png = c.worktrees.join("sendmeter/issue-12/docs/design/meter/1440.png");
    std::fs::create_dir_all(png.parent().unwrap()).unwrap();
    std::fs::write(&png, b"png").unwrap();
    let files = serde_json::json!([{"name": "docs/design/meter/1440.png", "path": png}]);
    let ev = |kind: &str, d: serde_json::Value| {
        drover::events::append(
            &c.state,
            &drover::events::Event::new(c.now, "sendmeter", Some(12), kind, d),
        )
        .unwrap();
    };
    ev(
        "design-review",
        serde_json::json!({"sha": "abc123", "files": files, "role": "design"}),
    );
    ev(
        "needs-decision",
        serde_json::json!({"id": "q1", "question": "Design for #12 is ready: approve, revise (type a note) or reject?",
            "options": ["approve", "revise", "reject"], "recommend": null, "kind": "owner", "design": true, "files": files, "role": "design"}),
    );
    let gate: &'static Gate = Box::leak(Box::default());
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || drover::serve::serve_on(c, gate, l, false));
    let call = move || std::thread::spawn(move || get(addr, "/design/sendmeter/12/0/q1"));
    let first: Vec<_> = (0..64).map(|_| call()).collect();
    gate.wait_for(64);
    let last = call();
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(gate.inside(), 64, "the 65th was answered while 64 were busy");
    assert!(!last.is_finished());
    gate.open();
    for h in first.into_iter().chain([last]) {
        let res = h.join().unwrap();
        assert!(
            res.starts_with("HTTP/1.1 200"),
            "{}",
            res.lines().next().unwrap_or_default()
        );
    }
    assert_eq!(gate.inside(), 65);
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
