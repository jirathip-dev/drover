mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};

const ADDR: &str = "127.0.0.1:7777";

fn threads(name: &str) -> (drover::Ctx, u64) {
    let c = ctx(
        &tmpdir(name),
        &format!("{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\n"),
    );
    let profile = c.config.with_file_name("claude").join("fleet-orchestrator");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    let now = drover::now();
    let ev = |ts: u64, kind: &str, d: Value| append(&c.state, &Event::new(ts, "sendmeter", None, kind, d)).unwrap();
    for (t, title) in [("t2", "Rate limits"), ("t3", "Dark mode")] {
        ev(
            now - 7_200,
            "thread-new",
            json!({"thread": t, "title": title, "harness": "claude"}),
        );
    }
    for i in 0..40u64 {
        let from = if i % 2 == 0 { "owner" } else { "orchestrator" };
        let d = json!({"thread": "t2", "from": from, "text": format!("message {i}")});
        ev(now - 7_000 + i, "thread-msg", d);
    }
    ev(
        now - 100,
        "thread-msg",
        json!({"thread": "t3", "from": "orchestrator", "text": "done"}),
    );
    (c, now)
}

fn get(s: &Server, now: u64, path: &str) -> Value {
    let res = s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), now);
    let res = String::from_utf8_lossy(&res);
    assert!(res.starts_with("HTTP/1.1 200"), "{path}: {res}");
    serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap()
}

#[test]
fn a_screen_built_a_minute_later_is_the_same_screen_its_ages_tick_on_the_page() {
    let (c, now) = threads("quiet-clock");
    let snap = drover::snapshot::Snapshot {
        ts: now,
        ..Default::default()
    };
    drover::db::put(&c.state, "board", now, &snap).unwrap();
    let r = OnShed::new(Fake::default());
    let s = Server::new(&c, &r, Some(ADDR.into()));
    for path in [
        "/screens/queue",
        "/screens/events",
        "/screens/threads",
        "/screens/threads/t2",
    ] {
        for at in [now + 100, now + 300, now + 3_000] {
            assert_eq!(
                get(&s, at, path),
                get(&s, at + 60, path),
                "{path}: nothing it shows changed in the minute after {}s",
                at - now
            );
        }
    }
}

fn data_until_ping(r: &mut BufReader<std::net::TcpStream>) -> Vec<Value> {
    let (mut out, mut line) = (vec![], String::new());
    loop {
        line.clear();
        assert!(r.read_line(&mut line).unwrap() > 0, "stream closed");
        if line.starts_with(": ping") {
            return out;
        }
        if let Some(d) = line.strip_prefix("data: ") {
            out.push(serde_json::from_str(d).unwrap());
        }
    }
}

#[test]
fn unrelated_events_send_no_stream_data_and_a_thread_read_reaches_only_the_threads_list() {
    let (c, now) = threads("quiet-stream");
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let c2 = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c2, &OnShed::new(Fake::default()), l, false));
    let open = |path: &str| {
        let mut s = std::net::TcpStream::connect(&addr).unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_secs(40))).unwrap();
        write!(s, "GET {path}/stream HTTP/1.1\r\nHost: {addr}\r\n\r\n").unwrap();
        let mut r = BufReader::new(s);
        let mut line = String::new();
        while !line.starts_with("data: ") {
            line.clear();
            assert!(r.read_line(&mut line).unwrap() > 0, "{path}: stream closed");
        }
        r
    };
    let mut streams: Vec<(&str, BufReader<_>)> = ["/screens/threads/t2", "/screens/queue", "/screens/threads"]
        .into_iter()
        .map(|p| (p, open(p)))
        .collect();
    for i in 0..20 {
        let d = json!({"agent": format!("otherfleet-impl-{i}"), "state": "idle"});
        append(&c.state, &Event::new(now, "otherfleet", Some(i), "lane-state", d)).unwrap();
    }
    append(
        &c.state,
        &Event::new(now, "sendmeter", None, "thread-read", json!({"thread": "t3"})),
    )
    .unwrap();
    let mut sent: Vec<(&str, Vec<Value>)> = streams.iter_mut().map(|(p, r)| (*p, data_until_ping(r))).collect();
    let (_, list) = sent.pop().unwrap();
    assert_eq!(
        sent,
        [("/screens/threads/t2", vec![]), ("/screens/queue", vec![])],
        "20 unrelated events and another thread's read send these screens nothing but a ping"
    );
    assert_eq!(list.len(), 1, "{list:?}");
    assert!(
        list[0]["ops"].to_string().contains("\"t3\"") && !list[0]["ops"].to_string().contains("\"new\""),
        "the threads list drops t3's unread dot: {list:?}"
    );
}
