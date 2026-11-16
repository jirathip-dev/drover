mod common;
use common::*;
use drover::db::ROWS;
use drover::orchestrator::WALKS;
use drover::serve::Server;
use serde_json::{json, Value};
use std::sync::atomic::Ordering::Relaxed;

const ADDR: &str = "127.0.0.1:7777";
const NOW: u64 = 1_000_000;
const EVENTS: usize = 50_000;
const THREADS: u64 = 40;
const MSGS: u64 = 20;

fn cost(f: impl FnOnce()) -> (usize, usize) {
    let (r, w) = (ROWS.load(Relaxed), WALKS.load(Relaxed));
    f();
    (ROWS.load(Relaxed) - r, WALKS.load(Relaxed) - w)
}

#[test]
fn a_quiet_tick_and_a_post_touch_only_new_events() {
    let c = ctx(&tmpdir("chat-index"), CONFIG);
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
        log.push(
            json!({"ts": NOW, "fleet": "sendmeter", "type": "needs-decision", "thread": id,
            "id": format!("q{t}"), "question": "go?", "options": ["yes", "no"], "recommend": 1}),
        );
    }
    while log.len() < EVENTS {
        log.push(json!({"ts": NOW, "fleet": "sendmeter", "issue": 1, "type": "note", "text": "filler"}));
    }
    let jsonl: String = log.iter().map(|e| format!("{e}\n")).collect();
    std::fs::write(&c.state, jsonl).unwrap();

    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let tick = |now: u64| {
        let v: Value = serde_json::from_str(&api(&s, ADDR, "", now, "chat")).unwrap();
        assert_eq!(
            v["threads"].as_array().unwrap().len(),
            usize::try_from(THREADS).unwrap()
        );
        let n: Value = serde_json::from_str(&api(&s, ADDR, "", now, "needs")).unwrap();
        assert_eq!(n["choosing"], THREADS, "every thread waits on its ask");
    };
    tick(NOW + 500);

    for now in [NOW + 501, NOW + 561] {
        let (rows, walks) = cost(|| tick(now));
        assert_eq!(rows, 0, "a quiet tick at {now} read {rows} event rows");
        assert!(walks < EVENTS, "a quiet tick at {now} looked at {walks} events");
    }

    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", NOW + 600);
    tick(NOW + 600);
    for body in ["op=say&t=t1&text=hi", "op=new&repo=sendmeter&text=hello"] {
        let req = format!(
            "POST /thread HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let mut res = String::new();
        let (rows, walks) = cost(|| res = String::from_utf8_lossy(&s.respond(&req, NOW + 601)).into_owned());
        assert!(res.starts_with("HTTP/1.1 200"), "{body}: {res}");
        assert!(rows <= 20, "{body} read {rows} event rows");
        assert!(walks < EVENTS, "{body} looked at {walks} events");
    }
}
