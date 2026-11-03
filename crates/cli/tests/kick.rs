//! #304 `herdsman kick`: one issue's due steps at once, debounced, under the drive lock; lane hooks and the board kick.
mod common;
use common::*;
use herdsman::events::{read, Event};
use serde_json::json;

fn fan(name: &str) -> herdsman::Ctx {
    let c = ctx(&tmpdir(name), CONFIG);
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "1", "2", "3"]).0,
        0
    );
    c
}

/// Spawns succeed, the host is calm; `kill -0 4242` answers `live` times as a live pid, then dead.
fn fake(live: usize) -> Fake {
    let mut r = Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("herdr worktree", r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#)
        .reply(
            "herdr pane list",
            r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#,
        )
        .reply("herdr", "{}")
        .reply("git -C", "");
    for _ in 0..live {
        r = r.once("kill -0 4242", 0, "");
    }
    r
}

fn kicks(c: &herdsman::Ctx) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "kick")
        .collect()
}

fn kick(c: &herdsman::Ctx, r: &Fake, n: &str) {
    let (code, out) = run(c, r, &["kick", "sendmeter", n, "--why", "push", "--now"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn a_kick_runs_only_its_issue() {
    let c = fan("kick-one");
    let r = fake(0);
    kick(&c, &r, "2");
    assert!(r.called("herdr agent start sendmeter-impl-2"), "{:?}", r.calls.borrow());
    assert!(!r.called("herdr agent start sendmeter-impl-1") && !r.called("herdr agent start sendmeter-impl-3"));
    let k = kicks(&c);
    assert_eq!((k.len(), k[0].issue, k[0].str("why")), (1, Some(2), Some("push")));
    assert!(k[0].str("result").unwrap().contains("started #2"), "{:?}", k[0]);
    assert_eq!(
        herdsman::drive::last_pass(&c, "sendmeter"),
        None,
        "a kick is no fleet pass"
    );
}

#[test]
fn three_kicks_coalesce_into_one_run_and_one_follow_up() {
    let c = fan("kick-debounce");
    let dir = c.state.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("kick-sendmeter-2.lock"), "4242").unwrap();
    // Two kicks while one runs: coalesced, nothing driven.
    let r = fake(2);
    kick(&c, &r, "2");
    kick(&c, &r, "2");
    assert!(kicks(&c).is_empty() && !r.called("herdr agent start"));
    assert!(dir.join("kick-sendmeter-2.again").exists());
    // The running kick's turn: its run, then the one follow-up, then none.
    let r = fake(0);
    kick(&c, &r, "2");
    assert_eq!(kicks(&c).len(), 2);
    assert_eq!(r.count("sleep 10"), 2);
    assert_eq!(
        r.count("herdr agent start sendmeter-impl-2"),
        1,
        "the follow-up finds it started"
    );
    assert!(!dir.join("kick-sendmeter-2.again").exists() && !dir.join("kick-sendmeter-2.lock").exists());
}

#[test]
fn a_busy_drive_lock_gets_one_follow_up_not_a_double_run() {
    let c = fan("kick-busy");
    let dir = c.state.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("drive.lock"), "4242").unwrap();
    let r = fake(1);
    kick(&c, &r, "2");
    assert_eq!(kicks(&c).len(), 1, "the busy try logs no run");
    assert_eq!(r.count("herdr agent start"), 1);
    assert_eq!(r.count("sleep 10"), 2);
}

#[test]
fn the_hook_matches_a_successful_push_and_a_stop_only() {
    let push = |resp: serde_json::Value| {
        let i = json!({"hook_event_name": "PostToolUse", "tool_name": "Bash", "cwd": "/w",
            "tool_input": {"command": "git push -u origin issue-24"}, "tool_response": resp});
        herdsman::hook::kick_why(&i)
    };
    let ok =
        json!({"stdout": "", "stderr": "To github.com:x/y.git\n   a..b  issue-24 -> issue-24", "interrupted": false});
    assert_eq!(push(ok), Some("push"));
    let rejected = json!({"stderr": " ! [rejected]  issue-24 -> issue-24 (fetch first)\nerror: failed to push"});
    assert_eq!(push(rejected), None);
    assert_eq!(push(json!({"stderr": "fatal: unable to access"})), None);
    let ls = json!({"hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": {"command": "ls"}});
    assert_eq!(herdsman::hook::kick_why(&ls), None);
    assert_eq!(
        herdsman::hook::kick_why(&json!({"hook_event_name": "Stop"})),
        Some("stop")
    );
    let pre = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": "git push"}});
    assert_eq!(herdsman::hook::kick_why(&pre), None);
}

/// One test mutates `HERDSMAN_LANE` (process-wide): the hook kicks in a lane only.
#[test]
fn the_hook_kicks_its_lanes_issue_and_is_inert_outside_lanes() {
    let c = ctx(&tmpdir("kick-hook"), CONFIG);
    let hook = |r: &Fake| {
        let stop = json!({"hook_event_name": "Stop", "session_id": "s"}).to_string();
        let argv = ["herdsman", "hook", "--role", "impl"].map(String::from).to_vec();
        let mut out = vec![];
        assert_eq!(herdsman::main_with(argv, &c, r, &mut out, &mut stop.as_bytes()), 0);
        assert!(out.is_empty());
    };
    std::env::remove_var("HERDSMAN_MAIN");
    std::env::remove_var("HERDSMAN_LANE");
    let r = Fake::default();
    hook(&r);
    assert!(r.calls.borrow().is_empty(), "inert: {:?}", r.calls.borrow());
    std::env::set_var("HERDSMAN_LANE", "sendmeter:24:impl");
    let r = Fake::default();
    hook(&r);
    std::env::remove_var("HERDSMAN_LANE");
    let calls = r.calls.borrow();
    assert_eq!(calls.len(), 1, "{calls:?}");
    let log = c.state.with_file_name("kick.log");
    let want = format!(
        "sh -c nohup '/opt/herdsman/bin/herdsman' kick sendmeter 24 --why stop --now >> '{}' 2>&1 < /dev/null &",
        log.display()
    );
    assert_eq!(calls[0], want);
}
