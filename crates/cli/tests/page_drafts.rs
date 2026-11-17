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
    let (c, o) = thread_t2("page-drafts", "drafts.mjs", &[]);
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

fn thread_t2(name: &str, script: &str, args: &[&str]) -> (drover::Ctx, std::process::Output) {
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
    let (_c, o) = thread_t2("page-repaint", "repaint.mjs", &[]);
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
