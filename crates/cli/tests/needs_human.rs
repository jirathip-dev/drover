//! #130: `needs-human` is never silent: it pushes by default, and every such issue gets a needs-you
//! card (Queue + Herd strip) with [merge anyway] only when the PR could merge but for the round limit.
mod common;
use common::*;
use drover::config::Config;
use drover::events::{append, fold, read, Event};
use drover::serve::Server;
use serde_json::json;

const NOW: u64 = 1_000_000;
const ADDR: &str = "127.0.0.1:7777";
const HEAD: &str = "1111111111111111111111111111111111111111";

/// #12: four review rounds (the last `last` at HEAD), then `needs-human`; the PR cache says open, green, mergeable.
fn fixture(name: &str, last: &str) -> drover::Ctx {
    let mut c = ctx(&tmpdir(name), CONFIG);
    c.now = NOW;
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let ev = |ts, kind, d| append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
    ev(
        10,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"}),
    );
    for (i, sha) in ["2", "3", "4"].iter().enumerate() {
        ev(
            20 + i as u64,
            "verdict",
            json!({"verdict": "FAIL", "sha": sha.repeat(40)}),
        );
    }
    let body = format!("{last}\n\nblocking: src/x.rs:9 still wrong\n");
    ev(30, "verdict", json!({"verdict": last, "sha": HEAD, "body": body}));
    ev(
        31,
        "needs-human",
        json!({"code": "ROUND_LIMIT", "reason": "review rounds ran out (4/3)"}),
    );
    let checks = json!([{"__typename": "CheckRun", "name": "build", "status": "COMPLETED", "conclusion": "SUCCESS"}]);
    let pr = json!({"number": 34, "state": "OPEN", "baseRefName": "staging", "headRefOid": HEAD, "mergeable": "MERGEABLE", "statusCheckRollup": checks});
    let cache = c.state.parent().unwrap().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let file = json!({"ts": NOW, "branch": "issue-12", "pr": pr});
    std::fs::write(cache.join("pr-sendmeter-12.json"), file.to_string()).unwrap();
    c
}

fn runner() -> Fake {
    Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("herdr workspace list", r#"{"result":{"workspaces":[]}}"#)
}

/// The response at `path`; the page (#269) with its needs cards as the board draws them.
fn get(s: &Server, path: &str, cookie: &str) -> String {
    let res = s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}\r\n"), NOW);
    let res = String::from_utf8_lossy(&res).into_owned();
    if path == "/" {
        with_board(&res, s, ADDR, cookie, NOW)
    } else {
        res
    }
}

/// The `#needs` strip's card for #12.
fn card(page: &str) -> &str {
    let a = page.find("<details class=ask data-n=12>").expect("no needs-human card");
    &page[a..a + page[a..].find("</details>").unwrap()]
}

#[test]
fn needs_human_pushes_with_the_default_on() {
    let cfg = Config::parse("[[notify]]\ntype = \"webpush\"\nsub = \"mailto:owner@example.com\"\n").unwrap();
    assert!(cfg.notify[0].wants("needs-human"));
}

#[test]
fn card_with_a_pass_at_head_offers_merge_anyway_to_a_paired_phone_only() {
    let c = fixture("nh-pass", "PASS");
    let r = runner();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "", NOW);
    let page = get(&s, "/", &cookie);
    let k = card(&page);
    assert!(
        k.contains("sendmeter · #12") && k.contains("review rounds ran out (4/3)"),
        "{k}"
    );
    assert!(k.contains("PASS at 1111111") && k.contains("mergeable"), "{k}");
    assert!(k.contains("value=override") && k.contains("data-confirm="), "{k}");
    assert!(
        k.contains("value=extend>one more round") && k.contains("value=park>park"),
        "{k}"
    );
    assert!(
        page.find("<details class=ask data-n=12>") < page.find("<main>"),
        "above the tabs"
    );
    assert!(
        k.starts_with(
            "<details class=ask data-n=12><summary><span>#12 · needs-human · review rounds ran out (4/3)</span>"
        ),
        "#816: folded to one line: {k}"
    );
    assert!(
        k.contains(
            r#"<button form="nf-sendmeter-12" name=op value=extend><span>one more round</span></button></summary>"#
        ) && k.contains(r#"<form method=post id="nf-sendmeter-12">"#),
        "#816: the line's main button posts the card's form: {k}"
    );

    let unpaired = get(&s, "/", "");
    let k = card(&unpaired);
    assert!(
        !k.contains("<form") && !k.contains("<button"),
        "an unpaired phone sees no buttons: {k}"
    );
    let ro = Server::new(&c, &r, None);
    let page = get(&ro, "/", "");
    let k = card(&page);
    assert!(!k.contains("<form"), "a read-only server shows no buttons: {k}");
}

#[test]
fn card_with_a_fail_has_no_merge_anyway() {
    let c = fixture("nh-fail", "FAIL");
    let r = runner();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "", NOW);
    let page = get(&s, "/", &cookie);
    let k = card(&page);
    assert!(k.contains("FAIL: blocking: src/x.rs:9 still wrong"), "{k}");
    assert!(!k.contains("value=override"), "{k}");
    assert!(k.contains("value=extend") && k.contains("value=park"), "{k}");
}

#[test]
fn herd_marks_the_issue_and_park_clears_the_card() {
    let c = fixture("nh-herd", "FAIL");
    let r = runner();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let res = get(&s, "/herd.json", "");
    let j: serde_json::Value = serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    assert_eq!(j["fleets"][0]["needs"], json!([12]));
    assert_eq!(j["fleets"][0]["queue"][0]["issue"], 12, "no lane: a gate horse");

    let page = get(&s, "/api/v1/state", ""); // #268: the forms send `State.mtime`
    let i = page.find("\"mtime\":\"").unwrap() + 9;
    let mt: String = page[i..].chars().take_while(char::is_ascii_digit).collect();
    let body = format!("op=park&fleet=sendmeter&mtime={mt}&issue=12");
    let post = |extra: &str| {
        let req = format!(
            "POST / HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        String::from_utf8_lossy(&s.respond(&req, NOW)).into_owned()
    };
    assert!(post("").starts_with("HTTP/1.1 401"), "unpaired cannot park");
    let cookie = pair(&s, &c, "", NOW);
    assert!(post(&cookie).starts_with("HTTP/1.1 303"));
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
    assert_eq!(st.state, "parked");
    assert!(!get(&s, "/", &cookie).contains("data-n=12"), "parked: no card");
}

/// Finding 1/2 of the first review: the card ops act only on a needs-human issue. #217: `[one more round]` logs
/// `round-extended` first; #778: a spawn held back (PAUSED) keeps it pending, its card answered, and a second tap grants
/// no more.
#[test]
fn card_ops_need_needs_human_and_a_refused_extend_logs_nothing() {
    let c = fixture("nh-guard", "FAIL");
    let r = runner();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "", NOW);
    assert_eq!(run(&c, &r, &["pause", "sendmeter"]).0, 0);
    let page = get(&s, "/", &cookie);
    let i = page.find("name=mtime value=").unwrap() + 17;
    let mt: String = page[i..].chars().take_while(char::is_ascii_digit).collect();
    let post = |op: &str| {
        let body = format!("op={op}&fleet=sendmeter&mtime={mt}&issue=12");
        let req = format!(
            "POST / HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        String::from_utf8_lossy(&s.respond(&req, NOW)).into_owned()
    };
    let extended = || {
        read(&c.state)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "round-extended")
            .count()
    };

    let res = post("extend");
    assert!(
        res.contains("msg=%2312%3A%20extend"),
        "PAUSED only holds the spawn: {res}"
    );
    let p = drover::pending::of(&read(&c.state).unwrap(), "sendmeter", 12).expect("pending");
    assert_eq!((p.reason.as_deref(), p.failed), (Some("PAUSED"), false));
    assert!(
        !get(&s, "/", &cookie).contains("<details class=ask data-n=12>"),
        "the card stays answered"
    );
    assert!(post("extend").contains("not%20needs"), "a second tap");
    assert_eq!(extended(), 1, "the decision is logged once, before the spawn");
    assert_eq!(fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)].extra, 1);
    let failed = read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "extend-failed")
        .count();
    assert_eq!(failed, 0);

    append(&c.state, &Event::new(40, "sendmeter", Some(12), "merged", json!({}))).unwrap();
    for op in ["park", "extend", "override"] {
        let res = post(op);
        assert!(res.contains("not%20needs"), "{op}: {res}");
    }
    assert_eq!(
        fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)].state,
        "merged"
    );
}

/// #217: a recorded extension with no lane yet is one muted pending line: no form, no buttons.
#[test]
fn a_recorded_extension_is_a_pending_line_without_buttons() {
    let c = fixture("nh-pending", "FAIL");
    append(
        &c.state,
        &Event::new(
            40,
            "sendmeter",
            Some(12),
            "round-extended",
            json!({"by": "owner", "spawn": true}),
        ),
    )
    .unwrap();
    let r = runner();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "", NOW);
    let page = get(&s, "/", &cookie);
    assert!(!page.contains("<details class=ask data-n=12>"), "no needs-you card");
    let a = page.find("<div class=pend data-n=12>").expect("no pending line");
    let p = &page[a..a + page[a..].find("</div>").unwrap()];
    assert!(p.contains("⟳ one more round · starting a lane…"), "{p}");
    assert!(!p.contains("<form") && !p.contains("<button"), "{p}");

    // Logs before #217 wrote `round-extended` right after its spawn: that lane exists, nothing is pending.
    let spawn = json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"});
    append(&c.state, &Event::new(41, "sendmeter", Some(12), "spawn", spawn)).unwrap();
    append(
        &c.state,
        &Event::new(42, "sendmeter", Some(12), "round-extended", json!({"by": "owner"})),
    )
    .unwrap();
    assert!(
        !get(&s, "/", &cookie).contains("class=pend"),
        "an old-order extension is not pending"
    );
}
