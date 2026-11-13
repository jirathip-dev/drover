//! #509: retry-safe side effects. A spawn two passes race starts once; a merge a crash left unlogged is recovered from
//! the PR, never merged twice; the same kick id is driven once.
mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use drover::runner::{Out, Runner};
use serde_json::json;

const WS: &str = r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#;
const PANES: &str = r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#;
const HEAD: &str = "1111111111111111111111111111111111111111";

fn fake() -> Fake {
    Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "")
        .reply("herdr worktree", WS)
        .reply("herdr pane list", PANES)
        .reply("herdr pane run", "{}")
        .reply("herdr agent start", "{}")
        .reply("herdr pane", "{}")
        .reply("herdr agent prompt", "{}")
        .reply("herdr agent wait", "{}")
        .reply("kill -0", "") // the intent's process (this test) still runs
}

/// The Fake, with `hook` run once, on the first call starting with `at`: after the call (`after`) or before it.
struct Hooked<'a> {
    fake: &'a Fake,
    at: &'a str,
    after: bool,
    hook: &'a (dyn Fn() + Sync),
    fired: Shared<bool>,
}

impl Runner for Hooked<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        let fire = format!("{prog} {}", args.join(" ")).starts_with(self.at) && !self.fired.get();
        self.fired.set(self.fired.get() || fire);
        if fire && !self.after {
            (self.hook)();
        }
        let out = self.fake.run(prog, args);
        if fire && self.after {
            (self.hook)();
        }
        out
    }
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

#[test]
fn two_passes_racing_the_same_spawn_start_it_once_and_the_other_skips() {
    let c = ctx(&tmpdir("race-spawn"), CONFIG);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let r = fake();
    let second = Shared::<Vec<String>>::default();
    // While the first pass creates the lane (its intent logged, nothing made yet), a kick's pass drives the same issue.
    let race = || {
        std::thread::scope(|s| {
            let b = s.spawn(|| run(&c, &r, &["kick", "sendmeter", "12", "--now"]));
            second.borrow_mut().push(b.join().unwrap().1);
        });
    };
    let a = Hooked {
        fake: &r,
        at: "herdr worktree",
        after: false,
        hook: &race,
        fired: Shared::new(false),
    };
    let (code, out) = run(&c, &a, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("sendmeter #12 start: "), "{out}");
    let b = second.borrow().concat();
    assert!(
        b.contains("REFUSED IN_FLIGHT: skipped: sendmeter#12:spawn@impl@0 in flight"),
        "{b}"
    );
    assert_eq!(
        r.count("herdr agent start sendmeter-impl-12"),
        1,
        "{:?}",
        r.calls.borrow()
    );
    assert_eq!(kinds(&c, "spawn").len(), 1);
    let id = |e: &Event| e.str("action_id").map(String::from);
    assert_eq!(
        kinds(&c, "intent").iter().filter_map(id).collect::<Vec<_>>(),
        ["sendmeter#12:spawn@impl@0"]
    );
    assert_eq!(
        kinds(&c, "done").iter().filter_map(id).collect::<Vec<_>>(),
        ["sendmeter#12:spawn@impl@0"]
    );
    assert!(kinds(&c, "failed").is_empty());
    let skipped = kinds(&c, "driven");
    assert!(
        skipped.iter().any(|e| e.str("code") == Some("IN_FLIGHT")),
        "{skipped:?}"
    );
}

fn ready(name: &str) -> drover::Ctx {
    let c = ctx(&tmpdir(name), CONFIG);
    let spawn = json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"});
    append(&c.state, &Event::new(10, "sendmeter", Some(12), "spawn", spawn)).unwrap();
    let v = json!({"verdict": "PASS", "sha": HEAD});
    append(&c.state, &Event::new(20, "sendmeter", Some(12), "verdict", v)).unwrap();
    c
}

fn merge_fake(state: &str) -> Fake {
    let ci = json!([{"__typename": "CheckRun", "name": "build", "status": "COMPLETED", "conclusion": "SUCCESS"}]);
    let pr =
        json!({"number": 34, "state": "OPEN", "baseRefName": "staging", "headRefOid": HEAD, "statusCheckRollup": ci});
    Fake::default()
        .reply("gh pr view issue-12", &pr.to_string())
        .reply("gh pr merge 34", "")
        .reply(
            "gh pr view 34 -R jirathip-dev/sendmeter --json state",
            &json!({"state": state}).to_string(),
        )
        .reply("gh pr view 34", &json!({"mergeCommit": {"oid": "m3rg3"}}).to_string())
        .reply("gh issue", "")
        .reply("kill -0", "")
}

#[test]
fn a_crash_between_merge_and_its_log_is_recovered_from_the_merged_pr_without_merging_again() {
    let mut c = ready("crash-merge");
    let r = merge_fake("MERGED");
    // The crash: the process dies right after `gh pr merge` went through, before anything after it is logged.
    let crash = || panic!("crash injected after gh pr merge");
    let a = Hooked {
        fake: &r,
        at: "gh pr merge 34",
        after: true,
        hook: &crash,
        fired: Shared::new(false),
    };
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run(&c, &a, &["merge", "sendmeter", "12"])
    }));
    assert!(crashed.is_err());
    assert_eq!(r.count("gh pr merge"), 1);
    let intent = kinds(&c, "intent");
    assert_eq!(intent.len(), 1);
    assert_eq!(intent[0].str("action_id"), Some(&*format!("sendmeter#12:merge@{HEAD}")));
    assert!(kinds(&c, "done").is_empty() && kinds(&c, "failed").is_empty() && kinds(&c, "merged").is_empty());

    // Within the timeout the next pass leaves it alone: the merge may still be running.
    let (code, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED IN_FLIGHT"), "{out}");
    // Past it, the next pass reads the PR: merged, so `done`, then the `merged` log; never a second merge.
    c.now += 601;
    let (code, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(r.count("gh pr merge"), 1, "{:?}", r.calls.borrow());
    let done = kinds(&c, "done");
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].str("action_id"), intent[0].str("action_id"));
    assert_eq!(done[0].data["recovered"], true);
    let merged = kinds(&c, "merged");
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].str("merge_sha"), Some("m3rg3"));
    assert_eq!(
        fold(&read(&c.state).unwrap())[&("sendmeter".into(), 12)].state,
        "merged"
    );
    assert!(r.called("gh issue comment 12"));
}

#[test]
fn a_timed_out_merge_intent_whose_pr_never_merged_fails_and_merges_once() {
    let mut c = ready("stale-merge");
    let i = json!({"action_id": format!("sendmeter#12:merge@{HEAD}"), "step": "merge", "pr": "34", "head": HEAD});
    append(&c.state, &Event::new(c.now, "sendmeter", Some(12), "intent", i)).unwrap();
    c.now += 601;
    let r = merge_fake("OPEN");
    let (code, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(r.count("gh pr merge"), 1);
    assert_eq!(kinds(&c, "failed").len(), 1);
    assert_eq!(kinds(&c, "intent").len(), 2);
    assert_eq!(kinds(&c, "done").len(), 1);
    assert_eq!(kinds(&c, "merged").len(), 1);
}

#[test]
fn the_same_kick_id_is_driven_once() {
    let c = ctx(&tmpdir("kick-once"), CONFIG);
    let r = fake();
    let kick = |id: &str| {
        run(
            &c,
            &r,
            &["kick", "sendmeter", "12", "--why", "verdict", "--now", "--id", id],
        )
    };
    let (code, _) = kick("abc-PASS");
    assert_eq!(code, 0);
    let (code, out) = kick("abc-PASS");
    assert_eq!(code, 0);
    assert_eq!(out, "kick verdict abc-PASS: already processed\n");
    let (code, out) = kick("def-PASS");
    assert_eq!(code, 0);
    assert!(!out.contains("already processed"), "{out}");
    assert_eq!(kinds(&c, "kick").len(), 2, "one pass per new id");
}
