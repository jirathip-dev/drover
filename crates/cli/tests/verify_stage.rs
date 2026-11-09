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
        .reply("gh api repos/jirathip-dev/sendmeter/pulls/100/files", files)
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
    // #731: the browser evidence helper sits beside it, the repo's own script.
    let vb = std::fs::read_to_string(c.worktrees.join("sendmeter/verify-12-r1/.verify-browser.mjs")).unwrap();
    let ours = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/verify-browser.mjs");
    assert_eq!(vb, std::fs::read_to_string(ours).unwrap());
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
    let w = waiting_on(
        &herdsman::waiting::Log::new(events),
        events,
        s,
        Some((head, 1)),
        &pr,
        f,
        None,
    );
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
        let w = d.to_string_lossy();
        let d = json!({"role": role, "stage": role, "check": role != "impl", "agent": format!("{role}-{ts}"), "worktree": w});
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

/// #547: only a verify verdict lists screenshots, and only real files inside its lane folder; #499's CI FAIL body
/// (path strings in a test list) lists none, and the board neither shows nor serves a stored path outside the lanes.
#[test]
fn shots_are_only_a_verify_verdicts_files_inside_its_lane_folder() {
    let d = tmpdir("verify-shots");
    let c = ctx(&d, &config());
    let lane = d.join("lane");
    std::fs::create_dir_all(lane.join("shots")).unwrap();
    for (p, b) in [
        ("lane/shots/home-390.png", "narrow"),
        ("lane/a.png", "a"),
        ("x.png", "outside"),
    ] {
        std::fs::write(d.join(p), [b"\x89PNG ".as_slice(), b.as_bytes()].concat()).unwrap();
    }
    std::os::unix::fs::symlink(d.join("x.png"), lane.join("link.png")).unwrap();
    let file = lane.join("verdict.md").to_string_lossy().to_string();
    let spawn = |ts, role: &str| {
        let w = lane.to_string_lossy();
        let d = json!({"role": role, "stage": role, "check": true, "agent": format!("{role}-{ts}"), "worktree": w});
        ev(&c, ts, "spawn", d);
    };
    let verdict = |body: &str| {
        std::fs::write(&file, body).unwrap();
        let s = sha("a");
        let args = ["verdict", "sendmeter", "12", "FAIL", "--sha", &s, "--file", &file];
        run(&c, &Fake::default().reply("gh api", &s), &args);
        kinds(&c, "verdict").pop().unwrap().data["files"].clone()
    };
    // #499's FAIL body, recorded by rev: `a.png` even exists in the lane, but rev's verdict has no screenshots.
    let ci = "FAIL\n\n- non-blocking: evidence: cases `/a.png`, `a.png`, `/tmp/a.png`, `./a.png`, `/../t5/a.png`, \
              `../t5/a.png`, `/link/a.png`\n";
    spawn(1, "rev");
    assert_eq!(verdict(ci), json!([]));
    spawn(2, "verify");
    let x = d.join("x.png").to_string_lossy().into_owned();
    let body = format!(
        "FAIL\n\n- non-blocking: see shots/home-390.png, {x}, ../x.png, shots/../../x.png, link.png, missing.png\n"
    );
    let files = verdict(&body);
    assert_eq!(files.as_array().unwrap().len(), 1, "{files}");
    assert_eq!(files[0]["name"], "shots/home-390.png");
    let none = Fake::default();
    let s = Server::new(&c, &none, Some(ADDR.into()));
    let get = |p: &str| {
        String::from_utf8_lossy(&s.respond(&format!("GET {p} HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), c.now)).into_owned()
    };
    assert!(get("/shot/sendmeter/12/0").ends_with("\u{fffd}PNG narrow"));
    // An older event that stored a path outside the lane folders: no card shot, and the route answers 404.
    let old = json!({"verdict": "FAIL", "stage": "verify", "files": [{"name": "x.png", "path": x}]});
    ev(&c, 3, "verdict", old);
    assert!(herdsman::verify::served(&read(&c.state).unwrap(), ("sendmeter", 12)).is_empty());
    assert!(get("/shot/sendmeter/12/0").starts_with("HTTP/1.1 404"));
}

/// #599: #12's impl lane and a gone rev lane whose verdict.md says PASS at `head`, the PR (CI green) changing
/// `files`; `recipe`: the head has `.herdsman/verify.md`.
fn rev_wrote_pass(name: &str, files: &str, recipe: bool) -> (herdsman::Ctx, Fake, String) {
    let c = ctx(&tmpdir(name), &config());
    let head = sha("abc");
    let rev = c.worktrees.join("sendmeter/rev-12-r1");
    std::fs::create_dir_all(&rev).unwrap();
    std::fs::write(rev.join("verdict.md"), "PASS\nall good\n").unwrap();
    let d = |role: &str, agent: &str| json!({"role": role, "agent": agent, "pane": "w9:p1", "workspace": "w9", "worktree": rev, "branch": "issue-12"});
    ev(&c, 10, "spawn", d("impl", "sendmeter-impl-12"));
    let mut r = d("rev", "sendmeter-rev-12-r1");
    r["stage"] = json!("rev");
    r["check"] = json!(true);
    ev(&c, 20, "spawn", r);
    let mut r = Fake::default()
        .reply(&format!("git -C {} log", rev.display()), &format!("{head} 20"))
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        );
    if recipe {
        r = r.reply(RECIPE, "");
    }
    r.replies.extend(fake(&head, files).replies);
    (c, r, head)
}

/// #599: drive's passes after it records the PASS: no rev lane is ever spawned again at the PASSed head.
fn after_pass(c: &herdsman::Ctx, r: &Fake, passes: usize) {
    for _ in 0..passes {
        drive(c, r);
    }
    let revs = kinds(c, "spawn").into_iter().filter(|e| e.str("role") == Some("rev"));
    assert_eq!(revs.count(), 1, "rev respawned at the PASSed head");
    assert!(!r.called("herdr agent start sendmeter-rev"));
}

#[test]
fn a_pass_drive_records_on_a_ui_pr_without_a_recipe_logs_verify_no_recipe_and_merges() {
    let (c, r, head) = rev_wrote_pass("verify-599-no-recipe", "web/index.html\n", false);
    after_pass(&c, &r, 3);
    let skip = kinds(&c, "verify-no-recipe");
    assert_eq!(skip.len(), 1, "{:?}", r.calls.borrow());
    assert_eq!(skip[0].str("sha"), Some(head.as_str()));
    assert!(kinds(&c, "verify-skipped").is_empty());
    assert_eq!(state(&c), "merged");
}

#[test]
fn a_pass_drive_records_on_a_ui_pr_with_a_recipe_spawns_verify_not_rev() {
    let (c, r, _) = rev_wrote_pass("verify-599-recipe", "src/a.rs\nweb/src/queue.ts\n", true);
    after_pass(&c, &r, 3);
    assert!(
        r.called("herdr agent start sendmeter-verify-12-r1"),
        "{:?}",
        r.calls.borrow()
    );
    assert!(!r.called("gh pr merge"));
    assert_eq!(state(&c), "verify(1)");
}

#[test]
fn a_pass_drive_records_outside_ui_paths_logs_verify_skipped_and_merges() {
    let (c, r, head) = rev_wrote_pass("verify-599-skip", "src/a.rs\n", true);
    after_pass(&c, &r, 3);
    let skip = kinds(&c, "verify-skipped");
    assert_eq!(skip.len(), 1, "{:?}", r.calls.borrow());
    assert_eq!(skip[0].str("sha"), Some(head.as_str()));
    assert_eq!(state(&c), "merged");
}

/// #599: an issue a merge already refused for want of a verify PASS (logs from before the fix) waits on verify at
/// its PASSed head, never "review again".
#[test]
fn a_rev_pass_refused_for_want_of_verify_waits_on_verify_not_review_again() {
    let cfg = Config::parse(&config()).unwrap();
    let a = sha("a");
    let e = |ts, kind, d| Event::new(ts, "sendmeter", Some(12), kind, d);
    let evs = vec![
        e(1, "spawn", json!({"role": "impl", "agent": "sendmeter-impl-12"})),
        e(
            2,
            "spawn",
            json!({"role": "rev", "stage": "rev", "check": true, "agent": "r1"}),
        ),
        e(
            3,
            "verdict",
            json!({"verdict": "PASS", "sha": a, "stage": "rev", "loop": "rev"}),
        ),
        e(
            4,
            "blocked",
            json!({"code": "STALE_VERDICT", "reason": format!("no verify PASS for head {a}")}),
        ),
    ];
    assert_eq!(
        next(&cfg, &evs, &a),
        (Some("herdsman spawn sendmeter 12 verify".into()), Err("STALE_VERDICT"))
    );
}

/// #650: a rev PASS at the head that a merge refused for want of verify, then `kind` (verify-skipped or
/// verify-no-recipe) at that head and green CI: `waiting` says merge, never "review again", and one drive pass merges.
fn skipped_after_a_refusal_merges_in_one_pass(name: &str, kind: &str, word: &str) {
    let (c, head) = passed(name, &config(), false);
    let why = format!("no verify PASS for head {head}");
    ev(&c, 40, "blocked", json!({"code": "STALE_VERDICT", "reason": why}));
    ev(&c, 50, kind, json!({"sha": head, "stage": "verify", "reason": "r"}));
    let cfg = Config::parse(&config()).unwrap();
    let evs = read(&c.state).unwrap();
    let s = &fold(&evs)[&("sendmeter".to_string(), 12)];
    let pr: serde_json::Value = serde_json::from_str(&pr(&head)).unwrap();
    let f = cfg.fleet("sendmeter").unwrap();
    let w = waiting_on(
        &herdsman::waiting::Log::new(&evs),
        &evs,
        s,
        Some((&head, 1)),
        &pr,
        f,
        None,
    );
    assert_eq!(w.what, format!("PASS · verify {word} · merging next pass"));
    assert_eq!(w.next_cmd.as_deref(), Some("herdsman merge sendmeter 12"));
    let r = fake(&head, "src/a.rs\n");
    drive(&c, &r);
    assert!(r.called(&format!(
        "gh pr merge 100 -R jirathip-dev/sendmeter --merge --match-head-commit {head}"
    )));
    assert_eq!(state(&c), "merged");
    assert!(kinds(&c, "spawn")
        .iter()
        .all(|e| e.str("role") != Some("rev") || e.ts == 20));
}

#[test]
fn a_rev_pass_and_verify_skipped_at_the_head_merge_in_one_drive_pass() {
    skipped_after_a_refusal_merges_in_one_pass("verify-650-skip", "verify-skipped", "skipped");
}

#[test]
fn a_rev_pass_and_verify_no_recipe_at_the_head_merge_in_one_drive_pass() {
    skipped_after_a_refusal_merges_in_one_pass("verify-650-no-recipe", "verify-no-recipe", "no recipe");
}

/// #650: a verified head (rev PASS, verify skipped) that a merge refused for another gate names that gate, never
/// "review again".
#[test]
fn a_verified_head_a_gate_refused_names_the_gate_not_review_again() {
    let cfg = Config::parse(&config()).unwrap();
    let a = sha("a");
    let e = |ts, kind, d| Event::new(ts, "sendmeter", Some(12), kind, d);
    let evs = vec![
        e(1, "spawn", json!({"role": "impl", "agent": "sendmeter-impl-12"})),
        e(
            2,
            "spawn",
            json!({"role": "rev", "stage": "rev", "check": true, "agent": "r1"}),
        ),
        e(
            3,
            "verdict",
            json!({"verdict": "PASS", "sha": a, "stage": "rev", "loop": "rev"}),
        ),
        e(4, "verify-skipped", json!({"sha": a, "stage": "verify", "reason": "r"})),
        e(
            5,
            "blocked",
            json!({"code": "SERIAL_UNMARKED", "reason": "touches serial paths"}),
        ),
    ];
    let s = &fold(&evs)[&("sendmeter".to_string(), 12)];
    let pr: serde_json::Value = serde_json::from_str(&pr(&a)).unwrap();
    let f = cfg.fleet("sendmeter").unwrap();
    let w = waiting_on(&herdsman::waiting::Log::new(&evs), &evs, s, Some((&a, 1)), &pr, f, None);
    assert_eq!(w.what, "PASS · merge refused SERIAL_UNMARKED: touches serial paths");
    assert_eq!(w.next_cmd, None);
}
