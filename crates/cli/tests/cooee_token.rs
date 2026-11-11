mod common;
use common::*;
use herdsman::events::read;
use herdsman::serve::Server;

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
