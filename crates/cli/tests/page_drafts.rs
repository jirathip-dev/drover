mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::serve::Server;
use serde_json::{json, Value};

#[test]
fn an_answered_card_leaves_the_cards_at_once_and_stays_gone_while_its_answer_runs() {
    if no_webkit() {
        return;
    }
    let c = ctx(&tmpdir("page-answered"), CONFIG);
    let now = drover::now();
    let ev = |n: Option<u64>, kind: &str, d: Value| {
        append(&c.state, &Event::new(now - 600, "sendmeter", n, kind, d)).unwrap();
    };
    let agent = "sendmeter-architect-1121";
    ev(
        Some(1121),
        "spawn",
        json!({"agent": agent, "pane": "%41", "role": "architect"}),
    );
    ev(
        Some(1121),
        "design-review",
        json!({"agent": agent, "role": "architect"}),
    );
    for (id, n, role) in [("q2", 1121, "architect"), ("q3", 1122, "design")] {
        let q = format!("Approve the {role} of #{n}?");
        let d =
            json!({"id": id, "question": q, "options": ["approve", "revise", "reject"], "design": true, "role": role});
        ev(Some(n), "needs-decision", d);
    }
    ev(
        None,
        "needs-decision",
        json!({"id": "q1", "question": "Ship the slow path?", "options": ["yes", "no"]}),
    );
    let ck = pair(
        &Server::new(&c, &Fake::default(), Some("127.0.0.1:7777".into())),
        &c,
        "Mozilla/5.0 (iPhone)",
        now,
    );
    let r = OnShed::new(Fake::default());
    r.1.add("%41", json!({}));
    r.1.lock().held = Some("%41".into());
    let (shed, moved) = (r.1.state.clone(), r.1.moved.clone());
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c2, &r, l, false));
    let o = std::process::Command::new("node")
        .arg(format!("{}/tests/ui/answered.mjs", env!("CARGO_MANIFEST_DIR")))
        .arg(format!("http://{addr}/"))
        .arg(ck.trim().trim_start_matches("Cookie: "))
        .output()
        .unwrap();
    shed.lock().unwrap().held = None;
    moved.notify_all();
    let out: Value =
        serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&o.stderr)));
    assert!(out.get("error").is_none() && out["errors"] == json!([]), "{out}");
    assert_eq!(out["before"], json!(["ask-q1", "ask-q2", "ask-q3"]), "{out}");
    for (id, left) in [
        ("ask-q2", json!(["ask-q1", "ask-q3"])),
        ("ask-q1", json!(["ask-q3"])),
        ("ask-q3", json!([])),
    ] {
        let s = &out["steps"][id];
        assert_eq!(s["status"], 200, "{id}: {out}");
        assert_eq!(
            (&s["at1s"], &s["after"]),
            (&json!(false), &json!(false)),
            "{id} is gone within 1 s and stays gone 3 s later while its answer runs: {out}"
        );
        assert_eq!(s["left"], left, "{id}: the other cards stay: {out}");
    }
    let note = json!({"value": "make the rows taller", "same": true});
    assert_eq!(
        (&out["typed"], &out["note"]),
        (&note, &note),
        "the typed revise note survives another card's answer: {out}"
    );
    assert_eq!(out["reloaded"], false, "no page reload: {out}");
    let want = [("q2", "approve"), ("q1", "yes"), ("q3", "make the rows taller")];
    let decided = || {
        let log = read(&c.state).unwrap().to_vec();
        want.iter().all(|(id, pick)| {
            log.iter()
                .any(|e| e.kind == "decision" && e.str("id") == Some(id) && e.str("choice") == Some(pick))
        })
    };
    for _ in 0..150 {
        if decided() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(decided(), "the held answers land once released: {out}");
}
