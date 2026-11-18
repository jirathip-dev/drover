mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const LANE: &str = "sendmeter-impl-12";

fn ev(c: &drover::Ctx, ts: u64, kind: &str, d: Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
}

fn spawn(c: &drover::Ctx, ts: u64) {
    let wt = c.worktrees.join("sendmeter/issue-12");
    let d = json!({"role": "impl", "agent": LANE, "pane": LANE, "worktree": wt, "branch": "issue-12"});
    ev(c, ts, "spawn", d);
}

fn runner() -> Fake {
    Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "")
}

fn ok(c: &drover::Ctx, r: &Fake, args: &[&str]) -> String {
    let (code, out) = run(c, r, args);
    assert_eq!(code, 0, "{args:?}: {out}");
    out
}

fn blocked(name: &str) -> (drover::Ctx, Fake) {
    set_env("DROVER_LANE", "sendmeter:12:impl");
    let c = ctx(&tmpdir(name), CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
    std::fs::create_dir_all(c.worktrees.join("sendmeter/issue-12")).unwrap();
    let r = runner();
    ok(&c, &r, &["queue", "sendmeter", "add", "12"]);
    spawn(&c, 10);
    for (ts, p) in [(11, "1"), (12, "2")] {
        let v = json!({"verdict": "FAIL", "sha": p.repeat(40), "body": "FAIL"});
        ev(&c, ts, "verdict", v);
    }
    spawn(&c, 13);
    ok(&c, &r, &["turn", "blocked", "--why", "waits on q1938"]);
    let out = ok(&c, &r, &["drive"]);
    assert!(out.contains("needs-human: lane blocked: waits on q1938"), "{out}");
    let q = json!({"id": "q5", "question": "One more round for #12? (lane blocked: waits on q1938)",
        "options": ["one more round", "park"], "recommend": 1, "kind": "owner", "extend": true, "spawn": true});
    ev(&c, c.now, "needs-decision", q);
    spawn(&c, c.now + 1);
    assert_eq!(state(&c), "fix(2)", "the lane went on after q1938's answer");
    (c, r)
}

fn state(c: &drover::Ctx) -> String {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)]
        .state
        .clone()
}

fn kinds(c: &drover::Ctx, kind: &str) -> usize {
    read(&c.state).unwrap().iter().filter(|e| e.kind == kind).count()
}

fn card(s: &Server, c: &drover::Ctx, cookie: &str) -> Option<Value> {
    let req = format!("GET /screens/cards HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}\r\n");
    let res = String::from_utf8_lossy(&s.respond(&req, c.now)).into_owned();
    let all: Value = serde_json::from_str(res.split_once("\r\n\r\n").unwrap().1).unwrap();
    all.as_array().unwrap().iter().find(|k| k["id"] == "ask-q5").cloned()
}

fn tap(s: &Server, c: &drover::Ctx, cookie: &str, choice: &str) -> String {
    let body = json!({"screen": "cards", "block": "q5", "action": "answer",
        "values": {"fleet": "sendmeter", "choice": choice}})
    .to_string();
    let req = format!(
        "POST /action HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&req, c.now)).into_owned()
}

fn open(c: &drover::Ctx, r: &Fake) -> bool {
    ok(c, r, &["asks", "sendmeter"]).contains("q5")
}

#[test]
fn one_more_round_on_a_blocked_lanes_ask_closes_it_and_gives_the_lane_its_round() {
    let (c, r) = blocked("extend-ask-round");
    let on = OnShed::new(runner());
    let s = Server::new(&c, &on, Some(ADDR.into()));
    let cookie = pair(&s, &c, "", c.now);
    assert!(card(&s, &c, &cookie).is_some(), "the ask's card before the tap");
    let res = tap(&s, &c, &cookie, "one more round");
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(!open(&c, &r), "the ask closes");
    assert_eq!(card(&s, &c, &cookie), None, "the card is gone");
    assert_eq!(kinds(&c, "round-extended"), 1, "the lane gets its round");
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
    assert_eq!((st.extra, st.state.as_str()), (1, "fix(2)"));
}

#[test]
fn park_on_a_blocked_lanes_ask_parks_the_issue_and_the_card_goes() {
    let (c, r) = blocked("extend-ask-park");
    let on = OnShed::new(runner());
    let s = Server::new(&c, &on, Some(ADDR.into()));
    let cookie = pair(&s, &c, "", c.now);
    let res = tap(&s, &c, &cookie, "park");
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert_eq!(state(&c), "parked");
    assert!(!open(&c, &r), "the ask closes");
    assert_eq!(card(&s, &c, &cookie), None, "the card is gone");
}

#[test]
fn a_refused_tap_shows_the_refusal_on_the_card() {
    let (c, r) = blocked("extend-ask-refused");
    ev(&c, c.now + 2, "parked", json!({"by": "owner"}));
    let on = OnShed::new(runner());
    let s = Server::new(&c, &on, Some(ADDR.into()));
    let cookie = pair(&s, &c, "", c.now);
    let res = tap(&s, &c, &cookie, "one more round");
    assert!(res.starts_with("HTTP/1.1 409"), "{res}");
    assert!(open(&c, &r), "a refused tap decides nothing");
    let k = card(&s, &c, &cookie).expect("the card stays");
    assert!(
        k["blocks"]
            .to_string()
            .contains("REFUSED NOT_NEEDS_HUMAN: #12 is parked"),
        "{k}"
    );
}
