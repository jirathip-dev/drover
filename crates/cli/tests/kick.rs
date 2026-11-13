//! #304 `drover kick`: one issue's due steps at once, debounced, beside the pass (#318); lane hooks and the board kick.
mod common;
use common::*;
use drover::events::{read, Event};
use serde_json::json;

fn fan(name: &str) -> (drover::Ctx, FakeShed) {
    let c = ctx(&tmpdir(name), CONFIG);
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "1", "2", "3"]).0,
        0
    );
    let sh = FakeShed::new();
    sh.attach(&c);
    (c, sh)
}

fn started(sh: &FakeShed) -> Vec<String> {
    let runs = sh.all("run").into_iter();
    runs.map(|p| p["name"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// Spawns succeed, the host is calm; `kill -0 4242` answers `live` times as a live pid, then dead.
fn fake(live: usize) -> Fake {
    let mut r = Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "");
    for _ in 0..live {
        r = r.once("kill -0 4242", 0, "");
    }
    r
}

fn kicks(c: &drover::Ctx) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == "kick")
        .collect()
}

fn kick(c: &drover::Ctx, r: &Fake, n: &str) {
    let (code, out) = run(c, r, &["kick", "sendmeter", n, "--why", "push", "--now"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn a_kick_runs_only_its_issue() {
    let (c, sh) = fan("kick-one");
    let r = fake(0);
    kick(&c, &r, "2");
    assert_eq!(started(&sh), ["sendmeter-impl-2"], "{:?}", r.calls.borrow());
    let k = kicks(&c);
    assert_eq!((k.len(), k[0].issue, k[0].str("why")), (1, Some(2), Some("push")));
    assert!(k[0].str("result").unwrap().contains("started #2"), "{:?}", k[0]);
    assert_eq!(
        drover::drive::last_pass(&c, "sendmeter"),
        None,
        "a kick is no fleet pass"
    );
}

#[test]
fn three_kicks_coalesce_into_one_run_and_one_follow_up() {
    let (c, sh) = fan("kick-debounce");
    lease_as(&c, "kick-sendmeter-2.lock", 4242, c.now, 300);
    // Two kicks while one runs: coalesced, nothing driven.
    let r = fake(2);
    kick(&c, &r, "2");
    kick(&c, &r, "2");
    assert!(kicks(&c).is_empty() && started(&sh).is_empty());
    assert!(lease_on(&c, "kick-sendmeter-2.again").is_some());
    // The running kick's turn: its run, then the one follow-up, then none.
    let r = fake(0);
    kick(&c, &r, "2");
    assert_eq!(kicks(&c).len(), 2);
    assert_eq!(r.count("sleep 10"), 2);
    assert_eq!(started(&sh), ["sendmeter-impl-2"], "the follow-up finds it started");
    assert!(lease_on(&c, "kick-sendmeter-2.again").is_none() && lease_on(&c, "kick-sendmeter-2.lock").is_none());
}

/// #318: a kick holds its issue's locks only, so a pass holding `drive.lock` never delays it.
#[test]
fn a_held_drive_lock_does_not_delay_a_kick() {
    let (c, sh) = fan("kick-busy");
    lease_as(&c, "drive-sendmeter.lock", 4242, c.now, 3600);
    let r = fake(1);
    kick(&c, &r, "2");
    assert_eq!(kicks(&c).len(), 1);
    assert_eq!(started(&sh).len(), 1);
    assert_eq!(r.count("sleep 10"), 1, "no follow-up");
    assert!(
        lease_on(&c, "drive-sendmeter.lock").is_some(),
        "the pass's lock is left alone"
    );
}

#[test]
fn the_hook_matches_a_successful_push_and_a_stop_only() {
    let push = |resp: serde_json::Value| {
        let i = json!({"hook_event_name": "PostToolUse", "tool_name": "Bash", "cwd": "/w",
            "tool_input": {"command": "git push -u origin issue-24"}, "tool_response": resp});
        drover::hook::kick_why(&i)
    };
    let ok =
        json!({"stdout": "", "stderr": "To github.com:x/y.git\n   a..b  issue-24 -> issue-24", "interrupted": false});
    assert_eq!(push(ok), Some("push"));
    let rejected = json!({"stderr": " ! [rejected]  issue-24 -> issue-24 (fetch first)\nerror: failed to push"});
    assert_eq!(push(rejected), None);
    assert_eq!(push(json!({"stderr": "fatal: unable to access"})), None);
    let ls = json!({"hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": {"command": "ls"}});
    assert_eq!(drover::hook::kick_why(&ls), None);
    assert_eq!(
        drover::hook::kick_why(&json!({"hook_event_name": "Stop"})),
        Some("stop")
    );
    let pre = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": "git push"}});
    assert_eq!(drover::hook::kick_why(&pre), None);
}

/// One test mutates `DROVER_LANE` (process-wide): the hook kicks in a lane only.
#[test]
fn the_hook_kicks_its_lanes_issue_and_is_inert_outside_lanes() {
    let c = ctx(&tmpdir("kick-hook"), CONFIG);
    // #511: the lane reported its turn, so the stop hook asks for none (tests/turn.rs).
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    std::fs::write(drover::hook::turn_mark(&c.state, "sendmeter:24:impl"), "turn").unwrap();
    let hook = |r: &Fake| {
        let stop = json!({"hook_event_name": "Stop", "session_id": "s"}).to_string();
        let argv = ["drover", "hook", "--role", "impl"].map(String::from).to_vec();
        let mut out = vec![];
        assert_eq!(drover::main_with(argv, &c, r, &mut out, &mut stop.as_bytes()), 0);
        assert!(out.is_empty());
    };
    std::env::remove_var("DROVER_OUTBOX");
    std::env::remove_var("DROVER_LANE");
    let r = Fake::default();
    hook(&r);
    assert!(r.calls.borrow().is_empty(), "inert: {:?}", r.calls.borrow());
    std::env::set_var("DROVER_LANE", "sendmeter:24:impl");
    let r = Fake::default();
    hook(&r);
    std::env::remove_var("DROVER_LANE");
    let calls = r.calls.borrow();
    assert_eq!(calls.len(), 1, "{calls:?}");
    let log = c.state.with_file_name("kick.log");
    let want = format!(
        "sh -c nohup '/opt/drover/bin/drover' kick sendmeter 24 --why stop --now >> '{}' 2>&1 < /dev/null & echo $!",
        log.display()
    );
    assert_eq!(calls[0], want);
}

/// #340: a kick is its own session and process group, out of reach of launchd's group kill when the job exits.
#[test]
fn a_detached_kick_runs_in_its_own_process_group() {
    use drover::runner::Runner;
    let log = tmpdir("kick-session").join("kick.log");
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    let pgid = |pid: u32| {
        let o = std::process::Command::new("ps")
            .args(["-o", "pgid=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    };
    let args = ["-c".to_string(), "ps -o pgid= -p $$".into()];
    let pid = drover::runner::Real.detach("sh", &args, &log).unwrap();
    let mut theirs = String::new();
    for _ in 0..100 {
        theirs = std::fs::read_to_string(&log).unwrap_or_default().trim().to_string();
        if !theirs.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(theirs, pid.to_string(), "the kick leads its own group");
    assert_ne!(theirs, pgid(std::process::id()), "not its parent's group");
}
