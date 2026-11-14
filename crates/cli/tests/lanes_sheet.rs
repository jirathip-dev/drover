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

fn section<'a>(page: &'a str, id: &str) -> &'a str {
    let a = page.find(&format!("<section id={id}>")).unwrap();
    &page[a..a + page[a..].find("</section>").unwrap()]
}

#[test]
fn lanes_group_by_state_then_repo_with_counts() {
    let (c, r) = fixture("group");
    let s = Server::new(&c, &r, None);
    let page = with_queue(&req(&s, "GET", "/?all=1", "", "", T), &s, ADDR, T);
    let lanes = section(&page, "lanes");
    let mut got = vec![];
    for part in lanes.split("<h").skip(1) {
        let text = &part[..part.find("</h").unwrap()];
        let text = text.trim_end_matches("</span>").rsplit('>').next().unwrap();
        got.push(if part.starts_with('2') {
            text.to_string()
        } else {
            format!("  {text}")
        });
        let mut rows = part
            .split("data-agent=\"")
            .skip(1)
            .map(|r| r[..r.find('"').unwrap()].to_string());
        got.extend(rows.by_ref().map(|a| format!("    {a}")));
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
    assert!(
        !lanes.contains("<details")
            && lanes.contains("<button type=button class=lr>")
            && lanes.contains("<div class=card hidden>")
    );
    assert!(lanes.contains("<p>impl · round 0 · claude · "), "{lanes}");
}

#[test]
fn lane_card_shows_its_port() {
    let (c, r) = fixture("port");
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude", "port": 41234});
    append(&c.state, &Event::new(900_800, "sendmeter", Some(12), "spawn", d)).unwrap();
    let s = Server::new(&c, &r, None);
    let page = with_queue(&req(&s, "GET", "/?all=1", "", "", T), &s, ADDR, T);
    assert!(section(&page, "lanes").contains(" · :41234 · "), "{page}");
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
        (&lines, &json!(2), &json!(5), &json!(false))
    );
    assert_eq!(
        r.1.last("read"),
        json!({"name": "sendmeter-impl-12", "lines": 200, "history": true})
    );
    assert!(req(&s, "GET", "/lane?agent=nobody", "", &cookie, T).starts_with("HTTP/1.1 404 "));
}

#[test]
fn others_sheet_offers_only_close_pane() {
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
    let res = req(&s, "POST", "/act", "agent=side-project&op=close&confirm=1", &cookie, T);
    assert!(
        res.starts_with("HTTP/1.1 200 OK") && r.1.called("kill", "side-project") && !r.1.has("side-project"),
        "{res}"
    );
    let res = req(
        &s,
        "POST",
        "/act",
        "agent=sendmeter-impl-12&op=close&confirm=1",
        &cookie,
        T,
    );
    assert!(
        res.contains("REFUSED IS_LANE") && !r.1.called("kill", "sendmeter-impl-12"),
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
fn pairing_is_a_phone_request_approved_on_the_mac() {
    let (c, r) = fixture("pair-req");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let at = |now: u64| drover::Ctx { now, ..c.clone() };
    assert!(req(&s, "POST", "/pair", "code=123456", "", T).starts_with("HTTP/1.1 404 "));
    let (rc, out) = run(&c, &Fake::default(), &["serve", "pair"]);
    assert_eq!(
        (rc, out.as_str()),
        (0, "now: open the page, tap Pair, then run serve approve <code>\n")
    );
    let page = with_queue(&req(&s, "GET", "/?all=1", "", "", T), &s, ADDR, T);
    assert!(page.contains("<button type=button class=pairb>Pair this phone</button></header>"));
    let host = host_tab(&s, ADDR, T);
    assert!(
        host.contains(
            "no paired device yet: tap [pair this phone], then <code>drover serve approve &lt;code&gt;</code>"
        ) && !(page.clone() + &host).contains("approve <code>"),
        "the hint's placeholder is text, never an open tag"
    );
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
    assert!(req(&s, "POST", "/send", "agent=sendmeter-impl-12&text=hi", &mine, T).starts_with("HTTP/1.1 401 "));
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
        !req(&s, "GET", "/?all=1", "", &mine, T + 61).contains("class=pairb"),
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
    assert!(req(&s3, "POST", "/send", "agent=sendmeter-impl-12&text=hi", &cookie, T + 4).starts_with("HTTP/1.1 200 "));
}

#[test]
fn pairing_codes_devices_and_prompt_events() {
    let (c, r) = fixture("pair");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let send = |cookie: &str, text: &str| {
        req(
            &s,
            "POST",
            "/send",
            &format!("agent=sendmeter-impl-12&text={text}"),
            cookie,
            T + 10,
        )
    };
    let res = send("", "hi");
    assert!(
        res.starts_with("HTTP/1.1 401 ") && res.contains("tap Pair this phone"),
        "{res}"
    );
    assert!(req(&s, "POST", "/pair/request", "", "Origin: https://evil.example\r\n", T).starts_with("HTTP/1.1 403 "));
    let evil = "POST /pair/claim HTTP/1.1\r\nHost: evil.example\r\nContent-Length: 0\r\n\r\n";
    assert!(String::from_utf8_lossy(&s.respond(evil, T)).starts_with("HTTP/1.1 403 "));
    let cookie = paired(&s, &c);
    let file = drover::db::file(&c.state);
    let stored = serde_json::to_string(&drover::devices::load(&c)).unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    let tok = &cookie[11..cookie.len() - 2];
    assert!(
        !stored.contains(tok) && stored.contains(&drover::devices::sha256(tok.as_bytes())),
        "{stored}"
    );
    let res = send(&cookie, "hello+secret+words");
    assert!(
        res.starts_with("HTTP/1.1 200 OK") && res.ends_with("sent to sendmeter-impl-12\n"),
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
        assert!(send(&cookie, ctl).contains("REFUSED BAD_VALUE: "), "{ctl}");
    }
    assert!(send(&cookie, "--wait%0Aline+two").starts_with("HTTP/1.1 200 OK"));
    assert_eq!(r.1.sent("sendmeter-impl-12")[1], "--wait\nline two");
    let host = host_tab(&s, ADDR, T);
    assert!(
        host.contains(&format!("<span>{label} <span class=m>paired ")) && host.contains("<button>Remove</button>"),
        "{host}"
    );
    let mt = &host[host.find("name=mtime value=").unwrap() + 17..];
    let mt: String = mt.chars().take_while(char::is_ascii_digit).collect();
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
    assert!(drover::devices::load(&c).is_empty());
    assert!(send(&cookie, "again").starts_with("HTTP/1.1 401 "));
}

#[test]
fn reply_refusals_rate_limit_and_read_only() {
    let (c, r) = fixture("refuse");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = paired(&s, &c);
    let send = |agent: &str, text: &str| req(&s, "POST", "/send", &format!("agent={agent}&text={text}"), &cookie, T);
    let body = |res: String| res[res.find("\r\n\r\n").unwrap() + 4..].trim().to_string();
    assert!(body(send("sendmeter-rev-9-r1", "stop")).starts_with("REFUSED REV_READONLY: "));
    assert!(body(send("sendmeter-impl-12", &"x".repeat(2001))).starts_with("REFUSED BAD_VALUE: "));
    assert!(
        body(send("sendmeter-impl-12", "+++")).starts_with("REFUSED BAD_VALUE: "),
        "blank"
    );
    assert_eq!(run(&c, &Fake::default(), &["pause", "sendmeter"]).0, 0);
    assert!(body(send("sendmeter-impl-12", "go")).starts_with("REFUSED PAUSED: "));
    assert!(r.1.all("send").is_empty() && !read(&c.state).unwrap().iter().any(|e| e.kind == "prompt"));
    for _ in 0..6 {
        send("synergy-impl-8", "go");
    }
    assert!(send("synergy-impl-8", "go").starts_with("HTTP/1.1 429 "));
    assert_eq!(r.1.sent("synergy-impl-8").len(), 6, "10 attempts incl. the 4 refused");
    let ro = Server::new(&c, &r, None);
    assert!(req(&ro, "POST", "/send", "agent=synergy-impl-8&text=go", &cookie, T).starts_with("HTTP/1.1 405 "));
}

#[test]
fn page_has_no_dash_placeholders() {
    let (c, r) = fixture("dash");
    run(&c, &Fake::default(), &["queue", "sendmeter", "add", "18", "12"]);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let page = req(&s, "GET", "/?all=1", "", "", T);
    let mut html = page[page.find("<body").unwrap()..].to_string();
    html.truncate(html.find("<script").unwrap());
    html += &queue_js(&s, ADDR, T, "tab(d) + sheet(d, 'sendmeter')");
    html += &parts(&s, ADDR, "", T).concat();
    html += &events_tab(&s, ADDR, T, "{}").unwrap_or_default();
    assert!(
        !html.contains(" - ") && !html.contains(">-<") && !html.contains(" · -") && !html.contains('—'),
        "{html}"
    );
    assert!(
        html.contains("<details class=\"q\" data-q data-act=idle data-grow=queued data-i=18><summary><span><a href=\"https://github.com/jirathip-dev/sendmeter/issues/18\">#18</a></span><span class=a>queued</span>"),
        "{html}"
    );
    assert!(
        html.contains("<option value=\"\" selected>default (profile)</option>"),
        "unset model"
    );
    assert!(
        html.contains("<option value=\"\" selected>default (none)</option>"),
        "unset effort"
    );
}

#[test]
fn ascii_horse_snapshots() {
    let states = ["working", "idle", "unknown", "done", "blocked", "dead"];
    let all = avatars(&states.map(|s| ("x", "impl", s)));
    let asc = |state: &str| {
        let a = &all[states.iter().position(|s| *s == state).unwrap()];
        a[a.find("</i>").unwrap() + 4..a.find("</i><svg").unwrap() + 4].to_string()
    };
    let stand = "<i class=asc><b>    _/>\n~(___)\n  | |</b></i>";
    assert_eq!(
        asc("working"),
        "<i class=\"asc run\"><b>    _/>\n~(___)\n /  \\</b><b>    _/>\n~(___)\n  ><</b></i>"
    );
    assert_eq!(asc("idle"), stand);
    assert_eq!(asc("unknown"), stand);
    assert_eq!(asc("done"), "<i class=asc><b>\n    _/>\n~(___)_</b></i>");
    assert_eq!(asc("blocked"), "<i class=asc><b>     />\n ~(_/\n  / \\</b></i>");
    assert_eq!(asc("dead"), asc("blocked"));
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
    let all = avatars(&of);
    let w = &all[0];
    assert!(
        w.contains("class=\"av g working\"") && w.contains("<use href=#ha-impl />") && w.contains("data-act=impl"),
        "{w}"
    );
    let horses = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/themes/farm/sprites.svg"
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
    assert!(all[1].contains("class=\"av r blocked\"") && all[2].contains("av g done"));
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

#[test]
fn lanes_and_sheet_fit_390px() {
    if no_webkit() {
        return;
    }
    let (c, r) = fixture("shots");
    std::fs::create_dir_all(c.state.parent().unwrap().join("cache")).unwrap();
    std::fs::write(
        c.state.parent().unwrap().join("cache/issue-sendmeter-12.json"),
        json!({"title": "Send meter readings nightly"}).to_string(),
    )
    .unwrap();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let page = req(&s, "GET", "/?all=1", "", "", T);
    let body = |res: String| res[res.find("\r\n\r\n").unwrap() + 4..].to_string();
    let cookie = paired(&s, &c);
    let lane = body(req(&s, "GET", "/lane?agent=sendmeter-impl-12", "", &cookie, T));
    let (pair, sent) = (
        body(req(&s, "POST", "/send", "agent=sendmeter-impl-12&text=x", "", T)),
        body(req(
            &s,
            "POST",
            "/send",
            "agent=sendmeter-impl-12&text=rerun+the+tests",
            &cookie,
            T,
        )),
    );
    let mut more: serde_json::Value = serde_json::from_str(&lane).unwrap();
    more["more"] = json!(true);
    let kill = "sendmeter-impl-12 · impl · 3 unpushed commit(s), 0 dirty file(s) → wip/sendmeter-impl-12-20261031-1200 · pane closes, worktree kept; dry run: nothing changed; pass --yes to kill";
    let fake = |lane: (u16, &str), send: (u16, &str)| {
        format!(
            "window.fetch = async (u, o) => u.startsWith('/lane') ? new Response({}, {{status: {}}}) : u === '/act' ? \
             (o.body.get('op') === 'clean' ? new Response('REFUSED UNSAFE_CLEAN: sendmeter-impl-12: not merged', {{status: 409}}) : new Response({}, {{status: 200}})) : new Response({}, {{status: {}}});",
            json!(lane.1), lane.0, json!(kill), json!(send.1), send.0
        )
    };
    let stub = |send: (u16, &str)| fake((200, &lane), send);
    let open = "open(document.querySelector('.lane[data-agent=\"sendmeter-impl-12\"]'))";
    let submit = "document.querySelector('.reply textarea').value = 'rerun the tests'; document.querySelector('.reply').requestSubmit()";
    let dir = std::env::var("DROVER_SNAPSHOTS").ok();
    let d = tmpdir("shots-png");
    let file = d.join("page.html");
    std::fs::write(&file, drawn(&page[page.find("<!doctype").unwrap()..], &s, ADDR, "", T)).unwrap();
    let scenes = [
        ("lanes-horse", "horse", String::new()),
        ("lanes-ascii", "ascii", String::new()),
        ("lanes-glyph", "glyph", String::new()),
        ("sheet-78", "horse", format!("{}; {open}", stub((200, &sent)))),
        (
            "sheet-full",
            "horse",
            format!("{}; {open}; snap('full')", stub((200, &sent))),
        ),
        (
            "pairing-prompt",
            "horse",
            format!("{}; {open}; {submit}", stub((401, &pair))),
        ),
        (
            "reply-sent",
            "horse",
            format!("{}; {open}; {submit}", stub((200, &sent))),
        ),
        (
            "transcript-more",
            "horse",
            format!(
                "{}; {open}; snap('full'); setTimeout(() => document.querySelector('.blk.run').open = true, 100)",
                fake((200, &more.to_string()), (200, &sent))
            ),
        ),
        ("chat-open", "horse", chat_scene(true)),
        ("chat-collapsed", "horse", chat_scene(false)),
        (
            "slash-menu",
            "horse",
            format!(
                "{}; {open}; ta.value = '/c'; ta.dispatchEvent(new Event('input'))",
                stub((200, &sent))
            ),
        ),
        (
            "kill-confirm",
            "horse",
            format!("{}; {open}; run('kill')", stub((200, &sent))),
        ),
        (
            "sheet-unpaired",
            "horse",
            format!(
                "{}; {open}",
                fake((401, "pair this device to read transcripts"), (401, &pair))
            ),
        ),
    ];
    for (name, mode, js) in scenes {
        let png = dir.as_ref().map_or(d.join(format!("{name}.png")), |p| {
            std::path::Path::new(p).join(format!("{name}.png"))
        });
        let o = webview()
            .args([file.to_str().unwrap(), "390", "lanes", mode, png.to_str().unwrap(), &js])
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

#[test]
fn chat_views_never_blank() {
    if no_webkit() {
        return;
    }
    let (c, r) = fixture("never-blank");
    let d = json!({"role": "impl", "agent": "synergy-impl-7", "harness": "claude", "machine": "air", "host": "air"});
    append(&c.state, &Event::new(900_800, "synergy", Some(7), "spawn", d)).unwrap();
    let screen = "⏺ Bash(cargo test)\n  ⎿ ok\n> fix the test\n⏺ Fixed: the test passes.\n";
    let OnShed(f, sh) = r;
    let air = FakeShed::new();
    air.lane("synergy-impl-7", "working", "/x");
    air.screen("synergy-impl-7", screen);
    let over = OverSsh {
        f: &f,
        hosts: vec![("air", &air)],
    };
    let r = drover::shed::With(&over, sh.shed());
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = paired(&s, &c);
    let sheet = |agent: &str| {
        let res = req(&s, "GET", &format!("/lane?agent={agent}"), "", &cookie, T);
        serde_json::from_str::<serde_json::Value>(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap()
    };
    let mut local = sheet("sendmeter-impl-12");
    local["said"] = json!([{"k": "me", "text": "fix the test", "at": "2026-11-03T09:00:00Z"},
        {"k": "ai", "text": "Fixed: **the test** passes.", "at": "2026-11-03T09:01:00Z"}]);
    let mut empty = local.clone();
    empty["said"] = json!([]);
    let remote = sheet("synergy-impl-7");
    assert_eq!(
        air.last("read"),
        json!({"name": "synergy-impl-7", "lines": 200, "history": true})
    );
    assert_eq!(
        (&remote["said"], remote["lines"].as_array().unwrap().len()),
        (&json!(null), 4),
        "{remote}"
    );
    let page = req(&s, "GET", "/?all=1", "", "", T);
    let d = tmpdir("never-blank-png");
    let file = d.join("page.html");
    std::fs::write(&file, drawn(&page[page.find("<!doctype").unwrap()..], &s, ADDR, "", T)).unwrap();
    for (agent, lane) in [
        ("sendmeter-impl-12", local),
        ("sendmeter-impl-12", empty),
        ("synergy-impl-7", remote),
        ("sendmeter-rev-9-r1", sheet("sendmeter-rev-9-r1")),
    ] {
        let js = format!(
            "window.errs = 0; window.onerror = () => {{ errs++; }}; addEventListener('unhandledrejection', () => errs++); \
             console.error = () => errs++; window.fetch = async () => new Response({}, {{status: 200}}); \
             if ($('.fv').ariaPressed === 'true') $('.fv').click(); open($('.lane[data-agent=\"{agent}\"]')); \
             setTimeout(() => {{ const a = now.innerHTML, msgs = $$('.now .blk.me, .now .blk.ai').length; $('.fv').click(); \
             window.late = [msgs, $$('.now .blk').length, now.innerHTML !== a ? 1 : 0, errs]; }}, 300)",
            json!(lane.to_string())
        );
        let png = d.join(format!("{agent}.png"));
        let o = webview()
            .args([
                file.to_str().unwrap(),
                "390",
                "lanes",
                "horse",
                png.to_str().unwrap(),
                &js,
            ])
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&o.stdout);
        let m: Vec<f64> = out.split_whitespace().skip(2).filter_map(|n| n.parse().ok()).collect();
        assert!(
            o.status.success() && m.len() == 4,
            "{agent}: {out} {}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(
            m[0] >= 1.0 && m[1] >= 1.0 && m[2] == 1.0 && m[3] == 0.0,
            "{agent}: default msgs, full blocks, toggled, errors: {m:?}"
        );
    }
}

#[test]
fn one_401_never_latches_unpaired() {
    if no_webkit() {
        return;
    }
    let (c, r) = fixture("pair-recheck");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = paired(&s, &c);
    let res = req(&s, "GET", "/lane?agent=sendmeter-impl-12", "", &cookie, T);
    let lane = &res[res.find("\r\n\r\n").unwrap() + 4..];
    let page = req(&s, "GET", "/?all=1", "", "", T);
    let d = tmpdir("pair-recheck-png");
    let file = d.join("page.html");
    std::fs::write(&file, drawn(&page[page.find("<!doctype").unwrap()..], &s, ADDR, "", T)).unwrap();
    let scene = |paired: bool| {
        let js = format!(
            "let k = 0, asked = 0; window.fetch = async u => u.includes('pair/status') ? (asked++, new Response(JSON.stringify({{paired: {paired}}}))) : \
             u.startsWith('/lane') ? (k++ ? new Response({lane}, {{headers: {{'X-Paired': '1'}}}}) : new Response('pair this device to read transcripts', {{status: 401}})) : \
             new Response('', {{status: 404}}); \
             const vis = () => +!document.querySelector('.pairf').hidden; \
             open(document.querySelector('.lane[data-agent=\"sendmeter-impl-12\"]')); \
             setTimeout(() => {{ const a = vis(), u = +!!document.querySelector('.unp'); poll().then(() => {{ window.late = [a, vis(), u, asked]; }}); }}, 200)",
            lane = json!(lane)
        );
        let png = d.join(format!("pair-{paired}.png"));
        let o = webview()
            .env("DROVER_WAIT", "1")
            .args([
                file.to_str().unwrap(),
                "390",
                "lanes",
                "horse",
                png.to_str().unwrap(),
                &js,
            ])
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&o.stdout).into_owned();
        assert!(o.status.success(), "{out} {}", String::from_utf8_lossy(&o.stderr));
        out.split_whitespace()
            .skip(2)
            .map(|n| n.parse::<f64>().unwrap() as u8)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        scene(false),
        [1, 0, 1, 1],
        "confirmed no: Pair UI, gone after the next paired read"
    );
    assert_eq!(scene(true), [0, 0, 0, 1], "status yes: no Pair UI at all");
}
