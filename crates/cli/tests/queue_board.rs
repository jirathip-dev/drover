mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::serve::Server;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

fn post(s: &Server, cookie: &str, fields: &BTreeMap<String, String>) -> Value {
    let body = fields
        .iter()
        .filter(|(k, _)| *k != "op")
        .map(|(k, v)| format!("{k}={}", v.replace(' ', "+")))
        .collect::<Vec<_>>()
        .join("&");
    let req = format!(
        "POST /api/v1/{} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        fields["op"],
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&req, T)).into_owned();
    serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap_or_else(|_| panic!("{res}"))
}

fn fields(s: &Server, op: &str, kv: &[(&str, &str)]) -> BTreeMap<String, String> {
    let st: Value = serde_json::from_str(&api(s, ADDR, "", T, "state")).unwrap();
    let mtime = st["mtime"].as_str().unwrap().to_string();
    [("op", op), ("fleet", "sendmeter"), ("mtime", mtime.as_str())]
        .iter()
        .chain(kv)
        .map(|(k, v)| ((*k).into(), (*v).into()))
        .collect()
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

#[test]
fn every_queue_write_posts_through_api_v1() {
    let c = ctx(
        &tmpdir("queue-board"),
        &format!("models = [\"opus\", \"sonnet\"]\n\n{CONFIG}").replace(
            "profile = \"fleet-rev\"\n",
            "profile = \"fleet-rev\"\nmodel = \"sonnet\"\n",
        ),
    );
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "17", "18"]).0,
        0
    );
    let ev =
        |n: u64, kind: &str, d: Value| append(&c.state, &Event::new(T - 60, "sendmeter", Some(n), kind, d)).unwrap();
    ev(
        12,
        "needs-decision",
        json!({"id": "q1", "question": "split?", "options": ["yes", "no"], "kind": "routine", "recommend": 2}),
    );
    ev(12, "decision", json!({"id": "q1", "choice": "2", "by": "default"}));
    let issue = |n: u64| {
        json!({"number": n, "title": format!("t{n}"), "state": "OPEN", "created": "2026-01-01T00:00:00Z",
            "updated": "2026-01-02T00:00:00Z", "labels": [], "ac": true, "pr": false})
    };
    let cache = drover::gh::cache_path(&c, "backlog", "sendmeter", 0);
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(
        &cache,
        json!({"ts": T - 60, "issues": {"30": issue(30), "31": issue(31)}}).to_string(),
    )
    .unwrap();
    let r = OnShed::new(Fake::default().reply("which hermes", "/usr/local/bin/hermes\n"));
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let queue = || api(&s, ADDR, "", T, "queue");
    let ok = |f: &BTreeMap<String, String>| {
        let v = post(&s, &cookie, f);
        assert_eq!(v["ok"], true, "{f:?}: {v}");
        v["message"].as_str().unwrap().to_string()
    };

    ok(&fields(&s, "mv", &[("issue", "12"), ("after", "17")]));
    let order: Vec<u64> = drover::queue::view(
        &read(&c.state).unwrap(),
        &drover::events::fold(&read(&c.state).unwrap()),
        "sendmeter",
    )
    .iter()
    .map(|i| i.issue)
    .collect();
    assert_eq!(order, [17, 12, 18]);

    assert!(queue().contains("\"q1\""), "{}", queue());
    ok(&fields(&s, "answer", &[("id", "q1"), ("choice", "1")]));
    let d = kinds(&c, "decision");
    assert_eq!(
        (d.len(), d[1].str("choice"), d[1].str("by")),
        (2, Some("yes"), Some("owner")),
        "{d:?}"
    );

    assert!(ok(&fields(&s, "bqueue", &[("issues", "31,30")])).ends_with("(needs-check)"));
    let q = kinds(&c, "queue");
    assert_eq!(
        (
            q.last().unwrap().val("issues").clone(),
            q.last().unwrap().val("check").clone()
        ),
        (json!([31, 30]), json!(true))
    );

    let before = kinds(&c, "drive-pass").len();
    ok(&fields(&s, "drive", &[]));
    assert_eq!(kinds(&c, "drive-pass").len(), before + 1);

    let paused = || std::fs::read_to_string(&c.config).unwrap().contains("paused = true");
    ok(&fields(&s, "pause", &[]));
    assert!(paused());
    ok(&fields(&s, "resume", &[]));
    assert!(!paused());

    let set = fields(
        &s,
        "set",
        &[
            ("role", "impl"),
            ("harness", "hermes"),
            ("model", "sonnet"),
            ("effort", ""),
        ],
    );
    assert!(ok(&set).contains("model sonnet"));
    let v = post(&s, "", &fields(&s, "drive", &[]));
    assert_eq!(v["code"], "UNPAIRED", "{v}");
    let mut stale = fields(&s, "pause", &[]);
    stale.insert("mtime".into(), "1".into());
    assert_eq!(post(&s, &cookie, &stale)["code"], "STALE");
}

#[test]
fn a_thread_ask_on_an_issue_is_answerable_from_its_queue_card() {
    let sh = FakeShed::new();
    let cfg = format!(
        "{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\naccount = \"C\"\n\n\
         [fleet.sendmeter.runtime]\nkind = \"shed-acp\"\nsocket = \"{}\"\n\n[accounts]\nC = \"C\"\n",
        sh.sock.display()
    );
    let mut c = ctx(&tmpdir("queue-thread-ask"), &cfg);
    c.exe = env!("CARGO_BIN_EXE_drover").into();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "2"]).0, 0);
    let ev = |n: Option<u64>, kind: &str, d: Value| {
        append(&c.state, &Event::new(T - 60, "sendmeter", n, kind, d)).unwrap();
    };
    ev(None, "thread-new", json!({"thread": "t6", "title": "the bell"}));
    let ask = json!({"id": "q398", "question": "ship the bell now?", "options": ["now", "after #10"],
        "recommend": 1, "kind": "owner", "thread": "t6"});
    ev(Some(2), "needs-decision", ask);
    let agents =
        json!([{"id": "claude-acp", "version": "0.101.0", "distribution": "npx", "cmd": ["echo", "claude-agent-acp"]}]);
    let host = Fake::default()
        .reply("shed help run", "--meta <JSON>")
        .reply("shed agents --json", &agents.to_string())
        .reply("shed", "");
    let cli = sh.cli(&host);
    let r = drover::shed::With(&cli, sh.shed());
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let q = api(&s, ADDR, "", T, "queue");
    assert!(q.contains("ship the bell now?") && q.contains("\"t6\""), "{q}");
    let needs = api(&s, ADDR, &cookie, T, "needs");
    assert!(
        needs.contains("ship the bell now?") && needs.contains("the bell"),
        "{needs}"
    );
    let f = fields(&s, "answer", &[("id", "q398"), ("choice", "2")]);
    let v = post(&s, &cookie, &f);
    assert_eq!(v["ok"], true, "{v}");
    let d = kinds(&c, "decision");
    assert_eq!(
        (d.len(), d[0].str("choice"), d[0].str("by")),
        (1, Some("after #10"), Some("owner"))
    );
    assert!(
        sh.has("orchestrator-t6") && host.called("shed send orchestrator-t6"),
        "{:?}",
        host.calls.borrow()
    );
    let chat: Value = serde_json::from_str(&api(&s, ADDR, &cookie, T, "chat")).unwrap();
    let t = (chat["threads"].as_array().unwrap().iter())
        .find(|t| t["id"] == "t6")
        .unwrap_or_else(|| panic!("{chat}"));
    let m = (t["page"]["msgs"].as_array().unwrap().iter())
        .find(|m| m["ask"]["id"] == "q398")
        .unwrap_or_else(|| panic!("{t}"));
    assert_eq!(
        (&m["open"], &m["answer"]["choice"]),
        (&json!(false), &json!("after #10")),
        "{m}"
    );
    assert!(!api(&s, ADDR, "", T, "queue").contains("ship the bell now?"));
}
