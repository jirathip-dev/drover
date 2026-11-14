//! #710: `check` counts `security find-generic-password` readers waiting over 60 s and a SecurityAgent up over 10 min;
//! more than `[limits] keychain_readers` (3) alerts `keychain-jammed` once per episode, with no argument values;
//! `reap_keychain_readers = true` kills those readers, then that SecurityAgent (never securityd); doctor shows the count.
mod common;
use common::*;

const PS: &str = "ps -axo pid=,ppid=,etime=,command=";

/// launchd, securityd and a 31 h SecurityAgent; readers 400, 401, 402 (and 405 when `jam`) over 60 s, 403 young, 404
/// another `security` verb. Arguments carry values that must never reach an alert.
fn ps(jam: bool) -> String {
    let mut s = String::from(
        "    1     0 30-00:00:00 /sbin/launchd
  200     1    01:00:00 /usr/sbin/securityd -i
  300     1 01-07:00:00 /System/Library/Frameworks/Security.framework/Versions/A/MachServices/SecurityAgent.bundle/Contents/MacOS/SecurityAgent
  400     1    01:05:00 /usr/bin/security find-generic-password -s Claude-Code-credentials -a acctNAME -w
  401   500       10:00 security find-generic-password -s drover/sendmeter/API_KEY -w
  402   500       02:00 /usr/bin/security find-generic-password -s PLAINsvc -w
  403     1       00:30 /usr/bin/security find-generic-password -s young -w
  404     1       05:00 /usr/bin/security find-internet-password -s other
  500     1    02:00:00 /opt/homebrew/bin/node /Users/guy/cooee/poll.js --token=S3CRETvalue
",
    );
    if jam {
        s += "  405   500       03:00 /usr/bin/security find-generic-password -s FOURTHsvc -w\n";
    }
    s
}

fn fake(jam: bool) -> Fake {
    Fake::default().reply(PS, &ps(jam)).reply("kill ", "")
}

fn jammed(c: &drover::Ctx) -> usize {
    log_kinds(c).iter().filter(|k| *k == "keychain-jammed").count()
}

#[test]
fn three_readers_say_nothing() {
    let c = ctx(&tmpdir("kc-jam-below"), CONFIG);
    let r = fake(false);
    let _sh = FakeShed::new();
    _sh.attach(&c);
    run(&c, &r, &["check"]);
    assert!(r.called(PS), "check reads ps: {:?}", r.calls.borrow());
    assert_eq!(jammed(&c), 0);
    assert!(!r.called("kill "), "{:?}", r.calls.borrow());
}

#[test]
fn a_jam_alerts_once_per_episode_with_pids_ages_parents_and_the_fix_but_no_secret() {
    let c = ctx(&tmpdir("kc-jam-once"), CONFIG);
    run(&c, &fake(true), &["check"]);
    let _sh = FakeShed::new();
    _sh.attach(&c);
    run(&c, &fake(true), &["check"]);
    assert_eq!(jammed(&c), 1, "none while it persists");
    let log = drover::events::jsonl(&c.state).unwrap();
    let e = log.lines().find(|l| l.contains("keychain-jammed")).unwrap();
    for want in [
        "400",
        "401",
        "402",
        "405",
        "launchd",
        "node",
        "SecurityAgent",
        "1h",
        "kill 400 401 402 405",
    ] {
        assert!(e.contains(want), "{want}: {e}");
    }
    assert!(
        e.contains(r#"pgrep -x SecurityAgent"#),
        "the fix from troubleshooting §2: {e}"
    );
    for secret in [
        "Claude-Code-credentials",
        "API_KEY",
        "PLAINsvc",
        "FOURTHsvc",
        "S3CRET",
        "poll.js",
        " -w",
        "acctNAME",
    ] {
        assert!(!log.contains(secret), "{secret} leaked: {log}");
    }
    assert!(
        !e.contains("403") && !e.contains("404"),
        "young or other verbs are no readers: {e}"
    );
    // The jam clears, then comes back: a new episode, a new alert.
    let r = fake(false);
    run(&c, &r, &["check"]);
    assert_eq!(jammed(&c), 1);
    run(&c, &fake(true), &["check"]);
    assert_eq!(jammed(&c), 2, "a new episode alerts again");
}

#[test]
fn reaping_is_off_by_default_and_kills_only_old_readers_then_the_agent() {
    let c = ctx(&tmpdir("kc-jam-off"), CONFIG);
    let r = fake(true);
    let _sh = FakeShed::new();
    _sh.attach(&c);
    run(&c, &r, &["check"]);
    assert!(!r.called("kill "), "off by default: {:?}", r.calls.borrow());

    let cfg = CONFIG.replace("[limits]\n", "[limits]\nreap_keychain_readers = true\n");
    let c = ctx(&tmpdir("kc-jam-reap"), &cfg);
    let r = fake(true);
    let _sh = FakeShed::new();
    _sh.attach(&c);
    run(&c, &r, &["check"]);
    let kills: Vec<String> = r
        .calls
        .borrow()
        .iter()
        .filter(|c| c.starts_with("kill "))
        .cloned()
        .collect();
    assert_eq!(
        kills,
        ["kill 400 401 402 405", "kill 300"],
        "readers, then SecurityAgent"
    );
    assert!(
        log_kinds(&c).iter().any(|k| k == "keychain-reap"),
        "{:?}",
        log_kinds(&c)
    );
}

#[test]
fn doctor_shows_the_reader_count() {
    let c = ctx(&tmpdir("kc-jam-doctor"), CONFIG);
    let (_, out) = run(&c, &fake(true), &["doctor"]);
    let row = out
        .lines()
        .find(|l| l.contains("keychain readers"))
        .unwrap_or_else(|| panic!("{out}"));
    assert!(row.starts_with('!') && row.contains("4 waiting"), "{row}");
    let (_, out) = run(&c, &fake(false), &["doctor"]);
    let row = out
        .lines()
        .find(|l| l.contains("keychain readers"))
        .unwrap_or_else(|| panic!("{out}"));
    assert!(row.starts_with('✓') && row.contains("3 waiting"), "{row}");
    assert!(!out.contains("S3CRET") && !out.contains("API_KEY"), "{out}");
}

fn log_kinds(c: &drover::Ctx) -> Vec<String> {
    drover::events::read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .map(|e| e.kind)
        .collect()
}
