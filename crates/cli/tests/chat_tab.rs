mod common;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use common::*;
use drover::serve::Server;
use serde_json::{json, Value};

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

fn chat(s: &Server, cookie: &str, now: u64) -> Value {
    serde_json::from_str(&api(s, ADDR, cookie, now, "chat")).unwrap()
}

fn thread<'a>(chat: &'a Value, t: &str) -> &'a Value {
    let all = chat["threads"].as_array().unwrap();
    all.iter()
        .find(|x| x["id"] == t)
        .unwrap_or_else(|| panic!("no {t}: {chat}"))
}

fn msgs<'a>(chat: &'a Value, t: &str) -> Vec<&'a Value> {
    thread(chat, t)["page"]["msgs"].as_array().unwrap().iter().collect()
}

fn files(chat: &Value, t: &str) -> Vec<String> {
    let all = msgs(chat, t).into_iter().flat_map(|m| m["files"].as_array().unwrap());
    all.map(|f| f["name"].as_str().unwrap().to_string()).collect()
}

fn setup(name: &str) -> drover::Ctx {
    let d = tmpdir(name);
    let c = ctx(
        &d,
        &format!("{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\n"),
    );
    let profile = c.config.with_file_name("claude").join("fleet-orchestrator");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    c
}

fn acp_setup(name: &str, b: &FakeShed) -> drover::Ctx {
    let cfg = format!(
        "{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\naccount = \"C\"\n\n\
         [fleet.sendmeter.runtime]\nkind = \"shed-acp\"\nsocket = \"{}\"\n\n[accounts]\nC = \"C\"\n",
        b.sock.display()
    );
    let mut c = ctx(&tmpdir(name), &cfg);
    c.exe = env!("CARGO_BIN_EXE_drover").into();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    c
}

fn acp() -> Fake {
    let agents =
        serde_json::json!([{"id": "claude-acp", "version": "0.101.0", "distribution": "npx", "cmd": ["echo"]}]);
    Fake::default()
        .reply("shed help run", "--meta <JSON>")
        .reply("shed agents --json", &agents.to_string())
        .reply("shed run", "")
        .reply("shed send", "")
}

const START_T1: &str = "shed run --acp claude-acp --policy allow --name orchestrator-t1";

fn leaked() -> &'static FakeShed {
    Box::leak(Box::new(FakeShed::new()))
}

#[test]
fn a_thread_starts_from_the_board_and_a_tapped_choice_types_its_decision_into_the_conductor() {
    let b = FakeShed::new();
    let c = acp_setup("chat-ask", &b);
    let f = acp();
    let cli = b.cli(&f);
    let r = b.on(&cli);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let res = req(
        &s,
        "POST",
        "/thread",
        "op=new&repo=sendmeter&text=Grill+%2312",
        &cookie,
        T,
    );
    assert!(
        res.starts_with("HTTP/1.1 200 OK") && body(&res) == "t1 started\n",
        "{res}"
    );
    assert!(f.called(START_T1), "{:?}", f.calls.borrow());
    assert!(b.has("orchestrator-t1"));
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
    let page = chat(&s, &cookie, T + 5);
    let ask = msgs(&page, "t1")
        .into_iter()
        .find(|m| m["kind"] == "ask")
        .unwrap_or_else(|| panic!("{page}"));
    assert_eq!(
        (
            &ask["ask"]["id"],
            &ask["ask"]["options"],
            &ask["ask"]["recommend"],
            &ask["open"]
        ),
        (&json!("q1"), &json!(["small", "full"]), &json!(2), &json!(true)),
        "{page}"
    );
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
        f.called("shed send orchestrator-t1 DECISION q1: small"),
        "{:?}",
        f.calls.borrow()
    );
    let page = chat(&s, &cookie, T + 20);
    let ask = msgs(&page, "t1").into_iter().find(|m| m["kind"] == "ask").unwrap();
    assert_eq!(
        (&ask["open"], &ask["answer"]["n"], &ask["answer"]["choice"]),
        (&json!(false), &json!(1), &json!("small")),
        "{page}"
    );
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
    assert!(f.called("shed send orchestrator-t1 DECISION q2: 600 a minute"));
    let page = chat(&s, &cookie, T + 40);
    let q2 = msgs(&page, "t1")
        .into_iter()
        .find(|m| m["kind"] == "ask" && m["ask"]["id"] == "q2")
        .unwrap();
    assert_eq!(
        (&q2["answer"]["n"], &q2["answer"]["choice"]),
        (&Value::Null, &json!("600 a minute")),
        "{page}"
    );
}

#[test]
fn an_attached_image_reaches_the_conductor_and_its_reply_file_shows_inline() {
    let b = FakeShed::new();
    let c = acp_setup("chat-img", &b);
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Look"]).1,
        "t1\n"
    );
    let f = acp();
    let cli = b.cli(&f);
    assert_eq!(
        run(&c, &cli, &["thread", "say", "t1", "hi"]).0,
        0,
        "its conductor is up"
    );
    let r = b.on(&cli);
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
        .with_file_name("orchestrator")
        .join("t1/uploads")
        .join(format!("{T}-IMG_1.PNG"));
    assert_eq!(std::fs::read(&saved).unwrap(), png, "in the conductor's folder");
    assert!(
        f.called(&format!(
            "shed send orchestrator-t1 [t1] owner: see this (attached: {})",
            saved.display()
        )),
        "the conductor gets its path: {:?}",
        f.calls.borrow()
    );
    let got = s.respond(
        &format!("GET /thread/t1/IMG%5F1%2EPNG HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"),
        T,
    );
    let at = got.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    assert!(String::from_utf8_lossy(&got[..at]).contains("Content-Type: image/png"));
    assert_eq!(&got[at..], &png[..]);

    let folder = c.state.with_file_name("orchestrator").join("t1");
    let mock = folder.join("mockup-390.png");
    std::fs::write(&mock, b"\x89PNG mock").unwrap();
    let (code, out) = run(
        &c,
        &r,
        &["thread", "reply", "t1", "Here it is.", "--file", mock.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{out}");
    let got = files(&chat(&s, &cookie, T + 5), "t1");
    assert!(
        got.contains(&"IMG_1.PNG".to_string()) && got.contains(&"mockup-390.png".to_string()),
        "{got:?}"
    );
    let got = req(&s, "GET", "/thread/t1/mockup-390.png", "", "", T + 5);
    assert!(got.ends_with("\r\n\r\n\u{FFFD}PNG mock"), "{got}");
    let res = req(&s, "GET", "/thread/t1/fleet.toml", "", "", T + 5);
    assert!(res.starts_with("HTTP/1.1 404"), "{res}");
    let (code, out) = run(&c, &r, &["thread", "reply", "t1", "x", "--file", "/no/such.png"]);
    assert_ne!(code, 0, "{out}");
    let toml = c.config.to_str().unwrap();
    let (code, out) = run(&c, &r, &["thread", "reply", "t1", "x", "--file", toml]);
    assert_ne!(code, 0, "{out}");
    assert!(!drover::events::jsonl(&c.state).unwrap().contains(toml), "never logged");
    std::os::unix::fs::symlink(&c.config, folder.join("link.toml")).unwrap();
    let line = |n: &str, p: &str| {
        format!("{{\"ts\":{T},\"fleet\":\"sendmeter\",\"type\":\"thread-msg\",\"thread\":\"t1\",\"from\":\"conductor\",\"text\":\"x\",\"files\":[{{\"name\":\"{n}\",\"path\":\"{p}\",\"size\":1}}]}}\n")
    };
    let mut log = drover::events::jsonl(&c.state).unwrap();
    log += &line("keys.toml", toml);
    log += &line("link.toml", folder.join("link.toml").to_str().unwrap());
    rewrite_log(&c.state, &log);
    for n in ["keys.toml", "link.toml"] {
        let res = req(&s, "GET", &format!("/thread/t1/{n}"), "", "", T + 5);
        assert!(res.starts_with("HTTP/1.1 404"), "{n}: {res}");
    }
}

#[test]
fn a_reply_file_from_a_lane_worktree_is_copied_into_the_thread_folder() {
    let c = setup("chat-wt-file");
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Look"]).1,
        "t1\n"
    );
    let design = c.worktrees.join("sendmeter/issue-12/docs/design/inbox");
    std::fs::create_dir_all(&design).unwrap();
    let png = design.join("inbox-390.png");
    std::fs::write(&png, b"\x89PNG design").unwrap();
    let r = Fake::default();
    let (code, out) = run(
        &c,
        &r,
        &["thread", "reply", "t1", "The mockup.", "--file", png.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{out}");
    let folder = c.state.with_file_name("orchestrator").join("t1");
    let log = drover::events::jsonl(&c.state).unwrap();
    let msg = log.lines().rfind(|l| l.contains("The mockup.")).unwrap();
    let e: serde_json::Value = serde_json::from_str(msg).unwrap();
    let f = &e["files"][0];
    assert_eq!(f["name"], "inbox-390.png", "{msg}");
    let copy = std::path::PathBuf::from(f["path"].as_str().unwrap());
    assert!(copy.starts_with(folder.canonicalize().unwrap()), "{msg}");
    assert_eq!(std::fs::read(&copy).unwrap(), b"\x89PNG design");

    let away = c.worktrees.parent().unwrap().join("elsewhere");
    for p in [away.join("x.png"), c.worktrees.join("other/issue-1/x.png")] {
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(&p, b"x").unwrap();
        let (code, out) = run(&c, &r, &["thread", "reply", "t1", "x", "--file", p.to_str().unwrap()]);
        assert_ne!(code, 0, "{}: {out}", p.display());
        assert!(!drover::events::jsonl(&c.state).unwrap().contains(p.to_str().unwrap()));
    }
}

fn url_enc(s: &str) -> String {
    s.replace('+', "%2B").replace('/', "%2F").replace('=', "%3D")
}

#[test]
fn a_device_that_is_not_paired_reads_the_thread_but_cannot_write() {
    let b = FakeShed::new();
    let c = acp_setup("chat-guest", &b);
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
    let f = acp();
    let cli = b.cli(&f);
    let r = b.on(&cli);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let page = chat(&s, "", T);
    assert_eq!(page["rw"], json!(false), "an unpaired device reads, no forms: {page}");
    for b in [
        "op=say&t=t1&text=hi",
        "op=answer&fleet=sendmeter&id=q1&choice=1",
        "op=new&repo=sendmeter&title=x",
    ] {
        let res = req(&s, "POST", "/thread", b, "", T);
        assert!(res.starts_with("HTTP/1.1 401"), "{b}: {res}");
    }
    assert!(
        !f.called("shed send") && !f.called("shed run"),
        "{:?}",
        f.calls.borrow()
    );
}

#[test]
fn a_large_attachment_arrives_whole_over_the_socket() {
    use std::io::{Read, Write};
    let b = leaked();
    let c = acp_setup("chat-big", b);
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
    std::thread::spawn(move || {
        let f = acp();
        drover::serve::serve_on(&c2, &b.cli(&f), l, false)
    });
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
    let up = c.state.with_file_name("orchestrator").join("t1/uploads");
    let saved = std::fs::read_dir(&up).unwrap().next().unwrap().unwrap().path();
    assert_eq!(std::fs::read(saved).unwrap(), png);
}

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

struct Held<'a>(
    common::fake_shed::Cli<'a>,
    std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    std::sync::Mutex<std::sync::mpsc::Sender<String>>,
);
impl drover::runner::Runner for Held<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        let typing = prog == "shed" && args[0] == "send";
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

#[test]
fn a_message_is_answered_once_logged_and_typed_into_the_conductor_after() {
    use std::io::{Read, Write};
    let b = leaked();
    let c = acp_setup("chat-async", b);
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Look"]).1,
        "t1\n"
    );
    assert_eq!(
        run(&c, &b.cli(&acp()), &["thread", "say", "t1", "hi"]).0,
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
        let f = acp();
        let r = Held(b.cli(&f), held.into(), typed.into());
        drover::serve::serve_on(&c2, &r, l, false)
    });
    let body = "op=say&t=t1&text=a+long+message";
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    write!(
        s,
        "POST /thread HTTP/1.1\r\nHost: {addr}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
    let mut res = String::new();
    s.read_to_string(&mut res).unwrap();
    assert!(
        res.starts_with("HTTP/1.1 200 OK") && res.ends_with("\r\n\r\nsent\n"),
        "{res}"
    );
    assert!(
        drover::events::jsonl(&c.state)
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
    assert_eq!(line, "shed send orchestrator-t1 [t1] owner: a long message");
}

#[test]
fn a_failed_delivery_shows_red_with_retry_and_retry_types_it_again() {
    let b = FakeShed::new();
    let c = acp_setup("chat-retry", &b);
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Look"]).1,
        "t1\n"
    );
    let down = Fake::default();
    let on = b.on(&down);
    let s = Server::new(&c, &on, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let res = req(&s, "POST", "/thread", "op=say&t=t1&text=hello", &cookie, T);
    assert_eq!(body(&res), "sent\n", "{res}");
    let page = chat(&s, &cookie, T + 5);
    let failed = msgs(&page, "t1")
        .into_iter()
        .find(|m| m["retry"] == true)
        .unwrap_or_else(|| panic!("{page}"));
    assert!(
        failed["text"].as_str().unwrap().starts_with("✗ not delivered: "),
        "{failed}"
    );
    let n = failed["seq"].to_string();
    let f = acp();
    let cli = b.cli(&f);
    let r = b.on(&cli);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let res = req(&s, "POST", "/thread", &format!("op=retry&t=t1&n={n}"), &cookie, T + 10);
    assert_eq!(body(&res), "retrying\n", "{res}");
    assert!(f.called(START_T1), "{:?}", f.calls.borrow());
    assert!(
        f.called("shed send orchestrator-t1 ") && f.calls.borrow().iter().any(|l| l.ends_with("[t1] owner: hello"))
    );
    let log = drover::events::jsonl(&c.state).unwrap();
    assert_eq!(log.matches("\"from\":\"owner\",\"text\":\"hello\"").count(), 1, "{log}");
    let res = req(&s, "POST", "/thread", "op=retry&t=t1&n=1", &cookie, T + 20);
    assert!(body(&res).contains("nothing to retry"), "{res}");
    b.set("orchestrator-t1", |t| {
        t["reported"] = serde_json::json!({"state": "working"})
    });
    let none = Fake::default();
    let busy = b.on(&none);
    let page = chat(&Server::new(&c, &busy, Some(ADDR.into())), &cookie, T + 30);
    let t1 = thread(&page, "t1");
    assert_eq!((&t1["state"], &t1["since"]), (&json!("working"), &json!(T)), "{t1}");
}

fn long(name: &str, n: u64, now: u64) -> (drover::Ctx, String) {
    fill(setup(name), n, now)
}

fn fill(c: drover::Ctx, n: u64, now: u64) -> (drover::Ctx, String) {
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Long"]).1,
        "t1\n"
    );
    for i in 1..=n {
        let d = serde_json::json!({"thread": "t1", "from": "conductor", "text": format!("m{i}")});
        let e = drover::events::Event::new(T + i, "sendmeter", None, "thread-msg", d);
        drover::events::append(&c.state, &e).unwrap();
    }
    let cookie = pair(
        &Server::new(&c, &Fake::default(), Some(ADDR.into())),
        &c,
        "Mozilla/5.0 (iPhone)",
        now,
    );
    (c, cookie)
}

#[test]
fn a_long_thread_carries_its_last_30_messages_and_older_ones_come_30_at_a_time() {
    let (c, cookie) = long("chat-window", 100, T);
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let texts = |p: &Value| -> Vec<String> {
        let all = p["msgs"].as_array().unwrap().iter().filter(|m| m["kind"] == "msg");
        all.map(|m| m["text"].as_str().unwrap().to_string()).collect()
    };
    let want = |a: u64, b: u64| (a..=b).map(|i| format!("m{i}")).collect::<Vec<_>>();
    let all = chat(&s, &cookie, T + 200);
    let view = &thread(&all, "t1")["page"];
    assert_eq!(texts(view), want(71, 100));
    assert!(
        view["msgs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|m| m["seq"].as_u64().unwrap() > 0),
        "each message names its event"
    );
    let page = |b: &Value| -> Value {
        serde_json::from_str(&api(&s, ADDR, &cookie, T + 200, &format!("thread/t1?before={b}&n=30"))).unwrap()
    };
    let p1 = page(&view["older"]);
    assert_eq!(texts(&p1), want(41, 70));
    let p2 = page(&p1["older"]);
    assert_eq!(texts(&p2), want(11, 40));
    let p3 = page(&p2["older"]);
    assert_eq!(texts(&p3), want(1, 10));
    assert!(p3["older"].is_null() && p3["started"].as_u64().unwrap() > 0, "{p3}");
    let no = req(&s, "GET", "/api/v1/thread/t9?before=5", "", &cookie, T + 200);
    assert!(no.starts_with("HTTP/1.1 404"), "{no}");
}

fn log(c: &drover::Ctx) -> String {
    drover::events::jsonl(&c.state).unwrap_or_default()
}

#[test]
fn a_first_message_starts_the_thread_titled_by_its_first_line_and_thread_title_retitles_it() {
    let b = FakeShed::new();
    let c = acp_setup("chat-first", &b);
    let f = acp();
    let cli = b.cli(&f);
    let r = b.on(&cli);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let page = chat(&s, &cookie, T);
    let repo = page["repos"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["repo"] == "sendmeter")
        .unwrap();
    assert_eq!(
        (&repo["rows"], &page["rw"]),
        (&json!([]), &json!(true)),
        "an empty thread to write in: {page}"
    );
    assert!(
        !log(&c).contains("thread-"),
        "an empty thread is never logged: {}",
        log(&c)
    );
    let res = req(
        &s,
        "POST",
        "/thread",
        "op=new&repo=sendmeter&text=Grill+me+on+%2312%0Athe+land+mode+part",
        &cookie,
        T,
    );
    assert_eq!(body(&res), "t1 started\n", "{res}");
    let events = drover::events::read(&c.state).unwrap();
    let new = events.iter().find(|e| e.kind == "thread-new").unwrap();
    assert_eq!(new.str("title"), Some("Grill me on #12"));
    let msgs: Vec<_> = events.iter().filter(|e| e.kind == "thread-msg").collect();
    assert_eq!(msgs.len(), 1, "{}", log(&c));
    assert_eq!(msgs[0].str("text"), Some("Grill me on #12\nthe land mode part"));
    let md = c.state.with_file_name("orchestrator").join("t1/.thread.md");
    assert_eq!(
        std::fs::read_to_string(md).unwrap(),
        "owner: Grill me on #12\nthe land mode part\n",
        "the conductor gets the message only"
    );
    assert!(f.called(START_T1), "{:?}", f.calls.borrow());
    let (code, out) = run(&c, &r, &["thread", "title", "t1", "Land mode for #12"]);
    assert_eq!(code, 0, "{out}");
    let t = drover::events::read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .rfind(|e| e.kind == "thread-title")
        .unwrap();
    assert_eq!(
        (t.str("thread"), t.str("title")),
        (Some("t1"), Some("Land mode for #12"))
    );
    assert!(run(&c, &r, &["thread", "show", "t1"])
        .1
        .starts_with("t1 sendmeter: Land mode for #12\n"));
    let page = chat(&s, &cookie, T + 5);
    let row = &page["repos"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["repo"] == "sendmeter")
        .unwrap()["rows"][0];
    assert_eq!(
        (&row["id"], &row["title"]),
        (&json!("t1"), &json!("Land mode for #12")),
        "{page}"
    );
    assert_eq!(thread(&page, "t1")["title"], "Land mode for #12");
    for bad in ["two\nlines", " "] {
        assert_ne!(run(&c, &r, &["thread", "title", "t1", bad]).0, 0, "{bad:?}");
    }
    let long = "word ".repeat(30);
    let res = req(
        &s,
        "POST",
        "/thread",
        &format!("op=new&repo=sendmeter&text={}", long.replace(' ', "+")),
        &cookie,
        T + 9,
    );
    assert_eq!(body(&res), "t2 started\n", "{res}");
    let t2 = drover::events::read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .rfind(|e| e.kind == "thread-new")
        .unwrap();
    assert_eq!(t2.str("title").unwrap(), format!("{}…", &long[..60]));
}

#[test]
fn archive_stops_the_conductor_and_folds_the_thread_and_restore_resumes_it() {
    let b = FakeShed::new();
    let c = acp_setup("chat-archive", &b);
    let f = acp();
    let cli = b.cli(&f);
    let r = b.on(&cli);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let png = url_enc(&B64.encode(b"\x89PNG one"));
    let res = req(
        &s,
        "POST",
        "/thread",
        &format!("op=new&repo=sendmeter&text=hi&name=a.png&data={png}"),
        &cookie,
        T,
    );
    assert_eq!(body(&res), "t1 started\n", "{res}");
    let dir = c.state.with_file_name("orchestrator").join("t1");
    let files = |d: &std::path::Path| {
        let mut v = vec![];
        let mut todo = vec![d.to_path_buf()];
        while let Some(p) = todo.pop() {
            for e in std::fs::read_dir(&p).unwrap().flatten() {
                if e.path().is_dir() {
                    todo.push(e.path());
                } else {
                    v.push(e.path());
                }
            }
        }
        v.sort();
        v
    };
    let before = files(&dir);
    assert!(
        before.iter().any(|f| f.ends_with(format!("uploads/{T}-a.png"))),
        "{before:?}"
    );
    let starts = |c: &drover::Ctx| {
        let s: Vec<_> = drover::events::read(&c.state)
            .unwrap()
            .to_vec()
            .into_iter()
            .filter(|e| e.kind == "orchestrator-start")
            .collect();
        s.iter()
            .map(|e| (e.str("runtime").unwrap().to_string(), e.val("resumed") == true))
            .collect::<Vec<_>>()
    };
    let res = req(&s, "POST", "/thread", "op=archive&t=t1", &cookie, T + 10);
    assert_eq!(body(&res), "t1 archived\n", "{res}");
    assert!(
        b.called("kill", "orchestrator-t1") && !b.has("orchestrator-t1"),
        "{:?}",
        b.methods()
    );
    let sleep = drover::events::read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .rfind(|e| e.kind == "orchestrator-sleep")
        .unwrap();
    assert_eq!(sleep.str("why"), Some("archived"));
    let page = chat(&s, &cookie, T + 11);
    let repo = |p: &Value| {
        p["repos"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["repo"] == "sendmeter")
            .unwrap()
            .clone()
    };
    let r = repo(&page);
    assert_eq!(
        (&r["rows"], &r["archived"][0]["id"]),
        (&json!([]), &json!("t1")),
        "{page}"
    );
    let res = req(&s, "POST", "/thread", "op=restore&t=t1", &cookie, T + 20);
    assert_eq!(body(&res), "t1 restored\n", "{res}");
    let r = repo(&chat(&s, &cookie, T + 21));
    assert_eq!((&r["archived"], &r["rows"][0]["id"]), (&json!([]), &json!("t1")), "{r}");
    let res = req(&s, "POST", "/thread", "op=say&t=t1&text=back+again", &cookie, T + 30);
    assert_eq!(body(&res), "sent\n", "{res}");
    let st = starts(&c);
    assert_eq!(st.len(), 2, "{st:?}");
    assert_eq!(st[1], ("shed-acp".to_string(), false), "{st:?}");
    assert_eq!(f.count(START_T1), 2, "{:?}", f.calls.borrow());
    assert_eq!(run(&c, &cli, &["thread", "archive", "t1"]).0, 0);
    assert_eq!(b.all("kill").len(), 2, "{:?}", b.methods());
    assert_eq!(run(&c, &cli, &["thread", "restore", "t1"]).0, 0);
    let l = log(&c);
    assert_eq!(
        (
            l.matches("\"thread-archive\"").count(),
            l.matches("\"thread-restore\"").count()
        ),
        (2, 2),
        "{l}"
    );
    let after = files(&dir);
    assert!(before.iter().all(|f| after.contains(f)), "{before:?} vs {after:?}");
}

#[test]
fn a_restored_shed_thread_resumes_its_orchestrators_session() {
    let c = setup("chat-archive-shed");
    let b = FakeShed::attached(&c);
    let r = Fake::default();
    assert_eq!(run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Look"]).0, 0);
    assert_eq!(run(&c, &r, &["thread", "say", "t1", "hi"]).0, 0);
    assert!(b.called("run", "orchestrator-t1"), "{:?}", b.methods());
    assert_eq!(run(&c, &r, &["thread", "archive", "t1"]).0, 0);
    assert!(
        b.called("kill", "orchestrator-t1") && !b.has("orchestrator-t1"),
        "{:?}",
        b.methods()
    );
    assert_eq!(run(&c, &r, &["thread", "restore", "t1"]).0, 0);
    assert_eq!(run(&c, &r, &["thread", "say", "t1", "back again"]).0, 0);
    let st: Vec<_> = drover::events::read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "orchestrator-start")
        .map(|e| (e.str("session").unwrap().to_string(), e.val("resumed") == true))
        .collect();
    assert_eq!(st.len(), 2, "{st:?}");
    assert!(st[1].1 && st[1].0 == st[0].0, "the same session resumed: {st:?}");
}

#[test]
fn a_message_with_three_images_delivers_all_three_and_eleven_are_refused() {
    let b = FakeShed::new();
    let c = acp_setup("chat-multi", &b);
    let f = acp();
    let cli = b.cli(&f);
    let r = b.on(&cli);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let many = |n: usize| {
        let names: Vec<String> = (0..n).map(|i| format!("IMG_{i}.PNG")).collect();
        let data: Vec<String> = (0..n).map(|i| url_enc(&B64.encode(format!("png {i}")))).collect();
        format!("name={}&data={}", names.join("%2F"), data.join("%2C"))
    };
    let res = req(
        &s,
        "POST",
        "/thread",
        &format!("op=new&repo=sendmeter&text=x&{}", many(11)),
        &cookie,
        T,
    );
    assert!(body(&res).contains("at most 10 files a message"), "{res}");
    assert!(!log(&c).contains("thread-new"), "no thread: {}", log(&c));
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Look"]).1,
        "t1\n"
    );
    assert_eq!(
        run(&c, &cli, &["thread", "say", "t1", "hi"]).0,
        0,
        "its conductor is up"
    );
    let res = req(
        &s,
        "POST",
        "/thread",
        &format!("op=say&t=t1&text=x&{}", many(11)),
        &cookie,
        T,
    );
    assert!(body(&res).contains("at most 10 files a message"), "{res}");
    let up = c.state.with_file_name("orchestrator").join("t1/uploads");
    assert_eq!(std::fs::read_dir(&up).map_or(0, |d| d.count()), 0, "nothing saved");
    let res = req(
        &s,
        "POST",
        "/thread",
        &format!("op=say&t=t1&text=&{}", many(3)),
        &cookie,
        T + 1,
    );
    assert_eq!(body(&res), "sent\n", "{res}");
    let saved: Vec<_> = (0..3).map(|i| up.join(format!("{}-IMG_{i}.PNG", T + 1))).collect();
    for (i, p) in saved.iter().enumerate() {
        assert_eq!(std::fs::read(p).unwrap(), format!("png {i}").as_bytes());
    }
    let att: String = saved.iter().map(|p| format!(" (attached: {})", p.display())).collect();
    assert!(
        f.called(&format!(
            "shed send orchestrator-t1 [t1] owner: IMG_0.PNG, IMG_1.PNG, IMG_2.PNG{att}"
        )),
        "{:?}",
        f.calls.borrow()
    );
    let got = files(&chat(&s, &cookie, T + 2), "t1");
    for i in 0..3 {
        assert!(got.contains(&format!("IMG_{i}.PNG")), "{got:?}");
    }
    let a = c.state.with_file_name("a.png");
    std::fs::write(&a, b"A").unwrap();
    let a = a.to_str().unwrap();
    let (code, out) = run(&c, &cli, &["thread", "say", "t1", "two", "--file", a, "--file", a]);
    assert_eq!(code, 0, "{out}");
    assert!(
        f.calls
            .borrow()
            .iter()
            .any(|l| l.starts_with("shed send orchestrator-t1 ")
                && l.contains("owner: two (attached: ")
                && l.contains("-a.png) (attached: ")
                && l.contains("-2-a.png)")),
        "{:?}",
        f.calls.borrow()
    );
}

fn working(b: &FakeShed, on: bool) {
    let st = ["idle", "working"][usize::from(on)];
    b.set("orchestrator-t1", |t| t["reported"] = serde_json::json!({"state": st}));
}

struct Typing<'a>(
    common::fake_shed::Cli<'a>,
    &'a FakeShed,
    std::sync::Mutex<Vec<(String, usize)>>,
);
impl Typing<'_> {
    fn esc(&self) -> usize {
        let keys = self.1.all("keys");
        keys.iter()
            .filter(|k| k["name"] == "orchestrator-t1" && k["keys"] == serde_json::json!(["esc"]))
            .count()
    }
}
impl drover::runner::Runner for Typing<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        if prog == "shed" && args[0] == "send" {
            let line = format!("{prog} {}", args.join(" "));
            self.2.lock().unwrap().push((line, self.esc()));
        }
        self.0.run(prog, args)
    }
}

fn post_to(addr: &std::net::SocketAddr, cookie: &str, body: &str) -> String {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    let head = format!(
        "POST /thread HTTP/1.1\r\nHost: {addr}\r\n{cookie}Content-Length: {}\r\n\r\n",
        body.len()
    );
    s.write_all((head + body).as_bytes()).unwrap();
    let mut res = String::new();
    s.read_to_string(&mut res).unwrap();
    body_of(&res)
}
fn body_of(res: &str) -> String {
    res[res.find("\r\n\r\n").unwrap() + 4..].into()
}

fn queued_ids(c: &drover::Ctx) -> Vec<(u64, String)> {
    let ev = drover::events::read(&c.state).unwrap();
    let q = ev
        .iter()
        .filter(|e| e.kind == "thread-queue" && e.data.get("q").is_none());
    q.map(|e| (e.seq, e.str("text").unwrap_or_default().to_string()))
        .collect()
}

fn until(f: impl Fn() -> bool) -> bool {
    (0..100).any(|_| f() || (std::thread::sleep(std::time::Duration::from_millis(100)), false).1)
}

#[test]
fn a_queued_message_waits_for_the_turn_to_end_and_steer_interrupts_and_sends_now() {
    let b = leaked();
    let c = acp_setup("chat-queue", b);
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Look"]).1,
        "t1\n"
    );
    assert_eq!(
        run(&c, &b.cli(&acp()), &["thread", "say", "t1", "hi"]).0,
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
    working(b, true);
    let f: &'static Fake = Box::leak(Box::new(acp()));
    let r: &'static Typing = Box::leak(Box::new(Typing(b.cli(f), b, Default::default())));
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c2, r, l, false));
    let typed = |t: &str| f.called(&format!("shed send orchestrator-t1 [t1] owner: {t}"));
    for t in ["one", "two", "three"] {
        assert_eq!(post_to(&addr, &cookie, &format!("op=queue&t=t1&text={t}")), "queued\n");
    }
    let ids = queued_ids(&c);
    assert_eq!(
        ids.iter().map(|q| q.1.as_str()).collect::<Vec<_>>(),
        ["one", "two", "three"]
    );
    let edit = format!("op=queue&t=t1&q={}&text=one+edited", ids[0].0);
    assert_eq!(post_to(&addr, &cookie, &edit), "queued\n");
    assert_eq!(
        post_to(&addr, &cookie, &format!("op=unqueue&t=t1&q={}", ids[2].0)),
        "removed\n"
    );
    assert_eq!(
        post_to(&addr, &cookie, "op=unqueue&t=t1&q=1"),
        "error: no such queued message\n"
    );
    let on = b.on(r);
    let page = chat(&Server::new(&c, &on, Some(ADDR.into())), &cookie, now);
    assert_eq!(
        thread(&page, "t1")["queued"],
        json!([{"q": ids[0].0, "text": "one edited"}, {"q": ids[1].0, "text": "two"}]),
        "{page}"
    );
    std::thread::sleep(std::time::Duration::from_secs(3));
    assert!(!typed("one") && !typed("two"), "typed mid-turn: {:?}", f.calls.borrow());
    working(b, false);
    assert!(until(|| typed("two")), "{:?}", f.calls.borrow());
    let calls = f.calls.borrow().clone();
    let at = |t: &str| calls.iter().position(|c| c.ends_with(&format!("owner: {t}")));
    assert!(
        at("one edited") < at("two") && at("three").is_none() && at("one").is_none(),
        "{calls:?}"
    );
    let l = log(&c);
    assert!(
        l.contains("\"from\":\"owner\",\"text\":\"one edited\"") && l.contains("\"from\":\"owner\",\"text\":\"two\"")
    );
    assert_eq!(l.matches("\"why\":\"sent\"").count(), 2, "{l}");
    let page = chat(&Server::new(&c, &on, Some(ADDR.into())), &cookie, now);
    assert_eq!(thread(&page, "t1")["queued"], json!([]), "the queue is empty");
    working(b, true);
    assert_eq!(r.esc(), 0);
    assert_eq!(post_to(&addr, &cookie, "op=steer&t=t1&text=stop+that"), "sent\n");
    assert!(until(|| typed("stop that")), "{:?}", f.calls.borrow());
    let seen = r.2.lock().unwrap().clone();
    let esc = seen.iter().find(|(l, _)| l.ends_with("owner: stop that")).map(|s| s.1);
    assert_eq!(esc, Some(1), "Esc before the message: {seen:?}");
    assert_eq!(post_to(&addr, &cookie, "op=queue&t=t1&text=four"), "queued\n");
    let four = queued_ids(&c).last().unwrap().0;
    assert_eq!(post_to(&addr, &cookie, &format!("op=steer&t=t1&q={four}")), "sent\n");
    assert!(until(|| typed("four")), "{:?}", f.calls.borrow());
    assert_eq!(r.esc(), 2);
    assert_eq!(log(&c).matches("\"why\":\"sent\"").count(), 3);
    assert_eq!(post_to(&addr, &cookie, "op=stop&t=t1"), "stopping\n");
    assert_eq!(r.esc(), 3, "[stop] is Esc alone");
}

#[test]
fn a_finished_turns_commands_fold_and_the_live_turns_stay_open() {
    let b = FakeShed::new();
    let c = acp_setup("chat-fold", &b);
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Look"]).1,
        "t1\n"
    );
    let cmd = |n: &str| {
        let d = serde_json::json!({"thread": "t1", "cmd": format!("drover brief sendmeter {n}")});
        drover::events::append(
            &c.state,
            &drover::events::Event::new(T, "sendmeter", None, "thread-cmd", d),
        )
        .unwrap();
    };
    let f = acp();
    let cli = b.cli(&f);
    assert_eq!(run(&c, &cli, &["thread", "say", "t1", "go"]).0, 0);
    cmd("1");
    cmd("2");
    assert_eq!(run(&c, &cli, &["thread", "reply", "t1", "done"]).0, 0);
    assert_eq!(run(&c, &cli, &["thread", "say", "t1", "again"]).0, 0);
    assert!(
        f.called("shed send orchestrator-t1 [t1] owner: again"),
        "{:?}",
        f.calls.borrow()
    );
    cmd("3");
    let ev = drover::events::read(&c.state).unwrap();
    let since = ev.iter().rfind(|e| e.str("text") == Some("again")).unwrap().ts;
    let shown = |st: &str| {
        b.set("orchestrator-t1", |t| t["reported"] = serde_json::json!({"state": st}));
        let f = Fake::default();
        let r = b.on(&f);
        let s = Server::new(&c, &r, Some(ADDR.into()));
        chat(&s, &pair(&s, &c, "Mozilla/5.0 (iPhone)", T), T)
    };
    let brief = |n: &str| format!("drover brief sendmeter {n}");
    let folds = |p: &Value| -> Vec<Value> {
        msgs(p, "t1")
            .into_iter()
            .filter(|m| m["kind"] == "fold")
            .map(|m| m["cmds"].clone())
            .collect()
    };
    let cmds = |p: &Value| -> Vec<String> {
        let all = msgs(p, "t1").into_iter().filter(|m| m["kind"] == "cmd");
        all.map(|m| m["text"].as_str().unwrap().to_string()).collect()
    };
    let w = shown("working");
    assert_eq!(folds(&w), [json!([brief("1"), brief("2")])], "{w}");
    assert_eq!(cmds(&w), [brief("3")], "the live turn's row stays: {w}");
    assert_eq!(thread(&w, "t1")["since"], json!(since), "{w}");
    let i = shown("idle");
    assert_eq!(folds(&i), [json!([brief("1"), brief("2")]), json!([brief("3")])], "{i}");
    assert!(cmds(&i).is_empty() && thread(&i, "t1")["state"] != "working", "{i}");
}

#[test]
fn a_stream_with_a_thread_open_stamps_its_watch_file() {
    use std::io::{Read, Write};
    let c = setup("chat-watch");
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
    let b = leaked();
    b.attach(&c);
    std::thread::spawn(move || drover::serve::serve_on(&c2, &Fake::default(), l, false));
    let open = |q: &str| {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        write!(s, "GET /live?tab=chat{q} HTTP/1.1\r\nHost: {addr}\r\n{cookie}\r\n").unwrap();
        let mut b = [0u8; 64];
        let _ = s.read(&mut b).unwrap();
        s
    };
    let (w, x) = (
        c.state.with_file_name("watch-t1.at"),
        c.state.with_file_name("watch-t2.at"),
    );
    let _plain = open("");
    let s = open("&thread=t1");
    let at = || {
        std::fs::read_to_string(&w)
            .ok()
            .and_then(|t| t.trim().parse::<u64>().ok())
            .unwrap_or(0)
    };
    assert!(until(|| at() > 0), "no stamp");
    let first = at();
    assert!(first >= now, "{first} < {now}");
    assert!(until(|| at() > first), "not stamped again");
    let _bad = open("&thread=../t2");
    drop(s);
    std::thread::sleep(std::time::Duration::from_secs(3));
    let last = at();
    std::thread::sleep(std::time::Duration::from_secs(2));
    assert_eq!(at(), last, "a closed stream stamps no more");
    assert!(
        !x.exists()
            && std::fs::read_dir(c.state.parent().unwrap())
                .unwrap()
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("watch-"))
                .count()
                == 1
    );
}
