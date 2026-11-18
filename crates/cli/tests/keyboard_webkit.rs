mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};

#[test]
fn the_focused_composer_stays_the_same_node_with_its_focus_text_and_caret_while_the_stream_changes_it() {
    if no_webkit() {
        return;
    }
    let c = ctx(
        &tmpdir("page-keyboard"),
        &format!("{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\n"),
    );
    let profile = c.config.with_file_name("claude").join("fleet-orchestrator");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    let now = drover::now();
    let ev = |ts: u64, fleet: &str, n: Option<u64>, kind: &str, d: Value| {
        append(&c.state, &Event::new(ts, fleet, n, kind, d)).unwrap();
    };
    let t2 = json!({"thread": "t2", "title": "Rate limits", "harness": "claude"});
    ev(now - 7_200, "sendmeter", None, "thread-new", t2);
    for i in 0..40u64 {
        let from = if i % 2 == 0 { "owner" } else { "orchestrator" };
        let d = json!({"thread": "t2", "from": from, "text": format!("message {i}")});
        ev(now - 7_000 + i, "sendmeter", None, "thread-msg", d);
    }
    let start = json!({"thread": "t2", "agent": "orchestrator-t2", "pane": "orchestrator-t2"});
    ev(now - 60, "sendmeter", None, "orchestrator-start", start);
    let sh = FakeShed::attached(&c);
    sh.add(
        "orchestrator-t2",
        json!({"fleet": "sendmeter", "issue": "-", "role": "orchestrator", "thread": "t2", "source": "drover"}),
    );
    let ck = pair(
        &Server::new(&c, &Fake::default(), Some("127.0.0.1:7777".into())),
        &c,
        "Mozilla/5.0 (iPhone)",
        now,
    );
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c2, &OnShed(Fake::default(), sh), l, false));
    let mut node = std::process::Command::new("node")
        .arg(format!("{}/tests/ui/keyboard.mjs", env!("CARGO_MANIFEST_DIR")))
        .arg(format!("http://{addr}/"))
        .arg(ck.trim().trim_start_matches("Cookie: "))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(node.stdout.take().unwrap());
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    assert_eq!(line.trim(), "typed", "{line}");
    for i in 0..20 {
        let d = json!({"agent": format!("otherfleet-impl-{i}"), "state": "idle"});
        ev(now, "otherfleet", Some(i), "lane-state", d);
    }
    let sleep = json!({"thread": "t2", "agent": "orchestrator-t2"});
    ev(now, "sendmeter", None, "orchestrator-sleep", sleep);
    let mut stdin = node.stdin.take().unwrap();
    stdin.write_all(b"go\n").unwrap();
    drop(stdin);
    line.clear();
    out.read_to_string(&mut line).unwrap();
    node.wait().unwrap();
    let out: Value = serde_json::from_str(&line).unwrap_or_else(|_| panic!("{line}"));
    assert!(out.get("error").is_none() && out["errors"] == json!([]), "{out}");
    assert_eq!(
        out["after"],
        json!({"working": false, "same": true, "focus": true, "value": "half a thought", "caret": [4, 6], "blurs": 0}),
        "20 unrelated events and the working pill turning off patch the composer in place: {out}"
    );
}
