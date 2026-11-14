mod common;
use common::*;
use drover::events::{append, Event};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Gate(Fake, Arc<AtomicBool>, &'static [u64]);
impl drover::runner::Runner for Gate {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        let line = args.join(" ");
        let held = |n: &u64| line.contains(&format!(" kick sendmeter {n} --why board"));
        if prog == "sh" && self.2.iter().any(held) {
            let t = Instant::now();
            while !self.1.load(SeqCst) && t.elapsed() < Duration::from_secs(30) {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        self.0.run(prog, args)
    }
    fn sleep(&self, _: u64) {}
}

struct Board {
    addr: SocketAddr,
    cookie: String,
    mtime: String,
    open: Arc<AtomicBool>,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn send(addr: SocketAddr, req: &str) -> String {
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out
}

fn board(name: &str, issues: &[u64], gated: &'static [u64]) -> Board {
    let c = ctx(&tmpdir(name), CONFIG);
    for &n in issues {
        let d = serde_json::json!({"code": "ROUND_LIMIT", "reason": "out of rounds"});
        append(&c.state, &Event::new(c.now, "sendmeter", Some(n), "needs-human", d)).unwrap();
    }
    let f = Fake::default();
    let s = drover::serve::Server::new(&c, &f, Some("127.0.0.1:7777".into()));
    let cookie = pair(&s, &c, "", now());
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let open = Arc::new(AtomicBool::new(false));
    let r = Gate(Fake::default(), open.clone(), gated);
    let c2 = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c2, &r, l, false));
    let page = send(
        addr,
        &format!("GET /api/v1/state HTTP/1.1\r\nHost: {addr}\r\n{cookie}\r\n"),
    );
    let i = page.find("\"mtime\":\"").unwrap() + 9;
    let mtime = page[i..].chars().take_while(char::is_ascii_digit).collect();
    Board {
        addr,
        cookie,
        mtime,
        open,
    }
}

impl Board {
    fn op(&self, op: &str, n: u64) -> String {
        let (addr, cookie) = (self.addr, &self.cookie);
        let body = format!("op={op}&fleet=sendmeter&issue={n}&why=x&mtime={}", self.mtime);
        let req = format!(
            "POST / HTTP/1.1\r\nHost: {addr}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        send(addr, &req)
    }

    fn live(&self) -> TcpStream {
        let mut live = TcpStream::connect(self.addr).unwrap();
        let req = format!("GET /live HTTP/1.1\r\nHost: {}\r\n{}\r\n", self.addr, self.cookie);
        live.write_all(req.as_bytes()).unwrap();
        live.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        live
    }
}

fn toasts(live: &mut TcpStream, seen: &mut String, n: usize) -> Vec<String> {
    let mut b = [0u8; 65_536];
    let all = |s: &str| s.matches("event: toast\ndata: ").count();
    while all(seen) < n {
        let k = live.read(&mut b).unwrap();
        assert!(k > 0, "the stream closed: {seen}");
        *seen += &String::from_utf8_lossy(&b[..k]);
    }
    let data = seen.split("event: toast\ndata: ").skip(1);
    data.map(|t| t.lines().next().unwrap_or_default().to_string()).collect()
}

#[test]
fn a_slow_write_on_one_issue_never_delays_another_and_one_issues_writes_run_in_order() {
    let b = board("serve-workers-order", &[1, 2], &[1]);
    let (mut live, mut seen) = (b.live(), String::new());
    assert!(b.op("park", 1).contains("park%20started"));
    assert!(b.op("override", 1).contains("override%20started"));
    assert!(b.op("park", 2).contains("park%20started"));
    assert_eq!(toasts(&mut live, &mut seen, 1), ["\"#2: park\""]);
    b.open.store(true, SeqCst);
    let t = toasts(&mut live, &mut seen, 3);
    assert_eq!(t[1], "\"#1: park\"", "{t:?}");
    assert!(t[2].contains("#1") || t[2].contains("override"), "{t:?}");
}

#[test]
fn two_hundred_reads_are_all_answered_while_four_writes_run() {
    let b = board("serve-workers-reads", &[1, 2, 3, 4], &[1, 2, 3, 4]);
    let mut live = b.live();
    for n in 1..=4 {
        assert!(b.op("park", n).contains("park%20started"));
    }
    let mut conns: Vec<TcpStream> = (0..200).map(|_| TcpStream::connect(b.addr).unwrap()).collect();
    let req = format!("GET / HTTP/1.1\r\nHost: {}\r\n\r\n", b.addr);
    for s in &mut conns {
        s.write_all(req.as_bytes()).unwrap();
    }
    let ok = conns.into_iter().filter(|s| {
        let mut out = String::new();
        let mut s = s.try_clone().unwrap();
        s.read_to_string(&mut out).unwrap();
        out.starts_with("HTTP/1.1 200")
    });
    assert_eq!(ok.count(), 200);
    b.open.store(true, SeqCst);
    assert_eq!(toasts(&mut live, &mut String::new(), 4).len(), 4);
}
