mod common;
use common::*;
use drover::events::read;

const NOW: u64 = 1_000_000;
const OFF: &str = "\n[job.lesson]\nevery = \"off\"\n[job.triage]\nevery = \"off\"\n";
const QUIET: &str = "\n[fleet.quiet]\nrepo = \"acme/quiet\"\nlocal = \"/tmp/does-not-matter/quiet\"\nintegration = \"staging\"\ndeps = \"off\"\n[fleet.quiet.role.impl]\nharness = \"claude\"\n";
const AUDIT: &str = "sh -c cd \"$0\" && { [ ! -f Cargo.lock ] || cargo deny check advisories; }";

fn spawner(n: u64) -> Fake {
    Fake::default()
        .reply("herdr status", "server:\n  status: running\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply(
            &format!("gh issue view {n}"),
            r#"{"title":"deps","body":"the template"}"#,
        )
        .reply("gh issue list", "[]")
        .reply(
            "gh issue create",
            &format!("https://github.com/jirathip-dev/sendmeter/issues/{n}\n"),
        )
        .reply("git -C", "0\n")
        .reply("which shed", "/opt/shed/bin/shed\n")
}

#[test]
fn deps_files_queues_and_spawns_a_deps_lane_and_never_a_second_while_one_is_open() {
    let c = ctx(&tmpdir("deps-run"), CONFIG);
    let sh = FakeShed::attached(&c);
    let r = spawner(80);
    let (code, out) = run(&c, &r, &["job", "sendmeter", "deps"]);
    assert!(code == 0 || code == 4, "{code} {out}");
    assert!(out.contains("filed sendmeter#80: deps: sendmeter 1970-01"), "{out}");
    let events = read(&c.state).unwrap();
    assert_eq!(
        drover::queue::replay(&events, "sendmeter").first().map(|i| i.issue),
        Some(80)
    );
    assert_eq!(sh.last("send")["name"], "sendmeter-deps-80", "{:?}", sh.methods());
    let spawn = events.iter().find(|e| e.is("spawn", "sendmeter", 80)).expect("a lane");
    assert_eq!(
        (spawn.str("role"), spawn.str("profile"), spawn.str("stage")),
        (Some("deps"), Some("fleet-impl"), Some("impl"))
    );
    let brief = std::fs::read_to_string(c.worktrees.join("sendmeter/issue-80/.brief.md")).unwrap();
    assert!(
        brief.contains("# ROLE: IMPLEMENTER") && brief.contains("# ROLE: DEPS") && brief.contains("open a PR to"),
        "{brief}"
    );

    let r = spawner(81);
    let (code, out) = run(&c, &r, &["job", "sendmeter", "deps"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("its issue #80 is open"), "{out}");
    assert!(!r.called("gh issue create"), "{:?}", r.calls.borrow());
}

#[test]
fn housekeeping_files_one_advisory_issue_with_the_tools_output_and_deps_monthly_where_not_off() {
    let c = ctx(&tmpdir("deps-tick"), &(CONFIG.to_string() + OFF + QUIET));
    let _sh = FakeShed::attached(&c);
    let old = c.state.with_file_name("deps-sendmeter");
    std::fs::create_dir_all(&old).unwrap();
    let tick = |now: u64, r: &Fake| {
        let (code, out) = run(&drover::Ctx { now, ..c.clone() }, r, &["housekeep"]);
        assert_eq!(code, 0, "{out}");
        let calls = r.calls.borrow();
        calls
            .iter()
            .filter(|l| l.starts_with("gh issue create"))
            .cloned()
            .collect::<Vec<_>>()
    };
    let checkout = c.state.with_file_name("job-sendmeter").display().to_string();
    let r = spawner(90)
        .once(
            AUDIT,
            1,
            &format!("{checkout}/Cargo.lock: RUSTSEC-2024-0001 smallvec: Upgrade to >=1.13.2\n"),
        )
        .once(
            "gh issue create",
            0,
            "https://github.com/jirathip-dev/sendmeter/issues/91\n",
        );
    let first = tick(NOW, &r);
    assert!(
        r.called(&format!(
            "git -C /tmp/does-not-matter/sendmeter worktree remove --force {}",
            old.display()
        )),
        "the old advisory checkout goes: {:?}",
        r.calls.borrow()
    );
    assert!(
        r.called(&format!(
            "git -C /tmp/does-not-matter/sendmeter worktree add -q --detach {checkout} origin/staging"
        )),
        "{:?}",
        r.calls.borrow()
    );
    assert!(!r.called("gh issue list -R acme/quiet"), "deps off: no run");
    assert_eq!(first.len(), 2, "{first:?}");
    assert!(
        first
            .iter()
            .any(|l| l.contains("--title deps: sendmeter 1970-01 --body")),
        "{first:?}"
    );
    let adv = first
        .iter()
        .find(|l| l.contains("--title deps: advisories 1970-01-12 "))
        .expect("one issue");
    assert!(
        adv.contains("./Cargo.lock: RUSTSEC-2024-0001 smallvec: Upgrade to >=1.13.2") && !adv.contains(&checkout),
        "{adv}"
    );

    let r = spawner(93).reply(AUDIT, "");
    assert!(tick(NOW + 3600, &r).is_empty(), "the same day: no check, no issue");
    assert!(!r.called(AUDIT));
    let r = spawner(93).reply(AUDIT, "");
    assert!(tick(NOW + 86_400, &r).is_empty(), "a passing check files nothing");
    assert!(r.called(AUDIT), "{:?}", r.calls.borrow());
}
