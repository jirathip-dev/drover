//! #345 `herdsman service` over real sockets: its health, a kick on the local socket, a remote lane's command over
//! API v1 run as that lane (with its token) or refused (another issue's token, a verdict from impl, a closed lane),
//! and the event stream carrying what it logged.
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use herdsman::runner::{Out, Runner};
use herdsman::service::{grant, Grant};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;

const NOW: u64 = 1_000_000;

/// The fake host, but `env HERDSMAN_LANE=<lane> <exe> <args>` (the service running a lane's command) runs
/// `herdsman <args>` in process on the same state, as the real binary would.
struct Lane(Fake, herdsman::Ctx);

impl Runner for Lane {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        if prog != "env" {
            return self.0.run(prog, args);
        }
        assert!(
            args[0].starts_with("HERDSMAN_LANE=") && args[1] == self.1.exe,
            "{args:?}"
        );
        let (code, stdout) = run(
            &self.1,
            &self.0,
            &args[2..].iter().map(String::as_str).collect::<Vec<_>>(),
        );
        Ok(Out {
            code,
            stdout,
            stderr: String::new(),
        })
    }
}

fn post(addr: &str, body: &Value) -> (u16, Value) {
    let mut s = TcpStream::connect(addr).unwrap();
    let b = body.to_string();
    let req = format!(
        "POST /api/v1/cmd HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{b}",
        b.len()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut res = String::new();
    s.read_to_string(&mut res).unwrap();
    let status = res[9..12].parse().unwrap();
    (
        status,
        serde_json::from_str(res.split("\r\n\r\n").nth(1).unwrap()).unwrap(),
    )
}

/// A logged spawn of `role` on issue 12 and its token.
fn lane(c: &herdsman::Ctx, role: &str, at: u64) -> String {
    let agent = format!("sendmeter-{role}-12");
    let d = json!({"role": role, "agent": agent, "pane": format!("p-{role}")});
    append(&c.state, &Event::new(at, "sendmeter", Some(12), "spawn", d)).unwrap();
    let g = Grant {
        fleet: "sendmeter".into(),
        issue: 12,
        role: role.into(),
        agent: agent.clone(),
        lane: format!("sendmeter:12:{role}"),
        at,
    };
    grant(c, &g).unwrap()
}

#[test]
fn the_service_runs_a_lanes_command_with_its_token_and_refuses_any_other() {
    let c = ctx(&tmpdir("service"), CONFIG);
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    append(
        &c.state,
        &Event::new(NOW - 9, "sendmeter", Some(12), "queued", json!({})),
    )
    .unwrap();
    let imp = lane(&c, "impl", NOW - 5);
    let rev = lane(&c, "rev", NOW - 4);
    let other = {
        let g = Grant {
            fleet: "sendmeter".into(),
            issue: 13,
            role: "impl".into(),
            agent: "sendmeter-impl-13".into(),
            lane: "sendmeter:13:impl".into(),
            at: NOW - 3,
        };
        grant(&c, &g).unwrap()
    };
    let api = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = api.local_addr().unwrap().to_string();
    let r: &'static Lane = Box::leak(Box::new(Lane(Fake::default().reply("herdr", "{}"), c.clone())));
    let sc = c.clone();
    std::thread::spawn(move || herdsman_service::run(sc, r, || NOW, api));
    let mut pid = None;
    for _ in 0..100 {
        pid = herdsman::service::up(&c);
        if pid.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    // The local socket answers with this process: a kick is taken by the service, not detached.
    assert_eq!(pid, Some(u64::from(std::process::id())));
    assert_eq!(
        herdsman::service::kick(&c, "sendmeter", 12, "push"),
        Some(std::process::id())
    );

    let ask = json!([
        "ask",
        "sendmeter",
        "12",
        "Which colour?",
        "--option",
        "red",
        "--option",
        "blue"
    ]);
    let asks = |c: &herdsman::Ctx| {
        read(&c.state)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "needs-decision")
            .count()
    };
    // Refused: another issue's token, a junk one, a verdict from an impl lane; nothing is run or logged.
    for (token, args) in [
        (&other, ask.clone()),
        (&"f".repeat(64), ask.clone()),
        (
            &imp,
            json!([
                "verdict",
                "sendmeter",
                "12",
                "PASS",
                "--sha",
                "a".repeat(40),
                "--file",
                "v.md"
            ]),
        ),
    ] {
        let (status, v) = post(&addr, &json!({"token": token, "args": args}));
        assert_eq!(status, 403, "{v}");
        assert!(v["stdout"].as_str().unwrap().starts_with("REFUSED TOKEN"), "{v}");
    }
    assert_eq!(asks(&c), 0);

    // Its own lane's ask runs as that lane: logged on this host.
    let (status, v) = post(&addr, &json!({"token": imp, "args": ask}));
    assert_eq!((status, v["code"].as_i64()), (200, Some(0)), "{v}");
    assert_eq!(asks(&c), 1);

    // The rev lane's token is closed with its lane.
    let d = json!({"agent": "sendmeter-rev-12", "pane": "p-rev"});
    append(&c.state, &Event::new(NOW, "sendmeter", Some(12), "lane-closed", d)).unwrap();
    let (status, v) = post(
        &addr,
        &json!({"token": rev, "args": ["done", "sendmeter", "12", "--no-change", "x"]}),
    );
    assert_eq!(status, 403, "{v}");
    assert!(v["stdout"].as_str().unwrap().contains("lane is closed"), "{v}");

    // The event stream (loopback) replays the log: the service's start and the lane's ask.
    let mut s = TcpStream::connect(&addr).unwrap();
    s.write_all(b"GET /api/v1/events?after=0 HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    let (mut got, mut buf) = (String::new(), [0u8; 8192]);
    while !(got.contains("\"service-started\"") && got.contains("Which colour?")) {
        let n = s.read(&mut buf).unwrap();
        assert!(n > 0, "{got}");
        got += &String::from_utf8_lossy(&buf[..n]);
    }
    assert!(
        got.starts_with("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream"),
        "{got}"
    );
}
