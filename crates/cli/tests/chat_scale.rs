mod common;
use common::*;
use drover::orchestrator::SCANS;
use drover::serve::Server;
use serde_json::json;
use std::sync::atomic::Ordering::Relaxed;

const ADDR: &str = "127.0.0.1:7777";
const NOW: u64 = 1_000_000;
const THREADS: u64 = 30;
const MSGS: u64 = 200;

#[test]
fn the_chat_tab_is_linear_in_the_log_and_cached_by_its_last_seq() {
    let c = ctx(&tmpdir("chat-scale"), CONFIG);
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let mut log = vec![];
    for t in 1..=THREADS {
        let id = format!("t{t}");
        log.push(json!({"ts": NOW, "fleet": "sendmeter", "type": "thread-new", "thread": id, "title": format!("thread {t}")}));
        for m in 0..MSGS {
            let from = if m % 2 == 0 { "owner" } else { "orchestrator" };
            log.push(
                json!({"ts": NOW + m, "fleet": "sendmeter", "type": "thread-msg", "thread": id, "from": from,
                "text": format!("message {m}")}),
            );
        }
    }
    log.push(
        json!({"ts": NOW, "fleet": "sendmeter", "type": "thread-msg", "thread": "t1", "from": "t2", "text": "handed"}),
    );
    let got = log.len() as u64;
    log.push(
        json!({"ts": NOW, "fleet": "sendmeter", "type": "thread-handoff", "thread": "t1", "from": "t1", "to": "t3",
        "title": "on", "text": "on", "forwards": got}),
    );
    while log.len() < 60_000 {
        log.push(json!({"ts": NOW, "fleet": "sendmeter", "issue": 1, "type": "note", "text": "filler"}));
    }
    let jsonl: String = log.iter().map(|e| format!("{e}\n")).collect();
    std::fs::write(&c.state, jsonl).unwrap();

    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let before = SCANS.load(Relaxed);
    let first = api(&s, ADDR, "", NOW + 500, "chat");
    let scans = SCANS.load(Relaxed) - before;
    let v: serde_json::Value = serde_json::from_str(&first).unwrap();
    assert_eq!(v["threads"].as_array().unwrap().len(), THREADS as usize);
    assert!(first.contains("handed (forwarded)"), "the forwarded row is marked");
    assert!(
        scans <= 20 * THREADS as usize,
        "{scans} log scans for {THREADS} threads"
    );

    let before = SCANS.load(Relaxed);
    assert_eq!(api(&s, ADDR, "", NOW + 501, "chat"), first, "same answer");
    assert_eq!(SCANS.load(Relaxed) - before, 0, "the log was folded again");
    drover::events::append(
        &c.state,
        &drover::events::Event::new(NOW, "sendmeter", Some(1), "note", json!({})),
    )
    .unwrap();
    let before = SCANS.load(Relaxed);
    api(&s, ADDR, "", NOW + 502, "chat");
    assert!(SCANS.load(Relaxed) > before, "a new event folds the log again");
}
