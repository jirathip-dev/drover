mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

fn live_server(name: &str) -> (drover::Ctx, std::net::SocketAddr, FakeShed) {
    let c = ctx(&tmpdir(name), CONFIG);
    append(&c.state, &Event::new(T, "sendmeter", None, "note", json!({}))).unwrap();
    let sh = FakeShed::new();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let (c2, b) = (c.clone(), sh.shed());
    std::thread::spawn(move || drover::serve::serve_on(&c2, &drover::shed::With(&Fake::default(), b), l, false));
    (c, addr, sh)
}

struct Lanes {
    f: Fake,
    sh: FakeShed,
}

impl Lanes {
    fn new(f: Fake) -> Lanes {
        Lanes { f, sh: FakeShed::new() }
    }
    fn blocked(&self, name: &str, screen: &str) {
        self.sh.lane(name, "blocked", "/x");
        self.sh
            .set(name, |t| t["reported"]["note"] = json!("permission: Bash command"));
        self.sh.screen(name, screen);
    }
    fn keys(&self) -> Vec<serde_json::Value> {
        self.sh.all("keys")
    }
}

impl drover::runner::Runner for Lanes {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        self.f.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<drover::runner::Out> {
        self.f.run_in(prog, args, input)
    }
    fn sleep(&self, secs: u64) {
        self.f.sleep(secs);
    }
    fn shed(&self) -> Option<drover::shed::Shed> {
        Some(self.sh.shed())
    }
}

fn subscribe(addr: std::net::SocketAddr, host: &str) -> TcpStream {
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(format!("GET /live HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes())
        .unwrap();
    s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    s
}

fn drain(s: &mut TcpStream, ms: u64) -> String {
    let (end, mut out, mut b) = (Instant::now() + Duration::from_millis(ms), vec![], vec![0u8; 65_536]);
    while Instant::now() < end {
        match s.read(&mut b) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&b[..n]),
            Err(_) => {}
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[test]
fn live_pushes_one_frag_per_changed_section() {
    let (c, addr, _sh) = live_server("live");
    let mut s = subscribe(addr, &addr.to_string());
    let hello = drain(&mut s, 1_500);
    assert!(
        hello.starts_with("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n")
            && hello.contains(": ping\n\nevent: build\ndata: "),
        "{hello}"
    );
    assert_eq!(
        hello.matches("event:").count(),
        1,
        "nothing changed yet, the build alone (#391): {hello}"
    );
    let quiet = drain(&mut s, 2_500);
    assert!(
        quiet.lines().all(|l| l.is_empty() || l == ": ping"),
        "no change: pings only: {quiet}"
    );
    let q = json!({"op": "add", "issues": [99]});
    append(&c.state, &Event::new(T + 5, "sendmeter", None, "queue", q)).unwrap();
    let t0 = Instant::now();
    let mut got = String::new();
    while !got.contains("\n\n") && t0.elapsed() < Duration::from_millis(1_500) {
        got += &drain(&mut s, 50);
    }
    assert!(
        t0.elapsed() < Duration::from_millis(1_500),
        "the queue data within 1.5 s: {got}"
    );
    got += &drain(&mut s, 1_200);
    assert_eq!(got.matches("event: frag").count(), 0, "{got}");
    assert_eq!(got.matches("event: queue").count(), 1, "{got}");
    let line = got
        .lines()
        .find(|l| l.starts_with("data: {\"error\""))
        .expect("the queue data");
    let v: serde_json::Value = serde_json::from_str(&line[6..]).unwrap();
    assert_eq!(v["fleets"][0]["rows"][0]["issue"], 99, "{v}");
    assert_eq!(v["now"].as_u64().map(|n| n > 0), Some(true), "{v}");
    assert!(
        got.contains("event: ev\ndata: {\"f\":\"sendmeter\",\"k\":null,\"n\":null,\"seq\":2,"),
        "{got}"
    );
    let d = json!({"step": "start", "result": "REFUSED HOST_BUSY: load 9", "code": "HOST_BUSY"});
    append(&c.state, &Event::new(T + 6, "sendmeter", Some(7), "driven", d)).unwrap();
    let got = drain(&mut s, 2_500);
    assert_eq!(got.matches("event: host").count(), 1, "{got}");
    let line = got
        .lines()
        .find(|l| l.starts_with("data: {\"deepseek\""))
        .expect("the host data");
    let v: serde_json::Value = serde_json::from_str(&line[6..]).unwrap();
    assert_eq!(v["fleets"][0]["word"], "held", "{v}");
    assert_eq!(v["now"].as_u64().map(|n| n > 0), Some(true), "{v}");
    let a = json!({"id": "q1", "question": "Ship it?", "options": ["yes", "no"], "recommend": null, "kind": "owner"});
    append(&c.state, &Event::new(T + 7, "sendmeter", Some(7), "needs-decision", a)).unwrap();
    let got = drain(&mut s, 2_500);
    assert_eq!(got.matches("event: needs\n").count(), 1, "{got}");
    assert!(
        got.contains("\"question\":\"Ship it?\"") && got.contains("\"paired\":false"),
        "{got}"
    );
    assert_eq!(got.matches("event: lanes\n").count(), 0, "Lanes unchanged: {got}");
}

#[test]
fn live_caps_clients_and_checks_host() {
    let (_c, addr, _sh) = live_server("cap");
    let mut foreign = subscribe(addr, "evil.example");
    assert!(drain(&mut foreign, 500).starts_with("HTTP/1.1 403 "));
    let mut held: Vec<TcpStream> = (0..8).map(|_| subscribe(addr, &addr.to_string())).collect();
    for s in &mut held {
        assert!(drain(s, 300).starts_with("HTTP/1.1 200 OK"));
    }
    let mut ninth = subscribe(addr, &addr.to_string());
    assert!(
        drain(&mut ninth, 500).starts_with("HTTP/1.1 200 OK"),
        "#893: the 9th client closes the oldest"
    );
    drop(held);
    std::thread::sleep(Duration::from_millis(1_500));
    let mut again = subscribe(addr, &addr.to_string());
    assert!(
        drain(&mut again, 300).starts_with("HTTP/1.1 200 OK"),
        "closed clients free their slots"
    );
    let get = |path: &str| {
        let mut s = TcpStream::connect(addr).unwrap();
        s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
            .unwrap();
        let mut b = String::new();
        s.read_to_string(&mut b).unwrap();
        b
    };
    assert!(get("/frags?tab=lanes").starts_with("HTTP/1.1 404 "));
    for part in ["needs", "lanes", "chat"] {
        let r = get(&format!("/api/v1/{part}"));
        let j: serde_json::Value = serde_json::from_str(&r[r.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert!(j["now"].as_u64().is_some(), "{part}: {j}");
    }
}

const PERMISSION: &str = "╭──────────────────────────────────────────╮\n│ Bash command                             │\n│   cargo test --all                       │\n│ Do you want to proceed?                  │\n│ ❯ 1. Yes                                 │\n│   2. Yes, and don't ask again for cargo  │\n│      commands in /p/sendmeter            │\n│   3. No, and tell Claude what to do      │\n│      differently (esc)                   │\n╰──────────────────────────────────────────╯\n";

fn blocked(name: &str, imp: &str, rev: &str) -> (drover::Ctx, Lanes) {
    let c = ctx(&tmpdir(name), CONFIG);
    for (issue, role, agent) in [(12, "impl", "sendmeter-impl-12"), (9, "rev", "sendmeter-rev-9-r1")] {
        append(
            &c.state,
            &Event::new(
                T - 100,
                "sendmeter",
                Some(issue),
                "spawn",
                json!({"role": role, "agent": agent, "harness": "claude"}),
            ),
        )
        .unwrap();
    }
    let r = Lanes::new(Fake::default());
    r.blocked("sendmeter-impl-12", imp);
    r.blocked("sendmeter-rev-9-r1", rev);
    (c, r)
}

fn needs(s: &Server, extra: &str, now: u64) -> serde_json::Value {
    let r = String::from_utf8_lossy(&s.respond(
        &format!("GET /api/v1/needs HTTP/1.1\r\nHost: {ADDR}\r\n{extra}\r\n"),
        now,
    ))
    .into_owned();
    serde_json::from_str(&r[r.find("\r\n\r\n").unwrap() + 4..]).unwrap_or_else(|_| panic!("{r}"))
}

fn card<'a>(needs: &'a serde_json::Value, agent: &str) -> Option<&'a serde_json::Value> {
    needs["blocked"].as_array().unwrap().iter().find(|b| b["name"] == agent)
}

fn req(s: &Server, method: &str, path: &str, body: &str, extra: &str) -> String {
    let r = format!(
        "{method} {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&r, T)).into_owned()
}

fn cookie(s: &Server, c: &drover::Ctx) -> String {
    pair(
        s,
        c,
        "",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
}

#[test]
fn an_answer_is_refused_shed_reports_no_menu() {
    let (c, r) = blocked("answer-rev", "garbage, no prompt", PERMISSION);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = cookie(&s, &c);
    for agent in ["sendmeter-rev-9-r1", "sendmeter-impl-12"] {
        let res = req(&s, "POST", "/answer", &format!("agent={agent}&choice=1"), &ck);
        assert!(res.contains("REFUSED BAD_VALUE: no menu"), "{res}");
    }
    assert_eq!(r.keys(), [] as [serde_json::Value; 0]);
    let v = needs(&s, &ck, T);
    let b = card(&v, "sendmeter-impl-12").unwrap_or_else(|| panic!("{v}"));
    assert_eq!((&b["menu"], &b["push"]), (&json!(null), &json!(true)), "{b}");
}

#[test]
fn unparsed_blocked_pane_shows_masked_tail_and_reply() {
    let (c, r) = blocked(
        "tail",
        concat!(
            "l1\nl2\nl3\nl4\nl5\nl6\nl7 token=gh",
            "p_abcdefghijklmnopqrstuvwxyz0123456789\nl8"
        ),
        PERMISSION,
    );
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = cookie(&s, &c);
    let v = needs(&s, &ck, T);
    let b = card(&v, "sendmeter-impl-12").unwrap_or_else(|| panic!("{v}"));
    assert_eq!(b["tail"], "l3\nl4\nl5\nl6\nl7 token=•••\nl8", "{b}");
    assert!(
        !v.to_string()
            .contains(concat!("gh", "p_abcdefghijklmnopqrstuvwxyz0123456789")),
        "masked: {v}"
    );
}

fn reconnect(addr: std::net::SocketAddr, since: &str) -> String {
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(format!("GET /live{since} HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
        .unwrap();
    s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    drain(&mut s, 300)
}

#[test]
fn live_messages_carry_the_seq_and_a_reconnect_replays_what_it_missed() {
    let (c, addr, _sh) = live_server("replay");
    let mut s = subscribe(addr, &addr.to_string());
    drain(&mut s, 1_500);
    append(&c.state, &Event::new(T + 1, "sendmeter", Some(7), "merged", json!({}))).unwrap();
    let got = drain(&mut s, 1_500);
    assert!(
        got.contains(
            "id: 2\nevent: ev\ndata: {\"f\":\"sendmeter\",\"k\":\"merged\",\"n\":7,\"seq\":2,\"ts\":1000001}\n\n"
        ),
        "{got}"
    );
    assert!(
        !got.contains("\"id\":\"events\""),
        "#266: no Events frag, the board reads the API: {got}"
    );
    drop(s);
    for i in 0..3 {
        append(
            &c.state,
            &Event::new(
                T + 10 + i,
                "sendmeter",
                None,
                "note",
                json!({"reason": format!("missed-{i}")}),
            ),
        )
        .unwrap();
    }
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(format!("GET /live HTTP/1.1\r\nHost: {addr}\r\nLast-Event-ID: 2\r\n\r\n").as_bytes())
        .unwrap();
    s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    let back = drain(&mut s, 2_500);
    let ids: Vec<&str> = back.lines().filter(|l| l.starts_with("data: {\"f\"")).collect();
    assert_eq!(
        ids.len(),
        3,
        "each missed event once, the live tick resends none: {back}"
    );
    for n in 3..=5 {
        assert_eq!(back.matches(&format!("\"seq\":{n},")).count(), 1, "{back}");
    }
    assert_eq!(reconnect(addr, "?since=4").matches("event: ev").count(), 1);
    assert_eq!(
        reconnect(addr, "?since=5").matches("event:").count(),
        1,
        "the build alone (#391)"
    );
    let evs = String::from_utf8_lossy(&Server::new(&c, &Fake::default(), Some(addr.to_string())).respond(
        &format!("GET /api/v1/events?since=0 HTTP/1.1\r\nHost: {addr}\r\n\r\n"),
        T + 20,
    ))
    .into_owned();
    for i in 0..3 {
        assert_eq!(evs.matches(&format!("\"reason\":\"missed-{i}\"")).count(), 1, "{evs}");
    }
    for i in 0..501 {
        append(&c.state, &Event::new(T + 30 + i, "sendmeter", None, "note", json!({}))).unwrap();
    }
    let far = reconnect(addr, "?since=5");
    assert!(
        far.contains("id: 506\nevent: refresh\ndata: 506\n\n") && !far.contains("event: ev"),
        "{far}"
    );
    assert_eq!(reconnect(addr, "?since=6").matches("event: ev").count(), 500);
}

#[test]
fn unpaired_viewers_see_no_pane_text() {
    let (c, r) = blocked("unpaired", PERMISSION, PERMISSION);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = cookie(&s, &c);
    let pane = [
        "proceed",
        "class=pick",
        "class=rp",
        "cargo test --all",
        "don&#39;t ask",
        "don't ask",
    ];
    for path in ["/", "/api/v1/needs", "/api/v1/lanes"] {
        let res = req(&s, "GET", path, "", "");
        assert!(pane.iter().all(|p| !res.contains(p)), "{path}: {res}");
        assert!(
            req(&s, "GET", path, "", &ck).contains("don't ask") || path == "/",
            "{path} paired"
        );
    }
    let live = FakeShed::new();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let (c2, b) = (c.clone(), live.shed());
    std::thread::spawn(move || drover::serve::serve_on(&c2, &drover::shed::With(&Fake::default(), b), l, false));
    let sub = |extra: &str| {
        let mut s = TcpStream::connect(addr).unwrap();
        s.write_all(format!("GET /live HTTP/1.1\r\nHost: {addr}\r\n{extra}\r\n").as_bytes())
            .unwrap();
        s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        s
    };
    let (mut anon, mut dev) = (sub(""), sub(&ck));
    let hello = drain(&mut anon, 1_500);
    assert!(
        !live.all("ls").is_empty() && !hello.contains("event: needs\n"),
        "{hello}"
    );
    for n in ["sendmeter-impl-12", "sendmeter-rev-9-r1"] {
        live.lane(n, "blocked", "/x");
        live.set(n, |t| t["reported"]["note"] = json!("permission: Bash command"));
        live.screen(n, PERMISSION);
    }
    let (a, d) = (drain(&mut anon, 4_000), drain(&mut dev, 500));
    assert!(
        a.contains("event: needs\n") && a.contains("\"activity\":\"permission: Bash command"),
        "{a}"
    );
    assert!(pane.iter().all(|p| !a.contains(p)), "unpaired stream: {a}");
    assert!(
        d.contains("don't ask") && d.contains("\"tail\":\""),
        "paired stream: {d}"
    );
    drover::devices::store(&c, &[]).unwrap();
    append(&c.state, &Event::new(T + 5, "sendmeter", None, "note", json!({}))).unwrap();
    let after = drain(&mut dev, 2_500);
    assert!(pane.iter().all(|p| !after.contains(p)), "revoked device: {after}");
    assert!(matches!(dev.read(&mut [0; 1]), Ok(0)), "revoked device: stream closed");
}

#[test]
fn the_sampler_thread_fills_the_host() {
    let c = ctx(&tmpdir("sampler"), CONFIG);
    let sh = FakeShed::new();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let r = Fake::default()
        .reply("vm_stat", "Swapouts: 100.\n")
        .reply("sysctl", "4\n");
    let b = sh.shed();
    std::thread::spawn(move || drover::serve::serve_on(&c, &drover::shed::With(&r, b), l, false));
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let mut s = TcpStream::connect(addr).unwrap();
        s.write_all(format!("GET /api/v1/herd HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
            .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        if out.contains(r#""pressure":"critical""#) {
            break;
        }
        assert!(Instant::now() < end, "the host never filled: {out}");
        std::thread::sleep(Duration::from_millis(50));
    }
}
