mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::serve::Server;
use serde_json::{json, Value};

#[test]
fn typed_text_focus_caret_and_scroll_survive_repaints_and_a_draft_survives_a_reload_until_sent() {
    if no_webkit() {
        return;
    }
    let (c, o) = thread_t2("page-drafts", "drafts.mjs", &[], &[]);
    let out: Value =
        serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&o.stderr)));
    assert!(out.get("error").is_none() && out["errors"] == json!([]), "{out}");
    let typed = json!({"value": "half a thought", "focus": true, "caret": [4, 6], "draft": "half a thought"});
    let before = &out["before"];
    let after = &out["after"];
    for k in ["value", "focus", "caret", "draft"] {
        assert_eq!(after[k], typed[k], "5 stream ops and a 30 s tick kept {k}: {out}");
    }
    let top = before["top"].as_f64().unwrap();
    let live = after["live"].as_array().unwrap();
    assert!(live.len() >= 5, "{out}");
    for v in live {
        assert!(
            v[0] == live[0][0] && (v[1].as_f64().unwrap() - top).abs() <= 1.0,
            "#65: a live update never moves the scroll or what is on screen: {out}"
        );
    }
    assert_eq!(
        after["tick"][0], after["tick"][1],
        "the 30 s repaint keeps the scroll and the view: {out}"
    );
    assert_eq!(
        (&after["same"], &after["gone"]),
        (&json!(true), &json!(0)),
        "#65: a live update never replaces a kept node: {out}"
    );
    assert_eq!(
        (&out["reloaded"]["value"], &out["reloaded"]["draft"]),
        (&typed["value"], &typed["draft"]),
        "the draft is back after a reload: {out}"
    );
    assert_eq!(
        out["reloaded"]["hash"], "#threads/t2",
        "#1088: a reload keeps the open thread: {out}"
    );
    let failed = &out["failed"];
    assert_eq!(
        (&failed["value"], &failed["draft"]),
        (&typed["value"], &typed["draft"]),
        "{out}"
    );
    assert_eq!(failed["toast"]["red"], true, "{out}");
    assert!(
        failed["toast"]["text"].as_str().unwrap().starts_with("boom: not sent"),
        "{out}"
    );
    assert_eq!(failed["toast"]["retry"], json!(["retry"]), "{out}");
    assert_eq!(
        (&out["retried"]["posts"], &out["retried"]["value"]),
        (&json!(2), &typed["value"]),
        "[retry] posts again: {out}"
    );
    assert_eq!(
        (&out["sent"]["value"], &out["sent"]["draft"]),
        (&json!(""), &Value::Null),
        "{out}"
    );
    let log = read(&c.state).unwrap().to_vec();
    assert!(
        log.iter().any(|e| e.kind == "thread-msg"
            && e.str("text") == Some("half a thought")
            && e.str("from") == Some("owner")),
        "{out}"
    );
}

fn thread_t2(name: &str, script: &str, args: &[&str], more: &[(&str, Value)]) -> (drover::Ctx, std::process::Output) {
    let c = ctx(
        &tmpdir(name),
        &format!("{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\n"),
    );
    let profile = c.config.with_file_name("claude").join("fleet-orchestrator");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    let now = drover::now();
    let ev = |ts: u64, kind: &str, d: Value| append(&c.state, &Event::new(ts, "sendmeter", None, kind, d)).unwrap();
    ev(
        now - 7_200,
        "thread-new",
        json!({"thread": "t2", "title": "Rate limits", "harness": "claude"}),
    );
    for i in 0..40u64 {
        let from = if i % 2 == 0 { "owner" } else { "orchestrator" };
        let text = format!("message {i} with enough words to wrap onto a second line at 390");
        ev(
            now - 7_000 + i,
            "thread-msg",
            json!({"thread": "t2", "from": from, "text": text}),
        );
    }
    for (kind, d) in more {
        ev(now - 600, kind, d.clone());
    }
    let ck = pair(
        &Server::new(&c, &Fake::default(), Some("127.0.0.1:7777".into())),
        &c,
        "Mozilla/5.0 (iPhone)",
        now,
    );
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c2, &OnShed::new(Fake::default()), l, false));
    let o = std::process::Command::new("node")
        .arg(format!("{}/tests/ui/{script}", env!("CARGO_MANIFEST_DIR")))
        .arg(format!("http://{addr}/"))
        .arg(ck.trim().trim_start_matches("Cookie: "))
        .args(args)
        .output()
        .unwrap();
    (c, o)
}

#[test]
fn a_blurred_composer_with_storage_full_keeps_its_text_and_a_repaint_keeps_every_node() {
    if no_webkit() {
        return;
    }
    let (_c, o) = thread_t2("page-repaint", "repaint.mjs", &[], &[]);
    let out: Value =
        serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&o.stderr)));
    assert!(out.get("error").is_none() && out["errors"] == json!([]), "{out}");
    assert_eq!(out["full"], true, "the cached screens fill localStorage: {out}");
    assert_eq!(out["before"]["tag"], "TEXTAREA", "{out}");
    let top = &out["before"]["top"];
    assert!(
        top.as_i64().unwrap() > 0,
        "the messages are scrolled off their end: {out}"
    );
    for step in ["stream", "tick", "resumed"] {
        let s = &out[step];
        assert_eq!(
            (&s["value"], &s["same"]),
            (&json!("half a thought"), &json!(true)),
            "the blurred composer keeps its node and text after the {step}: {out}"
        );
    }
    for step in ["stream", "tick"] {
        let s = &out[step];
        assert_eq!(
            (&s["removed"], &s["fl"], &s["nodes"], &s["top"]),
            (&json!(0), &json!(0), &json!(true), top),
            "5 messages, a no-op replace, a badge and the tick replace no node, flash none and keep the scroll: {out}"
        );
    }
    assert_eq!(out["reloaded"], "half a thought", "the draft survives a reload: {out}");
}

#[test]
fn a_new_orchestrator_message_while_typing_keeps_the_composer_focused_with_its_caret_and_text() {
    if no_webkit() {
        return;
    }
    let (_c, o) = thread_t2("page-compose", "compose.mjs", &[]);
    let out: Value =
        serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&o.stderr)));
    assert!(out.get("error").is_none() && out["errors"] == json!([]), "{out}");
    let typed = json!({"value": "half a thought", "focus": true, "same": true, "caret": [4, 6], "blurs": 0});
    assert_eq!(out["before"], typed, "{out}");
    assert_eq!(
        out["after"], typed,
        "a turn starting and a reply arriving keep the composer, its focus, caret and text: {out}"
    );
    assert_eq!(
        out["typed"], "half! thought",
        "the next key lands in the composer: {out}"
    );
}

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

#[test]
fn a_send_shows_at_once_then_once_and_an_answered_card_goes_at_once_and_stays_gone() {
    if no_webkit() {
        return;
    }
    let ask = json!({"id": "q1", "question": "Ship the slow path?", "options": ["yes", "no"]});
    let (c, o) = thread_t2("page-optimistic", "optimistic.mjs", &[], &[("needs-decision", ask)]);
    let out: Value =
        serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&o.stderr)));
    assert!(out.get("error").is_none() && out["errors"] == json!([]), "{out}");
    let send = &out["send"];
    assert!(
        send["ms"].as_f64().is_some_and(|ms| ms < 100.0),
        "the sent message is in the DOM within 100 ms: {out}"
    );
    assert_eq!(
        send["pending"], true,
        "with the pending style while the POST is held: {out}"
    );
    assert_eq!(
        (&out["confirmed"]["shown"], &out["confirmed"]["pending"]),
        (&json!(1), &json!(0)),
        "once confirmed by the stream it shows once: {out}"
    );
    let answer = &out["answer"];
    assert!(
        answer["ms"].as_f64().is_some_and(|ms| ms < 100.0),
        "the answered card is gone within 100 ms: {out}"
    );
    assert!(
        answer["deltas"].as_u64().unwrap() >= 3,
        "3 s of stream screens still carrying the card: {out}"
    );
    assert_eq!(
        (&answer["back"], &answer["status"], &answer["after"]),
        (&json!(false), &json!(200), &json!(false)),
        "{out}"
    );
    assert_eq!(out["reloads"], 0, "no screen is read again after an action: {out}");
    let log = read(&c.state).unwrap().to_vec();
    assert!(
        log.iter().any(|e| e.kind == "decision" && e.str("id") == Some("q1")),
        "{out}"
    );
}
