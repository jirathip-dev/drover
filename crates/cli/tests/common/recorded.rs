use super::{ctx, run, tmpdir, Fake, OnShed, CONFIG};
use drover::events::{append, Event};
use serde_json::{json, Value};

const T: u64 = 1_000_000;

/// The sendmeter fleet the screen tests read at `T` (1,000,000): #9 merged, #12 in a fix round, #5 out of rounds, ask q7, a run card.
pub fn recorded(name: &str) -> (drover::Ctx, OnShed) {
    let c = ctx(&tmpdir(name), &format!("models = [\"opus\", \"sonnet\"]\n{CONFIG}"));
    for args in [
        &["queue", "sendmeter", "add", "17", "20"][..],
        &["queue", "sendmeter", "block", "--on", "17", "20"],
    ] {
        assert_eq!(run(&c, &Fake::default(), args).0, 0);
    }
    let ev = |ts: u64, n: Option<u64>, kind: &str, d: Value| {
        append(&c.state, &Event::new(ts, "sendmeter", n, kind, d)).unwrap();
    };
    let spawn = |ts, n, role: &str, agent: &str| {
        ev(
            ts,
            Some(n),
            "spawn",
            json!({"role": role, "agent": agent, "harness": "claude"}),
        );
    };
    spawn(T - 9_000, 9, "impl", "sendmeter-impl-9");
    ev(T - 600, Some(9), "merged", json!({"pr": 41, "into": "staging"}));
    spawn(T - 5_000, 12, "impl", "sendmeter-impl-12");
    spawn(T - 4_000, 12, "rev", "sendmeter-rev-12-r1");
    ev(
        T - 3_500,
        Some(12),
        "verdict",
        json!({"verdict": "FAIL", "sha": "a".repeat(40)}),
    );
    spawn(T - 3_000, 12, "fix", "sendmeter-impl-12");
    spawn(T - 8_000, 5, "impl", "sendmeter-impl-5");
    ev(
        T - 2_000,
        Some(5),
        "verdict",
        json!({"verdict": "FAIL", "sha": "b".repeat(40)}),
    );
    ev(
        T - 1_900,
        Some(5),
        "needs-human",
        json!({"code": "STUCK", "reason": "review rounds ran out (4/3)"}),
    );
    let ask = json!({"id": "q7", "question": "Move the branch out?", "options": ["move", "refuse"], "recommend": 1, "kind": "routine"});
    ev(T - 300, None, "needs-decision", ask);
    let card = json!({"thread": "t1", "id": "r1", "cmd": "git push origin --delete wip/x", "why": "Delete #9's wip snapshot",
        "blast": "origin: 1 wip/ branch", "dir": "/tmp/sendmeter", "host": "main", "timeout": 60, "sha": "3b7e0c55a1d9".repeat(5)});
    ev(T - 120, None, "run-card", card);
    let r = OnShed::new(Fake::default());
    r.1.lane("sendmeter-impl-12", "working", "/x");
    (c, r)
}
