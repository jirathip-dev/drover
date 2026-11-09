//! #713: the 11-08 outage as a test. The login Keychain stops answering: `security` and `codesign` never return and
//! the OS store's every call blocks. `doctor`, `serve` (startup through its first page render), `tick` and `check`
//! each still finish within 10 s of wall-clock time; doctor shows the timed-out row, serve still serves the board.
mod common;
use common::*;
use herdsman::runner::{Out, Runner};
use serde_json::json;
use std::any::Any;
use std::io::{Read, Write};
use std::time::Duration;

/// The tools that hang with the Keychain.
const WEDGED: [&str; 2] = ["security", "codesign"];

/// A keychain item behind a dialog nobody clicks: every call blocks for good.
struct Dialog;

impl keyring::credential::CredentialApi for Dialog {
    fn set_secret(&self, _: &[u8]) -> keyring::Result<()> {
        hang()
    }
    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        hang()
    }
    fn delete_credential(&self) -> keyring::Result<()> {
        hang()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl keyring::credential::CredentialBuilderApi for Dialog {
    fn build(&self, _: Option<&str>, _: &str, _: &str) -> keyring::Result<Box<keyring::credential::Credential>> {
        Ok(Box::new(Dialog))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

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
        self.0.calls.borrow_mut().push(format!("{prog} {}", args.join(" ")));
        Ok(Self::cut(
            prog,
            args,
            Option::<Duration>::from(herdsman::runner::limit(prog, args)),
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
    fn secret(&self, svc: &str) -> anyhow::Result<Option<String>> {
        herdsman::runner::store_get(self, svc)
    }
    fn set_secret(&self, svc: &str, value: Option<&str>) -> anyhow::Result<()> {
        herdsman::runner::store_set(self, svc, value)
    }
}

/// A Claude fleet with a `secret:` env and account A's usage meter (its login is read with `security`), and #12's
/// impl lane on A (check confirms its usage-limit banner from the meter).
fn setup(name: &str) -> herdsman::Ctx {
    keyring::set_default_credential_builder(Box::new(Dialog));
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
    let ev = herdsman::events::Event::new(c.now - 900, "sendmeter", Some(12), "spawn", d);
    herdsman::events::append(&c.state, &ev).unwrap();
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

/// `herdsman <args>` against the wedged Keychain, within 10 s: (exit code, output, the wedged calls made).
fn cli(name: &str, args: &'static [&'static str]) -> (i32, String, Vec<String>) {
    let c = setup(name);
    in_10_s(&format!("herdsman {}", args.join(" ")), move || {
        let r = wedged();
        let (code, out) = run(&c, &r, args);
        let calls = r.0.calls.borrow().clone();
        let hung = calls
            .into_iter()
            .filter(|l| WEDGED.iter().any(|w| l.starts_with(w)))
            .collect();
        (code, out, hung)
    })
}

#[test]
fn doctor_finishes_in_10_s_and_shows_the_timed_out_row() {
    let (_, out, hung) = cli("wedged-doctor", &["doctor"]);
    if cfg!(target_os = "macos") {
        assert!(hung.iter().any(|l| l.starts_with("codesign")), "{hung:?}");
        let row = out
            .lines()
            .find(|l| l.contains("signature — "))
            .unwrap_or_else(|| panic!("{out}"));
        assert!(row.starts_with('✗') && row.contains("timed out"), "{row}");
    }
}

#[test]
fn check_finishes_in_10_s() {
    let (code, out, hung) = cli("wedged-check", &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(!hung.is_empty(), "check reads the meter's login with security");
}

#[test]
fn tick_finishes_in_10_s() {
    let (_, out, hung) = cli("wedged-tick", &["tick"]);
    assert!(
        !hung.is_empty(),
        "tick's check reads the meter's login with security: {out}"
    );
}

#[test]
fn serve_starts_and_serves_the_board_in_10_s() {
    let c: &'static herdsman::Ctx = Box::leak(Box::new(setup("wedged-serve")));
    let page = in_10_s("serve", move || {
        let r: &'static Wedged = Box::leak(Box::new(wedged()));
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || herdsman::serve::serve_on(c, r, l, false));
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
