mod common;
use common::*;
use drover::events::read;
use drover::serve::Server;
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

fn ask(c: &drover::Ctx, r: &Fake, thread: Option<&str>) -> (i32, String) {
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
fn an_attached_cooee_thread_is_the_repos_orchestrator_until_detached() {
    let cfg = CONFIG.replace("harness  =  \"claude\"    # toggled in tests", "harness = \"claude\"");
    let c = ctx(&tmpdir("orchestrator-attach"), &cfg);
    let settings = c
        .config
        .with_file_name("claude")
        .join("lane-orchestrator/settings.json");
    std::fs::remove_file(&settings).unwrap();
    let b = FakeShed::new();
    b.attach(&c);
    let r = Fake::default().reply("gh", "");
    let on = b.on(&r);
    let s = Server::new(&c, &on, Some("127.0.0.1:7777".into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let post = |op: &str, v: Value| call(&s, &ck, ("POST", &format!("/api/v1/{op}")), &v.to_string());
    let attach = json!({"fleet": "sendmeter", "thread": COOEE});

    let anon = call(&s, "", ("POST", "/api/v1/orchestrator-attach"), &attach.to_string());
    assert_eq!(anon["code"], "UNPAIRED", "{anon}");
    let (code, out) = ask(&c, &r, Some(COOEE));
    assert!(code == 3 && out.contains("REFUSED NO_SUCH_THREAD"), "{out}");

    let a = post("orchestrator-attach", attach.clone());
    assert_eq!(
        (a["ok"].clone(), a["guarded"].clone()),
        (json!(true), json!(false)),
        "{a}"
    );
    assert!(a["why"].as_str().unwrap().contains("no drover PreToolUse hook"), "{a}");
    assert_eq!(a["settings"], settings.to_string_lossy().as_ref());
    assert_eq!(a["env"]["DROVER_LANE"], "sendmeter:-:orchestrator");
    assert_eq!(a["env"]["DROVER_THREAD"], COOEE);
    assert!(a["env"].get("CLAUDE_CONFIG_DIR").is_none(), "{a}");
    assert!(a["env"].get("CLAUDE_CODE_OAUTH_TOKEN").is_none(), "{a}");
    assert!(a["env"]["DROVER_PORT"].as_str().unwrap().parse::<u16>().is_ok(), "{a}");
    let brief = a["brief"].as_str().unwrap();
    assert!(
        brief.contains("# ROLE: ORCHESTRATOR") && brief.contains("Fleet for asks: sendmeter"),
        "{brief}"
    );
    assert_eq!(run(&c, &r, &["profiles", "sync"]).0, 0);
    let a = post("orchestrator-attach", attach.clone());
    assert_eq!(
        (a["guarded"].clone(), a["why"].clone()),
        (json!(true), json!("")),
        "{a}"
    );
    let attached = read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "orchestrator-attached")
        .count();
    assert_eq!(attached, 2);

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
    let m = call(&s, &ck, ("GET", "/api/v1/state"), "")["mtime"].clone();
    let calls = r.calls.borrow().len();
    let ans = post(
        "answer",
        json!({"fleet": "sendmeter", "id": "q1", "choice": "2", "mtime": m}),
    );
    assert_eq!(ans["ok"], true, "{ans}");
    let d = read(&c.state)
        .unwrap()
        .to_vec()
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
        .iter()
        .filter(|e| e.kind == "orchestrator-start")
        .count();
    assert_eq!(starts, 0);

    set_env("DROVER_LANE", "sendmeter:12:impl");
    let (code, _) = ask(&c, &r, None);
    std::env::remove_var("DROVER_LANE");
    assert_eq!(code, 0);
    let q = read(&c.state)
        .unwrap()
        .to_vec()
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

    assert_eq!(post("orchestrator-detach", attach.clone())["ok"], true);
    assert!(ask(&c, &r, Some(COOEE)).1.contains("REFUSED NO_SUCH_THREAD"));
    assert_eq!(post("orchestrator-detach", attach.clone())["code"], "NO_SUCH_THREAD");
    post("orchestrator-attach", attach.clone());
    assert_eq!(ask(&c, &r, Some(COOEE)).0, 0);
    assert_eq!(run(&c, &r, &["thread", "archive", COOEE]).0, 0);
    assert!(ask(&c, &r, Some(COOEE)).1.contains("REFUSED NO_SUCH_THREAD"));
    post("orchestrator-attach", attach);
    assert_eq!(ask(&c, &r, Some(COOEE)).0, 0);
}

const TOKEN: &str = "tok-attach";

fn chat(name: &str) -> (drover::Ctx, FakeShed, Fake) {
    let c = ctx(&tmpdir(name), CONFIG);
    std::fs::write(c.config.with_file_name("cooee-token"), TOKEN).unwrap();
    let b = FakeShed::new();
    b.attach(&c);
    (c, b, Fake::default().reply("gh", ""))
}

fn bearer() -> String {
    format!("Authorization: Bearer {TOKEN}\r\n")
}

#[test]
fn a_lane_ask_to_an_attached_chat_starts_no_session_and_its_row_carries_the_routing_line() {
    let (c, b, r) = chat("attach-lane-ask");
    let on = b.on(&r);
    let s = Server::new(&c, &on, Some("127.0.0.1:7777".into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let attach = json!({"fleet": "sendmeter", "thread": COOEE}).to_string();
    assert_eq!(
        call(&s, &ck, ("POST", "/api/v1/orchestrator-attach"), &attach)["ok"],
        true
    );

    set_env("DROVER_LANE", "sendmeter:12:impl");
    let (code, out) = ask(&c, &r, None);
    std::env::remove_var("DROVER_LANE");
    assert_eq!((code, out.as_str()), (0, "q1\n"));
    let shed = |c: &String| c.starts_with("shed run") || c.starts_with("shed send");
    assert!(
        !r.calls.borrow().iter().any(shed) && !b.methods().iter().any(|m| m == "run" || m == "send"),
        "{:?} {:?}",
        r.calls.borrow(),
        b.methods()
    );
    let rows = call(&s, &bearer(), ("GET", &format!("/api/v1/thread/{COOEE}/rows")), "");
    let rows = rows["rows"].as_array().unwrap();
    let row = rows
        .iter()
        .find(|r| r["kind"] == "system")
        .unwrap_or_else(|| panic!("{rows:?}"));
    let prompt = row["prompt"].as_str().unwrap();
    assert!(
        prompt.starts_with("lane ask q1 on #12") && prompt.contains("drover answer sendmeter q1"),
        "{row}"
    );
}

#[test]
fn a_chat_attaches_to_all_or_none_and_no_fleet_may_take_those_names() {
    let (c, b, r) = chat("attach-all-none");
    let on = b.on(&r);
    let s = Server::new(&c, &on, Some("127.0.0.1:7777".into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    for (fleet, id) in [("all", "chat-all"), ("none", "chat-none")] {
        let body = json!({"fleet": fleet, "thread": id}).to_string();
        let a = call(&s, &ck, ("POST", "/api/v1/orchestrator-attach"), &body);
        assert_eq!(
            (a["ok"].clone(), a["env"]["DROVER_THREAD"].clone()),
            (json!(true), json!(id)),
            "{a}"
        );
    }
    let fleets: Vec<String> = read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "orchestrator-attached")
        .map(|e| e.fleet.clone())
        .collect();
    assert_eq!(fleets, ["all", "none"]);
    for name in ["all", "none"] {
        let cfg = CONFIG.replace("fleet.sendmeter", &format!("fleet.{name}"));
        let e = drover::config::Config::parse(&cfg).err().unwrap();
        assert!(
            format!("{e:#}").contains(&format!("fleet.{name}: `all` and `none` are reserved")),
            "{e:#}"
        );
    }
}

#[test]
fn the_cli_attach_logs_the_attach_and_prints_the_brief_the_settings_and_the_env() {
    let (c, b, r) = chat("attach-cli");
    let on = b.on(&r);
    let (code, out) = run(&c, &on, &["attach", "sendmeter", "term-1"]);
    assert_eq!(code, 0, "{out}");
    let events = read(&c.state).unwrap();
    let e = events.iter().rfind(|e| e.kind == "orchestrator-attached").unwrap();
    assert_eq!((e.fleet.as_str(), e.str("thread")), ("sendmeter", Some("term-1")));
    let line = |k: &str| {
        out.lines()
            .find_map(|l| l.strip_prefix(k))
            .unwrap_or_else(|| panic!("{k} {out}"))
    };
    let brief = std::fs::read_to_string(line("brief: ")).unwrap();
    assert!(
        brief.contains("# ROLE: ORCHESTRATOR") && brief.contains("Thread: term-1"),
        "{brief}"
    );
    assert!(line("settings: ").ends_with("lane-orchestrator/settings.json"), "{out}");
    assert_eq!(line("DROVER_LANE="), "sendmeter:-:orchestrator");
    assert_eq!(line("DROVER_THREAD="), "term-1");
    assert!(line("DROVER_PORT=").parse::<u16>().is_ok(), "{out}");
}

#[test]
fn said_logs_an_owner_line_once_and_answer_typed_takes_it() {
    let (c, b, r) = chat("attach-said");
    let on = b.on(&r);
    let s = Server::new(&c, &on, Some("127.0.0.1:7777".into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let attach = json!({"fleet": "sendmeter", "thread": COOEE}).to_string();
    assert_eq!(
        call(&s, &ck, ("POST", "/api/v1/orchestrator-attach"), &attach)["ok"],
        true
    );
    assert_eq!(ask(&c, &r, Some(COOEE)), (0, "q1\n".into()));
    let said = |auth: &str, thread: &str, item: &str| {
        let body = json!({"id": item, "ts": T, "from": "owner", "text": "take full"}).to_string();
        call(&s, auth, ("POST", &format!("/api/v1/thread/{thread}/said")), &body)
    };
    assert_eq!(said("", COOEE, "m1")["code"], "UNPAIRED");
    assert_eq!(said(&bearer(), COOEE, "m1")["ok"], true);
    assert_eq!(said(&bearer(), COOEE, "m1")["ok"], true);
    let lines: Vec<_> = read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "thread-msg" && e.str("item") == Some("m1"))
        .map(|e| {
            (
                e.str("thread").map(String::from),
                e.str("from").map(String::from),
                e.str("via").map(String::from),
            )
        })
        .collect();
    assert_eq!(
        lines,
        [(Some(COOEE.into()), Some("owner".into()), Some("client".into()))]
    );
    let rows = call(&s, &bearer(), ("GET", &format!("/api/v1/thread/{COOEE}/rows")), "");
    let row = rows["rows"]
        .as_array()
        .unwrap()
        .iter()
        .rfind(|r| r["kind"] == "said")
        .cloned()
        .unwrap();
    assert_eq!(
        (row["text"].clone(), row["prompt"].clone(), row["push"].clone()),
        (json!("take full"), Value::Null, Value::Null),
        "{row}"
    );
    let other = said(&bearer(), "not-attached", "m2");
    assert_eq!(
        (other["ok"].clone(), other["code"].clone()),
        (json!(false), json!("NO_SUCH_THREAD")),
        "{other}"
    );

    let (code, out) = run(&c, &r, &["answer", "sendmeter", "q1", "2", "--typed", "take full"]);
    assert_eq!(code, 0, "{out}");
    let events = read(&c.state).unwrap();
    let d = events.iter().rfind(|e| e.kind == "decision").unwrap();
    assert_eq!((d.str("choice"), d.str("by")), (Some("full"), Some("owner (typed)")));
}

#[test]
fn attach_adopts_an_archived_drover_thread_and_the_issues_it_filed_route_to_the_chat() {
    let (c, b, r) = chat("attach-adopt");
    let on = b.on(&r);
    assert_eq!(old_thread(&c, "sendmeter", "Old work"), "t1");
    let ev = |n: u64, kind: &str, d: Value| {
        drover::events::append(&c.state, &drover::events::Event::new(T, "sendmeter", Some(n), kind, d)).unwrap();
    };
    ev(12, "issue-filed", json!({"filed_in": "t1"}));
    let (code, out) = run(&c, &on, &["attach", "sendmeter", "t1"]);
    assert!(code == 3 && out.contains("drover thread archive t1"), "{out}");
    assert_eq!(run(&c, &on, &["thread", "archive", "t1"]).0, 0);
    let (code, out) = run(&c, &on, &["attach", "sendmeter", "t1"]);
    assert_eq!(code, 0, "{out}");
    ev(12, "merged", json!({}));
    let rows = call(&Server::new(&c, &on, None), &bearer(), ("GET", "/api/v1/thread/t1/rows"), "");
    let merged = rows["rows"].as_array().unwrap().iter().find(|r| r["kind"] == "merged").cloned();
    assert_eq!(
        merged.map(|m| (m["issue"].clone(), m["to"].clone())),
        Some((json!(12), json!({"k": "thread", "id": "t1"}))),
        "{rows}"
    );
}

#[test]
fn check_logs_one_closing_row_in_each_open_drover_thread() {
    let (c, b, r) = chat("attach-closing");
    let on = b.on(&r);
    for t in ["Open", "Archived", "Adopted"] {
        old_thread(&c, "sendmeter", t);
    }
    for t in ["t2", "t3"] {
        assert_eq!(run(&c, &on, &["thread", "archive", t]).0, 0);
    }
    assert_eq!(run(&c, &on, &["attach", "sendmeter", "t3"]).0, 0);
    for _ in 0..2 {
        assert_eq!(run(&c, &on, &["check"]).0, 0);
    }
    let closing: Vec<_> = read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "thread-msg" && e.val("closing") == true)
        .map(|e| (e.str("thread").map(String::from), e.str("text").map(String::from)))
        .collect();
    assert_eq!(closing.len(), 1, "{closing:?}");
    let (thread, text) = closing[0].clone();
    assert_eq!(thread.as_deref(), Some("t1"));
    assert!(text.unwrap().contains("drover attach sendmeter t1"));
}
