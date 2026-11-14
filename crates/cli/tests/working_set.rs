mod common;
use common::tmpdir;
use drover::events::{append, fold, read, states, working, Event};
use serde_json::json;
use std::sync::atomic::Ordering::Relaxed;

#[test]
fn working_set_drops_quiet_history_and_the_fold_is_saved_on_append() {
    let d = tmpdir("working-set");
    let p = d.join("events.jsonl");
    let ev = |n: Option<u64>, kind: &str, d| Event::new(1, "app", n, kind, d);
    let lane = json!({"role": "impl", "agent": "app-impl-1"});
    append(&p, &ev(Some(1), "spawn", lane.clone())).unwrap();
    append(&p, &ev(Some(1), "verdict", json!({"verdict": "PASS", "sha": "a"}))).unwrap();
    append(&p, &ev(Some(1), "merged", json!({}))).unwrap();
    append(
        &p,
        &ev(Some(2), "spawn", json!({"role": "impl", "agent": "app-impl-2"})),
    )
    .unwrap();
    append(&p, &ev(Some(2), "kick", json!({"why": "board"}))).unwrap();
    for _ in 0..10_001 {
        append(&p, &ev(None, "drive-pass", json!({}))).unwrap();
    }

    let r0 = drover::db::ROWS.load(Relaxed);
    let st = states(&p).unwrap();
    assert_eq!(
        drover::db::ROWS.load(Relaxed) - r0,
        0,
        "the saved fold is current: no event read"
    );
    assert_eq!(st[&("app".to_string(), 1)].state, "merged");

    let w = working(&p).unwrap();
    let kinds = |n: Option<u64>| {
        w.iter()
            .filter(|e| e.issue == n)
            .map(|e| e.kind.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        kinds(Some(1)),
        ["spawn", "merged"],
        "a quiet merged issue keeps its lane and its merge"
    );
    assert_eq!(kinds(Some(2)), ["spawn", "kick"], "an open issue keeps every event");
    assert_eq!(
        kinds(None).len(),
        10_000,
        "old drive noise: only the last 10,000 events"
    );
    assert_eq!(w.len(), 10_004);
    let all = read(&p).unwrap();
    let json = |s: drover::events::States| serde_json::to_string(&s.into_values().collect::<Vec<_>>()).unwrap();
    assert_eq!(
        json(fold(&w)),
        json(fold(&all)),
        "the working set's fold is the whole log's"
    );
}
