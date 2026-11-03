//! #316: a harness core has never heard of runs on the generic fallbacks: spawn → chat → card → memory row.
mod common;
use common::*;
use herdsman::events::{append, Event};
use herdsman::harness;
use herdsman::serve::Server;
use serde_json::json;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;
const LANE: &str = "sendmeter-impl-12";
/// A numbered menu: Claude's reader would offer its options; the generic one offers none.
const MENU: &str = "⏺ Bash(cargo test)\nWhich database should the meter use?\n❯ 1. SQLite\n  2. Postgres\n";

#[test]
fn the_generic_harness_has_every_fallback() {
    let h = harness::get("codex");
    assert!(h.env().is_empty() && !h.accounts() && h.health_check().is_none() && h.process_names().is_empty());
    assert_eq!(
        (h.pending_menu(MENU), h.status_word("✻ Herding… (4m 12s)")),
        (None, None)
    );
    assert_eq!(h.transcript(std::path::Path::new("/"), "/wt"), None);
    assert!(h.now_line("$ cargo test") && !h.now_line("────────────────────"));
    assert_eq!(
        herdsman::lanes::activity("codex", MENU),
        "2. Postgres",
        "the last non-chrome line"
    );
}

#[test]
fn a_generic_lane_reads_its_pane_answers_by_reply_and_holds_no_claude_memory() {
    let d = tmpdir("generic-harness");
    let c = ctx(&d, CONFIG);
    let wt = d.join("wt").to_string_lossy().into_owned();
    std::fs::create_dir_all(&wt).unwrap();
    let sp = json!({"role": "impl", "agent": LANE, "harness": "codex", "pane": "w1:p2", "worktree": wt});
    append(&c.state, &Event::new(T - 60, "sendmeter", Some(12), "spawn", sp)).unwrap();
    let mut list: serde_json::Value = serde_json::from_str(&agents(&[(LANE, "blocked", &wt, "w1:p2")])).unwrap();
    list["result"]["agents"][0]["agent"] = json!("codex");
    let r = Fake::default()
        .reply("herdr agent list", &list.to_string())
        .reply("herdr workspace list", "{}")
        .reply("herdr pane read", MENU)
        .reply("lsof", &format!("p201\nccodex\nfcwd\nn{wt}\n"))
        .reply("ps -axo pid=,rss=", "  201 1048576\n");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = pair(&s, &c, "", T);
    let get = |path: &str| {
        let res = s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n{ck}\r\n"), T);
        String::from_utf8_lossy(&res).into_owned()
    };
    // Chat: the pane's lines, no session transcript.
    let res = get(&format!("/lane?agent={LANE}"));
    let chat: serde_json::Value = serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    assert_eq!(
        (&chat["harness"], &chat["said"]),
        (&json!("codex"), &json!(null)),
        "{chat}"
    );
    assert!(
        chat["lines"].as_array().unwrap().iter().any(|l| l == "  2. Postgres"),
        "{chat}"
    );
    // Card: the question and a reply box, never menu buttons.
    let page = get("/");
    let needs = &page[page.find("<div id=needs>").unwrap()..page.find("<main>").unwrap()];
    assert!(
        needs.contains("Which database should the meter use?") && !needs.contains("class=pick"),
        "{needs}"
    );
    let body = format!("agent={LANE}&choice=1");
    let req = format!(
        "POST /answer HTTP/1.1\r\nHost: {ADDR}\r\n{ck}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&req, T)).into_owned();
    assert!(res.contains("no menu on") && !r.called("herdr pane send-keys"), "{res}");
    // Memory: no Claude row for a codex lane, and no row of its own.
    assert!(page.contains("<span class=k>memory</span>"), "the Host tab renders");
    assert!(!page.contains("<span class=k>claude</span>") && !page.contains("<span class=k>codex</span>"));
}
