mod common;
use common::*;
use drover::runner::{Out, Runner};
use serde_json::json;
use std::io::{Read, Write};
use std::time::Duration;

const WEDGED: [&str; 3] = ["security", "codesign", "lsof"];

fn hang() -> ! {
    loop {
        std::thread::park();
    }
}

struct Wedged(Fake);

impl Wedged {
    fn cut(prog: &str, args: &[String], limit: Option<Duration>) -> Out {
        let Some(limit) = limit else { hang() };
        let stderr = format!("{prog} {}: no answer in {limit:?}", args.join(" "));
        Out {
            code: 124,
            stderr,
            ..Out::default()
        }
    }
}

impl Runner for Wedged {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.run_in(prog, args, "")
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        if !WEDGED.contains(&prog) {
            return self.0.run_in(prog, args, input);
        }
        Ok(Self::cut(
            prog,
            args,
            Option::<Duration>::from(drover::runner::limit(prog, args)),
        ))
    }
    fn run_for(&self, prog: &str, args: &[String], input: &str, limit: Duration) -> anyhow::Result<Out> {
        if !WEDGED.contains(&prog) {
            return self.0.run_for(prog, args, input, limit);
        }
        Ok(Self::cut(prog, args, Some(limit)))
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
}

fn setup(name: &str) -> (drover::Ctx, FakeShed) {
    let d = tmpdir(name);
    let cfg = CONFIG.replace(
        "harness  =  \"hermes\"    # toggled in tests",
        "harness = \"claude\"\naccounts = [\"A\"]",
    ) + &format!(
        "\n[fleet.sendmeter.env]\nAPI_KEY = \"secret:API_KEY\"\n\n[accounts]\nA = \"CLAUDE_CODE_OAUTH_TOKEN_A\"\n\n[accounts.usage]\nA = \"{}\"\n",
        d.join("logins/A").display()
    );
    let c = ctx(&d, &cfg);
    let wt = c.worktrees.join("sendmeter/issue-12");
    let d = json!({"role": "impl", "agent": IMPL, "pane": IMPL, "worktree": wt, "branch": "issue-12",
                   "harness": "claude", "account": "A"});
    let ev = drover::events::Event::new(c.now - 900, "sendmeter", Some(12), "spawn", d);
    drover::events::append(&c.state, &ev).unwrap();
    let sh = FakeShed::attached(&c);
    sh.lane(IMPL, "idle", "/x");
    sh.screen(IMPL, "Claude usage limit reached.");
    (c, sh)
}

fn wedged() -> Wedged {
    Wedged(
        Fake::default()
            .reply("hostname -s", "mini\n")
            .reply(
                "herdr status",
                "client:\n  version: 0.9.5\n\nserver:\n  status: running\n  version: 0.9.5\n",
            )
            .reply("sysctl -n vm.loadavg", "{ 0.4 1 1 }")
            .reply("sysctl -n hw.ncpu", "4")
            .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n"),
    )
}

fn in_10_s<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || tx.send(f()));
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(v) => v,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("{what}: still running after 10 s (wedged)"),
        Err(e) => panic!("{what}: {e}"),
    }
}

fn cli(name: &str, args: &'static [&'static str]) -> (i32, String) {
    let (c, sh) = setup(name);
    in_10_s(&format!("drover {}", args.join(" ")), move || {
        let r = run(&c, &wedged(), args);
        drop(sh);
        r
    })
}

#[test]
fn doctor_finishes_in_10_s() {
    cli("wedged-doctor", &["doctor"]);
}

#[test]
fn check_finishes_in_10_s() {
    let (code, out) = cli("wedged-check", &["check"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn tick_finishes_in_10_s() {
    cli("wedged-tick", &["tick"]);
}

#[test]
fn serve_starts_and_serves_the_board_in_10_s() {
    let (c, sh) = setup("wedged-serve");
    let (c, _sh): (&'static drover::Ctx, &'static FakeShed) = (Box::leak(Box::new(c)), Box::leak(Box::new(sh)));
    let page = in_10_s("serve", move || {
        let r: &'static Wedged = Box::leak(Box::new(wedged()));
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || drover::serve::serve_on(c, r, l, false));
        let get = |path: &str| {
            let mut s = std::net::TcpStream::connect(addr).unwrap();
            s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
                .unwrap();
            let mut res = String::new();
            s.read_to_string(&mut res).unwrap();
            assert!(res.starts_with("HTTP/1.1 200"), "{path}: {res}");
            res
        };
        let page = get("/");
        get("/api/v1/state");
        get("/api/v1/host");
        page
    });
    assert!(page.contains("<html") || page.contains("<!doctype"), "{page}");
}
