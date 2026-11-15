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

fn chat(s: &Server, cookie: &str) -> Value {
    serde_json::from_str(&api(s, ADDR, cookie, T + 500, "chat")).unwrap()
}

fn thread(chat: &Value) -> Value {
    let ts = chat["threads"].as_array().unwrap();
    ts.iter().find(|t| t["id"] == "t1").unwrap().clone()
}

fn page(s: &Server, cookie: &str, q: &str) -> Value {
    serde_json::from_str(&api(s, ADDR, cookie, T + 500, &format!("thread/t1?before={q}"))).unwrap()
}

fn configs(msgs: &Value) -> Vec<Value> {
    let all = msgs.as_array().unwrap().iter();
    all.filter(|m| m["kind"] == "config").cloned().collect()
}

#[test]
fn a_waiting_card_stays_on_the_first_page_and_a_stale_one_cannot_apply() {
    let c = setup("cfg-window");
    propose(&c);
    msgs(&c, T + 100, 0..40);
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T + 500);
    let all = chat(&s, &cookie);
    let t = thread(&all);
    let msgs = &t["page"]["msgs"];
    assert_eq!(
        (&msgs[0]["kind"], &msgs[0]["from"], &msgs[0]["open"]),
        (&json!("config"), &json!("c1"), &json!(true)),
        "kept first, above the window: {t}"
    );
    assert_eq!(msgs[1]["text"], "m10", "{t}");
    assert_eq!(msgs.as_array().unwrap().len(), 31, "the 30 and the card");
    assert_eq!(t["waiting"], json!(["c1"]), "on the thread's bar: {t}");
    let repo = all["repos"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["repo"] == "sendmeter")
        .unwrap();
    assert_eq!(repo["rows"][0]["waiting"], json!(["c1"]), "on its row: {repo}");
    let older = page(&s, &cookie, &format!("{}&n=30", t["page"]["older"]));
    assert!(
        older.to_string().contains("\"text\":\"m9\"") && configs(&older["msgs"]).is_empty(),
        "{older}"
    );

    let cur = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(&c.config, cur + "\n# hand edit\n").unwrap();
    let all = chat(&s, &cookie);
    let t = thread(&all);
    assert!(
        configs(&t["page"]["msgs"]).is_empty() && t["waiting"] == json!([]),
        "{t}"
    );
    let older = page(&s, &cookie, "999999&n=60");
    let cards = configs(&older["msgs"]);
    assert_eq!(cards.len(), 1, "{older}");
    assert_eq!(
        (&cards[0]["from"], &cards[0]["open"]),
        (&json!("c1"), &json!(false)),
        "stale: {older}"
    );
    let (code, out) = run(&c, &Fake::default(), &["config", "apply", "c1"]);
    assert!(code == 3 && out.contains("REFUSED STALE"), "{code} {out}");
    assert!(std::fs::read_to_string(&c.config).unwrap().ends_with("# hand edit\n"));
}

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

#[test]
fn a_proposal_over_16_kb_applies_byte_for_byte() {
    let c = setup("cfg-big");
    let old = std::fs::read_to_string(&c.config).unwrap();
    let pad: String = (0..400)
        .map(|i| format!("# padding line {i:04} of a long fleet.toml\n"))
        .collect();
    let new = format!("{}{pad}", old.replacen("model = \"opus\"", "model = \"sonnet\"", 1));
    assert!(new.len() > 16 << 10, "{}", new.len());
    let file = c.config.with_file_name("proposed.toml");
    std::fs::write(&file, &new).unwrap();
    let r = Fake::default();
    let (code, out) = run(&c, &r, &["config", "propose", "t1", file.to_str().unwrap()]);
    assert_eq!((code, out.as_str()), (0, "c1\n"));
    let (code, out) = run(&c, &r, &["config", "apply", "c1"]);
    assert_eq!(code, 0, "{out}");
    let got = std::fs::read_to_string(&c.config).unwrap();
    assert!(got == new, "applied {} bytes of {}", got.len(), new.len());
}

#[test]
fn a_proposal_stored_truncated_refuses_to_apply_and_asks_for_a_new_one() {
    let c = setup("cfg-cut");
    let old = std::fs::read_to_string(&c.config).unwrap();
    let base = drover::devices::sha256(old.as_bytes());
    let d =
        json!({"thread": "t1", "id": "c1", "diff": "+ x\n", "text": "[fleet… 6973 bytes cut …en A\"", "base": base});
    append(&c.state, &Event::new(T, "sendmeter", None, "config-propose", d)).unwrap();
    let (code, out) = run(&c, &Fake::default(), &["config", "apply", "c1"]);
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("REFUSED PROPOSAL_TRUNCATED") && out.contains("propose"),
        "{out}"
    );
    assert_eq!(std::fs::read_to_string(&c.config).unwrap(), old);
}
