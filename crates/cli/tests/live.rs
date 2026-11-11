//! #42: the live board: `/live` SSE frags, the client cap, the blocked-prompt parser and
//! answering a blocked Claude menu from the needs-you card.
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use herdsman::lanes::{pending, prompt};
use herdsman::serve::Server;
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

/// A server on a real socket (fake herdr: no agents); returns its address.
fn live_server(name: &str) -> (herdsman::Ctx, std::net::SocketAddr) {
    let c = ctx(&tmpdir(name), CONFIG);
    append(&c.state, &Event::new(T, "sendmeter", None, "note", json!({}))).unwrap();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || {
        herdsman::serve::serve_on(&c2, &Fake::default().reply("herdr agent list", NO_AGENTS), l, false)
    });
    (c, addr)
}

fn subscribe(addr: std::net::SocketAddr, host: &str) -> TcpStream {
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(format!("GET /live HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes())
        .unwrap();
    s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    s
}

/// What the stream sends within `ms`.
fn drain(s: &mut TcpStream, ms: u64) -> String {
    let (end, mut out, mut b) = (Instant::now() + Duration::from_millis(ms), vec![], [0u8; 65_536]);
    while Instant::now() < end {
        match s.read(&mut b) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&b[..n]),
            Err(_) => {}
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[test]
fn live_pushes_one_frag_per_changed_section() {
    let (c, addr) = live_server("live");
    let mut s = subscribe(addr, &addr.to_string());
    let hello = drain(&mut s, 1_500);
    assert!(
        hello.starts_with("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n")
            && hello.contains(": ping\n\nevent: build\ndata: "),
        "{hello}"
    );
    assert_eq!(
        hello.matches("event:").count(),
        1,
        "nothing changed yet, the build alone (#391): {hello}"
    );
    let quiet = drain(&mut s, 2_500);
    assert!(
        quiet.lines().all(|l| l.is_empty() || l == ": ping"),
        "no change: pings only: {quiet}"
    );
    // #266: a queued issue changes the Queue tab only (the Events tab is the board's, read on the event's `ev`); #268:
    // the Queue tab is the board's too: its data arrives as `queue`, once, and no frag.
    let q = json!({"op": "add", "issues": [99]});
    append(&c.state, &Event::new(T + 5, "sendmeter", None, "queue", q)).unwrap();
    let t0 = Instant::now();
    let mut got = String::new();
    while !got.contains("\n\n") && t0.elapsed() < Duration::from_millis(1_500) {
        got += &drain(&mut s, 50);
    }
    assert!(
        t0.elapsed() < Duration::from_millis(1_500),
        "the queue data within 1.5 s: {got}"
    );
    got += &drain(&mut s, 1_200);
    assert_eq!(got.matches("event: frag").count(), 0, "{got}");
    assert_eq!(got.matches("event: queue").count(), 1, "{got}");
    let line = got
        .lines()
        .find(|l| l.starts_with("data: {\"error\""))
        .expect("the queue data");
    let v: serde_json::Value = serde_json::from_str(&line[6..]).unwrap();
    assert_eq!(v["fleets"][0]["rows"][0]["issue"], 99, "{v}");
    assert_eq!(v["now"].as_u64().map(|n| n > 0), Some(true), "{v}");
    assert!(
        got.contains("event: ev\ndata: {\"f\":\"sendmeter\",\"k\":null,\"n\":null,\"seq\":2,"),
        "{got}"
    );
    // #267: a drive pass's result changes the Host tab's FLEETS row: its data arrives as `host`, once.
    let d = json!({"step": "start", "result": "REFUSED HOST_BUSY: load 9", "code": "HOST_BUSY"});
    append(&c.state, &Event::new(T + 6, "sendmeter", Some(7), "driven", d)).unwrap();
    let got = drain(&mut s, 2_500);
    assert_eq!(got.matches("event: host").count(), 1, "{got}");
    let line = got
        .lines()
        .find(|l| l.starts_with("data: {\"accounts\""))
        .expect("the host data");
    let v: serde_json::Value = serde_json::from_str(&line[6..]).unwrap();
    assert_eq!(v["fleets"][0]["word"], "held", "{v}");
    assert_eq!(v["now"].as_u64().map(|n| n > 0), Some(true), "{v}");
    // #269: an ask changes the needs cards: their data (as this unpaired device sees it) arrives as `needs`, once.
    let a = json!({"id": "q1", "question": "Ship it?", "options": ["yes", "no"], "recommend": null, "kind": "owner"});
    append(&c.state, &Event::new(T + 7, "sendmeter", Some(7), "needs-decision", a)).unwrap();
    let got = drain(&mut s, 2_500);
    assert_eq!(got.matches("event: needs\n").count(), 1, "{got}");
    assert!(
        got.contains("\"question\":\"Ship it?\"") && got.contains("\"paired\":false"),
        "{got}"
    );
    assert_eq!(got.matches("event: lanes\n").count(), 0, "Lanes unchanged: {got}");
}

#[test]
fn live_caps_clients_and_checks_host() {
    let (_c, addr) = live_server("cap");
    let mut foreign = subscribe(addr, "evil.example");
    assert!(drain(&mut foreign, 500).starts_with("HTTP/1.1 403 "));
    let mut held: Vec<TcpStream> = (0..8).map(|_| subscribe(addr, &addr.to_string())).collect();
    for s in &mut held {
        assert!(drain(s, 300).starts_with("HTTP/1.1 200 OK"));
    }
    let mut ninth = subscribe(addr, &addr.to_string());
    assert!(drain(&mut ninth, 500).starts_with("HTTP/1.1 503 "), "the 9th client");
    drop(held);
    std::thread::sleep(Duration::from_millis(1_500));
    let mut again = subscribe(addr, &addr.to_string());
    assert!(
        drain(&mut again, 300).starts_with("HTTP/1.1 200 OK"),
        "closed clients free their slots"
    );
    // #269: the server sends no HTML part (`/frags` is gone): the needs cards, Lanes and [chat] are API v1 data.
    let get = |path: &str| {
        let mut s = TcpStream::connect(addr).unwrap();
        s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
            .unwrap();
        let mut b = String::new();
        s.read_to_string(&mut b).unwrap();
        b
    };
    assert!(get("/frags?tab=lanes").starts_with("HTTP/1.1 404 "));
    for part in ["needs", "lanes", "chat"] {
        let r = get(&format!("/api/v1/{part}"));
        let j: serde_json::Value = serde_json::from_str(&r[r.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert!(j["now"].as_u64().is_some(), "{part}: {j}");
    }
}

const MENU: &str = "⏺ I need to pick a database.\n\n Which database should the meter use?\n\n❯ 1. SQLite\n  2. Postgres\n  3. Type something.\n\nEnter to select · ↑/↓ to navigate · Esc to cancel\n";
const PERMISSION: &str = "╭──────────────────────────────────────────╮\n│ Bash command                             │\n│   cargo test --all                       │\n│ Do you want to proceed?                  │\n│ ❯ 1. Yes                                 │\n│   2. Yes, and don't ask again for cargo  │\n│      commands in /p/sendmeter            │\n│   3. No, and tell Claude what to do      │\n│      differently (esc)                   │\n╰──────────────────────────────────────────╯\n";

#[test]
fn prompt_parser_fixtures() {
    assert_eq!(
        pending(MENU),
        Some((
            "Which database should the meter use?".into(),
            vec!["SQLite".into(), "Postgres".into(), "Type something.".into()]
        ))
    );
    let (q, opts) = pending(PERMISSION).unwrap();
    assert_eq!(q, "Do you want to proceed?");
    assert_eq!(
        opts,
        [
            "Yes",
            "Yes, and don't ask again for cargo",
            "No, and tell Claude what to do"
        ]
    );
    assert_eq!(
        pending("⏺ Done with the migration.\n\nShould I also update the README?\n\n> \n"),
        Some(("Should I also update the README?".into(), vec![]))
    );
    assert_eq!(
        pending("dsh> cargo build\n   Compiling herdsman\n    Finished dev\ndsh> "),
        None,
        "dsh pane"
    );
    // A numbered list in Claude's own reply (no `❯` cursor) is not a menu.
    assert_eq!(pending("Plan:\n1. Read the spec\n2. Write the code\n"), None);
    for junk in [
        "",
        "\n\n\n",
        "❯",
        "❯ 1.",
        "1. \n2. ",
        "❯ 99999999999999999999. x\n2. y",
        "\u{0}\u{1b}[2J?",
        "│││",
        "❯ 2. b\n❯ 1. a",
    ] {
        let _ = pending(junk);
    }
    assert_eq!(
        pending("❯ 2. b\n  1. a\nWhich?"),
        Some(("Which?".into(), vec![])),
        "out of order: no menu, never a guess"
    );
}

/// #127: the real screen (paths masked) of a prompt Claude auto-denies after 15 s.
const COUNTDOWN: &str = include_str!("golden/screens/claude-permission-countdown.txt");

#[test]
fn prompt_card_golden_from_the_auto_deny_screen() {
    assert_eq!(
        prompt(COUNTDOWN).unwrap(),
        json!({
            "title": "Bash command",
            "reason": "Dangerous rm operation on possibly-empty variable path: $S/*.png in `rm -f $S/*.png` (rewrite it as \"${S:?}\"/*.png or use a literal path)",
            "command": "cp /Users/me/qg.bak tests/queue_graph.rs && cargo fmt && S=/Users/me/snaps; rm -f $S/*.png; HERDSMAN_SNAPSHOTS=$S cargo test --test queue_graph",
            "countdown": 14,
            "question": "Do you want to proceed?",
            "options": ["Yes", "No"],
            "push": false
        })
    );
    // No countdown: it waits for the owner, so it pushes; a countdown that ran out pushes too.
    let c = prompt(PERMISSION).unwrap();
    assert_eq!(
        (c["title"].as_str(), c["countdown"].as_u64(), c["push"].as_bool()),
        (Some("Bash command"), None, Some(true))
    );
    assert_eq!(c["command"], "cargo test --all");
    assert_eq!(prompt(&COUNTDOWN.replace("in 0:14", "in 0:00")).unwrap()["push"], true);
    assert_eq!(
        prompt("Should I also update the README?\n> "),
        None,
        "no menu, no card JSON"
    );
}

#[test]
fn countdown_card_shows_reason_command_and_clears_when_the_prompt_goes() {
    let (c, r) = blocked("countdown", &[COUNTDOWN, COUNTDOWN]);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = cookie(&s, &c);
    let page = page(&s, "/", &ck);
    let needs = &page[page.find("<div id=needs>").unwrap()..page.find("<main>").unwrap()];
    for want in [
        "data-push=0",
        "<p class=title>Bash command</p>",
        "<p class=reason>Dangerous rm operation on possibly-empty variable path",
        "<p class=command>cp /Users/me/qg.bak",
        &format!("<p class=cd data-until={}>auto-denies in 0:14</p>", T + 14),
        "class=pick value=2",
    ] {
        assert!(needs.contains(want), "{want}: {needs}");
    }
    assert!(!needs.contains("data-push=1"), "{needs}");
    // Auto-denied: the next read shows Claude working again, so the card goes.
    let working = "⏺ Bash(cargo test)\n  ⎿ Error: denied\n\n✶ Pontificating… (12m 0s · ↓ 3.1k tokens)\n";
    let r2 = Fake::default()
        .reply(
            "herdr agent list",
            &agents(&[("sendmeter-impl-12", "blocked", "/x", "w1:p2")]),
        )
        .reply("herdr workspace list", "{}")
        .reply("herdr pane read", working);
    let s2 = Server::new(&c, &r2, Some(ADDR.into()));
    let page =
        String::from_utf8_lossy(&s2.respond(&format!("GET /?all=1 HTTP/1.1\r\nHost: {ADDR}\r\n{ck}\r\n"), T + 5))
            .into_owned();
    let page = with_board(&page, &s2, ADDR, &ck, T + 5);
    let needs = &page[page.find("<div id=needs>").unwrap()..page.find("<main>").unwrap()];
    assert!(!needs.contains("sendmeter-impl-12"), "no stale card: {needs}");
}

/// sendmeter-impl-12 (claude) blocked on a permission prompt; sendmeter-rev-9-r1 too.
fn blocked(name: &str, screens: &[&str]) -> (herdsman::Ctx, Fake) {
    let c = ctx(&tmpdir(name), CONFIG);
    for (issue, role, agent) in [(12, "impl", "sendmeter-impl-12"), (9, "rev", "sendmeter-rev-9-r1")] {
        append(
            &c.state,
            &Event::new(
                T - 100,
                "sendmeter",
                Some(issue),
                "spawn",
                json!({"role": role, "agent": agent, "harness": "claude"}),
            ),
        )
        .unwrap();
    }
    let list = agents(&[
        ("sendmeter-impl-12", "blocked", "/x", "w1:p2"),
        ("sendmeter-rev-9-r1", "blocked", "/x", "w1:p3"),
    ]);
    let mut r = Fake::default()
        .reply("herdr agent list", &list)
        .reply("herdr workspace list", "{}")
        .reply("herdr pane send-keys", "");
    for s in screens {
        r = r.once("herdr pane read", 0, s);
    }
    (c, r.reply("herdr pane read", PERMISSION))
}

/// #269: the page at `path` as the board draws it for the request's device (`extra`: its cookie).
fn page(s: &Server, path: &str, extra: &str) -> String {
    with_board(&req(s, "GET", path, "", extra), s, ADDR, extra, T)
}

fn req(s: &Server, method: &str, path: &str, body: &str, extra: &str) -> String {
    let r = format!(
        "{method} {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&r, T)).into_owned()
}

/// The `data-h` the needs-you card's menu buttons carry (the menu as the card shows it).
fn shown_h(page: &str) -> String {
    let i = page.find("data-h=").expect("menu buttons") + 7;
    page[i..].chars().take_while(char::is_ascii_digit).collect()
}

/// Paired at the real clock (#288: a pairing lasts a year after last use), as `serve_on` reads it.
fn cookie(s: &Server, c: &herdsman::Ctx) -> String {
    pair(
        s,
        c,
        "",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
}

#[test]
fn answer_sends_the_digit_and_logs_no_question() {
    let (c, r) = blocked("answer", &[]);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = cookie(&s, &c);
    let page = page(&s, "/", &ck);
    let needs = &page[page.find("<div id=needs>").unwrap()..page.find("<main>").unwrap()];
    let h = shown_h(needs);
    assert!(
        needs.contains("Do you want to proceed?")
            && needs.contains(&format!(
                "<button type=button class=pick value=2 data-h={h}>2. Yes, and don"
            )),
        "{needs}"
    );
    assert!(needs.contains("data-ask=\"sendmeter-impl-12\""), "{needs}");
    let res = req(&s, "POST", "/answer", "agent=sendmeter-impl-12&choice=2", "");
    assert!(res.starts_with("HTTP/1.1 401 "), "no device cookie: {res}");
    assert!(!r.called("herdr pane send-keys"));
    let foreign = req(
        &s,
        "POST",
        "/answer",
        "agent=sendmeter-impl-12&choice=2",
        &format!("{ck}Origin: https://evil.example\r\n"),
    );
    assert!(foreign.starts_with("HTTP/1.1 403 "), "{foreign}");
    for bad in ["0", "4", "12", "2%0A", "Enter"] {
        let res = req(
            &s,
            "POST",
            "/answer",
            &format!("agent=sendmeter-impl-12&choice={bad}"),
            &ck,
        );
        assert!(res.contains("REFUSED BAD_VALUE"), "{bad}: {res}");
    }
    assert!(!r.called("herdr pane send-keys"), "never a guessed key");
    for screen in [PERMISSION, "⏺ Bash(cargo test --all)\n  ⎿ ok"] {
        r.onces.borrow_mut().push((
            "herdr pane read".into(),
            herdsman::runner::Out {
                code: 0,
                stdout: screen.into(),
                stderr: String::new(),
            },
        ));
    }
    let res = req(
        &s,
        "POST",
        "/answer",
        &format!("agent=sendmeter-impl-12&choice=2&h={h}"),
        &ck,
    );
    assert!(
        res.starts_with("HTTP/1.1 200 OK") && res.contains("sent to sendmeter-impl-12"),
        "{res}"
    );
    assert_eq!(r.count("herdr pane send-keys"), 1);
    assert!(r.called("herdr pane send-keys w1:p2 2"));
    let e = read(&c.state).unwrap().pop().unwrap();
    assert_eq!((e.kind.as_str(), e.issue), ("answer", Some(12)));
    assert_eq!(
        serde_json::Value::Object(e.data.clone()),
        json!({"via": "web", "agent": "sendmeter-impl-12", "device": e.str("device").unwrap(), "choice": "2"})
    );
    assert!(
        !herdsman::events::jsonl(&c.state).unwrap().contains("proceed"),
        "never the question text"
    );
    // The prompt is still there after the key: "still waiting". A reviewer may answer a permission prompt.
    let res = req(
        &s,
        "POST",
        "/answer",
        &format!("agent=sendmeter-rev-9-r1&choice=1&h={h}"),
        &ck,
    );
    assert!(res.contains("still waiting: sendmeter-rev-9-r1"), "{res}");
    assert!(r.called("herdr pane send-keys w1:p3 1"));
}

/// The card showed menu A; the pane (read afresh, past every cache) now shows menu B: no key, and
/// "menu changed". After a send, the next render reads the pane again (no cached stale menu).
#[test]
fn answer_refuses_a_changed_menu_and_rerenders_fresh() {
    let (c, r) = blocked("stale-menu", &[]);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = cookie(&s, &c);
    let h = shown_h(&page(&s, "/", &ck));
    let b = PERMISSION.replace("for cargo  ", "for git    ");
    r.onces.borrow_mut().push((
        "herdr pane read".into(),
        herdsman::runner::Out {
            code: 0,
            stdout: b,
            stderr: String::new(),
        },
    ));
    let res = req(
        &s,
        "POST",
        "/answer",
        &format!("agent=sendmeter-impl-12&choice=2&h={h}"),
        &ck,
    );
    assert!(res.contains("REFUSED STALE: menu changed — refreshed"), "{res}");
    assert_eq!(
        r.count("herdr pane send-keys"),
        0,
        "menu A tapped, menu B on the pane: no key"
    );
    let res = req(&s, "POST", "/answer", "agent=sendmeter-impl-12&choice=2", &ck);
    assert!(res.contains("REFUSED STALE"), "no h: {res}");
    assert_eq!(r.count("herdr pane send-keys"), 0);
    let reads = r.count("herdr pane read");
    r.onces.borrow_mut().push((
        "herdr pane read".into(),
        herdsman::runner::Out {
            code: 0,
            stdout: PERMISSION.into(),
            stderr: String::new(),
        },
    ));
    r.onces.borrow_mut().push((
        "herdr pane read".into(),
        herdsman::runner::Out {
            code: 0,
            stdout: "⏺ Bash(cargo test --all)\n  ⎿ ok".into(),
            stderr: String::new(),
        },
    ));
    r.onces.borrow_mut().push((
        "herdr pane read".into(),
        herdsman::runner::Out {
            code: 0,
            stdout: "⏺ Bash(cargo test --all)\n  ⎿ ok".into(),
            stderr: String::new(),
        },
    ));
    let res = req(
        &s,
        "POST",
        "/answer",
        &format!("agent=sendmeter-impl-12&choice=2&h={h}"),
        &ck,
    );
    assert!(
        res.contains("sent to sendmeter-impl-12") && r.called("herdr pane send-keys w1:p2 2"),
        "same menu: the digit: {res}"
    );
    // Same second (cache TTLs not expired): the page still reads the pane afresh and drops the old buttons.
    let page = page(&s, "/", &ck);
    assert!(r.count("herdr pane read") > reads + 2, "a fresh read after the send");
    let card = &page
        [page.find("data-ask=\"sendmeter-impl-12\"").unwrap()..page.find("data-ask=\"sendmeter-rev-9-r1\"").unwrap()];
    assert!(!card.contains("class=pick") && card.contains("⎿ ok"), "{card}");
}

#[test]
fn answer_refuses_reviewer_menus_and_unparsed_panes() {
    let (c, r) = blocked("answer-rev", &[MENU, "garbage, no prompt"]);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = cookie(&s, &c);
    let res = req(&s, "POST", "/answer", "agent=sendmeter-rev-9-r1&choice=1", &ck);
    assert!(res.contains("REFUSED REV_READONLY"), "{res}");
    let res = req(&s, "POST", "/answer", "agent=sendmeter-impl-12&choice=1", &ck);
    assert!(res.contains("REFUSED BAD_VALUE: no menu"), "{res}");
    assert!(!r.called("herdr pane send-keys"));
}

#[test]
fn unparsed_blocked_pane_shows_masked_tail_and_reply() {
    let (c, r) = blocked(
        "tail",
        &[concat!(
            "l1\nl2\nl3\nl4\nl5\nl6\nl7 token=gh",
            "p_abcdefghijklmnopqrstuvwxyz0123456789\nl8"
        )],
    );
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = cookie(&s, &c);
    let page = page(&s, "/", &ck);
    let needs = &page[page.find("<div id=needs>").unwrap()..page.find("<main>").unwrap()];
    assert!(
        needs.contains("<pre>l3\nl4") && needs.contains("class=rp>Reply") && !needs.contains("l2\n"),
        "{needs}"
    );
    assert!(
        !needs.contains(concat!("gh", "p_abcdefghijklmnopqrstuvwxyz0123456789")),
        "masked: {needs}"
    );
}

/// The PR's 390 px screenshots (fixture config): the sync dot's three states, a pull in
/// progress, a blocked agent's permission menu on the needs-you card and in its sheet, and
/// the card after an answer; each with no horizontal scroll. Runs where WebKit is at hand;
/// PNGs go to `$HERDSMAN_SNAPSHOTS` when set.
#[test]
fn live_board_fits_390px() {
    if no_webkit() {
        return;
    }
    let (c, r) = blocked("live-shots", &[COUNTDOWN]);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = cookie(&s, &c);
    let page = drawn(&req(&s, "GET", "/?all=1", "", &ck), &s, ADDR, &ck, T);
    let d = tmpdir("live-png");
    let file = d.join("page.html");
    std::fs::write(&file, &page[page.find("<!doctype").unwrap()..]).unwrap();
    let stub =
        "window.fetch = async u => new Response(u === '/answer' ? 'sent to sendmeter-impl-12' : '{}', {status: 200})";
    let scenes = [
        ("sync-green", "queue", "sync('g'); pulse()".to_string()),
        ("sync-amber", "queue", "sync('a')".into()),
        ("sync-red", "queue", "last = Date.now() - 600000; sync('r')".into()),
        ("pull", "queue", "sync('g'); M.style.transform = 'translateY(60px)'; ptr.hidden = false; ptr.style.transform = 'rotate(240deg)'".into()),
        ("needs-you-menu", "queue", "sync('g')".into()),
        ("needs-you-countdown", "queue", "sync('g')".into()),
        ("answer-sent", "queue", format!("sync('g'); {stub}; document.querySelector('.ask .pick[value=\"1\"]').click()")),
        ("sheet-blocked", "lanes", "sync('g'); open(document.querySelector('.lane[data-agent=\"sendmeter-impl-12\"]'))".into()),
    ];
    let dir = std::env::var("HERDSMAN_SNAPSHOTS").ok();
    for (name, tab, js) in scenes {
        let png = dir.as_ref().map_or(d.join(format!("{name}.png")), |p| {
            std::path::Path::new(p).join(format!("{name}.png"))
        });
        let o = webview()
            .args([file.to_str().unwrap(), "390", tab, "horse", png.to_str().unwrap(), &js])
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&o.stdout);
        let w: Vec<u32> = out.split_whitespace().filter_map(|n| n.parse().ok()).collect();
        assert!(
            o.status.success() && w.len() == 2,
            "{name}: {out} {}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(w[0] <= 390 && w[1] == 390, "{name}: scrollWidth {} at {}", w[0], w[1]);
    }
}

/// #259: `/live` from `Last-Event-ID` / `?since=`: the events after it, once each, as `ev` with its seq.
fn reconnect(addr: std::net::SocketAddr, since: &str) -> String {
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(format!("GET /live{since} HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
        .unwrap();
    s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    drain(&mut s, 300)
}

#[test]
fn live_messages_carry_the_seq_and_a_reconnect_replays_what_it_missed() {
    let (c, addr) = live_server("replay");
    let mut s = subscribe(addr, &addr.to_string());
    drain(&mut s, 1_500);
    append(&c.state, &Event::new(T + 1, "sendmeter", Some(7), "merged", json!({}))).unwrap();
    let got = drain(&mut s, 1_500);
    assert!(
        got.contains(
            "id: 2\nevent: ev\ndata: {\"f\":\"sendmeter\",\"k\":\"merged\",\"n\":7,\"seq\":2,\"ts\":1000001}\n\n"
        ),
        "{got}"
    );
    assert!(
        !got.contains("\"id\":\"events\""),
        "#266: no Events frag, the board reads the API: {got}"
    );
    // Gate 2: the phone drops at seq 2; three events land; it comes back with Last-Event-ID: 2.
    drop(s);
    for i in 0..3 {
        append(
            &c.state,
            &Event::new(
                T + 10 + i,
                "sendmeter",
                None,
                "note",
                json!({"reason": format!("missed-{i}")}),
            ),
        )
        .unwrap();
    }
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(format!("GET /live HTTP/1.1\r\nHost: {addr}\r\nLast-Event-ID: 2\r\n\r\n").as_bytes())
        .unwrap();
    s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    let back = drain(&mut s, 2_500);
    let ids: Vec<&str> = back.lines().filter(|l| l.starts_with("data: {\"f\"")).collect();
    assert_eq!(
        ids.len(),
        3,
        "each missed event once, the live tick resends none: {back}"
    );
    for n in 3..=5 {
        assert_eq!(back.matches(&format!("\"seq\":{n},")).count(), 1, "{back}");
    }
    assert_eq!(reconnect(addr, "?since=4").matches("event: ev").count(), 1);
    assert_eq!(
        reconnect(addr, "?since=5").matches("event:").count(),
        1,
        "the build alone (#391)"
    );
    // Every missed Events row is on the tab once (#266: the board's, from `/api/v1/events`).
    let tab = events_tab(
        &Server::new(&c, &Fake::default(), Some(addr.to_string())),
        &addr.to_string(),
        T + 20,
        "{ f: { k: 'all', fl: [] } }",
    );
    for i in 0..3 {
        let Some(tab) = &tab else { break };
        assert_eq!(tab.matches(&format!(">note · missed-{i}<")).count(), 1, "{tab}");
    }
    // Past the 500-event bound: one `refresh`, no replay.
    for i in 0..501 {
        append(&c.state, &Event::new(T + 30 + i, "sendmeter", None, "note", json!({}))).unwrap();
    }
    let far = reconnect(addr, "?since=5");
    assert!(
        far.contains("id: 506\nevent: refresh\ndata: 506\n\n") && !far.contains("event: ev"),
        "{far}"
    );
    assert!(reconnect(addr, "?since=6").matches("event: ev").count() == 500);
}

/// #259: the board plays a moment once per seq, never for a replayed event older than 2 minutes.
#[test]
fn the_board_dedupes_by_seq_and_skips_stale_moments() {
    if std::process::Command::new("node").arg("--version").output().is_err() {
        return eprintln!("skipping: no node here");
    }
    let src = include_str!("../../../assets/serve.js");
    let part = &src[src.find("  const at = e").unwrap()..src.find("  es.addEventListener('refresh'").unwrap()];
    let js = format!(
        "let seq = 0; const played = [], told = [], h = {{}}, es = {{ addEventListener: (n, f) => (h[n] = f) }};\n\
         const rmoment = k => played.push(k), dispatchEvent = e => told.push(e.type);\n{part}\n\
         const now = Math.floor(Date.now() / 1e3), ev = (s, ts, k) => h.ev({{ lastEventId: String(s), data: JSON.stringify({{ seq: s, ts, k }}) }});\n\
         ev(1, now, 'merged'); ev(1, now, 'merged'); ev(2, now - 300, 'fail'); ev(3, now, null); ev(2, now, 'fail'); ev(4, now, 'needs-you');\n\
         console.log(played.join(','), seq, told.join());"
    );
    let d = tmpdir("dedupe-js");
    std::fs::write(d.join("t.js"), js).unwrap();
    let o = std::process::Command::new("node").arg(d.join("t.js")).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    // #266: and tells the board (its Events tab) once per new seq.
    assert_eq!(
        String::from_utf8_lossy(&o.stdout).trim(),
        "merged,needs-you 4 herd-ev,herd-ev,herd-ev,herd-ev"
    );
}

/// Pane text (the question, its options, the tail) only reaches a paired device: GET /, API v1's needs cards and
/// Lanes (#269) and `/live` show an unpaired viewer who is blocked, the activity and a pairing hint.
#[test]
fn unpaired_viewers_see_no_pane_text() {
    let (c, r) = blocked("unpaired", &[]);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = cookie(&s, &c);
    let pane = [
        "proceed",
        "class=pick",
        "class=rp",
        "cargo test --all",
        "don&#39;t ask",
        "don't ask",
    ];
    // #269: what the server sends (the page, API v1's needs cards and Lanes), then as the board draws it.
    for path in ["/", "/api/v1/needs", "/api/v1/lanes"] {
        let res = req(&s, "GET", path, "", "");
        assert!(pane.iter().all(|p| !res.contains(p)), "{path}: {res}");
        assert!(
            req(&s, "GET", path, "", &ck).contains("Do you want to proceed?") || path == "/",
            "{path} paired"
        );
    }
    let res = page(&s, "/", "");
    assert!(
        pane.iter().all(|p| !res.contains(p)) && res.contains("pair this phone to answer"),
        "{res}"
    );
    assert!(page(&s, "/", &ck).contains("Do you want to proceed?"), "paired");
    // /live: the blocked lane appears after the watcher's first look; each audience gets its own render.
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || herdsman::serve::serve_on(&c2, &r.once("herdr agent list", 0, NO_AGENTS), l, false));
    let sub = |extra: &str| {
        let mut s = TcpStream::connect(addr).unwrap();
        s.write_all(format!("GET /live HTTP/1.1\r\nHost: {addr}\r\n{extra}\r\n").as_bytes())
            .unwrap();
        s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        s
    };
    let (mut anon, mut dev) = (sub(""), sub(&ck));
    let (a, d) = (drain(&mut anon, 4_000), drain(&mut dev, 500));
    assert!(a.contains("event: needs\n") && a.contains("\"activity\":\""), "{a}");
    assert!(pane.iter().all(|p| !a.contains(p)), "unpaired stream: {a}");
    assert!(
        d.contains("Do you want to proceed?") && d.contains("\"menu\":{"),
        "paired stream: {d}"
    );
    // The device is removed: its stream closes before the next push, so no more pane text reaches it.
    herdsman::devices::store(&c, &[]).unwrap();
    append(&c.state, &Event::new(T + 5, "sendmeter", None, "note", json!({}))).unwrap();
    let after = drain(&mut dev, 2_500);
    assert!(pane.iter().all(|p| !after.contains(p)), "revoked device: {after}");
    assert!(matches!(dev.read(&mut [0; 1]), Ok(0)), "revoked device: stream closed");
}

/// #57: serve's sampler thread fills the host (`/herd.json`) without a request sampling it.
#[test]
fn the_sampler_thread_fills_the_host() {
    let c = ctx(&tmpdir("sampler"), CONFIG);
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let r = Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("vm_stat", "Swapouts: 100.\n")
        .reply("sysctl", "4\n");
    std::thread::spawn(move || herdsman::serve::serve_on(&c, &r, l, false));
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let mut s = TcpStream::connect(addr).unwrap();
        s.write_all(format!("GET /herd.json HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
            .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        if out.contains(r#""pressure":"critical""#) {
            break;
        }
        assert!(Instant::now() < end, "the host never filled: {out}");
        std::thread::sleep(Duration::from_millis(50));
    }
}
