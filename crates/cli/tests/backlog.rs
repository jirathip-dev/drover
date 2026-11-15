mod common;
use common::*;
use drover::events::read;
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const NOW: u64 = 10_000_000;
const READ: &str = "gh api graphql --paginate --jq .data.repository.issues";

fn issue(n: u64, state: &str, updated: &str, labels: &[&str], ac: bool, pr: bool) -> String {
    let i = json!({"number": n, "title": format!("issue {n} title"), "state": state, "created": updated,
        "updated": updated, "labels": labels, "ac": ac, "pr": pr});
    format!("{i}\n")
}

fn first_read() -> String {
    [
        issue(12, "OPEN", "1970-01-02T00:00:00Z", &["bug"], false, true),
        issue(15, "OPEN", "1970-04-20T00:00:00Z", &["ui", "phone"], true, false),
        issue(17, "OPEN", "1970-04-25T00:00:00Z", &[], true, false),
        issue(20, "OPEN", "1970-04-21T00:00:00Z", &[], false, false),
        issue(21, "OPEN", "1970-04-21T00:00:00Z", &[], false, false),
    ]
    .concat()
}

fn queue(s: &Server, cookie: &str) -> Value {
    serde_json::from_str(&api(s, ADDR, cookie, NOW, "queue")).unwrap()
}

fn post(s: &Server, cookie: &str, body: &str) -> String {
    let req = format!(
        "POST / HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&req, NOW)).into_owned()
}

fn mtime(q: &Value) -> String {
    q["mtime"].as_str().unwrap().to_string()
}

fn cache(c: &drover::Ctx) -> Value {
    let p = c.state.with_file_name("cache").join("backlog-sendmeter-0.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn keys(v: &Value) -> Vec<String> {
    v.as_object().unwrap().keys().cloned().collect()
}

#[test]
fn the_tick_reads_all_open_issues_then_only_those_updated_since() {
    let c = ctx(&tmpdir("backlog-tick"), CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
    let r = Fake::default().once(READ, 0, &first_read());
    run(&c, &r, &["drive", "--fast"]);
    assert_eq!(r.count(READ), 0, "a fast step reads no backlog");
    assert_eq!(run(&c, &r, &["drive"]).0, 0);
    let first: Vec<_> = r
        .calls
        .borrow()
        .iter()
        .filter(|l| l.starts_with(READ))
        .cloned()
        .collect();
    assert_eq!(first.len(), 1, "{:?}", r.calls.borrow());
    assert!(
        first[0].contains("issues(states:OPEN,") && !first[0].contains("since="),
        "{}",
        first[0]
    );
    let b = cache(&c);
    assert_eq!(b["since"], "1970-04-25T00:00:00Z");
    assert_eq!(keys(&b["issues"]), ["12", "15", "17", "20", "21"]);
    assert_eq!(b["issues"]["15"]["labels"], json!(["ui", "phone"]));

    let next = [
        issue(12, "CLOSED", "1970-04-26T00:00:00Z", &["bug"], false, false),
        issue(15, "OPEN", "1970-04-26T01:00:00Z", &["ui"], false, false),
        issue(30, "OPEN", "1970-04-26T02:00:00Z", &[], false, false),
    ];
    let r = Fake::default().once(READ, 0, &next.concat());
    assert_eq!(run(&c, &r, &["drive"]).0, 0);
    let call = r.calls.borrow().iter().find(|l| l.starts_with(READ)).cloned().unwrap();
    assert!(
        call.contains("issues(states:[OPEN,CLOSED],") && call.ends_with("-f since=1970-04-25T00:00:00Z"),
        "{call}"
    );
    let b = cache(&c);
    assert_eq!(keys(&b["issues"]), ["15", "17", "20", "21", "30"]);
    assert_eq!(
        (b["issues"]["15"]["labels"].clone(), b["issues"]["15"]["ac"].clone()),
        (json!(["ui"]), json!(false))
    );
    assert_eq!(b["since"], "1970-04-26T02:00:00Z");

    let r = Fake::default().once_err(READ, 1, "HTTP 502");
    assert_eq!(run(&c, &r, &["drive"]).0, 0, "a failed read never fails the pass");
    assert_eq!(r.count(READ), 1);
    assert_eq!(cache(&c)["issues"], b["issues"], "the last read kept");
}

#[test]
fn the_reads_jq_flattens_each_issue() {
    if std::process::Command::new("jq").arg("--version").output().is_err() {
        return eprintln!("skipping: no jq here");
    }
    let c = ctx(&tmpdir("backlog-jq"), CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
    let r = Fake::default();
    run(&c, &r, &["drive"]);
    let call = r.calls.borrow().iter().find(|l| l.starts_with(READ)).cloned().unwrap();
    let jq = &call[call.find("--jq ").unwrap() + 5..call.find(" -f owner=").unwrap()];
    let node = |n: u64, body: &str, labels: &[&str], refs: Value| {
        let labels: Vec<_> = labels.iter().map(|l| json!({"name": l})).collect();
        json!({"number": n, "title": format!("t{n}"), "state": "OPEN", "body": body, "createdAt": "c", "updatedAt": "u",
            "labels": {"nodes": labels}, "timelineItems": {"nodes": refs}})
    };
    let page = json!({"data": {"repository": {"issues": {"nodes": [
        node(1, "Intro\n## AC\n- works", &["bug", "ui"], json!([{"source": {"state": "OPEN"}}, {"source": {"state": "MERGED"}}])),
        node(2, "Has Acceptance Criteria below", &[], json!([{"source": {"state": "CLOSED"}}])),
        node(3, "an ACME thing, nothing more", &[], json!([{"source": {}}, {}, {"source": {"state": "OPEN"}}])),
    ]}}}});
    let d = tmpdir("backlog-jq-page");
    std::fs::write(d.join("page.json"), page.to_string()).unwrap();
    let o = std::process::Command::new("jq")
        .args(["-c", jq])
        .arg(d.join("page.json"))
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let got: Vec<Value> = String::from_utf8_lossy(&o.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        got,
        [
            json!({"number": 1, "title": "t1", "state": "OPEN", "created": "c", "updated": "u", "labels": ["bug", "ui"], "ac": true, "pr": true}),
            json!({"number": 2, "title": "t2", "state": "OPEN", "created": "c", "updated": "u", "labels": [], "ac": true, "pr": true}),
            json!({"number": 3, "title": "t3", "state": "OPEN", "created": "c", "updated": "u", "labels": [], "ac": false, "pr": false}),
        ]
    );
}

#[test]
fn the_queue_api_carries_the_backlog_from_the_last_read_with_no_github_call() {
    let c = ctx(&tmpdir("backlog-board"), CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "20"]).0, 0);
    let ev = drover::events::Event::new(
        1,
        "sendmeter",
        Some(21),
        "spawn",
        json!({"agent": "sendmeter-impl-21", "role": "impl"}),
    );
    drover::events::append(&c.state, &ev).unwrap();
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "", NOW);
    assert_eq!(
        queue(&s, &cookie)["fleets"][0]["backlog"],
        Value::Null,
        "never read: no backlog section"
    );
    assert_eq!(run(&c, &Fake::default().once(READ, 0, &first_read()), &["drive"]).0, 0);
    for q in [queue(&s, &cookie), queue(&s, "")] {
        let bl = &q["fleets"][0]["backlog"]["issues"];
        assert_eq!(keys(bl), ["12", "15", "17"], "#20 queued and #21 with a lane left out");
        assert_eq!(bl["12"], cache(&c)["issues"]["12"]);
    }
    assert_eq!(r.count("gh"), 0, "no GitHub call per view: {:?}", r.calls.borrow());
}

#[test]
fn queue_n_follows_tick_order_and_marks_needs_check() {
    let c = ctx(&tmpdir("backlog-queue"), CONFIG);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "20"]).0, 0);
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let mt = mtime(&queue(&s, ""));
    let body = format!("op=bqueue&fleet=sendmeter&mtime={mt}&issues=17%2C12%2C15");
    assert!(post(&s, "", &body).starts_with("HTTP/1.1 401"), "unpaired cannot queue");
    let cookie = pair(&s, &c, "", NOW);
    let res = post(&s, &cookie, &body);
    assert!(
        res.contains("%2320%20%2317%20%2312%20%2315%20%28needs%2Dcheck%29"),
        "{res}"
    );
    let (_, out) = run(&c, &r, &["queue", "sendmeter", "ls", "--json"]);
    let ls: Value = serde_json::from_str(&out).unwrap();
    let order: Vec<u64> = ls
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["issue"].as_u64().unwrap())
        .collect();
    assert_eq!(order, [20, 17, 12, 15]);
    assert_eq!(run(&c, &r, &["queue", "sendmeter", "mv", "--top", "15"]).0, 0);
    let q = queue(&s, &cookie);
    let row = |n: u64| {
        let rows = q["fleets"][0]["rows"].as_array().unwrap();
        rows.iter()
            .find(|r| r["issue"] == n)
            .cloned()
            .unwrap_or_else(|| panic!("no #{n}: {q}"))
    };
    for n in [17, 12, 15] {
        assert_eq!(row(n)["check"], true, "#{n} needs-check: from the backlog");
    }
    assert_eq!(row(20)["check"], false, "queued by hand: no mark");
    let qs: Vec<_> = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == "queue")
        .collect();
    assert_eq!(
        (qs[1].val("check").clone(), qs[1].val("via").clone()),
        (json!(true), json!("web"))
    );
}
