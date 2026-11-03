//! #170: the Events tab: each kind's glyph and colour, `[important]` leaving out the routine kinds, the noise fold
//! by kind (a failure never shares a row with `clean`), and the time groups.
mod common;
use common::*;
use herdsman::events::{append, Event};
use herdsman::serve::{words, Server};
use serde_json::{json, Value};

const NOW: u64 = 1_000_000; // 13:46:40 UTC; the Fake has no `date`, so the page counts days in UTC.

fn ev(kind: &str, d: Value) -> Event {
    Event::new(NOW, "sendmeter", Some(7), kind, d)
}

#[test]
fn each_kind_has_its_glyph_colour_and_weight() {
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

fn tab(events: &[Event]) -> String {
    let c = ctx(&tmpdir("events-tab"), CONFIG);
    for e in events {
        append(&c.state, e).unwrap();
    }
    let f = Fake::default();
    let s = Server::new(&c, &f, Some("127.0.0.1:7777".into()));
    let r = s.respond("GET /frag?id=events HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n\r\n", NOW);
    String::from_utf8_lossy(&r).into_owned()
}

/// A view's rows: `imp` is `[important]`, `all` is `[all]`.
fn view<'a>(h: &'a str, v: &str) -> Vec<&'a str> {
    let a = h.find(&format!("<div data-v={v}")).unwrap();
    let end = h[a + 1..].find("<div data-v=").map_or(h.len(), |e| a + 1 + e);
    h[a..end].split("<details class=\"er").skip(1).collect()
}

#[test]
fn important_leaves_out_routine_kinds_and_capacity_holds() {
    let at = |ts, kind: &str, d: Value| Event::new(NOW - ts, "sendmeter", Some(7), kind, d);
    let h = tab(&[
        at(90, "spawn", json!({"role": "impl"})),
        at(80, "driven", json!({"step": "start", "result": "ok"})),
        at(70, "clean", json!({"why": "merged"})),
        at(60, "push-sent", json!({"devices": 1})),
        at(
            50,
            "driven",
            json!({"code": "HOST_BUSY", "result": "REFUSED HOST_BUSY: load 9"}),
        ),
        at(40, "verdict", json!({"verdict": "PASS", "round": 1})),
        at(30, "merged", json!({"into": "staging", "pr": "9"})),
    ]);
    let imp = view(&h, "imp");
    assert_eq!(imp.len(), 2, "{h}");
    assert!(imp[0].contains("merged into staging · PR #9") && imp[1].contains("review PASS"));
    assert!(imp.iter().all(|r| r.starts_with(" k-") && r.contains(" imp\"")));
    let all = view(&h, "all");
    assert_eq!(all.len(), 7, "{h}");
    assert!(h.contains("<div data-v=all hidden>") && h.contains("data-k=imp aria-pressed=true"));
    assert!(h.contains("data-fl=\"sendmeter\"") && h.contains("aria-label=\"filter events\""));
}

#[test]
fn the_noise_fold_never_hides_a_failure() {
    let at = |ts, kind: &str| {
        Event::new(
            NOW - ts,
            "sendmeter",
            None,
            kind,
            json!({"why": "merged", "error": "x"}),
        )
    };
    // Newest first: clean ×3, a failure, then clean ×2 (one run per kind).
    let h = tab(&[
        at(9, "clean"),
        at(8, "clean"),
        at(7, "notify-failed"),
        at(6, "clean"),
        at(5, "clean"),
        at(5, "clean"),
    ]);
    let all = view(&h, "all");
    assert_eq!(all.len(), 3, "{h}");
    assert!(
        all[0].contains("clean ×3 · lanes cleaned") && all[0].contains("<ul class=fl2>"),
        "{}",
        all[0]
    );
    assert!(all[1].contains("<i class=\"kg t-r\" data-act=blocked>✗</i>") && all[1].contains("alert failed"));
    assert!(all[2].contains("clean ×2 · lanes cleaned"));
    assert!(!all.iter().any(|r| r.contains("clean") && r.contains("alert failed")));
    let imp = view(&h, "imp");
    assert!(imp.len() == 1 && imp[0].contains("alert failed"), "{h}");
}

#[test]
fn rows_sit_under_now_5m_1h_earlier_today_and_yesterday() {
    let at = |ago| {
        Event::new(
            NOW - ago,
            "sendmeter",
            Some(7),
            "merged",
            json!({"into": "staging", "pr": "9"}),
        )
    };
    let h = tab(&[at(60_000), at(30_000), at(5_000), at(600), at(10)]);
    let heads: Vec<&str> = h
        .split("<h3><span>")
        .skip(1)
        .map(|s| &s[..s.find('<').unwrap()])
        .collect();
    let want = ["now", "5m", "1h", "earlier today", "yesterday"];
    assert_eq!(heads, [want, want].concat(), "both views");
    assert!(h.contains("5 important rows · back to yesterday 21:06"), "{h}");
}

#[test]
fn old_rows_are_dated_without_a_date_call_each() {
    let c = ctx(&tmpdir("events-old"), CONFIG);
    for i in 0..30 {
        let e = Event::new(
            NOW - 200_000 - i * 3600,
            "sendmeter",
            Some(7),
            "merged",
            json!({"into": "staging"}),
        );
        append(&c.state, &e).unwrap();
    }
    let f = Fake::default();
    let s = Server::new(&c, &f, Some("127.0.0.1:7777".into()));
    let h = String::from_utf8_lossy(&s.respond("GET /frag?id=events HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n\r\n", NOW))
        .into_owned();
    assert!(
        h.contains("<h3><span>jan 10</span>") && h.contains("<h3><span>jan 9</span>"),
        "{h}"
    );
    assert!(f.count("date ") <= 1, "{:?}", f.calls.borrow());
}
