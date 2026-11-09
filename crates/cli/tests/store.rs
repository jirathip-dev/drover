//! #707: herdsman keeps no secret store: `serve` starts with none, a `secret:` env comes from its 0600 file,
//! `secret rm` deletes that file, and `migrate` prints the commands that delete the old Keychain items.
mod common;
use common::*;
use std::io::{Read, Write};

/// CONFIG with a `secret:` env and `[accounts]` A and B, the configs that once made serve probe the store.
fn setup(name: &str) -> herdsman::Ctx {
    let d = tmpdir(name);
    let cfg = CONFIG.replace(
        "[fleet.sendmeter.role.impl]",
        "[fleet.sendmeter.env]\nAPI_KEY = \"secret:API_KEY\"\n\n[fleet.sendmeter.role.impl]",
    ) + "\n[accounts]\nA = \"A\"\nB = \"B\"\n";
    ctx(&d, &cfg)
}

#[test]
fn serve_starts_with_no_secret_store() {
    let c = setup("no-store");
    // The runner has no store (`Runner::secret` refuses): serve gets past its old probe to the server itself.
    let r = Fake::default();
    assert!(herdsman::runner::Runner::secret(&r, "herdsman/all/X").is_err());
    // Exit 1 is the test binary's missing server ("no serve", on stderr); the old probe refused 3 `NO_STORE`.
    let (code, out) = run(&c, &r, &["serve"]);
    assert_eq!(code, 1, "{out}");
    assert!(!out.contains("REFUSED"), "{out}");
    // And the board answers over a real socket with that config.
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || herdsman::serve::serve_on(&c2, &Fake::default(), l, true));
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let mut page = String::new();
    s.read_to_string(&mut page).unwrap();
    assert!(page.starts_with("HTTP/1.1 200 OK"), "{page}");
}

#[test]
fn secret_rm_deletes_the_secret_file() {
    let c = setup("secret-rm");
    let f = secret_file(&c, "API_KEY", "v", 0o600);
    assert_eq!(run(&c, &Fake::default(), &["secret", "rm", "all", "API_KEY"]).0, 0);
    assert!(!f.exists());
    let (code, out) = run(&c, &Fake::default(), &["secret", "rm", "all", "../fleet.toml"]);
    assert_eq!(code, 3, "{out}");
    assert!(c.config.exists());
}

#[test]
fn migrate_leaves_the_keychain_alone_and_prints_how_to_delete_its_items() {
    let c = setup("migrate-keychain");
    let e = herdsman::events::Event::new(
        1,
        "sendmeter",
        None,
        "secret-set",
        serde_json::json!({"name": "API_KEY"}),
    );
    herdsman::events::append(&c.state, &e).unwrap();
    let r = Fake::default();
    let (code, out) = run(&c, &r, &["migrate"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("security delete-generic-password -a herdsman -s herdsman/sendmeter/API_KEY"),
        "{out}"
    );
    assert!(!r.called("security"), "nothing deleted: {:?}", r.calls.borrow());
}
