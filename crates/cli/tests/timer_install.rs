mod common;
use common::*;
use drover::events::{read, Event};
use serde_json::json;

fn installs(c: &drover::Ctx) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
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
    attach_chat(&c, "sendmeter", "t1");

    assert_eq!(run(&c, &r, &["timer", "install", "--no-load"]).0, 0);
    let e = &installs(&c)[0];
    assert_eq!(e.str("by"), Some("owner"));
    let jobs = json!([
        "dev.drover.tick",
        "dev.drover.service",
        "dev.drover.update",
        "dev.drover.clean"
    ]);
    assert_eq!((e.val("written"), e.val("reloaded")), (&jobs, &json!([])));
    assert!(e.str("thread").is_none());

    set_env("DROVER_LANE", "sendmeter:-:orchestrator");
    set_env("DROVER_THREAD", "t1");
    assert_eq!(run(&c, &r, &["timer", "install", "--no-load"]).0, 0);
    let e = &installs(&c)[1];
    assert_eq!(
        (e.str("by"), e.str("thread")),
        (Some("sendmeter:-:orchestrator"), Some("t1"))
    );
    assert_eq!(*e.val("written"), json!([]), "already written");
    std::env::remove_var("DROVER_LANE");
    std::env::remove_var("DROVER_THREAD");
}
