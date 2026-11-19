mod common;
use common::*;
use drover::serve::Server;
use serde_json::Value;

const BUDGET_MS: u64 = 150;
const KINDS: [(&str, &str); 5] = [
    ("tab", "a tab switch"),
    ("sheet", "a lane's sheet"),
    ("tap", "the thread opening"),
    ("send", "a send"),
    ("answer", "an answer"),
];

fn health(addr: std::net::SocketAddr) -> Value {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    write!(
        s,
        "GET /api/v1/health HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut r = String::new();
    s.read_to_string(&mut r).unwrap();
    serde_json::from_str(r.split_once("\r\n\r\n").unwrap().1).unwrap()
}

#[test]
fn tab_sheet_thread_send_and_answer_paint_within_150ms_on_the_large_board_in_webkit() {
    if no_webkit() {
        return;
    }
    let _lock = ui_lock();
    let (c, sh) = large::large("phone-timings");
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
    let o = std::process::Command::new("node")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ui/timings.mjs"))
        .arg(format!("http://{addr}/"))
        .arg(ck.trim().trim_start_matches("Cookie: "))
        .arg("3")
        .output()
        .unwrap();
    let out: Value =
        serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&o.stderr)));
    assert!(
        out.get("error").is_none() && out["errors"] == serde_json::json!([]),
        "{out}"
    );
    let h = health(addr);
    let phone = h["serve"]["phone"].as_array().cloned().unwrap_or_default();
    let mut over = vec![];
    for (kind, what) in KINDS {
        let k = phone.iter().find(|p| p["route"] == kind);
        let k = k.unwrap_or_else(|| panic!("no {kind} in serve's health: {h} (the page sent {})", out["batch"]));
        let (n, p50, max) = (
            k["count"].as_u64().unwrap(),
            k["p50"].as_u64().unwrap(),
            k["max"].as_u64().unwrap(),
        );
        eprintln!("#1312 phone timings: {what} ({kind}) median {p50} ms, max {max} ms over {n}");
        assert!(n >= 3, "{kind}: {k}");
        if p50 > BUDGET_MS {
            over.push(format!("{what} {p50} ms"));
        }
    }
    let delta = phone.iter().find(|p| p["route"] == "delta");
    assert!(
        delta.is_some_and(|d| d["count"].as_u64() >= Some(1)),
        "no stream delta timed: {h}"
    );
    assert!(
        over.is_empty(),
        "over the {BUDGET_MS} ms budget (median): {over:?}; the page sent {}",
        out["batch"]
    );
}
