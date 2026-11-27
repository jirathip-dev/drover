mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::sync::Arc;
use std::time::{Duration, Instant};

const TABS: [&str; 10] = [
    "/screens/queue",
    "/screens/lanes",
    "/screens/events",
    "/screens/threads",
    "/screens/threads/t1",
    "/screens/cards",
    "/screens/scene",
    "/screens/fleets",
    "/screens/workflow",
    "/screens/issue?issue=1&repo=sendmeter",
];

struct Board {
    c: drover::Ctx,
    addr: SocketAddr,
    cookies: Vec<String>,
}

fn board(name: &str) -> Board {
    let (c, sh) = large::large(name);
    let fake = Fake::default();
    let pairing = Server::new(&c, &fake, Some("127.0.0.1:7777".into()));
    let cookies = ["iPhone", "iPad", "Macintosh"]
        .iter()
        .map(|ua| pair(&pairing, &c, &format!("Mozilla/5.0 ({ua})"), drover::now()))
        .collect();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c2, &OnShed(Fake::default(), sh), l, false));
    Board { c, addr, cookies }
}

fn event(c: &drover::Ctx, i: u64) {
    let ask = json!({"id": format!("qx{i}"), "thread": "t1", "question": format!("change {i}: per key or per IP?"), "options": ["per key", "per ip"], "recommend": 1, "kind": "owner"});
    append(
        &c.state,
        &Event::new(drover::now() - 1, "sendmeter", None, "needs-decision", ask),
    )
    .unwrap();
}

fn get(b: &Board, path: &str, extra: &str) -> (String, String, String) {
    fetch(b.addr, &b.cookies[0], path, extra)
}

fn fetch(addr: SocketAddr, cookie: &str, path: &str, extra: &str) -> (String, String, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\n{cookie}{extra}Connection: close\r\n\r\n"
    )
    .unwrap();
    let mut r = String::new();
    s.read_to_string(&mut r).unwrap();
    let (head, body) = r.split_once("\r\n\r\n").unwrap();
    let tag = head.lines().find_map(|l| l.strip_prefix("ETag: ")).unwrap_or_default();
    (
        head.lines().next().unwrap().to_string(),
        tag.to_string(),
        body.to_string(),
    )
}

fn keys(b: &Board) -> Vec<Value> {
    let (status, _, body) = get(b, "/api/v1/health", "");
    assert!(status.contains("200"), "{status}: {body}");
    let h: Value = serde_json::from_str(&body).unwrap();
    h["serve"]["keys"].as_array().cloned().unwrap()
}

fn key<'k>(keys: &'k [Value], path: &str) -> &'k Value {
    keys.iter()
        .find(|k| k["key"] == path && k["paired"] == true)
        .unwrap_or_else(|| panic!("{path} not in the hub: {keys:?}"))
}

fn settled(b: &Board) -> Vec<Value> {
    let shape = |k: &[Value]| {
        k.iter()
            .map(|k| (k["key"].clone(), k["rev"].clone(), k["builds"].clone()))
            .collect::<Vec<_>>()
    };
    let mut last = keys(b);
    for _ in 0..60 {
        std::thread::sleep(Duration::from_secs(1));
        let now = keys(b);
        if shape(&now) == shape(&last) {
            return now;
        }
        last = now;
    }
    panic!("the hub never settled: {last:?}");
}

fn settled_early_in_a_clock_window(b: &Board) -> Vec<Value> {
    for _ in 0..3 {
        let k = settled(b);
        if drover::now() % 30 < 15 {
            return k;
        }
        std::thread::sleep(Duration::from_secs(31 - drover::now() % 30));
    }
    panic!("no quiet clock window");
}

struct Held {
    path: &'static str,
    deltas: Arc<AtomicUsize>,
}

fn open(b: &Board, path: &'static str, cookie: &str) -> Held {
    let mut s = TcpStream::connect(b.addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    let (base, q) = path.split_once('?').unwrap_or((path, ""));
    write!(s, "GET {base}/stream?{q} HTTP/1.1\r\nHost: {}\r\n{cookie}\r\n", b.addr).unwrap();
    let mut r = BufReader::new(s);
    let (mut status, mut line) = (String::new(), String::new());
    r.read_line(&mut status).unwrap();
    assert!(status.starts_with("HTTP/1.1 200"), "{path}: {status}");
    while !line.starts_with("data: ") {
        line.clear();
        assert!(r.read_line(&mut line).unwrap() > 0, "{path}: no first frame");
    }
    assert!(line.contains(r#""op":"screen""#), "{path}: {line}");
    let deltas = Arc::new(AtomicUsize::new(0));
    let n = deltas.clone();
    std::thread::spawn(move || {
        let mut line = String::new();
        while r.read_line(&mut line).unwrap_or(0) > 0 {
            if line.starts_with("data: ") {
                n.fetch_add(1, SeqCst);
            }
            line.clear();
        }
    });
    Held { path, deltas }
}

fn wait_for(what: &str, ok: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while !ok() {
        assert!(Instant::now() < end, "{what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn one_change_is_one_build_for_every_reader() {
    let b = board("hub-one-build");
    let held: Vec<Held> = (0..30)
        .map(|i| open(&b, "/screens/events", &b.cookies[i % 3]))
        .collect();
    let k = settled_early_in_a_clock_window(&b);
    let k = k
        .iter()
        .find(|k| k["holders"] == 30)
        .unwrap_or_else(|| panic!("no key held by 30: {k:?}"));
    let (path, before) = (k["key"].as_str().unwrap().to_string(), k["builds"].as_u64().unwrap());
    let seen: Vec<usize> = held.iter().map(|h| h.deltas.load(SeqCst)).collect();
    event(&b.c, 1);
    let gets: Vec<_> = (0..30)
        .map(|i| {
            let (addr, ck, p) = (b.addr, b.cookies[i % 3].clone(), path.clone());
            std::thread::spawn(move || fetch(addr, &ck, &p, ""))
        })
        .collect();
    for g in gets {
        let (status, _, body) = g.join().unwrap();
        assert!(status.contains("200") && body.contains("change 1"), "{status}");
    }
    for (h, n) in held.iter().zip(seen) {
        wait_for(&format!("a stream of {} never got the change", h.path), || {
            h.deltas.load(SeqCst) > n
        });
    }
    let after = key(&keys(&b), &path)["builds"].as_u64().unwrap();
    assert_eq!(after, before + 1, "30 streams and 30 GETs of {path}, one event");
}

#[test]
fn fresh_data_is_under_150ms_p95_on_every_screen_and_sheet() {
    let b = board("hub-fresh");
    let mut paths: Vec<String> = TABS.iter().map(ToString::to_string).collect();
    let mut i = 0;
    while let Some(p) = paths.get(i).cloned() {
        i += 1;
        let (status, _, body) = get(&b, &p, "");
        assert!(status.contains("200"), "{p}: {status} {body}");
        let mut more = vec![];
        sheets(&serde_json::from_str(&body).unwrap(), &mut more);
        more.retain(|m| !paths.contains(m));
        paths.extend(more);
    }
    let mut ms = vec![];
    for (n, p) in paths.iter().enumerate() {
        event(&b.c, u64::try_from(n).unwrap());
        let t = Instant::now();
        let (status, _, body) = get(&b, p, "");
        ms.push((t.elapsed().as_millis(), p.clone()));
        assert!(status.contains("200"), "{p}: {status} {body}");
    }
    ms.sort();
    let p95 = &ms[ms.len() * 95 / 100];
    eprintln!(
        "#1372 fresh data: {} screens and sheets, p95 {} ms ({}), slowest {:?}",
        ms.len(),
        p95.0,
        p95.1,
        ms.last().unwrap()
    );
    if cfg!(debug_assertions) {
        return eprintln!("debug build: the 150 ms budget applies in release");
    }
    assert!(p95.0 < 150, "p95 {} ms on {}", p95.0, p95.1);
}

fn sheets(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            let p = m.get("sheet").and_then(Value::as_str);
            let p = p.filter(|p| p.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._-?=&".contains(&b)));
            out.extend(
                p.filter(|p| p.starts_with("/screens/") && !out.iter().any(|o| o == p))
                    .map(String::from),
            );
            m.values().for_each(|x| sheets(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| sheets(x, out)),
        _ => {}
    }
}

#[test]
fn ten_screens_across_three_devices_get_no_503() {
    let b = board("hub-ten-screens");
    let held: Vec<Held> = b
        .cookies
        .iter()
        .flat_map(|ck| TABS.iter().map(move |p| (p, ck)))
        .map(|(p, ck)| open(&b, p, ck))
        .collect();
    let rev = |k: &[Value], p: &str| key(k, p)["rev"].as_u64().unwrap();
    let k0 = settled(&b);
    let seen: Vec<usize> = held.iter().map(|h| h.deltas.load(SeqCst)).collect();
    for i in 0..200 {
        event(&b.c, i);
        if i % 20 == 0 {
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    let k1 = settled(&b);
    assert!(rev(&k1, "/screens/events") > rev(&k0, "/screens/events"), "{k1:?}");
    for (h, n) in held.iter().zip(seen) {
        let want = usize::try_from(rev(&k1, h.path) - rev(&k0, h.path)).unwrap();
        wait_for(
            &format!("{}: {} of {want} deltas", h.path, h.deltas.load(SeqCst) - n),
            || h.deltas.load(SeqCst) - n == want,
        );
    }
}

#[test]
fn a_screen_get_answers_304_to_its_etag() {
    let b = board("hub-etag");
    let (status, tag, body) = get(&b, "/screens/events", "");
    assert!(status.contains("200") && !tag.is_empty(), "{status} {tag}");
    let (status, again, body2) = get(&b, "/screens/events", &format!("If-None-Match: {tag}\r\n"));
    assert!(status.contains("304"), "{status}");
    assert_eq!((again.as_str(), body2.as_str()), (tag.as_str(), ""));
    event(&b.c, 7);
    let (status, new, body3) = get(&b, "/screens/events", &format!("If-None-Match: {tag}\r\n"));
    assert!(status.contains("200"), "{status}");
    assert_ne!(new, tag);
    assert_ne!(body3, body);
    assert!(body3.contains("change 7"), "{body3}");
}
