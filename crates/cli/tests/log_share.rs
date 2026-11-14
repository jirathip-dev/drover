mod common;
use common::*;
use drover::events::{append, closed, shared, Event, REBUILDS};
use drover::serve::Server;
use serde_json::{json, Value};
use std::sync::atomic::Ordering::Relaxed;

const NOW: u64 = 1_000_000;
const ADDR: &str = "127.0.0.1:7777";

fn log_10k(name: &str) -> drover::Ctx {
    let c = ctx(&tmpdir(name), CONFIG);
    let ev = |ts, kind, d| serde_json::to_string(&Event::new(ts, "sendmeter", None, kind, d)).unwrap();
    let mut log = vec![];
    for t in 0..20 {
        let id = format!("t{t}");
        log.push(ev(NOW - 9_000, "thread-new", json!({"thread": id, "title": id})));
        let ask = json!({"id": format!("q{t}"), "question": "Ship it?", "options": ["yes", "no"], "recommend": 1,
            "kind": "owner", "thread": id, "agent": null});
        log.push(ev(NOW - 8_000, "needs-decision", ask));
    }
    for m in 0..10_000 - log.len() {
        let d = json!({"thread": format!("t{}", m % 20), "from": "owner", "text": "hi"});
        log.push(ev(NOW - 7_000 + m as u64 / 20, "thread-msg", d));
    }
    rewrite_log(&c.state, &(log.join("\n") + "\n"));
    c
}

fn one_more(c: &drover::Ctx) {
    let d = json!({"thread": "t0", "from": "owner", "text": "one more"});
    append(&c.state, &Event::new(NOW, "sendmeter", None, "thread-msg", d)).unwrap();
}

#[test]
fn a_render_on_the_log_it_holds_while_an_event_lands_rebuilds_no_ask_index() {
    let c = log_10k("share-render");
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let choosing = || {
        let needs: Value = serde_json::from_str(&api(&s, ADDR, "", NOW, "needs")).unwrap();
        needs["choosing"].as_u64().unwrap()
    };
    api(&s, ADDR, "", NOW, "chat");
    assert_eq!(choosing(), 20);
    let before = REBUILDS.load(Relaxed);
    let held = shared(&c.state).unwrap();
    one_more(&c);
    api(&s, ADDR, "", NOW, "chat");
    let asks: Vec<&Event> = held.iter().filter(|e| e.kind == "needs-decision").collect();
    assert_eq!(asks.len(), 20);
    assert!(asks.iter().all(|a| closed(&held, a).is_none()));
    assert_eq!(choosing(), 20);
    assert_eq!(
        REBUILDS.load(Relaxed),
        before,
        "a render on the log it held rebuilt the ask index"
    );
}

#[test]
fn an_event_appended_while_a_reader_holds_the_log_copies_none_of_its_events() {
    let c = log_10k("share-append");
    let held = shared(&c.state).unwrap();
    assert_eq!(held.len(), 10_000);
    one_more(&c);
    let now = shared(&c.state).unwrap();
    assert_eq!(now.len(), 10_001);
    assert_eq!(held.len(), 10_000, "the held log is as it was");
    assert!(
        std::ptr::eq(held.as_ptr(), now.as_ptr()),
        "the new event copied the held log's 10k events"
    );
}
