//! #265: the JSON API v1 (`/api/v1/…`, docs/serve.md): each endpoint's JSON golden-tested (`BLESS=1` rewrites
//! tests/golden/api/), the same Host/Origin and pairing refusals as the HTML board, and the HTML board unchanged.
mod common;
use common::*;
use herdsman::events::{append, Event};
use herdsman::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

fn req(s: &Server, method: &str, path: &str, body: &str, extra: &str) -> (String, Value) {
    let r = format!(
        "{method} {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&r, T)).into_owned();
    let (head, body) = res.split_once("\r\n\r\n").unwrap();
    assert!(head.contains("Content-Type: application/json"), "{res}");
    (
        head.lines().next().unwrap().to_string(),
        serde_json::from_str(body).unwrap(),
    )
}

/// `v` against tests/golden/api/<name>.json (`mtime` masked: the fixture's file time).
fn golden(name: &str, mut v: Value) {
    if v.get("mtime").is_some() {
        v["mtime"] = json!("<mtime>");
    }
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/golden/api/{name}.json"));
    let got = serde_json::to_string_pretty(&v).unwrap() + "\n";
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&file, &got).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&file).unwrap_or_default(),
        got,
        "{name}: `BLESS=1` rewrites it"
    );
}

/// sendmeter: #12 in work (a lane), #17 queued behind #12, #9 asking; herdr shows the impl lane working.
fn fixture(name: &str) -> (herdsman::Ctx, Fake) {
    let c = ctx(&tmpdir(name), CONFIG);
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "17"]).0,
        0
    );
    assert_eq!(
        run(
            &c,
            &Fake::default(),
            &["queue", "sendmeter", "block", "--on", "12", "17"]
        )
        .0,
        0
    );
    let ev = |ts, issue, kind, d| append(&c.state, &Event::new(ts, "sendmeter", Some(issue), kind, d)).unwrap();
    ev(
        900_100,
        12,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude"}),
    );
    ev(
        900_200,
        9,
        "needs-decision",
        json!({"id": "q7", "question": "Which DB?", "options": ["sqlite", "postgres"], "recommend": 1}),
    );
    ev(900_300, 12, "merged", json!({"pr": 40, "into": "staging"}));
    let list = agents(&[("sendmeter-impl-12", "working", "/x", "w1:p2")]);
    let r = Fake::default()
        .reply("herdr agent list", &list)
        .reply("herdr pane read", "⏺ Bash(cargo test)\n  ⎿ ok\n");
    (c, r)
}

#[test]
fn state_events_and_lane_json_match_the_goldens() {
    let (c, r) = fixture("api-get");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let (status, state) = req(&s, "GET", "/api/v1/state", "", "");
    assert_eq!(status, "HTTP/1.1 200 OK");
    golden("state", state);
    let (_, all) = req(&s, "GET", "/api/v1/events", "", "");
    golden("events", all);
    let (_, after) = req(&s, "GET", "/api/v1/events?since=4", "", "");
    assert_eq!(after["list"].as_array().unwrap().len(), 1, "{after}");
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone) Safari/604.1", T);
    let (status, lane) = req(&s, "GET", "/api/v1/lane/sendmeter-impl-12", "", &cookie);
    assert_eq!(status, "HTTP/1.1 200 OK");
    golden("lane", lane);
    let (status, none) = req(&s, "GET", "/api/v1/lane/nobody", "", &cookie);
    assert_eq!(
        (status.as_str(), &none["code"]),
        ("HTTP/1.1 404 Not Found", &json!("NOT_FOUND"))
    );
}

#[test]
fn over_500_new_events_ask_for_a_refresh() {
    let (c, r) = fixture("api-refresh");
    for i in 0..501 {
        append(
            &c.state,
            &Event::new(950_000 + i, "sendmeter", None, "driven", json!({})),
        )
        .unwrap();
    }
    let (_, v) = req(
        &Server::new(&c, &r, Some(ADDR.into())),
        "GET",
        "/api/v1/events?since=1",
        "",
        "",
    );
    assert_eq!((&v["refresh"], &v["list"]), (&json!(true), &json!([])), "{v}");
}

#[test]
fn refusals_are_json_with_a_code() {
    let (c, r) = fixture("api-refuse");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let code = |method, path, body, extra| {
        let (status, v) = req(&s, method, path, body, extra);
        assert_eq!(v["ok"], json!(false), "{v}");
        (status, v["code"].as_str().unwrap().to_string())
    };
    let want = |s: &str, c: &str| (format!("HTTP/1.1 {s}"), c.to_string());
    // Unpaired: no transcript, no paired op; a foreign Host or Origin: nothing at all.
    assert_eq!(
        code("GET", "/api/v1/lane/sendmeter-impl-12", "", ""),
        want("401 Unauthorized", "UNPAIRED")
    );
    assert_eq!(
        code("POST", "/api/v1/drive", "fleet=sendmeter", ""),
        want("401 Unauthorized", "UNPAIRED")
    );
    let evil = "Origin: https://evil.example\r\n";
    assert_eq!(
        code("GET", "/api/v1/state", "", evil),
        want("403 Forbidden", "FORBIDDEN")
    );
    assert_eq!(
        code("POST", "/api/v1/pause", "fleet=sendmeter", evil),
        want("403 Forbidden", "FORBIDDEN")
    );
    assert!(!r.called("herdr pane read"));
    // A stale page's mtime is REFUSED STALE, as on the HTML board; an unknown path is 404.
    assert_eq!(
        code("POST", "/api/v1/pause", "fleet=sendmeter&mtime=1", ""),
        want("409 Conflict", "STALE")
    );
    assert_eq!(code("GET", "/api/v2/state", "", ""), want("404 Not Found", "NOT_FOUND"));
    // Read-only (`serve --read-only`): GET only.
    let ro = Server::new(&c, &r, None);
    let (status, v) = req(&ro, "POST", "/api/v1/pause", "fleet=sendmeter", "");
    assert_eq!(
        (status.as_str(), &v["code"]),
        ("HTTP/1.1 405 Method Not Allowed", &json!("READ_ONLY"))
    );
}

#[test]
fn a_post_runs_the_boards_op_and_answers_json() {
    let (c, r) = fixture("api-post");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let mtime = req(&s, "GET", "/api/v1/state", "", "").1["mtime"]
        .as_str()
        .unwrap()
        .to_string();
    let (status, v) = req(
        &s,
        "POST",
        "/api/v1/pause",
        &format!("fleet=sendmeter&mtime={mtime}"),
        "",
    );
    assert_eq!(status, "HTTP/1.1 200 OK");
    golden("pause", v);
    assert_eq!(
        req(&s, "GET", "/api/v1/state", "", "").1["fleets"][0]["paused"],
        json!(true)
    );
    // The paired ops (answer) need a paired device, then run the CLI's own path.
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone) Safari/604.1", T);
    let mtime = req(&s, "GET", "/api/v1/state", "", "").1["mtime"].clone(); // the pause rewrote fleet.toml
    let body = format!("fleet=sendmeter&id=q7&choice=1&mtime={}", mtime.as_str().unwrap());
    let (status, v) = req(&s, "POST", "/api/v1/answer", &body, &cookie);
    assert_eq!((status.as_str(), &v["ok"]), ("HTTP/1.1 200 OK", &json!(true)), "{v}");
    assert_eq!(req(&s, "GET", "/api/v1/state", "", "").1["asks"], json!([]));
}
