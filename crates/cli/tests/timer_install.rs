//! #461: every `timer install` logs `timer-install` with its `by`; a conductor's shows in its thread.
//! Its own test binary: `HERDSMAN_LANE` and `HERDSMAN_THREAD` are process-wide.
mod common;
use common::*;
use herdsman::events::{read, Event};
use serde_json::json;

fn installs(c: &herdsman::Ctx) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "timer-install")
        .collect()
}

#[test]
fn timer_install_logs_who_ran_it() {
    let c = ctx(&tmpdir("timer-install-log"), CONFIG);
    let r = Fake::default();
    std::env::remove_var("HERDSMAN_LANE");
    std::env::remove_var("HERDSMAN_THREAD");
    assert_eq!(run(&c, &r, &["thread", "new", "sendmeter", "Timer"]).1, "t1\n");

    assert_eq!(run(&c, &r, &["timer", "install", "--no-load"]).0, 0);
    let e = &installs(&c)[0];
    assert_eq!(e.str("by"), Some("owner"));
    let jobs = json!([
        "dev.herdsman.tick",
        "dev.herdsman.fast",
        "dev.herdsman.update",
        "dev.herdsman.clean"
    ]);
    assert_eq!((&e.data["written"], &e.data["reloaded"]), (&jobs, &json!([])));
    assert!(e.str("thread").is_none());

    std::env::set_var("HERDSMAN_LANE", "sendmeter:-:orchestrator");
    std::env::set_var("HERDSMAN_THREAD", "t1");
    assert_eq!(run(&c, &r, &["timer", "install", "--no-load"]).0, 0);
    let e = &installs(&c)[1];
    assert_eq!(
        (e.str("by"), e.str("thread")),
        (Some("sendmeter:-:orchestrator"), Some("t1"))
    );
    assert_eq!(e.data["written"], json!([]), "already written");
    std::env::remove_var("HERDSMAN_LANE");
    std::env::remove_var("HERDSMAN_THREAD");
    let (_, show) = run(&c, &r, &["thread", "show", "t1"]);
    let line = "system: sendmeter:-:orchestrator ran `herdsman timer install`: wrote [], reloaded []";
    assert!(show.contains(line), "{show}");
}
