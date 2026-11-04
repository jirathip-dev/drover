//! #386: the [chat] tab: the thread list and a thread from the event log (the demo's), a choice round-trip from the
//! tap to `DECISION` typed into the conductor, an image in and out, and a read-only thread on a device that is not paired.
mod common;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use common::*;
use herdsman::serve::Server;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

fn req(s: &Server, method: &str, path: &str, body: &str, extra: &str, now: u64) -> String {
    let r = format!(
        "{method} {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&r, now)).into_owned()
}

fn body(res: &str) -> &str {
    &res[res.find("\r\n\r\n").unwrap() + 4..]
}

/// The chat part as `/frags?tab=chat` sends it, and the needs-you part.
fn frags(s: &Server, cookie: &str, now: u64) -> (String, String) {
    let res = req(s, "GET", "/frags?tab=chat", "", cookie, now);
    let j: serde_json::Value = serde_json::from_str(body(&res)).unwrap();
    (j["chat"].as_str().unwrap().into(), j["needs"].as_str().unwrap().into())
}

/// The demo's fleets and log, in a temp dir; its thread files' paths are relative to the repo root.
fn demo() -> herdsman::Ctx {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::env::set_current_dir(&root).unwrap();
    let d = tmpdir("chat-demo");
    let c = ctx(&d, &std::fs::read_to_string(root.join("demo/fleet.toml")).unwrap());
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    std::fs::copy(root.join("demo/events.jsonl"), &c.state).unwrap();
    c
}

#[test]
fn the_tab_shows_each_repo_with_its_threads_and_a_thread_as_chat_blocks() {
    let c = demo();
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let now = 1_793_429_670;
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", now);
    let page = req(&s, "GET", "/", "", &format!("{cookie}Cookie: tab=chat\r\n"), now);
    assert!(
        page.contains("<a href=#lanes><span>lanes</span></a><a href=#chat><span>chat</span></a><a href=#host>"),
        "[chat] is the middle tab"
    );
    let (chat, needs) = frags(&s, &cookie, now);
    // The list: all repos first, then the fleets by name; newest thread first, the state and the last message.
    let at = |s: &str| chat.find(s).unwrap_or_else(|| panic!("{s} not in {chat}"));
    assert!(at("<b>all repos</b>") < at("<b>acme-api</b>") && at("<b>acme-api</b>") < at("<b>acme-web</b>"));
    assert!(chat.contains(
        "<a class=\"tr\" href=#chat data-t=\"t4\"><i class=ud>●</i><span class=tt>Rate limits for /v1/search</span>"
    ));
    assert!(chat.contains("<b class=r>! choose</b>conductor: Return 429 with `Retry-After` only"));
    assert!(
        chat.contains("<a class=\"tr rd\" href=#chat data-t=\"t2\">"),
        "t2 was read"
    );
    assert!(at("data-t=\"t4\"><i") < at("data-t=\"t2\"><i"), "newest first");
    assert!(chat.contains("<span class=btns><button type=button class=ntb>+ thread</button></span>"));
    // The thread: you, a tool row (closed), the conductor, an answered choice, an image, a file, a system line, the open choice.
    let t4 = &chat[at("<div class=cvp data-t=\"t4\">")..];
    let t4 = &t4[..t4.find("</form></div>").unwrap()];
    for want in [
        "<span class=ds><i class=dt></i>acme-api · claude · opus · awake</span>",
        "conductor in ",
        "<details class=\"blk me\" open><summary><span class=k>you</span>",
        "<code>/v1/search</code>",
        "<figure class=im><a href=\"thread/t4/IMG%5F2041%2EPNG\"><img src=\"thread/t4/IMG%5F2041%2EPNG\"",
        "<figcaption>IMG_2041.PNG · 218 KB</figcaption>",
        "<details class=\"blk run\"><summary><span class=k>$</span><span class=cmd>herdsman brief acme-api 19</span>",
        "<p class=ans>✓ 1. per API key <span class=m>· you · ",
        "<p class=ans>✓ other: \"600 a minute, burst 100\" <span class=m>· you · ",
        "<a class=tf href=\"thread/t4/429%2Dbody%2Ejson\"><span>▤ 429-body.json</span><span class=m>87 B</span></a>",
        "<a href=\"https://docs.github.com/rest/using-the-rest-api/rate-limits-for-the-rest-api\">",
        "<p class=\"qs note\">⇄ switched to account B",
        "<form class=chq data-id=\"q33\" data-fleet=\"acme-api\"><button type=button name=choice value=1><span>1. both:",
        "<small>(recommended)</small>",
        "placeholder=\"other: type an answer\"",
        "<p class=tlv><b>●</b> waiting for your answer",
        "<form class=tw data-t=\"t4\"><input type=file hidden>",
    ] {
        assert!(t4.contains(want), "{want} not in {t4}");
    }
    assert!(!t4.contains("DECISION q31"), "a DECISION line shows as the ✓ line only");
    assert!(!t4.contains("disabled"), "a paired owner device writes");
    // The needs-you stack keeps lane asks only; `[chat·N]` counts the threads that wait on a choice.
    assert!(needs.contains("Rate-limit the public search endpoint") && !needs.contains("X-RateLimit"));
    assert!(needs.contains("<i id=chn hidden data-n=1></i>"), "{needs}");
}

/// The conductor's tool rows come from the lane hook; a conductor's thread with `fleet` and conductor role.
fn setup(name: &str) -> herdsman::Ctx {
    let d = tmpdir(name);
    let c = ctx(
        &d,
        &format!("{CONFIG}\n[fleet.sendmeter.role.conductor]\nharness = \"claude\"\nmodel = \"opus\"\n"),
    );
    let profile = c.config.with_file_name("claude").join("fleet-conductor");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    c
}

/// herdr with the thread's conductor `conductor-t1` live (idle) in pane w9:p1.
fn live() -> Fake {
    Fake::default()
        .reply("herdr agent list", &agents(&[("conductor-t1", "idle", "/x", "w9:p1")]))
        .reply("herdr agent prompt", "")
        .reply("herdr pane send-keys", "")
        .reply("herdr pane run", "")
        .reply(
            "herdr workspace create",
            r#"{"result":{"root_pane":{"pane_id":"w9:p1"}}}"#,
        )
        .reply("herdr agent start", "")
        .reply("herdr agent wait", "")
}

#[test]
fn a_thread_starts_from_the_board_and_a_tapped_choice_types_its_decision_into_the_conductor() {
    let c = setup("chat-ask");
    let r = live();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    // [+ thread] → [start]: the thread, then its conductor with the title as the first message.
    let res = req(
        &s,
        "POST",
        "/thread",
        "op=new&repo=sendmeter&title=Grill+%2312",
        &cookie,
        T,
    );
    assert!(
        res.starts_with("HTTP/1.1 200 OK") && body(&res) == "t1 started\n",
        "{res}"
    );
    assert!(r.called("herdr workspace create"), "{:?}", r.calls.borrow());
    assert!(r
        .calls
        .borrow()
        .iter()
        .any(|c| c.starts_with("herdr agent start conductor-t1")));
    let ask = [
        "ask",
        "sendmeter",
        "-",
        "Which scope?",
        "--option",
        "small",
        "--option",
        "full",
        "--recommend",
        "2",
        "--thread",
        "t1",
    ];
    assert_eq!(run(&c, &r, &ask).1, "q1\n");
    let (chat, _) = frags(&s, &cookie, T + 5);
    assert!(
        chat.contains("<form class=chq data-id=\"q1\" data-fleet=\"sendmeter\"><button type=button name=choice value=2><span>2. full <small>(recommended)</small></span></button><button type=button name=choice value=1><span>1. small</span></button>"),
        "{chat}"
    );
    // The tap: the choice row's `answer` POST.
    let res = req(
        &s,
        "POST",
        "/thread",
        "op=answer&fleet=sendmeter&id=q1&choice=1",
        &cookie,
        T + 10,
    );
    assert_eq!(body(&res), "answered q1\n", "{res}");
    assert!(
        r.called("herdr agent prompt conductor-t1 DECISION q1: small"),
        "{:?}",
        r.calls.borrow()
    );
    let (chat, _) = frags(&s, &cookie, T + 20);
    assert!(!chat.contains("data-id=\"q1\""), "the rows fold");
    assert!(
        chat.contains("<p class=ans>✓ 1. small <span class=m>· you · "),
        "{chat}"
    );
    // The "other" box, on a second ask.
    let ask2 = ["ask", "sendmeter", "-", "How fast?", "--option", "a", "--thread", "t1"];
    assert_eq!(run(&c, &r, &ask2).1, "q2\n");
    let res = req(
        &s,
        "POST",
        "/thread",
        "op=answer&fleet=sendmeter&id=q2&text=600+a+minute",
        &cookie,
        T + 30,
    );
    assert_eq!(body(&res), "answered q2\n", "{res}");
    assert!(r.called("herdr agent prompt conductor-t1 DECISION q2: 600 a minute"));
    assert!(frags(&s, &cookie, T + 40).0.contains("✓ other: \"600 a minute\""));
}

#[test]
fn an_attached_image_reaches_the_conductor_and_its_reply_file_shows_inline() {
    let c = setup("chat-img");
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Look"]).1,
        "t1\n"
    );
    let r = live();
    assert_eq!(run(&c, &r, &["thread", "say", "t1", "hi"]).0, 0, "its conductor is up");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let png: Vec<u8> = (0..40_000u32).map(|i| (i % 251) as u8).collect();
    let data: String = url_enc(&B64.encode(&png));
    let res = req(
        &s,
        "POST",
        "/thread",
        &format!("op=say&t=t1&text=see+this&name=IMG_1.PNG&data={data}"),
        &cookie,
        T,
    );
    assert!(res.starts_with("HTTP/1.1 200 OK"), "{res}");
    let saved = c
        .state
        .with_file_name("conductor")
        .join("t1/uploads")
        .join(format!("{T}-IMG_1.PNG"));
    assert_eq!(std::fs::read(&saved).unwrap(), png, "in the conductor's folder");
    assert!(
        r.called(&format!(
            "herdr agent prompt conductor-t1 [t1] owner: see this (attached: {})",
            saved.display()
        )),
        "the conductor gets its path: {:?}",
        r.calls.borrow()
    );
    let got = s.respond(
        &format!("GET /thread/t1/IMG%5F1%2EPNG HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"),
        T,
    );
    let at = got.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    assert!(String::from_utf8_lossy(&got[..at]).contains("Content-Type: image/png"));
    assert_eq!(&got[at..], &png[..]);

    // Out: the conductor's reply with a mockup PNG.
    let mock = c.config.with_file_name("mockup-390.png");
    std::fs::write(&mock, b"\x89PNG mock").unwrap();
    let (code, out) = run(
        &c,
        &r,
        &["thread", "reply", "t1", "Here it is.", "--file", mock.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{out}");
    let (chat, _) = frags(&s, &cookie, T + 5);
    assert!(chat.contains("<img src=\"thread/t1/IMG%5F1%2EPNG\""), "{chat}");
    assert!(chat.contains("<img src=\"thread/t1/mockup%2D390%2Epng\""), "{chat}");
    let got = req(&s, "GET", "/thread/t1/mockup-390.png", "", "", T + 5);
    assert!(got.ends_with("\r\n\r\n\u{FFFD}PNG mock"), "{got}");
    // Only files a thread's message carries are served.
    let res = req(&s, "GET", "/thread/t1/fleet.toml", "", "", T + 5);
    assert!(res.starts_with("HTTP/1.1 404"), "{res}");
    let (code, out) = run(&c, &r, &["thread", "reply", "t1", "x", "--file", "/no/such.png"]);
    assert_ne!(code, 0, "{out}");
}

fn url_enc(s: &str) -> String {
    s.replace('+', "%2B").replace('/', "%2F").replace('=', "%3D")
}

#[test]
fn a_device_that_is_not_paired_reads_the_thread_but_cannot_write() {
    let c = setup("chat-guest");
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Grill"]).1,
        "t1\n"
    );
    let ask = [
        "ask",
        "sendmeter",
        "-",
        "Which?",
        "--option",
        "a",
        "--option",
        "b",
        "--thread",
        "t1",
    ];
    assert_eq!(run(&c, &Fake::default(), &ask).1, "q1\n");
    let r = live();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let (chat, _) = frags(&s, "", T);
    assert!(
        chat.contains("<p class=\"ns m\">pair this phone to answer or write</p>"),
        "{chat}"
    );
    assert!(
        chat.contains("<button type=button name=choice value=1 disabled>"),
        "{chat}"
    );
    assert!(
        chat.contains("<textarea name=text rows=1 aria-label=message placeholder=\"message the conductor\" disabled>")
    );
    assert!(
        !chat.contains("class=ntb") && !chat.contains("class=nt "),
        "no [+ thread]"
    );
    for b in [
        "op=say&t=t1&text=hi",
        "op=answer&fleet=sendmeter&id=q1&choice=1",
        "op=new&repo=sendmeter&title=x",
    ] {
        let res = req(&s, "POST", "/thread", b, "", T);
        assert!(res.starts_with("HTTP/1.1 401"), "{b}: {res}");
    }
    assert!(!r.called("herdr agent prompt"), "{:?}", r.calls.borrow());
}

/// A screenshot is far past the 32 KB other posts get: over the socket, `POST /thread` reads its whole body.
#[test]
fn a_large_attachment_arrives_whole_over_the_socket() {
    use std::io::{Read, Write};
    let c = setup("chat-big");
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Look"]).1,
        "t1\n"
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let cookie = pair(
        &Server::new(&c, &Fake::default(), Some(ADDR.into())),
        &c,
        "Mozilla/5.0 (iPhone)",
        now,
    );
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || herdsman::serve::serve_on(&c2, &live(), l, false));
    let png: Vec<u8> = (0..600_000u32).map(|i| (i % 253) as u8).collect();
    let body = format!("op=say&t=t1&text=big&name=big.png&data={}", url_enc(&B64.encode(&png)));
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    write!(
        s,
        "POST /thread HTTP/1.1\r\nHost: {addr}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut res = String::new();
    s.read_to_string(&mut res).unwrap();
    assert!(res.starts_with("HTTP/1.1 200 OK"), "{res}");
    let up = c.state.with_file_name("conductor").join("t1/uploads");
    let saved = std::fs::read_dir(&up).unwrap().next().unwrap().unwrap().path();
    assert_eq!(std::fs::read(saved).unwrap(), png);
}
