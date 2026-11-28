mod common;
use common::*;
use drover::serve::Server;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::Path;

const T: u64 = 1_000_000;
const TOKEN: &str = "tok-contract-v1";

const LOG: &[&str] = &[
    r#"{"seq":1,"ts":999000,"fleet":"sendmeter","type":"thread-new","thread":"t1","title":"gate flakes"}"#,
    r#"{"seq":2,"ts":999001,"fleet":"sendmeter","type":"thread-new","thread":"t2","title":"facets"}"#,
    r#"{"seq":3,"ts":999002,"fleet":"sendmeter","type":"queue","op":"add","issues":[12],"thread":"t1","orchestrator":"default"}"#,
    r#"{"seq":4,"ts":999003,"fleet":"sendmeter","issue":12,"type":"needs-decision","id":"q1","question":"retry the flaky gate?","options":["retry","mark"],"recommend":1,"kind":"owner","agent":"sendmeter-impl-12","lane_ask":true}"#,
    r#"{"seq":5,"ts":999004,"fleet":"sendmeter","type":"thread-msg","thread":"t1","from":"system","text":"lane ask q1 on #12","lane_ask":"q1"}"#,
    r#"{"seq":6,"ts":999005,"fleet":"sendmeter","issue":30,"type":"needs-decision","id":"q2","question":"split the PR?","options":["split","keep"],"recommend":null,"kind":"owner","agent":"sendmeter-impl-30"}"#,
    r#"{"seq":7,"ts":999006,"fleet":"sendmeter","issue":12,"type":"decision","id":"q1","ask_seq":4,"choice":"retry","by":"owner"}"#,
    r#"{"seq":8,"ts":999007,"fleet":"sendmeter","type":"thread-handoff","thread":"t1","from":"t1","to":"t2","title":"Facets","text":"add facets","target":"facets · t2"}"#,
    r#"{"seq":9,"ts":999008,"fleet":"sendmeter","type":"run-card","thread":"t1","id":"r1","cmd":"just gen","why":"regenerate","blast":"the branch","dir":"~/p","host":"local","sha":"abc"}"#,
    r#"{"seq":10,"ts":999009,"fleet":"sendmeter","type":"run-settled","thread":"t1","id":"r1","approved":true,"by":"owner"}"#,
    r#"{"seq":11,"ts":999010,"fleet":"sendmeter","type":"config-propose","thread":"t1","id":"c1","diff":"+x = 1","base":"b","sha":"s"}"#,
    r#"{"seq":12,"ts":999011,"fleet":"sendmeter","type":"secret-request","thread":"t1","name":"API_KEY","why":"the deploy"}"#,
    r#"{"seq":13,"ts":999012,"fleet":"sendmeter","type":"thread-msg","thread":"t1","from":"owner","text":"go ahead"}"#,
    r#"{"seq":14,"ts":999013,"fleet":"sendmeter","issue":12,"type":"merged","pr":40}"#,
    r#"{"seq":15,"ts":999014,"fleet":"sendmeter","issue":12,"type":"rollout-open","why":"apply the config"}"#,
    r#"{"seq":16,"ts":999015,"fleet":"sendmeter","issue":30,"type":"stall","agent":"sendmeter-impl-30","mins":40}"#,
    r#"{"seq":17,"ts":999016,"fleet":"drover","type":"host-busy","why":"load 9"}"#,
];

fn board(name: &str) -> drover::Ctx {
    let c = ctx(&tmpdir(name), CONFIG);
    rewrite_log(&c.state, &(LOG.join("\n") + "\n"));
    std::fs::write(c.config.with_file_name("cooee-token"), TOKEN).unwrap();
    c
}

fn get(s: &Server, path: &str, auth: &str) -> (String, Value) {
    let r = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:4810\r\n{auth}\r\n");
    let res = String::from_utf8_lossy(&s.respond(&r, T)).into_owned();
    let (head, body) = res.split_once("\r\n\r\n").unwrap();
    (
        head[9..12].to_string(),
        serde_json::from_str(body).unwrap_or(Value::Null),
    )
}

fn bearer() -> String {
    format!("Authorization: Bearer {TOKEN}\r\n")
}

fn ty(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn walk(at: &str, v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(o) if at.rsplit('.').next() != Some("block") => {
            for (k, v) in o {
                let p = if at.is_empty() { k.clone() } else { format!("{at}.{k}") };
                match (k.as_str(), v) {
                    ("k" | "kind", Value::String(s)) => drop(out.insert(format!("{p} = {s:?}"))),
                    _ => walk(&p, v, out),
                }
            }
        }
        Value::Array(a) if !a.is_empty() => a.iter().for_each(|v| walk(&format!("{at}[]"), v, out)),
        v => drop(out.insert(format!("{at}: {}", ty(v)))),
    }
}

fn keys(v: &Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    walk("", v, &mut out);
    out
}

fn compare(want: &BTreeSet<String>, got: &BTreeSet<String>) -> (Vec<String>, Vec<String>) {
    (
        want.difference(got).cloned().collect(),
        got.difference(want).cloned().collect(),
    )
}

fn check(name: &str, sample: &Value, fails: &mut Vec<String>) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/contract/v1");
    let file = dir.join(format!("{name}.keys"));
    let want: BTreeSet<String> = std::fs::read_to_string(&file)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect();
    let got = keys(sample);
    let (gone, new) = compare(&want, &got);
    if std::env::var_os("BLESS").is_some() && !new.is_empty() {
        let all: BTreeSet<_> = want.union(&got).cloned().collect();
        std::fs::write(&file, all.into_iter().map(|l| l + "\n").collect::<String>()).unwrap();
        std::fs::write(
            dir.join(format!("{name}.json")),
            serde_json::to_string_pretty(sample).unwrap() + "\n",
        )
        .unwrap();
    } else {
        fails.extend(new.iter().map(|l| format!("{name}: new `{l}` (BLESS=1 adds it)")));
    }
    fails.extend(
        gone.iter()
            .map(|l| format!("{name}: `{l}` removed or retyped: that takes /api/v2")),
    );
}

#[test]
fn the_rows_routes_keep_contract_v1() {
    let c = board("contract-v1");
    let r = Fake::default();
    let s = Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    assert_eq!(get(&s, "/api/v1/rows?since=0", "").0, "401");
    let (code, all) = get(&s, "/api/v1/rows?since=0", &bearer());
    assert_eq!(code, "200", "{all}");
    let (_, open) = get(&s, "/api/v1/rows?open=1", &bearer());
    let (_, t1) = get(&s, "/api/v1/thread/t1/rows?n=50", &bearer());
    let mut fails = vec![];
    check("rows", &all, &mut fails);
    check("rows-open", &open, &mut fails);
    check("thread-rows", &t1, &mut fails);
    let mut kinds = std::collections::BTreeMap::<String, Vec<Value>>::new();
    for row in all["rows"].as_array().unwrap() {
        kinds
            .entry(row["kind"].as_str().unwrap().into())
            .or_default()
            .push(row.clone());
    }
    for (kind, rows) in kinds {
        check(&format!("row-{kind}"), &json!(rows), &mut fails);
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

#[test]
fn a_row_goes_to_the_thread_that_queued_its_issue_else_the_repos_desk() {
    let c = board("contract-home");
    let r = Fake::default();
    let s = Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    let (_, all) = get(&s, "/api/v1/rows?since=0", &bearer());
    let rows = all["rows"].as_array().unwrap();
    let to = |key: &str| rows.iter().find(|r| r["key"] == key).unwrap()["to"].clone();
    assert_eq!(to("ask-sendmeter-q1"), json!({"k": "thread", "id": "t1"}));
    assert_eq!(to("ask-sendmeter-q2"), json!({"k": "desk", "repo": "sendmeter"}));
    assert_eq!(to("merged-14"), json!({"k": "thread", "id": "t1"}));
    assert_eq!(to("alert-16"), json!({"k": "desk", "repo": "sendmeter"}));
    assert_eq!(to("alert-17"), json!({"k": "owner"}));
    let settled = rows.iter().rfind(|r| r["key"] == "ask-sendmeter-q1").unwrap();
    assert_eq!(
        (settled["kind"].clone(), settled["open"].clone()),
        (json!("decision"), json!(false))
    );
    let (_, open) = get(&s, "/api/v1/rows?open=1", &bearer());
    let open: Vec<&str> = open["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["key"].as_str().unwrap())
        .collect();
    assert_eq!(
        open,
        [
            "ask-sendmeter-q2",
            "config-c1",
            "secret-t1-API_KEY",
            "rollout-sendmeter-12"
        ]
    );
}

#[test]
fn the_contract_test_fails_on_a_removed_or_retyped_key_and_not_on_an_added_one() {
    let want = keys(&json!({"rows": [{"seq": 1, "to": {"k": "desk", "repo": "a"}}]}));
    let retyped = keys(&json!({"rows": [{"seq": "1", "to": {"k": "desk", "repo": "a"}}]}));
    let removed = keys(&json!({"rows": [{"to": {"k": "desk", "repo": "a"}}]}));
    let added = keys(&json!({"rows": [{"seq": 1, "new": true, "to": {"k": "desk", "repo": "a"}}]}));
    assert_eq!(compare(&want, &retyped).0, ["rows[].seq: number"]);
    assert_eq!(compare(&want, &removed).0, ["rows[].seq: number"]);
    assert_eq!(
        compare(&want, &added),
        (vec![], vec!["rows[].new: boolean".to_string()])
    );
}

#[test]
fn the_stream_resumes_after_last_event_id() {
    let c: &'static drover::Ctx = Box::leak(Box::new(board("contract-stream")));
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let r: &'static Fake = Box::leak(Box::default());
    std::thread::spawn(move || drover::serve::serve_on(c, r, l, false));
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    let req = format!(
        "GET /api/v1/rows/stream HTTP/1.1\r\nHost: 127.0.0.1:4810\r\n{}Last-Event-ID: 15\r\n\r\n",
        bearer()
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut got = String::new();
    let mut buf = [0; 4096];
    while !got.contains("id: 17\n") {
        let n = s.read(&mut buf).unwrap();
        assert!(n > 0, "{got}");
        got += &String::from_utf8_lossy(&buf[..n]);
    }
    let ids: Vec<&str> = got.lines().filter_map(|l| l.strip_prefix("id: ")).collect();
    assert_eq!(ids, ["16", "17"], "{got}");
    let data = got.lines().find_map(|l| l.strip_prefix("data: ")).unwrap();
    let row: Value = serde_json::from_str(data).unwrap();
    assert_eq!((row["seq"].clone(), row["kind"].clone()), (json!(16), json!("alert")));
}

#[test]
fn attach_and_said_keep_contract_v1() {
    let c = board("contract-said");
    let r = Fake::default().reply("gh", "");
    let s = Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let post = |path: &str, auth: &str, body: Value| {
        let body = body.to_string();
        let r = format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n{auth}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let res = String::from_utf8_lossy(&s.respond(&r, T)).into_owned();
        serde_json::from_str::<Value>(res.split_once("\r\n\r\n").unwrap().1).unwrap()
    };
    let attach = post(
        "/api/v1/orchestrator-attach",
        &ck,
        json!({"fleet": "sendmeter", "thread": "chat-1"}),
    );
    assert_eq!(attach["ok"], true, "{attach}");
    let said = json!({"id": "m1", "ts": T, "from": "owner", "text": "go"});
    let said = post("/api/v1/thread/chat-1/said", &bearer(), said);
    assert_eq!(said["ok"], true, "{said}");
    let mut fails = vec![];
    check("orchestrator-attach", &attach, &mut fails);
    check("said", &said, &mut fails);
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

#[test]
fn the_manifest_keeps_contract_v1_and_names_every_chat_path() {
    let c = board("contract-manifest");
    let r = Fake::default();
    let s = Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    assert_eq!(get(&s, "/api/v1/manifest", "").0, "401");
    let (code, m) = get(&s, "/api/v1/manifest", &bearer());
    assert_eq!(code, "200", "{m}");
    assert_eq!(m["api"], "http://127.0.0.1:4810");
    assert_eq!(m["threads"]["profiles"], "/cooee/profiles");
    assert_eq!(m["threads"]["started"], "/cooee/thread-started");
    assert_eq!(m["threads"]["said"], "/api/v1/thread/<id>/said");
    let screens: Vec<&str> = m["screens"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["screen"].as_str())
        .collect();
    assert_eq!(
        screens,
        ["/screens/queue", "/screens/events", "/screens/lanes", "/screens/fleets"]
    );
    let actions = m["actions"].as_array().unwrap();
    assert!(m["destructive"].as_array().unwrap().iter().all(|d| actions.contains(d)));
    let mut fails = vec![];
    check("manifest", &m, &mut fails);
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

#[test]
fn the_new_issue_card_keeps_contract_v1_and_its_actions_are_in_the_manifest() {
    let c = board("contract-new-issue");
    for (kind, req) in [
        ("scope-request", "new-1"),
        ("scope-started", "new-1"),
        ("scope-request", "new-2"),
    ] {
        let d = json!({"req": req, "text": "a line", "files": []});
        drover::events::append(&c.state, &drover::events::Event::new(T, "sendmeter", None, kind, d)).unwrap();
    }
    let r = Fake::default();
    let s = Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    let (code, cards) = get(&s, "/screens/cards", &bearer());
    assert_eq!(code, "200", "{cards}");
    let card = cards.as_array().unwrap().last().unwrap();
    assert_eq!(card["id"], "new-issue", "{cards}");
    let (_, m) = get(&s, "/api/v1/manifest", &bearer());
    for a in ["new-issue", "scope-drop"] {
        assert!(m["actions"].as_array().unwrap().contains(&json!(a)), "{a}: {m}");
    }
    assert!(
        m["destructive"].as_array().unwrap().contains(&json!("scope-drop")),
        "{m}"
    );
    let mut fails = vec![];
    check("cards-new-issue", card, &mut fails);
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}
