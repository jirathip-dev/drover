mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::json;

fn setup(name: &str, fake: Fake) -> (drover::Ctx, Fake, FakeShed) {
    let cfg = CONFIG.replace(
        "[limits]",
        "[fleet.drover]\nrepo = \"jirathip-dev/herdsman\"\nlocal = \"/tmp/x\"\nintegration = \"staging\"\n\n[limits]",
    );
    let c = ctx(&tmpdir(name), &cfg);
    let r = fake
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", &json!({"title": "t", "body": "b"}).to_string())
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("git -C", "");
    let sh = FakeShed::new();
    sh.attach(&c);
    (c, r, sh)
}

fn ok(c: &drover::Ctx, r: &Fake, args: &[&str]) -> String {
    let (code, out) = run(c, r, args);
    assert_eq!(code, 0, "{args:?}: {out}");
    out
}

fn drive(c: &drover::Ctx, r: &Fake, secs: u64) -> Vec<String> {
    let c = drover::Ctx {
        now: c.now + secs,
        ..c.clone()
    };
    let before = read(&c.state).unwrap().len();
    ok(&c, r, &["drive"]);
    let new = read(&c.state).unwrap().to_vec().split_off(before);
    let steps = new.iter().filter(|e| e.kind == "driven" && e.issue == Some(12));
    steps.map(|e| e.str("step").unwrap().to_string()).collect()
}

#[test]
fn another_fleets_blocker_holds_the_start_until_its_merged_event() {
    let (c, r, sh) = setup("xrepo-fleet", Fake::default());
    ok(&c, &r, &["queue", "sendmeter", "add", "12"]);
    ok(&c, &r, &["queue", "sendmeter", "block", "12", "--on", "drover#633"]);
    let (code, out) = run(&c, &r, &["queue", "sendmeter", "block", "12", "--on", "nofleet#1"]);
    assert_ne!(code, 0, "an unknown fleet is refused: {out}");
    let ls = ok(&c, &r, &["queue", "sendmeter", "ls"]);
    assert!(ls.contains("#12    queued  blocked by drover#633"), "{ls}");
    assert_eq!(drive(&c, &r, 0), [] as [std::string::String; 0]);
    assert!(drive(&c, &r, 3600).is_empty(), "never while it is open");
    assert!(sh.all("run").is_empty(), "no lane started");
    let d = json!({"pr": "640", "merge_sha": "abc", "into": "staging"});
    append(&c.state, &Event::new(c.now, "drover", Some(633), "merged", d)).unwrap();
    assert_eq!(drive(&c, &r, 3700), ["start"]);
}

#[test]
fn a_blocker_in_another_repo_is_released_once_it_reads_closed() {
    let view = "gh issue view 87 -R jirathip-dev/cooee --json state";
    let fake = Fake::default().once(view, 0, r#"{"state":"OPEN"}"#);
    let (c, r, _sh) = setup("xrepo-gh", fake.reply(view, r#"{"state":"CLOSED"}"#));
    ok(&c, &r, &["queue", "sendmeter", "add", "12"]);
    let on = ["--on", "drover#633", "--on", "jirathip-dev/cooee#87"];
    ok(&c, &r, &[&["queue", "sendmeter", "block", "12"][..], &on].concat());
    let ls = ok(&c, &r, &["queue", "sendmeter", "ls"]);
    assert!(ls.contains("blocked by drover#633, cooee#87"), "{ls}");
    let v: serde_json::Value = serde_json::from_str(&ok(&c, &r, &["queue", "sendmeter", "ls", "--json"])).unwrap();
    assert_eq!(v[0]["blocked_ext"], json!(["drover#633", "jirathip-dev/cooee#87"]));
    ok(&c, &r, &["queue", "sendmeter", "unblock", "12", "--on", "drover#633"]);
    assert!(drive(&c, &r, 0).is_empty(), "reads OPEN");
    assert_eq!(r.count(view), 1);
    let next = run(&c, &r, &["next", "sendmeter"]);
    assert_eq!(next, (2, "#12 is blocked by cooee#87\n".into()));
    assert_eq!(drive(&c, &r, 200), [] as [std::string::String; 0]);
    assert_eq!(r.count(view), 1, "one read per 5 min");
    assert_eq!(drive(&c, &r, 400), ["start"], "reads CLOSED: starts this pass");
    assert_eq!(r.count(view), 2);
    let closed = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == "blocker-closed");
    assert_eq!(
        closed.map(|e| e.data["ref"].clone()).collect::<Vec<_>>(),
        [json!("jirathip-dev/cooee#87")]
    );
}
