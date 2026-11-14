//! #483: the Queue and Chat tabs show why they cannot read, config or log, in the error's own words.
mod common;
use common::*;
use drover::events::Event;
use drover::serve::Server;
use serde_json::Value;

const CFG: &str =
    "[fleet.acme]\nrepo = \"acme/app\"\nlocal = \"/tmp/does-not-matter/app\"\nintegration = \"staging\"\n";

fn error(s: &Server, tab: &str) -> String {
    let r = s.respond(
        &format!("GET /api/v1/{tab} HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n\r\n"),
        1_000_000,
    );
    let r = String::from_utf8_lossy(&r).into_owned();
    let v: Value = serde_json::from_str(&r[r.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    v["error"].as_str().unwrap_or_default().to_string()
}

#[test]
fn a_tab_names_the_real_read_error() {
    // The owner's own state dir (`$HOME/.local/state/drover`) with a log from before #344: reads refuse it.
    let h = tmpdir("board-errors");
    std::env::set_var("HOME", &h);
    let mut c = ctx(&h, CFG);
    c.state = h.join(".local/state/drover/events.jsonl");
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let ev = Event::new(1, "acme", Some(1), "queue-add", serde_json::json!({}));
    std::fs::write(&c.state, serde_json::to_string(&ev).unwrap() + "\n").unwrap();
    let r = Fake::default();
    for tab in ["queue", "chat"] {
        let e = error(&Server::new(&c, &r, Some("127.0.0.1:7777".into())), tab);
        assert!(e.contains("migrate --live"), "{tab}: {e}");
    }
    // A broken fleet.toml: the error names it and the parse error.
    let c = ctx(&tmpdir("board-errors-cfg"), "[fleet.acme\n");
    for tab in ["queue", "chat"] {
        let e = error(&Server::new(&c, &r, Some("127.0.0.1:7777".into())), tab);
        assert!(
            e.starts_with("config invalid: ") && e.contains("(fleet.toml:1)"),
            "{tab}: {e}"
        );
    }
}
