mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::runner::{Out, Runner};
use serde_json::json;
use std::io::{BufRead, BufReader, Read, Write};

const NOW: u64 = 1_000_000;
const WT: &str = "/home/me/.herdr/worktrees/sendmeter/issue-12";
const TOKEN: &str = concat!("sk", "-ant-oat01-DDDD-secret");

fn setup(name: &str, limits: &str) -> (drover::Ctx, FakeShed, String, u16) {
    spawned_ago(name, limits, 900)
}

fn spawned_ago(name: &str, limits: &str, ago: u64) -> (drover::Ctx, FakeShed, String, u16) {
    use std::os::unix::fs::PermissionsExt;
    let port = upstream();
    let d = tmpdir(name);
    let tf = d.join("config/tokens/proxy");
    std::fs::create_dir_all(tf.parent().unwrap()).unwrap();
    std::fs::write(&tf, TOKEN).unwrap();
    std::fs::set_permissions(&tf, std::fs::Permissions::from_mode(0o600)).unwrap();
    let base = CONFIG
        .replace("harness  =  \"hermes\"", "harness = \"claude\"")
        .replace("[limits]\n", &format!("[limits]\n{limits}"));
    let toml = format!(
        "{base}\n[proxy]\nport = 9\nupstream = \"http://127.0.0.1:{port}\"\ntoken-file = \"{}\"\n\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n",
        tf.display()
    );
    let mut c = ctx(&d, &toml);
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "bazzite": {"tags": ["os:linux"], "home": "/home/me", "probed": NOW, "seen": NOW, "up": true},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let sh = FakeShed::attached(&c);
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let key = drover::proxy::new_key().unwrap();
    c.now = NOW - 1000;
    drover::proxy::mint(&c, &cfg, &key, ("sendmeter", 12, IMPL), "bazzite").unwrap();
    c.now = NOW;
    let d = json!({"role": "impl", "agent": IMPL, "harness": "claude", "pane": IMPL, "workspace": "",
                   "worktree": WT, "branch": "issue-12", "host": "bazzite", "machine": "bazzite"});
    append(&c.state, &Event::new(NOW - ago, "sendmeter", Some(12), "spawn", d)).unwrap();
    let port = proxy(&c);
    (c, sh, key, port)
}

fn upstream() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut c in l.incoming().flatten() {
            let mut rd = BufReader::new(c.try_clone().unwrap());
            let mut len = 0;
            loop {
                let mut h = String::new();
                rd.read_line(&mut h).unwrap();
                if let Some(v) = h.to_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
                if h.trim().is_empty() {
                    break;
                }
            }
            rd.read_exact(&mut vec![0; len]).unwrap();
            let b = "event: message_stop\ndata: {}\n\n";
            let _ = write!(c, "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n{b}", b.len());
        }
    });
    port
}

fn proxy(c: &drover::Ctx) -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let dir = c.config.parent().unwrap().parent().unwrap();
    let pc: &'static drover::Ctx = Box::leak(Box::new(ctx(dir, &std::fs::read_to_string(&c.config).unwrap())));
    let cfg: &'static drover::config::Config = Box::leak(Box::new(drover::config::Config::load(&c.config).unwrap()));
    let f: &'static Fake = Box::leak(Box::new(Fake::default()));
    std::thread::spawn(move || drover::proxy::serve_on(pc, cfg, f, &l, &|| NOW));
    port
}

fn call(port: u16, key: &str) -> (String, String) {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    let body = r#"{"model":"opus","messages":[]}"#;
    write!(s, "POST /v1/messages HTTP/1.1\r\nhost: x\r\nauthorization: Bearer {key}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut text = String::new();
    s.read_to_string(&mut text).unwrap();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    (head.lines().next().unwrap().to_string(), body.to_string())
}

fn main_host() -> Fake {
    Fake::default()
        .reply("sysctl -n vm.loadavg", "{ 0.4 1 1 }")
        .reply("sysctl -n hw.ncpu", "4")
        .reply("gh issue view", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply("git ", "")
        .reply("which shed", "/opt/shed/bin/shed\n")
}

fn dead(c: &drover::Ctx) -> usize {
    read(&c.state).unwrap().iter().filter(|e| e.kind == "lane-dead").count()
}

fn key_state(c: &drover::Ctx) -> String {
    let (code, list) = run(c, &Fake::default(), &["hosts", "keys"]);
    assert_eq!(code, 0, "{list}");
    let row = list.lines().find(|l| l.contains(IMPL)).unwrap();
    row.split_whitespace().nth(1).unwrap().to_string()
}

struct Unanswered<'a>(&'a Fake, i32);

impl Runner for Unanswered<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.run_in(prog, args, "")
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        let there = prog == "ssh" && args[0] != "-G" && args.iter().any(|a| a == "bazzite");
        let shed = args.last().is_some_and(|l| l.contains("'shed' 'connect'"));
        if there && (shed || self.1 == 124) {
            return Ok(Out {
                code: self.1,
                stderr: "shed: no answer".into(),
                ..Out::default()
            });
        }
        self.0.run_in(prog, args, input)
    }
    fn sleep(&self, _: u64) {}
}

#[test]
fn a_pass_that_cannot_read_a_remote_hosts_sessions_logs_no_lane_dead() {
    let (c, _sh, key, port) = setup("ldr-unread", "");
    let f = main_host();
    for _ in 0..2 {
        let (code, out) = run(&c, &Unanswered(&f, 1), &["check"]);
        assert_eq!(code, 0, "{out}");
    }
    assert_eq!(
        dead(&c),
        0,
        "a host whose session list failed says nothing about its lanes"
    );
    assert_eq!(key_state(&c), "live");
    let (status, body) = call(port, &key);
    assert!(status.contains("200"), "{status} {body}");
}

#[test]
fn a_tick_that_skips_a_remote_host_over_budget_logs_no_lane_dead() {
    let (c, _sh, key, port) = setup("ldr-budget", "slow_secs = 0\n");
    let f = main_host();
    let (code, out) = run(&c, &Unanswered(&f, 124), &["tick"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("tick: bazzite: over budget; skipped for the rest of this pass"),
        "{out}"
    );
    assert_eq!(
        dead(&c),
        0,
        "a host the pass skipped says nothing about its lanes: {out}"
    );
    assert_eq!(key_state(&c), "live");
    let (status, body) = call(port, &key);
    assert!(status.contains("200"), "{status} {body}");
}

#[test]
fn a_lane_logged_dead_and_then_seen_running_again_keeps_a_live_key() {
    let (c, _sh, key, port) = setup("ldr-back", "");
    let (f, bazzite) = (main_host(), FakeShed::new());
    let r = OverSsh {
        f: &f,
        hosts: vec![("bazzite", &bazzite)],
    };
    assert_eq!(run(&c, &r, &["check"]).0, 0);
    assert_eq!(dead(&c), 1);
    assert_eq!(key_state(&c), "ended");
    bazzite.lane(IMPL, "working", WT);
    look(&c, &r);
    for _ in 0..2 {
        let (code, out) = run(&c, &r, &["check"]);
        assert_eq!(code, 0, "{out}");
    }
    assert_eq!(key_state(&c), "live", "the lane runs on its host");
    let (status, body) = call(port, &key);
    assert!(
        status.contains("200") && body.contains("message_stop"),
        "{status} {body}"
    );
    bazzite.exit(IMPL);
    look(&c, &r);
    assert_eq!(run(&c, &r, &["check"]).0, 0);
    assert_eq!(dead(&c), 2, "a later real death is logged again");
    assert_eq!(key_state(&c), "ended");
}

#[test]
fn a_lane_whose_session_really_exited_on_an_answering_host_is_dead_and_its_key_ends() {
    let (c, _sh, key, port) = setup("ldr-exited", "");
    let (f, bazzite) = (main_host(), FakeShed::new());
    bazzite.lane(IMPL, "working", WT);
    bazzite.exit(IMPL);
    let r = OverSsh {
        f: &f,
        hosts: vec![("bazzite", &bazzite)],
    };
    for _ in 0..2 {
        assert_eq!(run(&c, &r, &["check"]).0, 0);
    }
    assert_eq!(dead(&c), 1);
    assert_eq!(key_state(&c), "ended");
    let (status, body) = call(port, &key);
    assert!(status.contains("401") && body.contains("ended"), "{status} {body}");
}

#[test]
fn a_probe_that_times_out_once_ends_no_fresh_lane_and_its_next_request_passes() {
    let (c, _sh, key, port) = spawned_ago("ldr-timeout", "", 120);
    let (code, out) = run(&c, &Unanswered(&main_host(), 124), &["check"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(dead(&c), 0, "a timed-out probe is unknown: {out}");
    assert_eq!(key_state(&c), "live");
    let (status, body) = call(port, &key);
    assert!(status.contains("200"), "{status} {body}");
}

#[test]
fn a_fresh_lane_its_answering_host_does_not_list_yet_is_dead_only_after_its_grace() {
    let (mut c, _sh, key, port) = spawned_ago("ldr-grace", "", 120);
    let (f, bazzite) = (main_host(), FakeShed::new());
    let r = OverSsh {
        f: &f,
        hosts: vec![("bazzite", &bazzite)],
    };
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(dead(&c), 0, "a lane two minutes old is not judged dead: {out}");
    assert_eq!(key_state(&c), "live");
    let (status, body) = call(port, &key);
    assert!(status.contains("200"), "{status} {body}");
    c.now = NOW + 600;
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        dead(&c),
        1,
        "past its grace, an answering host without it means gone: {out}"
    );
    assert_eq!(key_state(&c), "ended");
}
