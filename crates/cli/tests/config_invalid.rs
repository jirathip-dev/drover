mod common;
use common::*;
use drover::events::read;
use drover::serve::Server;
use serde_json::{json, Value};

const HOST: &str = "127.0.0.1:7777";
const GOOD: &str =
    "[fleet.acme]\nrepo = \"acme/app\"\nlocal = \"/tmp/does-not-matter/app\"\nintegration = \"staging\"\n";
const WANT: &str = "config invalid: invalid value: integer `-1`, expected usize (fleet.toml:5)";

fn bad() -> String {
    format!("{GOOD}max_impl = -1\n")
}

fn health(s: &Server, now: u64) -> Value {
    serde_json::from_str(&api(s, HOST, "", now, "health")).unwrap()
}

fn alerts(c: &drover::Ctx, kind: &str) -> usize {
    read(&c.state).unwrap().iter().filter(|e| e.kind == kind).count()
}

#[test]
fn an_invalid_config_shows_a_banner_and_alerts_once_until_it_is_fixed() {
    let c = ctx(&tmpdir("config-invalid"), GOOD);
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(HOST.into()));
    assert_eq!(health(&s, c.now)["config"], "ok");
    assert!(api(&s, HOST, "", c.now, "queue").contains("acme/app"));

    std::fs::write(&c.config, bad()).unwrap();
    assert!(
        api(&s, HOST, "", c.now, "queue").contains("acme/app"),
        "the queue tab keeps its data"
    );
    let needs: Value = serde_json::from_str(&api(&s, HOST, "", c.now, "needs")).unwrap();
    assert_eq!(needs["config"], WANT, "banner: {needs}");
    let screen = |p: &str| {
        let r = String::from_utf8_lossy(&s.respond(&format!("GET {p} HTTP/1.1\r\nHost: {HOST}\r\n\r\n"), c.now))
            .into_owned();
        r[r.find("\r\n\r\n").unwrap() + 4..].to_string()
    };
    let cards: Value = serde_json::from_str(&screen("/screens/cards")).unwrap();
    let card = cards
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["id"] == "config-invalid")
        .cloned();
    assert_eq!(
        card.map(|k| k["blocks"][0]["markdown"].clone()),
        Some(json!(WANT)),
        "the page's banner card: {cards}"
    );
    let h = health(&s, c.now);
    assert_eq!(
        (h["ok"].as_bool(), h["config"].as_str()),
        (Some(false), Some("invalid")),
        "{h}"
    );
    assert_eq!(h["error"], WANT);

    for _ in 0..2 {
        let (code, out) = run(&c, &r, &["check"]);
        assert_ne!(code, 0, "{out}");
    }
    assert_eq!(alerts(&c, "config-invalid"), 1);
    let (code, out) = run(&c, &r, &["tick"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains(WANT), "{out}");
    assert_eq!(alerts(&c, "config-invalid"), 1, "the same error alerts once");

    std::fs::write(&c.config, GOOD).unwrap();
    let needs: Value = serde_json::from_str(&api(&s, HOST, "", c.now, "needs")).unwrap();
    assert!(needs["config"].is_null(), "{needs}");
    assert!(!screen("/screens/cards").contains("config-invalid"));
    let h = health(&s, c.now);
    assert_eq!(
        (h["ok"].as_bool(), h["config"].as_str()),
        (Some(true), Some("ok")),
        "{h}"
    );
    run(&c, &r, &["check"]);
    assert_eq!(alerts(&c, "config-ok"), 1);
}
