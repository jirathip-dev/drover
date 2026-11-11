//! #831: a needs-human issue whose lane never started: a granted extend starts it on the first pass the host admits.
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use serde_json::{json, Value};

const WS: &str = r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#;
const PANES: &str = r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#;

fn ev(c: &herdsman::Ctx, ts: u64, kind: &str, d: Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
}

/// Spawns succeed; memory pressure `level` (4: the host is busy, HOST_BUSY).
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

/// #12 queued; every start refused on load until it went needs-human (no lane ever ran, no round used).
fn stuck(name: &str) -> herdsman::Ctx {
    let c = ctx(&tmpdir(name), CONFIG);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let (code, out) = run(&c, &fake("4"), &["drive"]);
    assert_eq!((code, spawns(&c)), (0, 0), "refused on load: {out}");
    let why = "lane keeps ending without a push";
    ev(&c, c.now, "needs-human", json!({"code": "LANE_ENDS", "reason": why}));
    c
}

/// AC1, AC2: granted one more round while the host is busy; the first pass the host admits spawns the lane.
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
