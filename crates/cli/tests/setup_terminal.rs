//! #303: setup terminals: only configured names run, a remote host's pane goes over `--machine`, typed text never
//! reaches the event log, and a setup closes on its 10-minute timeout (lanes are hook-blocked: tests/hook.rs).
mod common;
use common::*;
use drover::events::read;
use drover::serve::Server;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;
const WS: &str = r#"{"result": {"root_pane": {"pane_id": "w9:p1"}}}"#;

fn config() -> String {
    format!("{CONFIG}\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n\n[[setup]]\nname = \"claude B\"\ncmd = \"claude /login\"\nenv = {{CLAUDE_CONFIG_DIR = \"~/accounts/B\"}}\n")
}

fn log(c: &drover::Ctx) -> String {
    drover::events::jsonl(&c.state).unwrap_or_default()
}

#[test]
fn only_configured_names_run_and_a_remote_pane_uses_machine() {
    let c = ctx(&tmpdir("setup-names"), &config());
    let r = Fake::default()
        .reply("herdr machine list --json", BAZZITE)
        .reply("herdr", WS);
    let (code, out) = run(&c, &r, &["setup", "bazzite", "rm -rf ~"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED NO_SUCH_SETUP") && !r.called("herdr"), "{out}");
    assert_eq!(
        run(&c, &r, &["setup", "nowhere", "gh"]).0,
        3,
        "an unknown host is refused"
    );
    let (code, out) = run(&c, &r, &["setup", "bazzite", "claude B"]);
    assert_eq!(code, 0, "{out}");
    assert!(r.called("herdr --machine bazzite workspace create --cwd /tmp --label drover-setup"));
    assert!(
        r.called("herdr --machine bazzite pane run w9:p1 cd; CLAUDE_CONFIG_DIR=~/'accounts/B' claude /login; exit"),
        "{:?}",
        r.calls.borrow()
    );
    assert!(run(&c, &r, &["setup", "mini", "gh"]).1.contains("w9:p1"));
    assert!(
        r.called("herdr pane run w9:p1 cd; gh auth login; exit"),
        "a built-in, on this host: no --machine"
    );
    let e = read(&c.state).unwrap();
    assert_eq!(
        (e.len(), e[0].kind.as_str(), e[0].str("machine")),
        (2, "setup", Some("bazzite"))
    );
    assert!(!log(&c).contains("/login"), "the command line is not logged");
}

#[test]
fn the_board_starts_by_name_types_unlogged_and_shows_the_screen() {
    let c = ctx(&tmpdir("setup-web"), &config());
    let screen =
        "Open this link to sign in:\nhttps://claude.ai/oauth?code=abc&state=12345678\n90abcdef\nPaste code here >\n";
    let r = Fake::default()
        .reply("herdr --machine bazzite pane read", screen)
        .reply("herdr machine list --json", BAZZITE)
        .reply("herdr", WS);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let post = |path: &str, body: &str, cookie: &str| {
        let req = format!(
            "POST {path} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        String::from_utf8_lossy(&s.respond(&req, T)).into_owned()
    };
    assert!(
        post("/setup", "host=bazzite&name=gh", "").starts_with("HTTP/1.1 401"),
        "unpaired"
    );
    let cookie = pair(&s, &c, "", T);
    assert!(post("/setup", "host=bazzite&name=claude+%2Flogin", &cookie).contains("REFUSED NO_SUCH_SETUP"));
    let res = post("/setup", "host=bazzite&name=claude+B", &cookie);
    assert!(res.starts_with("HTTP/1.1 200") && res.ends_with("w9:p1\n"), "{res}");
    let res = post("/send", "setup=w9%3Ap1&text=sk-SECRET-CODE-4242", &cookie);
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(r.called("herdr --machine bazzite pane send-text w9:p1 sk-SECRET-CODE-4242"));
    assert!(r.called("herdr --machine bazzite pane send-keys w9:p1 Enter"));
    assert!(!log(&c).contains("SECRET"), "typed input is never in the event log");
    let res = String::from_utf8_lossy(&s.respond(
        &format!("GET /lane?setup=w9%3Ap1 HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}\r\n"),
        T + 5,
    ))
    .into_owned();
    let j: serde_json::Value = serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    assert_eq!(
        j["urls"][0], "https://claude.ai/oauth?code=abc&state=1234567890abcdef",
        "a wrapped URL is joined"
    );
    assert_eq!(
        (j["closed"].as_bool(), j["lines"][3].as_str()),
        (Some(false), Some("Paste code here >"))
    );
    let page = host_tab(&s, ADDR, T);
    assert!(page.contains("data-su=\"claude B\" data-host=\"bazzite\"") && page.contains("data-su=\"codex\""));
}

#[test]
fn a_setup_closes_on_its_timeout() {
    let c = ctx(&tmpdir("setup-timeout"), &config());
    let r = Fake::default()
        .reply("herdr machine list --json", BAZZITE)
        .reply("herdr", WS);
    assert_eq!(run(&c, &r, &["setup", "bazzite", "gh"]).0, 0);
    let at = |now| drover::Ctx { now, ..c.clone() };
    drover::setup::reap(&at(T + 599), &r, &read(&c.state).unwrap()).unwrap();
    assert!(!r.called("herdr --machine bazzite workspace close"));
    drover::setup::reap(&at(T + 600), &r, &read(&c.state).unwrap()).unwrap();
    assert_eq!(r.count("herdr --machine bazzite workspace close w9"), 1);
    let e = read(&c.state).unwrap();
    assert_eq!((e[1].kind.as_str(), e[1].str("why")), ("setup-closed", Some("timeout")));
    drover::setup::reap(&at(T + 900), &r, &e).unwrap();
    assert_eq!(r.count("herdr --machine bazzite workspace close"), 1, "closed once");
    assert!(
        drover::setup::send(&at(T + 900), &r, "w9:p1", "x").is_err(),
        "no typing into a closed setup"
    );
}
