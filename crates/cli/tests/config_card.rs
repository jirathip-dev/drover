mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;
const CARD: &str = "<form class=chc data-id=\"c1\"><button name=act value=apply>apply</button>";

fn setup(name: &str) -> drover::Ctx {
    let c = ctx(
        &tmpdir(name),
        &format!("{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\n"),
    );
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Models"]).1,
        "t1\n"
    );
    msgs(&c, T, 0..3);
    c
}

fn propose(c: &drover::Ctx) {
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

fn msgs(c: &drover::Ctx, ts: u64, ids: std::ops::Range<u64>) {
    for i in ids {
        let d = json!({"thread": "t1", "from": "orchestrator", "text": format!("m{i}")});
        append(&c.state, &Event::new(ts + i, "sendmeter", None, "thread-msg", d)).unwrap();
    }
}

fn chat(s: &Server, cookie: &str) -> (String, String) {
    let h = board_js(&[(&api(s, ADDR, cookie, T + 500, "chat"), "chat.tab(d)")]).remove(0);
    let at = h.find("<div class=cvp data-t=\"t1\">").unwrap();
    (h[at..].into(), h[..at].into())
}

fn page(s: &Server, cookie: &str, q: &str) -> String {
    let j = api(s, ADDR, cookie, T + 500, &format!("thread/t1?before={q}"));
    board_js(&[(&j, "chat.page(0, 't1', d, true)")]).remove(0)
}

#[test]
fn a_waiting_card_stays_on_the_first_page_and_a_stale_one_cannot_apply() {
    let c = setup("cfg-window");
    propose(&c);
    msgs(&c, T + 100, 0..40);
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
    let b = &view[view.find("data-b=").unwrap() + 7..];
    let older = page(&s, &cookie, &format!("{}&n=30", &b[..b.find('>').unwrap()]));
    assert!(older.contains(">m9</span>") && !older.contains("class=chc"), "{older}");

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

fn drain(s: &mut TcpStream, ms: u64) -> String {
    let (end, mut out, mut b) = (Instant::now() + Duration::from_millis(ms), vec![], vec![0u8; 65_536]);
    while Instant::now() < end {
        match s.read(&mut b) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&b[..n]),
            Err(_) => {}
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn serve(c: &drover::Ctx) -> (std::net::SocketAddr, FakeShed) {
    let sh = FakeShed::new();
    sh.attach(c);
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c2, &Fake::default(), l, false));
    (addr, sh)
}

#[test]
fn a_proposal_reaches_a_thread_open_on_the_phone_live() {
    let c = setup("cfg-live");
    let (addr, _sh) = serve(&c);
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
    while !chat(&got).is_some_and(|j| j.to_string().contains("\"text\":\"m39\""))
        && t0.elapsed() < Duration::from_secs(5)
    {
        got += &drain(&mut s, 100);
    }
    let j = chat(&got).unwrap_or_else(|| panic!("no chat data within 5 s: {got}"));
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

fn webkit(url: &str, cookie: &str, js: &str, wait: &str, png: &std::path::Path) -> Vec<f64> {
    let o = webview()
        .env("DROVER_WAIT", wait)
        .env("DROVER_COOKIE", cookie.trim().trim_start_matches("Cookie: "))
        .args([url, "390", "chat", "scene", png.to_str().unwrap(), js])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{out} {}", String::from_utf8_lossy(&o.stderr));
    out.split_whitespace().skip(2).filter_map(|n| n.parse().ok()).collect()
}

#[test]
fn the_card_lands_live_in_view_and_stays_reachable_in_webkit() {
    if no_webkit() {
        return;
    }
    let shots = std::env::var("DROVER_SNAPSHOTS").map_or_else(|_| tmpdir("cfg-png"), std::path::PathBuf::from);
    let now = drover::now();
    let c = setup("cfg-webkit");
    let cookie = pair(&Server::new(&c, &Fake::default(), Some(ADDR.into())), &c, "Safari", now);
    let (addr, _sh) = serve(&c);
    let url = format!("http://{addr}/");
    let c2 = c.clone();
    let later = std::thread::spawn(move || {
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

#[test]
fn applying_a_new_fleet_kicks_its_first_queued_issue() {
    let c = setup("cfg-add-fleet");
    let old = std::fs::read_to_string(&c.config).unwrap();
    let added = old.split("\n[limits]").next().unwrap().replace("sendmeter", "drover");
    let file = c.config.with_file_name("proposed.toml");
    std::fs::write(&file, format!("{old}\n{added}")).unwrap();
    let r = Fake::default();
    assert_eq!(
        run(&c, &r, &["config", "propose", "t1", file.to_str().unwrap()]).1,
        "c1\n"
    );
    for (f, n) in [("drover", 5), ("drover", 6), ("sendmeter", 9)] {
        let d = json!({"op": "add", "issues": [n]});
        append(&c.state, &Event::new(T, f, None, "queue", d)).unwrap();
    }
    let (code, out) = run(&c, &r, &["config", "apply", "c1"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.trim_end().ends_with(", drover #5 kicked"), "{out}");
    let calls = r.calls.borrow();
    let kicks: Vec<_> = calls.iter().filter(|c| c.contains(" kick ")).collect();
    assert_eq!(kicks.len(), 1, "{kicks:?}");
    assert!(
        kicks[0].contains(" kick drover 5 --why config-apply --now "),
        "{kicks:?}"
    );
}

#[test]
fn applying_a_new_role_syncs_its_profile_and_hook_approval() {
    let c = setup("cfg-sync");
    let yaml = c.homes["hermes"].join("fleet-verify/config.yaml");
    std::fs::create_dir_all(yaml.parent().unwrap()).unwrap();
    std::fs::write(&yaml, "model: x\n").unwrap();
    let old = std::fs::read_to_string(&c.config).unwrap();
    let file = c.config.with_file_name("proposed.toml");
    let role = "\n[fleet.sendmeter.role.verify]\nharness = \"hermes\"\nprofile = \"fleet-verify\"\n";
    std::fs::write(&file, format!("{old}{role}")).unwrap();
    let r = Fake::default();
    assert_eq!(
        run(&c, &r, &["config", "propose", "t1", file.to_str().unwrap()]).1,
        "c1\n"
    );
    let (code, out) = run(&c, &r, &["config", "apply", "c1"]);
    assert_eq!(code, 0, "{out}");
    let allow = std::fs::read_to_string(c.homes["hermes"].join("fleet-verify/shell-hooks-allowlist.json")).unwrap();
    assert!(allow.contains("hook --role rev"), "{allow}");
    assert!(std::fs::read_to_string(&yaml).unwrap().contains("hooks:"));
    let settings = drover::providers::profiles_root(&c).join("fleet-verify/settings.json");
    assert!(std::fs::read_to_string(settings).unwrap().contains("hook --role rev"));
}
