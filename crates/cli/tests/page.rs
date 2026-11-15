mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const LANE: &str = "sendmeter-impl-12";

#[test]
fn a_paired_phone_answers_settles_cards_pauses_stops_and_saves_fleets_through_the_page_and_paints_offline() {
    let Some(chrome) = std::env::var_os("CHROME") else {
        eprintln!("skipping: no CHROME here");
        return;
    };
    let d = tmpdir("page-taps");
    let c = ctx(&d, &format!("models = [\"opus\", \"sonnet\"]\n{CONFIG}"));
    let sh = FakeShed::new();
    sh.attach(&c);
    let wt = d.join("wt").to_string_lossy().into_owned();
    std::fs::create_dir_all(&wt).unwrap();
    let now = drover::now();
    let ev = |ts: u64, n: Option<u64>, kind: &str, d: Value| {
        append(&c.state, &Event::new(ts, "sendmeter", n, kind, d)).unwrap();
    };
    let sp =
        json!({"role": "impl", "agent": LANE, "pane": LANE, "worktree": wt, "branch": "issue-12", "runtime": "shed"});
    ev(now - 600, Some(12), "spawn", sp);
    let ask = json!({"id": "q7", "question": "Move the branch out?", "options": ["move", "refuse"], "recommend": 1, "kind": "routine"});
    ev(now - 300, None, "needs-decision", ask);
    for id in ["r1", "r2"] {
        let card = json!({"thread": "t1", "id": id, "cmd": "true", "why": format!("card {id}"), "blast": "nothing",
            "dir": wt, "host": "main", "timeout": 60, "sha": drover::devices::sha256(b"true")});
        ev(now - 60, None, "run-card", card);
    }
    sh.lane(LANE, "working", &wt);
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let ck = pair(
        &Server::new(&c, &Fake::default(), Some("127.0.0.1:7777".into())),
        &c,
        "Mozilla/5.0 (iPhone)",
        now,
    );
    let c2 = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c2, &OnShed(Fake::default(), sh), l, false));
    let o = std::process::Command::new("node")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ui/page.mjs"))
        .arg(&chrome)
        .arg(format!("http://{addr}/"))
        .arg(ck.trim().trim_start_matches("Cookie: "))
        .output()
        .unwrap();
    let out: Value =
        serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&o.stderr)));
    assert!(out.get("error").is_none() && out["errors"] == json!([]), "{out}");
    assert_eq!(
        out["posted"],
        json!([
            "answer",
            "run-card",
            "run-card",
            "pause",
            "pause",
            "stop",
            "save-fleets"
        ]),
        "{out}"
    );
    assert_eq!(out["sheetClosed"], true, "a saved Fleets sheet closes: {out}");
    let off = &out["offline"];
    assert_eq!((&off["dot"], &off["tab"]), (&json!("r"), &json!("lanes")), "{out}");
    assert!(
        off["stale"].as_str().unwrap().starts_with("last update "),
        "offline, the cached screen's age: {out}"
    );

    let log = read(&c.state).unwrap().to_vec();
    let last = |kind: &str, f: &dyn Fn(&Event) -> bool| log.iter().rfind(|e| e.kind == kind && f(e)).cloned();
    let d = last("decision", &|e| e.str("id") == Some("q7")).expect("the ask answered");
    assert_eq!(d.str("choice"), Some("refuse"));
    for (id, ok) in [("r1", true), ("r2", false)] {
        let s = last("run-settled", &|e| e.str("id") == Some(id)).unwrap_or_else(|| panic!("{id} settled: {out}"));
        assert_eq!(s.val("approved"), &json!(ok), "{s:?}");
    }
    let act = last("act", &|e| e.str("op") == Some("stop")).expect("the lane stopped");
    assert_eq!(act.str("agent"), Some(LANE));
    let cfg = std::fs::read_to_string(&c.config).unwrap();
    assert!(!cfg.contains("paused = true"), "paused, then resumed:\n{cfg}");
    let words: Vec<&str> = ["pause", "resume"]
        .iter()
        .map(|k| out["steps"][k].as_str().unwrap_or_default())
        .collect();
    assert!(words[0].contains("paused") && words[1].contains("resumed"), "{words:?}");
    let role = cfg.split("[fleet.sendmeter.role.impl]").nth(1).unwrap();
    assert!(
        role.lines().take(5).any(|l| l.replace(' ', "") == "model=\"sonnet\""),
        "the Fleets sheet set impl's model:\n{cfg}"
    );
}
