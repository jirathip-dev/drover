mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::json;

const H1: &str = "1111111111111111111111111111111111111111";
const H2: &str = "2222222222222222222222222222222222222222";
const POST: &str = "gh api -X POST repos/jirathip-dev/sendmeter/statuses/";
const VERIFY: &str = "\n[fleet.sendmeter.role.verify]\nharness = \"claude\"\nprofile = \"fleet-verify\"\n";

fn setup(name: &str, cfg: &str) -> (std::path::PathBuf, drover::Ctx) {
    let d = tmpdir(name);
    let c = ctx(&d, cfg);
    let spawn = json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"});
    append(&c.state, &Event::new(10, "sendmeter", Some(12), "spawn", spawn)).unwrap();
    (d, c)
}

fn spawn(c: &drover::Ctx, ts: u64, role: &str) {
    let d = json!({"role": role, "stage": role, "check": true, "agent": format!("sendmeter-{role}-12")});
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), "spawn", d)).unwrap();
}

fn verdict(c: &drover::Ctx, d: &std::path::Path, r: &Fake, (v, sha, body): (&str, &str, &str)) -> String {
    let f = d.join("verdict.md");
    std::fs::write(&f, body).unwrap();
    let fp = f.to_str().unwrap();
    let (exit, out) = run(c, r, &["verdict", "sendmeter", "12", v, "--sha", sha, "--file", fp]);
    assert_eq!(exit, 0, "{out}");
    out
}

fn at(head: &str) -> Fake {
    Fake::default()
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply(POST, "{}")
}

fn status(sha: &str, state: &str, context: &str, desc: &str) -> String {
    format!("{POST}{sha} -f state={state} -f context={context} -f description={desc}")
}

fn kinds(c: &drover::Ctx, k: &str) -> usize {
    read(&c.state).unwrap().iter().filter(|e| e.kind == k).count()
}

const FAIL: &str = "FAIL\n\n- blocking [correctness] src/x.rs:3: wrong\n  Proof: spec\n  > AC1: it works\n  src/x.rs:3 breaks it\n- non-blocking: a nit\n";

fn drive_fake(head: &str) -> Fake {
    let pr = json!({"number": 34, "state": "OPEN", "baseRefName": "staging", "headRefOid": head, "mergeable": "MERGEABLE",
        "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}]});
    Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("gh pr view issue-12", &pr.to_string())
        .reply("gh api repos/jirathip-dev/sendmeter/pulls/34/files", "src/a.rs\n")
        .reply("git -C", "")
        .reply("which shed", "/opt/shed/bin/shed\n")
}

fn drive(c: &drover::Ctx, r: &Fake) {
    let sh = FakeShed::new();
    sh.attach(c);
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn a_rev_verdict_posts_drover_review_at_its_head_and_a_new_push_is_pending_until_its_own() {
    let (d, c) = setup("status-review", CONFIG);
    let r = at(H1);
    verdict(&c, &d, &r, ("PASS", H1, "PASS\nlooks right\n"));
    assert!(
        r.called(&status(H1, "success", "drover/review", "PASS · round 1 · 1111111")),
        "{:?}",
        r.calls.borrow()
    );

    let r = drive_fake(H2).once(POST, 0, "{}");
    drive(&c, &r);
    let pending = status(H2, "pending", "drover/review", "awaiting review of 2222222");
    assert!(r.called(&pending), "{:?}", r.calls.borrow());
    assert_eq!(r.count(&format!("{POST}{H1}")), 0, "the old head keeps its status");
    let r = drive_fake(H2);
    drive(&c, &r);
    assert!(!r.called(POST), "posted once: {:?}", r.calls.borrow());

    let r = at(H2);
    let out = verdict(&c, &d, &r, ("FAIL", H2, FAIL));
    assert!(
        r.called(&status(
            H2,
            "failure",
            "drover/review",
            "FAIL · round 2 · 2222222 · 1 blocking"
        )),
        "{out} {:?}",
        r.calls.borrow()
    );
}

#[test]
fn a_verify_verdict_posts_drover_verify() {
    let (d, c) = setup("status-verify", &(CONFIG.to_string() + VERIFY));
    spawn(&c, 20, "rev");
    verdict(&c, &d, &at(H1), ("PASS", H1, "PASS\n"));
    spawn(&c, 30, "verify");
    let r = at(H1);
    verdict(&c, &d, &r, ("PASS", H1, "PASS\n"));
    assert!(r.called(&format!(
        "{POST}{H1} -f state=success -f context=drover/verify -f description=PASS · round "
    )));
    assert!(!r.called(&format!("{POST}{H1} -f state=success -f context=drover/review")));
    spawn(&c, 40, "verify");
    let r = at(H1);
    verdict(&c, &d, &r, ("FAIL", H1, FAIL));
    assert!(
        r.called(&format!(
            "{POST}{H1} -f state=failure -f context=drover/verify -f description=FAIL · round "
        )),
        "{:?}",
        r.calls.borrow()
    );
    assert!(r
        .calls
        .borrow()
        .iter()
        .any(|l| l.contains("context=drover/verify") && l.ends_with("· 1 blocking")));
}

#[test]
fn a_failed_post_keeps_the_verdict_logs_once_is_retried_and_doctor_names_it() {
    let (d, c) = setup("status-403", CONFIG);
    let r = at(H1).once_err(POST, 1, "gh: Resource not accessible by integration (HTTP 403)");
    let out = verdict(&c, &d, &r, ("PASS", H1, "PASS\n"));
    assert!(out.contains("recorded PASS"), "{out}");
    assert_eq!((kinds(&c, "verdict"), kinds(&c, "status-failed")), (1, 1));
    let (_, doc) = run(&c, &Fake::default(), &["doctor"]);
    assert!(
        doc.contains("✗ commit status drover/review at 1111111 (#12) not posted, retried each pass")
            && doc.contains("HTTP 403")
            && doc.contains("Commit statuses: write"),
        "{doc}"
    );

    let r = drive_fake(H1).once_err(POST, 1, "gh: Resource not accessible by integration (HTTP 403)");
    drive(&c, &r);
    assert!(r.called(&status(H1, "success", "drover/review", "PASS · round 1 · 1111111")));
    assert_eq!(kinds(&c, "status-failed"), 1, "a failing retry logs no second event");

    let r = drive_fake(H1).once(POST, 0, "{}");
    drive(&c, &r);
    assert!(r.called(&status(H1, "success", "drover/review", "PASS · round 1 · 1111111")));
    let (_, doc) = run(&c, &Fake::default(), &["doctor"]);
    assert!(!doc.contains("commit status"), "{doc}");
    let r = drive_fake(H1);
    drive(&c, &r);
    assert!(!r.called(POST), "{:?}", r.calls.borrow());
}

#[test]
fn drovers_own_statuses_leave_ci_and_the_check_count_alone() {
    let ci = json!({"__typename": "CheckRun", "name": "build", "status": "COMPLETED", "conclusion": "SUCCESS"});
    let ours = [
        json!({"__typename": "StatusContext", "context": "drover/review", "state": "FAILURE"}),
        json!({"__typename": "StatusContext", "context": "drover/verify", "state": "PENDING"}),
    ];
    let mut lines = vec![];
    for (i, rollup) in [json!([ci]), json!([ci, ours[0], ours[1]])].iter().enumerate() {
        let (_d, c) = setup(&format!("status-ci-{i}"), CONFIG);
        let pr = json!({"number": 34, "state": "OPEN", "baseRefName": "staging", "headRefOid": H1,
            "mergeable": "MERGEABLE", "statusCheckRollup": rollup});
        let r = Fake::default().reply("gh pr view issue-12", &pr.to_string());
        let (code, out) = run(&c, &r, &["pr", "sendmeter", "12"]);
        assert_eq!(code, 0, "{out}");
        assert!(out.contains("ci green (1 checks)"), "{out}");
        lines.push(out);
    }
    assert_eq!(lines[0], lines[1]);
}

fn hook(cmd: &str) -> String {
    use std::io::Write;
    let d = tmpdir("status-hook");
    std::fs::write(d.join("fleet.toml"), CONFIG).unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_drover"))
        .args(["hook", "--role", "impl"])
        .env_remove("FLEET_LANE")
        .env_remove("DROVER_SCRATCH_TARGET")
        .env("DROVER_LANE", "sendmeter:12:impl")
        .env("FLEET_CONFIG", d.join("fleet.toml"))
        .env("FLEET_STATE", d.join("state/events.jsonl"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let input = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "cwd": d, "tool_input": {"command": cmd}});
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
}

#[test]
fn a_lane_posting_a_commit_status_is_refused() {
    for cmd in [
        "gh api -X POST repos/o/r/statuses/abc -f state=success -f context=drover/review",
        "gh api repos/o/r/statuses/abc -f state=success",
        "gh api repos/o/r/statuses/abc",
        "cd /x && gh api --method POST repos/o/r/commits/abc/statuses -f state=success",
    ] {
        let out = hook(cmd);
        assert!(
            out.contains("\"deny\"") && out.contains("only drover posts commit statuses"),
            "{cmd}: {out}"
        );
    }
    assert_eq!(
        hook("gh api repos/o/r/commits/abc/statuses"),
        "",
        "reading statuses is allowed"
    );
}
