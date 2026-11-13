mod common;
use common::*;
use drover::events::{read, Event};
use drover::runner::{Out, Runner};
use drover::serve::Server;
use serde_json::{json, Value};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

const ADDR: &str = "127.0.0.1:7777";
const DESIGN: &str = "\n[fleet.sendmeter.role.design]\nharness = \"hermes\"\nprofile = \"fleet-design\"\n";
const HEAD: &str = "dddddddddddddddddddddddddddddddddddddddd";

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

struct Meet {
    fake: Fake,
    diff: String,
    met: Mutex<usize>,
    cv: Condvar,
}

impl Runner for Meet {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        if format!("{prog} {}", args.join(" ")).starts_with(&self.diff) {
            let mut n = self.met.lock().unwrap();
            *n += 1;
            self.cv.notify_all();
            let _ = self
                .cv
                .wait_timeout_while(n, Duration::from_secs(5), |n| *n < 2)
                .unwrap();
        }
        self.fake.run(prog, args)
    }
    fn sleep(&self, _: u64) {}
}

#[test]
fn concurrent_full_fast_and_kick_passes_over_one_finished_design_lane_post_one_ask() {
    let c = ctx(&tmpdir("ask-ids-design"), &format!("{CONFIG}{DESIGN}"));
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--design"]).0,
        0
    );
    let wt = c.worktrees.join("sendmeter/issue-12").display().to_string();
    let fake = Fake::default()
        .reply("herdr agent list", &agents(&[]))
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply(&format!("git -C {wt} log -1"), &format!("{HEAD}\n"))
        .reply(&format!("git -C {wt} rev-list"), "0\n")
        .reply(
            &format!("git -C {wt} log --format= --name-only"),
            "docs/design/meter/index.html\ndocs/design/meter/1440.png\n",
        )
        .reply("git -C", "")
        .reply("herdr worktree", r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#)
        .reply(
            "herdr pane list",
            r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#,
        )
        .reply(
            "herdr workspace create",
            r#"{"result":{"root_pane":{"pane_id":"w9:c1"}}}"#,
        )
        .reply("herdr pane", "{}")
        .reply("herdr agent", "{}");
    let (code, out) = run(&c, &fake, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(kinds(&c, "spawn").len(), 1, "the design lane: {out}");
    let r = Meet {
        fake,
        diff: format!("git -C {wt} log --format= --name-only"),
        met: Mutex::new(0),
        cv: Condvar::new(),
    };
    let passes: [&[&str]; 3] = [
        &["drive"],
        &["drive", "--fast"],
        &["kick", "sendmeter", "12", "--why", "drive", "--now"],
    ];
    std::thread::scope(|s| {
        for p in passes {
            let (c, r) = (&c, &r);
            s.spawn(move || run(c, r, p));
        }
    });
    assert!(*r.met.lock().unwrap() >= 2, "two passes reached the hand-off");
    assert_eq!(kinds(&c, "design-review").len(), 1);
    assert_eq!(kinds(&c, "needs-decision").len(), 1);
}

#[test]
fn fifty_concurrent_asks_from_different_fleets_get_fifty_distinct_ids() {
    let fleets: String = (0..50)
        .map(|i| {
            format!(
                "[fleet.f{i}]\nrepo = \"o/f{i}\"\nlocal = \"/tmp/does-not-matter/f{i}\"\nintegration = \"staging\"\n"
            )
        })
        .collect();
    let c = ctx(&tmpdir("ask-ids-fifty"), &format!("{CONFIG}{fleets}"));
    std::thread::scope(|s| {
        for i in 0..50 {
            let c = &c;
            s.spawn(move || {
                let f = format!("f{i}");
                let (code, out) = run(c, &Fake::default(), &["ask", &f, "-", "Go?", "--option", "yes"]);
                assert_eq!(code, 0, "{out}");
            });
        }
    });
    let mut ids: Vec<String> = kinds(&c, "needs-decision")
        .iter()
        .map(|e| e.str("id").unwrap().to_string())
        .collect();
    assert_eq!(ids.len(), 50);
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 50, "{ids:?}");
}

#[test]
fn two_events_for_one_ask_id_show_one_row_in_the_thread_and_on_the_board() {
    let c = ctx(&tmpdir("ask-ids-dup"), CONFIG);
    let ask = json!({"id": "q1411", "question": "Design for #254 is ready (04d48d7): approve, revise or reject?",
        "options": ["approve", "revise", "reject"], "recommend": null, "kind": "owner", "design": true, "thread": "t70"});
    let log = [
        json!({"ts": 1_000, "fleet": "sendmeter", "type": "thread-new", "thread": "t70", "title": "designs"}),
        json!({"ts": 1_002, "fleet": "sendmeter", "issue": 254, "type": "needs-decision"}),
        json!({"ts": 1_001, "fleet": "sendmeter", "issue": 254, "type": "needs-decision"}),
    ];
    let line = |mut v: Value| {
        if v["type"] == "needs-decision" {
            v.as_object_mut().unwrap().extend(ask.as_object().unwrap().clone());
        }
        v.to_string() + "\n"
    };
    rewrite_log(&c.state, &log.into_iter().map(line).collect::<String>());
    assert_eq!(kinds(&c, "needs-decision").len(), 2);
    let (code, out) = run(&c, &Fake::default(), &["thread", "show", "t70"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.matches("asks q1411").count(), 1, "{out}");
    let (code, out) = run(&c, &Fake::default(), &["asks", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out.matches("q1411").count(), 1, "{out}");
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let state: Value = serde_json::from_str(&api(&s, ADDR, "", c.now, "state")).unwrap();
    let ids: Vec<&Value> = state["asks"].as_array().unwrap().iter().map(|a| &a["id"]).collect();
    assert_eq!(ids, [&json!("q1411")], "{state}");
}
