//! #715: a conductor cold start never freezes the board: while a start blocks (30 s here), the request that caused it
//! has its reply, and `GET /` and a second device's send answer at once; a failed background start shows its error in
//! the thread, the message that triggered it still logged (#717).
mod common;
use common::*;
use herdsman::runner::{Out, Runner};
use herdsman::serve::Server;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::time::{Duration, Instant};

const ADDR: &str = "127.0.0.1:7777";
const TIMEOUT: &str = r#"{"error":{"code":"timeout","message":"timed out waiting for agent startup"}}"#;

/// Thread t1 (conductor asleep: never started) with ask q1 from it.
fn setup(name: &str) -> herdsman::Ctx {
    let c = ctx(
        &tmpdir(name),
        &format!("{CONFIG}\n[fleet.sendmeter.role.conductor]\nharness = \"claude\"\nmodel = \"opus\"\n"),
    );
    let profile = c.config.with_file_name("claude").join("fleet-conductor");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    let f = Fake::default();
    assert_eq!(run(&c, &f, &["thread", "new", "sendmeter", "Look"]).1, "t1\n");
    let ask = [
        "ask",
        "sendmeter",
        "-",
        "Which?",
        "--option",
        "a",
        "--option",
        "b",
        "--thread",
        "t1",
    ];
    assert_eq!(run(&c, &f, &ask).1, "q1\n");
    c
}

fn herdr() -> Fake {
    Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply(
            "herdr workspace create",
            r#"{"result":{"root_pane":{"pane_id":"w9:p1"}}}"#,
        )
        .reply("herdr workspace close", "")
        .reply("herdr pane run", "")
        .reply("herdr agent start", "")
        .reply("herdr agent wait", "")
        .reply("herdr agent prompt", "")
}

/// `.0`, with `herdr agent start` blocking for 30 s (or until `.2`); `.1` once a start is under way.
struct Slow(Fake, AtomicBool, AtomicBool);
impl Slow {
    fn block(&self, prog: &str, args: &[String]) {
        if prog == "herdr" && args.starts_with(&["agent".into(), "start".into()]) {
            self.1.store(true, SeqCst);
            let t = Instant::now();
            while !self.2.load(SeqCst) && t.elapsed() < Duration::from_secs(30) {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}
impl Runner for Slow {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.block(prog, args);
        self.0.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.block(prog, args);
        self.0.run_in(prog, args, input)
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
}

/// One request over the socket, given 2 s: (ms, response text; empty when it timed out).
fn call(addr: SocketAddr, method: &str, path: &str, cookie: &str, body: &str) -> (u128, String) {
    let t = Instant::now();
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut res = vec![];
    let _ = s.read_to_end(&mut res);
    (t.elapsed().as_millis(), String::from_utf8_lossy(&res).into_owned())
}

#[test]
fn a_blocked_conductor_start_holds_no_request() {
    let c = setup("cold-start");
    let r = Slow(herdr(), AtomicBool::new(false), AtomicBool::new(false));
    let (c, r): (&'static herdsman::Ctx, &'static Slow) = (Box::leak(Box::new(c)), Box::leak(Box::new(r)));
    let pairing = Server::new(c, r, Some(ADDR.into()));
    let a = pair(&pairing, c, "Mozilla/5.0 (iPhone)", herdsman::now());
    let b = pair(&pairing, c, "Mozilla/5.0 (iPad)", herdsman::now());
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || herdsman::serve::serve_on(c, r, l, false));

    // Device A answers t1's ask: its conductor is asleep, so the answer wakes it (the start blocks).
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        drop(tx.send(call(
            addr,
            "POST",
            "/thread",
            &a,
            "op=answer&fleet=sendmeter&id=q1&choice=2",
        )))
    });
    let t = Instant::now();
    while !r.1.load(SeqCst) && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(r.1.load(SeqCst), "the answer never started t1's conductor");

    let (ms, res) = call(addr, "GET", "/", "", "");
    assert!(res.starts_with("HTTP/1.1 200"), "GET / during the start: {res:.300}");
    assert!(ms < 1000, "GET / took {ms} ms during the start");
    let (ms, res) = call(addr, "POST", "/thread", &b, "op=say&t=t1&text=hi");
    assert!(
        res.starts_with("HTTP/1.1 200"),
        "device B's send during the start: {res:.300}"
    );
    assert!(ms < 1000, "device B's send took {ms} ms during the start");
    // The answer itself was replied to, not held until the conductor is up.
    let (ms, res) = rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the answer's reply waits on the start");
    assert!(
        res.starts_with("HTTP/1.1 200") && res.ends_with("answered q1\n"),
        "{res:.300}"
    );
    assert!(ms < 1000, "the answer took {ms} ms");
    r.2.store(true, SeqCst);
}

#[test]
fn a_failed_background_start_shows_in_the_thread_its_message_kept() {
    let c = setup("cold-start-fail");
    let r = herdr()
        .once_err("herdr agent start", 1, TIMEOUT)
        .once_err("herdr agent start", 1, TIMEOUT);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", 1_000_000);
    let body = "op=answer&fleet=sendmeter&id=q1&choice=2";
    let req = format!(
        "POST /thread HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&req, 1_000_000)).into_owned();
    assert!(
        res.starts_with("HTTP/1.1 200") && res.ends_with("answered q1\n"),
        "{res}"
    );
    let log = herdsman::events::jsonl(&c.state).unwrap();
    assert!(
        log.contains(r#""text":"DECISION q1: b""#),
        "the message stays logged: {log}"
    );
    assert!(
        log.contains(r#""text":"✗ not delivered: herdr agent start failed: timed out waiting for agent startup""#),
        "{log}"
    );
}
