mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

fn setup(name: &str) -> drover::Ctx {
    let c = ctx(
        &tmpdir(name),
        &format!("{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\n"),
    );
    attach_chat(&c, "sendmeter", "t1");
    msgs(&c, T, 0..3);
    c
}

fn propose(c: &drover::Ctx) {
    let file = c.config.with_file_name("proposed.toml");
    let old = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(&file, old.replacen("model = \"opus\"", "model = \"sonnet\"", 1)).unwrap();
    let (code, out) = run(
        c,
        &Fake::default(),
        &["config", "propose", "t1", file.to_str().unwrap()],
    );
    assert_eq!((code, out.as_str()), (0, "c1\n"));
}

fn msgs(c: &drover::Ctx, ts: u64, ids: std::ops::Range<u64>) {
    for i in ids {
        let d = json!({"thread": "t1", "from": "orchestrator", "text": format!("m{i}")});
        append(&c.state, &Event::new(ts + i, "sendmeter", None, "thread-msg", d)).unwrap();
    }
}

#[test]
fn a_stale_proposal_cannot_apply() {
    let c = setup("cfg-window");
    propose(&c);
    let cur = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(&c.config, cur + "\n# hand edit\n").unwrap();
    let (code, out) = run(&c, &Fake::default(), &["config", "apply", "c1"]);
    assert!(code == 3 && out.contains("REFUSED STALE"), "{code} {out}");
    assert!(std::fs::read_to_string(&c.config).unwrap().ends_with("# hand edit\n"));
}

#[test]
fn applying_a_new_fleet_kicks_its_first_queued_issue() {
    let c = setup("cfg-add-fleet");
    let old = std::fs::read_to_string(&c.config).unwrap();
    let added = old.split("\n[limits]").next().unwrap().replace("sendmeter", "drover");
    let file = c.config.with_file_name("proposed.toml");
    std::fs::write(&file, format!("{old}\n{added}")).unwrap();
    let r = Fake::default();
    assert_eq!(
        run(&c, &r, &["config", "propose", "t1", file.to_str().unwrap()]).1,
        "c1\n"
    );
    for (f, n) in [("drover", 5), ("drover", 6), ("sendmeter", 9)] {
        let d = json!({"op": "add", "issues": [n]});
        append(&c.state, &Event::new(T, f, None, "queue", d)).unwrap();
    }
    let (code, out) = run(&c, &r, &["config", "apply", "c1"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.trim_end().ends_with(", drover #5 kicked"), "{out}");
    let calls = r.calls.borrow();
    let kicks: Vec<_> = calls.iter().filter(|c| c.contains(" kick ")).collect();
    assert_eq!(kicks.len(), 1, "{kicks:?}");
    assert!(
        kicks[0].contains(" kick drover 5 --why config-apply --now "),
        "{kicks:?}"
    );
}

#[test]
fn applying_a_new_role_syncs_its_profile() {
    let c = setup("cfg-sync");
    let old = std::fs::read_to_string(&c.config).unwrap();
    let file = c.config.with_file_name("proposed.toml");
    let role = "\n[fleet.sendmeter.role.verify]\nharness = \"claude\"\nprofile = \"fleet-verify\"\n";
    std::fs::write(&file, format!("{old}{role}")).unwrap();
    let r = Fake::default();
    assert_eq!(
        run(&c, &r, &["config", "propose", "t1", file.to_str().unwrap()]).1,
        "c1\n"
    );
    let (code, out) = run(&c, &r, &["config", "apply", "c1"]);
    assert_eq!(code, 0, "{out}");
    let settings = drover::providers::profiles_root(&c).join("fleet-verify/settings.json");
    assert!(std::fs::read_to_string(settings).unwrap().contains("hook --role rev"));
}

#[test]
fn a_proposal_over_16_kb_applies_byte_for_byte() {
    let c = setup("cfg-big");
    let old = std::fs::read_to_string(&c.config).unwrap();
    let pad: String = (0..400)
        .map(|i| format!("# padding line {i:04} of a long fleet.toml\n"))
        .collect();
    let new = format!("{}{pad}", old.replacen("model = \"opus\"", "model = \"sonnet\"", 1));
    assert!(new.len() > 16 << 10, "{}", new.len());
    let file = c.config.with_file_name("proposed.toml");
    std::fs::write(&file, &new).unwrap();
    let r = Fake::default();
    let (code, out) = run(&c, &r, &["config", "propose", "t1", file.to_str().unwrap()]);
    assert_eq!((code, out.as_str()), (0, "c1\n"));
    let (code, out) = run(&c, &r, &["config", "apply", "c1"]);
    assert_eq!(code, 0, "{out}");
    let got = std::fs::read_to_string(&c.config).unwrap();
    assert!(got == new, "applied {} bytes of {}", got.len(), new.len());
}

#[test]
fn a_proposal_stored_truncated_refuses_to_apply_and_asks_for_a_new_one() {
    let c = setup("cfg-cut");
    let old = std::fs::read_to_string(&c.config).unwrap();
    let base = drover::devices::sha256(old.as_bytes());
    let d =
        json!({"thread": "t1", "id": "c1", "diff": "+ x\n", "text": "[fleet… 6973 bytes cut …en A\"", "base": base});
    append(&c.state, &Event::new(T, "sendmeter", None, "config-propose", d)).unwrap();
    let (code, out) = run(&c, &Fake::default(), &["config", "apply", "c1"]);
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("REFUSED PROPOSAL_TRUNCATED") && out.contains("propose"),
        "{out}"
    );
    assert_eq!(std::fs::read_to_string(&c.config).unwrap(), old);
}

fn screen(s: &Server, cookie: &str, path: &str) -> Value {
    let r = format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}\r\n");
    let res = String::from_utf8_lossy(&s.respond(&r, T + 500)).into_owned();
    assert!(res.starts_with("HTTP/1.1 200"), "{path}: {res}");
    serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap()
}

fn config_cards(v: &Value) -> Vec<Value> {
    fn go(v: &Value, out: &mut Vec<Value>) {
        match v {
            Value::Object(m) if m.get("kind") == Some(&json!("config")) => out.push(v.clone()),
            Value::Object(m) => m.values().for_each(|x| go(x, out)),
            Value::Array(a) => a.iter().for_each(|x| go(x, out)),
            _ => {}
        }
    }
    let mut out = vec![];
    go(v, &mut out);
    out
}

fn labels(card: &Value) -> Vec<String> {
    let b = card["buttons"].as_array().cloned().unwrap_or_default();
    b.iter().map(|b| b["label"].as_str().unwrap().to_string()).collect()
}

#[test]
fn an_applied_proposal_shows_applied_with_no_buttons_and_never_stale() {
    let c = setup("cfg-applied");
    propose(&c);
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T + 500);
    let proposed = std::fs::read_to_string(c.config.with_file_name("proposed.toml")).unwrap();
    let old = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(&c.config, &proposed).unwrap();
    let mid = config_cards(&screen(&s, &cookie, "/screens/cards"));
    assert!(
        mid.iter().all(|k| !k.to_string().contains("stale")),
        "mid-apply: {mid:?}"
    );
    std::fs::write(&c.config, &old).unwrap();

    let (code, out) = run(&c, &r, &["config", "apply", "c1"]);
    assert_eq!(code, 0, "{out}");
    let backup = format!("{}.bak-{}", c.config.display(), c.now);
    let home = config_cards(&screen(&s, &cookie, "/screens/cards"));
    assert!(home.is_empty(), "settled: off the owner's cards: {home:?}");
    let all = rows(&c, &s, T + 500);
    let last = all.iter().rfind(|r| r["key"] == "config-c1").unwrap();
    let cards = config_cards(&last["block"]);
    assert_eq!(cards.len(), 1, "{cards:?}");
    let card = cards[0].to_string();
    assert!(
        card.contains("applied by owner") && card.contains(&backup),
        "who, when and the backup: {card}"
    );
    assert!(!card.contains("stale"), "{card}");
    assert!(labels(&cards[0]).is_empty(), "no [discard] or [later]: {card}");
}

#[test]
fn a_proposal_whose_fleet_toml_changed_another_way_shows_stale_with_discard() {
    let c = setup("cfg-stale-card");
    propose(&c);
    let cur = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(&c.config, cur + "\n# hand edit\n").unwrap();
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T + 500);
    let cards = config_cards(&screen(&s, &cookie, "/screens/cards"));
    assert_eq!(cards.len(), 1, "{cards:?}");
    assert!(
        cards[0]["subtitle"].as_str().unwrap().starts_with("stale"),
        "{}",
        cards[0]
    );
    assert_eq!(labels(&cards[0]), ["discard", "later"]);
}
