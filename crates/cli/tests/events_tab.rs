mod common;
use drover::events::Event;
use drover::serve::words;
use serde_json::{json, Value};

const NOW: u64 = 1_000_000;

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
