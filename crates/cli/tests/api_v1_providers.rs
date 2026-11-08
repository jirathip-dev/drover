//! #682: API v1's agent providers: `GET providers` (paired) lists each account and API key with its state and usage,
//! never a secret's value; `key-add` puts a key in the store and nothing in the log; `use` is a config proposal card,
//! never a direct fleet.toml write; `sign-in`'s token goes from the setup pane straight into the store.
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use herdsman::runner::Runner;
use herdsman::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;
const TOKEN: &str = "sk-ant-oat01-FAKETOKENaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const KEY: &str = "sk-fakekey0123456789abcdef";
const BALANCE: &str = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n{\"balance_infos\": [{\"currency\": \"USD\", \"total_balance\": \"12.50\"}]}";

/// sendmeter's impl on account A, its rev on accounts C then A; B capped, C signed out; A's 5h/7d status-line row.
fn board(name: &str) -> (herdsman::Ctx, Fake) {
    let cfg = CONFIG
        .replace(
            "harness  =  \"hermes\"    # toggled in tests",
            "harness = \"claude\"\naccount = \"A\"",
        )
        .replace(
            "harness = \"hermes\"\nprofile = \"fleet-rev\"",
            "harness = \"claude\"\nprofile = \"fleet-rev\"\naccounts = [\"C\", \"A\"]",
        )
        + "\n[accounts]\nA = \"TOK_A\"\nB = \"TOK_B\"\nC = \"TOK_C\"\n\n[providers.deepseek]\nkey_env = \"DEEPSEEK_API_KEY\"\n";
    let c = ctx(&tmpdir(name), &cfg);
    let ev = |kind: &str, d: Value| append(&c.state, &Event::new(T - 100, "sendmeter", None, kind, d)).unwrap();
    ev(
        "account-capped",
        json!({"account": "B", "until": T + 3600, "meter": "5h 100%, 7d 40%"}),
    );
    ev("account-signed-out", json!({"account": "C", "meter": "usage 401"}));
    let row = json!({"ts": T - 30, "account": "A", "five_hour_pct": 41.0, "five_hour_reset": T + 600, "seven_day_pct": 63.0, "seven_day_reset": T + 86400});
    std::fs::write(c.state.with_file_name("usage.jsonl"), format!("{row}\n")).unwrap();
    let r = Fake::default()
        .stored("herdsman/all/TOK_A", TOKEN)
        .stored("herdsman/all/DEEPSEEK_API_KEY", KEY)
        .reply("curl", BALANCE);
    (c, r)
}

fn call(s: &Server, (method, path): (&str, &str), body: &str, cookie: &str) -> (String, Value) {
    let r = format!(
        "{method} {path} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&r, T)).into_owned();
    let (head, body) = res.split_once("\r\n\r\n").unwrap();
    let v = serde_json::from_str(body).unwrap_or_else(|e| panic!("{e}: {res}"));
    (head.lines().next().unwrap().into(), v)
}

#[test]
fn providers_lists_accounts_with_state_and_usage_and_never_a_secret() {
    let (c, r) = board("providers-list");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let (status, v) = call(&s, ("GET", "/api/v1/providers"), "", &ck);
    assert_eq!(status, "HTTP/1.1 200 OK", "{v}");
    let text = v.to_string();
    assert!(!text.contains("FAKETOKEN") && !text.contains(KEY), "{text}");
    let p = |n: &str| {
        v["providers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == n)
            .cloned()
            .unwrap()
    };
    let (a, b, cc) = (p("A"), p("B"), p("C"));
    assert_eq!(
        [&a, &b, &cc].map(|p| p["state"].as_str().unwrap()),
        ["ok", "capped", "signed out"]
    );
    assert_eq!(
        (a["kind"].as_str(), a["secret"].as_str()),
        (Some("anthropic"), Some("herdsman/all/TOK_A"))
    );
    // The default conductor runs on its fleet's impl role, so on A too.
    assert_eq!(
        a["roles"],
        json!(["sendmeter.conductor", "sendmeter.impl", "sendmeter.rev"])
    );
    assert_eq!(a["fleets"], json!(["sendmeter"]));
    let meters: Vec<_> = a["account"]["meters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["pct"].clone())
        .collect();
    assert_eq!(meters, [json!(41.0), json!(63.0)]);
    assert_eq!(b["until"], json!(T + 3600));
    assert!(b["account"]["cap"].as_str().unwrap().contains("5h 100%"), "{b}");
    assert!(
        cc["account"]["cap"]
            .as_str()
            .unwrap()
            .starts_with("signed out: usage 401"),
        "{cc}"
    );
    let ds = p("DEEPSEEK_API_KEY");
    assert_eq!(
        (ds["kind"].as_str(), ds["state"].as_str()),
        (Some("deepseek"), Some("ok"))
    );
    assert_eq!(ds["balance"]["balance"], "USD 12.50");
}

#[test]
fn unpaired_gets_401_for_providers_and_every_provider_op() {
    let (c, r) = board("providers-unpaired");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let (status, v) = call(&s, ("GET", "/api/v1/providers"), "", "");
    assert_eq!(
        (status.as_str(), &v["code"]),
        ("HTTP/1.1 401 Unauthorized", &json!("UNPAIRED"))
    );
    for op in ["sign-in", "key-add", "key-check", "key-remove", "use"] {
        let body = json!({"name": "DS_X", "key": KEY, "account": "A", "to": "B"}).to_string();
        let (status, v) = call(&s, ("POST", &format!("/api/v1/{op}")), &body, "");
        assert_eq!(
            (status.as_str(), &v["code"]),
            ("HTTP/1.1 401 Unauthorized", &json!("UNPAIRED")),
            "{op}"
        );
    }
    assert!(r.secret("herdsman/all/DS_X").unwrap().is_none());
}

#[test]
fn key_add_stores_the_key_and_logs_no_value_then_check_and_remove() {
    let (c, r) = board("providers-key");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let post = |op: &str, v: Value| call(&s, ("POST", &format!("/api/v1/{op}")), &v.to_string(), &ck);
    let fresh = "sk-fresh-0123456789abcdefFAKE";
    let (status, v) = post("key-add", json!({"name": "DS_TEAM", "key": fresh}));
    assert_eq!(status, "HTTP/1.1 200 OK", "{v}");
    assert_eq!(v["message"], "key DS_TEAM saved; DS_TEAM: USD 12.50");
    assert_eq!(r.secret("herdsman/all/DS_TEAM").unwrap().as_deref(), Some(fresh));
    let log = serde_json::to_string(&read(&c.state).unwrap()).unwrap();
    assert!(!log.contains(fresh) && !log.contains("FAKE"), "{log}");
    assert!(
        log.contains("\"type\":\"secret-set\"") && log.contains("\"provider\":\"deepseek\""),
        "{log}"
    );
    // The key reached curl on stdin only, never its argv.
    assert!(!r.calls.borrow().iter().any(|c| c.contains(fresh)));
    let (_, v) = call(&s, ("GET", "/api/v1/providers"), "", &ck);
    let team = v["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "DS_TEAM")
        .cloned();
    assert_eq!(
        team.map(|t| (t["state"].clone(), t["balance"]["balance"].clone())),
        Some((json!("ok"), json!("USD 12.50")))
    );
    // An account's token name is refused; key-check reads the store; key-remove deletes it.
    let (status, v) = post("key-add", json!({"name": "TOK_A", "key": fresh}));
    assert_eq!(
        (status.as_str(), &v["code"]),
        ("HTTP/1.1 409 Conflict", &json!("BAD_VALUE"))
    );
    assert_eq!(r.secret("herdsman/all/TOK_A").unwrap().as_deref(), Some(TOKEN));
    assert_eq!(
        post("key-check", json!({"name": "DS_TEAM"})).1["message"],
        "DS_TEAM: USD 12.50"
    );
    assert_eq!(post("key-remove", json!({"name": "DS_TEAM"})).1["code"], "OK");
    assert!(r.secret("herdsman/all/DS_TEAM").unwrap().is_none());
    assert_eq!(post("key-check", json!({"name": "DS_TEAM"})).1["code"], "NO_SUCH_KEY");
}

#[test]
fn use_posts_a_config_proposal_and_never_writes_fleet_toml() {
    let (c, r) = board("providers-use");
    assert_eq!(
        run(&c, &r, &["thread", "new", "sendmeter", "Accounts"]),
        (0, "t1\n".into())
    );
    let before = std::fs::read_to_string(&c.config).unwrap();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let body = json!({"to": "B", "scope": "sendmeter", "thread": "t1", "move": true}).to_string();
    let (status, v) = call(&s, ("POST", "/api/v1/use"), &body, &ck);
    assert_eq!(status, "HTTP/1.1 200 OK", "{v}");
    assert!(
        v["message"].as_str().unwrap().starts_with("c1: use B for sendmeter"),
        "{v}"
    );
    assert_eq!(
        std::fs::read_to_string(&c.config).unwrap(),
        before,
        "fleet.toml is untouched"
    );
    let ev = read(&c.state).unwrap();
    let p = ev.iter().find(|e| e.kind == "config-propose").unwrap();
    assert_eq!(p.data["use"]["roles"], json!(["sendmeter.impl", "sendmeter.rev"]));
    let text = p.str("text").unwrap();
    assert!(
        text.contains("account = \"B\"") && text.contains("accounts = [\"B\", \"C\", \"A\"]"),
        "{text}"
    );
    assert!(!ev.iter().any(|e| e.kind == "set"));
    // Only the owner's apply writes it.
    assert_eq!(run(&c, &r, &["config", "apply", "c1"]).0, 0);
    assert!(std::fs::read_to_string(&c.config).unwrap().contains("account = \"B\""));
}

#[test]
fn sign_in_captures_the_token_into_the_store_never_shown_or_logged() {
    let (c, r) = board("providers-sign-in");
    let r = r
        .reply(
            "herdr pane read",
            "Your OAuth token:\n\nsk-ant-oat01-FAKECAPTURED\nxyz\n\nStore it safely.\n",
        )
        .reply("herdr", r#"{"result": {"root_pane": {"pane_id": "w9:p1"}}}"#);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let (_, v) = call(
        &s,
        ("POST", "/api/v1/sign-in"),
        &json!({"account": "C"}).to_string(),
        &ck,
    );
    assert_eq!((&v["code"], &v["message"]), (&json!("OK"), &json!("w9:p1")), "{v}");
    assert!(r.called("herdr pane run w9:p1"), "{:?}", r.calls.borrow());
    let (_, v) = call(&s, ("GET", "/api/v1/setup/w9:p1"), "", &ck);
    assert_eq!(v["closed"], true, "{v}");
    assert!(!v.to_string().contains("FAKECAPTURED"));
    assert_eq!(
        r.secret("herdsman/all/TOK_C").unwrap().as_deref(),
        Some("sk-ant-oat01-FAKECAPTUREDxyz")
    );
    let log = serde_json::to_string(&read(&c.state).unwrap()).unwrap();
    assert!(!log.contains("FAKECAPTURED"), "{log}");
    let ev = read(&c.state).unwrap();
    assert!(ev
        .iter()
        .any(|e| e.kind == "setup-closed" && e.str("why") == Some("captured")));
    assert!(ev
        .iter()
        .any(|e| e.kind == "account-reset" && e.str("account") == Some("C")));
}
