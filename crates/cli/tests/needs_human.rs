mod common;
use common::*;
use drover::config::Config;
use drover::events::{append, fold, read, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const NOW: u64 = 1_000_000;
const ADDR: &str = "127.0.0.1:7777";
const HEAD: &str = "1111111111111111111111111111111111111111";

fn fixture(name: &str, last: &str) -> drover::Ctx {
    let mut c = ctx(&tmpdir(name), CONFIG);
    c.now = NOW;
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let ev = |ts, kind, d| append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
    ev(
        10,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"}),
    );
    for (i, sha) in ["2", "3", "4"].iter().enumerate() {
        ev(
            20 + i as u64,
            "verdict",
            json!({"verdict": "FAIL", "sha": sha.repeat(40)}),
        );
    }
    let body = format!("{last}\n\nblocking: src/x.rs:9 still wrong\n");
    ev(30, "verdict", json!({"verdict": last, "sha": HEAD, "body": body}));
    ev(
        31,
        "needs-human",
        json!({"code": "ROUND_LIMIT", "reason": "review rounds ran out (4/3)"}),
    );
    let checks = json!([{"__typename": "CheckRun", "name": "build", "status": "COMPLETED", "conclusion": "SUCCESS"}]);
    let pr = json!({"number": 34, "state": "OPEN", "baseRefName": "staging", "headRefOid": HEAD, "mergeable": "MERGEABLE", "statusCheckRollup": checks});
    let cache = c.state.parent().unwrap().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let file = json!({"ts": NOW, "branch": "issue-12", "pr": pr});
    std::fs::write(cache.join("pr-sendmeter-12.json"), file.to_string()).unwrap();
    c
}

fn runner() -> OnShed {
    OnShed::new(Fake::default())
}

fn get(s: &Server, path: &str, cookie: &str) -> String {
    let res = s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}\r\n"), NOW);
    String::from_utf8_lossy(&res).into_owned()
}

fn card(s: &Server, cookie: &str) -> Option<Value> {
    let res = get(s, "/screens/cards", cookie);
    let all: Value = serde_json::from_str(res.split_once("\r\n\r\n").unwrap().1).unwrap();
    all.as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "nhc-sendmeter-12")
        .cloned()
}

fn choice(k: &Value) -> Option<&Value> {
    k["blocks"].as_array().unwrap().iter().find(|b| b["type"] == "choice")
}

fn options(k: &Value) -> Vec<&str> {
    choice(k).map_or(vec![], |c| {
        c["options"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["value"].as_str().unwrap())
            .collect()
    })
}

fn action(s: &Server, cookie: &str, choice: &str) -> String {
    let body = json!({"screen": "cards", "block": "nh-sendmeter-12", "action": "needs-human",
        "values": {"fleet": "sendmeter", "issue": 12, "choice": choice}})
    .to_string();
    let req = format!(
        "POST /action HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&req, NOW)).into_owned()
}

#[test]
fn needs_human_pushes_with_the_default_on() {
    let cfg = Config::parse("[[notify]]\ntype = \"webpush\"\nsub = \"mailto:owner@example.com\"\n").unwrap();
    assert!(cfg.notify[0].wants("needs-human"));
}

#[test]
fn card_with_a_pass_at_head_offers_merge_anyway_to_a_paired_phone_only() {
    let c = fixture("nh-pass", "PASS");
    let r = runner();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "", NOW);
    let k = card(&s, &cookie).expect("no needs-human card");
    let text = k["blocks"][0]["markdown"].as_str().unwrap();
    assert!(text.contains("review rounds ran out (4/3)"), "{k}");
    assert!(text.contains("blocking: src/x.rs:9 still wrong"), "{k}");
    assert_eq!(options(&k), ["extend", "merge", "park"], "{k}");
    let merge = &choice(&k).unwrap()["options"][1];
    assert_eq!(merge["confirm"]["ok"], "Merge", "{k}");
    assert_eq!(merge["label"], "merge anyway");
    assert!(
        k["fold"]["line"]
            .as_str()
            .unwrap()
            .starts_with("#12 · needs-human · review rounds ran out (4/3)"),
        "#816: folded to one line: {k}"
    );
    assert_eq!(
        (&k["fold"]["main"]["value"], &k["fold"]["main"]["label"]),
        (&json!("extend"), &json!("one more round")),
        "#816: the line's main button is the card's first choice: {k}"
    );

    assert!(
        action(&s, "", "park").starts_with("HTTP/1.1 401"),
        "an unpaired phone cannot act"
    );
    let ro = Server::new(&c, &r, None);
    let res = action(&ro, &cookie, "park");
    assert!(
        res.starts_with("HTTP/1.1 40"),
        "a read-only server takes no action: {res}"
    );
    assert_eq!(
        fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)].state,
        "needs-human"
    );
}

#[test]
fn card_with_a_fail_has_no_merge_anyway() {
    let c = fixture("nh-fail", "FAIL");
    let r = runner();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "", NOW);
    let k = card(&s, &cookie).expect("no needs-human card");
    assert!(
        k["blocks"][0]["markdown"]
            .as_str()
            .unwrap()
            .contains("blocking: src/x.rs:9 still wrong"),
        "{k}"
    );
    assert_eq!(options(&k), ["extend", "park"], "{k}");
}

#[test]
fn herd_marks_the_issue_and_park_clears_the_card() {
    let c = fixture("nh-herd", "FAIL");
    let r = runner();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let res = get(&s, "/api/v1/herd", "");
    let j: serde_json::Value = serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    assert_eq!(j["fleets"][0]["needs"], json!([12]));
    assert_eq!(j["fleets"][0]["queue"][0]["issue"], 12, "no lane: a gate horse");

    let page = get(&s, "/api/v1/state", "");
    let i = page.find("\"mtime\":\"").unwrap() + 9;
    let mt: String = page[i..].chars().take_while(char::is_ascii_digit).collect();
    let body = format!("op=park&fleet=sendmeter&mtime={mt}&issue=12");
    let post = |extra: &str| {
        let req = format!(
            "POST / HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        String::from_utf8_lossy(&s.respond(&req, NOW)).into_owned()
    };
    assert!(post("").starts_with("HTTP/1.1 401"), "unpaired cannot park");
    let cookie = pair(&s, &c, "", NOW);
    assert!(post(&cookie).starts_with("HTTP/1.1 303"));
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
    assert_eq!(st.state, "parked");
    assert!(card(&s, &cookie).is_none(), "parked: no card");
}

#[test]
fn card_ops_need_needs_human_and_a_refused_extend_logs_nothing() {
    let c = fixture("nh-guard", "FAIL");
    let r = runner();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "", NOW);
    assert_eq!(run(&c, &r, &["pause", "sendmeter"]).0, 0);
    let page = get(&s, "/api/v1/state", &cookie);
    let i = page.find("\"mtime\":\"").unwrap() + 9;
    let mt: String = page[i..].chars().take_while(char::is_ascii_digit).collect();
    let post = |op: &str| {
        let body = format!("op={op}&fleet=sendmeter&mtime={mt}&issue=12");
        let req = format!(
            "POST / HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        String::from_utf8_lossy(&s.respond(&req, NOW)).into_owned()
    };
    let extended = || {
        read(&c.state)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "round-extended")
            .count()
    };

    let res = post("extend");
    assert!(
        res.contains("msg=%2312%3A%20extend"),
        "PAUSED only holds the start: {res}"
    );
    assert_eq!(drover::pending::of(&read(&c.state).unwrap(), "sendmeter", 12), None);
    assert_eq!(card(&s, &cookie), None, "the card leaves at the tap");
    assert!(post("extend").contains("not%20needs"), "a second tap decides nothing");
    assert_eq!(extended(), 1, "the decision is logged once");
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
    assert_eq!((st.extra, st.state.as_str()), (1, "queued"));
    let failed = read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "extend-failed")
        .count();
    assert_eq!(failed, 0);

    append(&c.state, &Event::new(40, "sendmeter", Some(12), "merged", json!({}))).unwrap();
    for op in ["park", "extend", "override"] {
        let res = post(op);
        assert!(res.contains("not%20needs"), "{op}: {res}");
    }
    assert_eq!(
        fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)].state,
        "merged"
    );
}

#[test]
fn a_recorded_extension_is_a_pending_line_without_buttons() {
    let c = fixture("nh-pending", "FAIL");
    append(
        &c.state,
        &Event::new(
            40,
            "sendmeter",
            Some(12),
            "round-extended",
            json!({"by": "owner", "spawn": true}),
        ),
    )
    .unwrap();
    let r = runner();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "", NOW);
    let k = card(&s, &cookie).expect("the card stays, pending");
    assert!(
        choice(&k).is_none() && k["fold"]["main"].is_null(),
        "no choice while one more round starts: {k}"
    );

    let spawn = json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"});
    append(&c.state, &Event::new(41, "sendmeter", Some(12), "spawn", spawn)).unwrap();
    append(
        &c.state,
        &Event::new(42, "sendmeter", Some(12), "round-extended", json!({"by": "owner"})),
    )
    .unwrap();
    assert!(
        card(&s, &cookie).is_none_or(|k| choice(&k).is_some()),
        "an old-order extension is not pending"
    );
}
