mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

fn req(s: &Server, method: &str, path: &str, body: &str, extra: &str) -> (u16, String) {
    let r = format!(
        "{method} {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&r, T)).into_owned();
    let (head, body) = res.split_once("\r\n\r\n").unwrap();
    (head[9..12].parse().unwrap(), body.to_string())
}

fn get(s: &Server, path: &str, extra: &str) -> Value {
    let (status, body) = req(s, "GET", path, "", extra);
    assert_eq!(status, 200, "{path}: {body}");
    serde_json::from_str(&body).unwrap()
}

fn golden(name: &str, got: &Value) {
    let f = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/golden/cooee/{name}.json"));
    if std::env::var_os("DROVER_BLESS").is_some() {
        std::fs::write(&f, serde_json::to_string_pretty(got).unwrap() + "\n").unwrap();
    }
    let want: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    assert_eq!(got, &want, "{name}: {}", serde_json::to_string_pretty(got).unwrap());
}

fn find<'a>(v: &'a Value, id: &str) -> &'a Value {
    fn go<'a>(v: &'a Value, id: &str) -> Option<&'a Value> {
        if v.get("id").and_then(Value::as_str) == Some(id) {
            return Some(v);
        }
        match v {
            Value::Object(m) => m.values().find_map(|x| go(x, id)),
            Value::Array(a) => a.iter().find_map(|x| go(x, id)),
            _ => None,
        }
    }
    go(v, id).unwrap_or_else(|| panic!("no block {id} in {v}"))
}

fn recorded(name: &str) -> drover::Ctx {
    let c = ctx(
        &tmpdir(name),
        &format!("{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\n"),
    );
    let profile = c.config.with_file_name("claude").join("fleet-orchestrator");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    let ev =
        |ts: u64, fleet: &str, kind: &str, d: Value| append(&c.state, &Event::new(ts, fleet, None, kind, d)).unwrap();
    let msg = |ts, from: &str, text: &str| {
        ev(
            ts,
            "sendmeter",
            "thread-msg",
            json!({"thread": "t1", "from": from, "text": text}),
        );
    };
    ev(
        T - 90_000,
        "sendmeter",
        "thread-new",
        json!({"thread": "t1", "title": "Search worker split", "harness": "claude"}),
    );
    ev(T - 89_000, "sendmeter", "thread-archive", json!({"thread": "t1"}));
    ev(
        T - 7_200,
        "sendmeter",
        "thread-new",
        json!({"thread": "t2", "title": "Rate limits for /v1/search", "harness": "claude"}),
    );
    let msg2 = |ts, from: &str, text: &str| {
        ev(
            ts,
            "sendmeter",
            "thread-msg",
            json!({"thread": "t2", "from": from, "text": text}),
        );
    };
    msg(T - 89_500, "owner", "split it");
    msg2(T - 7_100, "owner", "Grill me, then file it.");
    ev(
        T - 7_000,
        "sendmeter",
        "thread-cmd",
        json!({"thread": "t2", "cmd": "drover brief sendmeter 19"}),
    );
    msg2(T - 6_900, "orchestrator", "Two questions, one at a time.");
    ev(
        T - 6_850,
        "sendmeter",
        "thread-msg",
        json!({"thread": "t2", "from": "t3", "text": "See t3 and https://example.com/a.", "files": [{"name": "shot.png", "size": 2048}, {"name": "run.log", "size": 10}]}),
    );
    let ask = |ts, id: &str, q: &str, opts: Value| {
        ev(
            ts,
            "sendmeter",
            "needs-decision",
            json!({"id": id, "thread": "t2", "question": q, "options": opts, "recommend": 1, "kind": "owner"}),
        );
    };
    ask(
        T - 6_800,
        "q31",
        "Limit per API key or per client IP?",
        json!(["per key", "per ip"]),
    );
    ev(
        T - 6_700,
        "sendmeter",
        "decision",
        json!({"id": "q31", "choice": "per key", "by": "owner"}),
    );
    ev(
        T - 500,
        "sendmeter",
        "thread-msg",
        json!({"thread": "t2", "from": "system", "text": "✗ not delivered: no pane", "line": "and the print sheet"}),
    );
    ask(
        T - 300,
        "q33",
        "Retry-After only, or the X-RateLimit headers too?",
        json!(["retry-after", "both"]),
    );
    ev(
        T - 3_000,
        "all",
        "thread-new",
        json!({"thread": "t3", "title": "Weekly lesson digest", "harness": "claude"}),
    );
    ev(
        T - 2_900,
        "all",
        "thread-msg",
        json!({"thread": "t3", "from": "orchestrator", "text": "Filed lessons."}),
    );
    c
}

#[test]
fn the_threads_screens_are_their_golden_json_and_the_tab_is_standalone_only() {
    let c = recorded("threads-get");
    let r = OnShed::new(Fake::default());
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let list = get(&s, "/screens/threads", "");
    golden("threads", &list);
    assert_eq!(list["badge"], 1);
    let ids: Vec<&str> = list["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["r-all", "r-sendmeter", "archived"]);
    assert_eq!(
        find(&list, "t2")["subtitle"]["text"],
        "! choose · conductor: Retry-After only, or the X-RateLimit headers too?"
    );
    assert_eq!(find(&list, "t2")["tap"], json!({"open": "/screens/threads/t2"}));
    assert_eq!(find(&list, "t1")["swipe"][0]["action"], "thread-restore");

    let t = get(&s, "/screens/threads/t2", "");
    golden("thread", &t);
    assert_eq!(
        (&t["focus"], &t["actions"][0]["action"]),
        (&json!(true), &json!("thread-archive"))
    );
    let open = find(&t, "q33");
    assert_eq!(
        (&open["type"], &open["recommended"]),
        (&json!("choice"), &json!("retry-after"))
    );
    let blocks = t["blocks"].as_array().unwrap();
    let kinds: Vec<&str> = blocks.iter().filter_map(|b| b["kind"].as_str()).collect();
    assert_eq!(kinds, ["you", "tool", "orch", "orch", "ask", "you", "ask"]);
    assert_eq!(
        (&t["blocks"][0]["sticky"], &t["blocks"][0]["title"]),
        (&json!(true), &json!("Rate limits for /v1/search · t2"))
    );
    assert_eq!(find(&t, "m-8-i")["items"][0]["src"], "/thread/t2/shot.png");
    assert_eq!(find(&t, "m-8-f0")["tap"], json!({"url": "/thread/t2/run.log"}));
    assert_eq!(
        (&find(&t, "m-8-link")["label"], &find(&t, "m-8-link")["tap"]),
        (&json!("from t3"), &json!({"thread": "t3"}))
    );
    let bad = blocks.iter().find(|b| b["tone"] == "bad").unwrap();
    assert_eq!(bad["buttons"][0]["action"], "thread-retry", "{t}");
    assert!(t.to_string().contains("✓ **per key** · owner"), "{t}");
    assert_eq!(blocks.last().unwrap()["action"], "thread-say");
    assert!(get(&s, "/screens/threads/t1", "")["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .all(|b| b["type"] != "composer"));

    golden("thread-new", &get(&s, "/screens/threads/new/sendmeter", ""));
    let (status, _) = req(&s, "GET", "/screens/threads/new/nope", "", "");
    assert_eq!(status, 404);

    let (_, page) = req(&s, "GET", "/", "", "");
    assert!(
        page.contains(r#""id":"threads""#),
        "the standalone page has the Threads tab"
    );
    let manifest = include_str!("../../../cooee-plugin.toml");
    assert!(
        !manifest.contains("threads") && !manifest.contains("thread-"),
        "never the plugin's"
    );
}

#[test]
fn parity_15_the_owner_reads_a_thread_sends_and_answers_from_the_tab() {
    let c = recorded("threads-act");
    let r = OnShed::new(Fake::default());
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let post = |v: Value, extra: &str| req(&s, "POST", "/action", &v.to_string(), extra);
    let say = json!({"screen": "thread-t2", "block": "say", "action": "thread-say", "values": {"thread": "t2", "text": "Skip the print one."}});
    assert_eq!(post(say.clone(), "").0, 401);
    std::fs::write(c.config.with_file_name("cooee-token"), "tok").unwrap();
    let (status, v) = post(say.clone(), "Authorization: Bearer tok\r\n");
    assert_eq!(status, 403, "the plugin has no Threads: {v}");

    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let reads = || {
        read(&c.state)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "thread-read")
            .count()
    };
    assert_eq!(reads(), 0);
    get(&s, "/screens/threads/t2", &ck);
    get(&s, "/screens/threads/t2", &ck);
    assert_eq!(reads(), 1, "a paired open marks it read, once");

    let (status, v) = post(say, &ck);
    assert_eq!(status, 200, "{v}");
    let sent = read(&c.state).unwrap().to_vec();
    let sent = sent
        .iter()
        .find(|e| e.kind == "thread-msg" && e.str("text") == Some("Skip the print one."));
    assert_eq!(sent.and_then(|e| e.str("from")), Some("owner"));
    let t = get(&s, "/screens/threads/t2", &ck);
    let mine = t["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .rfind(|b| b["kind"] == "you" && b["tone"] != "bad");
    assert!(
        mine.is_some_and(|m| m.to_string().contains("Skip the print one.")),
        "{t}"
    );

    let answer = json!({"screen": "thread-t2", "block": "q33", "action": "answer", "values": {"thread": "t2", "choice": "both", "other": ""}});
    let (status, v) = post(answer, &ck);
    assert_eq!(status, 200, "{v}");
    let d = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .rfind(|e| e.kind == "decision")
        .unwrap();
    assert_eq!((d.str("id"), d.str("choice")), (Some("q33"), Some("both")));
    let t = get(&s, "/screens/threads/t2", &ck);
    assert!(
        t.to_string().contains("✓ **both**") && !t.to_string().contains(r#""type":"choice""#),
        "{t}"
    );

    let new = json!({"screen": "thread-new-sendmeter", "block": "say", "action": "thread-new", "values": {"repo": "sendmeter", "text": "Dark mode for settings"}});
    let (status, v) = post(new, &ck);
    assert_eq!(status, 200, "{v}");
    let v: Value = serde_json::from_str(&v).unwrap();
    assert_eq!(v["ops"][0]["screen"]["id"], "thread-t4", "{v}");
    assert_eq!(v["ops"][0]["screen"]["title"], "Dark mode for settings");

    let arch = json!({"screen": "threads", "block": "t3", "action": "thread-archive", "values": {"thread": "t3"}});
    assert_eq!(post(arch, &ck).0, 200);
    let list = get(&s, "/screens/threads", &ck);
    assert_eq!(find(&list, "t3")["swipe"][0]["action"], "thread-restore");
    assert!(find(&list, "archived").to_string().contains("\"t3\""), "{list}");
}

#[test]
fn parity_34_each_composer_op_reaches_its_thread_op_and_the_screen_shows_it() {
    let c = recorded("threads-ops");
    let sh = FakeShed::attached(&c);
    let start = json!({"thread": "t3", "agent": "orchestrator-t3", "pane": "orchestrator-t3"});
    append(&c.state, &Event::new(T - 60, "all", None, "orchestrator-start", start)).unwrap();
    sh.add(
        "orchestrator-t3",
        json!({"fleet": "all", "issue": "-", "role": "orchestrator", "thread": "t3", "source": "drover"}),
    );
    let r = OnShed(Fake::default(), sh);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let post = |action: &str, values: Value| {
        let a = json!({"screen": "thread-t3", "block": "say", "action": action, "values": values});
        let (status, body) = req(&s, "POST", "/action", &a.to_string(), &ck);
        assert_eq!(status, 200, "{action}: {body}");
        serde_json::from_str::<Value>(&body).unwrap()["toast"].clone()
    };
    let say = || find(&get(&s, "/screens/threads/t3", &ck), "say").clone();
    let esc = || {
        r.1.all("keys")
            .iter()
            .filter(|k| k["name"] == "orchestrator-t3")
            .count()
    };
    let log = || read(&c.state).unwrap().to_vec();

    let box0 = say();
    assert!(
        box0["working"]["since"].as_str().is_some_and(|t| t.ends_with('Z')),
        "{box0}"
    );
    assert_eq!(box0["acts"]["queue"], "thread-queue");

    let two = json!({"thread": "t3", "text": "two shots", "name": "a.png/b.png", "data": "cG5nMQ==,cG5nMg=="});
    assert_eq!(post("thread-say", two), "sent");
    let t = get(&s, "/screens/threads/t3", &ck);
    let mine = t["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .rfind(|b| b["kind"] == "you")
        .unwrap()
        .clone();
    let imgs: Vec<&str> = mine["blocks"][1]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["src"].as_str().unwrap())
        .collect();
    assert_eq!(imgs, ["/thread/t3/a.png", "/thread/t3/b.png"], "{mine}");
    let (status, png) = req(&s, "GET", imgs[1], "", "");
    assert_eq!((status, png.as_str()), (200, "png2"));

    assert_eq!(
        post("thread-queue", json!({"thread": "t3", "text": "after this"})),
        "queued"
    );
    let q = say()["queued"][0]["q"].as_u64().unwrap();
    post(
        "thread-queue",
        json!({"thread": "t3", "text": "after this, edited", "q": q}),
    );
    assert_eq!(say()["queued"], json!([{"q": q, "text": "after this, edited"}]));
    let before = esc();
    assert_eq!(post("thread-steer", json!({"thread": "t3", "q": q})), "sent");
    assert!(
        say()["queued"] == json!([]) && esc() > before,
        "steered: the queue empties, the turn gets Esc"
    );
    let sent = log()
        .into_iter()
        .rfind(|e| e.kind == "thread-msg" && e.str("from") == Some("owner"));
    assert_eq!(
        sent.and_then(|e| e.str("text").map(String::from)).as_deref(),
        Some("after this, edited")
    );

    post("thread-queue", json!({"thread": "t3", "text": "never mind"}));
    let q2 = say()["queued"][0]["q"].clone();
    assert_eq!(post("thread-unqueue", json!({"thread": "t3", "q": q2})), "removed");
    assert_eq!(say()["queued"], json!([]));
    let gone = log().into_iter().rfind(|e| e.kind == "thread-unqueue").unwrap();
    assert_eq!(gone.str("why"), Some("removed"));

    let before = esc();
    assert_eq!(post("thread-stop", json!({"thread": "t3"})), "stopping");
    assert_eq!(esc(), before + 1);

    assert_eq!(
        post("thread-model", json!({"thread": "t3", "effort": "high"})),
        "picked"
    );
    let chips = say()["chips"].clone();
    let effort = chips
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "effort")
        .unwrap();
    assert_eq!(effort["cur"], "high", "{chips}");

    let failed = log().into_iter().find(|e| e.str("line").is_some()).unwrap();
    let retry = json!({"thread": "t2", "msg": failed.seq});
    assert_eq!(post("thread-retry", retry), "retrying");

    let new = json!({"repo": "sendmeter", "text": "Pick hermes", "harness": "hermes"});
    post("thread-new", new);
    let made = log().into_iter().rfind(|e| e.kind == "thread-new").unwrap();
    assert_eq!(
        (made.str("title"), made.str("harness")),
        (Some("Pick hermes"), Some("hermes"))
    );
}

fn composer_in_webkit(name: &str, js: &str) -> Vec<f64> {
    let c = recorded(name);
    let sh = FakeShed::attached(&c);
    let now = drover::now();
    let start = json!({"thread": "t3", "agent": "orchestrator-t3", "pane": "orchestrator-t3"});
    append(
        &c.state,
        &Event::new(now - 60, "all", None, "orchestrator-start", start),
    )
    .unwrap();
    sh.add(
        "orchestrator-t3",
        json!({"fleet": "all", "issue": "-", "role": "orchestrator", "thread": "t3", "source": "drover"}),
    );
    let ck = pair(
        &Server::new(&c, &Fake::default(), Some(ADDR.into())),
        &c,
        "Mozilla/5.0 (iPhone)",
        now,
    );
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c2, &OnShed(Fake::default(), sh), l, false));
    let png = tmpdir(&format!("{name}-png")).join("page.png");
    let o = webview()
        .env("DROVER_COOKIE", ck.trim().trim_start_matches("Cookie: "))
        .args([&format!("http://{addr}/#threads"), "390", "-", "-"])
        .arg(&png)
        .arg(js)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(o.status.success(), "{out} {}", String::from_utf8_lossy(&o.stderr));
    out.split_whitespace().skip(2).filter_map(|n| n.parse().ok()).collect()
}

fn long(name: &str) -> drover::Ctx {
    let c = recorded(name);
    for i in 1..=70 {
        let d = json!({"thread": "t3", "from": "orchestrator", "text": format!("m{i}")});
        append(&c.state, &Event::new(T - 2_000 + i, "all", None, "thread-msg", d)).unwrap();
    }
    c
}

#[test]
fn a_long_thread_opens_on_its_last_30_and_its_marker_loads_the_30_before_with_the_later_ones() {
    let c = long("threads-older");
    let r = OnShed::new(Fake::default());
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let texts = |v: &Value| -> Vec<String> {
        let b = v["blocks"].as_array().unwrap().iter().filter(|b| b["kind"] == "orch");
        b.map(|b| b["blocks"][0]["markdown"].as_str().unwrap().to_string())
            .collect()
    };
    let m = |a: u32, b: u32| (a..=b).map(|i| format!("m{i}")).collect::<Vec<_>>();
    let t = get(&s, "/screens/threads/t3", "");
    assert_eq!(texts(&t), m(41, 70));
    let more = t["blocks"][1]["tap"]["open"].as_str().unwrap().to_string();
    assert_eq!(t["blocks"][1]["tap"]["keep"], true, "{t}");
    let t = get(&s, &more, "");
    assert_eq!(texts(&t), m(11, 70), "the 30 before, and every later one");
    assert_eq!(
        t["stream"],
        format!("/screens/threads/t3/stream{}", &more[more.find('?').unwrap()..])
    );
    let t = get(&s, t["blocks"][1]["tap"]["open"].as_str().unwrap(), "");
    let mut all = vec!["Filed lessons.".to_string()];
    all.extend(m(1, 70));
    assert_eq!(texts(&t), all);
    assert_eq!(
        t["blocks"][1]["type"], "card",
        "no marker once the thread's start is in: {t}"
    );
}

const WAIT: &str = "const st = window.setTimeout, $ = s => document.querySelector(s), $$ = s => [...document.querySelectorAll(s)]; \
    const until = async f => { for (let i = 0; i < 100 && !f(); i++) await new Promise(r => st(r, 50)); return !!f(); }; \
    const frame = () => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))); \
    const open = async t => { location.hash = '#threads'; await until(() => $(`.row[data-id=\"${t}\"]`)); $(`.row[data-id=\"${t}\"]`).click(); \
      await until(() => $(`.scr[data-id=\"thread-${t}\"] .cmp textarea`)); await frame(); return $(`.scr[data-id=\"thread-${t}\"] > .msgs`); }; \
    const end = sc => +(sc.scrollTop + sc.clientHeight >= sc.scrollHeight - 2 && sc.scrollTop > 0);";

fn webkit(name: &str, width: &str, js: &str) -> Vec<f64> {
    let c = long(name);
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || drover::serve::serve_on(&c, &Fake::default(), l, false));
    let png = std::env::var("DROVER_SNAPSHOTS").map_or(tmpdir(&format!("{name}-png")).join("t.png"), |p| {
        std::path::Path::new(&p).join(format!("{name}.png"))
    });
    let js = format!("window.late = (async () => {{ try {{ {WAIT} {js} }} catch (e) {{ return [-1]; }} }})()");
    let o = webview()
        .env("DROVER_WAIT", "1")
        .args([&format!("http://{addr}/"), width, "-", "-", png.to_str().unwrap(), &js])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{out} {}", String::from_utf8_lossy(&o.stderr));
    out.split_whitespace().skip(2).filter_map(|n| n.parse().ok()).collect()
}

const CMP: &str = "const q = s => document.querySelector(s), all = s => [...document.querySelectorAll(s)];
const wait = async f => { for (const t = Date.now(); Date.now() - t < 8000; await new Promise(r => setTimeout(r, 50))) { const v = f(); if (v) return v; } return 0; };
const type = t => { const b = q('.cmp textarea'); b.value = t; b.dispatchEvent(new Event('input')); };";

#[test]
fn parity_21_two_photos_go_with_the_message_and_a_failed_send_keeps_them_in_webkit() {
    if no_webkit() {
        return;
    }
    let js = format!("window.late = (async () => {{ {CMP}
(await wait(() => q('.row[data-id=\"t2\"]'))).click();
await wait(() => q('.cmp textarea'));
const px = Uint8Array.from(atob('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='), c => c.charCodeAt(0));
const dt = new DataTransfer();
for (const n of ['one.png', 'two.png']) dt.items.add(new File([px], n, {{ type: 'image/png' }}));
const pick = q('.cmp input[type=file]');
pick.files = dt.files;
pick.dispatchEvent(new Event('change'));
const thumbs = await wait(() => all('.cmp .tb img').length);
type('two photos');
const real = window.fetch;
window.fetch = (u, o) => (String(u).endsWith('/action') ? Promise.reject(new TypeError('offline')) : real(u, o));
q('.cmp .cr button.primary').click();
const kept = await wait(() => q('.cmp .cs.r button') && all('.cmp .tb img').length);
const text = Number(q('.cmp textarea').value === 'two photos');
window.fetch = real;
q('.cmp .cs.r button').click();
const shown = await wait(() => {{
  const c = all('.card.k-you').filter(c => c.textContent.includes('two photos')).pop();
  c?.scrollIntoView();
  const im = c ? [...c.querySelectorAll('.imgs img')] : [];
  return im.length && im.every(i => i.complete && i.naturalWidth > 0) && im.length;
}});
return [thumbs, kept, text, shown, all('.cmp .tb').length];
}})()");
    let v = composer_in_webkit("threads-webkit-files", &js);
    assert_eq!(
        v,
        [2.0, 2.0, 1.0, 2.0, 0.0],
        "thumbs, kept on failure, text kept, shown sent, box emptied"
    );
}

#[test]
fn a_thread_opens_at_its_end_in_webkit_with_its_bar_on_top_and_the_page_still() {
    if no_webkit() {
        return;
    }
    let js = "const sc = await open('t3'), col = sc.parentElement, bar = col.querySelector('.row.sticky'); \
        return [end(sc), document.scrollingElement.scrollTop, Math.round(bar.getBoundingClientRect().top - col.getBoundingClientRect().top), errs.length];";
    assert_eq!(webkit("thread-end-webkit", "390", js), [1., 0., 0., 0.]);
}

#[test]
fn older_messages_load_above_in_webkit_and_the_card_in_view_stays_in_place() {
    if no_webkit() {
        return;
    }
    let js = "const sc = await open('t3'), cards = () => $$('.scr[data-id^=thread-] .card.k-orch').length; \
        const at = [cards()], r = sc.getBoundingClientRect(); sc.scrollTop -= 200; await frame(); \
        const k = $$('.scr[data-id^=thread-] > .msgs > .card[data-id]').find(c => c.getBoundingClientRect().bottom > r.top + r.height / 2), id = k.dataset.id, y = k.getBoundingClientRect().top; \
        $('.scr button[data-id=older]').click(); await until(() => cards() > 30); await frame(); \
        const k2 = $(`.scr[data-id^=thread-] .card[data-id=\"${id}\"]`); \
        at.push(cards(), Math.round(Math.abs(k2.getBoundingClientRect().top - y)), +($('.scr[data-id^=thread-] > .msgs').scrollTop > 0), errs.length); return at;";
    assert_eq!(webkit("thread-older-webkit", "390", js), [30., 60., 0., 1., 0.]);
}

#[test]
fn an_image_opens_in_the_viewer_in_webkit_and_close_and_escape_shut_it() {
    if no_webkit() {
        return;
    }
    let js = "await open('t2'); $('.scr[data-id^=thread-] .imgs a').click(); await frame(); \
        const at = [+!!$('#iv'), +$('#iv img').src.endsWith('/thread/t2/shot.png'), +($('#iv a').download === 'shot.png')]; \
        [...$$('#iv button')].find(b => b.textContent === 'close').click(); at.push(+!!$('#iv')); \
        $('.scr[data-id^=thread-] .imgs a').click(); dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' })); at.push(+!!$('#iv'), errs.length); return at;";
    assert_eq!(webkit("thread-viewer-webkit", "390", js), [1., 1., 1., 0., 0., 0.]);
}

#[test]
fn the_keyboard_keeps_the_composer_flush_on_it_in_webkit_and_the_page_still() {
    if no_webkit() {
        return;
    }
    let js = "const sc = await open('t3'), f = $('.cmp'), box = f.querySelector('textarea'), vv = visualViewport, H = innerHeight; \
        const kb = h => { for (const [o, k] of [[vv, 'height'], [window, 'innerHeight']]) Object.defineProperty(o, k, { configurable: true, get: () => H - h }); vv.dispatchEvent(new Event('resize')); }; \
        const look = async y => { await frame(); return [Math.round(Math.abs(f.getBoundingClientRect().bottom - y)), document.scrollingElement.scrollTop, end(sc)]; }; \
        box.focus(); kb(336); const at = await look(vv.offsetTop + vv.height); \
        box.blur(); kb(0); at.push(...await look($('nav').getBoundingClientRect().top), errs.length); return at;";
    assert_eq!(webkit("thread-kb-webkit", "390", js), [0., 0., 1., 0., 0., 1., 0.]);
}

#[test]
fn the_composer_docks_below_the_messages_in_webkit_empty_and_with_4_lines_at_390_and_1440() {
    if no_webkit() {
        return;
    }
    let js = "const sc = await open('t3'), f = sc.parentElement.querySelector(':scope > .cmp'), box = f.querySelector('textarea'); \
        const look = async () => { await frame(); const t = f.getBoundingClientRect().top, k = [...sc.querySelectorAll(':scope > .card')].pop(); \
          const hit = document.elementFromPoint(innerWidth / 2, t - 2); \
          return [+(k.getBoundingClientRect().bottom <= t), +(hit === sc || !!hit?.closest('.msgs > .card')), end(sc)]; }; \
        const at = await look(); box.value = 'one\\ntwo\\nthree\\nfour'; box.dispatchEvent(new Event('input', { bubbles: true })); \
        at.push(...await look(), errs.length); return at;";
    for w in ["390", "1440"] {
        assert_eq!(
            webkit(&format!("thread-dock-webkit-{w}"), w, js),
            [1., 1., 1., 1., 1., 1., 0.],
            "at {w} px: the last card ends above the composer, which hides nothing, and the end stays in view"
        );
    }
}

fn cards(name: &str) -> drover::Ctx {
    let c = recorded(name);
    let f = Fake::default();
    assert_eq!(run(&c, &f, &["thread", "new", "sendmeter", "Ops"]).1, "t4\n");
    let file = c.config.with_file_name("proposed.toml");
    let old = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(&file, old.replacen("model = \"opus\"", "model = \"sonnet\"", 1)).unwrap();
    assert_eq!(
        run(&c, &f, &["config", "propose", "t4", file.to_str().unwrap()]),
        (0, "c1\n".into())
    );
    let secret = ["secret", "request", "t4", "API_KEY", "--why", "the search API"];
    assert_eq!(run(&c, &f, &secret).0, 0);
    let card = |cmd: &str| {
        let a = [
            "card",
            "post",
            "t4",
            "--cmd",
            cmd,
            "--why",
            "owner asked",
            "--blast",
            "read-only",
            "--dir",
            "/srv/app",
        ];
        let (code, id) = run(&c, &f, &a);
        assert_eq!(code, 0, "{id}");
        id.trim().to_string()
    };
    card("git log -1");
    let done = card("git status");
    let ev = |ts: u64, kind: &str, d: Value| append(&c.state, &Event::new(ts, "sendmeter", None, kind, d)).unwrap();
    ev(T, "run-settled", json!({"id": done, "approved": true, "by": "owner"}));
    ev(
        T,
        "run-done",
        json!({"id": done, "code": 1, "tail": "fatal: not a git repository"}),
    );
    let lane =
        json!({"id": "q9", "question": "Split the worker?", "options": ["yes", "no"], "recommend": 1, "kind": "owner"});
    append(
        &c.state,
        &Event::new(T - 120, "sendmeter", Some(12), "needs-decision", lane),
    )
    .unwrap();
    ev(
        T - 60,
        "decision",
        json!({"id": "q9", "choice": "yes", "by": "orchestrator-t4"}),
    );
    let say = |d: Value| ev(T - 30, "thread-msg", d);
    say(json!({"thread": "t4", "from": "system", "text": "answered q9 for you: yes", "change": "q9"}));
    say(json!({"thread": "t4", "from": "system", "text": "claude is signed out: sign in", "signin": "claude"}));
    ev(
        T - 20,
        "thread-handoff",
        json!({"thread": "t4", "to": "t2", "title": "Rate limits"}),
    );
    c
}

#[test]
fn every_card_kind_in_a_thread_is_its_golden_json() {
    let c = cards("thread-cards-get");
    let r = OnShed::new(Fake::default());
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let t = get(&s, "/screens/threads/t4", "");
    golden("thread-cards", &t);
    let blocks = t["blocks"].as_array().unwrap();
    let kinds: Vec<&str> = blocks.iter().filter_map(|b| b["kind"].as_str()).collect();
    assert_eq!(kinds, ["config", "secret", "run", "run", "ask", "sys", "sys"], "{t}");
    assert_eq!(
        blocks[0]["subtitle"],
        json!({"text": "waiting: config c1 · no orchestrator yet · sendmeter · claude · opus", "tone": "warn"}),
        "the bar names the waiting proposal"
    );
    let config = &blocks[1];
    let acts: Vec<&Value> = config["buttons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| &b["values"]["act"])
        .collect();
    assert_eq!(acts, [&json!("apply"), &json!("discard")]);
    assert!(config.to_string().contains(r#"+ model = \"sonnet\""#), "{config}");
    assert!(
        blocks[2].to_string().contains("pair this device") && !blocks[2].to_string().contains("\"form\""),
        "an unpaired device sees no secret entry: {}",
        blocks[2]
    );
}

#[test]
fn parity_26_a_message_sent_while_working_queues_and_send_now_steers_it_in_webkit() {
    if no_webkit() {
        return;
    }
    let js = format!("window.late = (async () => {{ {CMP}
(await wait(() => q('.row[data-id=\"t3\"]'))).click();
const pill = Number(!!(await wait(() => q('.cmp .pill .pt')))?.textContent.startsWith('working'));
type('after this');
const go = q('.cmp .cr button.primary'), label = Number(go.textContent === 'queue');
go.click();
const queued = await wait(() => all('.cmp .qd').filter(r => r.textContent.includes('after this')).length);
q('.cmp .qd button.primary').click();
const steered = await wait(() => Number(!q('.cmp .qd') && all('.card.k-you').some(c => c.textContent.includes('after this'))));
return [pill, label, queued, steered];
}})()");
    let v = composer_in_webkit("threads-webkit-queue", &js);
    assert_eq!(v, [1.0, 1.0, 1.0, 1.0], "pill, [queue], the queued chip, steered in");
}

#[test]
fn parity_32_the_owner_settles_each_card_from_inside_its_thread() {
    let c = cards("thread-cards-act");
    let r = OnShed::new(Fake::default());
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let t = get(&s, "/screens/threads/t4", &ck);
    let secret = find(&t, "m-17-f");
    assert_eq!(
        secret["fields"][0]["type"], "secret",
        "a paired device enters it masked: {t}"
    );
    let post = |a: &Value| {
        let (status, v) = req(&s, "POST", "/action", &a.to_string(), &ck);
        assert_eq!(status, 200, "{a}: {v}");
    };
    let tap =
        |b: &Value| json!({"screen": "thread-t4", "block": b["id"], "action": b["action"], "values": b["values"]});
    let logged = |kind: &str| read(&c.state).unwrap().to_vec().into_iter().rfind(|e| e.kind == kind);

    post(&tap(find(&t, "m-16-apply")));
    assert!(std::fs::read_to_string(&c.config)
        .unwrap()
        .contains("model = \"sonnet\""));
    assert_eq!(
        logged("config-settled")
            .and_then(|e| e.str("id").map(String::from))
            .as_deref(),
        Some("c1")
    );

    let mut save = tap(&secret["submit"]);
    save["values"]["text"] = json!("s3cr3t-value");
    post(&save);
    assert!(logged("secret-set").is_some_and(|e| e.str("name") == Some("API_KEY")));
    assert!(!format!("{:?}", read(&c.state).unwrap().to_vec()).contains("s3cr3t-value"));

    let run = find(&t, "r1");
    post(
        &json!({"screen": "thread-t4", "block": "r1", "action": run["action"], "values": {"choice": "deny", "sha": run["sha"]}}),
    );
    let settled = logged("run-settled").unwrap();
    assert_eq!(
        (settled.str("id"), settled.val("approved")),
        (Some("r1"), &json!(false))
    );

    let mut change = tap(find(&t, "q9"));
    change["values"]["choice"] = json!("no");
    post(&change);
    let d = logged("decision").unwrap();
    assert_eq!(
        (d.str("id"), d.str("choice"), d.str("by")),
        (Some("q9"), Some("no"), Some("owner"))
    );

    post(&tap(find(&t, "m-25-sign-in")));
    assert!(
        logged("setup").is_some_and(|e| e.str("name") == Some("claude")),
        "a sign-in terminal starts"
    );

    let t = get(&s, "/screens/threads/t4", &ck);
    let kinds: Vec<&str> = t["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|b| b["kind"].as_str())
        .collect();
    assert!(
        !kinds.contains(&"config") && !kinds.contains(&"secret"),
        "settled cards leave: {t}"
    );
    assert!(!t.to_string().contains(r#""type":"choice""#), "{t}");
}
