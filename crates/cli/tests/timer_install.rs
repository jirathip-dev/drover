mod common;
use common::*;
use drover::events::{read, Event};
use serde_json::json;

fn installs(c: &drover::Ctx) -> Vec<Event> {
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
    std::env::remove_var("DROVER_LANE");
    std::env::remove_var("DROVER_THREAD");
    assert_eq!(run(&c, &r, &["thread", "new", "sendmeter", "Timer"]).1, "t1\n");

    assert_eq!(run(&c, &r, &["timer", "install", "--no-load"]).0, 0);
    let e = &installs(&c)[0];
    assert_eq!(e.str("by"), Some("owner"));
    let jobs = json!([
        "dev.drover.tick",
        "dev.drover.fast",
        "dev.drover.update",
        "dev.drover.clean"
    ]);
    assert_eq!((&e.data["written"], &e.data["reloaded"]), (&jobs, &json!([])));
    assert!(e.str("thread").is_none());

    std::env::set_var("DROVER_LANE", "sendmeter:-:orchestrator");
    std::env::set_var("DROVER_THREAD", "t1");
    assert_eq!(run(&c, &r, &["timer", "install", "--no-load"]).0, 0);
    let e = &installs(&c)[1];
    assert_eq!(
        (e.str("by"), e.str("thread")),
        (Some("sendmeter:-:orchestrator"), Some("t1"))
    );
    assert_eq!(e.data["written"], json!([]), "already written");
    std::env::remove_var("DROVER_LANE");
    std::env::remove_var("DROVER_THREAD");
    let (_, show) = run(&c, &r, &["thread", "show", "t1"]);
    let line = "system: sendmeter:-:orchestrator ran `drover timer install`: wrote [], reloaded []";
    assert!(show.contains(line), "{show}");
}
