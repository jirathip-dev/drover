//! #893: serve under load: with the database write-locked (a drive pass mid-write), a paired device's API v1 reads and
//! `/live` still answer at once; held `/live` streams never lock out a new one.
mod common;
use common::*;
use herdsman::events::{append, Event};
use serde_json::json;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

fn server(name: &str) -> (herdsman::Ctx, SocketAddr) {
    let c = ctx(&tmpdir(name), CONFIG);
    append(&c.state, &Event::new(1_000_000, "sendmeter", None, "note", json!({}))).unwrap();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || {
        herdsman::serve::serve_on(&c2, &Fake::default().reply("herdr agent list", NO_AGENTS), l, false)
    });
    (c, addr)
}

/// The first `want` bytes of `path`'s answer (all of it: 0) and how long they took; 2 s at most.
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
fn api_v1_reads_and_live_answer_within_200ms_with_the_db_write_locked() {
    let (c, addr) = server("load-locked");
    let now = herdsman::now();
    let r = Fake::default();
    let cookie = pair(
        &herdsman::serve::Server::new(&c, &r, Some("127.0.0.1:7777".into())),
        &c,
        "",
        now,
    );
    // The device was last seen over an hour ago: its first request rewrites last-seen.
    let db = rusqlite::Connection::open(herdsman::db::file(&c.state)).unwrap();
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
}

#[test]
fn eight_held_live_streams_do_not_lock_out_a_ninth() {
    let (_c, addr) = server("load-ninth");
    let open = |s: &mut TcpStream| {
        let mut b = [0u8; 4096];
        s.read(&mut b).map(|n| String::from_utf8_lossy(&b[..n]).into_owned())
    };
    // Phones gone without a FIN: their sockets stay open and never read again.
    let mut held: Vec<TcpStream> = (0..8)
        .map(|_| {
            let mut s = TcpStream::connect(addr).unwrap();
            s.write_all(format!("GET /live HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
                .unwrap();
            s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            assert!(open(&mut s).unwrap().starts_with("HTTP/1.1 200 OK"));
            s
        })
        .collect();
    let (r, d) = get(addr, "/live", "", 15);
    eprintln!("ninth /live: {d:?} {}", r.lines().next().unwrap_or(""));
    assert!(r.starts_with("HTTP/1.1 200 OK"), "the ninth: {r}");
    let mut first = held.remove(0);
    let end = Instant::now() + Duration::from_secs(2);
    while matches!(open(&mut first), Ok(s) if !s.is_empty()) && Instant::now() < end {}
    assert!(
        matches!(open(&mut first), Ok(s) if s.is_empty()),
        "the oldest stream is closed for it"
    );
}
