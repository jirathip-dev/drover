//! #713: the 11-08 outage as a test. The login Keychain stops answering: `security` and `codesign` never return.
//! `doctor`, `serve` (startup through its first page render), `tick` and `check` each still finish within 10 s of
//! wall-clock time, and serve still serves the board. #707 retired the store and the signing step, so `lsof`, which
//! all four still call (doctor's port probe, the host's cwd scan) and which hangs the same way on a stuck mount,
//! stands in for the next tool that stops answering.
mod common;
use common::*;
use drover::runner::{Out, Runner};
use serde_json::json;
use std::io::{Read, Write};
use std::time::Duration;

/// The tools that hang with the Keychain, and `lsof`, the one of the next kind that all four commands call.
const WEDGED: [&str; 3] = ["security", "codesign", "lsof"];

fn hang() -> ! {
    loop {
        std::thread::park();
    }
}

/// `.0`, except that a `WEDGED` program never answers. As `Real`, a call is cut at its limit (`run_for`'s, else
/// `runner::limit`'s): exit 124 at once (the limit is virtual time); a call with no limit blocks for good.
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

/// A Claude fleet with a `secret:` env and account A's usage meter, and #12's
/// impl lane on A (check confirms its usage-limit banner from the meter).
fn setup(name: &str) -> drover::Ctx {
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
    let d = json!({"role": "impl", "agent": IMPL, "pane": "w9:p12", "worktree": wt, "branch": "issue-12",
                   "harness": "claude", "account": "A"});
    let ev = drover::events::Event::new(c.now - 900, "sendmeter", Some(12), "spawn", d);
    drover::events::append(&c.state, &ev).unwrap();
    c
}

fn wedged() -> Wedged {
    Wedged(
        Fake::default()
            .reply("hostname -s", "mini\n")
            .reply(
                "herdr status",
                "client:\n  version: 0.9.5\n\nserver:\n  status: running\n  version: 0.9.5\n",
            )
            .reply("herdr agent list", &agents(&[(IMPL, "idle", "/x", "w9:p12")]))
            .reply("herdr pane read w9:p12", "Claude usage limit reached.")
            .reply("sysctl -n vm.loadavg", "{ 0.4 1 1 }")
            .reply("sysctl -n hw.ncpu", "4")
            .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n"),
    )
}

/// `f` on its own thread; its result, or a failure once 10 s of wall-clock time have passed.
fn in_10_s<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || tx.send(f()));
    match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(v) => v,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("{what}: still running after 10 s (wedged)"),
        Err(e) => panic!("{what}: {e}"),
    }
}

/// `drover <args>` against the wedged Keychain, within 10 s: (exit code, output).
fn cli(name: &str, args: &'static [&'static str]) -> (i32, String) {
    let c = setup(name);
    in_10_s(&format!("drover {}", args.join(" ")), move || run(&c, &wedged(), args))
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
    let c: &'static drover::Ctx = Box::leak(Box::new(setup("wedged-serve")));
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
