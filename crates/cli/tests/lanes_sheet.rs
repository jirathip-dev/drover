mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::serve::Server;
use serde_json::json;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

fn two() -> String {
    format!("{CONFIG}\n[fleet.synergy]\nrepo = \"o/synergy-apps\"\nlocal = \"/p/synergy-apps\"\nintegration = \"staging\"\n")
}

fn req(s: &Server, method: &str, path: &str, body: &str, extra: &str, now: u64) -> String {
    let r = format!(
        "{method} {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&r, now)).into_owned()
}

fn fixture(name: &str) -> (drover::Ctx, OnShed) {
    let c = ctx(&tmpdir(name), &two());
    let spawn = |fleet: &str, issue: u64, ts: u64, role: &str, agent: &str| {
        let d = json!({"role": role, "agent": agent, "harness": if role == "rev" { "hermes" } else { "claude" }});
        append(&c.state, &Event::new(ts, fleet, Some(issue), "spawn", d)).unwrap();
    };
    spawn("sendmeter", 12, 900_100, "impl", "sendmeter-impl-12");
    spawn("sendmeter", 9, 900_200, "rev", "sendmeter-rev-9-r1");
    spawn("sendmeter", 14, 900_300, "impl", "sendmeter-impl-14");
    spawn("synergy", 7, 900_400, "impl", "synergy-impl-7");
    spawn("synergy", 8, 900_500, "impl", "synergy-impl-8");
    spawn("synergy", 5, 900_600, "design", "synergy-design-5");
    spawn("synergy", 20, 900_700, "impl", "synergy-impl-20");
    let screen = "⏺ Read(src/serve.rs)\n  ⎿ Read 120 lines\n> fix the test\n⏺ Bash(cargo test)\n  ⎿ ok\n";
    let r = OnShed::new(Fake::default());
    for (n, st) in [
        ("sendmeter-impl-12", "working"),
        ("sendmeter-rev-9-r1", "blocked"),
        ("synergy-impl-7", "working"),
        ("synergy-impl-8", "working"),
        ("synergy-design-5", "done"),
        ("synergy-impl-20", "unknown"),
    ] {
        r.1.lane(n, st, "/x");
        r.1.screen(n, screen);
    }
    for (n, harness, st, cwd) in [
        ("side-project", "codex", "idle", "/home/me/side"),
        ("scratch", "claude", "working", "/home/me/scratch"),
    ] {
        r.1.add(n, json!({"fleet": "-", "harness": harness}));
        r.1.set(n, |t| {
            t["cwd"] = json!(cwd);
            t["reported"] = json!({"state": st});
        });
        r.1.screen(n, screen);
    }
    (c, r)
}

fn json_of(res: &str) -> serde_json::Value {
    serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap_or_else(|_| panic!("{res}"))
}

#[test]
fn parity_49_lanes_group_by_state_then_repo_with_counts() {
    let (c, r) = fixture("group");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let lanes = json_of(&req(&s, "GET", "/api/v1/lanes", "", "", T));
    let mut got = vec![];
    for sec in lanes["sections"].as_array().unwrap() {
        let n: usize = sec["repos"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["lanes"].as_array().unwrap().len())
            .sum();
        got.push(format!("{} ({n})", sec["name"].as_str().unwrap()));
        for repo in sec["repos"].as_array().unwrap() {
            let rows = repo["lanes"].as_array().unwrap();
            got.push(format!("  {} ({})", repo["name"].as_str().unwrap(), rows.len()));
            got.extend(rows.iter().map(|l| format!("    {}", l["name"].as_str().unwrap())));
        }
    }
    let want = [
        "blocked (1)",
        "  sendmeter (1)",
        "    sendmeter-rev-9-r1",
        "working (4)",
        "  sendmeter (1)",
        "    sendmeter-impl-12",
        "  synergy (2)",
        "    synergy-impl-8",
        "    synergy-impl-7",
        "  Others (1)",
        "    scratch",
        "idle (2)",
        "  synergy (1)",
        "    synergy-impl-20",
        "  Others (1)",
        "    side-project",
        "done (1)",
        "  synergy (1)",
        "    synergy-design-5",
        "dead (1)",
        "  sendmeter (1)",
        "    sendmeter-impl-14",
    ];
    assert_eq!(got, want);
}

#[test]
fn lane_card_shows_its_port() {
    let (c, r) = fixture("port");
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude", "port": 41234});
    append(&c.state, &Event::new(900_800, "sendmeter", Some(12), "spawn", d)).unwrap();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = paired(&s, &c);
    let sheet = json_of(&req(&s, "GET", "/screens/lanes/sendmeter-impl-12", "", &cookie, T));
    let port = sheet["blocks"][0]["blocks"][0]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "f-port");
    assert_eq!(port.map(|r| &r["value"]), Some(&json!(":41234")), "{sheet}");
}

#[test]
fn sheet_transcript_is_the_masked_pane_tail() {
    let (c, r) = fixture("tail");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    assert!(req(&s, "GET", "/lane?agent=sendmeter-impl-12", "", "", T).starts_with("HTTP/1.1 401 "));
    let evil = "GET /lane?agent=sendmeter-impl-12 HTTP/1.1\r\nHost: evil.example\r\n\r\n";
    assert!(String::from_utf8_lossy(&s.respond(evil, T)).starts_with("HTTP/1.1 403 "));
    let cookie = paired(&s, &c);
    let origin = format!("Origin: https://evil.example\r\n{cookie}");
    assert!(req(&s, "GET", "/lane?agent=sendmeter-impl-12", "", &origin, T).starts_with("HTTP/1.1 403 "));
    assert!(!r.1.called("read", "sendmeter-impl-12"));
    let res = req(&s, "GET", "/lane?agent=sendmeter-impl-12", "", &cookie, T);
    assert!(
        res.starts_with("HTTP/1.1 200 OK") && res.contains("application/json"),
        "{res}"
    );
    let v: serde_json::Value = serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    assert_eq!(v["harness"], "claude");
    let lines = json!([
        "⏺ Read(src/serve.rs)",
        "  ⎿ Read 120 lines",
        "> fix the test",
        "⏺ Bash(cargo test)",
        "  ⎿ ok"
    ]);
    assert_eq!(
        (&v["lines"], &v["live"], &v["before"], &v["more"]),
        (&lines, &json!(0), &json!(5), &json!(false))
    );
    let read = r.1.last("read");
    assert_eq!(
        (&read["name"], &read["history"]),
        (&json!("sendmeter-impl-12"), &json!(true)),
        "shed's text"
    );
    assert!(req(&s, "GET", "/lane?agent=nobody", "", &cookie, T).starts_with("HTTP/1.1 404 "));
}

#[test]
fn parity_53_others_sheet_offers_only_close_pane() {
    let (c, r) = fixture("others");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = paired(&s, &c);
    let sheet = |agent: &str| {
        let res = req(&s, "GET", &format!("/lane?agent={agent}"), "", &cookie, T);
        serde_json::from_str::<serde_json::Value>(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap()
    };
    let (other, lane) = (sheet("side-project"), sheet("sendmeter-impl-12"));
    assert_eq!(
        (&other["acts"], &other["pane"]),
        (&json!(["close"]), &json!("side-project"))
    );
    assert_eq!(
        (&lane["acts"], &lane["pane"]),
        (&json!(["stop", "kill", "clean"]), &json!(null))
    );
    let res = req(
        &s,
        "POST",
        "/api/v1/act",
        "agent=side-project&op=close&confirm=1",
        &cookie,
        T,
    );
    assert!(
        res.starts_with("HTTP/1.1 200 OK") && r.1.called("kill", "side-project") && !r.1.has("side-project"),
        "{res}"
    );
    let res = req(
        &s,
        "POST",
        "/api/v1/act",
        "agent=sendmeter-impl-12&op=close&confirm=1",
        &cookie,
        T,
    );
    assert!(
        res.contains("\"code\":\"IS_LANE\"") && !r.1.called("kill", "sendmeter-impl-12"),
        "{res}"
    );
}

fn paired(s: &Server, c: &drover::Ctx) -> String {
    let cookie = pair(s, c, "Mozilla/5.0 (iPhone) Safari/604.1", T);
    assert_eq!(cookie.len() - "Cookie: hd=\r\n".len(), 64, "32 random bytes, hex");
    cookie
}

fn ask_pair(s: &Server, ip: &str, now: u64) -> (String, String, String) {
    let res = req(
        s,
        "POST",
        "/pair/request",
        "",
        &format!("X-Peer: {ip}\r\nUser-Agent: Mozilla/5.0 (iPhone) Safari/604.1\r\n"),
        now,
    );
    let set = res
        .lines()
        .find_map(|l| l.strip_prefix("Set-Cookie: "))
        .unwrap_or_default();
    let cookie = format!("Cookie: {}\r\n", &set[..set.find(';').unwrap_or(0)]);
    (
        res.lines().next().unwrap().into(),
        res.rsplit("\r\n\r\n").next().unwrap().trim().into(),
        cookie,
    )
}

#[test]
fn parity_6_pairing_is_a_phone_request_approved_on_the_mac() {
    let (c, r) = fixture("pair-req");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let at = |now: u64| drover::Ctx { now, ..c.clone() };
    assert!(req(&s, "POST", "/pair", "code=123456", "", T).starts_with("HTTP/1.1 404 "));
    let (rc, out) = run(&c, &Fake::default(), &["serve", "pair"]);
    assert_eq!(
        (rc, out.as_str()),
        (0, "now: open the page, tap Pair, then run serve approve <code>\n")
    );
    let page = req(&s, "GET", "/?all=1", "", "", T);
    assert!(page.contains(" data-pair=1>") && page.contains("<button type=button class=pairb>pair</button></header>"));
    let res = req(
        &s,
        "POST",
        "/pair/request",
        "",
        "X-Peer: 10.0.0.1\r\nUser-Agent: Mozilla/5.0 (iPhone) Safari/604.1\r\n",
        T,
    );
    assert!(
        res.contains("Set-Cookie: hd=") && res.contains("; Path=/; Max-Age=600; HttpOnly; Secure; SameSite=Strict\r\n"),
        "{res}"
    );
    let (st, code, mine) = ask_pair(&s, "10.0.0.2", T);
    assert!(
        st.contains("200 OK") && code.len() == 6 && code.chars().all(|ch| ch.is_ascii_digit()),
        "{st} {code}"
    );
    assert!(req(&s, "POST", "/pair/claim", "", &mine, T).starts_with("HTTP/1.1 403 "));
    assert!(req(&s, "POST", "/api/v1/send", "agent=sendmeter-impl-12&text=hi", &mine, T).starts_with("HTTP/1.1 401 "));
    let (_, out) = run(&at(T + 60), &Fake::default(), &["serve", "pending"]);
    assert!(
        out.contains(&format!("{code} iPhone Safari ")) && out.contains(" 9m left") && out.lines().count() == 2,
        "{out}"
    );
    let (rc, out) = run(&at(T + 60), &Fake::default(), &["serve", "approve", "abcdef"]);
    assert!(rc == 3 && out.contains("REFUSED NO_SUCH_CODE"), "{out}");
    let (rc, out) = run(&at(T + 60), &Fake::default(), &["serve", "approve", &code]);
    assert!(
        rc == 0 && out.starts_with("paired iPhone Safari ") && out.len() == 26,
        "{out}"
    );
    assert!(
        req(&s, "GET", "/api/v1/events?since=0", "", "", T + 60).contains("pair-approve"),
        "the Events tab shows the approval"
    );
    let (_, _, other) = ask_pair(&s, "10.0.0.3", T + 60);
    for cookie in ["", other.as_str()] {
        let res = req(&s, "POST", "/pair/claim", "", cookie, T + 61);
        assert!(res.starts_with("HTTP/1.1 403 ") && !res.contains("Set-Cookie"), "{res}");
    }
    let res = req(&s, "POST", "/pair/claim", "", &mine, T + 61);
    assert!(
        res.starts_with("HTTP/1.1 200 OK") && res.contains("\r\n\r\npaired as iPhone Safari "),
        "{res}"
    );
    assert!(
        res.contains(&format!(
            "Set-Cookie: {}; Path=/; Max-Age=31536000; HttpOnly; Secure; SameSite=Strict\r\n",
            mine.trim().strip_prefix("Cookie: ").unwrap()
        )),
        "{res}"
    );
    assert!(
        !req(&s, "GET", "/?all=1", "", &mine, T + 61).contains("data-pair"),
        "paired: no Pair button"
    );
    let (rc, out) = run(&at(T + 62), &Fake::default(), &["serve", "approve", &code]);
    assert!(rc == 3 && out.contains("REFUSED NO_SUCH_CODE"), "{out}");
    let (_, late, cookie) = ask_pair(&s, "10.0.0.4", T + 100);
    let (rc, out) = run(&at(T + 700), &Fake::default(), &["serve", "approve", &late]);
    assert!(rc == 3 && out.contains("REFUSED EXPIRED"), "{out}");
    assert!(req(&s, "POST", "/pair/claim", "", &cookie, T + 700).starts_with("HTTP/1.1 403 "));
    assert!(
        ask_pair(&s, "10.0.0.5", T + 700).0.contains("200 OK")
            && ask_pair(&s, "10.0.0.5", T + 700).0.contains("200 OK")
    );
    assert!(ask_pair(&s, "10.0.0.5", T + 700).0.contains("429 "), "2 per address");
    assert!(ask_pair(&s, "10.0.0.6", T + 700).0.contains("200 OK"));
    assert!(ask_pair(&s, "10.0.0.6", T + 700).0.contains("200 OK"));
    assert!(ask_pair(&s, "10.0.0.7", T + 700).0.contains("429 "), "4 open");
    let ev: Vec<_> = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind.starts_with("pair"))
        .collect();
    let n = |k: &str| ev.iter().filter(|e| e.kind == k).count();
    assert_eq!((n("pair-request"), n("pair-approve"), n("paired")), (8, 1, 0));
    assert!(
        ev.iter()
            .all(|e| e.data.len() == 1 && e.str("device").is_some_and(|d| d.starts_with("iPhone Safari"))),
        "{ev:?}"
    );
    assert!(
        !drover::events::jsonl(&c.state)
            .unwrap()
            .contains(&mine[11..mine.len() - 2]),
        "no token in the log"
    );
}

#[test]
fn pairing_behind_a_proxy_and_retaps() {
    let (c, r) = fixture("pair-proxy");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let tap = |h: &str| req(&s, "POST", "/pair/request", "", h, T);
    let open = || drover::devices::pending(&c).len();
    for ip in ["100.64.0.1", "100.64.0.2"] {
        let h = format!("X-Peer: 127.0.0.1\r\nX-Forwarded-For: {ip}\r\n");
        assert!(
            tap(&h).starts_with("HTTP/1.1 200 ") && tap(&h).starts_with("HTTP/1.1 200 "),
            "{ip}"
        );
    }
    assert_eq!(open(), 4);
    assert!(
        tap("X-Peer: 127.0.0.1\r\nX-Forwarded-For: 100.64.0.3\r\n").starts_with("HTTP/1.1 429 "),
        "4 open at most"
    );
    let (c2, r2) = fixture("pair-spoof");
    let s2 = Server::new(&c2, &r2, Some(ADDR.into()));
    let spoof =
        |n: u8| format!("X-Peer: 10.0.0.9\r\nX-Forwarded-For: 100.64.0.{n}\r\nTailscale-User-Login: u{n}@x\r\n");
    let res: Vec<_> = (1..=3)
        .map(|n| req(&s2, "POST", "/pair/request", "", &spoof(n), T))
        .collect();
    assert!(res[0].starts_with("HTTP/1.1 200 ") && res[1].starts_with("HTTP/1.1 200 "));
    assert!(res[2].starts_with("HTTP/1.1 429 "), "2 per address: {}", res[2]);
    let (c3, r3) = fixture("pair-retap");
    let s3 = Server::new(&c3, &r3, Some(ADDR.into()));
    let mut cookie = String::new();
    for _ in 0..5 {
        let res = req(
            &s3,
            "POST",
            "/pair/request",
            "",
            &format!("X-Peer: 127.0.0.1\r\n{cookie}"),
            T,
        );
        let set = res.lines().find_map(|l| l.strip_prefix("Set-Cookie: ")).expect(&res);
        cookie = format!("Cookie: {}\r\n", &set[..set.find(';').unwrap()]);
    }
    let left = drover::devices::pending(&c3);
    assert_eq!(left.len(), 1, "{left:?}");
    let page = req(&s3, "GET", "/?all=1", "", "", T);
    assert!(!page.contains("pair separately"), "no device paired yet");
    drover::devices::approve(&c3, left[0].code.as_deref().unwrap()).unwrap();
    assert!(req(&s3, "POST", "/pair/claim", "", &cookie, T + 1).starts_with("HTTP/1.1 200 OK"));
    let page = req(&s3, "GET", "/?all=1", "", "", T + 1);
    assert!(
        page.contains("<p class=m>Safari and the home-screen app pair separately</p>"),
        "{page}"
    );
    assert!(!req(&s3, "GET", "/?all=1", "", &cookie, T + 1).contains("pair separately"));
    let (_, _, mine) = ask_pair(&s3, "10.0.0.3", T + 2);
    assert_eq!(drover::devices::pending(&c3).len(), 1);
    for k in [&cookie, &mine] {
        assert!(req(&s3, "POST", "/pair/cancel", "", k, T + 3).starts_with("HTTP/1.1 200 OK"));
    }
    assert_eq!(drover::devices::pending(&c3).len(), 0);
    assert!(req(
        &s3,
        "POST",
        "/api/v1/send",
        "agent=sendmeter-impl-12&text=hi",
        &cookie,
        T + 4
    )
    .starts_with("HTTP/1.1 200 "));
}

#[test]
fn pairing_codes_devices_and_prompt_events() {
    use std::os::unix::fs::PermissionsExt;
    let (c, r) = fixture("pair");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let send = |cookie: &str, text: &str| {
        req(
            &s,
            "POST",
            "/api/v1/send",
            &format!("agent=sendmeter-impl-12&text={text}"),
            cookie,
            T + 10,
        )
    };
    let res = send("", "hi");
    assert!(
        res.starts_with("HTTP/1.1 401 ") && res.contains("\"code\":\"UNPAIRED\""),
        "{res}"
    );
    assert!(req(&s, "POST", "/pair/request", "", "Origin: https://evil.example\r\n", T).starts_with("HTTP/1.1 403 "));
    let evil = "POST /pair/claim HTTP/1.1\r\nHost: evil.example\r\nContent-Length: 0\r\n\r\n";
    assert!(String::from_utf8_lossy(&s.respond(evil, T)).starts_with("HTTP/1.1 403 "));
    let cookie = paired(&s, &c);
    let file = drover::db::file(&c.state);
    let stored = serde_json::to_string(&drover::devices::load(&c)).unwrap();
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    let tok = &cookie[11..cookie.len() - 2];
    assert!(
        !stored.contains(tok) && stored.contains(&drover::devices::sha256(tok.as_bytes())),
        "{stored}"
    );
    let res = send(&cookie, "hello+secret+words");
    assert!(
        res.starts_with("HTTP/1.1 200 OK") && res.contains("\"message\":\"sent to sendmeter-impl-12\""),
        "{res}"
    );
    assert_eq!(r.1.sent("sendmeter-impl-12"), ["hello secret words"]);
    let seen = r.1.lock().seen.clone();
    let at = |m: &str| {
        seen.iter()
            .position(|(k, p)| k == m && p["name"] == "sendmeter-impl-12")
    };
    assert!(at("send") < at("keys"), "{seen:?}");
    assert_eq!(
        r.1.last("keys"),
        json!({"name": "sendmeter-impl-12", "keys": ["Enter"]})
    );
    let e = read(&c.state).unwrap().to_vec().pop().unwrap();
    let label = drover::devices::load(&c)[0].label.clone();
    assert!(label.starts_with("iPhone "), "{label}");
    assert_eq!(
        (e.kind.as_str(), e.fleet.as_str(), e.issue),
        ("prompt", "sendmeter", Some(12))
    );
    assert_eq!(
        serde_json::Value::Object(e.data),
        json!({"via": "web", "agent": "sendmeter-impl-12", "device": label, "chars": 18})
    );
    assert!(
        !drover::events::jsonl(&c.state).unwrap().contains("secret"),
        "no message text in the log"
    );
    for ctl in ["%1B%5B2J", "stop%03", "x%7F", "a%0Db", "a%09b"] {
        assert!(send(&cookie, ctl).contains("\"code\":\"BAD_VALUE\""), "{ctl}");
    }
    assert!(send(&cookie, "--wait%0Aline+two").starts_with("HTTP/1.1 200 OK"));
    assert_eq!(r.1.sent("sendmeter-impl-12")[1], "--wait\nline two");
    let mt = json_of(&req(&s, "GET", "/api/v1/state", "", "", T))["mtime"]
        .as_str()
        .unwrap()
        .to_string();
    let hash = drover::devices::sha256(tok.as_bytes());
    let unpair = format!("op=unpair&fleet=&hash={hash}&mtime={mt}");
    assert!(
        req(&s, "POST", "/", &unpair, "", T).starts_with("HTTP/1.1 401 "),
        "unpair needs a paired cookie"
    );
    let evil = format!(
        "POST / HTTP/1.1\r\nHost: evil.example\r\n{cookie}Content-Length: {}\r\n\r\n{unpair}",
        unpair.len()
    );
    assert!(String::from_utf8_lossy(&s.respond(&evil, T)).starts_with("HTTP/1.1 403 "));
    assert_eq!(drover::devices::load(&c).len(), 1);
    let res = req(&s, "POST", "/", &unpair, &cookie, T);
    assert!(
        res.starts_with("HTTP/1.1 303 ") && res.contains("device%20removed"),
        "{res}"
    );
    assert_eq!(drover::devices::load(&c), [] as [drover::devices::Device; 0]);
    assert!(send(&cookie, "again").starts_with("HTTP/1.1 401 "));
}

#[test]
fn reply_refusals_rate_limit_and_read_only() {
    let (c, r) = fixture("refuse");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = paired(&s, &c);
    let send = |agent: &str, text: &str| {
        req(
            &s,
            "POST",
            "/api/v1/send",
            &format!("agent={agent}&text={text}"),
            &cookie,
            T,
        )
    };
    let code = |res: String| json_of(&res)["code"].as_str().unwrap().to_string();
    assert_eq!(code(send("sendmeter-rev-9-r1", "stop")), "REV_READONLY");
    assert_eq!(code(send("sendmeter-impl-12", &"x".repeat(2001))), "BAD_VALUE");
    assert_eq!(code(send("sendmeter-impl-12", "+++")), "BAD_VALUE", "blank");
    assert_eq!(run(&c, &Fake::default(), &["pause", "sendmeter"]).0, 0);
    assert_eq!(code(send("sendmeter-impl-12", "go")), "PAUSED");
    assert!(r.1.all("send").is_empty() && !read(&c.state).unwrap().iter().any(|e| e.kind == "prompt"));
    for _ in 0..6 {
        send("synergy-impl-8", "go");
    }
    assert!(send("synergy-impl-8", "go").starts_with("HTTP/1.1 429 "));
    assert_eq!(r.1.sent("synergy-impl-8").len(), 6, "10 attempts incl. the 4 refused");
    let ro = Server::new(&c, &r, None);
    assert!(req(&ro, "POST", "/api/v1/send", "agent=synergy-impl-8&text=go", &cookie, T).starts_with("HTTP/1.1 405 "));
}

#[test]
fn page_has_no_dash_placeholders() {
    let (c, r) = fixture("dash");
    run(&c, &Fake::default(), &["queue", "sendmeter", "add", "18", "12"]);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = paired(&s, &c);
    let mut text = String::new();
    for p in [
        "/screens/queue",
        "/screens/lanes",
        "/screens/events",
        "/screens/fleets",
        "/screens/cards",
        "/screens/lanes/sendmeter-impl-12",
    ] {
        text += &req(&s, "GET", p, "", &cookie, T);
    }
    assert!(
        !text.contains(" - ") && !text.contains("\"-\"") && !text.contains(" · -") && !text.contains('—'),
        "{text}"
    );
    assert!(text.contains("\"id\":\"q-sendmeter-18\""), "{text}");
}

#[test]
fn vector_horses_keep_a_coat_per_name() {
    let names: Vec<String> = (0..40).map(|i| format!("agent-{i}")).collect();
    let mut of = vec![
        ("sendmeter-impl-12", "impl", "working"),
        ("a", "impl", "blocked"),
        ("a", "impl", "done"),
        ("synergy-impl-7", "impl", "idle"),
        ("synergy-impl-7", "impl", "idle"),
    ];
    of.extend(names.iter().map(|n| (n.as_str(), "impl", "idle")));
    let all: Vec<String> = of.iter().map(|(n, _, _)| drover::serve::coat(n)).collect();
    let horses = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/swag/farm/sprites.svg"
    ))
    .unwrap();
    assert!(
        horses
            .lines()
            .nth(1)
            .unwrap()
            .contains("class=\"leg leg-nf\" style=\"--a:28deg\"")
            && horses.contains("fill=\"var(--c)\"")
    );
    let coat = |a: &String| a[a.find("--c:").unwrap()..a.find(";--d").unwrap()].to_string();
    assert_eq!(coat(&all[3]), coat(&all[4]));
    let coats: std::collections::BTreeSet<String> = all[5..].iter().map(coat).collect();
    assert_eq!(coats.len(), 8, "all 8 Corral coats in use: {coats:?}");
    assert_eq!(
        drover::devices::sha256(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        drover::devices::sha256(&[b'a'; 56]),
        "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a"
    );
}

static STREAM_SLOTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn http(addr: std::net::SocketAddr, method: &str, path: &str, body: &str, cookie: &str) -> String {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\n{cookie}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut r = String::new();
    s.read_to_string(&mut r).unwrap();
    r
}

#[test]
fn a_lane_prompt_replies_before_shed_takes_it_and_a_failed_delivery_reaches_the_sheet() {
    let _slots = STREAM_SLOTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (c, r) = fixture("prompt-later");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", drover::now());
    drop(s);
    let (shed, moved) = (r.1.state.clone(), r.1.moved.clone());
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c2, &r, l, false));
    let agent = "sendmeter-impl-12";
    assert!(http(addr, "GET", "/api/v1/lanes", "", &cookie).starts_with("HTTP/1.1 200 "));
    let sent = || {
        shed.lock()
            .unwrap()
            .seen
            .iter()
            .filter(|(m, p)| m == "send" && p["name"] == agent)
            .count()
    };
    let settle = |done: &dyn Fn() -> bool| {
        for _ in 0..500 {
            if done() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the delivery never ran");
    };

    shed.lock().unwrap().held = Some(agent.into());
    let t0 = std::time::Instant::now();
    let res = http(
        addr,
        "POST",
        "/api/v1/send",
        &format!("agent={agent}&text=hello"),
        &cookie,
    );
    let took = t0.elapsed();
    assert!(
        res.starts_with("HTTP/1.1 200 ") && res.contains("sent to sendmeter-impl-12"),
        "{res}"
    );
    assert!(
        took < std::time::Duration::from_millis(50),
        "replied in {took:?} while shed held the prompt"
    );
    let log = read(&c.state).unwrap().to_vec();
    assert_eq!(
        log.last().map(|e| e.kind.as_str()),
        Some("prompt"),
        "logged before the reply"
    );
    shed.lock().unwrap().held = None;
    moved.notify_all();
    settle(&|| sent() == 1);

    shed.lock().unwrap().held = Some(agent.into());
    let res = http(
        addr,
        "POST",
        "/api/v1/send",
        &format!("agent={agent}&text=again"),
        &cookie,
    );
    assert!(res.starts_with("HTTP/1.1 200 "), "{res}");
    let gone = shed.lock().unwrap().terms.remove(agent).unwrap();
    shed.lock().unwrap().held = None;
    moved.notify_all();
    let failed = || {
        read(&c.state)
            .unwrap()
            .to_vec()
            .into_iter()
            .rfind(|e| e.kind == "prompt-failed")
    };
    settle(&|| failed().is_some());
    shed.lock().unwrap().terms.insert(agent.into(), gone);
    let e = failed().unwrap();
    assert_eq!((e.str("agent"), e.issue), (Some(agent), Some(12)), "{e:?}");
    let sheet = json_of(&http(addr, "GET", &format!("/screens/lanes/{agent}"), "", &cookie));
    let said = sheet["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == "transcript")
        .unwrap();
    let row = said["blocks"].as_array().unwrap().last().unwrap();
    assert_eq!(
        (&row["id"], &row["title"], &row["subtitle"]["tone"]),
        (
            &json!(format!("undelivered-{}", e.seq)),
            &json!("✗ not delivered"),
            &json!("bad")
        ),
        "{sheet}"
    );
    assert!(row["subtitle"]["text"].as_str().unwrap().contains(agent), "{row}");
    let first = {
        use std::io::{BufRead, Write};
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        write!(
            s,
            "GET /screens/lanes/{agent}/stream HTTP/1.1\r\nHost: {addr}\r\n{cookie}\r\n"
        )
        .unwrap();
        let line = std::io::BufReader::new(s)
            .lines()
            .map_while(Result::ok)
            .find(|l| l.starts_with("data: "));
        serde_json::from_str::<serde_json::Value>(&line.unwrap()[6..]).unwrap()
    };
    assert_eq!(
        first["ops"][0]["screen"], sheet,
        "the sheet's stream carries the row: {first}"
    );
}

#[test]
fn a_stream_the_client_left_frees_its_slot_within_two_seconds() {
    let _slots = STREAM_SLOTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (c, r) = fixture("stream-slots");
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c2, &r, l, false));
    let open = || {
        use std::io::{BufRead, Write};
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        write!(s, "GET /screens/lanes/stream HTTP/1.1\r\nHost: {addr}\r\n\r\n").unwrap();
        let first = std::io::BufReader::new(s.try_clone().unwrap()).lines().next();
        (first.and_then(Result::ok).unwrap_or_default(), s)
    };
    let mut left = vec![];
    while left.len() < 5 {
        let (head, s) = open();
        if !head.starts_with("HTTP/1.1 200 ") {
            assert!(head.starts_with("HTTP/1.1 404 "), "{head}");
            break;
        }
        left.push(s);
    }
    assert!(
        !left.is_empty() && left.len() <= 4,
        "at most 4 streams at once: {}",
        left.len()
    );
    drop(left);
    let t0 = std::time::Instant::now();
    while !open().0.starts_with("HTTP/1.1 200 ") {
        assert!(
            t0.elapsed() < std::time::Duration::from_secs(2),
            "the left streams still hold their slots"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}
