//! #441: the main host's secret store: a round trip, the no-store refusal, `accounts import` checked by hash.
mod common;
use common::*;
use herdsman::events::read;
use herdsman::runner::{Out, Runner};
use sha2::{Digest, Sha256};

const TOKEN_A: &str = concat!("sk", "-ant-oat01-AAAA-secret");
const TOKEN_B: &str = concat!("sk", "-ant-oat01-BBBB-secret");
const SVC_A: &str = "herdsman/all/CLAUDE_CODE_OAUTH_TOKEN_A";
const SVC_B: &str = "herdsman/all/CLAUDE_CODE_OAUTH_TOKEN_B";

/// CONFIG with `[accounts]` A and B, and a `tokens.env` fixture holding both tokens.
fn setup(name: &str) -> (herdsman::Ctx, String) {
    let d = tmpdir(name);
    let cfg =
        CONFIG.to_string() + "\n[accounts]\nA = \"CLAUDE_CODE_OAUTH_TOKEN_A\"\nB = \"CLAUDE_CODE_OAUTH_TOKEN_B\"\n";
    let env = d.join("tokens.env");
    let text = format!("CLAUDE_CODE_OAUTH_TOKEN_A={TOKEN_A}\nexport CLAUDE_CODE_OAUTH_TOKEN_B=\"{TOKEN_B}\"\n");
    std::fs::write(&env, text).unwrap();
    (ctx(&d, &cfg), env.display().to_string())
}

fn sha(v: &str) -> Vec<u8> {
    Sha256::digest(v.as_bytes()).to_vec()
}

#[test]
fn import_copies_each_token_into_the_store_with_matching_hashes() {
    let (c, env) = setup("import");
    let r = Fake::default();
    let (code, out) = run(&c, &r, &["accounts", "import", &env]);
    assert_eq!(code, 0, "{out}");
    let store = r.store.borrow();
    assert_eq!(sha(&store[SVC_A]), sha(TOKEN_A));
    assert_eq!(sha(&store[SVC_B]), sha(TOKEN_B));
    assert!(out.contains(&format!("A        ✓ {SVC_A} (sha256 matches)")), "{out}");
    assert!(out.contains(&format!("B        ✓ {SVC_B} (sha256 matches)")), "{out}");
    assert!(!out.contains(TOKEN_A) && !out.contains(TOKEN_B), "{out}");
    let log = format!("{:?}", read(&c.state).unwrap());
    assert!(!log.contains(TOKEN_A) && !log.contains(TOKEN_B), "token in the log");
    assert!(std::path::Path::new(&env).exists(), "import never deletes tokens.env");
}

/// A store that keeps something other than what it was given.
struct Mangled<'a>(&'a Fake);

impl Runner for Mangled<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.0.run(prog, args)
    }
    fn secret(&self, svc: &str) -> anyhow::Result<Option<String>> {
        self.0.secret(svc)
    }
    fn set_secret(&self, svc: &str, value: Option<&str>) -> anyhow::Result<()> {
        self.0.set_secret(svc, value.map(|v| format!("{v}x")).as_deref())
    }
}

#[test]
fn import_fails_when_the_stored_copy_hashes_differently() {
    let (c, env) = setup("import-bad");
    let f = Fake::default();
    let (code, _) = run(&c, &Mangled(&f), &["accounts", "import", &env]);
    assert_eq!(code, 1);
    assert!(
        read(&c.state).unwrap().iter().all(|e| e.kind != "secret-set"),
        "a failed check logged"
    );
}

#[test]
fn a_secret_round_trips_through_the_store() {
    let (c, env) = setup("round-trip");
    let r = Fake::default();
    assert_eq!(run(&c, &r, &["accounts", "import", &env]).0, 0);
    let row = |r: &Fake| {
        let out = run(&c, r, &["doctor"]).1;
        out.lines().find(|l| l.contains("account A")).unwrap().to_string()
    };
    assert!(row(&r).starts_with('✓'), "{}", row(&r));
    assert_eq!(run(&c, &r, &["secret", "rm", "all", "CLAUDE_CODE_OAUTH_TOKEN_A"]).0, 0);
    assert!(!r.store.borrow().contains_key(SVC_A));
    assert!(row(&r).starts_with('✗'), "{}", row(&r));
}

#[test]
fn a_main_host_without_a_store_refuses_to_start() {
    let (c, _) = setup("no-store");
    let none = Fake {
        no_store: true,
        ..Fake::default()
    };
    assert_eq!(run(&c, &none, &["serve"]).0, 3, "REFUSED NO_STORE");
    assert_ne!(run(&c, &Fake::default(), &["serve"]).0, 3);
    // A config that names no secret needs no store (the README's quick start).
    let plain = ctx(&tmpdir("no-store-plain"), CONFIG);
    assert_ne!(run(&plain, &none, &["serve"]).0, 3);
}
