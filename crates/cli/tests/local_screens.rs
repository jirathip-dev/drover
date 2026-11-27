mod common;
use common::*;
use drover::events::{append, read, Event};
use drover_chat::Local;
use serde_json::{json, Value};

const T: u64 = 1_000_000;
const HTTP: &str = "127.0.0.1:7777";

fn state(name: &str) -> (drover::Ctx, OnShed) {
    let c = ctx(&tmpdir(name), CONFIG);
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "17", "20"]).0,
        0
    );
    let ev = |ts: u64, n: Option<u64>, kind: &str, d: Value| {
        append(&c.state, &Event::new(ts, "sendmeter", n, kind, d)).unwrap();
    };
    ev(
        T - 5_000,
        Some(12),
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude"}),
    );
    let ask = json!({"id": "q7", "question": "Move the branch out?", "options": ["move", "refuse"], "recommend": 1, "kind": "routine"});
    ev(T - 300, None, "needs-decision", ask);
    let r = OnShed::new(Fake::default());
    r.1.lane("sendmeter-impl-12", "working", "/x");
    (c, r)
}

#[test]
fn the_door_builds_what_http_serves_with_no_socket_and_prunes_what_the_terminal_does_not_offer() {
    let (c, r) = state("local-parity");
    let door = Local::new(&c, &r);
    let s = drover::serve::Server::new(&c, &r, Some(HTTP.into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let get = |p: &str| {
        let res = s.respond(&format!("GET {p} HTTP/1.1\r\nHost: {HTTP}\r\n{ck}\r\n"), T);
        let res = String::from_utf8_lossy(&res).into_owned();
        serde_json::from_str::<Value>(res.split_once("\r\n\r\n").unwrap().1).unwrap()
    };
    for p in [
        "/screens/queue",
        "/screens/queue?repo=sendmeter",
        "/screens/lanes",
        "/screens/lanes/sendmeter-impl-12",
        "/screens/fleets",
        "/screens/workflow",
        "/screens/issue?repo=sendmeter&issue=17",
    ] {
        assert_eq!(door.screen(T, p), Some(get(p)), "{p}");
    }
    assert_eq!(
        door.screen(T, &Local::cards()).unwrap()["blocks"],
        get("/screens/cards")
    );
    assert_eq!(Local::home(Some("sendmeter")), "/screens/queue?repo=sendmeter");
    for gone in [
        "/screens/legend",
        "/screens/scene",
        "/screens/threads",
        "/screens/threads/t1",
    ] {
        assert_eq!(door.screen(T, gone), None, "{gone}");
    }
    let ev = door.screen(T, "/screens/events").unwrap().to_string();
    assert!(
        !ev.contains("\"thread\":\"sendmeter-impl-12\""),
        "a lane's thread tap is its sheet: {ev}"
    );
    assert!(ev.contains("/screens/lanes/sendmeter-impl-12"), "{ev}");
}

#[test]
fn the_door_runs_the_phones_handler_with_its_permissions_and_refusals() {
    let (c, r) = state("local-act");
    let door = Local::new(&c, &r);
    let before = read(&c.state).unwrap().len();
    let merge = json!({"screen": "queue", "block": "q-sendmeter-17", "action": "merge", "values": {"fleet": "sendmeter", "issue": 17}});
    assert!(door.act(T, &merge).err().unwrap().starts_with("REFUSED NOT_PERMITTED"));
    let secret = json!({"screen": "cards", "block": "s1", "action": "secret-card", "values": {"act": "save", "name": "K", "text": "v"}});
    assert!(door.act(T, &secret).err().unwrap().starts_with("REFUSED NOT_PERMITTED"));
    assert_eq!(read(&c.state).unwrap().len(), before, "a refused action logs nothing");
    let stamp = door.stamp();
    let drop17 = json!({"screen": "queue", "block": "q-sendmeter-17", "action": "drop", "values": {"fleet": "sendmeter", "issue": 17}});
    let done = door.act(T, &drop17).unwrap();
    assert_ne!(done.toast, "");
    let last = read(&c.state).unwrap().to_vec().pop().unwrap();
    assert_eq!((last.kind.as_str(), last.val("op")), ("queue", &json!("rm")));
    assert_ne!(door.stamp(), stamp, "the stamp moves with the log");
    let filter = json!({"screen": "events", "block": "kind", "action": "filter", "values": {"kind": ["merge"]}});
    let shown = door.act(T, &filter).unwrap().screen.unwrap();
    assert_eq!(shown["id"], "events");
}
