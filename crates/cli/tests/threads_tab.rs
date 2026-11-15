mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

fn req(s: &Server, method: &str, path: &str, body: &str, extra: &str) -> (u16, String) {
    let r = format!(
        "{method} {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&r, T)).into_owned();
    let (head, body) = res.split_once("\r\n\r\n").unwrap();
    (head[9..12].parse().unwrap(), body.to_string())
}

fn get(s: &Server, path: &str, extra: &str) -> Value {
    let (status, body) = req(s, "GET", path, "", extra);
    assert_eq!(status, 200, "{path}: {body}");
    serde_json::from_str(&body).unwrap()
}

fn golden(name: &str, got: &Value) {
    let f = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/golden/cooee/{name}.json"));
    if std::env::var_os("DROVER_BLESS").is_some() {
        std::fs::write(&f, serde_json::to_string_pretty(got).unwrap() + "\n").unwrap();
    }
    let want: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    assert_eq!(got, &want, "{name}: {}", serde_json::to_string_pretty(got).unwrap());
}

fn find<'a>(v: &'a Value, id: &str) -> &'a Value {
    fn go<'a>(v: &'a Value, id: &str) -> Option<&'a Value> {
        if v.get("id").and_then(Value::as_str) == Some(id) {
            return Some(v);
        }
        match v {
            Value::Object(m) => m.values().find_map(|x| go(x, id)),
            Value::Array(a) => a.iter().find_map(|x| go(x, id)),
            _ => None,
        }
    }
    go(v, id).unwrap_or_else(|| panic!("no block {id} in {v}"))
}

fn recorded(name: &str) -> drover::Ctx {
    let c = ctx(
        &tmpdir(name),
        &format!("{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\n"),
    );
    let profile = c.config.with_file_name("claude").join("fleet-orchestrator");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    let ev =
        |ts: u64, fleet: &str, kind: &str, d: Value| append(&c.state, &Event::new(ts, fleet, None, kind, d)).unwrap();
    let msg = |ts, from: &str, text: &str| {
        ev(
            ts,
            "sendmeter",
            "thread-msg",
            json!({"thread": "t1", "from": from, "text": text}),
        )
    };
    ev(
        T - 90_000,
        "sendmeter",
        "thread-new",
        json!({"thread": "t1", "title": "Search worker split", "harness": "claude"}),
    );
    ev(T - 89_000, "sendmeter", "thread-archive", json!({"thread": "t1"}));
    ev(
        T - 7_200,
        "sendmeter",
        "thread-new",
        json!({"thread": "t2", "title": "Rate limits for /v1/search", "harness": "claude"}),
    );
    let msg2 = |ts, from: &str, text: &str| {
        ev(
            ts,
            "sendmeter",
            "thread-msg",
            json!({"thread": "t2", "from": from, "text": text}),
        )
    };
    msg(T - 89_500, "owner", "split it");
    msg2(T - 7_100, "owner", "Grill me, then file it.");
    ev(
        T - 7_000,
        "sendmeter",
        "thread-cmd",
        json!({"thread": "t2", "cmd": "drover brief sendmeter 19"}),
    );
    msg2(T - 6_900, "orchestrator", "Two questions, one at a time.");
    let ask = |ts, id: &str, q: &str, opts: Value| {
        ev(
            ts,
            "sendmeter",
            "needs-decision",
            json!({"id": id, "thread": "t2", "question": q, "options": opts, "recommend": 1, "kind": "owner"}),
        )
    };
    ask(
        T - 6_800,
        "q31",
        "Limit per API key or per client IP?",
        json!(["per key", "per ip"]),
    );
    ev(
        T - 6_700,
        "sendmeter",
        "decision",
        json!({"id": "q31", "choice": "per key", "by": "owner"}),
    );
    ev(
        T - 500,
        "sendmeter",
        "thread-msg",
        json!({"thread": "t2", "from": "system", "text": "✗ not delivered: no pane", "line": "and the print sheet"}),
    );
    ask(
        T - 300,
        "q33",
        "Retry-After only, or the X-RateLimit headers too?",
        json!(["retry-after", "both"]),
    );
    ev(
        T - 3_000,
        "all",
        "thread-new",
        json!({"thread": "t3", "title": "Weekly lesson digest", "harness": "claude"}),
    );
    ev(
        T - 2_900,
        "all",
        "thread-msg",
        json!({"thread": "t3", "from": "orchestrator", "text": "Filed lessons."}),
    );
    c
}

#[test]
fn the_threads_screens_are_their_golden_json_and_the_tab_is_standalone_only() {
    let c = recorded("threads-get");
    let r = OnShed::new(Fake::default());
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let list = get(&s, "/screens/threads", "");
    golden("threads", &list);
    assert_eq!(list["badge"], 1);
    let ids: Vec<&str> = list["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["r-all", "r-sendmeter", "archived"]);
    assert_eq!(
        find(&list, "t2")["subtitle"]["text"],
        "! choose · conductor: Retry-After only, or the X-RateLimit headers too?"
    );
    assert_eq!(find(&list, "t2")["tap"], json!({"open": "/screens/threads/t2"}));
    assert_eq!(find(&list, "t1")["swipe"][0]["action"], "thread-restore");

    let t = get(&s, "/screens/threads/t2", "");
    golden("thread", &t);
    assert_eq!(
        (&t["focus"], &t["actions"][0]["action"]),
        (&json!(true), &json!("thread-archive"))
    );
    let open = find(&t, "q33");
    assert_eq!(
        (&open["type"], &open["recommended"]),
        (&json!("choice"), &json!("retry-after"))
    );
    let blocks = t["blocks"].as_array().unwrap();
    let kinds: Vec<&str> = blocks.iter().filter_map(|b| b["kind"].as_str()).collect();
    assert_eq!(kinds, ["you", "tool", "orch", "ask", "you", "ask"]);
    let bad = blocks.iter().find(|b| b["tone"] == "bad").unwrap();
    assert_eq!(bad["buttons"][0]["action"], "thread-retry", "{t}");
    assert!(t.to_string().contains("✓ **per key** · owner"), "{t}");
    assert_eq!(blocks.last().unwrap()["submit"]["action"], "thread-say");
    assert!(get(&s, "/screens/threads/t1", "")["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|b| b["type"] != "form"));

    golden("thread-new", &get(&s, "/screens/threads/new/sendmeter", ""));
    let (status, _) = req(&s, "GET", "/screens/threads/new/nope", "", "");
    assert_eq!(status, 404);

    let (_, page) = req(&s, "GET", "/", "", "");
    assert!(
        page.contains(r#""id":"threads""#),
        "the standalone page has the Threads tab"
    );
    let manifest = include_str!("../../../cooee-plugin.toml");
    assert!(
        !manifest.contains("threads") && !manifest.contains("thread-"),
        "never the plugin's"
    );
}

#[test]
fn the_owner_reads_a_thread_sends_and_answers_from_the_tab() {
    let c = recorded("threads-act");
    let r = OnShed::new(Fake::default());
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let post = |v: Value, extra: &str| req(&s, "POST", "/action", &v.to_string(), extra);
    let say = json!({"screen": "thread-t2", "block": "say", "action": "thread-say", "values": {"thread": "t2", "text": "Skip the print one."}});
    assert_eq!(post(say.clone(), "").0, 401);
    std::fs::write(c.config.with_file_name("cooee-token"), "tok").unwrap();
    let (status, v) = post(say.clone(), "Authorization: Bearer tok\r\n");
    assert_eq!(status, 403, "the plugin has no Threads: {v}");

    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let reads = || {
        read(&c.state)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "thread-read")
            .count()
    };
    assert_eq!(reads(), 0);
    get(&s, "/screens/threads/t2", &ck);
    get(&s, "/screens/threads/t2", &ck);
    assert_eq!(reads(), 1, "a paired open marks it read, once");

    let (status, v) = post(say, &ck);
    assert_eq!(status, 200, "{v}");
    let sent = read(&c.state).unwrap().to_vec();
    let sent = sent
        .iter()
        .find(|e| e.kind == "thread-msg" && e.str("text") == Some("Skip the print one."));
    assert_eq!(sent.and_then(|e| e.str("from")), Some("owner"));
    let t = get(&s, "/screens/threads/t2", &ck);
    let mine = t["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .rfind(|b| b["kind"] == "you" && b["tone"] != "bad");
    assert!(
        mine.is_some_and(|m| m.to_string().contains("Skip the print one.")),
        "{t}"
    );

    let answer = json!({"screen": "thread-t2", "block": "q33", "action": "answer", "values": {"thread": "t2", "choice": "both", "other": ""}});
    let (status, v) = post(answer, &ck);
    assert_eq!(status, 200, "{v}");
    let d = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .rfind(|e| e.kind == "decision")
        .unwrap();
    assert_eq!((d.str("id"), d.str("choice")), (Some("q33"), Some("both")));
    let t = get(&s, "/screens/threads/t2", &ck);
    assert!(
        t.to_string().contains("✓ **both**") && !t.to_string().contains(r#""type":"choice""#),
        "{t}"
    );

    let new = json!({"screen": "thread-new-sendmeter", "block": "say", "action": "thread-new", "values": {"repo": "sendmeter", "text": "Dark mode for settings"}});
    let (status, v) = post(new, &ck);
    assert_eq!(status, 200, "{v}");
    let v: Value = serde_json::from_str(&v).unwrap();
    assert_eq!(v["ops"][0]["screen"]["id"], "thread-t4", "{v}");
    assert_eq!(v["ops"][0]["screen"]["title"], "Dark mode for settings");

    let arch = json!({"screen": "threads", "block": "t3", "action": "thread-archive", "values": {"thread": "t3"}});
    assert_eq!(post(arch, &ck).0, 200);
    let list = get(&s, "/screens/threads", &ck);
    assert_eq!(find(&list, "t3")["swipe"][0]["action"], "thread-restore");
    assert!(find(&list, "archived").to_string().contains("\"t3\""), "{list}");
}
