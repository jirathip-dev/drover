mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

fn get(s: &Server, path: &str, extra: &str) -> String {
    let res = s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}\r\n"), T);
    let res = String::from_utf8_lossy(&res).into_owned();
    assert!(
        res.starts_with("HTTP/1.1 200"),
        "{path}: {}",
        &res[..res.len().min(300)]
    );
    res[res.find("\r\n\r\n").unwrap() + 4..].to_string()
}

#[test]
fn the_events_api_reads_a_seq_window() {
    let c = ctx(&tmpdir("events-pages"), CONFIG);
    for i in 0..250 {
        let d = json!({"reason": format!("n-{i}")});
        append(&c.state, &Event::new(T - 10_000 + i * 10, "sendmeter", None, "note", d)).unwrap();
    }
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let w: Value = serde_json::from_str(&get(&s, "/api/v1/events?since=100&until=150", "")).unwrap();
    let seqs: Vec<u64> = w["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["seq"].as_u64().unwrap())
        .collect();
    assert_eq!((seqs.len(), seqs[0], seqs[49], &w["seq"]), (50, 101, 150, &json!(250)));
}
