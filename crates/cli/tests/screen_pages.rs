mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};

const NOW: u64 = 1_000_000;

fn get(s: &Server, path: &str) -> Value {
    let r = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n\r\n");
    let res = String::from_utf8_lossy(&s.respond(&r, NOW)).into_owned();
    let (head, body) = res.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("HTTP/1.1 200"), "{path}: {res}");
    serde_json::from_str(body).unwrap()
}

fn find<'a>(v: &'a Value, id: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) if m.get("id").and_then(Value::as_str) == Some(id) => Some(v),
        Value::Object(m) => m.values().find_map(|x| find(x, id)),
        Value::Array(a) => a.iter().find_map(|x| find(x, id)),
        _ => None,
    }
}

fn pages(s: &Server, first: &str, id: &str, key: &str) -> Vec<Vec<String>> {
    let (mut out, mut path) = (vec![], Some(first.to_string()));
    while let Some(p) = path {
        let v = get(s, &p);
        let b = find(&v, id).unwrap_or_else(|| panic!("no {id} in {v}"));
        let rows = b[key].as_array().unwrap();
        let ids: Vec<String> = rows
            .iter()
            .filter(|r| r["type"] != "text")
            .map(|r| r["id"].as_str().or(r["name"].as_str()).unwrap().to_string())
            .collect();
        assert!(ids.len() <= 100, "{p}: {} rows", ids.len());
        path = b["more"].as_str().map(String::from);
        if let Some(m) = &path {
            assert!(m.contains("after="), "{m}");
            assert_eq!(ids.len(), 100, "{p}: a page with more after it is full");
        }
        out.push(ids);
        assert!(out.len() < 20, "endless pages");
    }
    out
}

fn whole(pages: &[Vec<String>]) -> Vec<String> {
    let all: Vec<String> = pages.concat();
    assert_eq!(
        all.iter().collect::<BTreeSet<_>>().len(),
        all.len(),
        "a row on two pages: {all:?}"
    );
    all
}

fn write_log(c: &drover::Ctx, events: &[Event]) {
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let lines: Vec<String> = events.iter().map(|e| serde_json::to_string(e).unwrap()).collect();
    std::fs::write(&c.state, lines.join("\n") + "\n").unwrap();
}

fn verdicts(base: u64, n: u64) -> Vec<Event> {
    (0..n)
        .map(|i| {
            let d = json!({"verdict": if i.is_multiple_of(3) { "FAIL" } else { "PASS" }, "round": 1});
            Event::new(base - 5_000 + i * 10, "sendmeter", Some(1000 + i), "verdict", d)
        })
        .collect()
}

#[test]
fn events_come_in_pages_of_100_by_cursor_with_no_gap_or_duplicate_and_filters_hold_across_pages() {
    let c = ctx(&tmpdir("pages-events"), CONFIG);
    let mut log = verdicts(NOW, 250);
    for i in 0..30 {
        let d = json!({"role": "impl", "agent": format!("sendmeter-impl-{i}")});
        log.push(Event::new(NOW - 4_000 + i * 7, "sendmeter", Some(i), "spawn", d));
    }
    write_log(&c, &log);
    let f = Fake::default();
    let s = Server::new(&c, &f, Some("127.0.0.1:7777".into()));

    let all = pages(&s, "/screens/events", "ev", "rows");
    assert_eq!(all.iter().map(Vec::len).collect::<Vec<_>>(), [100, 100, 80]);
    let ids = whole(&all);
    let seqs: Vec<u64> = ids.iter().map(|i| i[2..].parse().unwrap()).collect();
    assert!(seqs.windows(2).all(|w| w[0] > w[1]), "newest first: {seqs:?}");

    let lanes = pages(&s, "/screens/events?kind=lane", "ev", "rows");
    assert_eq!(whole(&lanes).len(), 30);
    let first = get(&s, "/screens/events?kind=merge,verdict");
    let more = find(&first, "ev").unwrap()["more"].as_str().unwrap().to_string();
    assert!(more.contains("kind=merge%2Cverdict"), "{more}");
    let verdict = whole(&pages(&s, "/screens/events?kind=merge,verdict", "ev", "rows"));
    assert_eq!(verdict.len(), 250);
    let titles: Vec<Value> = [&first, &get(&s, &more)]
        .iter()
        .flat_map(|v| find(v, "ev").unwrap()["rows"].as_array().unwrap().clone())
        .collect();
    assert!(
        titles
            .iter()
            .all(|r| r["title"].as_str().unwrap().starts_with("review ")),
        "{titles:?}"
    );
}

#[test]
fn done_today_and_the_backlog_come_in_pages_of_100_with_their_filters() {
    let c = ctx(&tmpdir("pages-done"), CONFIG);
    let merged: Vec<Event> = (1..=130u64)
        .map(|n| {
            let d = json!({"pr": n.to_string(), "into": "staging", "merge_sha": "abcdef0123"});
            Event::new(NOW - 10_000 + n / 3, "sendmeter", Some(n), "merged", d)
        })
        .collect();
    write_log(&c, &merged);
    let issue = |n: u64| {
        json!({"number": n, "title": format!("t{n}"), "state": "OPEN", "created": "2026-01-01T00:00:00Z",
            "updated": "2026-01-02T00:00:00Z", "labels": [], "ac": n.is_multiple_of(2), "pr": false})
    };
    let issues: serde_json::Map<String, Value> = (200..430u64).map(|n| (n.to_string(), issue(n))).collect();
    let cache = drover::gh::cache_path(&c, "backlog", "sendmeter", 0);
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(&cache, json!({"ts": NOW - 60, "issues": issues}).to_string()).unwrap();
    let f = Fake::default();
    let s = Server::new(&c, &f, Some("127.0.0.1:7777".into()));

    let done = pages(&s, "/screens/queue/done?repo=sendmeter", "done-list", "rows");
    assert_eq!(done.iter().map(Vec::len).collect::<Vec<_>>(), [100, 30]);
    let mut got: Vec<u64> = whole(&done)
        .iter()
        .map(|i| i.split('-').nth(1).unwrap().parse().unwrap())
        .collect();
    got.sort_unstable();
    assert_eq!(got, (1..=130).collect::<Vec<_>>());

    let bl = pages(&s, "/screens/backlog?repo=sendmeter", "bl-sendmeter", "fields");
    assert_eq!(bl.iter().map(Vec::len).collect::<Vec<_>>(), [100, 100, 30]);
    let picks = whole(&bl);
    assert_eq!(picks.first().map(String::as_str), Some("sendmeter.pick.429"));
    assert_eq!(picks.last().map(String::as_str), Some("sendmeter.pick.200"));

    let ac = whole(&pages(
        &s,
        "/screens/backlog?repo=sendmeter&on=ac",
        "bl-sendmeter",
        "fields",
    ));
    assert_eq!(ac.len(), 115);
    assert!(ac
        .iter()
        .all(|p| p.rsplit('.').next().unwrap().parse::<u64>().unwrap().is_multiple_of(2)));
}

fn next(r: &mut BufReader<std::net::TcpStream>) -> Vec<Value> {
    let mut line = String::new();
    loop {
        line.clear();
        assert!(r.read_line(&mut line).unwrap() > 0, "stream closed");
        if let Some(d) = line.strip_prefix("data: ") {
            let v: Value = serde_json::from_str(d).unwrap();
            return v["ops"].as_array().unwrap().clone();
        }
    }
}

#[test]
fn a_stream_delta_puts_a_new_event_on_top_and_drops_no_loaded_row() {
    let c = ctx(&tmpdir("pages-stream"), CONFIG);
    write_log(&c, &verdicts(drover::now(), 150));
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c2, &Fake::default(), l, false));
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(20))).unwrap();
    write!(s, "GET /screens/events/stream HTTP/1.1\r\nHost: {addr}\r\n\r\n").unwrap();
    let mut r = BufReader::new(s);
    let ops = next(&mut r);
    let ev = find(&ops[0]["screen"], "ev").unwrap().clone();
    let rows = ev["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 100);
    let top = rows[0]["id"].clone();

    let d = json!({"verdict": "PASS", "round": 2});
    append(&c.state, &Event::new(drover::now(), "sendmeter", Some(7), "verdict", d)).unwrap();
    let ops = next(&mut r);
    assert!(
        ops.iter().all(|o| o["op"] != "remove" && o["op"] != "screen"),
        "{ops:?}"
    );
    assert!(ops.iter().all(|o| o["id"] != "ev"), "the list's cursor stays: {ops:?}");
    let insert = ops
        .iter()
        .find(|o| o["op"] == "insert")
        .unwrap_or_else(|| panic!("{ops:?}"));
    assert_eq!((&insert["into"], &insert["before"]), (&json!("ev"), &top), "{ops:?}");
    assert_eq!(insert["block"]["title"], "review PASS · round 2");
}

#[test]
fn scrolling_the_events_tab_to_its_end_appends_the_next_page_in_place_and_it_paints_within_150ms_in_webkit() {
    if no_webkit() {
        return;
    }
    let _lock = ui_lock();
    let (c, sh) = large::large("pages-webkit");
    for e in verdicts(drover::now(), 300) {
        append(&c.state, &e).unwrap();
    }
    let ck = pair(
        &Server::new(&c, &Fake::default(), Some("127.0.0.1:7777".into())),
        &c,
        "Mozilla/5.0 (iPhone)",
        drover::now(),
    );
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c2, &OnShed(Fake::default(), sh), l, false));
    let mut h = std::net::TcpStream::connect(addr).unwrap();
    write!(
        h,
        "GET /screens/events HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut body = String::new();
    h.read_to_string(&mut body).unwrap();
    assert!(body.len() < 100_000, "the first page is {} bytes", body.len());
    let o = std::process::Command::new("node")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ui/pages.mjs"))
        .arg(format!("http://{addr}/"))
        .arg(ck.trim().trim_start_matches("Cookie: "))
        .output()
        .unwrap();
    let out: Value =
        serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&o.stderr)));
    assert!(out.get("error").is_none() && out["errors"] == json!([]), "{out}");
    assert_eq!(out["first"], 100, "{out}");
    assert_eq!(out["after"], 200, "{out}");
    assert_eq!(
        out["moved"],
        json!([0, 0]),
        "scroll y and a row's top before and after the append: {out}"
    );
    assert_eq!((&out["total"], &out["unique"]), (&json!(313), &json!(313)), "{out}");
    let mut ms: Vec<u64> = out["paints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    ms.sort_unstable();
    let p95 = ms[(ms.len() * 95).div_ceil(100) - 1];
    eprintln!("#1316 events first paint: p95 {p95} ms over {} ({ms:?})", ms.len());
    assert!(ms.len() >= 10 && p95 <= 150, "events first paint p95 {p95} ms: {ms:?}");
}
