mod common;
use common::*;
use drover::events::read;

const NOW: u64 = 1_000_000;
const LIST: &str = "gh issue list -R jirathip-dev/sendmeter --state";
const MONTHLY: &str = "\n[fleet.quiet]\nrepo = \"acme/quiet\"\nlocal = \"/tmp/does-not-matter/quiet\"\nintegration = \"staging\"\n[fleet.quiet.role.impl]\nharness = \"claude\"\n";
const RUSTSEC: &str = r#"{"type":"diagnostic","fields":{"severity":"error","code":"vulnerability","advisory":{"id":"RUSTSEC-2024-0001","package":"smallvec"},"notes":["ID: RUSTSEC-2024-0001","Solution: Upgrade to >=1.13.2"]}}"#;
const PNPM: &str = r#"{"advisories":{"1096366":{"module_name":"semver","patched_versions":">=7.5.2","github_advisory_id":"GHSA-c2qf-rxjj-qqgw"}}}"#;

fn spawner(n: u64, open: &str) -> Fake {
    Fake::default()
        .reply(
            &format!("gh issue view {n}"),
            r#"{"title":"deps","body":"the template"}"#,
        )
        .reply(&format!("{LIST} open"), open)
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
    let r = spawner(80, "[]");
    let (code, out) = run(&c, &r, &["deps", "sendmeter"]);
    assert!(code == 0 || code == 4, "{code} {out}");
    assert!(out.contains("filed sendmeter#80: deps: sendmeter 1970-01"), "{out}");
    let events = read(&c.state).unwrap();
    assert_eq!(
        drover::queue::replay(&events, "sendmeter").first().map(|i| i.issue),
        Some(80)
    );
    assert_eq!(sh.last("run")["name"], "sendmeter-deps-80", "{:?}", sh.methods());
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

    let r = spawner(81, r#"[{"number":80,"title":"deps: sendmeter 1970-01"}]"#);
    let (code, out) = run(&c, &r, &["deps", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("deps issue #80 is open"), "{out}");
    assert!(!r.called("gh issue create"), "{:?}", r.calls.borrow());
}

fn ticker(n: u64, filed: &str) -> Fake {
    Fake::default()
        .reply("herdr status", "server:\n  status: running\n")
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply(&format!("{LIST} open"), "[]")
        .reply(
            &format!("{LIST} all --search \"deps: advisory RUSTSEC-2024-0001\""),
            filed,
        )
        .reply(&format!("{LIST} all"), "[]")
        .once_err("cargo deny", 1, &format!("{RUSTSEC}\n{RUSTSEC}\nerror: 1 advisory\n"))
        .once("pnpm audit --json", 1, PNPM)
        .once(
            "gh issue create",
            0,
            &format!("https://github.com/jirathip-dev/sendmeter/issues/{n}\n"),
        )
        .once(
            "gh issue create",
            0,
            &format!("https://github.com/jirathip-dev/sendmeter/issues/{}\n", n + 1),
        )
        .once(
            "gh issue create",
            0,
            &format!("https://github.com/jirathip-dev/sendmeter/issues/{}\n", n + 2),
        )
        .reply("gh issue view", r#"{"title":"deps","body":"the template"}"#)
        .reply("git -C", "0\n")
}

fn created(r: &Fake) -> Vec<String> {
    let calls = r.calls.borrow();
    calls
        .iter()
        .filter(|l| l.starts_with("gh issue create"))
        .cloned()
        .collect()
}

#[test]
fn tick_runs_deps_monthly_and_files_each_new_advisory_once_only_where_monthly() {
    let cfg = CONFIG.replace(
        "integration = \"staging\"   # never main",
        "integration = \"staging\"\ndeps = \"monthly\"",
    );
    let c = ctx(&tmpdir("deps-tick"), &(cfg + MONTHLY));
    let _sh = FakeShed::attached(&c);
    let head = c.state.with_file_name("deps-sendmeter");
    std::fs::create_dir_all(&head).unwrap();
    for f in ["Cargo.lock", "pnpm-lock.yaml"] {
        std::fs::write(head.join(f), "").unwrap();
    }
    let tick = |now: u64, r: &Fake| {
        let (code, out) = run(&drover::Ctx { now, ..c.clone() }, r, &["tick"]);
        assert_eq!(code, 0, "{out}");
        created(r)
    };

    let r = ticker(90, "[]");
    let first = tick(NOW, &r);
    assert!(
        r.called(
            "git -C /tmp/does-not-matter/sendmeter fetch -q origin +refs/heads/staging:refs/remotes/origin/staging"
        ),
        "{:?}",
        r.calls.borrow()
    );
    let at = format!("git -C {} checkout -q --force --detach origin/staging", head.display());
    assert!(r.called(&at), "{:?}", r.calls.borrow());
    assert!(!r.called("gh issue list -R acme/quiet"), "deps off: no run");
    assert_eq!(first.len(), 3, "{first:?}");
    assert!(first[0].contains("--title deps: sendmeter 1970-01 --body"), "{first:?}");
    let sec = first
        .iter()
        .find(|l| l.contains("--title deps: advisory RUSTSEC-2024-0001 "))
        .expect("one issue");
    assert!(
        sec.contains("`smallvec`") && sec.contains("Upgrade to >=1.13.2"),
        "{sec}"
    );
    let ghsa = first
        .iter()
        .find(|l| l.contains("--title deps: advisory GHSA-c2qf-rxjj-qqgw "))
        .expect("pnpm's");
    assert!(ghsa.contains("`semver`") && ghsa.contains(">=7.5.2"), "{ghsa}");

    let filed = r#"[{"number":91,"title":"deps: advisory RUSTSEC-2024-0001"}]"#;
    let r = ticker(93, filed);
    assert!(tick(NOW + 3600, &r).is_empty(), "the same day: no check, no issue");
    assert!(!r.called("cargo deny"), "{:?}", r.calls.borrow());
    let r = ticker(93, filed);
    let next = tick(NOW + 86400, &r);
    assert!(r.called("cargo deny --manifest-path"), "{:?}", r.calls.borrow());
    assert_eq!(next.len(), 1, "only the advisory with no issue yet: {next:?}");
    assert!(next[0].contains("deps: advisory GHSA-c2qf-rxjj-qqgw"), "{next:?}");
    assert!(
        !next.iter().any(|l| l.contains("deps: sendmeter")),
        "a month between monthly runs"
    );
    let q = drover::queue::replay(&read(&c.state).unwrap(), "sendmeter");
    assert_eq!(q.first().map(|i| i.issue), Some(93), "an advisory goes on top");
    let r = ticker(96, filed);
    assert!(
        tick(NOW + 30 * 86400, &r)
            .iter()
            .any(|l| l.contains("--title deps: sendmeter 1970-02")),
        "a month on"
    );
}

#[test]
fn only_an_orchestrator_or_the_owner_runs_drover_deps() {
    let d = tmpdir("deps-hook");
    std::fs::write(d.join("fleet.toml"), CONFIG).unwrap();
    let hook = |lane: &str| {
        let input = serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "Bash",
            "tool_input": {"command": "drover deps sendmeter"}});
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_drover"))
            .args(["hook", "--role", "impl"])
            .current_dir(&d)
            .env_remove("FLEET_LANE")
            .env_remove("DROVER_SCRATCH_TARGET")
            .env("DROVER_LANE", lane)
            .env("FLEET_CONFIG", d.join("fleet.toml"))
            .env("FLEET_STATE", d.join("state/events.jsonl"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut child.stdin.take().unwrap(), input.to_string().as_bytes()).unwrap();
        String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap()
    };
    assert!(hook("sendmeter:12:deps").contains("REFUSED ORCHESTRATOR_ONLY"));
    assert!(hook("sendmeter:12:impl").contains("REFUSED ORCHESTRATOR_ONLY"));
    assert_eq!(hook("sendmeter:-:orchestrator"), "");
}
