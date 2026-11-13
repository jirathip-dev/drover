mod common;
use common::*;
use drover::events::read;
use drover::serve::Server;

fn status(s: &Server, method: &str, path: &str, body: &str, extra: &str) -> String {
    let r = format!(
        "{method} {path} HTTP/1.1\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&r, 1_000_000))[9..12].to_string()
}

#[test]
fn cooees_plugin_token_reads_screens_and_posts_actions_without_a_paired_device() {
    std::env::set_var("COOEE_PLUGIN_TOKEN", "tok-814");
    let c = ctx(&tmpdir("cooee-token"), CONFIG);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "17"]).0, 0);
    let r = Fake::default();
    let s = Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    let (cooee, wrong) = (
        "Host: 127.0.0.1:4810\r\nAuthorization: Bearer tok-814\r\n",
        "Host: 127.0.0.1:4810\r\nAuthorization: Bearer nope\r\n",
    );
    assert_eq!(status(&s, "GET", "/screens/queue", "", cooee), "200");
    assert_eq!(status(&s, "GET", "/screens/queue", "", wrong), "403");
    let drop = r#"{"screen": "queue", "block": "q-sendmeter-17", "action": "drop", "values": {"fleet": "sendmeter", "issue": 17}}"#;
    let local = "Host: 127.0.0.1:7777\r\nAuthorization: Bearer nope\r\n";
    assert_eq!(status(&s, "POST", "/action", drop, local), "401");
    assert_eq!(read(&c.state).unwrap().last().unwrap().data["op"], "add");
    assert_eq!(status(&s, "POST", "/action", drop, cooee), "200");
    assert_eq!(read(&c.state).unwrap().last().unwrap().data["op"], "rm");
}

#[test]
fn the_manifest_attaches_to_the_running_serve_with_its_token_file() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cooee-plugin.toml");
    let m: toml::Value = toml::from_str(&std::fs::read_to_string(root).unwrap()).unwrap();
    assert_eq!(m["attach"].as_str(), Some("http://127.0.0.1:7777"));
    assert_eq!(m["attach_token"].as_str(), Some("~/.config/drover/cooee-token"));
}

fn ask(addr: std::net::SocketAddr, method: &str, path: &str, body: &str, extra: &str) -> String {
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    let r = format!(
        "{method} {path} HTTP/1.1\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    s.write_all(r.as_bytes()).unwrap();
    let mut res = vec![];
    s.read_to_end(&mut res).unwrap();
    String::from_utf8_lossy(&res)[9..12].to_string()
}

fn start(c: &'static drover::Ctx) -> std::net::SocketAddr {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let r: &'static Fake = Box::leak(Box::default());
    std::thread::spawn(move || drover::serve::serve_on(c, r, l, false));
    addr
}

#[test]
fn serve_makes_the_cooee_token_file_once_and_accepts_it_on_action_and_screens() {
    use std::os::unix::fs::PermissionsExt;
    let c: &'static drover::Ctx = Box::leak(Box::new(ctx(&tmpdir("cooee-token-file"), CONFIG)));
    assert_eq!(run(c, &Fake::default(), &["queue", "sendmeter", "add", "17"]).0, 0);
    let file = c.config.with_file_name("cooee-token");
    assert!(!file.exists());
    let first = start(c);
    assert_eq!(
        ask(first, "GET", "/screens/queue", "", "Host: 127.0.0.1:4810\r\n"),
        "403"
    );
    let token = std::fs::read_to_string(&file).unwrap();
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(token.len() >= 32, "{token}");
    let bearer = format!("Host: 127.0.0.1:4810\r\nAuthorization: Bearer {token}\r\n");
    let drop = r#"{"screen": "queue", "block": "q-sendmeter-17", "action": "drop", "values": {"fleet": "sendmeter", "issue": 17}}"#;
    let local = format!("Host: {first}\r\n");
    assert_eq!(ask(first, "POST", "/action", drop, &local), "401");
    assert_eq!(read(&c.state).unwrap().last().unwrap().data["op"], "add");

    let second = start(c);
    assert_eq!(
        ask(second, "GET", "/screens/queue", "", "Host: 127.0.0.1:4810\r\n"),
        "403"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), token);
    assert_eq!(ask(second, "GET", "/screens/queue", "", &bearer), "200");
    assert_eq!(ask(second, "POST", "/action", drop, &bearer), "200");
    assert_eq!(read(&c.state).unwrap().last().unwrap().data["op"], "rm");
}
