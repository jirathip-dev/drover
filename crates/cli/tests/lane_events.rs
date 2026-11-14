mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::runner::{Out, Runner};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

const NOW: u64 = 1_000_000;

struct Host {
    alive: Arc<AtomicBool>,
    lists: AtomicUsize,
}

impl Runner for Host {
    fn local(&self) -> bool {
        true
    }
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        let mut stdout = String::new();
        if prog == "herdr" && args == ["agent", "list"] {
            self.lists.fetch_add(1, SeqCst);
            let lane =
                json!({"name": "sendmeter-impl-12", "pane_id": "w1:p1", "agent": "claude", "agent_status": "idle"});
            let agents: Vec<Value> = self.alive.load(SeqCst).then_some(lane).into_iter().collect();
            stdout = json!({"result": {"agents": agents}}).to_string();
        } else if prog == "herdr" && args.ends_with(&["agent".into(), "list".into()]) {
            stdout = NO_AGENTS.into();
        }
        Ok(Out {
            code: 0,
            stdout,
            stderr: String::new(),
        })
    }
}

fn status(s: &str) -> String {
    let d = json!({"type": "pane_agent_status_changed", "pane_id": "w1:p1", "workspace_id": "w1", "agent": "claude", "agent_status": s});
    json!({"event": "pane_agent_status_changed", "data": d}).to_string()
}

fn until(what: &str, secs: f64, f: impl Fn() -> bool) -> Duration {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed().as_secs_f64() < secs, "not within {secs} s: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
    t0.elapsed()
}

#[test]
fn the_service_keeps_lane_state_from_herdrs_events_and_polls_when_the_stream_drops() {
    let c = ctx(&tmpdir("lane-events"), CONFIG);
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "w1:p1"});
    append(&c.state, &Event::new(NOW - 9, "sendmeter", Some(12), "spawn", d)).unwrap();

    let mut lane = std::process::Command::new("sleep").arg("60").spawn().unwrap();
    let pid = lane.id().to_string();
    let alive = Arc::new(AtomicBool::new(true));
    let sock = std::env::temp_dir().join(format!("hle-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&sock);
    let l = UnixListener::bind(&sock).unwrap();
    let (subs_tx, subs) = mpsc::channel::<Value>();
    let (drop_tx, drop_rx) = mpsc::channel::<()>();
    let (a2, s2) = (alive.clone(), sock.clone());
    std::thread::spawn(move || {
        let conn = || {
            let (c, _) = l.accept().unwrap();
            let mut text = String::new();
            BufReader::new(&c).read_line(&mut text).unwrap();
            subs_tx.send(serde_json::from_str(&text).unwrap()).unwrap();
            writeln!(&c, r#"{{"id":"drover","result":{{"type":"subscription_started"}}}}"#).unwrap();
            c
        };
        let c1 = conn();
        writeln!(&c1, "{}\n{}", status("working"), status("done")).unwrap();
        lane.wait().unwrap();
        a2.store(false, SeqCst);
        writeln!(
            &c1,
            r#"{{"event":"pane_exited","data":{{"type":"pane_exited","pane_id":"w1:p1","workspace_id":"w1"}}}}"#
        )
        .unwrap();
        let c2 = conn();
        drop_rx.recv().unwrap();
        drop((c2, l));
        std::fs::remove_file(&s2).unwrap();
    });

    let host: &'static Host = Box::leak(Box::new(Host {
        alive,
        lists: AtomicUsize::new(0),
    }));
    let api = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let (sc, hs) = (c.clone(), sock.clone());
    std::thread::spawn(move || drover_service::run(sc, host, || NOW, api, Some(hs)));
    let states = || -> Vec<String> {
        let ev = read(&c.state).unwrap_or_default();
        let lane =
            |e: &&Event| e.kind == "lane-state" && e.issue == Some(12) && e.str("agent") == Some("sendmeter-impl-12");
        ev.iter()
            .filter(lane)
            .filter_map(|e| e.str("state").map(String::from))
            .collect()
    };

    let s = subs.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(s["method"], "events.subscribe");
    let types: Vec<&str> = s["params"]["subscriptions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["type"].as_str().unwrap())
        .collect();
    assert!(types.contains(&"pane.exited") && types.contains(&"pane.created"), "{s}");
    assert!(
        s["params"]["subscriptions"]
            .as_array()
            .unwrap()
            .contains(&json!({"type": "pane.agent_status_changed", "pane_id": "w1:p1"})),
        "{s}"
    );
    until("idle, working, done", 2.0, || states() == ["idle", "working", "done"]);

    let n = host.lists.load(SeqCst);
    let agents = drover::herdr::agents(host).unwrap();
    assert_eq!((agents.len(), agents[0]["agent_status"].as_str()), (1, Some("done")));
    assert_eq!(host.lists.load(SeqCst), n, "no herdr agent list while the stream is up");
    let there = drover::hosts::On::new(host, Some("mini2"));
    assert_eq!(drover::herdr::agents(&there).unwrap(), [] as [serde_json::Value; 0]);

    assert!(std::process::Command::new("kill")
        .args(["-9", &pid])
        .status()
        .unwrap()
        .success());
    let took = until("gone", 2.0, || states().last().map(String::as_str) == Some("gone"));
    assert!(took < Duration::from_secs(2), "{took:?}");
    assert_eq!(states(), ["idle", "working", "done", "gone"]);
    assert_eq!(drover::herdr::agents(host).unwrap(), [] as [serde_json::Value; 0]);
    let s = subs.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        !s.to_string().contains("w1:p1"),
        "the gone pane is no longer subscribed: {s}"
    );

    drop_tx.send(()).unwrap();
    let down = |e: &Event| e.kind == "lane-stream" && e.str("state") == Some("down");
    until("stream down", 2.0, || read(&c.state).unwrap().iter().any(down));
    let n = host.lists.load(SeqCst);
    drover::herdr::agents(host).unwrap();
    assert_eq!(host.lists.load(SeqCst), n + 1, "polled once the stream is down");
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let rows = drover::timer::services(&c, &cfg, host, Some(String::new()));
    let row = rows
        .iter()
        .find(|r| r.0 == "herdr lane events")
        .expect("a lane events row");
    assert_eq!(row.1, "a", "{row:?}");
    assert!(row.2.contains("polling"), "{row:?}");
}
