mod common;
use common::*;
use herdsman::events::{append, read, Event};
use serde_json::{json, Value};

const WS: &str = r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#;
const PANES: &str = r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#;

fn ev(c: &herdsman::Ctx, ts: u64, kind: &str, d: Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
}

fn fake(level: &str) -> Fake {
    Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("herdr workspace list", r#"{"result":{"workspaces":[]}}"#)
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", &format!("{level}\n"))
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("gh pr view issue-12", "")
        .reply("git -C", "")
        .reply("herdr worktree", WS)
        .reply("herdr pane list", PANES)
        .reply("herdr pane run", "{}")
        .reply("herdr agent start", "{}")
        .reply("herdr pane", "{}")
        .reply("herdr agent prompt", "{}")
        .reply("herdr agent wait", "{}")
        .reply("kill -0", "")
}

fn spawns(c: &herdsman::Ctx) -> usize {
    read(&c.state).unwrap().iter().filter(|e| e.kind == "spawn").count()
}

fn stuck(name: &str) -> herdsman::Ctx {
    let c = ctx(&tmpdir(name), CONFIG);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let (code, out) = run(&c, &fake("4"), &["drive"]);
    assert_eq!((code, spawns(&c)), (0, 0), "refused on load: {out}");
    let why = "lane keeps ending without a push";
    ev(&c, c.now, "needs-human", json!({"code": "LANE_ENDS", "reason": why}));
    c
}

#[test]
fn a_granted_extend_on_a_never_started_lane_spawns_it_once_the_host_admits() {
    let c = stuck("never-started-extend");
    let (code, out) = run(&c, &fake("4"), &["extend", "sendmeter", "12"]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = run(&c, &fake("4"), &["drive"]);
    assert_eq!((code, spawns(&c)), (0, 0), "still refused on load: {out}");
    let calm = fake("1");
    let (code, out) = run(&c, &calm, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(calm.called("herdr agent start sendmeter-impl-12"), "{out}");
    assert_eq!(spawns(&c), 1, "{out}");
}

fn used(name: &str) -> herdsman::Ctx {
    let c = ctx(&tmpdir(name), CONFIG);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let d =
        json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "w9:x", "workspace": "w9", "branch": "issue-12"});
    ev(&c, 10, "spawn", d);
    for (ts, p) in [(11, "1"), (12, "2"), (13, "3")] {
        ev(&c, ts, "verdict", json!({"verdict": "FAIL", "sha": p.repeat(40)}));
    }
    ev(
        &c,
        14,
        "needs-human",
        json!({"code": "ROUND_LIMIT", "reason": "out of rounds"}),
    );
    c
}

fn card(c: &herdsman::Ctx) -> String {
    let addr = "127.0.0.1:7777";
    let r = fake("1");
    let s = herdsman::serve::Server::new(c, &r, Some(addr.into()));
    let cookie = pair(&s, c, "", c.now);
    let res = s.respond(&format!("GET / HTTP/1.1\r\nHost: {addr}\r\n{cookie}\r\n"), c.now);
    let page = with_board(&String::from_utf8_lossy(&res), &s, addr, &cookie, c.now);
    let a = page.find("<div class=ask data-n=12>").expect("no needs-human card");
    page[a..a + page[a..].find("</div>").unwrap()].to_string()
}

#[test]
fn the_card_offers_start_lane_only_for_a_lane_that_never_ran() {
    let k = card(&stuck("never-started-card"));
    assert!(
        k.contains("value=extend>start lane") && !k.contains("one more round"),
        "{k}"
    );
    let k = card(&used("used-rounds-card"));
    assert!(
        k.contains("value=extend>one more round") && !k.contains("start lane"),
        "{k}"
    );
}

#[test]
fn waiting_names_the_card_or_the_load_never_a_question_mark() {
    let waiting = |c: &herdsman::Ctx| {
        let (code, w) = run(c, &fake("4"), &["waiting", "sendmeter"]);
        assert!(code == 0 && !w.contains("→ ?"), "{w}");
        w
    };
    let c = stuck("never-started-waiting");
    let w = waiting(&c);
    let card = "sendmeter #12 owner: needs-human: your card's [start lane]";
    assert!(w.contains(card) && w.contains("→ herdsman extend sendmeter 12"), "{w}");
    let u = used("used-rounds-waiting");
    let w = waiting(&u);
    let card = "sendmeter #12 owner: needs-human: your card's [one more round]";
    assert!(w.contains(card) && w.contains("→ herdsman extend sendmeter 12"), "{w}");

    assert_eq!(run(&c, &fake("4"), &["extend", "sendmeter", "12"]).0, 0);
    let w = waiting(&c);
    assert!(
        w.contains("sendmeter #12 drive: starting, waiting for host: HOST_BUSY"),
        "{w}"
    );
    let legacy = stuck("never-started-legacy");
    ev(
        &legacy,
        legacy.now,
        "round-extended",
        json!({"by": "owner", "spawn": false}),
    );
    assert_eq!(run(&legacy, &fake("4"), &["drive"]).0, 0);
    let w = waiting(&legacy);
    assert!(
        w.contains("sendmeter #12 drive: next in the queue, held back: HOST_BUSY"),
        "{w}"
    );
    let calm = fake("1");
    let (code, out) = run(&legacy, &calm, &["drive"]);
    assert!(code == 0 && calm.called("herdr agent start sendmeter-impl-12"), "{out}");
}
