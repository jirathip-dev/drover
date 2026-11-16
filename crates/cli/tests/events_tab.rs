mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::{words, Server};
use serde_json::{json, Value};

const NOW: u64 = 1_000_000;

fn ev(kind: &str, d: Value) -> Event {
    Event::new(NOW, "sendmeter", Some(7), kind, d)
}

#[test]
fn parity_83_each_kind_has_its_glyph_colour_and_weight() {
    let busy = "REFUSED HOST_BUSY: load/cpu 3.09 over 2.5; retry when it drops";
    let golden: &[(Event, &str, &str, &str, bool)] = &[
        (
            ev("merged", json!({"into": "staging", "pr": "161"})),
            "✓",
            "g",
            "merged into staging · PR #161",
            true,
        ),
        (
            ev("verdict", json!({"verdict": "PASS", "round": 1})),
            "✓",
            "g",
            "review PASS · round 1",
            true,
        ),
        (
            ev("verdict", json!({"verdict": "FAIL", "round": 2})),
            "✗",
            "r",
            "review FAIL · round 2",
            true,
        ),
        (
            ev("needs-human", json!({"reason": "4 rounds"})),
            "!",
            "r",
            "needs you · 4 rounds",
            true,
        ),
        (
            ev("needs-decision", json!({"id": "q53", "question": "which db?"})),
            "!",
            "r",
            "asks you (q53) · which db?",
            true,
        ),
        (
            ev("ask-waiting", json!({"reason": "q53 for 35m"})),
            "!",
            "r",
            "still waiting on you · q53 for 35m",
            true,
        ),
        (
            ev(
                "notify-failed",
                json!({"event": "merged", "error": "timed out after 15s"}),
            ),
            "✗",
            "r",
            "alert failed · the merged alert: timed out after 15s",
            true,
        ),
        (
            ev("lane-dead", json!({"reason": "gone"})),
            "✗",
            "r",
            "lane gone · gone",
            true,
        ),
        (
            ev("blocked", json!({"code": "CI_NOT_GREEN", "reason": "pending"})),
            "⊘",
            "a",
            "waiting · #7's CI is not green yet",
            true,
        ),
        (
            ev(
                "driven",
                json!({"code": "ROUND_LIMIT", "result": "REFUSED ROUND_LIMIT: 4 rounds > 3"}),
            ),
            "!",
            "r",
            "needs you · #7: 4 rounds > 3 · your call on its card",
            true,
        ),
        (
            ev("driven", json!({"code": "ERROR", "result": "herdr: no shell"})),
            "✗",
            "r",
            "failed · herdr: no shell",
            true,
        ),
        (
            ev("driven", json!({"code": "EXIT", "result": "exit 4: x"})),
            "▸",
            "l",
            "started · #7 · prompt not confirmed yet",
            true,
        ),
        (
            ev("driven", json!({"code": "HOST_BUSY", "result": busy})),
            "⊘",
            "a",
            "held · the host is busy: load/cpu 3.09 over 2.5 · starts again when it drops",
            false,
        ),
        (
            ev("driven", json!({"step": "merge", "result": "merged"})),
            "·",
            "m",
            "drive · merged · #7",
            false,
        ),
        (
            ev("spawn", json!({"role": "rev", "round": 2})),
            "▸",
            "l",
            "rev started · round 2",
            false,
        ),
        (
            ev("push-sent", json!({"devices": 1})),
            "↗",
            "m",
            "alert sent to 1 phone(s)",
            false,
        ),
        (
            ev("clean", json!({"why": "merged"})),
            "·",
            "m",
            "lane cleaned (merged)",
            false,
        ),
        (
            ev("stall", json!({"reason": "35m in fix(1)"})),
            "!",
            "a",
            "stall · 35m in fix(1)",
            false,
        ),
        (
            ev("decision", json!({"id": "q53", "choice": "1"})),
            "✓",
            "m",
            "you answered q53 · 1",
            false,
        ),
        (
            ev("queue", json!({"op": "add", "issues": [169]})),
            "·",
            "m",
            "queue · add #169",
            false,
        ),
    ];
    for (e, g, t, w, imp) in golden {
        assert_eq!(words(e, "serial"), (*g, *t, w.to_string(), *imp), "{}", e.kind);
    }
}

fn screen(s: &Server, method: &str, path: &str, body: &str, extra: &str) -> (u16, Value) {
    let r = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&r, NOW)).into_owned();
    let (head, body) = res.split_once("\r\n\r\n").unwrap();
    (head[9..12].parse().unwrap(), serde_json::from_str(body).unwrap())
}

#[test]
fn an_open_ask_row_opens_its_choice_recommended_first_and_an_answered_one_offers_none() {
    let c = ctx(&tmpdir("events-asks"), CONFIG);
    let (code, out) = run(&c, &Fake::default(), &["thread", "new", "sendmeter", "scope"]);
    assert_eq!((code, out.trim()), (0, "t1"));
    let at = |ago, issue, kind: &str, d: Value| Event::new(NOW - ago, "sendmeter", issue, kind, d);
    for e in [
        at(
            300,
            Some(7),
            "needs-decision",
            json!({"id": "q1", "question": "Split it?", "options": ["yes", "no"], "recommend": 2, "agent": "sendmeter-impl-7"}),
        ),
        at(
            200,
            None,
            "needs-decision",
            json!({"id": "q2", "question": "Which scope?", "options": ["small", "full"], "recommend": 1, "thread": "t1"}),
        ),
        at(
            100,
            None,
            "needs-decision",
            json!({"id": "q3", "question": "Ship it?", "options": ["ship", "hold"], "recommend": 1}),
        ),
        at(
            50,
            None,
            "decision",
            json!({"id": "q3", "choice": "hold", "by": "owner"}),
        ),
    ] {
        append(&c.state, &e).unwrap();
    }
    let f = Fake::default();
    let s = Server::new(&c, &f, Some("127.0.0.1:7777".into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", NOW);
    let rows = |kind: &str| {
        let (status, v) = screen(&s, "GET", &format!("/screens/events{kind}"), "", "");
        assert_eq!(status, 200, "{v}");
        let ev = v["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["id"] == "today")
            .unwrap();
        let rows = ev["blocks"][0]["rows"].as_array().unwrap().clone();
        rows.into_iter()
            .filter(|r| r["title"].as_str().unwrap().starts_with("asks you"))
            .collect::<Vec<_>>()
    };
    let sheet = |r: &Value| {
        let (status, v) = screen(&s, "GET", r["tap"]["sheet"].as_str().unwrap(), "", "");
        assert_eq!(status, 200, "{v}");
        v
    };
    for kind in ["", "?kind=ask"] {
        let asks = rows(kind);
        assert_eq!(asks.len(), 3, "{asks:?}");
        assert!(asks[0]["title"].as_str().unwrap().contains("(q3)"));
        assert_eq!(asks[0]["subtitle"], "answered · hold", "{:?}", asks[0]);
        assert!(asks[0].get("tap").is_none(), "{:?}", asks[0]);
        let q1 = sheet(&asks[2]);
        let choice = q1["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["type"] == "choice")
            .unwrap();
        assert_eq!(
            (&choice["id"], &choice["action"], &choice["recommended"]),
            (&json!("q1"), &json!("answer"), &json!("no"))
        );
        assert_eq!(
            choice["options"],
            json!([{"value": "no", "label": "no"}, {"value": "yes", "label": "yes"}])
        );
        assert_eq!(choice["other"]["placeholder"], "other…");
    }
    let row = rows("")[1].clone();
    let q2 = sheet(&row);
    let choice = q2["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["type"] == "choice")
        .unwrap();
    assert_eq!(choice["options"][0]["value"], "small", "{q2}");
    let answer =
        json!({"screen": q2["id"], "block": "q2", "action": "answer", "values": {"choice": "full", "other": ""}});
    let (status, v) = screen(&s, "POST", "/action", &answer.to_string(), &ck);
    assert_eq!((status, &v["close"]), (200, &json!(true)), "{v}");
    let q2 = &rows("?kind=ask")[1];
    assert_eq!(q2["subtitle"], "answered · full", "{q2:?}");
    assert_eq!(q2["tap"], json!({"thread": "t1"}), "{q2:?}");
    let closed = sheet(&row);
    assert!(!closed.to_string().contains("\"choice\""), "{closed}");
    assert!(closed.to_string().contains("answered · full"), "{closed}");
}
