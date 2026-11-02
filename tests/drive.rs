//! #92 `herdsman drive`: each step once per pass, a second pass a no-op; paused, HOST_BUSY and the lock.
mod common;
use common::*;
use herdsman::events::{append, fold, read, Event};
use serde_json::json;
use std::path::PathBuf;

const WS: &str = r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#;
const PANES: &str = r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#;

fn sha(p: &str) -> String {
    format!("{p}{}", "0".repeat(40 - p.len()))
}

fn setup(name: &str) -> herdsman::Ctx {
    ctx(&tmpdir(name), CONFIG)
}

fn ev(c: &herdsman::Ctx, ts: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
}

/// A lane of #12 logged as spawned, in a real worktree dir.
fn lane(c: &herdsman::Ctx, ts: u64, role: &str, agent: &str, dir: &str) -> PathBuf {
    let wt = c.worktrees.join("sendmeter").join(dir);
    std::fs::create_dir_all(&wt).unwrap();
    let d = json!({"role": role, "agent": agent, "pane": format!("w9:{agent}"), "workspace": "w9", "worktree": wt,
        "branch": "issue-12"});
    ev(c, ts, "spawn", d);
    wt
}

fn pr(head: &str, ci: &str) -> String {
    let check = match ci {
        "green" => json!({"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}),
        "red" => json!({"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "FAILURE",
            "detailsUrl": "https://github.com/jirathip-dev/sendmeter/actions/runs/777/job/1"}),
        _ => json!({"__typename": "CheckRun", "name": "ci", "status": "IN_PROGRESS"}),
    };
    json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": head, "statusCheckRollup": [check]})
        .to_string()
}

/// A Fake on which spawns succeed and the host is calm; `agents` = the herdr agent list.
fn fake(agents: &str) -> Fake {
    Fake::default()
        .reply("herdr agent list", agents)
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("herdr worktree", WS)
        .reply("herdr pane list", PANES)
        .reply("herdr pane run", "{}")
        .reply("herdr agent start", "{}")
        .reply("herdr pane", "{}")
        .reply("herdr agent prompt", "{}")
        .reply("herdr agent wait", "{}")
}

/// `herdsman drive`: (output, `driven` steps logged by this pass).
fn drive(c: &herdsman::Ctx, r: &Fake) -> (String, Vec<String>) {
    let before = read(&c.state).unwrap().len();
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    let new = read(&c.state).unwrap().split_off(before);
    (
        out,
        new.iter()
            .filter(|e| e.kind == "driven")
            .map(|e| e.str("step").unwrap().to_string())
            .collect(),
    )
}

/// The second pass changes nothing: no output, no event but its own `drive-pass` record (#166).
fn idle(c: &herdsman::Ctx, r: &Fake) {
    let acts = || {
        read(&c.state)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind != "drive-pass")
            .count()
    };
    let before = acts();
    assert_eq!(drive(c, r).0, "");
    assert_eq!(acts(), before);
}

fn state(c: &herdsman::Ctx) -> String {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)]
        .state
        .clone()
}

#[test]
fn start_spawns_the_next_queued_issue() {
    let c = setup("drive-start");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let r = fake(NO_AGENTS).reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["start"], "{out}");
    assert!(out.starts_with("sendmeter #12 start: "), "{out}");
    assert!(r.called("herdr agent start sendmeter-impl-12"));
    assert_eq!(state(&c), "impl");
    let wt = c.worktrees.join("sendmeter/issue-12");
    idle(
        &c,
        &fake(&agents(&[(
            "sendmeter-impl-12",
            "working",
            &wt.to_string_lossy(),
            "w9:p1",
        )])),
    );
}

#[test]
fn review_spawns_rev_at_the_pushed_head() {
    let c = setup("drive-review");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("abc");
    let r = fake(&agents(&[(
        "sendmeter-impl-12",
        "idle",
        &wt.to_string_lossy(),
        "w9:p1",
    )]))
    .reply("gh pr view issue-12", &pr(&head, "pending"))
    .reply(&format!("git -C {} log", wt.display()), &format!("{head} 20"))
    .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["review"], "{out}");
    assert!(r.called(
        "herdr worktree create --cwd /tmp/does-not-matter/sendmeter --branch rev-12-r1 --base origin/issue-12"
    ));
    assert_eq!(state(&c), "review(1)");
    idle(&c, &r);
}

#[test]
fn a_written_verdict_is_recorded_at_the_head_then_pending_ci_waits() {
    let c = setup("drive-verdict");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let rev = lane(&c, 20, "rev", "sendmeter-rev-12-r1", "rev-12-r1");
    std::fs::write(rev.join("verdict.md"), "PASS\nall good\n").unwrap();
    let head = sha("abc");
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&head, "pending"))
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply(&format!("git -C {} log", rev.display()), &format!("{head} 20"))
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["verdict"], "{out}");
    let all = read(&c.state).unwrap();
    let v = all.iter().find(|e| e.kind == "verdict").unwrap();
    assert_eq!((v.str("verdict"), v.str("sha")), (Some("PASS"), Some(head.as_str())));
    // PASS with CI pending: drive waits.
    idle(&c, &r);
    assert!(!r.called("gh pr merge"));
}

#[test]
fn a_head_moved_during_review_gets_a_fresh_reviewer_not_the_old_verdict() {
    let c = setup("drive-moved");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let rev = c.worktrees.join("sendmeter/rev-12-r1");
    std::fs::create_dir_all(&rev).unwrap();
    ev(
        &c,
        20,
        "spawn",
        json!({"role": "rev", "agent": "sendmeter-rev-12-r1", "pane": "w9:r", "workspace": "w9",
        "worktree": rev, "branch": "rev-12-r1", "sha": sha("0d")}),
    );
    std::fs::write(rev.join("verdict.md"), "PASS\nall good\n").unwrap();
    let head = sha("bee");
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&head, "pending"))
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply(
            &format!("git -C {} rev-parse HEAD", rev.display()),
            &format!("{head}\n"),
        )
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["review"], "{out}");
    let all = read(&c.state).unwrap();
    assert!(
        !all.iter().any(|e| e.kind == "verdict"),
        "the old head's verdict is not recorded for the new head"
    );
    assert!(r.called(&format!("git -C {} reset --hard origin/issue-12", rev.display())));
    assert!(!rev.join("verdict.md").exists(), "the stale verdict is gone");
    let spawn = all.iter().rev().find(|e| e.kind == "spawn").unwrap();
    assert_eq!(
        (spawn.str("role"), spawn.str("sha")),
        (Some("rev"), Some(head.as_str())),
        "logged at the new head"
    );
    // The fresh reviewer's verdict is recorded at the head it was spawned at.
    std::fs::write(rev.join("verdict.md"), "PASS\n").unwrap();
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["verdict"], "{out}");
    let v = read(&c.state)
        .unwrap()
        .into_iter()
        .find(|e| e.kind == "verdict")
        .expect(&out);
    assert_eq!(v.str("sha"), Some(head.as_str()));
}

#[test]
fn pass_and_green_ci_merges() {
    let c = setup("drive-merge");
    lane(&c, 10, "impl", "sendmeter-impl-12", "gone-12");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/gone-12")).unwrap();
    let head = sha("abc");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
    );
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&head, "green"))
        .reply("gh pr merge 100", "")
        .reply("gh pr view 100", r#"{"mergeCommit":{"oid":"m3rg3"}}"#)
        .reply("gh issue", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["merge"], "{out}");
    assert!(r.called(&format!(
        "gh pr merge 100 -R jirathip-dev/sendmeter --merge --match-head-commit {head}"
    )));
    assert_eq!(state(&c), "merged");
    idle(&c, &r);
}

#[test]
fn a_blocked_ci_not_green_item_merges_once_ci_turns_green() {
    let c = setup("drive-blocked-retry");
    lane(&c, 10, "impl", "sendmeter-impl-12", "gone-12");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/gone-12")).unwrap();
    let head = sha("abc");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
    );
    ev(
        &c,
        40,
        "blocked",
        json!({"code": "CI_NOT_GREEN", "reason": "no checks reported"}),
    );
    assert_eq!(state(&c), "blocked");
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&head, "green"))
        .reply("gh pr merge 100", "")
        .reply("gh pr view 100", r#"{"mergeCommit":{"oid":"m3rg3"}}"#)
        .reply("gh issue", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["merge"], "{out}");
    assert_eq!(state(&c), "merged");
}

#[test]
fn a_fail_starts_a_fix_round_with_the_findings_verbatim() {
    let c = setup("drive-fix");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let body = "FAIL\n1. blocking: src/x.rs:3 off by one";
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "FAIL", "sha": sha("abc"), "body": body}),
    );
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&sha("abc"), "green"))
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["fix"], "{out}");
    let brief = std::fs::read_to_string(wt.join(".brief.md")).unwrap();
    assert!(brief.contains(body), "{brief}");
    assert!(brief.contains("If a finding is wrong or out of scope, say so in your report with file:line proof"));
    assert_eq!(state(&c), "fix(1)");
    idle(
        &c,
        &fake(&agents(&[(
            "sendmeter-impl-12",
            "working",
            &wt.to_string_lossy(),
            "w9:p1",
        )])),
    );
}

#[test]
fn the_third_fail_stops_at_needs_human_with_the_findings() {
    let c = setup("drive-limit");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(
        &c,
        11,
        "verdict",
        json!({"verdict": "FAIL", "sha": sha("1"), "body": "FAIL"}),
    );
    ev(
        &c,
        12,
        "verdict",
        json!({"verdict": "FAIL", "sha": sha("2"), "body": "FAIL"}),
    );
    let rev = lane(&c, 20, "rev", "sendmeter-rev-12-r3", "rev-12-r3");
    std::fs::write(rev.join("verdict.md"), "FAIL\nsrc/y.rs:9 still wrong\n").unwrap();
    let head = sha("3");
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&head, "green"))
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply(&format!("git -C {} log", rev.display()), &format!("{head} 20"))
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["verdict"], "{out}");
    assert_eq!(state(&c), "needs-human");
    let all = read(&c.state).unwrap();
    let nh = all.iter().find(|e| e.kind == "needs-human").unwrap();
    assert!(nh.str("findings").unwrap().contains("src/y.rs:9 still wrong"));
    idle(&c, &r);
    assert!(!r.called("herdr agent start"), "no fix lane after the limit");
}

#[test]
fn red_ci_after_pass_is_a_fail_with_the_log_tail() {
    let c = setup("drive-red");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    let head = sha("abc");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
    );
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&head, "red"))
        .reply(
            "gh run view 777 -R jirathip-dev/sendmeter --log-failed",
            "test x ... FAILED\nGH_TOKEN=ghp_abcdef123456\nerror: 1 test failed\n",
        )
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["ci-red"], "{out}");
    let all = read(&c.state).unwrap();
    let v = all.iter().rev().find(|e| e.kind == "verdict").unwrap();
    assert_eq!(v.str("verdict"), Some("FAIL"));
    assert!(
        v.str("body").unwrap().contains("error: 1 test failed"),
        "{:?}",
        v.str("body")
    );
    assert!(
        !v.str("body").unwrap().contains("ghp_abcdef"),
        "the log tail is masked: {:?}",
        v.str("body")
    );
    let st = &fold(&all)[&("sendmeter".to_string(), 12)];
    assert_eq!((st.state.as_str(), st.rounds), ("fix(1)", 1), "same head: no new round");
}

#[test]
fn clean_reclaims_merged_lanes() {
    let c = setup("drive-clean");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(&c, 30, "merged", json!({"pr": "100"}));
    let r = fake(NO_AGENTS)
        .reply(&format!("git -C {} rev-list", wt.display()), "0\n")
        .reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["clean"], "{out}");
    assert!(r.called("herdr worktree remove --workspace w9"));
    std::fs::remove_dir_all(&wt).unwrap(); // herdr removed it
    idle(&c, &r);
}

#[test]
fn paused_fleets_and_host_busy_are_no_ops() {
    let c = setup("drive-paused");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    assert_eq!(run(&c, &Fake::default(), &["pause", "sendmeter"]).0, 0);
    let r = fake(NO_AGENTS).reply("git -C", "");
    idle(&c, &r);
    assert!(!r.called("herdr"), "{:?}", r.calls.borrow());
    assert_eq!(
        herdsman::drive::last_pass(&c, "sendmeter"),
        None,
        "a paused fleet had no pass"
    );

    let c = setup("drive-busy");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let busy = Fake::default().once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "4\n");
    let busy = Fake {
        replies: fake(NO_AGENTS).reply("git -C", "").replies,
        ..busy
    };
    let (out, steps) = drive(&c, &busy);
    assert_eq!(steps, ["start"]);
    assert!(out.contains("REFUSED HOST_BUSY"), "{out}");
    assert!(!busy.called("herdr worktree"));
    // Retried next pass, logged and printed once per hour.
    let busy = Fake {
        replies: fake(NO_AGENTS).reply("git -C", "").replies,
        ..Default::default()
    }
    .once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "4\n");
    let (out, steps) = drive(&c, &busy);
    assert!(out.is_empty() && steps.is_empty(), "{out}");
    assert_eq!(
        busy.count("sysctl -n kern.memorystatus_vm_pressure_level"),
        1,
        "retried"
    );
    assert_eq!(state(&c), "queued");
}

#[test]
fn the_lock_keeps_passes_apart_and_a_stale_one_is_taken_over() {
    let c = setup("drive-lock");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let lock = c.state.parent().unwrap().join("drive.lock");
    std::fs::write(&lock, "").unwrap();
    let r = fake(NO_AGENTS).reply("git -C", "");
    let (out, steps) = drive(&c, &r);
    assert!(out.contains("another pass holds") && steps.is_empty(), "{out}");
    assert!(!r.called("herdr"));
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    std::fs::File::options()
        .write(true)
        .open(&lock)
        .unwrap()
        .set_modified(old)
        .unwrap();
    assert_eq!(drive(&c, &r).1, ["start"]);
    assert!(!lock.exists(), "released after the pass");
    assert_eq!(herdsman::drive::last_pass(&c, "sendmeter"), Some(c.now));
}

#[test]
fn drive_now_on_the_board_is_paired_only_and_toasts_its_line() {
    let c = setup("drive-web");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let r = fake(NO_AGENTS).reply("git -C", "");
    let s = herdsman::serve::Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    let get = |s: &herdsman::serve::Server| {
        let res = s.respond("GET / HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n\r\n", c.now);
        String::from_utf8_lossy(&res).into_owned()
    };
    let page = get(&s);
    assert!(page.contains("<button>Drive now</button>") && page.contains("driver: herdsman · no pass yet"));
    let i = page.find("name=mtime value=").unwrap() + 17;
    let mtime: String = page[i..].chars().take_while(char::is_ascii_digit).collect();
    let body = format!("op=drive&fleet=sendmeter&mtime={mtime}");
    let post = |host: &str, extra: &str| {
        let req = format!(
            "POST / HTTP/1.1\r\nHost: {host}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        String::from_utf8_lossy(&s.respond(&req, c.now)).into_owned()
    };
    assert!(post("127.0.0.1:7777", "").starts_with("HTTP/1.1 401"));
    let cookie = pair(&s, &c, "", c.now);
    assert!(post("evil.example", &cookie).starts_with("HTTP/1.1 403"));
    assert!(!r.called("herdr agent start"));
    let res = post("127.0.0.1:7777", &cookie);
    assert!(
        res.starts_with("HTTP/1.1 303") && res.contains("sendmeter%3A%20started%20%2312"),
        "{res}"
    );
    assert!(r.called("herdr agent start sendmeter-impl-12"));
    assert!(get(&s).contains("driver: herdsman · last pass 0s ago"));
}

/// #140: a drive pass's lines as one toast, per action, then the refusals by code.
#[test]
fn the_drive_summary_groups_per_action() {
    let out = "sendmeter #2752 merge: merged #2752 into staging\nsendmeter #2755 start: spawned sendmeter-impl-2755\n\
        sendmeter #2781 merge: merged\nsendmeter #9 review: spawned\nsendmeter #2756 start: REFUSED SERIAL_BUSY: x\n\
        sendmeter #2757 start: REFUSED HOST_BUSY: y\nsendmeter #3 fix: error: boom\nsendmeter #4 start: REFUSED HOST_BUSY: z\n\
        sendmeter clean: removed 1\n";
    assert_eq!(
        herdsman::drive::summary("sendmeter", out),
        "sendmeter: clean · merged #2752, #2781 · reviewed #9 · started #2755 · 4 skipped (HOST_BUSY, SERIAL_BUSY, error)"
    );
    assert_eq!(herdsman::drive::summary("sendmeter", ""), "sendmeter: nothing due");
}

/// Sleeps 3 s on each agent start: a slow drive pass.
struct Slow(Fake);
impl herdsman::runner::Runner for Slow {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<herdsman::runner::Out> {
        if prog == "herdr" && args.join(" ").starts_with("agent start") {
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
        self.0.run(prog, args)
    }
}

/// #140: `[Drive now]` replies at once and runs on the job thread; a second tap is DRIVE_RUNNING, and the
/// board (a thread per connection) answers a GET meanwhile; the result reaches `/live` as a toast.
#[test]
fn drive_now_runs_in_the_background_and_the_board_keeps_answering() {
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};
    let c = setup("drive-bg");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let cookie = pair(
        &herdsman::serve::Server::new(&c, &Fake::default(), Some("127.0.0.1:7777".into())),
        &c,
        "",
        c.now,
    );
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || herdsman::serve::serve_on(&c2, &Slow(fake(NO_AGENTS).reply("git -C", "")), l, false));
    let send = |req: String| {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        s.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    };
    let get = |path: &str, extra: &str| send(format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n{extra}\r\n"));
    let page = get("/", &cookie);
    let i = page.find("name=mtime value=").unwrap() + 17;
    let mtime: String = page[i..].chars().take_while(char::is_ascii_digit).collect();
    let body = format!("op=drive&fleet=sendmeter&mtime={mtime}");
    let post = || {
        let t = Instant::now();
        let req = format!(
            "POST / HTTP/1.1\r\nHost: {addr}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        (send(req), t.elapsed())
    };
    let mut live = std::net::TcpStream::connect(addr).unwrap();
    live.write_all(format!("GET /live HTTP/1.1\r\nHost: {addr}\r\n{cookie}\r\n").as_bytes())
        .unwrap();
    let (res, took) = post();
    assert!(res.contains("drive%20started%20for%20sendmeter"), "{res}");
    assert!(took < Duration::from_millis(200), "the POST took {took:?}");
    let (res, _) = post();
    assert!(res.contains("REFUSED%20DRIVE%5FRUNNING"), "{res}");
    let t = Instant::now();
    assert!(get("/", "").starts_with("HTTP/1.1 200"));
    assert!(t.elapsed() < Duration::from_secs(1), "GET / waited {:?}", t.elapsed());
    // The pass ends: its summary reaches the board as a toast, and a new tap starts another pass.
    live.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let (mut seen, mut b) = (String::new(), [0u8; 65_536]);
    while !seen.contains("event: toast") {
        let n = live.read(&mut b).unwrap();
        assert!(n > 0, "the stream closed: {seen}");
        seen += &String::from_utf8_lossy(&b[..n]);
    }
    assert!(
        seen.contains("event: toast\ndata: \"sendmeter: started #12\""),
        "{seen}"
    );
    assert!(post().0.contains("drive%20started"), "the flag is cleared");
}

/// A PR view whose `mergeable` / `mergeStateStatus` are set (CI green).
fn stale_pr(head: &str, mergeable: &str, status: &str) -> String {
    let mut v: serde_json::Value = serde_json::from_str(&pr(head, "green")).unwrap();
    v["mergeable"] = json!(mergeable);
    v["mergeStateStatus"] = json!(status);
    v.to_string()
}

#[test]
fn a_conflicting_pr_after_pass_gets_update_rounds_then_needs_human() {
    let c = setup("drive-conflict");
    let wt = lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    for (i, (v, p)) in [("FAIL", "a1"), ("FAIL", "b2")].into_iter().enumerate() {
        ev(
            &c,
            11 + i as u64,
            "verdict",
            json!({"verdict": v, "sha": sha(p), "body": v}),
        );
    }
    // #123: round 3 PASSes, then three update rounds PASS; none of them is a review round.
    for (k, p) in [(1, "c3"), (2, "d4"), (3, "e5"), (4, "f6")] {
        ev(
            &c,
            20 + k,
            "verdict",
            json!({"verdict": "PASS", "sha": sha(p), "body": "PASS"}),
        );
        herdsman::gh::forget_pr(&c, "sendmeter", 12);
        let r = fake(NO_AGENTS)
            .reply("gh pr view issue-12", &stale_pr(&sha(p), "CONFLICTING", "DIRTY"))
            .reply("git -C", "");
        let s = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
        assert_eq!((s.rounds, s.updates), (3, u32::try_from(k - 1).unwrap()));
        let (_, out) = run(&c, &r, &["waiting", "sendmeter"]);
        assert!(out.contains("conflict: PR #100 conflicts with staging"), "{out}");
        assert!(
            out.contains("herdsman spawn sendmeter 12 impl --supersede --update"),
            "{out}"
        );
        let (out, steps) = drive(&c, &r);
        assert_eq!(steps, ["conflict"], "{out}");
        if k < 4 {
            assert_eq!(state(&c), format!("fix(3)+u{k}"));
            let brief = std::fs::read_to_string(wt.join(".brief.md")).unwrap();
            assert!(brief.contains("Merge `origin/staging` into this branch"), "{brief}");
            assert!(!brief.contains("Review findings to fix"), "{brief}");
            lane(&c, 30 + k, "rev", "sendmeter-rev-12", "issue-12-rev");
            assert_eq!(state(&c), format!("review(3)+u{k}"));
        } else {
            assert!(out.contains("REFUSED UPDATE_LIMIT"), "{out}");
            assert_eq!(state(&c), "needs-human");
            assert_eq!(r.count("herdr agent start"), 0, "no fourth update lane");
            idle(&c, &r);
        }
    }
}

#[test]
fn a_behind_pr_after_pass_is_updated_the_same_way() {
    let c = setup("drive-behind");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(
        &c,
        20,
        "verdict",
        json!({"verdict": "PASS", "sha": sha("a1"), "body": "PASS"}),
    );
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &stale_pr(&sha("a1"), "MERGEABLE", "BEHIND"))
        .reply("git -C", "");
    let (_, out) = run(&c, &r, &["waiting", "sendmeter"]);
    assert!(out.contains("conflict: PR #100 is behind staging"), "{out}");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["conflict"], "{out}");
    assert_eq!(state(&c), "fix(1)+u1");
}

#[test]
fn a_pushed_update_or_fix_head_still_conflicting_gets_a_review_not_another_update() {
    let c = setup("drive-conflict-stale");
    lane(&c, 10, "impl", "sendmeter-impl-12", "issue-12");
    ev(
        &c,
        20,
        "verdict",
        json!({"verdict": "PASS", "sha": sha("a1"), "body": "PASS"}),
    );
    let pr_at = |head: &str| {
        herdsman::gh::forget_pr(&c, "sendmeter", 12);
        fake(NO_AGENTS)
            .reply("gh pr view issue-12", &stale_pr(&sha(head), "CONFLICTING", "DIRTY"))
            .reply("git -C", "")
    };
    assert_eq!(drive(&c, &pr_at("a1")).1, ["conflict"]);
    assert_eq!(state(&c), "fix(1)+u1");
    // The update lane pushed b2; GitHub has not recomputed `mergeable` yet.
    let (_, out) = run(&c, &pr_at("b2"), &["waiting", "sendmeter"]);
    assert!(
        out.contains("waiting for drive (next pass ≤ 5 min) to spawn review"),
        "{out}"
    );
    assert!(
        out.contains("herdsman spawn sendmeter 12 rev") && !out.contains("--update"),
        "{out}"
    );
    // A FAIL's fix round pushing a conflicting head is reviewed, too.
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "FAIL", "sha": sha("b2"), "body": "FAIL"}),
    );
    ev(
        &c,
        31,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"}),
    );
    let (_, out) = run(&c, &pr_at("c3"), &["waiting", "sendmeter"]);
    assert!(
        out.contains("waiting for drive (next pass ≤ 5 min) to spawn review") && !out.contains("--update"),
        "{out}"
    );
    let (code, out) = run(&c, &Fake::default(), &["spawn", "sendmeter", "12", "rev", "--update"]);
    assert!(code == 3 && out.contains("REFUSED BAD_VALUE"), "{out}");
}

const SERIAL: &str = "serial_paths = [\"supabase/migrations/**\"]";

/// #100: a driven fleet with `extra` config lines, issues `ns` queued.
fn fanout(name: &str, extra: &str, ns: &[&str]) -> herdsman::Ctx {
    let c = ctx(
        &tmpdir(name),
        &CONFIG.replace(
            "integration = \"staging\"",
            &format!("integration = \"staging\"\n{extra}"),
        ),
    );
    let add = [&["queue", "sendmeter", "add"][..], ns].concat();
    assert_eq!(run(&c, &Fake::default(), &add).0, 0);
    c
}

fn fan_fake(agents: &str) -> Fake {
    fake(agents)
        .reply("git -C", "")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
}

/// The herdr agent list with issues `ns`' impl lanes working.
fn working(c: &herdsman::Ctx, ns: &[u64]) -> String {
    let wt = |n: u64| {
        c.worktrees
            .join(format!("sendmeter/issue-{n}"))
            .to_string_lossy()
            .to_string()
    };
    let rows: Vec<(String, String)> = ns.iter().map(|n| (format!("sendmeter-impl-{n}"), wt(*n))).collect();
    let rows: Vec<_> = rows
        .iter()
        .map(|(a, w)| (a.as_str(), "working", w.as_str(), "w9:p1"))
        .collect();
    agents(&rows)
}

fn state_of(c: &herdsman::Ctx, n: u64) -> String {
    fold(&read(&c.state).unwrap())
        .get(&("sendmeter".to_string(), n))
        .map_or("queued".into(), |s| s.state.clone())
}

#[test]
fn one_pass_starts_every_free_item_in_order_with_a_fresh_host_sample_each() {
    let c = fanout("drive-fan-all", "", &["1", "2", "3"]);
    let r = fan_fake(NO_AGENTS);
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["start", "start", "start"], "{out}");
    let at = |n: u64| r.index(&format!("herdr agent start sendmeter-impl-{n}")).unwrap();
    assert!(at(1) < at(2) && at(2) < at(3), "{:?}", r.calls.borrow());
    assert_eq!(
        r.count("sysctl -n kern.memorystatus_vm_pressure_level"),
        3,
        "one sample per start"
    );
    assert!((1..=3).all(|n| state_of(&c, n) == "impl"));
    // Nothing free on the second pass: a no-op.
    idle(&c, &fake(&working(&c, &[1, 2, 3])));
}

#[test]
fn a_serial_busy_item_is_skipped_and_keeps_its_place() {
    let c = fanout("drive-fan-serial", SERIAL, &["9", "1", "--serial"]);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "2", "3"]).0, 0);
    let wt = c.worktrees.join("sendmeter/issue-9");
    std::fs::create_dir_all(&wt).unwrap();
    let d = json!({"role": "impl", "agent": "sendmeter-impl-9", "worktree": wt, "branch": "issue-9"});
    append(&c.state, &Event::new(5, "sendmeter", Some(9), "spawn", d)).unwrap();
    let r = fan_fake(&working(&c, &[9]));
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["start", "start", "start"], "{out}");
    assert!(out.contains("sendmeter #1 start: REFUSED SERIAL_BUSY"), "{out}");
    assert!(!r.called("herdr agent start sendmeter-impl-1"));
    assert_eq!((state_of(&c, 2), state_of(&c, 3)), ("impl".into(), "impl".into()));
    assert_eq!(
        run(&c, &Fake::default(), &["next", "sendmeter"]),
        (0, "1\n".into()),
        "1 stays first"
    );
    let status = run(&c, &fake(&working(&c, &[2, 3, 9])), &["status", "sendmeter"]).1;
    assert!(
        status.contains("skipped this pass: SERIAL_BUSY (serial #9 is unmerged (impl)"),
        "{status}"
    );
    let board = run(&c, &Fake::default(), &["board", "sendmeter"]).1;
    assert!(board.contains(" #1     queued      skip SERIAL_BUSY "), "{board}");
    // The second pass: #1 still refused (logged once an hour), nothing else free: a no-op.
    let r = fan_fake(&working(&c, &[2, 3, 9]));
    idle(&c, &r);
}

/// #103: DISK_LOW stops the pass like HOST_BUSY: #3 is not tried.
#[test]
fn disk_low_stops_the_pass() {
    let c = fanout("drive-fan-disk", "", &["1", "2", "3"]);
    let df = |gb: u64| {
        format!(
            "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/d 1 1 {} 1% /\n",
            gb << 20
        )
    };
    let r = fan_fake(NO_AGENTS)
        .once("df -k /", 0, &df(50))
        .once("df -k /", 0, &df(5));
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["start", "start"], "{out}");
    assert!(out.contains("sendmeter #2 start: REFUSED DISK_LOW"), "{out}");
    assert_eq!(r.count("df -k /"), 2, "#3 not tried");
    assert_eq!(state_of(&c, 3), "queued");
}

#[test]
fn host_busy_stops_the_pass_and_cap_stops_starting() {
    let c = fanout("drive-fan-busy", "", &["1", "2", "3"]);
    let busy = Fake {
        replies: fan_fake(NO_AGENTS).replies,
        ..Default::default()
    }
    .once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "1\n")
    .once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "4\n");
    let (out, steps) = drive(&c, &busy);
    assert_eq!(steps, ["start", "start"], "{out}");
    assert!(out.contains("sendmeter #2 start: REFUSED HOST_BUSY"), "{out}");
    assert_eq!(
        busy.count("sysctl -n kern.memorystatus_vm_pressure_level"),
        2,
        "#3 not tried"
    );
    assert_eq!(state_of(&c, 3), "queued");

    let c = fanout("drive-fan-cap", "max_impl = 1", &["1", "2", "3"]);
    let r = fan_fake(NO_AGENTS);
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["start", "start"], "{out}");
    assert!(out.contains("sendmeter #2 start: REFUSED CAP"), "{out}");
    assert!(!out.contains("#3"), "{out}");
    assert_eq!((state_of(&c, 1), state_of(&c, 3)), ("impl".into(), "queued".into()));
    idle(&c, &fan_fake(&working(&c, &[1])));
}

/// #149: unmarked #12 at PASS + green whose PR changes `files`: one drive pass, then the `serial-auto` events.
fn serial_auto(name: &str, files: &str) -> (herdsman::Ctx, Fake, String, Vec<Event>) {
    let cfg = CONFIG.replace(
        "integration = \"staging\"",
        &format!("integration = \"staging\"\ndriver = \"herdsman\"\n{SERIAL}"),
    );
    let c = ctx(&tmpdir(name), &cfg);
    lane(&c, 10, "impl", "sendmeter-impl-12", "gone-12");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/gone-12")).unwrap();
    let head = sha("abc");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
    );
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &pr(&head, "green"))
        .reply("gh pr diff 100", files)
        .reply("gh pr merge 100", "")
        .reply("gh pr view 100", r#"{"mergeCommit":{"oid":"m3rg3"}}"#)
        .reply("gh issue", "");
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["merge"], "{out}");
    assert!(!out.contains("SERIAL_UNMARKED"), "{out}");
    assert_eq!(state(&c), "merged");
    let auto = read(&c.state).unwrap().into_iter().filter(|e| e.kind == "serial-auto");
    (c, r, out, auto.collect())
}

#[test]
fn an_unmarked_pr_touching_a_serial_path_is_auto_marked_and_merges() {
    let (c, r, out, auto) = serial_auto("drive-serial-auto", "src/a.rs\nsupabase/migrations/1_x.sql\n");
    assert!(
        out.contains("sendmeter #12 serial-auto: supabase/migrations/1_x.sql"),
        "{out}"
    );
    assert_eq!(auto.len(), 1);
    assert_eq!(
        (auto[0].issue, auto[0].str("path")),
        (Some(12), Some("supabase/migrations/1_x.sql"))
    );
    idle(&c, &r);
}

#[test]
fn a_pr_off_serial_paths_is_not_marked() {
    let (_, _, out, auto) = serial_auto("drive-serial-none", "src/a.rs\n");
    assert!(auto.is_empty(), "{out}");
}

/// A PR view (CI: the full run not reported yet) carrying `labels`.
fn labelled_pr(head: &str, labels: &[&str]) -> String {
    let mut v: serde_json::Value = serde_json::from_str(&pr(head, "pending")).unwrap();
    v["labels"] = json!(labels.iter().map(|l| json!({"name": l})).collect::<Vec<_>>());
    v.to_string()
}

#[test]
fn a_pass_at_the_head_gets_the_merge_label_once_and_a_moved_head_loses_it() {
    let dir = tmpdir("drive-label");
    let cfg = CONFIG.replace(
        "integration = \"staging\"",
        "integration = \"staging\"\nmerge_label = \"merge-ready\"",
    );
    let c = ctx(&dir, &cfg);
    lane(&c, 10, "impl", "sendmeter-impl-12", "gone-12");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/gone-12")).unwrap();
    let (head, moved) = (sha("abc"), sha("def"));
    let file = dir.join("verdict.md");
    std::fs::write(&file, "PASS\nall good\n").unwrap();
    let add = "gh pr edit 100 -R jirathip-dev/sendmeter --add-label merge-ready";
    let kinds = |k: &str| read(&c.state).unwrap().iter().filter(|e| e.kind == k).count();
    // PASS at the head: labelled once, by the verdict itself.
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &labelled_pr(&head, &[]))
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply("gh pr edit", "");
    let f = file.to_string_lossy();
    let (code, out) = run(
        &c,
        &r,
        &["verdict", "sendmeter", "12", "PASS", "--sha", &head, "--file", &f],
    );
    assert_eq!(code, 0, "{out}");
    assert!(r.called(add), "{out}");
    assert_eq!(kinds("labelled"), 1);
    assert_eq!(state(&c), "ready", "labelled is not activity");
    // The label shows on the PR: the next pass doesn't re-add it.
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &labelled_pr(&head, &["merge-ready"]))
        .reply("gh pr edit", "");
    idle(&c, &r);
    assert!(!r.called("gh pr edit"));
    // The head moved past the PASS (a fresh PR read, as after 60 s): the label comes off, and is never added back.
    herdsman::gh::forget_pr(&c, "sendmeter", 12);
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &labelled_pr(&moved, &["merge-ready"]))
        .reply("gh pr edit", "");
    let (out, _) = drive(&c, &r);
    assert!(
        r.called("gh pr edit 100 -R jirathip-dev/sendmeter --remove-label merge-ready"),
        "{out}"
    );
    assert_eq!((kinds("labelled"), kinds("unlabelled")), (1, 1));
    herdsman::gh::forget_pr(&c, "sendmeter", 12);
    let r = fake(NO_AGENTS)
        .reply("gh pr view issue-12", &labelled_pr(&moved, &[]))
        .reply("gh pr edit", "");
    drive(&c, &r);
    assert!(!r.called("gh pr edit"), "no label for the old PASS");
    assert_eq!(kinds("labelled"), 1);
}

#[test]
fn status_says_a_ready_pr_lacks_the_merge_label() {
    let c = ctx(
        &tmpdir("drive-label-status"),
        &CONFIG.replace(
            "integration = \"staging\"",
            "integration = \"staging\"\nmerge_label = \"merge-ready\"",
        ),
    );
    lane(&c, 10, "impl", "sendmeter-impl-12", "gone-12");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/gone-12")).unwrap();
    let head = sha("abc");
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "body": "PASS"}),
    );
    let r = fake(NO_AGENTS).reply("gh pr view issue-12", &labelled_pr(&head, &[]));
    // Prime the PR cache the way a pass does; `status` reads only the cache.
    run(&c, &r, &["pr", "sendmeter", "12"]);
    let (code, out) = run(&c, &r, &["status", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("PR lacks label `merge-ready`"), "{out}");
}

/// #166: the `drive-pass` events of `c` (newest last).
fn passes(c: &herdsman::Ctx) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "drive-pass")
        .collect()
}

#[test]
fn every_pass_manual_or_automatic_writes_a_drive_pass_with_each_action_a_silent_repeat_too() {
    let c = setup("drive-pass");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let busy = || {
        Fake {
            replies: fake(NO_AGENTS).reply("git -C", "").replies,
            ..Default::default()
        }
        .once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "4\n")
    };
    drive(&c, &busy());
    let mut out = vec![];
    let cfg = herdsman::config::Config::load(&c.config).unwrap();
    // The second pass is a manual press and prints nothing (a repeat), but its record still lists the refusal.
    herdsman::drive::drive(&c, &cfg, &busy(), &mut out, Some("sendmeter"), true).unwrap();
    let p = passes(&c);
    assert_eq!(p.len(), 2, "{p:?}");
    assert_eq!((p[0].str("by"), p[1].str("by")), (Some("auto"), Some("manual")));
    for e in &p {
        assert_eq!(e.fleet, "sendmeter");
        assert_eq!(e.data["started"], c.now);
        assert!(e.data["ended"].as_u64().unwrap() >= c.now);
        let a = &e.data["actions"][0];
        assert_eq!((a["issue"].as_u64(), a["step"].as_str()), (Some(12), Some("start")));
        assert_eq!(
            (a["outcome"].as_str(), a["code"].as_str()),
            (Some("waiting"), Some("HOST_BUSY"))
        );
        assert!(
            a["words"].as_str().unwrap().starts_with("Mac busy (memory pressure"),
            "{a}"
        );
    }
    let (tone, line) = herdsman::drive::result(&p[1]);
    assert_eq!(tone, "a", "everything held back: amber");
    assert!(
        line.ends_with(" · started 0 · merged 0 · 1 waiting: Mac busy (memory pressure critical)"),
        "{line}"
    );
}

/// The other pass: it ends (its record, then the lock goes) on the press's 3rd one-second wait.
struct Other<'a>(&'a Fake, &'a herdsman::Ctx, std::sync::atomic::AtomicUsize);
impl herdsman::runner::Runner for Other<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<herdsman::runner::Out> {
        self.0.run(prog, args)
    }
    fn sleep(&self, _: u64) {
        if self.2.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 2 {
            let a = json!([{"issue": 12, "step": "start", "outcome": "started", "code": null, "words": "started"},
                {"issue": 13, "step": "start", "outcome": "waiting", "code": "CAP", "words": "lane limit or usage cap reached"}]);
            let d =
                json!({"by": "auto", "started": self.1.now - 3, "ended": self.1.now + 2, "at": "07:41", "actions": a});
            append(
                &self.1.state,
                &Event::new(self.1.now - 3, "sendmeter", None, "drive-pass", d),
            )
            .unwrap();
            std::fs::remove_file(self.1.state.parent().unwrap().join("drive.lock")).unwrap();
        }
    }
}

#[test]
fn a_manual_press_that_finds_a_pass_running_waits_for_it_and_shows_its_actions() {
    let c = setup("drive-wait");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let lock = c.state.parent().unwrap().join("drive.lock");
    std::fs::write(&lock, "").unwrap();
    let cfg = herdsman::config::Config::load(&c.config).unwrap();
    assert!(herdsman::drive::running(&c, "sendmeter"));
    let (f, mut out) = (fake(NO_AGENTS).reply("git -C", ""), vec![]);
    let r = Other(&f, &c, 0.into());
    herdsman::drive::drive(&c, &cfg, &r, &mut out, Some("sendmeter"), true).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert_eq!(
        out,
        "sendmeter #12 start: started\nsendmeter #13 start: REFUSED CAP: lane limit or usage cap reached\n"
    );
    assert_eq!(r.2.into_inner(), 3, "waited 3 s, then ran no pass of its own");
    assert!(!f.called("herdr"), "never two passes");
    assert_eq!(
        herdsman::drive::summary("sendmeter", &out),
        "sendmeter: started #12 · 1 skipped (CAP)"
    );
    assert_eq!(passes(&c).len(), 1);

    // Bounded: a pass still running after 120 s leaves the press with a pointer to the line.
    std::fs::write(&lock, "").unwrap();
    let mut out = vec![];
    herdsman::drive::drive(&c, &cfg, &f, &mut out, Some("sendmeter"), true).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "drive: still driving · result will appear here\n"
    );
    assert_eq!(f.count("sleep 1"), 120);
}

#[test]
fn the_result_line_is_red_on_an_error_green_on_a_start_or_merge_amber_when_all_held_back() {
    let line = |acts: serde_json::Value| {
        herdsman::drive::result(&Event::new(
            1,
            "f",
            None,
            "drive-pass",
            json!({"at": "07:41", "actions": acts}),
        ))
    };
    let a = |o: &str, w: &str| json!({"issue": 1, "step": "start", "outcome": o, "words": w});
    let busy = a("waiting", "Mac busy (load/cpu 4.60 > 2.5)");
    assert_eq!(
        line(json!([a("started", "started"), a("merged", "merged"), busy])),
        (
            "g",
            "07:41 · started 1 · merged 1 · 1 waiting: Mac busy (load/cpu 4.60 > 2.5)".into()
        )
    );
    let all = json!([busy, a("waiting", "waiting for CI"), busy, busy]);
    assert_eq!(
        line(all),
        (
            "a",
            "07:41 · started 0 · merged 0 · 4 waiting: Mac busy (load/cpu 4.60 > 2.5)".into()
        )
    );
    assert_eq!(
        line(json!([a("started", "started"), a("error", "failed: exit 1")])),
        ("r", "07:41 · started 1 · merged 0 · 1 failed".into())
    );
    assert_eq!(line(json!([])), ("m", "07:41 · started 0 · merged 0".into()));
}
