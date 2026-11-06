//! #548: a keychain read that waits on a macOS access dialog never blocks: `KEYCHAIN_LOCKED` within 5 s (a system
//! row in the thread, one `check` alert); a second message behind a stuck delivery is `not delivered`, not a hang;
//! `doctor` prints a prompting item's one-time fix.
mod common;
use common::*;
use herdsman::runner::{Out, Runner};
use herdsman::serve::Server;
use std::any::Any;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;
const SVC_A: &str = "herdsman/all/CLAUDE_CODE_OAUTH_TOKEN_A";

/// A keychain item behind a dialog nobody clicks: every call blocks for good.
struct Dialog;

impl keyring::credential::CredentialApi for Dialog {
    fn set_secret(&self, _: &[u8]) -> keyring::Result<()> {
        loop {
            std::thread::park();
        }
    }
    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        loop {
            std::thread::park();
        }
    }
    fn delete_credential(&self) -> keyring::Result<()> {
        loop {
            std::thread::park();
        }
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

/// `.0` for the tools and its clock; the secret store is herdsman's own, over a keyring that hangs.
struct Hung<'a>(&'a Fake);

impl Runner for Hung<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.0.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.0.run_in(prog, args, input)
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
    fn secret(&self, svc: &str) -> anyhow::Result<Option<String>> {
        keyring::set_default_credential_builder(Box::new(Dialog));
        herdsman::runner::store_get(self, svc)
    }
    fn set_secret(&self, svc: &str, value: Option<&str>) -> anyhow::Result<()> {
        keyring::set_default_credential_builder(Box::new(Dialog));
        herdsman::runner::store_set(self, svc, value)
    }
}

/// CONFIG with a Claude conductor on `[accounts]` A, and thread t1.
fn setup(name: &str) -> herdsman::Ctx {
    let d = tmpdir(name);
    let cfg = format!(
        "{CONFIG}\n[fleet.sendmeter.role.conductor]\nharness = \"claude\"\nmodel = \"opus\"\naccount = \"A\"\n\n[accounts]\nA = \"CLAUDE_CODE_OAUTH_TOKEN_A\"\n"
    );
    let c = ctx(&d, &cfg);
    let profile = c.config.with_file_name("claude").join("fleet-conductor");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Look"]).1,
        "t1\n"
    );
    c
}

/// herdr with no agents, ready to start a conductor; the host is `mini`.
fn herdr() -> Fake {
    Fake::default()
        .reply("hostname -s", "mini\n")
        .reply("codesign -dvvv", "CDHash=abc\n")
        .reply("herdr agent list", &agents(&[]))
        .reply(
            "herdr workspace create",
            r#"{"result":{"root_pane":{"pane_id":"w9:p1"}}}"#,
        )
        .reply("herdr workspace close", "")
        .reply("herdr pane run", "")
        .reply("herdr agent start", "")
        .reply("herdr agent prompt", "")
}

/// Post `text` to t1 from a paired phone (its delivery runs before `respond` returns); the event log after.
fn say(c: &herdsman::Ctx, r: &dyn Runner, text: &str) -> String {
    let s = Server::new(c, r, Some(ADDR.into()));
    let cookie = pair(&s, c, "Mozilla/5.0 (iPhone)", T);
    let body = format!("op=say&t=t1&text={text}");
    let req = format!(
        "POST /thread HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&req, T)).into_owned();
    assert!(res.ends_with("\r\n\r\nsent\n"), "{res}");
    herdsman::events::jsonl(&c.state).unwrap()
}

fn sleeps(r: &Fake) -> usize {
    r.calls.borrow().iter().filter(|c| *c == "sleep 1").count()
}

#[test]
fn a_hung_keychain_is_keychain_locked_in_5_s_with_a_system_row_and_one_alert() {
    let c = setup("keychain-hung");
    let r = herdr();
    let log = say(&c, &Hung(&r), "hello");
    let row = format!(
        r#""text":"✗ not delivered: REFUSED KEYCHAIN_LOCKED: approve herdsman in the Keychain dialog on mini, or run security set-generic-password-partition-list -S apple-tool:,apple:,unsigned:,cdhash:abc -s {SVC_A} -a herdsman""#
    );
    assert!(log.contains(&row), "{log}");
    assert_eq!(sleeps(&r), 5, "5 s on the clock, then refused: {:?}", r.calls.borrow());
    assert!(!r.called("herdr agent start"), "no conductor without its token");
    let alerts = || log_kinds(&c).iter().filter(|k| *k == "keychain-locked").count();
    run(&c, &r, &["check"]);
    run(&c, &r, &["check"]);
    assert_eq!(alerts(), 1, "check alerts once");
}

#[test]
fn a_second_message_behind_a_stuck_delivery_is_not_delivered_in_30_s() {
    let c = setup("keychain-lock");
    let dir = c.state.with_file_name("conductor").join("t1");
    std::fs::create_dir_all(&dir).unwrap();
    // The first message's delivery, stuck on the dialog, holds the thread's lock.
    let first = std::fs::File::create(dir.join(".deliver.lock")).unwrap();
    first.lock().unwrap();
    let r = herdr();
    let log = say(&c, &r, "second");
    assert!(
        log.contains(r#""text":"✗ not delivered: t1's previous message is still being delivered after 30 s""#),
        "{log}"
    );
    assert_eq!(sleeps(&r), 30, "{:?}", r.calls.borrow());
    assert!(!r.called("herdr agent start"), "{:?}", r.calls.borrow());
}

#[test]
fn doctor_prints_a_prompting_items_fix() {
    let d = tmpdir("keychain-doctor");
    let c = ctx(
        &d,
        &(CONFIG.to_string() + "\n[accounts]\nA = \"CLAUDE_CODE_OAUTH_TOKEN_A\"\n"),
    );
    let r = Fake::default().reply("codesign -dvvv", "CDHash=abc\n");
    let out = run(&c, &Hung(&r), &["doctor"]).1;
    let want = format!(
        "✗ account A — {SVC_A} prompts for Keychain access → security set-generic-password-partition-list -S apple-tool:,apple:,unsigned:,cdhash:abc -s {SVC_A} -a herdsman"
    );
    assert!(out.lines().any(|l| l == want), "{out}");
}

/// `security dump-keychain -a` for item A (another app's item first) with partition list `parts`.
fn dump(parts: &str) -> String {
    format!(
        "keychain: \"/Users/x/Library/Keychains/login.keychain-db\"\nclass: \"genp\"\nattributes:\n    \"svce\"<blob>=\"other\"\naccess: 1 entries\n    entry 0:\n        authorizations (1): partition_id\n        don't-require-password\n        description: apple:\n\
         keychain: \"/Users/x/Library/Keychains/login.keychain-db\"\nclass: \"genp\"\nattributes:\n    \"acct\"<blob>=\"herdsman\"\n    \"svce\"<blob>=\"{SVC_A}\"\naccess: 2 entries\n    entry 0:\n        authorizations (1): encrypt\n        don't-require-password\n        description: {SVC_A}\n        applications: <null>\n    entry 1:\n        authorizations (1): partition_id\n        don't-require-password\n        description: {parts}\n        applications: <null>\n"
    )
}

#[test]
fn doctor_flags_an_item_whose_partition_list_lacks_this_build_without_reading_it() {
    let d = tmpdir("keychain-partition");
    let c = ctx(
        &d,
        &(CONFIG.to_string() + "\n[accounts]\nA = \"CLAUDE_CODE_OAUTH_TOKEN_A\"\n"),
    );
    // The build before this one is in the list; this one (abc) is not, so a read would pop the dialog.
    let r = Fake::default().reply("codesign -dvvv", "CDHash=abc\n").reply(
        "security dump-keychain -a",
        &dump("apple-tool:, apple:, unsigned:, cdhash:old"),
    );
    let out = run(&c, &Hung(&r), &["doctor"]).1;
    let want = format!(
        "✗ keychain {SVC_A} — prompts for Keychain access (partition list lacks cdhash:abc) → security set-generic-password-partition-list -S apple-tool:,apple:,unsigned:,cdhash:abc -s {SVC_A} -a herdsman"
    );
    assert!(out.lines().any(|l| l == want), "{out}");
    assert_eq!(
        out.matches(SVC_A).count(),
        2,
        "one row, no read that would prompt: {out}"
    );
    assert_eq!(sleeps(&r), 0, "{:?}", r.calls.borrow());
    // Once the list admits this build, there is no keychain row and the item is read.
    let r = Fake::default().reply("codesign -dvvv", "CDHash=abc\n").reply(
        "security dump-keychain -a",
        &dump("apple-tool:, apple:, unsigned:, cdhash:abc"),
    );
    r.store.borrow_mut().insert(SVC_A.into(), "tok".into());
    let out = run(&c, &r, &["doctor"]).1;
    assert!(!out.contains("prompts for Keychain access"), "{out}");
    assert!(out.contains(&format!("account A — {SVC_A} in the store")), "{out}");
}

fn log_kinds(c: &herdsman::Ctx) -> Vec<String> {
    herdsman::events::read(&c.state)
        .unwrap()
        .into_iter()
        .map(|e| e.kind)
        .collect()
}
