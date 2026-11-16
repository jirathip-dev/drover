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
    let c = ctx(
        &tmpdir("page-drafts"),
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
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ui/drafts.mjs"))
        .arg(format!("http://{addr}/"))
        .arg(ck.trim().trim_start_matches("Cookie: "))
        .output()
        .unwrap();
    let out: Value =
        serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&o.stderr)));
    assert!(out.get("error").is_none() && out["errors"] == json!([]), "{out}");
    let typed = json!({"value": "half a thought", "focus": true, "caret": [4, 6], "draft": "half a thought"});
    let before = &out["before"];
    let after = &out["after"];
    for k in ["value", "focus", "caret", "draft"] {
        assert_eq!(after[k], typed[k], "5 stream ops and a 30 s tick kept {k}: {out}");
    }
    let moved = (after["top"].as_f64().unwrap() - before["top"].as_f64().unwrap()).abs();
    assert!(moved <= 1.0, "#65: a live update never moves what is on screen: {out}");
    assert_eq!(
        after["tick"][0], after["tick"][1],
        "the 30 s repaint keeps the scroll: {out}"
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
