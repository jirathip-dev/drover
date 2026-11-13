mod common;
use common::*;
use drover::events::{read, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

fn req(s: &Server, method: &str, path: &str, body: &str, extra: &str) -> (u16, Value) {
    let r = format!(
        "{method} {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&r, T)).into_owned();
    let (head, body) = res.split_once("\r\n\r\n").unwrap();
    (head[9..12].parse().unwrap(), serde_json::from_str(body).unwrap())
}

fn board(name: &str, extra: &str) -> (drover::Ctx, OnShed, String) {
    let (c, r) = v1_board(name);
    let text = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(
        &c.config,
        text.replacen("integration = \"staging\"   # never main\n", extra, 1),
    )
    .unwrap();
    let ck = pair(&Server::new(&c, &r, Some(ADDR.into())), &c, "Mozilla/5.0 (iPhone)", T);
    (c, r, ck)
}

fn get(s: &Server, ck: &str) -> Value {
    let (status, v) = req(s, "GET", "/api/v1/config", "", ck);
    assert_eq!(status, 200, "{v}");
    v
}

fn field<'a>(v: &'a Value, path: &str) -> &'a Value {
    v["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == path)
        .unwrap_or_else(|| panic!("no {path}"))
}

fn edits(c: &drover::Ctx) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == "config-edit")
        .collect()
}

const KEEP: &str = "integration = \"staging\"   # never main\nenv = { STRIPE = \"secret:STRIPE_KEY\" }\n";

#[test]
fn read_gives_every_setting_with_secrets_as_names_and_needs_a_paired_device() {
    let (c, r, ck) = board("cfg-read", KEEP);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    assert_eq!(req(&s, "GET", "/api/v1/config", "", "").0, 401);
    let v = get(&s, &ck);
    let text = std::fs::read_to_string(&c.config).unwrap();
    assert_eq!(v["hash"], json!(drover::devices::sha256(text.as_bytes())));
    assert_eq!(v["text"], json!(text));
    let env = field(&v, "fleet.sendmeter.env.STRIPE");
    assert_eq!((&env["secret"], &env["kind"]), (&json!("STRIPE_KEY"), &json!("string")));
    let model = field(&v, "fleet.sendmeter.role.impl.model");
    assert_eq!(
        (&model["value"], &model["allowed"]),
        (&json!("opus"), &json!(["opus", "sonnet"]))
    );
    assert_eq!(
        field(&v, "fleet.sendmeter.role.impl.harness")["allowed"],
        json!(["claude", "hermes", "dsh"])
    );
    assert_eq!(
        field(&v, "fleet.sendmeter.integration")["doc"],
        json!("drover merge target; main is never a target")
    );
    assert_eq!(field(&v, "limits.review_rounds")["kind"], json!("integer"));
    let token = format!("ghp_{}", "a1".repeat(18));
    std::fs::write(&c.config, text.replace("secret:STRIPE_KEY", &token)).unwrap();
    let (status, v) = req(&s, "GET", "/api/v1/config", "", &ck);
    assert_ne!(status, 200);
    assert!(!v.to_string().contains(&token), "{v}");
    assert!(v.to_string().contains("SECRET_IN_CONFIG"), "{v}");
}

#[test]
fn a_stale_hash_or_an_invalid_edit_writes_nothing() {
    let (c, r, ck) = board("cfg-stale", KEEP);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let before = std::fs::read_to_string(&c.config).unwrap();
    let hash = get(&s, &ck)["hash"].clone();
    let model = json!([{"path": "fleet.sendmeter.role.impl.model", "value": "sonnet"}]);
    let (status, v) = req(
        &s,
        "POST",
        "/api/v1/config-edit",
        &json!({"hash": "x", "set": model}).to_string(),
        &ck,
    );
    assert_eq!((status, &v["code"]), (409, &json!("STALE")), "{v}");
    let bad = json!({"hash": hash, "set": [{"path": "fleet.sendmeter.role.impl.harness", "value": "bogus"}]});
    let (status, v) = req(&s, "POST", "/api/v1/config-check", &bad.to_string(), &ck);
    assert_eq!(status, 200);
    assert!(v["diff"].as_str().unwrap().contains("+ harness  =  \"bogus\""), "{v}");
    assert!(
        v["errors"][0].as_str().unwrap().contains("harness must be one of"),
        "{v}"
    );
    let (status, v) = req(&s, "POST", "/api/v1/config-edit", &bad.to_string(), &ck);
    assert_eq!((status, &v["code"]), (409, &json!("BAD_VALUE")), "{v}");
    let raw = json!({"hash": hash, "text": "[fleet.x\n"});
    let (_, v) = req(&s, "POST", "/api/v1/config-check", &raw.to_string(), &ck);
    assert_eq!(v["errors"].as_array().unwrap().len(), 1, "{v}");
    assert_eq!(
        req(&s, "POST", "/api/v1/config-edit", &raw.to_string(), &ck).1["code"],
        json!("BAD_VALUE")
    );
    assert_eq!(req(&s, "POST", "/api/v1/config-edit", &raw.to_string(), "").0, 401);
    assert_eq!(std::fs::read_to_string(&c.config).unwrap(), before);
    assert!(edits(&c).is_empty());
}

#[test]
fn a_field_edit_keeps_comments_backs_up_and_logs_the_owner() {
    let (c, r, ck) = board("cfg-edit", KEEP);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let before = std::fs::read_to_string(&c.config).unwrap();
    let set = json!([
        {"path": "fleet.sendmeter.role.impl.harness", "value": "claude"},
        {"path": "fleet.sendmeter.role.impl.model", "value": "sonnet"},
        {"path": "models", "value": ["opus", "sonnet", "haiku"]},
        {"path": "limits.stray_roots", "value": ["/scratch"]},
    ]);
    let body = json!({"hash": get(&s, &ck)["hash"], "set": set}).to_string();
    let (status, v) = req(&s, "POST", "/api/v1/config-edit", &body, &ck);
    assert_eq!(status, 200, "{v}");
    let after = std::fs::read_to_string(&c.config).unwrap();
    for kept in [
        "# fleet config (test)\n",
        "harness  =  \"claude\"    # toggled in tests\n",
        "integration = \"staging\"   # never main\n",
        "stray_roots = [\"/scratch\"]          # #536: never clean the real /tmp or $TMPDIR\n",
        "models = [\"opus\", \"sonnet\", \"haiku\"]\n",
        "model = \"sonnet\"\n",
    ] {
        assert!(after.contains(kept), "{kept:?} not in\n{after}");
    }
    let e = edits(&c).pop().unwrap();
    assert_eq!(
        (e.str("by"), e.data["doctor"].is_boolean()),
        (Some("owner (cooee)"), true)
    );
    assert_eq!(std::fs::read_to_string(e.str("backup").unwrap()).unwrap(), before);
    assert!(e.str("diff").unwrap().contains("+ model = \"sonnet\""));
    assert_eq!(
        req(&s, "POST", "/api/v1/config-edit", &body, &ck).1["code"],
        json!("STALE")
    );
}

#[test]
fn a_fleet_is_added_removed_paused_and_resumed_with_each_logged() {
    let (c, r, ck) = board("cfg-fleets", KEEP);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let before = std::fs::read_to_string(&c.config).unwrap();
    let post = |op: &str, v: Value| req(&s, "POST", &format!("/api/v1/{op}"), &v.to_string(), &ck);
    let add = json!({"name": "web", "repo": "acme/web", "local": "/tmp/does-not-matter/web", "integration": "staging"});
    let (status, v) = post("config-edit", json!({"hash": get(&s, &ck)["hash"], "add": add}));
    assert_eq!(status, 200, "{v}");
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let web = cfg.fleet("web").unwrap();
    assert_eq!((web.repo.as_str(), web.integration.as_str()), ("acme/web", "staging"));
    assert_eq!(web.role("impl").unwrap().profile.as_deref(), Some("fleet-impl"));
    assert_eq!(web.role("rev").unwrap().harness, "hermes");
    assert!(std::fs::read_to_string(&c.config).unwrap().starts_with(&before));
    assert_eq!(edits(&c).pop().unwrap().str("added"), Some("web"));
    let (status, v) = post("config-edit", json!({"hash": get(&s, &ck)["hash"], "remove": "web"}));
    assert_eq!(status, 200, "{v}");
    assert_eq!(
        std::fs::read_to_string(&c.config).unwrap().trim_end(),
        before.trim_end()
    );
    let e = edits(&c).pop().unwrap();
    assert_eq!(
        (e.fleet.as_str(), e.str("removed"), e.str("by")),
        ("web", Some("web"), Some("owner (cooee)"))
    );
    let gone = json!({"hash": get(&s, &ck)["hash"], "remove": "sendmeter"});
    assert_eq!(post("config-edit", gone).1["code"], json!("LANE_LIVE"));
    assert_eq!(
        std::fs::read_to_string(&c.config).unwrap().trim_end(),
        before.trim_end()
    );
    for op in ["pause", "resume"] {
        let mtime = req(&s, "GET", "/api/v1/state", "", "").1["mtime"].clone();
        let (status, v) = post(op, json!({"fleet": "sendmeter", "mtime": mtime}));
        assert_eq!(status, 200, "{v}");
        let paused = drover::config::Config::load(&c.config)
            .unwrap()
            .fleet("sendmeter")
            .unwrap()
            .paused;
        assert_eq!(paused, op == "pause");
        let e = read(&c.state)
            .unwrap()
            .to_vec()
            .into_iter()
            .rfind(|e| e.kind == op)
            .unwrap();
        assert_eq!((e.fleet.as_str(), e.str("via")), ("sendmeter", Some("web")));
    }
}
