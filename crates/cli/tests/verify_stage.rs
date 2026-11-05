//! #417: the optional verify stage: rev PASS → verify (by `ui_paths`, a design stage or `--verify`), skipped or
//! passed on without a recipe, its FAILs in rev's rounds, and its screenshots on the board.
mod common;
use common::*;
use herdsman::config::Config;
use herdsman::events::{append, fold, read, Event};
use herdsman::gate::gate;
use herdsman::serve::Server;
use herdsman::waiting::waiting_on;
use serde_json::json;

const VERIFY: &str = "\n[fleet.sendmeter.role.verify]\nharness = \"hermes\"\nprofile = \"fleet-verify\"\n";
const RECIPE: &str = "gh api repos/jirathip-dev/sendmeter/contents/.herdsman/verify.md";
const ADDR: &str = "127.0.0.1:7777";

/// The test fleet with a verify role and `ui_paths = ["web/**"]`.
fn config() -> String {
    CONFIG.replace(
        "integration = \"staging\"   # never main",
        "integration = \"staging\"\nui_paths = [\"web/**\"]",
    ) + VERIFY
}

fn sha(p: &str) -> String {
    format!("{p}{}", "0".repeat(40 - p.len()))
}

fn ev(c: &herdsman::Ctx, ts: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
}

fn pr(head: &str) -> String {
    json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": head, "mergeable": "MERGEABLE",
        "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}]})
    .to_string()
}

/// Spawns succeed, the host is calm; the PR at `head` changes `files`.
fn fake(head: &str, files: &str) -> Fake {
    Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("gh pr view issue-12", &pr(head))
        .reply("gh pr diff 100", files)
        .reply("gh pr merge 100", "")
        .reply("gh pr view 100", r#"{"mergeCommit":{"oid":"m3rg3"}}"#)
        .reply("gh issue", "")
        .reply("git -C", "")
        .reply("herdr worktree", r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#)
        .reply(
            "herdr pane list",
            r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#,
        )
        .reply("herdr pane", "{}")
        .reply("herdr agent", "{}")
}

/// #12 built (its lane gone; `design`: after its design was approved) and rev PASSed at `head`.
fn passed(name: &str, cfg: &str, design: bool) -> (herdsman::Ctx, String) {
    let c = ctx(&tmpdir(name), cfg);
    let head = sha("abc");
    if design {
        ev(&c, 5, "design-approved", json!({"id": "q1", "by": "owner"}));
    }
    ev(
        &c,
        10,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"}),
    );
    ev(
        &c,
        20,
        "spawn",
        json!({"role": "rev", "stage": "rev", "check": true, "agent": "sendmeter-rev-12-r1"}),
    );
    ev(
        &c,
        30,
        "verdict",
        json!({"verdict": "PASS", "sha": head, "stage": "rev", "loop": "rev", "body": "PASS"}),
    );
    (c, head)
}

fn drive(c: &herdsman::Ctx, r: &Fake) -> String {
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    out
}

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

fn state(c: &herdsman::Ctx) -> String {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)].shown()
}

#[test]
fn without_a_verify_role_the_builtin_pipeline_is_unchanged() {
    let names = |t: &str| {
        let cfg = Config::parse(t).unwrap();
        let p = cfg.fleet("sendmeter").unwrap().flow();
        p.stage.iter().map(|s| s.name.clone()).collect::<Vec<_>>()
    };
    assert_eq!(names(CONFIG), ["impl", "rev", "gates"]);
    assert_eq!(names(&config()), ["impl", "rev", "verify", "gates"]);
    let cfg = Config::parse(CONFIG).unwrap();
    assert_eq!(
        cfg.fleet("sendmeter").unwrap().flow().next("rev", "PASS"),
        Some(("gates", false))
    );
}

#[test]
fn a_pr_touching_ui_paths_goes_from_rev_pass_to_a_verify_lane() {
    let (c, head) = passed("verify-ui", &config(), false);
    let r = fake(&head, "src/a.rs\nweb/src/queue.ts\n").reply(RECIPE, "");
    drive(&c, &r);
    assert!(
        r.called("herdr agent start sendmeter-verify-12-r1"),
        "{:?}",
        r.calls.borrow()
    );
    assert!(!r.called("gh pr merge"));
    assert_eq!(state(&c), "verify(1)");
    assert!(kinds(&c, "verify-skipped").is_empty());
    let sp = kinds(&c, "spawn").pop().unwrap();
    assert_eq!(
        (sp.str("stage"), sp.data["check"].as_bool()),
        (Some("verify"), Some(true))
    );
    // Its brief is the verify prompt, as a check.
    let brief = std::fs::read_to_string(c.worktrees.join("sendmeter/verify-12-r1/.brief.md")).unwrap();
    assert!(
        brief.contains("# ROLE: VERIFIER") && brief.contains("verdict.md"),
        "{brief}"
    );
}

#[test]
fn a_pr_outside_ui_paths_skips_verify_and_goes_to_the_gates() {
    let (c, head) = passed("verify-skip", &config(), false);
    let r = fake(&head, "src/a.rs\n").reply(RECIPE, "");
    let out = drive(&c, &r);
    assert!(out.contains("sendmeter #12 verify-skipped at abc0000"), "{out}");
    let skip = &kinds(&c, "verify-skipped")[0];
    assert_eq!(
        (skip.str("sha"), skip.str("stage")),
        (Some(head.as_str()), Some("verify"))
    );
    assert!(!r.called("herdr agent start sendmeter-verify"));
    drive(&c, &r);
    assert!(r.called(&format!(
        "gh pr merge 100 -R jirathip-dev/sendmeter --merge --match-head-commit {head}"
    )));
    assert_eq!(state(&c), "merged");
}

#[test]
fn a_design_stage_issue_always_verifies() {
    let (c, head) = passed("verify-design", &config(), true);
    let r = fake(&head, "src/a.rs\n").reply(RECIPE, "");
    drive(&c, &r);
    assert!(r.called("herdr agent start sendmeter-verify-12-r1"));
    assert!(kinds(&c, "verify-skipped").is_empty());
}

#[test]
fn verify_off_skips_the_stage_and_verify_on_forces_it() {
    let (c, head) = passed("verify-off", &config(), false);
    assert_eq!(
        run(
            &c,
            &Fake::default(),
            &["queue", "sendmeter", "add", "12", "--verify", "off"]
        )
        .0,
        0
    );
    let r = fake(&head, "web/index.html\n").reply(RECIPE, "");
    drive(&c, &r);
    assert_eq!(kinds(&c, "verify-skipped").len(), 1);
    assert!(!r.called("herdr agent start sendmeter-verify"));

    let (c, head) = passed("verify-on", &config(), false);
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "--verify"]).0,
        0
    );
    // `queue set` flips it either way.
    assert_eq!(
        run(
            &c,
            &Fake::default(),
            &["queue", "sendmeter", "set", "12", "verify", "off"]
        )
        .0,
        0
    );
    assert_eq!(
        run(
            &c,
            &Fake::default(),
            &["queue", "sendmeter", "set", "12", "verify", "on"]
        )
        .0,
        0
    );
    let (_, ls) = run(&c, &Fake::default(), &["queue", "sendmeter", "ls"]);
    assert!(ls.contains("verify on"), "{ls}");
    let r = fake(&head, "src/a.rs\n").reply(RECIPE, "");
    drive(&c, &r);
    assert!(r.called("herdr agent start sendmeter-verify-12-r1"));
    assert!(kinds(&c, "verify-skipped").is_empty());
}

#[test]
fn no_recipe_logs_verify_no_recipe_shows_it_and_goes_on_to_the_gates() {
    let (c, head) = passed("verify-no-recipe", &config(), false);
    // No `.herdsman/verify.md` at the head: the contents read fails.
    let r = fake(&head, "web/index.html\n");
    let out = drive(&c, &r);
    assert!(r.called(&format!("{RECIPE}?ref={head}")), "{:?}", r.calls.borrow());
    assert!(out.contains("verify-no-recipe"), "{out}");
    assert_eq!(kinds(&c, "verify-no-recipe").len(), 1);
    assert!(!r.called("herdr agent start sendmeter-verify"));
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let res =
        String::from_utf8_lossy(&s.respond(&format!("GET /api/v1/queue HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), c.now))
            .into_owned();
    assert!(res.contains("verify skipped: no .herdsman/verify.md"), "{res}");
    drive(&c, &r);
    assert_eq!(state(&c), "merged");
}

/// `waiting_on`'s next command and the merge gate's answer after `events`, for the PR at `head`.
fn next(cfg: &Config, events: &[Event], head: &str) -> (Option<String>, Result<(), &'static str>) {
    let f = cfg.fleet("sendmeter").unwrap();
    let s = &fold(events)[&("sendmeter".to_string(), 12)];
    let pr: serde_json::Value = serde_json::from_str(&pr(head)).unwrap();
    let w = waiting_on(events, s, Some((head, 1)), &pr, f, None);
    (w.next_cmd, gate(f, &pr, Some(s), false).map(drop).map_err(|r| r.code))
}

#[test]
fn a_verify_pass_at_an_older_sha_does_not_let_the_new_head_merge() {
    let cfg = Config::parse(&config()).unwrap();
    let (a, b) = (sha("a"), sha("b"));
    let e = |ts, kind, d| Event::new(ts, "sendmeter", Some(12), kind, d);
    let v = |ts, stage, v, s: &str| {
        e(
            ts,
            "verdict",
            json!({"verdict": v, "sha": s, "stage": stage, "loop": stage}),
        )
    };
    let mut evs = vec![
        e(1, "spawn", json!({"role": "impl", "agent": "sendmeter-impl-12"})),
        e(
            2,
            "spawn",
            json!({"role": "rev", "stage": "rev", "check": true, "agent": "r1"}),
        ),
        v(3, "rev", "PASS", &a),
    ];
    assert_eq!(
        next(&cfg, &evs, &a),
        (Some("herdsman spawn sendmeter 12 verify".into()), Err("STALE_VERDICT"))
    );
    evs.extend([
        e(
            4,
            "spawn",
            json!({"role": "verify", "stage": "verify", "check": true, "agent": "v1"}),
        ),
        v(5, "verify", "PASS", &a),
    ]);
    assert_eq!(
        next(&cfg, &evs, &a),
        (Some("herdsman merge sendmeter 12".into()), Ok(()))
    );
    evs.extend([
        e(6, "spawn", json!({"role": "impl", "agent": "sendmeter-impl-12"})),
        e(
            7,
            "spawn",
            json!({"role": "rev", "stage": "rev", "check": true, "agent": "r2"}),
        ),
        v(8, "rev", "PASS", &b),
    ]);
    assert_eq!(
        next(&cfg, &evs, &b),
        (Some("herdsman spawn sendmeter 12 verify".into()), Err("STALE_VERDICT"))
    );
    // A FAIL goes to a fix round in impl.
    evs.extend([
        e(
            9,
            "spawn",
            json!({"role": "verify", "stage": "verify", "check": true, "agent": "v2"}),
        ),
        v(10, "verify", "FAIL", &b),
    ]);
    assert_eq!(
        next(&cfg, &evs, &b),
        (
            Some("herdsman spawn sendmeter 12 impl --supersede".into()),
            Err("VERDICT_FAIL")
        )
    );
}

/// rev FAIL, then verify FAIL twice: verify's FAILs are in rev's rounds, and the third FAIL parks the issue. The
/// needs-you card shows the verdict's screenshots inline.
#[test]
fn verify_fails_count_in_revs_rounds_and_the_third_fail_parks() {
    let d = tmpdir("verify-rounds");
    let c = ctx(&d, &config());
    let (a, b, h) = (sha("a"), sha("b"), sha("c"));
    let file = d.join("verdict.md");
    std::fs::create_dir_all(d.join("shots")).unwrap();
    std::fs::write(d.join("shots/home-390.png"), b"\x89PNG narrow").unwrap();
    let file = file.to_string_lossy().to_string();
    let verdict = |v: &str, s: &str, body: &str| {
        std::fs::write(&file, body).unwrap();
        run(
            &c,
            &Fake::default().reply("gh api", s),
            &["verdict", "sendmeter", "12", v, "--sha", s, "--file", &file],
        )
        .1
    };
    let spawn = |ts, role: &str| {
        let d = json!({"role": role, "stage": role, "check": role != "impl", "agent": format!("{role}-{ts}")});
        ev(&c, ts, "spawn", d);
    };
    let fail = "FAIL\n\n- non-blocking: off\n";
    let shot = "FAIL\n\n- non-blocking: the badge is gone, see shots/home-390.png\n";
    spawn(1, "impl");
    spawn(2, "rev");
    assert!(verdict("FAIL", &a, fail).contains("(round 1/3)"));
    spawn(3, "impl");
    spawn(4, "rev");
    assert!(verdict("PASS", &b, "PASS\n").contains("(round 2/3)"));
    spawn(5, "verify");
    let out = verdict("FAIL", &b, shot);
    assert!(out.contains("(round 2/3)") && !out.contains("needs-human"), "{out}");
    spawn(6, "impl");
    spawn(7, "rev");
    assert!(verdict("PASS", &h, "PASS\n").contains("(round 3/3)"));
    spawn(8, "verify");
    let out = verdict("FAIL", &h, shot);
    assert!(
        out.contains("(round 3/3)") && out.contains("round limit reached → needs-human"),
        "{out}"
    );
    let s = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
    assert_eq!((s.state.as_str(), s.stage_rounds["rev"]), ("needs-human", 3));
    let v = kinds(&c, "verdict").pop().unwrap();
    assert_eq!(v.str("stage"), Some("verify"));
    assert_eq!(v.data["files"][0]["name"], "shots/home-390.png");
    // The board's needs-you card shows the screenshot inline, and serves it.
    let none = Fake::default();
    let s = Server::new(&c, &none, Some(ADDR.into()));
    let get = |p: &str| {
        String::from_utf8_lossy(&s.respond(&format!("GET {p} HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), c.now)).into_owned()
    };
    let [needs, ..] = parts(&s, ADDR, "", c.now);
    assert!(needs.contains("<img src=\"shot/sendmeter/12/0\""), "{needs}");
    let png = get("/shot/sendmeter/12/0");
    assert!(
        png.starts_with("HTTP/1.1 200") && png.ends_with("\u{fffd}PNG narrow"),
        "{png}"
    );
}
