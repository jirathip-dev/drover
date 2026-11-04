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
        "<figure class=im><img src=\"thread/t4/IMG%5F2041%2EPNG\"",
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
    let folder = c.state.with_file_name("conductor").join("t1");
    let mock = folder.join("mockup-390.png");
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
    // A file outside the thread's folder is never attached, nor served when a log line names it.
    let toml = c.config.to_str().unwrap();
    let (code, out) = run(&c, &r, &["thread", "reply", "t1", "x", "--file", toml]);
    assert_ne!(code, 0, "{out}");
    assert!(
        !std::fs::read_to_string(&c.state).unwrap().contains(toml),
        "never logged"
    );
    std::os::unix::fs::symlink(&c.config, folder.join("link.toml")).unwrap();
    let line = |n: &str, p: &str| {
        format!("{{\"ts\":{T},\"fleet\":\"sendmeter\",\"type\":\"thread-msg\",\"thread\":\"t1\",\"from\":\"conductor\",\"text\":\"x\",\"files\":[{{\"name\":\"{n}\",\"path\":\"{p}\",\"size\":1}}]}}\n")
    };
    let mut log = std::fs::read_to_string(&c.state).unwrap();
    log += &line("keys.toml", toml);
    log += &line("link.toml", folder.join("link.toml").to_str().unwrap());
    std::fs::write(&c.state, log).unwrap();
    for n in ["keys.toml", "link.toml"] {
        let res = req(&s, "GET", &format!("/thread/t1/{n}"), "", "", T + 5);
        assert!(res.starts_with("HTTP/1.1 404"), "{n}: {res}");
    }
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

/// #406: sw.js under node: a navigation to a thread's file gets the file (the cache, then the network), never the
/// page; another navigation off the page (`/lane`, #67) still gets the page.
#[test]
fn the_service_worker_answers_a_thread_file_navigation_with_the_file() {
    if std::process::Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping: no node here");
        return;
    }
    let c = setup("chat-sw");
    let sw = req(
        &Server::new(&c, &Fake::default(), Some(ADDR.into())),
        "GET",
        "/sw.js",
        "",
        "",
        T,
    );
    let js = format!(
        "const L = {{}}, log = [];
globalThis.self = {{ registration: {{ scope: 'http://h/' }}, addEventListener: (k, f) => {{ L[k] = f; }} }};
globalThis.location = {{ origin: 'http://h' }};
globalThis.fetch = async r => {{ log.push('net ' + (r.url ?? r)); return new Response('body of ' + (r.url ?? r)); }};
globalThis.caches = {{ match: async k => {{ log.push('cache ' + (k.url ?? k)); }}, open: async () => ({{ put() {{}} }}) }};
{}
const nav = async url => {{
  log.length = 0;
  let p = null;
  L.fetch({{ request: {{ url, method: 'GET', mode: 'navigate' }}, respondWith: r => {{ p = r; }}, waitUntil() {{}} }});
  return [p && await (await p).text(), ...log];
}};
(async () => console.log(JSON.stringify([await nav('http://h/thread/t1/IMG%5F1%2EPNG'), await nav('http://h/lane')])))();",
        body(&sw)
    );
    let f = tmpdir("chat-sw-js").join("sw.js");
    std::fs::write(&f, js).unwrap();
    let o = std::process::Command::new("node").arg(&f).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let got: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let file = "http://h/thread/t1/IMG%5F1%2EPNG";
    assert_eq!(
        got[0],
        serde_json::json!([
            format!("body of {file}"),
            format!("cache {file}"),
            format!("net {file}")
        ]),
        "the file, never the page"
    );
    assert_eq!(got[1], serde_json::json!(["body of /", "cache /", "net /"]), "the page");
}

/// #406 in WebKit against a real `Server` that also answers `/deep/…` with the page (as the SW once did at a file's
/// URL): the page moves to `/` and loads the thread (from `frags`) with both images; a tap on one opens the viewer
/// (the URL stays), a swipe left shows the second, a double tap zooms, [×] and a swipe down close it, and the
/// thread's images are all still loaded.
#[test]
fn an_image_tap_opens_the_viewer_and_a_page_at_a_nested_path_still_loads_its_images() {
    if !(cfg!(target_os = "macos") && std::process::Command::new("swift").arg("--version").output().is_ok()) {
        eprintln!("skipping: no swift/WebKit here");
        return;
    }
    let c = setup("chat-viewer");
    let folder = c.state.with_file_name("conductor").join("t1");
    std::fs::create_dir_all(&folder).unwrap();
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    for f in ["icon-180.png", "icon-192.png"] {
        std::fs::copy(assets.join(f), folder.join(f)).unwrap();
    }
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Look"]).1,
        "t1\n"
    );
    let (a, b) = (folder.join("icon-180.png"), folder.join("icon-192.png"));
    let reply = [
        "thread",
        "reply",
        "t1",
        "Two shots.",
        "--file",
        a.to_str().unwrap(),
        "--file",
        b.to_str().unwrap(),
    ];
    let (code, out) = run(&c, &Fake::default(), &reply);
    assert_eq!(code, 0, "{out}");
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let (c2, at) = (c.clone(), addr.clone());
    std::thread::spawn(move || {
        let r = Fake::default();
        let s = Server::new(&c2, &r, Some(at));
        for mut st in l.incoming().flatten() {
            let mut buf = [0u8; 8192];
            let n = std::io::Read::read(&mut st, &mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).replacen("GET /deep/thread/t1/icon-180.png ", "GET / ", 1);
            let _ = std::io::Write::write_all(&mut st, &s.respond(&head, T));
        }
    });
    let js = "const ptr = (k, x, y) => IV.dispatchEvent(new PointerEvent(k, { pointerId: 1, clientX: x, clientY: y, bubbles: true })); \
        const swipe = (x0, y0, x1, y1) => { ptr('pointerdown', x0, y0); ptr('pointermove', x1, y1); ptr('pointerup', x1, y1); }; \
        const tap2 = () => { for (const k of [0, 1]) { ptr('pointerdown', 195, 400); ptr('pointerup', 195, 400); } }; \
        (function go() { const im = [...$$('#chat .cvp[data-t=\"t1\"] .im img')]; \
          if (im.length < 2 || !im.every(i => i.complete)) return setTimeout(go, 50); \
          const href = location.href, ok = () => im.filter(i => i.naturalWidth > 0).length, at = [im.length, ok(), +(location.pathname === '/')]; \
          im[0].click(); \
          at.push(+!IV.hidden, +(location.href === href), +(IVI.src === im[0].src), +($('.ivn').textContent === '1 / 2')); \
          swipe(300, 400, 100, 400); \
          at.push(+(IVI.src === im[1].src), +($('.ivn').textContent === '2 / 2')); \
          tap2(); \
          at.push(+IVI.style.transform.includes('scale(2.5)')); \
          $('.ivx').click(); \
          at.push(+IV.hidden); \
          im[1].click(); swipe(200, 300, 200, 500); \
          at.push(+IV.hidden, ok(), +(location.href === href), errs.length); \
          window.late = at; })()";
    let png = tmpdir("chat-viewer-png").join("thread.png");
    let o = swift()
        .env("HERDSMAN_WAIT", "3")
        .args([
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/webview.swift"),
            &format!("http://{addr}/deep/thread/t1/icon-180.png"),
            "390",
            "chat",
            "scene",
            png.to_str().unwrap(),
            &format!("chatOpen('t1'); {{ {js} }}"),
        ])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{out} {}", String::from_utf8_lossy(&o.stderr));
    let m: Vec<f64> = out.split_whitespace().skip(2).filter_map(|n| n.parse().ok()).collect();
    // images, loaded, at `/`; open, same URL, first, `1 / 2`; second, `2 / 2`; zoomed; [×] closed; swipe closed,
    // still loaded, same URL, page errors.
    assert_eq!(m, [2., 2., 1., 1., 1., 1., 1., 1., 1., 1., 1., 1., 2., 1., 0.], "{out}");
/// #403: a runner whose typing into an agent (`herdr agent prompt`) waits for the test's go, then reports the line.
struct Held(
    Fake,
    std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    std::sync::Mutex<std::sync::mpsc::Sender<String>>,
);
impl herdsman::runner::Runner for Held {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<herdsman::runner::Out> {
        let typing = prog == "herdr" && args.starts_with(&["agent".into(), "prompt".into()]);
        if typing {
            self.1.lock().unwrap().recv().unwrap();
        }
        let out = self.0.run(prog, args);
        if typing {
            self.2
                .lock()
                .unwrap()
                .send(format!("{prog} {}", args.join(" ")))
                .unwrap();
        }
        out
    }
}

/// #403: `say` answers as soon as the message is logged; typing it into the conductor runs after the reply.
#[test]
fn a_message_is_answered_once_logged_and_typed_into_the_conductor_after() {
    use std::io::{Read, Write};
    let c = setup("chat-async");
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Look"]).1,
        "t1\n"
    );
    assert_eq!(
        run(&c, &live(), &["thread", "say", "t1", "hi"]).0,
        0,
        "its conductor is up"
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
    let (go, held) = std::sync::mpsc::channel();
    let (typed, seen) = std::sync::mpsc::channel();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || {
        let r = Held(live(), held.into(), typed.into());
        herdsman::serve::serve_on(&c2, &r, l, false)
    });
    let body = "op=say&t=t1&text=a+long+message";
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    write!(
        s,
        "POST /thread HTTP/1.1\r\nHost: {addr}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    // Before #403 the reply waited for the typing, which waits for `go`: no reply.
    s.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    let mut res = String::new();
    s.read_to_string(&mut res).unwrap();
    assert!(
        res.starts_with("HTTP/1.1 200 OK") && res.ends_with("\r\n\r\nsent\n"),
        "{res}"
    );
    assert!(
        std::fs::read_to_string(&c.state)
            .unwrap()
            .contains("\"from\":\"owner\",\"text\":\"a long message\""),
        "logged"
    );
    assert!(
        seen.try_recv().is_err(),
        "the reply came while the conductor was still not typed into"
    );
    go.send(()).unwrap();
    let line = seen.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
    assert_eq!(line, "herdr agent prompt conductor-t1 [t1] owner: a long message");
}

/// #403: a delivery that fails is a red system line with [retry]; [retry] types the same line, logging no copy.
#[test]
fn a_failed_delivery_shows_red_with_retry_and_retry_types_it_again() {
    let c = setup("chat-retry");
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Look"]).1,
        "t1\n"
    );
    let down = Fake::default();
    let s = Server::new(&c, &down, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let res = req(&s, "POST", "/thread", "op=say&t=t1&text=hello", &cookie, T);
    assert_eq!(body(&res), "sent\n", "{res}");
    let (chat, _) = frags(&s, &cookie, T + 5);
    let at = chat
        .find("<p class=\"qs note r\">✗ not delivered: ")
        .unwrap_or_else(|| panic!("{chat}"));
    let line = &chat[at..at + chat[at..].find("</p>").unwrap()];
    let n = line.split("data-n=").nth(1).and_then(|x| x.split('>').next()).unwrap();
    assert!(
        line.ends_with(&format!(" <button type=button class=rty data-n={n}>retry</button>")),
        "{line}"
    );
    let r = live();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let res = req(&s, "POST", "/thread", &format!("op=retry&t=t1&n={n}"), &cookie, T + 10);
    assert_eq!(body(&res), "retrying\n", "{res}");
    assert!(r.called("herdr agent start conductor-t1"), "{:?}", r.calls.borrow());
    let log = std::fs::read_to_string(&c.state).unwrap();
    assert_eq!(log.matches("\"from\":\"owner\",\"text\":\"hello\"").count(), 1, "{log}");
    let res = req(&s, "POST", "/thread", "op=retry&t=t1&n=1", &cookie, T + 20);
    assert!(body(&res).contains("nothing to retry"), "{res}");
    // Mid-turn, the live line is the inline mark serve.js fills with the theme's loading art.
    let busy = Fake::default().reply(
        "herdr agent list",
        &agents(&[("conductor-t1", "working", "/x", "w9:p1")]),
    );
    let (chat, _) = frags(&Server::new(&c, &busy, Some(ADDR.into())), &cookie, T + 30);
    assert!(
        chat.contains("<p class=tlv><i class=tld aria-hidden=true></i> working</p>"),
        "{chat}"
    );
}
