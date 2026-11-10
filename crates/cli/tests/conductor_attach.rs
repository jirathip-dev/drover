//! #633: a cooee thread attached as a repo's conductor (`POST /api/v1/conductor-attach`): its `guarded`, its asks in
//! `/api/v1/events`, lane asks routed to it, and the link ended by detach or archive. Its own test binary:
//! `HERDSMAN_LANE` is process-wide.
mod common;
use common::*;
use herdsman::events::read;
use herdsman::serve::Server;
use serde_json::{json, Value};

const T: u64 = 1_000_000;
const COOEE: &str = "cooee-7f3a";

fn call(s: &Server, ck: &str, (method, path): (&str, &str), body: &str) -> Value {
    let r = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n{ck}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&r, T)).into_owned();
    serde_json::from_str(res.split_once("\r\n\r\n").unwrap().1).unwrap()
}

/// `ask --thread <t>` on sendmeter: exit code and output.
fn ask(c: &herdsman::Ctx, r: &Fake, thread: Option<&str>) -> (i32, String) {
    let mut a = vec![
        "ask",
        "sendmeter",
        "12",
        "Which scope?",
        "--option",
        "small",
        "--option",
        "full",
    ];
    a.extend(thread.map(|t| ["--thread", t]).into_iter().flatten());
    run(c, r, &a)
}

#[test]
fn an_attached_cooee_thread_is_the_repos_conductor_until_detached() {
    // The impl role (the conductor's fallback, profile `lane-conductor`) on Claude with an account: the guard's other
    // checks hold, so `guarded` turns on the synced hook alone.
    let cfg = CONFIG.replace(
        "harness  =  \"hermes\"    # toggled in tests",
        "harness = \"claude\"\naccount = \"C\"",
    ) + "\n[accounts]\naccount_env = \"fake-cooee accounts env {provider} {name}\"\nC = \"C\"\n";
    let c = ctx(&tmpdir("conductor-attach"), &cfg);
    // #771: lanes (and any conductor herdsman would start) on shed.
    let b = FakeShed::new();
    b.attach(&c);
    let r = Fake::default().reply("gh", "").reply(
        "fake-cooee accounts env claude C",
        r#"{"CLAUDE_CONFIG_DIR": "/accounts/claude/C"}"#,
    );
    let on = b.on(&r);
    let s = Server::new(&c, &on, Some("127.0.0.1:7777".into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let post = |op: &str, v: Value| call(&s, &ck, ("POST", &format!("/api/v1/{op}")), &v.to_string());
    let attach = json!({"fleet": "sendmeter", "thread": COOEE});

    // Unpaired: refused. An unattached id is still NO_SUCH_THREAD.
    let anon = call(&s, "", ("POST", "/api/v1/conductor-attach"), &attach.to_string());
    assert_eq!(anon["code"], "UNPAIRED", "{anon}");
    let (code, out) = ask(&c, &r, Some(COOEE));
    assert!(code == 3 && out.contains("REFUSED NO_SUCH_THREAD"), "{out}");

    // No settings synced: attached, but not guarded, and why.
    let a = post("conductor-attach", attach.clone());
    assert_eq!(
        (a["ok"].clone(), a["guarded"].clone()),
        (json!(true), json!(false)),
        "{a}"
    );
    assert!(
        a["why"].as_str().unwrap().contains("no herdsman PreToolUse hook"),
        "{a}"
    );
    let settings = c.config.with_file_name("claude").join("lane-conductor/settings.json");
    assert_eq!(a["settings"], settings.to_string_lossy().as_ref());
    assert_eq!(a["env"]["HERDSMAN_LANE"], "sendmeter:-:conductor");
    assert_eq!(a["env"]["HERDSMAN_THREAD"], COOEE);
    // #683: the account's folder, no token.
    assert_eq!(a["env"]["CLAUDE_CONFIG_DIR"], "/accounts/claude/C");
    assert!(a["env"].get("CLAUDE_CODE_OAUTH_TOKEN").is_none(), "{a}");
    assert!(
        a["env"]["HERDSMAN_PORT"].as_str().unwrap().parse::<u16>().is_ok(),
        "{a}"
    );
    let brief = a["brief"].as_str().unwrap();
    assert!(
        brief.contains("# ROLE: CONDUCTOR") && brief.contains("Fleet for asks: sendmeter"),
        "{brief}"
    );
    // `profiles sync` writes the hook: guarded.
    assert_eq!(run(&c, &r, &["profiles", "sync"]).0, 0);
    let a = post("conductor-attach", attach.clone());
    assert_eq!(
        (a["guarded"].clone(), a["why"].clone()),
        (json!(true), json!("")),
        "{a}"
    );
    let attached = read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "conductor-attached")
        .count();
    assert_eq!(attached, 2);

    // Its ask is accepted and in the events with its thread, options and recommend; nothing typed into a lane.
    let seq = call(&s, &ck, ("GET", "/api/v1/events?since=0"), "")["seq"]
        .as_u64()
        .unwrap();
    let (code, out) = run(
        &c,
        &r,
        &[
            "ask",
            "sendmeter",
            "12",
            "Which scope?",
            "--option",
            "small",
            "--option",
            "full",
            "--recommend",
            "2",
            "--thread",
            COOEE,
        ],
    );
    assert_eq!((code, out.as_str()), (0, "q1\n"));
    let ev = call(&s, &ck, ("GET", &format!("/api/v1/events?since={seq}")), "");
    let e = &ev["list"].as_array().unwrap()[0];
    assert_eq!(e["type"], "needs-decision", "{ev}");
    assert_eq!(e["data"]["thread"], COOEE);
    assert_eq!(e["data"]["options"], json!(["small", "full"]));
    assert_eq!(e["data"]["recommend"], 2);
    // The card's answer comes back through /api/v1/answer: decided, no herdsman conductor started for it.
    let m = call(&s, &ck, ("GET", "/api/v1/state"), "")["mtime"].clone();
    let calls = r.calls.borrow().len();
    let ans = post(
        "answer",
        json!({"fleet": "sendmeter", "id": "q1", "choice": "2", "mtime": m}),
    );
    assert_eq!(ans["ok"], true, "{ans}");
    let d = read(&c.state)
        .unwrap()
        .into_iter()
        .rfind(|e| e.kind == "decision")
        .unwrap();
    assert_eq!((d.str("id"), d.str("choice")), (Some("q1"), Some("full")));
    let started = r.calls.borrow()[calls..].iter().any(|c| c.starts_with("shed run"));
    assert!(
        !started && !b.methods().iter().any(|m| m == "run"),
        "{:?} {:?}",
        r.calls.borrow(),
        b.methods()
    );
    let starts = read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "conductor-start")
        .count();
    assert_eq!(starts, 0);

    // #541: a lane's ask is routed to it (the event carries `routed`; cooee posts it), not typed into a conductor.
    std::env::set_var("HERDSMAN_LANE", "sendmeter:12:impl");
    let (code, _) = ask(&c, &r, None);
    std::env::remove_var("HERDSMAN_LANE");
    assert_eq!(code, 0);
    let q = read(&c.state)
        .unwrap()
        .into_iter()
        .rfind(|e| e.kind == "needs-decision")
        .unwrap();
    assert_eq!(q.str("routed"), Some(COOEE), "{:?}", q.data);
    assert!(
        !r.calls.borrow().iter().any(|c| c.starts_with("shed send")) && !b.methods().iter().any(|m| m == "send"),
        "{:?} {:?}",
        r.calls.borrow(),
        b.methods()
    );

    // Detach ends the link: NO_SUCH_THREAD again, and a second detach is refused.
    assert_eq!(post("conductor-detach", attach.clone())["ok"], true);
    assert!(ask(&c, &r, Some(COOEE)).1.contains("REFUSED NO_SUCH_THREAD"));
    assert_eq!(post("conductor-detach", attach.clone())["code"], "NO_SUCH_THREAD");
    // So does archiving it.
    post("conductor-attach", attach.clone());
    assert_eq!(ask(&c, &r, Some(COOEE)).0, 0);
    assert_eq!(run(&c, &r, &["thread", "archive", COOEE]).0, 0);
    assert!(ask(&c, &r, Some(COOEE)).1.contains("REFUSED NO_SUCH_THREAD"));
    // Attached again after the archive: the link holds again.
    post("conductor-attach", attach);
    assert_eq!(ask(&c, &r, Some(COOEE)).0, 0);
}
