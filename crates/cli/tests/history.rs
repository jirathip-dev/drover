//! #351: bounded history on first open: a lane's chat carries its last 10 turns within ~64 KB (10 more a scroll),
//! the Events tab its last 100 rows a view, then 100 a page (`frag?id=events&skip=`).
mod common;
use common::*;
use herdsman::events::{append, Event};
use herdsman::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

fn get(s: &Server, path: &str, extra: &str) -> String {
    let res = s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}\r\n"), T);
    let res = String::from_utf8_lossy(&res).into_owned();
    assert!(
        res.starts_with("HTTP/1.1 200"),
        "{path}: {}",
        &res[..res.len().min(300)]
    );
    res[res.find("\r\n\r\n").unwrap() + 4..].to_string()
}

/// A Claude lane with 40 turns of 2 KB in its session transcript and 600 pane lines of 300 bytes: the sheet's first
/// read is its last 10 turns and the lines that fit in 32 KB (≤ 64 KB in all); `turns=20` and `before=` page back.
#[test]
fn a_chat_opens_with_its_last_10_turns_within_64kb() {
    let dir = tmpdir("chat-window");
    // `Lane::session` reads `$HOME/.claude/projects`; this binary's only test that reads HOME.
    std::env::set_var("HOME", dir.join("home"));
    let c = ctx(&dir, CONFIG);
    let wt = dir.join("wt/issue-12").to_string_lossy().into_owned();
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude", "worktree": wt});
    append(&c.state, &Event::new(T - 100, "sendmeter", Some(12), "spawn", d)).unwrap();
    let proj = dir
        .join("home/.claude/projects")
        .join(wt.replace(|c: char| !c.is_ascii_alphanumeric(), "-"));
    std::fs::create_dir_all(&proj).unwrap();
    let turn = |i: u32| {
        let (k, text) = (
            ["user", "assistant"][i as usize % 2],
            format!("turn {i} {}", "x".repeat(2000)),
        );
        json!({"type": k, "message": {"content": text}, "timestamp": format!("2026-11-03T09:{:02}:00Z", i)}).to_string()
    };
    std::fs::write(proj.join("s.jsonl"), (0..40).map(turn).collect::<Vec<_>>().join("\n")).unwrap();
    let screen: String = (0..600).map(|i| format!("⏺ line {i} {}\n", "y".repeat(290))).collect();
    let list = json!({"result": {"agents": [{"name": "sendmeter-impl-12", "agent_status": "working", "pane_id": "w1:p2", "cwd": wt}]}});
    let r = Fake::default()
        .reply("herdr agent list", &list.to_string())
        .reply("herdr pane read", &screen);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone) Safari/604.1", T);
    let first = get(&s, "/lane?agent=sendmeter-impl-12&lines=200", &ck);
    let j: Value = serde_json::from_str(&first).unwrap();
    let said = j["said"].as_array().unwrap();
    assert_eq!(said.len(), 10, "the last 10 turns");
    assert!(said[9]["text"].as_str().unwrap().starts_with("turn 39 "));
    let lines = j["lines"].as_array().unwrap().len();
    assert!((50..200).contains(&lines), "the lines that fit in 32 KB: {lines}");
    assert!(j["older"] == true && j["more"] == true, "{}", &first[..300]);
    assert!(
        first.len() <= 64 * 1024,
        "first open is {} bytes (budget 64 KB)",
        first.len()
    );
    // Scrolling up: 10 more turns (their budget grows with them), the lines before the ones seen.
    let j: Value = serde_json::from_str(&get(
        &s,
        &format!("/lane?agent=sendmeter-impl-12&turns=20&before={lines}"),
        &ck,
    ))
    .unwrap();
    assert_eq!(j["said"].as_array().unwrap().len(), 20);
    let older = j["lines"].as_array().unwrap();
    assert!(!older.is_empty() && j["before"].as_u64() > Some(lines as u64), "{j}");
}

/// 250 events: each view opens with its last 100 rows and an [older] button; `skip=100` is the next 100, `skip=200`
/// the last 50 with no [older].
#[test]
fn events_open_with_100_rows_then_page() {
    let c = ctx(&tmpdir("events-pages"), CONFIG);
    for i in 0..250 {
        let d = json!({"reason": format!("n-{i}")});
        append(&c.state, &Event::new(T - 10_000 + i * 10, "sendmeter", None, "note", d)).unwrap();
    }
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let view = |skip: u64| {
        let h = get(&s, &format!("/frag?id=events&skip={skip}"), "");
        let all = h[h.find("<div data-v=all").unwrap()..].to_string();
        (all.matches("<details class=\"er").count(), all)
    };
    let (n, first) = view(0);
    assert_eq!(n, 100, "{first}");
    assert!(first.contains("n-249") && !first.contains("n-149<") && first.contains("data-skip=100"));
    let (n, page) = view(100);
    assert_eq!(n, 100);
    assert!(page.contains("n-149") && !page.contains("n-150<") && page.contains("data-skip=200"));
    let (n, last) = view(200);
    assert_eq!(n, 50);
    assert!(last.contains("n-0<") && !last.contains("data-skip"));
    // The page and `/frags` carry the first page only.
    let tab = get(&s, "/frags?tab=events", "");
    assert_eq!(
        tab.matches("details class=\\\"er").count(),
        100,
        "notes are not important: the all view's 100"
    );
}
