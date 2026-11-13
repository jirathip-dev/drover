//! #501: a 401 is checked, not believed. `GET /pair/status` says whether this browser is paired; a database error
//! reading the devices answers 503, never 401; each 401 and 503 is one `serve.log` line (never the token) that
//! `doctor` counts over 24 h; a browser pairing again replaces its own row.
mod common;
use common::*;
use drover::devices;
use drover::serve::Server;

const T: u64 = 1_000_000;
const IPHONE: &str = "Mozilla/5.0 (iPhone) Safari/604.1";

fn req(s: &Server, method: &str, path: &str, extra: &str, now: u64) -> String {
    let r = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n{extra}Content-Length: 0\r\n\r\n");
    String::from_utf8_lossy(&s.respond(&r, now)).into_owned()
}

fn body(res: &str) -> &str {
    &res[res.find("\r\n\r\n").unwrap() + 4..]
}

/// The `auth` lines `serve.log` holds.
fn logged(c: &drover::Ctx) -> Vec<String> {
    let log = std::fs::read_to_string(c.state.with_file_name("serve.log")).unwrap_or_default();
    log.lines()
        .filter(|l| l.starts_with("auth "))
        .map(String::from)
        .collect()
}

#[test]
fn pair_status_says_whether_this_browser_is_paired() {
    let c = ctx(&tmpdir("pair-status"), CONFIG);
    let r = Fake::default();
    let s = Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    let cookie = pair(&s, &c, IPHONE, T);
    let yes = req(&s, "GET", "/pair/status", &cookie, T);
    assert!(
        yes.starts_with("HTTP/1.1 200 OK") && yes.contains("\r\nX-Paired: 1\r\n"),
        "{yes}"
    );
    assert_eq!(body(&yes), r#"{"paired":true}"#);
    let no = req(&s, "GET", "/pair/status", "Cookie: hd=feed\r\n", T);
    assert!(no.starts_with("HTTP/1.1 200 OK") && !no.contains("X-Paired"), "{no}");
    assert_eq!(body(&no), r#"{"paired":false}"#);
    assert!(logged(&c).is_empty(), "a 200 is not logged");
}

#[test]
fn a_database_error_is_503_never_401_and_each_refusal_logs_one_line() {
    let c = ctx(&tmpdir("pair-busy"), CONFIG);
    let r = Fake::default();
    let s = Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    let cookie = pair(&s, &c, IPHONE, T);
    let token = cookie.trim().strip_prefix("Cookie: hd=").unwrap().to_string();
    let hash = devices::load(&c)[0].hash.clone();
    let send = "POST /send HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n";
    // The three 401s, each with its reason.
    assert!(req(&s, "GET", "/lane?agent=x", "", T).starts_with("HTTP/1.1 401 "));
    assert!(req(&s, "GET", "/lane?agent=x", "Cookie: hd=feed\r\n", T).starts_with("HTTP/1.1 401 "));
    let late = T + devices::YEAR + 1;
    assert!(req(&s, "GET", "/lane?agent=x", &cookie, late).starts_with("HTTP/1.1 401 "));
    // The devices unreadable (state.db gone, a directory in its place): 503, never 401.
    let db = drover::db::file(&c.state);
    for x in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{x}", db.display()));
    }
    std::fs::create_dir(&db).unwrap();
    let why = devices::check(&c, cookie.trim().strip_prefix("Cookie: ").unwrap()).unwrap_err();
    assert_eq!(why, "db error: is a directory");
    let sent = String::from_utf8_lossy(&s.respond(&format!("{send}{cookie}Content-Length: 9\r\n\r\nagent=x&t"), T))
        .into_owned();
    assert!(
        sent.starts_with("HTTP/1.1 503 ") && sent.contains("board busy"),
        "{sent}"
    );
    for p in ["/lane?agent=x", "/pair/status", "/api/v1/lane/x", "/api/v1/needs"] {
        let res = req(&s, "GET", p, &cookie, T);
        assert!(res.starts_with("HTTP/1.1 503 "), "{p}: {res}");
    }
    let lines = logged(&c);
    let want = [
        "401 GET /lane: no cookie",
        "401 GET /lane: unknown token",
        "401 GET /lane: expired",
        "503 POST /send: db error: is a directory",
        "503 GET /lane: db error: is a directory",
        "503 GET /pair/status: db error: is a directory",
        "503 GET /api/v1/lane/x: db error: is a directory",
        "503 GET /api/v1/needs: db error: is a directory",
    ];
    assert_eq!(lines.len(), want.len(), "{lines:#?}");
    for (l, w) in lines.iter().zip(want) {
        let at = l.split(' ').nth(1).unwrap().parse::<u64>().unwrap();
        assert!(l.ends_with(w) && [T, late].contains(&at), "{l} / {w}");
        assert!(!l.contains(&token) && !l.contains(&hash), "no token: {l}");
    }
    // doctor counts the last 24 h: a day later they are gone.
    assert_eq!(devices::denials(&c), (3, 5));
    let (_, out) = run(&c, &Fake::default(), &["doctor"]);
    assert!(out.contains("! board auth — 24 h: 3 × 401, 5 × 503 → see "), "{out}");
    assert_eq!(
        devices::denials(&drover::Ctx {
            now: late + 86_400,
            ..c.clone()
        }),
        (0, 0)
    );
}

#[test]
fn pairing_the_same_browser_again_keeps_one_row() {
    let c = ctx(&tmpdir("pair-again"), CONFIG);
    let r = Fake::default();
    let s = Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    let mac = pair(&s, &c, "Mozilla/5.0 (Macintosh; Mac OS X) Safari/605.1", T);
    let old = pair(&s, &c, IPHONE, T + 10);
    let new = pair(&s, &c, IPHONE, T + 20);
    let paired: Vec<_> = devices::load(&c).into_iter().filter(|d| d.code.is_none()).collect();
    let labels: Vec<_> = paired.iter().map(|d| d.label.rsplit_once(' ').unwrap().0).collect();
    assert_eq!(
        labels,
        ["Mac Safari", "iPhone Safari"],
        "the iPhone's older row replaced, the Mac kept"
    );
    let status = |ck: &str| body(&req(&s, "GET", "/pair/status", ck, T + 30)).to_string();
    assert_eq!(status(&new), r#"{"paired":true}"#);
    assert_eq!(status(&mac), r#"{"paired":true}"#);
    assert_eq!(status(&old), r#"{"paired":false}"#);
    let (rc, out) = run(&c, &Fake::default(), &["serve", "devices"]);
    assert_eq!(
        (rc, out.lines().filter(|l| l.contains("Safari")).count()),
        (0, 2),
        "{out}"
    );
}
