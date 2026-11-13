//! #778: a card tap that starts a lane stays answered while the lane waits to start: its card leaves `/api/v1` at once,
//! the issue's row says `starting, waiting for host: <reason>`, and drive starts the lane with no second tap. A hard
//! refusal brings the card back at once with its code; 30 min unstarted brings it back naming the wait.
mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::runner::{Out, Runner};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const WS: &str = r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#;
const PANES: &str = r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#;

fn sha(p: &str) -> String {
    format!("{p}{}", "0".repeat(40 - p.len()))
}

fn ev(c: &drover::Ctx, ts: u64, kind: &str, d: Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
}

/// #12 queued, its impl lane through three FAILs, then out of rounds (needs-human).
fn setup(name: &str) -> drover::Ctx {
    let c = ctx(&tmpdir(name), CONFIG);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "w9:x", "workspace": "w9", "worktree": wt,
        "branch": "issue-12"});
    ev(&c, 10, "spawn", d);
    for (ts, p) in [(11, "1"), (12, "2"), (13, "3")] {
        ev(
            &c,
            ts,
            "verdict",
            json!({"verdict": "FAIL", "sha": sha(p), "body": "FAIL"}),
        );
    }
    ev(
        &c,
        14,
        "needs-human",
        json!({"code": "ROUND_LIMIT", "reason": "out of rounds"}),
    );
    c
}

/// Spawns succeed; memory pressure `level` (4: the host is busy, HOST_BUSY).
fn fake(level: &str) -> Fake {
    let pr = json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": sha("3"),
        "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}]});
    Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", &format!("{level}\n"))
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("gh pr view issue-12", &pr.to_string())
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

/// `fake(level)` with 0.5 GB of memory free + inactive: under the default `min_free_mem_gb` (2), MEM_LOW (#975: the
/// host gate an owner's extend still meets).
fn low_mem(level: &str) -> Fake {
    let vm = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free: 16384.\n\
        Pages inactive: 16384.\nSwapouts: 100.\n";
    let low = Fake::default().reply("vm_stat", vm);
    Fake {
        replies: [low.replies, fake(level).replies].concat(),
        ..Default::default()
    }
}

fn get(c: &drover::Ctx, r: &Fake, path: &str, now: u64) -> Value {
    let s = Server::new(c, r, Some(ADDR.into()));
    serde_json::from_str(&api(&s, ADDR, "", now, path)).unwrap()
}

/// `/api/v1/needs`: the needs-human cards' (issue, why).
fn cards(c: &drover::Ctx, r: &Fake, now: u64) -> Vec<(u64, String)> {
    let n = get(c, r, "needs", now);
    let i = n["issues"].as_array().unwrap().iter();
    i.map(|i| (i["issue"].as_u64().unwrap(), i["why"].as_str().unwrap().into()))
        .collect()
}

/// `/api/v1/state`'s queue row for #12.
fn row(c: &drover::Ctx, r: &Fake) -> Value {
    let s = get(c, r, "state", c.now);
    s["queue"]
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["issue"] == 12)
        .unwrap()
        .clone()
}

fn spawns(c: &drover::Ctx) -> usize {
    read(&c.state).unwrap().iter().filter(|e| e.kind == "spawn").count()
}

fn pending(c: &drover::Ctx) -> Option<drover::pending::Pending> {
    drover::pending::of(&read(&c.state).unwrap(), "sendmeter", 12)
}

/// AC1: [one more round] while spawns are refused MEM_LOW: the card goes at once and stays gone through a busy pass,
/// the issue row, `status` and `waiting` say what it waits for; once load drops, drive starts it with no second tap.
#[test]
fn a_held_back_extend_stays_answered_and_drive_starts_it_with_no_second_tap() {
    let c = setup("pending-extend");
    let (busy, calm) = (low_mem("1"), fake("1"));
    assert_eq!(cards(&c, &calm, c.now).len(), 1, "the needs-human card before the tap");
    let (code, out) = run(&c, &busy, &["extend", "sendmeter", "12"]);
    assert!(code == 0 && out.contains("waiting for host (MEM_LOW)"), "{out}");
    let (code, out) = run(&c, &busy, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(spawns(&c), 1, "nothing started on a busy host");
    assert_eq!(cards(&c, &calm, c.now), [], "gone, and stays gone");
    let q = row(&c, &calm);
    assert_eq!(q["pending"]["action"], "extend", "{q}");
    assert_eq!(q["pending"]["reason"], "MEM_LOW", "{q}");
    assert_eq!(
        q["status"], "extended · starting, waiting for host: MEM_LOW · 0s",
        "{q}"
    );
    let (_, st) = run(&c, &calm, &["status", "sendmeter"]);
    assert!(st.contains("extended · starting, waiting for host: MEM_LOW"), "{st}");
    let (_, w) = run(&c, &calm, &["waiting", "sendmeter"]);
    assert!(
        w.contains("sendmeter #12 drive: extended · starting, waiting for host: MEM_LOW"),
        "{w}"
    );

    let (code, out) = run(&c, &calm, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(calm.called("herdr agent start sendmeter-impl-12"), "{out}");
    assert_eq!((spawns(&c), pending(&c)), (2, None));
    assert_eq!(cards(&c, &calm, c.now), []);
    assert_eq!(row(&c, &calm)["status"], Value::Null);
}

/// AC3: still unstarted at 30 min, one card comes back naming the reason and the wait; a tap on it starts the round.
#[test]
fn a_start_pending_for_30_minutes_brings_its_card_back_once() {
    let mut c = setup("pending-late");
    let (busy, calm) = (low_mem("1"), fake("1"));
    assert_eq!(run(&c, &busy, &["extend", "sendmeter", "12"]).0, 0);
    assert_eq!(cards(&c, &calm, c.now + 1799), []);
    let back = cards(&c, &calm, c.now + 1800);
    assert_eq!(back.len(), 1, "{back:?}");
    assert_eq!(
        back[0],
        (
            12,
            "extend did not start a lane (still waiting after 30m): MEM_LOW".into()
        )
    );
    c.now += 1800;
    let (code, out) = run(&c, &calm, &["extend", "sendmeter", "12"]);
    assert_eq!(code, 0, "the returned card's tap: {out}");
    assert!(calm.called("herdr agent start sendmeter-impl-12"), "{out}");
    assert_eq!((pending(&c), cards(&c, &calm, c.now)), (None, vec![]));
    let rounds = read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "round-extended")
        .count();
    assert_eq!(rounds, 1, "the second tap grants no second round");
}

/// AC3, AC5: soft refusals keep the start pending, a hard one (here the spawn's own error) ends it and its card is
/// back at once with the code, until a lane starts after all (#724's retry of a failed spawn).
#[test]
fn soft_refusals_keep_it_pending_and_a_hard_one_brings_the_card_back_at_once() {
    let c = setup("pending-hard");
    let busy = low_mem("1");
    assert_eq!(run(&c, &busy, &["extend", "sendmeter", "12"]).0, 0);
    for code in drover::pending::SOFT {
        assert!(
            drover::pending::refused(&c, ("sendmeter", 12), code, "x").unwrap(),
            "{code}"
        );
        let p = pending(&c).unwrap();
        assert_eq!((p.reason.as_deref(), p.failed), (Some(code), false));
    }
    assert!(drover::pending::refused(&c, ("sendmeter", 12), "IN_FLIGHT", "another pass").unwrap());
    assert_eq!(
        pending(&c).unwrap().reason.as_deref(),
        Some("HOST_OFFLINE"),
        "a skip is no refusal"
    );
    assert_eq!(cards(&c, &busy, c.now), []);

    let broken = fake("1").once_err("herdr worktree", 1, "worktree exists");
    let (code, out) = run(&c, &broken, &["drive"]);
    assert_eq!(code, 0, "{out}");
    let p = pending(&c).unwrap();
    assert!(p.failed && p.reason.as_deref().unwrap().starts_with("ERROR: "), "{p:?}");
    let back = cards(&c, &broken, c.now);
    assert_eq!(back.len(), 1, "{back:?}");
    assert!(
        back[0]
            .1
            .starts_with("extend did not start a lane (refused after 0s): ERROR: "),
        "{back:?}"
    );
    for code in ["ROUND_LIMIT", "MERGED", "UPDATE_LIMIT", "BAD_VALUE", "DISK_LOW"] {
        drover::pending::record(&c, ("sendmeter", 12), "extend", "impl", None).unwrap();
        assert!(
            !drover::pending::refused(&c, ("sendmeter", 12), code, "x").unwrap(),
            "{code}"
        );
        let p = pending(&c).unwrap();
        assert_eq!((p.failed, p.reason), (true, Some(format!("{code}: x"))));
    }
    let calm = fake("1");
    let (code, out) = run(&c, &calm, &["drive"]);
    assert!(code == 0 && calm.called("herdr agent start sendmeter-impl-12"), "{out}");
    assert_eq!((pending(&c), cards(&c, &calm, c.now)), (None, vec![]));
}

/// The Fake; on the first call starting with `at`, `hook` runs before it (a second pass racing this one).
struct Hooked<'a> {
    fake: &'a Fake,
    at: &'a str,
    hook: &'a (dyn Fn() + Sync),
    fired: Shared<bool>,
}

impl Runner for Hooked<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        let fire = format!("{prog} {}", args.join(" ")).starts_with(self.at) && !self.fired.get();
        self.fired.set(self.fired.get() || fire);
        if fire {
            (self.hook)();
        }
        self.fake.run(prog, args)
    }
}

/// AC2, AC5: an answer whose lane is gone is pending until the issue's next lane spawns; two passes racing that spawn
/// start it once (#509's action id), and the skipped one neither fails it nor spawns again.
#[test]
fn an_answer_for_a_gone_lane_is_pending_and_two_racing_passes_start_it_once() {
    let c = ctx(&tmpdir("pending-answer"), CONFIG);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let ask = json!({"id": "q1", "question": "Which DB?", "options": ["sqlite", "postgres"], "kind": "owner",
        "agent": "sendmeter-impl-12"});
    ev(&c, 20, "needs-decision", ask);
    let (busy, calm) = (fake("4"), fake("1"));
    let (code, out) = run(&c, &busy, &["answer", "sendmeter", "q1", "1"]);
    assert!(code == 0 && out.contains("asking agent gone"), "{out}");
    assert_eq!(run(&c, &busy, &["drive"]).0, 0);
    let p = pending(&c).expect("pending");
    assert_eq!((p.action.as_str(), p.role.as_str()), ("answer", "impl"));
    assert_eq!(row(&c, &calm)["status"], "starting, waiting for host: HOST_BUSY · 0s");

    let second = Shared::<Vec<String>>::default();
    let race = || {
        std::thread::scope(|s| {
            let b = s.spawn(|| run(&c, &calm, &["kick", "sendmeter", "12", "--now"]));
            second.borrow_mut().push(b.join().unwrap().1);
        });
    };
    let a = Hooked {
        fake: &calm,
        at: "herdr worktree",
        hook: &race,
        fired: Shared::new(false),
    };
    let (code, out) = run(&c, &a, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        second.borrow().concat().contains("REFUSED IN_FLIGHT"),
        "{:?}",
        second.borrow()
    );
    assert_eq!(calm.count("herdr agent start sendmeter-impl-12"), 1);
    assert_eq!((spawns(&c), pending(&c)), (1, None));
    let failed = read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "pending-action");
    assert!(
        failed.into_iter().all(|e| e.data["failed"] == false),
        "the skip failed nothing"
    );
}

/// #825: a start held by host load alone never goes back to the owner (#975: an answer's; an owner's extend never
/// waits on load).
#[test]
fn a_start_held_by_host_load_alone_never_goes_back_to_the_owner_and_starts_once_admitted() {
    let mut c = ctx(&tmpdir("pending-load"), CONFIG);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let ask = json!({"id": "q1", "question": "Which DB?", "options": ["sqlite", "postgres"], "kind": "owner",
        "agent": "sendmeter-impl-12"});
    ev(&c, 20, "needs-decision", ask);
    let load = Out {
        stdout: "{ 40.00 3.00 2.00 }\n".into(),
        ..Out::default()
    };
    let swapd = Out {
        stdout: "  104     1 900.0 /usr/sbin/swapd\n".into(),
        ..Out::default()
    };
    let loaded = Fake {
        replies: [
            vec![
                ("sysctl -n vm.loadavg".to_string(), load),
                ("ps -Ao".to_string(), swapd),
            ],
            fake("1").replies,
        ]
        .concat(),
        ..Fake::default()
    };
    let calm = fake("1");
    let (code, out) = run(&c, &loaded, &["answer", "sendmeter", "q1", "1"]);
    assert!(code == 0 && out.contains("asking agent gone"), "{out}");
    c.now += 7200;
    let (code, out) = run(&c, &loaded, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(spawns(&c), 0, "nothing started on a loaded host");
    assert_eq!(pending(&c).unwrap().reason.as_deref(), Some(drover::pending::LOAD));
    assert_eq!(cards(&c, &calm, c.now), [], "two hours on load alone: no card");
    let (code, out) = run(&c, &calm, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(calm.called("herdr agent start sendmeter-impl-12"), "{out}");
    assert_eq!((spawns(&c), pending(&c)), (1, None));
}
