//! #441: the main host's secret store: a round trip and the no-store refusal (#683: `accounts import` retired, and
//! `[accounts]` need no store).
mod common;
use common::*;

const TOKEN_A: &str = concat!("sk", "-ant-oat01-AAAA-secret");
const SVC_A: &str = "herdsman/all/CLAUDE_CODE_OAUTH_TOKEN_A";

/// CONFIG with `[accounts]` A and B.
fn setup(name: &str) -> herdsman::Ctx {
    let d = tmpdir(name);
    let cfg = CONFIG.to_string() + "\n[accounts]\nA = \"A\"\nB = \"B\"\n";
    ctx(&d, &cfg)
}

#[test]
fn an_old_account_token_goes_from_the_store_by_secret_rm() {
    let c = setup("round-trip");
    let r = Fake::default().stored(SVC_A, TOKEN_A);
    assert_eq!(run(&c, &r, &["secret", "rm", "all", "CLAUDE_CODE_OAUTH_TOKEN_A"]).0, 0);
    assert!(!r.store.borrow().contains_key(SVC_A));
}

#[test]
fn accounts_import_is_retired() {
    let c = setup("import-retired");
    assert_eq!(
        run(&c, &Fake::default(), &["accounts", "import"]).0,
        2,
        "no such subcommand"
    );
}

#[test]
fn a_main_host_without_a_store_refuses_to_start_only_for_a_secret_env() {
    let c = setup("no-store");
    let none = Fake {
        no_store: true,
        ..Fake::default()
    };
    // #683: `[accounts]` sign in with their own login: no store needed.
    assert_ne!(run(&c, &none, &["serve"]).0, 3);
    let secret = CONFIG.replace(
        "[fleet.sendmeter.role.impl]",
        "[fleet.sendmeter.env]\nAPI_KEY = \"secret:API_KEY\"\n\n[fleet.sendmeter.role.impl]",
    );
    let s = ctx(&tmpdir("no-store-secret"), &secret);
    assert_eq!(run(&s, &none, &["serve"]).0, 3, "REFUSED NO_STORE");
    assert_ne!(run(&s, &Fake::default(), &["serve"]).0, 3);
}
