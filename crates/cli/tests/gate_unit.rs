//! `gate::ci_green`, moved out of `src/gate.rs`.
use drover::gate::ci_green;
use serde_json::{json, Value};

fn run(name: &str, conclusion: &str, started: &str) -> Value {
    json!({"__typename": "CheckRun", "name": name, "status": "COMPLETED", "conclusion": conclusion, "startedAt": started})
}
fn req() -> Vec<String> {
    vec!["quality".into()]
}

#[test]
fn required_quality_ignores_narrowed_failure() {
    let r = json!([
        run("quality", "SUCCESS", "2026-10-31T10:00:00Z"),
        run("quality-narrowed", "FAILURE", "2026-10-31T09:00:00Z")
    ]);
    assert_eq!(ci_green(&r, &req()), Ok(()));
}

#[test]
fn required_quality_missing_is_refused() {
    let r = json!([run("quality-narrowed", "SUCCESS", "2026-10-31T09:00:00Z")]);
    assert_eq!(ci_green(&r, &req()), Err("required check `quality` is missing".into()));
    assert_eq!(
        ci_green(&json!([]), &req()),
        Err("required check `quality` is missing".into())
    );
}

#[test]
fn newer_success_supersedes_older_failure() {
    let r = json!([
        run("quality", "SUCCESS", "2026-10-31T10:00:00Z"),
        run("quality", "CANCELLED", "2026-10-31T09:00:00Z")
    ]);
    assert_eq!(ci_green(&r, &req()), Ok(()));
}

#[test]
fn newer_failure_is_refused() {
    let r = json!([
        run("quality", "SUCCESS", "2026-10-31T09:00:00Z"),
        run("quality", "FAILURE", "2026-10-31T10:00:00Z")
    ]);
    assert_eq!(ci_green(&r, &req()), Err("required check `quality` is failed".into()));
}

#[test]
fn pending_required_is_refused() {
    let r = json!([{"__typename": "CheckRun", "name": "quality", "status": "IN_PROGRESS", "startedAt": "2026-10-31T10:00:00Z"}]);
    assert_eq!(ci_green(&r, &req()), Err("required check `quality` is pending".into()));
}

#[test]
fn unset_checks_every_check() {
    let r = json!([
        run("quality", "SUCCESS", "2026-10-31T10:00:00Z"),
        run("quality-narrowed", "FAILURE", "2026-10-31T09:00:00Z")
    ]);
    assert_eq!(ci_green(&r, &[]), Err("check `quality-narrowed` is failed".into()));
    assert_eq!(ci_green(&json!([run("quality", "SUCCESS", "")]), &[]), Ok(()));
    assert_eq!(ci_green(&json!([]), &[]), Err("no checks reported".into()));
}

#[test]
fn superseded_failure_at_the_same_second_is_ignored() {
    let ev = |c: &str, done: &str, id: u64| {
        json!({"__typename": "CheckRun", "name": "evidence", "status": "COMPLETED", "conclusion": c,
            "startedAt": "2026-11-12T02:32:37Z", "completedAt": done, "databaseId": id})
    };
    let mut later = run("evidence", "SUCCESS", "2026-11-12T02:32:46Z");
    let mut last = run("evidence", "SUCCESS", "2026-11-12T02:32:56Z");
    later["databaseId"] = json!(3);
    last["databaseId"] = json!(4);
    let r = json!([
        ev("SUCCESS", "2026-11-12T02:33:10Z", 2),
        later,
        last,
        ev("FAILURE", "2026-11-12T02:33:00Z", 1)
    ]);
    assert_eq!(ci_green(&r, &[]), Ok(()));
    assert_eq!(ci_green(&r, &["evidence".into()]), Ok(()));
}

#[test]
fn same_start_ties_break_by_completed_then_id() {
    let at = |c: &str, done: &str, id: u64| {
        json!({"__typename": "CheckRun", "name": "evidence", "status": "COMPLETED", "conclusion": c,
            "startedAt": "2026-11-12T02:32:37Z", "completedAt": done, "databaseId": id})
    };
    let req = vec!["evidence".to_string()];
    let by_done = json!([
        at("SUCCESS", "2026-11-12T02:33:10Z", 1),
        at("FAILURE", "2026-11-12T02:33:00Z", 2)
    ]);
    assert_eq!(ci_green(&by_done, &req), Ok(()));
    assert_eq!(ci_green(&by_done, &[]), Ok(()));
    let by_id = json!([
        at("FAILURE", "2026-11-12T02:33:00Z", 7),
        at("SUCCESS", "2026-11-12T02:33:00Z", 9)
    ]);
    assert_eq!(ci_green(&by_id, &req), Ok(()));
    assert_eq!(ci_green(&by_id, &[]), Ok(()));
    let red = json!([
        at("FAILURE", "2026-11-12T02:33:00Z", 9),
        at("SUCCESS", "2026-11-12T02:33:00Z", 7)
    ]);
    assert_eq!(ci_green(&red, &[]), Err("check `evidence` is failed".into()));
}

#[test]
fn latest_failed_run_of_a_name_stays_red() {
    let r = json!([
        run("evidence", "SUCCESS", "2026-11-12T02:32:37Z"),
        run("evidence", "FAILURE", "2026-11-12T02:32:56Z"),
        run("quality", "SUCCESS", "2026-11-12T02:32:37Z")
    ]);
    assert_eq!(ci_green(&r, &[]), Err("check `evidence` is failed".into()));
    assert_eq!(
        ci_green(&r, &["evidence".into()]),
        Err("required check `evidence` is failed".into())
    );
}
