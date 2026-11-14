//! #316: a harness core has never heard of runs on the generic fallbacks: spawn → chat → card → memory row.
mod common;
use common::*;
use drover::events::{append, Event};
use drover::providers;
use drover::serve::Server;
use serde_json::json;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;
const LANE: &str = "sendmeter-impl-12";
/// A numbered menu: Claude's reader would offer its options; the generic one offers none.
const MENU: &str = "⏺ Bash(cargo test)\nWhich database should the meter use?\n❯ 1. SQLite\n  2. Postgres\n";

#[test]
fn the_generic_harness_has_every_fallback() {
    let h = providers::get("gemini");
    assert!(h.env().is_empty() && h.health_check().is_none() && h.process_names().is_empty());
    assert!(h.login().is_none() && h.hook_dialect().is_none() && h.token_env().is_none());
    let banner = "Claude usage limit reached. Your limit will reset at 3pm.";
    assert!(!h.signed_out("Please run /login") && !h.usage_limited(banner));
    assert_eq!(h.tool_line("⏺ Bash(cargo test)"), None);
    assert_eq!(h.answer_keys(2), "2");
    assert_eq!(
        (h.pending_menu(MENU), h.status_word("✻ Herding… (4m 12s)")),
        (None, None)
    );
    assert_eq!(h.transcript(std::path::Path::new("/"), "/wt"), None);
    assert!(h.now_line("$ cargo test") && !h.now_line("────────────────────"));
    assert_eq!(
        drover::lanes::activity("gemini", MENU),
        "2. Postgres",
        "the last non-chrome line"
    );
}

#[test]
fn a_generic_lane_reads_its_pane_answers_by_reply_and_gets_its_own_memory_row() {
    let d = tmpdir("generic-harness");
    let c = ctx(&d, CONFIG);
    let wt = d.join("wt").to_string_lossy().into_owned();
    std::fs::create_dir_all(&wt).unwrap();
    let sp = json!({"role": "impl", "agent": LANE, "harness": "gemini", "pane": LANE, "worktree": wt});
    append(&c.state, &Event::new(T - 60, "sendmeter", Some(12), "spawn", sp)).unwrap();
    let sh = FakeShed::new();
    sh.attach(&c);
    sh.lane(LANE, "blocked", &wt);
    sh.set(LANE, |t| t["labels"]["harness"] = json!("gemini"));
    sh.screen(LANE, MENU);
    let r = Fake::default()
        .reply("lsof", &format!("p201\ncgemini\nfcwd\nn{wt}\n"))
        .reply("ps -axo pid=,rss=", "  201 1048576\n");
    let r = sh.on(&r);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = pair(&s, &c, "", T);
    let get = |path: &str| {
        let res = s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n{ck}\r\n"), T);
        String::from_utf8_lossy(&res).into_owned()
    };
    // Chat: the pane's lines, no session transcript.
    let res = get(&format!("/lane?agent={LANE}"));
    let chat: serde_json::Value = serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).expect(&res);
    assert_eq!(
        (&chat["harness"], &chat["said"]),
        (&json!("gemini"), &json!(null)),
        "{chat}"
    );
    assert!(
        chat["lines"].as_array().unwrap().iter().any(|l| l == "  2. Postgres"),
        "{chat}"
    );
    // Card: the question and a reply box, never menu buttons.
    let page = with_board(&get("/?all=1"), &s, ADDR, &ck, T); // #269: the needs cards as the board draws them
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
    assert!(
        res.contains("no menu on") && !sh.called("keys", LANE) && sh.sent(LANE).is_empty(),
        "{res}"
    );
    // Memory: the gemini lane's pane tree in a row of its own, none under Claude.
    let page = host_tab(&s, ADDR, T);
    assert!(!page.contains("<span class=k>claude</span>"), "{page}");
    assert!(
        page.contains("<span class=k>gemini</span>")
            && page.contains("<b>1.0 GB</b> <span class=mt>(1 lanes: working 0 / done 0)</span>"),
        "{page}"
    );
}
