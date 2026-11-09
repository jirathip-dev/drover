//! #720: herdsman serves drover's cooee tabs itself: each tab's module (web/plugin/dist) at `/plugin/<tab>.js`, and a
//! cooee plugin's bearer token, a paired device's token, counts as that device (cooee never forwards a cookie).
mod common;
use common::*;
use herdsman::serve::Server;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

fn get(s: &Server, path: &str, extra: &str) -> String {
    let r = format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}\r\n");
    String::from_utf8_lossy(&s.respond(&r, T)).into_owned()
}

#[test]
fn each_tab_module_is_served_as_built() {
    let (c, r) = v1_board("plugin-modules");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    for tab in ["queue", "lanes", "asks", "fleets"] {
        let res = get(&s, &format!("/plugin/{tab}.js"), "");
        let built = std::fs::read_to_string(format!("{}/../../web/plugin/dist/{tab}.js", env!("CARGO_MANIFEST_DIR")));
        assert!(
            res.starts_with("HTTP/1.1 200 OK") && res.contains("Content-Type: text/javascript"),
            "{tab}: {res}"
        );
        assert_eq!(res.split_once("\r\n\r\n").unwrap().1, built.unwrap(), "{tab}");
    }
    for p in ["/plugin/system.js", "/plugin/queue", "/plugin/../serve.js"] {
        assert!(get(&s, p, "").starts_with("HTTP/1.1 404 "), "{p}");
    }
}

#[test]
fn a_paired_devices_token_as_bearer_is_that_device() {
    let (c, r) = v1_board("plugin-bearer");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = pair(&s, &c, "cooee", T);
    let token = ck.trim().strip_prefix("Cookie: hd=").unwrap();
    let status = |extra: &str| {
        get(&s, "/pair/status", extra)
            .split_once("\r\n\r\n")
            .unwrap()
            .1
            .to_string()
    };
    assert_eq!(
        status(&format!("Authorization: Bearer {token}\r\n")),
        r#"{"paired":true}"#
    );
    assert_eq!(
        status("Authorization: Bearer 0123456789abcdef0123\r\n"),
        r#"{"paired":false}"#
    );
    // A paired-only op, cooee's way: the bearer token, no cookie.
    let send = |auth: &str| {
        let body = format!(r#"{{"agent":"{IMPL}","text":"rerun"}}"#);
        let r = format!(
            "POST /api/v1/send HTTP/1.1\r\nHost: {ADDR}\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        String::from_utf8_lossy(&s.respond(&r, T)).into_owned()
    };
    let no = send("Authorization: Bearer 0123456789abcdef0123\r\n");
    assert!(no.starts_with("HTTP/1.1 401 ") && no.contains("UNPAIRED"), "{no}");
    assert!(!r.called("herdr agent prompt"));
    let yes = send(&format!("Authorization: Bearer {token}\r\n"));
    assert!(yes.starts_with("HTTP/1.1 200 OK"), "{yes}");
    assert!(r.called(&format!("herdr agent prompt {IMPL} rerun")));
}
