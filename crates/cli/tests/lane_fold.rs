mod common;
use common::{ctx, run, tmpdir, Fake, CONFIG};
use drover::events::{append, closed, lanes, last_spawn, read, working, Event};
use serde_json::json;
use std::sync::atomic::Ordering::Relaxed;

#[test]
fn quiet_merged_lanes_and_old_ask_endings_come_from_the_saved_folds() {
    let c = ctx(&tmpdir("lane-fold"), CONFIG);
    let put = |n: Option<u64>, kind: &str, d| append(&c.state, &Event::new(1, "sendmeter", n, kind, d)).unwrap();
    let agent = "sendmeter-impl-7";
    let lane = |pane: &str| json!({"role": "impl", "agent": agent, "pane": pane});
    let ask = |id: &str| json!({"id": id, "question": "go?", "agent": agent});
    put(Some(7), "spawn", lane("p1"));
    put(Some(7), "spawn", lane("p1"));
    put(Some(7), "needs-decision", ask("q1"));
    put(Some(7), "spawn", lane("p2"));
    put(Some(7), "decision", json!({"id": "q1", "choice": "yes", "by": "owner"}));
    put(Some(8), "needs-decision", ask("q2"));
    put(Some(8), "needs-decision", ask("q2"));
    put(Some(7), "merged", json!({}));
    for _ in 0..10_001 {
        put(None, "drive-pass", json!({}));
    }

    let w = working(&c.state).unwrap();
    let spawns: Vec<_> = w.iter().filter(|e| e.kind == "spawn").map(|e| e.seq).collect();
    assert_eq!(
        spawns,
        [2, 4],
        "a quiet merged issue keeps each lane's newest spawn, not its history"
    );
    assert_eq!(last_spawn(&w, agent).and_then(|e| e.str("pane")), Some("p2"));
    let all = read(&c.state).unwrap();
    let seqs = |v: Vec<&Event>| v.iter().map(|e| e.seq).collect::<Vec<_>>();
    assert_eq!(
        seqs(lanes(&w)),
        seqs(lanes(&all.to_vec())),
        "the working set's lane fold is the whole log's"
    );
    let asks = all.iter().filter(|e| e.kind == "needs-decision");
    let ends: Vec<_> = asks.map(|a| closed(&all, a)).collect();
    let one_pass = all.to_vec();
    let again: Vec<_> = (one_pass.iter().filter(|e| e.kind == "needs-decision"))
        .map(|a| closed(&one_pass, a))
        .collect();
    assert_eq!(ends, [Some("answered"), None, Some("repeat")]);
    assert_eq!(
        ends, again,
        "the saved ask fold ends each ask as one pass over the whole log does"
    );

    let (code, out) = run(&c, &Fake::default(), &["kill", agent]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.starts_with(&format!("{agent} · merged · ")) && out.contains("dry run"),
        "{out}"
    );

    let r0 = drover::db::ROWS.load(Relaxed);
    let (_, open) = run(&c, &Fake::default(), &["asks"]);
    let (_, every) = run(&c, &Fake::default(), &["asks", "--all"]);
    let (code, status) = run(&c, &Fake::default(), &["status", "--json"]);
    assert_eq!(code, 0, "{status}");
    assert_eq!(
        drover::db::ROWS.load(Relaxed) - r0,
        0,
        "asks and status --json read the saved folds, no log"
    );
    assert_eq!(open.lines().count(), 1, "{open}");
    assert!(open.starts_with("sendmeter q2 #8 ") && !open.contains('['), "{open}");
    assert!(
        every.contains("q1 #7") && every.contains("[answered]") && every.contains("[repeat]"),
        "{every}"
    );
}
