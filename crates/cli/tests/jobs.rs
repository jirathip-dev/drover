mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::json;

const NOW: u64 = 1_000_000;
const HOUR: u64 = 3600;
const AUDIT: &str = r#"
[job.audit]
every = "12h"
check = "audit-check {fleet}"
title = "audit: {fleet} {date}"
body = "Audit {fleet}.\n\n{report}"
role = "impl"
empty = "nothing to audit"

[job.lesson]
every = "off"
[job.triage]
every = 0
[job.deps]
every = "off"
[job.advisory]
every = "off"
"#;
const CHECK: &str = "sh -c cd \"$0\" && audit-check sendmeter";
const CREATED: &str = "https://github.com/jirathip-dev/sendmeter/issues/";

fn config(extra: &str) -> String {
    CONFIG.to_string() + AUDIT + extra
}

fn fake() -> Fake {
    Fake::default()
        .reply("herdr status", "server:\n  status: running\n")
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("git -C", "0\n")
        .reply("gh issue list", "[]")
        .reply("gh issue view", r#"{"title":"audit","body":"the entry"}"#)
}

fn tick(c: &drover::Ctx, now: u64, r: &Fake) -> String {
    let (code, out) = run(&drover::Ctx { now, ..c.clone() }, r, &["tick"]);
    assert_eq!(code, 0, "{out}");
    out
}

#[test]
fn a_job_runs_when_due_files_only_when_its_check_fails_and_12h_is_due_after_12_hours() {
    let c = ctx(&tmpdir("jobs-due"), &config(""));
    let _sh = FakeShed::attached(&c);
    let r = fake().once(CHECK, 0, "all clean\n");
    tick(&c, NOW, &r);
    assert!(r.called(CHECK), "{:?}", r.calls.borrow());
    assert!(!r.called("gh issue create"), "a passing check files nothing");

    let r = fake().once(CHECK, 1, "finding A\nfinding B\n");
    tick(&c, NOW + 11 * HOUR, &r);
    assert!(!r.called(CHECK), "not due before 12 hours");

    let r = fake()
        .once(CHECK, 1, "finding A\nfinding B\n")
        .reply("gh issue create", &format!("{CREATED}90\n"));
    tick(&c, NOW + 12 * HOUR, &r);
    let created = r
        .calls
        .borrow()
        .iter()
        .find(|l| l.starts_with("gh issue create"))
        .cloned();
    let created = created.expect("a failing check files");
    assert!(
        created.contains("--title audit: sendmeter 1970-01-13 --body Audit sendmeter.\n\nfinding A\nfinding B"),
        "{created}"
    );
    let events = read(&c.state).unwrap();
    let filed = events
        .iter()
        .find(|e| e.is("job-filed", "sendmeter", 90))
        .expect("job-filed");
    assert_eq!((filed.str("job"), filed.str("role")), (Some("audit"), Some("impl")));
    assert_eq!(
        drover::queue::replay(&events, "sendmeter").first().map(|i| i.issue),
        Some(90)
    );
}

#[test]
fn off_and_0_never_run_and_a_paused_fleet_waits() {
    let off = "\n[fleet.sendmeter.job.audit]\nevery = \"off\"\n";
    for (name, extra) in [
        ("jobs-off", off),
        ("jobs-zero", "\n[fleet.sendmeter.job.audit]\nevery = 0\n"),
    ] {
        let c = ctx(&tmpdir(name), &config(extra));
        let _sh = FakeShed::attached(&c);
        let r = fake().reply(CHECK, "");
        for days in [0, 1, 30, 400] {
            tick(&c, NOW + days * 86_400, &r);
        }
        assert!(
            !r.called(CHECK) && !r.called("gh issue"),
            "{name}: {:?}",
            r.calls.borrow()
        );
    }
    let c = ctx(
        &tmpdir("jobs-paused"),
        &config("").replace("[fleet.sendmeter]\n", "[fleet.sendmeter]\npaused = true\n"),
    );
    let _sh = FakeShed::attached(&c);
    let r = fake().reply(CHECK, "");
    tick(&c, NOW, &r);
    assert!(!r.called(CHECK), "paused");
}

#[test]
fn by_hand_a_job_ignores_its_check_but_never_files_while_one_is_open_or_the_title_exists() {
    let c = ctx(&tmpdir("jobs-hand"), &config(""));
    let _sh = FakeShed::attached(&c);
    let r = fake()
        .reply(CHECK, "all clean\n")
        .reply("gh issue create", &format!("{CREATED}91\n"));
    let (code, out) = run(&c, &r, &["job", "sendmeter", "audit"]);
    assert!(code == 0 || code == 4, "{code} {out}");
    assert!(out.contains("filed sendmeter#91: audit: sendmeter 1970-01-12"), "{out}");
    assert!(
        !c.state.with_file_name("audit-sendmeter.at").exists(),
        "a hand run leaves the stamp"
    );
    let spawn = read(&c.state).unwrap();
    let spawn = spawn.iter().find(|e| e.is("spawn", "sendmeter", 91)).expect("its lane");
    assert_eq!(spawn.str("role"), Some("impl"));

    let r = fake()
        .reply(CHECK, "")
        .reply("gh issue create", &format!("{CREATED}92\n"));
    let (code, out) = run(&c, &r, &["job", "sendmeter", "audit"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("its issue #91 is open"), "{out}");
    assert!(!r.called("gh issue create"));
    let (_, out) = run(&c, &r, &["job", "sendmeter"]);
    assert!(
        out.contains("audit      every 12h") && out.contains("open #91"),
        "{out}"
    );
    assert!(out.contains("lesson     off"), "{out}");

    append(
        &c.state,
        &Event::new(NOW, "sendmeter", Some(91), "closed", json!({"why": "x"})),
    )
    .unwrap();
    let r = fake()
        .reply(CHECK, "")
        .once(
            "gh issue list",
            0,
            r#"[{"number":91,"title":"audit: sendmeter 1970-01-12"}]"#,
        )
        .reply("gh issue create", &format!("{CREATED}93\n"));
    let (code, out) = run(&c, &r, &["job", "sendmeter", "audit"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("#91 has the title"), "{out}");
    assert!(!r.called("gh issue create"));
}

#[test]
fn a_refused_spawn_leaves_the_issue_queued_and_drive_starts_it_with_the_jobs_role() {
    let c = ctx(
        &tmpdir("jobs-refused"),
        &config("").replace("[fleet.sendmeter]\n", "[fleet.sendmeter]\npaused = true\n"),
    );
    let _sh = FakeShed::attached(&c);
    let r = fake()
        .reply(CHECK, "x\n")
        .reply("gh issue create", &format!("{CREATED}94\n"));
    let (code, out) = run(&c, &r, &["job", "sendmeter", "audit"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("sendmeter#94 stays queued") && out.contains("PAUSED"),
        "{out}"
    );
    let events = read(&c.state).unwrap();
    assert!(!events.iter().any(|e| e.is("spawn", "sendmeter", 94)));
    assert_eq!(drover::jobs::role(&events, "sendmeter", 94), Some("impl"));
    assert_eq!(drover::jobs::of(&events, "sendmeter", 94), Some("audit"));
}

#[test]
fn an_empty_jobs_lane_closes_its_issue_with_its_line_and_the_undo_list() {
    let c = ctx(&tmpdir("jobs-empty"), &config(""));
    let _sh = FakeShed::attached(&c);
    let r = fake()
        .reply(CHECK, "x\n")
        .reply("gh issue create", &format!("{CREATED}95\n"))
        .reply("gh issue close", "");
    run(&c, &r, &["job", "sendmeter", "audit"]);
    let undo = json!({"target": 7, "labels": ["stale?"], "undo": "gh issue edit 7 --remove-label stale?"});
    append(
        &c.state,
        &Event::new(NOW, "sendmeter", Some(95), "triage-labeled", undo),
    )
    .unwrap();
    let (code, out) = run(&c, &r, &["done", "sendmeter", "95", "--no-change", "nothing to audit"]);
    assert_eq!(code, 0, "{out}");
    let close = r
        .calls
        .borrow()
        .iter()
        .find(|l| l.starts_with("gh issue close 95"))
        .cloned();
    let close = close.expect("closed");
    assert!(
        close.contains("--comment nothing to audit\n- ")
            && close.contains("undo: `gh issue edit 7 --remove-label stale?`"),
        "{close}"
    );
    let events = read(&c.state).unwrap();
    assert_eq!(fold(&events)[&("sendmeter".into(), 95)].state, "closed");

    let r = fake().reply("gh issue close", "");
    run(&c, &r, &["spawn", "sendmeter", "96", "impl"]);
    let (code, out) = run(&c, &r, &["done", "sendmeter", "96", "--no-change", "nothing"]);
    assert_eq!(code, 0, "{out}");
    assert!(!r.called("gh issue close"), "not a job's issue");
}

#[test]
fn the_three_layers_merge_key_by_key_and_old_keys_are_read_with_a_warning() {
    let cfg = config("\n[fleet.sendmeter.job.audit]\nqueue = \"end\"\n[fleet.sendmeter.job.deps]\nevery = \"2d\"\n")
        .replace("[job.deps]\nevery = \"off\"\n", "")
        .replace("[job.triage]\nevery = 0\n", "")
        .replace(
            "integration = \"staging\"   # never main",
            "integration = \"staging\"\ntriage = \"weekly\"\nlessons = \"off\"",
        );
    let cfg = drover::config::Config::parse(&cfg).unwrap();
    let jobs = &cfg.fleet["sendmeter"].jobs;
    let audit = &jobs["audit"];
    assert_eq!(
        (audit.every, audit.queue, audit.role.as_str()),
        (12 * HOUR, drover::jobs::Place::End, "impl")
    );
    assert_eq!(jobs["deps"].every, 2 * 86_400, "the fleet's own over the built-in");
    assert!(
        jobs["deps"].title.starts_with("deps: {fleet}"),
        "the built-in's other keys"
    );
    assert_eq!(jobs["triage"].every, 7 * 86_400, "the old key, read");
    assert_eq!(jobs["lesson"].every, 0, "[job.lesson] off over the built-in");
    let w = cfg.warnings();
    let triage = w.iter().find(|w| w.0 == "triage").expect("a warning");
    assert!(
        triage.1.contains("[fleet.sendmeter.job.triage] every = \"7d\""),
        "{triage:?}"
    );
    assert!(w
        .iter()
        .any(|w| w.0 == "lessons" && w.1.contains("[fleet.sendmeter.job.lesson] every = \"off\"")));

    let builtin = drover::config::Config::parse(CONFIG).unwrap();
    let every = |j: &str| builtin.fleet["sendmeter"].jobs[j].every;
    assert_eq!(
        (every("triage"), every("lesson"), every("deps"), every("advisory")),
        (86_400, 7 * 86_400, 30 * 86_400, 86_400)
    );
    assert_eq!(builtin.warnings().len(), 0);
}

#[test]
fn each_load_error_names_the_job() {
    let new = "\n[job.x]\ntitle = \"x\"\nbody = \"b\"\nrole = \"impl\"\n";
    for (extra, want) in [
        (
            "\n[job.x]\ntitle = \"x\"\nbody = \"b\"\n",
            "fleet.sendmeter.job.x: no `role`",
        ),
        (
            "\n[job.x]\nbody = \"b\"\nrole = \"impl\"\n",
            "fleet.sendmeter.job.x: no `title`",
        ),
        (
            "\n[job.x]\ntitle = \"x\"\nrole = \"impl\"\n",
            "fleet.sendmeter.job.x: no `body`",
        ),
        (
            &new.replace("title = \"x\"", "title = \"x {week}\""),
            "has a placeholder",
        ),
        (&new.replace("role = \"impl\"", "role = \"a b\""), "is letters, digits"),
        (&format!("{new}every = \"3w\"\n"), "a duration like"),
        (&format!("{new}every = \"0h\"\n"), "a duration like"),
        (&format!("{new}queue = \"middle\"\n"), "middle"),
        (&format!("{new}colour = \"red\"\n"), "colour"),
    ] {
        let e = drover::config::Config::parse(&(CONFIG.to_string() + extra)).expect_err(extra);
        assert!(format!("{e:#}").contains(want), "{extra}: {e:#}");
    }
}

#[test]
fn a_job_that_fails_to_file_logs_job_failed() {
    let c = ctx(&tmpdir("jobs-failed"), &config(""));
    let _sh = FakeShed::attached(&c);
    let r = fake().once(CHECK, 1, "x\n").once_err("gh issue create", 1, "HTTP 502");
    tick(&c, NOW, &r);
    let events = read(&c.state).unwrap();
    let failed = events.iter().find(|e| e.kind == "job-failed").expect("job-failed");
    assert_eq!(failed.str("job"), Some("audit"));
    assert!(failed.str("why").unwrap_or_default().contains("HTTP 502"), "{failed:?}");
}

#[test]
fn only_an_orchestrator_or_the_owner_runs_a_job_by_hand_and_any_lane_lists() {
    let d = tmpdir("jobs-hook");
    std::fs::write(d.join("fleet.toml"), CONFIG).unwrap();
    let hook = |lane: &str, cmd: &str| {
        let input = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": cmd}});
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
    for lane in ["sendmeter:12:impl", "sendmeter:12:deps", "sendmeter:12:rev"] {
        assert!(
            hook(lane, "drover job sendmeter deps").contains("REFUSED ORCHESTRATOR_ONLY"),
            "{lane}"
        );
        assert_eq!(hook(lane, "drover job sendmeter"), "", "{lane} lists");
        assert_eq!(hook(lane, "drover lesson digest sendmeter --days 7"), "", "{lane}");
    }
    assert_eq!(hook("sendmeter:-:orchestrator", "drover job sendmeter lesson"), "");
}
