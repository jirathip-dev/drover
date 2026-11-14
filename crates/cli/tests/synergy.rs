mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::json;

const HEAD: &str = "1111111111111111111111111111111111111111";

fn config(extra: &str) -> String {
    CONFIG
        .replace(
            "integration = \"staging\"   # never main",
            &format!("integration = \"staging\"\n{extra}"),
        )
        .replace(
            "[limits]",
            "[fleet.sendmeter.role.design]\nharness = \"hermes\"\nprofile = \"fleet-design\"\n\n[limits]",
        )
}

const SERIAL: &str = "serial_paths = [\"supabase/migrations/**\"]";

fn happy() -> Fake {
    Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Swapouts: 100.\n")
        .reply("git -C", "")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("which shed", "/opt/shed/bin/shed\n")
}

fn spawned(c: &drover::Ctx, n: u64, role: &str) {
    let d = json!({"role": role, "agent": format!("sendmeter-{role}-{n}"), "branch": format!("issue-{n}")});
    append(&c.state, &Event::new(10, "sendmeter", Some(n), "spawn", d)).unwrap();
}

fn log(c: &drover::Ctx, n: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(20, "sendmeter", Some(n), kind, d)).unwrap();
}

fn spawn(c: &drover::Ctx, n: u64, role: &str) -> (i32, String) {
    run(c, &happy(), &["spawn", "sendmeter", &n.to_string(), role])
}

fn refused(c: &drover::Ctx, n: u64, role: &str, code: &str) {
    let before = read(&c.state).unwrap().len();
    let (exit, out) = spawn(c, n, role);
    assert_eq!(exit, 3, "{out}");
    assert!(out.contains(&format!("REFUSED {code}")), "{out}");
    assert_eq!(read(&c.state).unwrap().len(), before, "nothing logged");
}

#[test]
fn second_serial_impl_waits_until_first_merges() {
    let d = tmpdir("serial-busy");
    let c = ctx(&d, &config(SERIAL));
    let _sh = FakeShed::new();
    _sh.attach(&c);
    assert_eq!(
        run(
            &c,
            &Fake::default(),
            &["queue", "sendmeter", "add", "1", "2", "--serial"]
        )
        .0,
        0
    );
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "3"]).0, 0);
    let (exit, out) = spawn(&c, 1, "impl");
    assert_eq!(exit, 0, "{out}");
    refused(&c, 2, "impl", "SERIAL_BUSY");
    assert_eq!(spawn(&c, 3, "impl").0, 0);
    spawned(&c, 1, "rev");
    refused(&c, 2, "impl", "SERIAL_BUSY");
    log(&c, 1, "verdict", json!({"verdict": "PASS", "sha": HEAD}));
    refused(&c, 2, "impl", "SERIAL_BUSY");
    log(&c, 1, "merged", json!({}));
    let (exit, out) = spawn(&c, 2, "impl");
    assert_eq!(exit, 0, "allowed once #1 merged: {out}");
    let (_, ls) = run(&c, &Fake::default(), &["queue", "sendmeter", "ls"]);
    assert!(ls.contains("#2     impl  serial"), "{ls}");
}

#[test]
fn serial_max_two_admits_a_second_serial_and_refuses_a_third() {
    let d = tmpdir("serial-max");
    let c = ctx(&d, &config(&format!("{SERIAL}\nserial_max = 2")));
    let _sh = FakeShed::new();
    _sh.attach(&c);
    assert_eq!(
        run(
            &c,
            &Fake::default(),
            &["queue", "sendmeter", "add", "1", "2", "3", "--serial"]
        )
        .0,
        0
    );
    assert_eq!(spawn(&c, 1, "impl").0, 0);
    let (exit, out) = spawn(&c, 2, "impl");
    assert_eq!(exit, 0, "second serial admitted: {out}");
    let (_, board) = run(&c, &Fake::default(), &["board", "sendmeter"]);
    assert!(
        board.starts_with("sendmeter: 3 queued, 2 active, serial: db 2/2\n"),
        "{board}"
    );
    let (_, status) = run(&c, &Fake::default(), &["status", "sendmeter"]);
    assert!(status.contains("sendmeter: serial: db 2/2\n"), "{status}");
    let (exit, out) = spawn(&c, 3, "impl");
    assert_eq!(exit, 3, "{out}");
    assert!(
        out.contains("REFUSED SERIAL_BUSY") && out.contains("db: #1 #2 hold 2/2"),
        "names both: {out}"
    );
    log(&c, 1, "merged", json!({}));
    let (exit, out) = spawn(&c, 3, "impl");
    assert_eq!(exit, 0, "allowed once #1 merged: {out}");
}

#[test]
fn in_flight_serial_issue_keeps_its_lanes_when_over_full() {
    let d = tmpdir("serial-over");
    let c = ctx(&d, &config(SERIAL));
    let _sh = FakeShed::new();
    _sh.attach(&c);
    let add = ["queue", "sendmeter", "add", "1", "3", "--serial"];
    assert_eq!(run(&c, &Fake::default(), &add).0, 0);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "2"]).0, 0);
    assert_eq!(spawn(&c, 1, "impl").0, 0);
    spawned(&c, 2, "impl");
    log(&c, 2, "serial-auto", json!({"path": "supabase/migrations/x.sql"}));
    let (_, status) = run(&c, &Fake::default(), &["status", "sendmeter"]);
    assert!(status.contains("sendmeter: serial: db 2/1\n"), "{status}");
    let (_, board) = run(&c, &Fake::default(), &["board", "sendmeter"]);
    assert!(board.contains(", serial: db 2/1"), "{board}");
    for n in [1, 2] {
        let argv = ["spawn", "sendmeter", &n.to_string(), "impl", "--supersede"];
        let (exit, out) = run(&c, &happy(), &argv);
        assert_eq!(exit, 0, "#{n}'s own fix lane is admitted: {out}");
    }
    refused(&c, 3, "impl", "SERIAL_BUSY");
}

fn merge_fake(files: &str) -> Fake {
    let checks = json!([{"__typename": "CheckRun", "name": "build", "status": "COMPLETED", "conclusion": "SUCCESS"}]);
    let pr = json!({"number": 34, "state": "OPEN", "baseRefName": "staging", "headRefOid": HEAD, "statusCheckRollup": checks});
    Fake::default()
        .reply("gh pr view issue-12", &pr.to_string())
        .reply("gh api repos/jirathip-dev/sendmeter/pulls/34/files", files)
        .reply("gh pr merge 34", "")
        .reply("gh pr view 34", &json!({"mergeCommit": {"oid": "m3rg3"}}).to_string())
        .reply("gh issue", "")
}

fn merge(serial: bool, files: &str) -> (i32, String, Fake) {
    merge_on(serial, merge_fake(files)).2
}

fn merge_on(serial: bool, r: Fake) -> (drover::Ctx, FakeShed, (i32, String, Fake)) {
    let d = tmpdir("serial-merge");
    let c = ctx(&d, &config(SERIAL));
    let sh = FakeShed::new();
    sh.attach(&c);
    let mut q = vec!["queue", "sendmeter", "add", "12"];
    if serial {
        q.push("--serial");
    }
    run(&c, &Fake::default(), &q);
    spawned(&c, 12, "impl");
    log(&c, 12, "verdict", json!({"verdict": "PASS", "sha": HEAD}));
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    (c, sh, (exit, out, r))
}

const FILES: &str = "gh api repos/jirathip-dev/sendmeter/pulls/34/files --paginate";

fn many(n: usize, last: &str) -> String {
    (0..n)
        .map(|i| format!("supabase/schemas/t{i}.sql\n"))
        .collect::<String>()
        + last
}

#[test]
fn a_2011_file_pr_is_listed_whole_and_its_last_page_serial_path_refuses() {
    let (exit, out, r) = merge(false, &many(2010, "supabase/migrations/x.sql\n"));
    assert_eq!(exit, 3, "{out}");
    assert!(
        out.contains("REFUSED SERIAL_UNMARKED") && out.contains("supabase/migrations/x.sql"),
        "{out}"
    );
    assert!(r.called(FILES) && !r.called("git -C"), "{:?}", r.calls.borrow());
}

#[test]
fn a_rename_out_of_a_serial_path_reports_both_names() {
    let (c, _sh, (exit, out, _)) = merge_on(false, merge_fake("src/x.sql\tsupabase/migrations/x.sql\n"));
    assert!(exit == 3 && out.contains("supabase/migrations/x.sql"), "{out}");
    let cache = drover::gh::cache_path(&c, "diff", "sendmeter", 12);
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(cache).unwrap()).unwrap();
    assert_eq!(v["files"], json!(["src/x.sql", "supabase/migrations/x.sql"]));
}

#[test]
fn a_cached_head_is_not_listed_again() {
    let (c, _sh, (exit, _, r)) = merge_on(false, merge_fake("supabase/migrations/x.sql\n"));
    let (again, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert!(
        exit == 3 && again == 3 && out.contains("supabase/migrations/x.sql"),
        "{out}"
    );
    assert_eq!(r.count(FILES), 1, "{:?}", r.calls.borrow());
}

#[test]
fn above_the_apis_3000_files_the_checkout_lists_them() {
    let git = format!("git -C /tmp/does-not-matter/sendmeter diff --name-only --no-renames origin/staging...{HEAD}");
    let r = merge_fake(&many(3000, ""))
        .reply("git -C /tmp/does-not-matter/sendmeter fetch -q origin", "")
        .reply(&git, "supabase/schemas/t0.sql\nsupabase/migrations/x.sql\n");
    let (exit, out, r) = merge_on(false, r).2;
    assert!(exit == 3 && out.contains("supabase/migrations/x.sql"), "{out}");
    assert!(
        r.index("git -C /tmp/does-not-matter/sendmeter fetch") < r.index(&git),
        "{:?}",
        r.calls.borrow()
    );
}

#[test]
fn unmarked_pr_touching_serial_path_is_refused() {
    let (exit, out, r) = merge(false, "src/a.ts\nsupabase/migrations/x.sql\n");
    assert_eq!(exit, 3, "{out}");
    assert!(
        out.contains("REFUSED SERIAL_UNMARKED") && out.contains("supabase/migrations/x.sql"),
        "{out}"
    );
    assert!(!r.called("gh pr merge"));
}

#[test]
fn owner_override_still_refuses_an_unmarked_serial_pr() {
    let c = ctx(&tmpdir("serial-override"), &config(SERIAL));
    let _sh = FakeShed::new();
    _sh.attach(&c);
    spawned(&c, 12, "impl");
    log(&c, 12, "verdict", json!({"verdict": "PASS", "sha": HEAD}));
    let r = merge_fake("supabase/migrations/x.sql\n");
    let (exit, out) = run(&c, &r, &["merge", "sendmeter", "12", "--owner-override"]);
    assert!(exit == 3 && out.contains("REFUSED SERIAL_UNMARKED"), "{out}");
    assert!(!r.called("gh pr merge"));
}

#[test]
fn marked_pr_touching_serial_path_merges() {
    let (exit, out, r) = merge(true, "supabase/migrations/x.sql\n");
    assert_eq!(exit, 0, "{out}");
    assert!(r.called("gh pr merge 34"));
    assert!(
        !r.called("gh api repos/jirathip-dev/sendmeter/pulls/"),
        "a marked issue needs no diff"
    );
}

#[test]
fn pr_outside_serial_globs_merges_unmarked() {
    let (exit, out, r) = merge(false, "supabase/seed.sql\nsrc/migrations/x.sql\n");
    assert_eq!(exit, 0, "{out}");
    assert!(r.called("gh pr merge 34"));
}

#[test]
fn serial_globs() {
    use drover::config::glob;
    let g = |p: &str, s: &str| glob(p.as_bytes(), s.as_bytes());
    assert!(g("supabase/migrations/**", "supabase/migrations/x.sql"));
    assert!(g("supabase/migrations/**", "supabase/migrations/a/b.sql"));
    assert!(!g("supabase/migrations/**", "supabase/seed.sql"));
    assert!(g("db/*.sql", "db/x.sql") && !g("db/*.sql", "db/a/x.sql"));
    assert!(g("**/schema.prisma", "apps/web/schema.prisma"));
}

#[test]
fn no_cap_by_default_even_with_50_lanes() {
    let d = tmpdir("cap-unset");
    let c = ctx(&d, &config(""));
    let _sh = FakeShed::new();
    _sh.attach(&c);
    for n in 100..150 {
        spawned(&c, n, "impl");
    }
    for n in 200..250 {
        spawned(&c, n, "impl");
        log(&c, n, "spawn", json!({"role": "rev", "agent": format!("rev-{n}")}));
    }
    let (exit, out) = spawn(&c, 1, "impl");
    assert_eq!(exit, 0, "{out}");
    spawned(&c, 2, "impl");
    let (exit, out) = spawn(&c, 2, "rev");
    assert_eq!(exit, 0, "{out}");
}

#[test]
fn max_impl_refuses_the_third_impl() {
    let d = tmpdir("cap-impl");
    let c = ctx(&d, &config("max_impl = 2\nmax_rev = 1"));
    let _sh = FakeShed::new();
    _sh.attach(&c);
    assert_eq!(spawn(&c, 1, "impl").0, 0);
    assert_eq!(spawn(&c, 2, "impl").0, 0);
    refused(&c, 3, "impl", "CAP");
    assert_eq!(
        run(&c, &happy(), &["spawn", "sendmeter", "2", "impl", "--supersede"]).0,
        0
    );
    spawned(&c, 1, "rev");
    refused(&c, 2, "rev", "CAP");
}

#[test]
fn design_lane_spawns_but_never_moves_the_issue() {
    let d = tmpdir("design");
    let c = ctx(&d, &config(SERIAL));
    let sh = FakeShed::new();
    sh.attach(&c);
    let r = happy().once("git -C /tmp/does-not-matter/sendmeter rev-parse --verify", 1, "");
    let (exit, out) = run(&c, &r, &["spawn", "sendmeter", "12", "design"]);
    assert_eq!(exit, 0, "{out}");
    assert!(sh.called("run", "sendmeter-design-12"), "{:?}", sh.methods());
    let wt = c.worktrees.join("sendmeter/design-12");
    assert!(
        r.called(&format!(
            "git -C /tmp/does-not-matter/sendmeter worktree add -b design-12 {} origin/staging",
            wt.display()
        )),
        "{:?}",
        r.calls.borrow()
    );
    let brief = std::fs::read_to_string(c.worktrees.join("sendmeter/design-12/.brief.md")).unwrap();
    assert!(
        brief.contains("# ROLE: DESIGNER") && brief.contains("owner's call"),
        "{brief}"
    );
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)];
    assert_eq!(
        (st.state.as_str(), st.branch.as_deref(), st.lane.as_deref()),
        ("queued", None, None)
    );
    sh.set("sendmeter-design-12", |t| t["cwd"] = json!("/elsewhere"));
    let (exit, out) = run(&c, &happy(), &["spawn", "sendmeter", "12", "design"]);
    assert_eq!(exit, 3, "{out}");
    assert!(
        out.contains("REFUSED LANE_LIVE") && out.contains("sendmeter-design-12"),
        "{out}"
    );
    assert_eq!(spawn(&c, 12, "impl").0, 0);
}
