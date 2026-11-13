//! #511: an impl lane's typed turn end (`herdsman turn`), drive's action on each state, the stop hook's one prompt.
//! Every test here runs inside the same lane (`HERDSMAN_LANE` is process-wide: its own test binary).
mod common;
use common::*;
use herdsman::events::{append, fold, read, Event};
use serde_json::json;

const LANE: &str = "sendmeter-impl-12";

fn setup(name: &str) -> herdsman::Ctx {
    std::env::set_var("HERDSMAN_LANE", "sendmeter:12:impl");
    ctx(&tmpdir(name), CONFIG)
}

fn ev(c: &herdsman::Ctx, ts: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
}

fn sha(p: &str) -> String {
    format!("{p}{}", "0".repeat(40 - p.len()))
}

fn at(c: &herdsman::Ctx, secs: u64) -> herdsman::Ctx {
    herdsman::Ctx {
        now: c.now + secs,
        ..c.clone()
    }
}

/// #12's impl lane, spawned at ts 10 in a real worktree, and a Fake on which it sits idle in its pane (`live`) or is
/// gone; `pr` = the PR view (`None`: no PR), `head` = the worktree's (`None`: unread).
fn lane(name: &str, live: bool, pr: Option<&str>) -> (herdsman::Ctx, Fake) {
    lane_at(name, live, pr, None)
}

fn lane_at(name: &str, live: bool, pr: Option<&str>, head: Option<&str>) -> (herdsman::Ctx, Fake) {
    let c = setup(name);
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let d = json!({"role": "impl", "agent": LANE, "pane": "w9:p1", "workspace": "w9", "worktree": wt,
        "branch": "issue-12"});
    ev(&c, 10, "spawn", d);
    let list = agents(&[(LANE, "idle", &wt.to_string_lossy(), "w9:p1")]);
    let mut r = Fake::default()
        .reply("herdr agent list", if live { &list } else { NO_AGENTS })
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("herdr worktree", r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#)
        .reply(
            "herdr pane list",
            r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#,
        )
        .reply("herdr pane", "{}")
        .reply("herdr agent start", "{}")
        .reply("herdr agent prompt", "{}")
        .reply("herdr agent wait", "{}");
    if let Some(pr) = pr {
        r = r.reply("gh pr view issue-12", pr);
    }
    if let Some(h) = head {
        r = r.reply(&format!("git -C {} log", wt.display()), &format!("{h} 20"));
    }
    (c, r.reply("git -C", ""))
}

fn pr(head: &str, ci: &str) -> String {
    let check = match ci {
        "green" => json!({"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}),
        _ => json!({"__typename": "CheckRun", "name": "ci", "status": "IN_PROGRESS"}),
    };
    let body = format!("## For humans\n\nx\n\n## Evidence (head {head})");
    json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": head, "statusCheckRollup": [check], "body": body})
        .to_string()
}

/// `herdsman turn <args>` as the lane: its exit code.
fn turn(c: &herdsman::Ctx, r: &Fake, args: &[&str]) -> String {
    let (code, out) = run(c, r, &[&["turn"], args].concat());
    assert_eq!(code, 0, "{out}");
    out
}

/// `herdsman drive`: (output, `driven` steps logged by this pass).
fn drive(c: &herdsman::Ctx, r: &Fake) -> (String, Vec<String>) {
    let before = read(&c.state).unwrap().len();
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    let new = read(&c.state).unwrap().to_vec().split_off(before);
    let steps = new.iter().filter(|e| e.kind == "driven");
    (out, steps.map(|e| e.str("step").unwrap().to_string()).collect())
}

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

fn state(c: &herdsman::Ctx) -> String {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)]
        .state
        .clone()
}

/// The `waiting` and `status` lines of #12.
fn texts(c: &herdsman::Ctx, r: &Fake) -> (String, String) {
    (
        run(c, r, &["waiting", "sendmeter"]).1,
        run(c, r, &["status", "sendmeter"]).1,
    )
}

const NUDGE: &str = "herdr agent prompt sendmeter-impl-12 herdsman: you reported `done`, but nothing new is pushed";
const GO_ON: &str = "herdr agent prompt sendmeter-impl-12 herdsman: continue the task";

#[test]
fn the_verb_logs_turn_end_with_the_lanes_role_and_is_no_issue_activity() {
    let (c, r) = lane("turn-verb", true, None);
    let out = turn(&c, &r, &["working", "--why", "tests left"]);
    assert!(out.contains("recorded sendmeter#12 turn-end: working"), "{out}");
    let e = &kinds(&c, "turn-end")[0];
    assert_eq!(
        (e.str("state"), e.str("why"), e.str("role")),
        (Some("working"), Some("tests left"), Some("impl"))
    );
    assert_eq!(state(&c), "impl");
    assert_eq!(
        fold(&read(&c.state).unwrap())[&("sendmeter".into(), 12)].last_event,
        "spawn"
    );
    // `blocked` needs its reason.
    let (code, _) = run(&c, &r, &["turn", "blocked"]);
    assert_eq!(code, 2);
    assert_eq!(kinds(&c, "turn-end").len(), 1);
}

#[test]
fn no_state_reads_idle_without_pushing_as_before() {
    let (c, r) = lane("turn-none", true, None);
    let (w, st) = texts(&c, &r);
    assert!(w.contains("idle without pushing (check the pane)"), "{w}");
    assert!(st.contains("idle without pushing (check the pane)"), "{st}");
}

#[test]
fn done_at_a_pushed_head_spawns_review_at_once_without_waiting_for_ci() {
    // The PR head moved (the worktree is elsewhere), CI still running: no handoff before `done`.
    let head = sha("abc");
    let (c, r) = lane_at("turn-done-pushed", true, Some(&pr(&head, "pending")), Some(&sha("def")));
    let (w, _) = texts(&c, &r);
    assert!(w.contains("idle without pushing"), "{w}");
    // #677: with its why note (git and gh writes are the Fake's).
    ev(&c, 11, "decision", json!({"id": "q1", "choice": "yes", "by": "owner"}));
    let note = "Decided: d\nBecause: q1\nRejected: r — x\nCheck: c\nOwner: o\n";
    std::fs::write(c.worktrees.join("sendmeter/issue-12/why.md"), note).unwrap();
    turn(&c, &r, &["done"]);
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["review"], "{out}");
    assert!(out.contains(&format!("pushed: {head:.7} via pr")), "{out}");
    assert_eq!(state(&c), "review(1)");
    assert!(!r.called(NUDGE));
}

#[test]
fn done_with_nothing_pushed_is_nudged_once_then_blocked() {
    let (c, r) = lane("turn-done-empty", true, None);
    turn(&c, &r, &["done"]);
    let (w, st) = texts(&c, &r);
    assert!(
        w.contains("#12 drive: reported done, nothing new pushed: nudging it once"),
        "{w}"
    );
    assert!(
        st.contains("reported done, nothing new pushed") && !st.contains("idle without"),
        "{st}"
    );
    let (out, steps) = drive(&c, &r);
    assert!(
        steps.is_empty() && out.contains("sendmeter #12 turn done: prompted"),
        "{out}"
    );
    assert_eq!(r.count(NUDGE), 1, "{:?}", r.calls.borrow());
    // Nudged: the next pass waits, the nudge is never sent twice.
    let (w, _) = texts(&at(&c, 60), &r);
    assert!(w.contains("reported done, nothing pushed: nudged 60s ago"), "{w}");
    drive(&at(&c, 60), &r);
    assert_eq!(r.count(NUDGE), 1);
    // `done` again with still nothing pushed: an evidence mismatch, now blocked on the owner.
    turn(&at(&c, 120), &r, &["done"]);
    let (out, _) = drive(&at(&c, 120), &r);
    assert!(
        out.contains("needs-human: lane blocked: done again with nothing pushed"),
        "{out}"
    );
    assert_eq!(state(&c), "needs-human");
    let nh = &kinds(&c, "needs-human")[0];
    assert_eq!(nh.str("code"), Some("LANE_BLOCKED"));
    drive(&at(&c, 7200), &r);
    assert_eq!(
        (kinds(&c, "needs-human").len(), r.count(NUDGE)),
        (1, 1),
        "it never loops"
    );
    assert!(!r.called("herdr agent start"), "never respawned");
}

#[test]
fn a_nudged_done_left_unanswered_is_blocked_after_10_min() {
    let (c, r) = lane("turn-done-quiet", true, None);
    turn(&c, &r, &["done"]);
    drive(&c, &r);
    let (out, _) = drive(&at(&c, 601), &r);
    assert!(
        out.contains("needs-human: lane blocked: done again with nothing pushed"),
        "{out}"
    );
}

#[test]
fn blocked_is_an_owner_card_with_its_reason() {
    let (c, r) = lane("turn-blocked", true, None);
    turn(&c, &r, &["blocked", "--why", "the API key is missing"]);
    let (w, st) = texts(&c, &r);
    assert!(w.contains("reported blocked: the API key is missing"), "{w}");
    assert!(st.contains("reported blocked: the API key is missing"), "{st}");
    let (out, _) = drive(&c, &r);
    assert!(
        out.contains("sendmeter #12 needs-human: lane blocked: the API key is missing"),
        "{out}"
    );
    let nh = &kinds(&c, "needs-human")[0];
    assert_eq!(nh.str("reason"), Some("lane blocked: the API key is missing"));
    assert_eq!(state(&c), "needs-human");
    assert!(!r.called("herdr agent prompt") && !r.called("herdr agent start"));
}

#[test]
fn working_gets_one_continue_prompt_after_the_idle_threshold_never_a_respawn() {
    let (c, r) = lane("turn-working", true, None);
    turn(&c, &r, &["working", "--why", "two tests left"]);
    let (w, st) = texts(&at(&c, 300), &r);
    assert!(
        w.contains("reported working (two tests left): a continue prompt after 30m (5m)"),
        "{w}"
    );
    assert!(
        st.contains("reported working (two tests left)") && !st.contains("idle without"),
        "{st}"
    );
    let (out, _) = drive(&at(&c, 300), &r);
    assert!(!out.contains("turn working"), "{out}");
    assert_eq!(r.count(GO_ON), 0);
    let (out, _) = drive(&at(&c, 1801), &r);
    assert!(out.contains("sendmeter #12 turn working: prompted"), "{out}");
    assert_eq!(r.count(GO_ON), 1, "{:?}", r.calls.borrow());
    let (w, _) = texts(&at(&c, 1900), &r);
    assert!(w.contains("prompted to continue 99s ago"), "{w}");
    drive(&at(&c, 7200), &r);
    assert_eq!(r.count(GO_ON), 1);
    assert!(!r.called("herdr agent start") && kinds(&c, "respawn").is_empty());
}

#[test]
fn need_input_without_an_ask_says_so() {
    let (c, r) = lane("turn-input", true, None);
    turn(&c, &r, &["need-input", "--why", "which port"]);
    let (w, st) = texts(&c, &r);
    assert!(w.contains("reported need-input (which port) with no open ask"), "{w}");
    assert!(st.contains("reported need-input (which port)"), "{st}");
    // An open ask waits on the owner as before.
    let (code, out) = run(&c, &r, &["ask", "sendmeter", "12", "which port?"]);
    assert_eq!(code, 0, "{out}");
    let (w, _) = texts(&c, &r);
    assert!(w.contains("waiting on your answer"), "{w}");
}

#[test]
fn a_reported_lane_whose_pane_is_gone_takes_the_respawn_path() {
    let (c, r) = lane("turn-gone", false, None);
    turn(&c, &r, &["working"]);
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["respawn"], "{out}");
}

/// `herdsman hook` fed Claude's `event` JSON (`active`: its `stop_hook_active`): its stdout.
fn hook(c: &herdsman::Ctx, event: &str, active: bool) -> String {
    let input = json!({"hook_event_name": event, "stop_hook_active": active, "session_id": "s"}).to_string();
    let mut out = Vec::new();
    let argv = ["herdsman", "hook", "--role", "impl"].map(String::from).to_vec();
    let code = herdsman::main_with(argv, c, &Fake::default(), &mut out, &mut input.as_bytes());
    assert_eq!(code, 0);
    String::from_utf8(out).unwrap()
}

#[test]
fn the_stop_hook_asks_for_the_turn_once_per_prompt_and_never_loops() {
    let (c, r) = lane("turn-hook", true, None);
    let asked = |o: &str| {
        let v: serde_json::Value = serde_json::from_str(o.trim()).unwrap();
        v["decision"] == "block" && v["reason"] == herdsman::hook::TURN_ASK
    };
    assert_eq!(hook(&c, "UserPromptSubmit", false), "");
    let out = hook(&c, "Stop", false);
    assert!(asked(&out), "{out}");
    // Ended again unreported: the turn ends, no second prompt.
    assert_eq!(hook(&c, "Stop", false), "");
    // A new prompt: asked again, but never while Claude already goes on from a stop hook.
    hook(&c, "UserPromptSubmit", false);
    assert_eq!(hook(&c, "Stop", true), "");
    hook(&c, "UserPromptSubmit", false);
    assert!(asked(&hook(&c, "Stop", false)));
    // Reported: the turn ends.
    hook(&c, "UserPromptSubmit", false);
    turn(&c, &r, &["working"]);
    assert_eq!(hook(&c, "Stop", false), "");
}

#[test]
fn the_stop_hook_leaves_check_lanes_alone() {
    let c = setup("turn-hook-rev");
    let mark = herdsman::hook::turn_mark(&c.state, "sendmeter:12:rev");
    std::fs::create_dir_all(mark.parent().unwrap()).unwrap();
    let input = json!({"hook_event_name": "Stop"});
    assert!(herdsman::hook::turn_end(&input, &mark, || false).is_none());
    assert!(herdsman::hook::turn_end(&input, &mark, || true).is_some());
}
