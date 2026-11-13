//! #294: named serial groups: both config forms, joining by mark or by the pushed diff, a full group holds the lane
//! until the holder merges (then an update round), fixed-order slots over two groups, grandfathering and the wording.
mod common;
use common::*;
use drover::config::Config;
use drover::events::{append, fold, read, Event};
use serde_json::json;

const DB_CI: &str = r#"[[fleet.sendmeter.serial]]
name = "db"
paths = ["supabase/migrations/**"]
max = 10

[[fleet.sendmeter.serial]]
name = "ci"
paths = [".github/workflows/ci.yml", "scripts/lib/ci*", "scripts/check-*"]
"#;

/// CONFIG with `extra` in `[fleet.sendmeter]` (keys) or after it (tables).
fn config(keys: &str, tables: &str) -> String {
    CONFIG.replace(
        "integration = \"staging\"   # never main",
        &format!("integration = \"staging\"\ndriver = \"drover\"\n{keys}\n{tables}"),
    )
}

fn sctx(name: &str, cfg: &str) -> (drover::Ctx, FakeShed) {
    let c = ctx(&tmpdir(name), cfg);
    let sh = FakeShed::new();
    sh.attach(&c);
    (c, sh)
}

fn sha(p: &str) -> String {
    format!("{p}{}", "0".repeat(40 - p.len()))
}

fn ev(c: &drover::Ctx, n: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(10, "sendmeter", Some(n), kind, d)).unwrap();
}

/// An impl lane of `n` logged as spawned, in a real worktree dir.
fn lane(c: &drover::Ctx, n: u64) {
    let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
    std::fs::create_dir_all(&wt).unwrap();
    let agent = format!("sendmeter-impl-{n}");
    let d = json!({"role": "impl", "agent": agent, "pane": agent, "workspace": "", "runtime": "shed",
        "worktree": wt, "branch": format!("issue-{n}")});
    ev(c, n, "spawn", d);
}

fn pr(n: u64) -> String {
    let check = json!({"__typename": "CheckRun", "name": "ci", "status": "IN_PROGRESS"});
    json!({"number": 100 + n, "state": "OPEN", "baseRefName": "staging", "headRefOid": sha(&format!("{n}a")),
        "statusCheckRollup": [check]})
    .to_string()
}

/// Spawns succeed, the host is calm, and issue `n`'s open PR changes `files`.
fn fake(prs: &[(u64, &str)]) -> Fake {
    let mut f = Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("git -C", "");
    for (n, files) in prs {
        f = f.reply(&format!("gh pr view issue-{n}"), &pr(*n)).reply(
            &format!("gh api repos/jirathip-dev/sendmeter/pulls/{}/files", 100 + n),
            files,
        );
    }
    f
}

fn drive(c: &drover::Ctx, r: &Fake) -> (String, Vec<String>) {
    let before = read(&c.state).unwrap().len();
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    let new = read(&c.state).unwrap().to_vec().split_off(before);
    let steps = new
        .iter()
        .filter(|e| e.kind == "driven")
        .map(|e| e.str("step").unwrap().to_string());
    (out, steps.collect())
}

/// `kind` events of `n`, as `group` (`holder`).
fn logged(c: &drover::Ctx, n: u64, kind: &str) -> Vec<String> {
    let all = read(&c.state).unwrap();
    let mine = all.iter().filter(|e| e.kind == kind && e.issue == Some(n));
    mine.map(|e| match e.data.get("holder") {
        Some(h) => format!("{} ({h})", e.str("group").unwrap()),
        None => e.str("group").unwrap().to_string(),
    })
    .collect()
}

fn queue_groups(c: &drover::Ctx, n: u64) -> Vec<String> {
    let cfg = Config::load(&c.config).unwrap();
    let ev = read(&c.state).unwrap();
    let q = drover::queue::replay(&ev, "sendmeter");
    let gs = drover::queue::groups(&q, &ev, &cfg.fleet["sendmeter"], "sendmeter", n);
    gs.iter().map(|g| g.name.clone()).collect()
}

// --- config ---

#[test]
fn both_forms_parse_and_together_are_an_error() {
    let c = Config::parse(&config("", DB_CI)).unwrap();
    let g = &c.fleet["sendmeter"].serial;
    let got: Vec<_> = g.iter().map(|g| (g.name.as_str(), g.paths.len(), g.max)).collect();
    assert_eq!(got, [("db", 1, 10), ("ci", 3, 1)]);
    // The old form is one group named `serial_label`, `db` when unset.
    let old = "serial_paths = [\"supabase/migrations/**\"]\nserial_max = 3";
    let c = Config::parse(&config(old, "")).unwrap();
    let g = &c.fleet["sendmeter"].serial;
    assert_eq!((g.len(), g[0].name.as_str(), g[0].max), (1, "db", 3));
    let c = Config::parse(&config(&format!("{old}\nserial_label = \"schema\""), "")).unwrap();
    assert_eq!(c.fleet["sendmeter"].serial[0].name, "schema");
    let e = format!("{:#}", Config::parse(&config(old, DB_CI)).unwrap_err());
    assert!(
        e.contains("serial_paths and [[fleet.sendmeter.serial]] together"),
        "{e}"
    );
    let long = DB_CI.replace("name = \"ci\"", "name = \"workflows\"");
    let e = format!("{:#}", Config::parse(&config("", &long)).unwrap_err());
    assert!(e.contains("must be 1 to 8 characters"), "{e}");
}

#[test]
fn doctor_suggests_the_new_form() {
    let (c, _sh) = sctx("sg-doctor", &config("serial_paths = [\"db/**\"]", ""));
    let (_, out) = run(&c, &Fake::default(), &["doctor"]);
    assert!(
        out.contains("! serial_paths is the old form; [[fleet.sendmeter.serial]] name=\"db\"…"),
        "{out}"
    );
    let (c, _sh) = sctx("sg-doctor-new", &config("", DB_CI));
    assert!(!run(&c, &Fake::default(), &["doctor"]).1.contains("old form"));
}

// --- joining ---

#[test]
fn a_mark_joins_its_group_plain_serial_the_first() {
    let (c, _sh) = sctx("sg-mark", &config("", DB_CI));
    let ok = |args: &[&str]| assert_eq!(run(&c, &Fake::default(), args).0, 0, "{args:?}");
    ok(&["queue", "sendmeter", "add", "1", "--serial", "ci"]);
    ok(&["queue", "sendmeter", "add", "--serial", "2", "3"]);
    ok(&["queue", "sendmeter", "add", "4"]);
    ok(&["queue", "sendmeter", "set", "4", "serial", "ci,db"]);
    assert_eq!(queue_groups(&c, 1), ["ci"]);
    assert_eq!(
        (queue_groups(&c, 2), queue_groups(&c, 3)),
        (vec!["db".to_string()], vec!["db".to_string()])
    );
    assert_eq!(queue_groups(&c, 4), ["ci", "db"], "name order");
    let (exit, out) = run(
        &c,
        &Fake::default(),
        &["queue", "sendmeter", "add", "5", "--serial", "nope"],
    );
    assert_ne!(exit, 0, "an unknown group: {out}");
    let (_, ls) = run(&c, &Fake::default(), &["queue", "sendmeter", "ls"]);
    assert!(ls.contains("#1     queued  serial"), "{ls}");
}

#[test]
fn a_pushed_diff_joins_a_free_group_and_carries_on() {
    let (c, _sh) = sctx("sg-diff", &config("", DB_CI));
    lane(&c, 12);
    let r = fake(&[(12, "src/a.rs\nscripts/check-x.mjs\n")]);
    let (out, _) = drive(&c, &r);
    assert!(
        out.contains("sendmeter #12 serial-joined ci: scripts/check-x.mjs"),
        "{out}"
    );
    let all = read(&c.state).unwrap();
    let j = all.iter().find(|e| e.kind == "serial-joined").unwrap();
    assert_eq!((j.str("group"), j.str("by")), (Some("ci"), Some("diff")));
    assert!(logged(&c, 12, "serial-held").is_empty(), "a free slot: no hold");
    assert_eq!(queue_groups(&c, 12), ["ci"]);
    let (_, status) = run(&c, &Fake::default(), &["status", "sendmeter"]);
    assert!(status.contains("sendmeter: serial: db 0/10 · ci 1/1\n"), "{status}");
}

// --- holding ---

#[test]
fn a_second_ci_push_holds_until_the_holder_merges_then_updates() {
    let (c, sh) = sctx("sg-hold", &config("", DB_CI));
    assert_eq!(
        run(
            &c,
            &Fake::default(),
            &["queue", "sendmeter", "add", "9", "--serial", "ci"]
        )
        .0,
        0
    );
    lane(&c, 9);
    lane(&c, 12);
    let r = fake(&[(12, "scripts/lib/ci.mjs\n")]);
    sh.lane("sendmeter-impl-9", "working", "/x");
    sh.lane("sendmeter-impl-12", "done", "/x");
    sh.exit("sendmeter-impl-12");
    let stops = || {
        let sent = sh.sent("sendmeter-impl-12");
        sent.into_iter().filter(|t| t.starts_with("stop:")).collect::<Vec<_>>()
    };
    let (out, steps) = drive(&c, &r);
    assert_eq!(logged(&c, 12, "serial-held"), ["ci (9)"], "{out}");
    let stop = "stop: ci is held by #9; you'll get an update round when it merges";
    assert_eq!(stops(), [stop], "{:?}", sh.methods());
    assert!(steps.is_empty(), "no review while held: {steps:?}");
    assert!(!sh.called("run", "sendmeter-rev-12-r1"));
    let (_, w) = run(&c, &r, &["waiting", "sendmeter"]);
    assert!(w.contains("waits: ci held by #9 (1/1)"), "{w}");
    // Still held next pass, and told once.
    let (_, steps) = drive(&c, &r);
    assert!(steps.is_empty(), "{steps:?}");
    assert_eq!(stops().len(), 1);
    // A manual spawn says who holds the slot.
    let (exit, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl", "--supersede"]);
    assert!(
        exit == 3 && out.contains("REFUSED SERIAL_BUSY: ci: #9 holds 1/1; retry after they merge"),
        "{out}"
    );
    // The holder merges: #12 takes the slot and gets its update round.
    ev(&c, 9, "merged", json!({}));
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["conflict"], "{out}");
    assert_eq!(logged(&c, 12, "serial-slot"), ["ci"]);
    assert!(sh.called("run", "sendmeter-impl-12"), "{out}");
    let brief = std::fs::read_to_string(c.worktrees.join("sendmeter/issue-12/.brief.md")).unwrap();
    assert!(brief.contains("Merge `origin/staging` into this branch"), "{brief}");
}

#[test]
fn the_first_held_in_queue_order_takes_a_freed_slot() {
    let (c, _sh) = sctx("sg-order", &config("", DB_CI));
    let ok = |args: &[&str]| assert_eq!(run(&c, &Fake::default(), args).0, 0, "{args:?}");
    ok(&["queue", "sendmeter", "add", "9", "--serial", "ci"]);
    ok(&["queue", "sendmeter", "add", "13", "12"]);
    for n in [9, 12, 13] {
        lane(&c, n);
    }
    let r = fake(&[(12, "scripts/check-a\n"), (13, "scripts/check-b\n")]);
    drive(&c, &r);
    assert_eq!(
        (logged(&c, 12, "serial-held"), logged(&c, 13, "serial-held")),
        (vec!["ci (9)".into()], vec!["ci (9)".into()])
    );
    ev(&c, 9, "merged", json!({}));
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["conflict"], "one slot, one update: {out}");
    assert_eq!(
        (logged(&c, 13, "serial-slot"), logged(&c, 12, "serial-slot")),
        (vec!["ci".into()], vec![])
    );
}

#[test]
fn two_groups_take_slots_in_name_order_so_two_issues_never_deadlock() {
    let groups = "[[fleet.sendmeter.serial]]\nname = \"bb\"\npaths = [\"b/**\"]\n\n\
                  [[fleet.sendmeter.serial]]\nname = \"aa\"\npaths = [\"a/**\"]\n";
    let (c, _sh) = sctx("sg-ab", &config("", groups));
    lane(&c, 12);
    lane(&c, 13);
    // Both touch aa and bb at once (listed b first): #12 takes both, #13 holds on aa and so never takes bb.
    let r = fake(&[(12, "b/x\na/x\n"), (13, "b/y\na/y\n")]);
    drive(&c, &r);
    assert_eq!(logged(&c, 12, "serial-joined"), ["aa", "bb"]);
    assert!(logged(&c, 12, "serial-held").is_empty());
    assert_eq!(logged(&c, 13, "serial-held"), ["aa (12)", "bb (12)"]);
    let (_, status) = run(&c, &Fake::default(), &["status", "sendmeter"]);
    assert!(status.contains("serial: bb 1/1 · aa 1/1"), "{status}");
    // #12 merges: #13 takes aa, then bb, then updates.
    ev(&c, 12, "merged", json!({}));
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["conflict"], "{out}");
    assert_eq!(logged(&c, 13, "serial-slot"), ["aa", "bb"]);
}

#[test]
fn a_held_issue_waits_on_its_second_group_with_its_first_slot_taken() {
    let groups = "[[fleet.sendmeter.serial]]\nname = \"aa\"\npaths = [\"a/**\"]\n\n\
                  [[fleet.sendmeter.serial]]\nname = \"bb\"\npaths = [\"b/**\"]\n";
    let (c, _sh) = sctx("sg-ab2", &config("", groups));
    let ok = |args: &[&str]| assert_eq!(run(&c, &Fake::default(), args).0, 0, "{args:?}");
    ok(&["queue", "sendmeter", "add", "8", "--serial", "aa"]);
    ok(&["queue", "sendmeter", "add", "9", "--serial", "bb"]);
    for n in [8, 9, 12] {
        lane(&c, n);
    }
    let r = fake(&[(12, "a/x\nb/x\n")]);
    drive(&c, &r);
    assert_eq!(logged(&c, 12, "serial-held"), ["aa (8)", "bb (9)"]);
    ev(&c, 8, "merged", json!({}));
    let (_, steps) = drive(&c, &r);
    assert!(steps.is_empty(), "aa taken, bb still held: {steps:?}");
    assert_eq!(logged(&c, 12, "serial-slot"), ["aa"]);
    let (_, w) = run(&c, &r, &["waiting", "sendmeter"]);
    assert!(w.contains("waits: bb held by #9 (1/1)"), "{w}");
    ev(&c, 9, "merged", json!({}));
    let (_, steps) = drive(&c, &r);
    assert_eq!(steps, ["conflict"]);
}

// --- grandfathering and wording ---

#[test]
fn work_over_the_limit_when_a_group_lands_keeps_going_and_new_joins_wait() {
    let (c, _sh) = sctx("sg-grand", &config("", DB_CI));
    // #8 and #9 already touch ci.yml and are in review when the group lands: no join events yet.
    for n in [8, 9] {
        lane(&c, n);
        let d = json!({"role": "rev", "agent": format!("sendmeter-rev-{n}-r1"), "pane": format!("sendmeter-rev-{n}-r1"), "workspace": "", "runtime": "shed"});
        ev(&c, n, "spawn", d);
    }
    let ci = ".github/workflows/ci.yml\n";
    let (out, _) = drive(&c, &fake(&[(8, ci), (9, ci)]));
    for n in [8, 9] {
        assert_eq!(logged(&c, n, "serial-joined"), ["ci"], "{out}");
        assert!(logged(&c, n, "serial-held").is_empty(), "#{n} keeps going: {out}");
    }
    let (_, status) = run(&c, &Fake::default(), &["status", "sendmeter"]);
    assert!(status.contains("serial: db 0/10 · ci 2/1"), "{status}");
    for n in ["8", "9"] {
        let (exit, out) = run(&c, &fake(&[]), &["spawn", "sendmeter", n, "impl", "--supersede"]);
        assert_eq!(exit, 0, "#{n} keeps its slot: {out}");
    }
    lane(&c, 12);
    drive(&c, &fake(&[(12, ".github/workflows/ci.yml\n")]));
    assert_eq!(logged(&c, 12, "serial-held"), ["ci (8)"]);
    assert_eq!(
        run(
            &c,
            &Fake::default(),
            &["queue", "sendmeter", "add", "14", "--serial", "ci"]
        )
        .0,
        0
    );
    let (exit, out) = run(&c, &fake(&[]), &["spawn", "sendmeter", "14", "impl"]);
    assert!(
        exit == 3 && out.contains("REFUSED SERIAL_BUSY: ci: #8 #9 hold 2/1"),
        "{out}"
    );
}

#[test]
fn merge_refuses_a_pr_touching_a_group_it_never_joined() {
    let (c, _sh) = sctx("sg-unmarked", &config("", DB_CI));
    assert_eq!(
        run(
            &c,
            &Fake::default(),
            &["queue", "sendmeter", "add", "12", "--serial", "db"]
        )
        .0,
        0
    );
    lane(&c, 12);
    let head = sha("12a");
    ev(&c, 12, "verdict", json!({"verdict": "PASS", "sha": head}));
    let checks = json!([{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}]);
    let view = json!({"number": 112, "state": "OPEN", "baseRefName": "staging", "headRefOid": head, "statusCheckRollup": checks});
    let r = Fake::default().reply("gh pr view issue-12", &view.to_string()).reply(
        "gh api repos/jirathip-dev/sendmeter/pulls/112/files",
        "supabase/migrations/1.sql\nscripts/check-x.mjs\n",
    );
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(exit, 3, "{out}");
    assert!(
        out.contains("REFUSED SERIAL_UNMARKED: ci: PR touches scripts/check-x.mjs"),
        "{out}"
    );
    assert!(!r.called("gh pr merge"));
    assert_eq!(
        fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)].state,
        "blocked"
    );
}

#[test]
fn crossed_joins_over_two_groups_never_deadlock() {
    let groups = "[[fleet.sendmeter.serial]]\nname = \"aa\"\npaths = [\"a/**\"]\n\n\
                  [[fleet.sendmeter.serial]]\nname = \"bb\"\npaths = [\"b/**\"]\n";
    let (c, _sh) = sctx("sg-cross", &config("", groups));
    let ok = |args: &[&str]| assert_eq!(run(&c, &Fake::default(), args).0, 0, "{args:?}");
    ok(&["queue", "sendmeter", "add", "13", "12"]);
    lane(&c, 12);
    lane(&c, 13);
    // #12 holds aa, #13 holds bb; then each one's push touches the other's group.
    ev(&c, 12, "serial-joined", json!({"group": "aa", "by": "diff"}));
    ev(&c, 13, "serial-joined", json!({"group": "bb", "by": "diff"}));
    let r = fake(&[(12, "a/x\nb/x\n"), (13, "b/y\na/y\n")]);
    drive(&c, &r);
    assert_eq!(logged(&c, 12, "serial-held"), ["bb (13)"]);
    // #13 waits on aa, the earlier group, so it lets go of bb: #12 takes it and updates, though #13 is first in queue.
    assert_eq!(logged(&c, 13, "serial-held"), ["aa (12)", "bb (12)"]);
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["conflict"], "{out}");
    assert_eq!(logged(&c, 12, "serial-slot"), ["bb"]);
    ev(&c, 12, "merged", json!({}));
    let (out, steps) = drive(&c, &r);
    assert_eq!(steps, ["conflict"], "{out}");
    assert_eq!(logged(&c, 13, "serial-slot"), ["aa", "bb"]);
}

#[test]
fn a_marked_issue_logs_its_join_by_mark() {
    let (c, _sh) = sctx("sg-mark-log", &config("", DB_CI));
    let ok = |args: &[&str]| assert_eq!(run(&c, &Fake::default(), args).0, 0, "{args:?}");
    ok(&["queue", "sendmeter", "add", "12", "--serial", "ci"]);
    lane(&c, 12);
    let r = fake(&[(12, "README.md\n")]);
    drive(&c, &r);
    drive(&c, &r);
    let all = read(&c.state).unwrap();
    let by: Vec<_> = all
        .iter()
        .filter(|e| e.kind == "serial-joined")
        .map(|e| (e.str("group"), e.str("by")))
        .collect();
    assert_eq!(by, [(Some("ci"), Some("mark"))], "logged once");
}
