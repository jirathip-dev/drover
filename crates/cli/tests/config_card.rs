//! #460: a config proposal's card in a thread: delivered live to a thread open on the phone, kept on the first page
//! past the 30-message window while it waits (`config cN waiting` on the thread's bar and row), and stale once
//! fleet.toml moves (no [apply]; `config apply` refuses it).
mod common;
use common::*;
use herdsman::events::{append, Event};
use herdsman::serve::Server;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;
const CARD: &str = "<form class=chc data-id=\"c1\"><button name=act value=apply>apply</button>";

/// Thread t1 with a conductor role; `config propose` of an impl model change gives `c1`.
fn setup(name: &str) -> herdsman::Ctx {
    let c = ctx(
        &tmpdir(name),
        &format!("{CONFIG}\n[fleet.sendmeter.role.conductor]\nharness = \"claude\"\nmodel = \"opus\"\n"),
    );
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Models"]).1,
        "t1\n"
    );
    msgs(&c, T, 0..3);
    c
}

fn propose(c: &herdsman::Ctx) {
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

/// Conductor messages `m<i>` in t1.
fn msgs(c: &herdsman::Ctx, ts: u64, ids: std::ops::Range<u64>) {
    for i in ids {
        let d = json!({"thread": "t1", "from": "conductor", "text": format!("m{i}")});
        append(&c.state, &Event::new(ts + i, "sendmeter", None, "thread-msg", d)).unwrap();
    }
}

/// The [chat] tab as the board draws it (#269) from API v1's `chat`: the t1 view and the list.
fn chat(s: &Server, cookie: &str) -> (String, String) {
    let h = board_js(&[(&api(s, ADDR, cookie, T + 500, "chat"), "chat.tab(d)")]).remove(0);
    let at = h.find("<div class=cvp data-t=\"t1\">").unwrap();
    (h[at..].into(), h[..at].into())
}

/// API v1's page of t1 (`q`: `before=`'s value and more), as the board draws it.
fn page(s: &Server, cookie: &str, q: &str) -> String {
    let j = api(s, ADDR, cookie, T + 500, &format!("thread/t1?before={q}"));
    board_js(&[(&j, "chat.page(0, 't1', d, true)")]).remove(0)
}

#[test]
fn a_waiting_card_stays_on_the_first_page_and_a_stale_one_cannot_apply() {
    let c = setup("cfg-window");
    propose(&c);
    msgs(&c, T + 100, 0..40); // 40 messages after it: the card is past the last 30
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T + 500);
    let (view, list) = chat(&s, &cookie);
    assert_eq!(view.matches(CARD).count(), 1, "{view}");
    let (marker, card, first) = (
        view.find("class=\"qs tmo\"").unwrap(),
        view.find(CARD).unwrap(),
        view.find(">m10</span>").unwrap(),
    );
    assert!(
        marker < card && card < first,
        "kept right under the marker, above the window: {view}"
    );
    assert_eq!(view.matches("<details data-n=").count(), 31, "the 30 and the card");
    assert!(
        view.contains("<b class=\"a cw\">config c1 waiting</b>"),
        "on the thread's bar: {view}"
    );
    assert!(
        list.contains("<b class=\"a cw\">config c1 waiting</b>"),
        "on its row: {list}"
    );
    // The older page leaves it out (the first page has it).
    let b = &view[view.find("data-b=").unwrap() + 7..];
    let older = page(&s, &cookie, &format!("{}&n=30", &b[..b.find('>').unwrap()]));
    assert!(older.contains(">m9</span>") && !older.contains("class=chc"), "{older}");

    // fleet.toml moves (another proposal applied, a hand edit): stale, no [apply], not pinned, not waiting.
    let cur = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(&c.config, cur + "\n# hand edit\n").unwrap();
    let (view, list) = chat(&s, &cookie);
    assert!(
        !view.contains("class=chc") && !view.contains("waiting") && !list.contains("waiting"),
        "{view}"
    );
    let older = page(&s, &cookie, "999999&n=60");
    assert!(
        older.contains("<p class=qid>config c1</p>")
            && older.contains("</pre></div><p class=m>stale: fleet.toml changed since</p></details>")
            && !older.contains("class=chc"),
        "{older}"
    );
    let (code, out) = run(&c, &Fake::default(), &["config", "apply", "c1"]);
    assert!(code == 3 && out.contains("REFUSED STALE"), "{code} {out}");
    assert!(std::fs::read_to_string(&c.config).unwrap().ends_with("# hand edit\n"));
}

/// What `s` sends within `ms`.
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

/// A real `serve` on a socket; its address.
fn serve(c: &herdsman::Ctx) -> std::net::SocketAddr {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || {
        herdsman::serve::serve_on(&c2, &Fake::default().reply("herdr agent list", NO_AGENTS), l, false)
    });
    addr
}

/// The `/live` stream of a phone with t1 open: a proposal (with a burst of 40 messages after it, in the same second)
/// arrives as the [chat] data (#269) carrying the card and the waiting marker.
#[test]
fn a_proposal_reaches_a_thread_open_on_the_phone_live() {
    let c = setup("cfg-live");
    let addr = serve(&c);
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(format!("GET /live?tab=chat&thread=t1 HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
        .unwrap();
    s.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    assert!(drain(&mut s, 1_500).starts_with("HTTP/1.1 200 OK"));
    propose(&c);
    msgs(&c, T + 100, 0..40);
    let chat = |got: &str| {
        let parts = got.split("event: chat\ndata: ").skip(1);
        parts
            .filter_map(|l| serde_json::from_str::<Value>(l.lines().next()?).ok())
            .last()
    };
    let (t0, mut got) = (Instant::now(), String::new());
    while chat(&got).is_none() && t0.elapsed() < Duration::from_secs(3) {
        got += &drain(&mut s, 100);
    }
    let j = chat(&got).unwrap_or_else(|| panic!("no chat data within 3 s: {got}"));
    let t = j["threads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "t1")
        .unwrap();
    let msgs = t["page"]["msgs"].as_array().unwrap();
    let card = (msgs[0]["kind"].as_str(), msgs[0]["from"].as_str(), &msgs[0]["open"]);
    assert_eq!(
        card,
        (Some("config"), Some("c1"), &json!(true)),
        "first, past the window: {t}"
    );
    assert_eq!(msgs.last().unwrap()["text"], "m39", "{t}");
    assert_eq!(t["waiting"], json!(["c1"]), "{t}");
    let repo = j["repos"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["repo"] == "sendmeter")
        .unwrap();
    assert_eq!(repo["rows"][0]["waiting"], json!(["c1"]), "{repo}");
}

/// The page at a phone's width in WebKit on a real `serve`, `js` run on the chat tab; `window.late` after `wait` s.
fn webkit(url: &str, cookie: &str, js: &str, wait: &str, png: &std::path::Path) -> Vec<f64> {
    let o = swift()
        .env("HERDSMAN_WAIT", wait)
        .env("HERDSMAN_COOKIE", cookie.trim().trim_start_matches("Cookie: "))
        .args([
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/webview.swift"),
            url,
            "390",
            "chat",
            "scene",
            png.to_str().unwrap(),
            js,
        ])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{out} {}", String::from_utf8_lossy(&o.stderr));
    out.split_whitespace().skip(2).filter_map(|n| n.parse().ok()).collect()
}

/// The same in WebKit at 390 px: the card lands in the open thread without a reload, at its end in view; 40 messages
/// after it, it is still there once, and a tap on `config c1 waiting` brings it into view. Then the row's marker in the
/// list, and the card once stale. (HERDSMAN_SNAPSHOTS keeps the three shots.)
#[test]
fn the_card_lands_live_in_view_and_stays_reachable_in_webkit() {
    if std::process::Command::new("swift").arg("--version").output().is_err() || !cfg!(target_os = "macos") {
        eprintln!("skipping: no swift/WebKit here");
        return;
    }
    let shots = std::env::var("HERDSMAN_SNAPSHOTS").map_or_else(|_| tmpdir("cfg-png"), std::path::PathBuf::from);
    let now = herdsman::now();
    let c = setup("cfg-webkit");
    let cookie = pair(&Server::new(&c, &Fake::default(), Some(ADDR.into())), &c, "Safari", now);
    let url = format!("http://{}/", serve(&c));
    let c2 = c.clone();
    let later = std::thread::spawn(move || {
        // Once the page's stream watches t1 (serve stamps it each second).
        let w = c2.state.with_file_name("watch-t1.at");
        while !w.exists() {
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_millis(1_000));
        propose(&c2);
        std::thread::sleep(Duration::from_millis(1_500));
        msgs(&c2, now, 0..40);
    });
    let js = "chatOpen('t1'); const cards = () => $$('#chat .cvp[data-t=\"t1\"] .chc[data-id=\"c1\"]'), late = window.late = []; \
        const seen = e => { const r = e.closest('.blk').getBoundingClientRect(); return +(r.top >= 0 && r.bottom <= innerHeight); }; \
        const until = f => new Promise(r => (function go() { f() ? r() : setTimeout(go, 50); })()); \
        (async () => { await until(() => cards().length); await new Promise(r => setTimeout(r, 300)); late.push(seen(cards()[0]), $$('#chat .cw').length); \
          await until(() => $('#chat .cvp[data-t=\"t1\"] .tc').textContent.includes('m39')); await new Promise(r => setTimeout(r, 300)); \
          late.push(cards().length, seen(cards()[0])); $('#chat .cvp[data-t=\"t1\"] .th .cw').click(); await new Promise(r => setTimeout(r, 300)); \
          late.push(seen(cards()[0]), errs.length); })()";
    let m = webkit(&url, &cookie, js, "12", &shots.join("config-live.png"));
    later.join().unwrap();
    // In view on arrival, marked waiting on its bar and row; once after the burst, scrolled away (the thread follows its
    // end); the tap on the marker: in view; no errors.
    assert_eq!(
        m,
        [1., 2., 1., 0., 1., 0.],
        "arrived in view, markers, count, after the burst in view, after the tap, errors"
    );

    let js = "chatOpen(''); setTimeout(() => { window.late = [$$('#chat .tr[data-t=\"t1\"] .cw').length]; }, 1000)";
    assert_eq!(
        webkit(&url, &cookie, js, "1.5", &shots.join("config-waiting.png")),
        [1.]
    );

    let cur = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(&c.config, cur + "\n# hand edit\n").unwrap();
    let js = "chatOpen('t1'); setTimeout(() => $('#chat .cvp[data-t=\"t1\"] .tmo').click(), 800); setTimeout(() => { const s = [...$$('#chat .cvp[data-t=\"t1\"] .tc p.m')].find(p => p.textContent.startsWith('stale')); \
        s && s.closest('.blk').scrollIntoView({ block: 'center' }); window.late = [+!!s, $$('#chat .chc').length]; }, 1800)";
    assert_eq!(
        webkit(&url, &cookie, js, "2.5", &shots.join("config-stale.png")),
        [1., 0.]
    );
}
