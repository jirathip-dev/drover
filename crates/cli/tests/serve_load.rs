mod common;
use common::*;
use drover::events::{append, Event};
use drover::runner::{Out, Runner};
use serde_json::json;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct SlowGh(OnShed, Arc<AtomicUsize>);

impl Runner for SlowGh {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        if prog == "gh" {
            self.1.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_secs(10));
        }
        self.0.run(prog, args)
    }
    fn shed(&self) -> Option<drover::shed::Shed> {
        self.0.shed()
    }
}

fn server(name: &str, config: &str) -> (drover::Ctx, SocketAddr, Arc<AtomicUsize>) {
    let c = ctx(&tmpdir(name), config);
    append(&c.state, &Event::new(1_000_000, "sendmeter", None, "note", json!({}))).unwrap();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let (c2, gh) = (c.clone(), Arc::new(AtomicUsize::new(0)));
    let r = SlowGh(OnShed::new(Fake::default()), gh.clone());
    std::thread::spawn(move || drover::serve::serve_on(&c2, &r, l, false));
    (c, addr, gh)
}

fn get(addr: SocketAddr, path: &str, cookie: &str, want: usize) -> (String, Duration) {
    let t = Instant::now();
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let _ = s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n{cookie}\r\n").as_bytes());
    let (mut out, mut b) = (vec![], [0u8; 65_536]);
    while let Ok(n @ 1..) = s.read(&mut b) {
        out.extend_from_slice(&b[..n]);
        if want > 0 && out.len() >= want {
            break;
        }
    }
    (String::from_utf8_lossy(&out).into_owned(), t.elapsed())
}

fn p95(mut v: Vec<Duration>) -> Duration {
    v.sort();
    v[(v.len() * 95).div_ceil(100) - 1]
}

#[test]
fn api_v1_reads_and_live_answer_within_200ms_with_the_db_write_locked_and_gh_slow() {
    let (c, addr, gh) = server("load-locked", CONFIG);
    let now = drover::now();
    let r = Fake::default();
    let cookie = pair(
        &drover::serve::Server::new(&c, &r, Some("127.0.0.1:7777".into())),
        &c,
        "",
        now,
    );
    let db = rusqlite::Connection::open(drover::db::file(&c.state)).unwrap();
    let old = "UPDATE devices SET json = json_set(json, '$.used', ?1)";
    db.execute(old, [now - 7200]).unwrap();
    db.execute_batch("BEGIN IMMEDIATE; INSERT INTO leases VALUES ('drive', 1, 'h', 0, 0, '')")
        .unwrap();
    let paths = ["state", "needs", "lanes", "queue", "herd", "chat"];
    let (mut api, mut live, mut status) = (vec![], vec![], vec![]);
    for round in 0..4 {
        for p in paths.iter().map(|p| format!("/api/v1/{p}")).chain(["/live".into()]) {
            let (r, d) = get(addr, &p, &cookie, 15);
            let line = r.lines().next().unwrap_or("(no answer)").to_string();
            eprintln!("round {round} {p} first bytes: {d:?} {line}");
            [&mut api, &mut live][usize::from(p == "/live")].push(d);
            status.push(line);
        }
    }
    db.execute_batch("COMMIT").unwrap();
    eprintln!(
        "p95 /api/v1 {:?}, p95 /live first byte {:?}",
        p95(api.clone()),
        p95(live.clone())
    );
    assert!(status.iter().all(|s| s == "HTTP/1.1 200 OK"), "{status:?}");
    assert!(p95(api) < Duration::from_millis(200));
    assert!(p95(live) < Duration::from_millis(200));
    assert_eq!(gh.load(Ordering::SeqCst), 0, "no read waited on gh");
}

#[test]
fn eight_silent_live_streams_are_dropped_after_two_missed_heartbeats_and_a_ninth_connects() {
    let (c, addr, _) = server("load-silent", &format!("{CONFIG}serve_heartbeat_secs = 1\n"));
    let fleet = "f".repeat(30_000);
    for n in 0..400 {
        append(&c.state, &Event::new(1_000_000 + n, &fleet, None, "note", json!({}))).unwrap();
    }
    let r = Fake::default();
    let s = drover::serve::Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    let now = drover::now();
    let silent: Vec<TcpStream> = (0..8)
        .map(|_| {
            let cookie = pair(&s, &c, "", now);
            let mut t = TcpStream::connect(addr).unwrap();
            let req = format!("GET /live?since=0 HTTP/1.1\r\nHost: {addr}\r\n{cookie}\r\n");
            t.write_all(req.as_bytes()).unwrap();
            t
        })
        .collect();
    let (end, mut open) = (Instant::now() + Duration::from_secs(8), silent);
    while !open.is_empty() && Instant::now() < end {
        open.retain_mut(|t| t.write_all(b"\n").is_ok());
        std::thread::sleep(Duration::from_millis(200));
    }
    eprintln!("silent streams closed {:?} before the deadline", end - Instant::now());
    assert!(open.is_empty(), "{} silent streams still open", open.len());
    let (r, d) = get(addr, "/live", "", 15);
    eprintln!("ninth /live: {d:?} {}", r.lines().next().unwrap_or(""));
    assert!(r.starts_with("HTTP/1.1 200 OK"), "the ninth: {r}");
}
