mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

fn setup(name: &str, extra: u64) -> drover::Ctx {
    let d = tmpdir(name);
    let c = ctx(
        &d,
        &format!("{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\n"),
    );
    let profile = c.config.with_file_name("claude").join("fleet-orchestrator");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    let r = Fake::default();
    for i in 1..=27 {
        let title = format!("Thread {i}");
        assert_eq!(
            run(&c, &r, &["thread", "new", "sendmeter", &title]).1,
            format!("t{i}\n")
        );
    }
    let ev = |k: &str, d: Value| append(&c.state, &Event::new(T, "sendmeter", None, k, d)).unwrap();
    for i in 1..=extra {
        ev(
            "thread-msg",
            json!({"thread": "t1", "from": "orchestrator", "text": format!("m{i}")}),
        );
    }
    ev(
        "thread-msg",
        json!({"thread": "t1", "from": "orchestrator", "text": "see t27, t999 and `t27`"}),
    );
    ev(
        "thread-handoff",
        json!({"thread": "t1", "title": "Grill", "target": "t27"}),
    );
    ev(
        "thread-msg",
        json!({"thread": "t1", "from": "system", "text": "t27 is asleep"}),
    );
    let ask = [
        "ask",
        "sendmeter",
        "-",
        "Go on in t27?",
        "--option",
        "move to t27",
        "--thread",
        "t1",
    ];
    assert_eq!(run(&c, &r, &ask).1, "q1\n");
    for i in 1..=extra {
        ev(
            "thread-msg",
            json!({"thread": "t1", "from": "orchestrator", "text": format!("n{i}")}),
        );
    }
    assert_eq!(run(&c, &r, &["thread", "archive", "t27"]).0, 0);
    c
}

#[test]
fn only_the_archived_thread_reads_archived_in_the_chat_api() {
    let c = setup("thread-refs", 0);
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let tab: Value = serde_json::from_str(&api(&s, ADDR, &cookie, T + 5, "chat")).unwrap();
    let pages: Vec<(String, bool)> = tab["threads"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| (t["page"]["id"].as_str().unwrap().into(), t["page"]["archived"] == true))
        .collect();
    assert_eq!(pages.len(), 27);
    assert!(pages.iter().all(|(id, a)| *a == (id == "t27")), "{pages:?}");
    let p: Value = serde_json::from_str(&api(&s, ADDR, &cookie, T + 5, "thread/t27?before=99999")).unwrap();
    assert_eq!(
        (p["id"].as_str(), p["archived"].as_bool()),
        (Some("t27"), Some(true)),
        "{p}"
    );
}
