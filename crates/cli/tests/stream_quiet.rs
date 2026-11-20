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
    let hour = now / 3_600 * 3_600;
    let snap = drover::snapshot::Snapshot {
        ts: hour,
        ..Default::default()
    };
    drover::db::put(&c.state, "board", hour, &snap).unwrap();
    let r = OnShed::new(Fake::default());
    let s = Server::new(&c, &r, Some(ADDR.into()));
    for path in [
        "/screens/queue",
        "/screens/events",
        "/screens/threads",
        "/screens/threads/t2",
    ] {
        for at in [hour + 100, hour + 300, hour + 3_000] {
            assert_eq!(
                get(&s, at, path),
                get(&s, at + 60, path),
                "{path}: nothing it shows changed in the minute after {}s",
                at - hour
            );
        }
    }
}

fn serve(c: &drover::Ctx) -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let c = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c, &OnShed::new(Fake::default()), l, false));
    addr
}

fn open(addr: &str, path: &str) -> Result<BufReader<std::net::TcpStream>, String> {
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(40))).unwrap();
    write!(s, "GET {path}/stream HTTP/1.1\r\nHost: {addr}\r\n\r\n").unwrap();
    let mut r = BufReader::new(s);
    let (mut head, mut line) = (String::new(), String::new());
    while !line.starts_with("data: ") {
        line.clear();
        if r.read_line(&mut line).unwrap_or(0) == 0 {
            return Err(head);
        }
        head += &line;
    }
    Ok(r)
}

#[test]
fn with_four_streams_open_a_thread_view_still_gets_a_new_message_within_3_s() {
    let (c, now) = threads("stream-fifth");
    let addr = serve(&c);
    let held: Vec<_> = [
        "/screens/queue",
        "/screens/threads",
        "/screens/events",
        "/screens/threads/t3",
    ]
    .into_iter()
    .map(|p| open(&addr, p).unwrap_or_else(|h| panic!("{p}: {h}")))
    .collect();
    let mut t2 = open(&addr, "/screens/threads/t2").unwrap_or_else(|h| panic!("a 5th stream is refused: {h}"));
    let d = json!({"thread": "t2", "from": "orchestrator", "text": "the fifth stream hears this"});
    append(&c.state, &Event::new(now, "sendmeter", None, "thread-msg", d)).unwrap();
    let t0 = std::time::Instant::now();
    t2.get_ref()
        .set_read_timeout(Some(std::time::Duration::from_secs(3)))
        .unwrap();
    let mut line = String::new();
    while !(line.starts_with("data: ") && line.contains("the fifth stream hears this")) {
        line.clear();
        assert!(
            t2.read_line(&mut line).is_ok_and(|n| n > 0) && t0.elapsed().as_secs() < 3,
            "the message reaches the thread's stream within 3 s"
        );
    }
    drop(held);
}

#[test]
fn a_dropped_stream_frees_its_slot_within_20_s() {
    let (c, _) = threads("stream-dropped");
    let addr = serve(&c);
    let mut held = vec![];
    let refused = loop {
        match open(&addr, "/screens/threads/t2") {
            Ok(r) if held.len() < 100 => held.push(r),
            Ok(_) => panic!("100 streams from one device and none refused"),
            Err(h) => break h,
        }
    };
    assert!(held.len() >= 8, "a device holds 8 streams or more, not {}", held.len());
    assert!(refused.starts_with("HTTP/1.1 503"), "{refused}");
    drop(held.pop());
    let t0 = std::time::Instant::now();
    let again = loop {
        match open(&addr, "/screens/threads/t2") {
            Ok(r) => break r,
            Err(h) => assert!(
                t0.elapsed().as_secs() < 20,
                "the dropped stream's slot is still held: {h}"
            ),
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    };
    drop((again, held));
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
    let addr = serve(&c);
    let mut streams: Vec<(&str, BufReader<_>)> = ["/screens/threads/t2", "/screens/queue", "/screens/threads"]
        .into_iter()
        .map(|p| (p, open(&addr, p).unwrap()))
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

struct MidBuild(OnShed, std::path::PathBuf, std::sync::atomic::AtomicBool);

impl drover::runner::Runner for MidBuild {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        self.0.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<drover::runner::Out> {
        self.0.run_in(prog, args, input)
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
    fn shed(&self) -> Option<drover::shed::Shed> {
        let building = std::backtrace::Backtrace::force_capture()
            .to_string()
            .contains("threads_screen");
        if building && !self.2.swap(true, std::sync::atomic::Ordering::SeqCst) {
            let d = json!({"thread": "t3", "from": "orchestrator", "text": "landed mid-build"});
            append(&self.1, &Event::new(drover::now(), "sendmeter", None, "thread-msg", d)).unwrap();
        }
        self.0.shed()
    }
}

#[test]
fn an_event_logged_while_the_stream_builds_its_first_screen_reaches_the_stream() {
    let (c, _) = threads("stream-mid-build");
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let r = MidBuild(OnShed::new(Fake::default()), c.state.clone(), false.into());
    std::thread::spawn(move || drover::serve::serve_on(&c, &r, l, false));
    let mut s = std::net::TcpStream::connect(&addr).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(40))).unwrap();
    write!(s, "GET /screens/threads/t3/stream HTTP/1.1\r\nHost: {addr}\r\n\r\n").unwrap();
    let sent = data_until_ping(&mut BufReader::new(s));
    assert!(
        sent.iter().any(|d| d.to_string().contains("landed mid-build")),
        "a reply logged during the first build shows before the next ping, not 30 s later: {sent:?}"
    );
}
