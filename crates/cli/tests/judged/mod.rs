use super::*;

fn judged_config(sock: &std::path::Path) -> String {
    acp_config(sock, "claude").replace(
        "[fleet.sendmeter.role.impl]\nharness = \"claude\"",
        "[fleet.sendmeter.role.impl]\nagent = \"fake-acp\"",
    )
}

fn catalog(cmd: &[&str]) -> String {
    json!([{"id": "fake-acp", "source": "built-in",
        "provider": {"agent": "fake", "model_env": "FAKE_MODEL", "efforts": ["high"], "install": "fake install"},
        "installed": {"id": "fake-acp", "version": "1.0.0", "distribution": "binary", "cmd": cmd}}])
    .to_string()
}

fn judged_fake(help: &str, cmd: &[&str]) -> Fake {
    spawn_fake()
        .reply("shed help run", help)
        .reply("shed agents --json", &catalog(cmd))
        .reply("shed run", "")
        .reply("shed send", "")
        .reply("shed wait", "")
        .reply("shed rm", "")
        .reply("shed cancel", "")
}

fn log(c: &drover::Ctx) -> String {
    let all = drover::events::read(&c.state).unwrap();
    all.iter().map(|e| serde_json::to_string(e).unwrap() + "\n").collect()
}

fn ready(b: &FakeShed) {
    b.lock()
        .accounts
        .push(json!({"provider": "fake", "name": "work", "state": "ready"}));
}

const JUDGE: &str = "--meta <JSON>\n--judge <CMD>";

fn stamp(c: &drover::Ctx, verdict: &str) {
    let d = json!({"acp": "fake-acp", "version": "1.0.0", "verdict": verdict, "why": "test"});
    drover::events::append(&c.state, &Event::new(c.now, "drover", None, "agent-guard", d)).unwrap();
}

#[test]
fn a_role_on_an_agent_drover_has_no_module_for_spawns_judged() {
    let b = FakeShed::new();
    ready(&b);
    let c = acp_ctx("acp-judged", &judged_config(&b.sock));
    stamp(&c, "guarded");
    let f = judged_fake(JUDGE, &["env"]);
    let cli = b.cli(&f);
    let (code, out) = run(&c, &cli, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "{out}");
    let calls = f.calls.borrow();
    let start = calls.iter().find(|c| c.starts_with("shed run")).unwrap();
    let exe = std::fs::canonicalize(&c.exe).unwrap();
    for w in [
        "--acp fake-acp ".to_string(),
        "--policy judge ".into(),
        format!("--judge {} hook --acp --judge-secs 30 ", exe.display()),
        "--env DROVER_LANE=sendmeter:12:impl ".into(),
        "--env DROVER_GUARD=judge ".into(),
        "--model opus ".into(),
    ] {
        assert!(start.contains(&w), "{w} not in {start}");
    }
    assert!(!start.contains(" --meta "), "no Claude session options: {start}");
    let env = &cli.out.borrow()[0];
    assert!(
        env.contains("DROVER_LANE=sendmeter:12:impl") && env.contains("DROVER_GUARD=judge"),
        "{env}"
    );
    let log = log(&c);
    let sp = log
        .lines()
        .rfind(|l| l.contains("\"type\":\"spawn\""))
        .unwrap_or_else(|| panic!("{log}"));
    assert!(
        sp.contains("\"guard\":\"judge\"") && sp.contains("\"acp\":\"fake-acp\""),
        "{sp}"
    );
}

#[test]
fn a_judged_agent_without_a_judging_shed_or_with_a_bad_value_is_refused() {
    let b = FakeShed::new();
    ready(&b);
    let c = acp_ctx("acp-judged-old", &judged_config(&b.sock));
    stamp(&c, "guarded");
    let f = judged_fake("--meta <JSON>", &["env"]);
    let (code, out) = run(&c, &b.cli(&f), &["spawn", "sendmeter", "12", "impl"]);
    assert!(
        code == 3 && out.contains("REFUSED ACP_UNGUARDED") && out.contains("fake-acp"),
        "{out}"
    );
    assert!(out.contains("no judge policy"), "{out}");
    let cfg = judged_config(&b.sock).replace("agent = \"fake-acp\"", "agent = \"fake-acp\"\neffort = \"low\"");
    let c = acp_ctx("acp-judged-effort", &cfg);
    let f = judged_fake(JUDGE, &["env"]);
    let (code, out) = run(&c, &b.cli(&f), &["spawn", "sendmeter", "12", "impl"]);
    assert!(
        code == 3 && out.contains("REFUSED BAD_VALUE") && out.contains("effort \"low\""),
        "{out}"
    );
}

#[test]
fn the_test_run_lets_only_an_agent_that_asks_first_start() {
    let exe = env!("CARGO_BIN_EXE_drover");
    let ask = |cmd: &str| {
        let req = json!({"tool_call": {"kind": "execute", "rawInput": {"command": cmd}}}).to_string();
        format!("printf '%s' '{req}' | '{exe}' hook --acp --probe \"$PWD\" >/dev/null; true")
    };
    let asks = ask("touch ran-shell");
    let unreadable = "printf '%s' '{\"tool_call\":{\"kind\":\"execute\",\"rawInput\":{}}}' | '".to_string()
        + exe
        + "' hook --acp --probe \"$PWD\" >/dev/null; true";
    for (name, script, verdict) in [
        ("acp-probe-guarded", asks.as_str(), None),
        (
            "acp-probe-unasked",
            "touch ran-shell",
            Some("unguarded: ran a shell command without asking"),
        ),
        ("acp-probe-unreadable", unreadable.as_str(), Some("unreadable")),
        ("acp-probe-silent", "true", Some("inconclusive")),
    ] {
        let b = FakeShed::new();
        ready(&b);
        let c = acp_ctx(name, &judged_config(&b.sock));
        let f = judged_fake(JUDGE, &["sh", "-c", script]);
        let (code, out) = run(&c, &b.cli(&f), &["spawn", "sendmeter", "12", "impl"]);
        let log = log(&c);
        let stamp = log
            .lines()
            .find(|l| l.contains("\"agent-guard\""))
            .unwrap_or_else(|| panic!("{name}: {log}"));
        match verdict {
            None => {
                assert_eq!(code, 0, "{name}: {out}");
                assert!(stamp.contains("\"verdict\":\"guarded\""), "{name}: {stamp}");
            }
            Some(v) => {
                assert!(code == 3 && out.contains("REFUSED ACP_UNGUARDED"), "{name}: {out}");
                assert!(
                    out.contains("agent fake-acp 1.0.0 is ") && out.contains(v),
                    "{name}: {out}"
                );
                let (again, out2) = run(&c, &b.cli(&f), &["spawn", "sendmeter", "12", "impl"]);
                assert!(
                    again == 3 && out2.contains(v),
                    "{name}: the stamp refuses at once: {out2}"
                );
                let runs = f
                    .calls
                    .borrow()
                    .iter()
                    .filter(|c| c.contains("drover-probe-fake-acp --cwd"))
                    .count();
                assert_eq!(runs, 1, "{name}: one test run per agent version");
            }
        }
    }
}

#[test]
fn a_rev_on_the_impls_program_is_refused_at_spawn() {
    let b = FakeShed::new();
    let cfg = acp_config(&b.sock, "claude").replace(
        "[fleet.sendmeter.role.rev]\nharness = \"claude\"",
        "[fleet.sendmeter.role.rev]\nagent = \"claude-acp\"",
    );
    assert!(
        drover::config::Config::parse(&cfg).is_ok(),
        "the check needs shed's list: not at load"
    );
    let c = acp_ctx("acp-same-vendor", &cfg);
    let agents =
        json!([{"id": "claude-acp", "provider": {"agent": "claude"}, "installed": {"version": "1", "cmd": ["env"]}}]);
    let f = spawn_fake()
        .reply("shed help run", "--meta <JSON>")
        .reply("shed agents --json", &agents.to_string())
        .reply("shed run", "")
        .reply("shed send", "");
    let (code, out) = run(&c, &b.cli(&f), &["spawn", "sendmeter", "12", "rev", "--dry-run"]);
    assert!(
        code == 3 && out.contains("REVIEW_SAME_PROVIDER") && out.contains("claude"),
        "{out}"
    );
    let ok = cfg.replace(
        "integration = \"staging\"   # never main",
        "integration = \"staging\"\nrev_same_provider = true # one program for now (q911)",
    );
    let c = acp_ctx("acp-same-vendor-ok", &ok);
    let (code, out) = run(&c, &b.cli(&f), &["spawn", "sendmeter", "12", "rev", "--dry-run"]);
    assert!(!out.contains("REVIEW_SAME_PROVIDER"), "{code}: {out}");
}

#[test]
fn the_follower_tells_a_judged_lane_its_refusals_once_and_stops_one_that_acts_unasked() {
    let lane = "sendmeter-impl-12";
    let ev = |seq: u64, kind: Value| {
        let mut e = json!({"seq": seq, "name": lane, "labels": lane_labels()});
        e.as_object_mut().unwrap().extend(kind.as_object().unwrap().clone());
        e
    };
    let push = json!({"command": "git push origin staging"});
    let events = vec![
        ev(1, json!({"event": "turn", "prompt": "go"})),
        ev(
            2,
            json!({"event": "tool_call", "id": "c1", "title": "git push", "status": "pending", "kind": "execute", "raw_input": push}),
        ),
        ev(
            3,
            json!({"event": "approval_request", "id": "q1", "tool": "git push origin staging", "options": [],
            "call": {"toolCallId": "c1", "kind": "execute", "rawInput": push}}),
        ),
        ev(
            4,
            json!({"event": "approved", "option": "r", "by": "judge", "decision": "deny",
            "reason": "drover: pushing to `staging` is blocked"}),
        ),
        ev(
            5,
            json!({"event": "tool_call", "id": "c1", "title": "git push", "status": "failed", "kind": "execute", "raw_input": push}),
        ),
        ev(6, json!({"event": "reply", "text": "done", "stop_reason": "end_turn"})),
        ev(7, json!({"event": "turn", "prompt": "again"})),
        ev(
            8,
            json!({"event": "approved", "option": "r", "by": "judge", "decision": "deny",
            "reason": "drover: pushing to `staging` is blocked"}),
        ),
        ev(9, json!({"event": "reply", "text": "done", "stop_reason": "end_turn"})),
        ev(10, json!({"event": "turn", "prompt": "third"})),
        ev(
            11,
            json!({"event": "tool_call", "id": "c9", "title": "gh pr merge 5", "status": "completed", "kind": "execute",
            "raw_input": {"command": "gh pr merge 5"}}),
        ),
    ];
    let b = FakeShed::start(shed::proto::VERSION, events);
    let c = ctx(&tmpdir("acp-follow"), &judged_config(&b.sock));
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let wt = c.state.parent().unwrap().join("wt");
    std::fs::create_dir_all(&wt).unwrap();
    let sp = json!({"agent": lane, "pane": lane, "role": "impl", "worktree": wt, "guard": "judge", "acp": "fake-acp",
        "version": "1.0.0", "runtime": "shed-acp"});
    drover::events::append(&c.state, &Event::new(c.now, "sendmeter", Some(12), "spawn", sp)).unwrap();
    std::fs::write(c.state.with_file_name("turn-sendmeter-12-impl"), "turn").unwrap();
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let f = Fake::default()
        .reply("shed send", "")
        .reply("git -C", "")
        .reply("shed kill", "")
        .reply("shed rm", "");
    let wire = b.shed().wire;
    let r = OnShed(f, b);
    drover::session::stamp::intake(&c, &cfg, &r, &wire).unwrap();
    let calls = r.0.calls.borrow();
    let sends: Vec<_> = calls.iter().filter(|c| c.starts_with("shed send")).collect();
    assert_eq!(
        sends.len(),
        2,
        "a reason is told once per lane, the turn line once: {calls:?}"
    );
    assert!(sends[1].contains("end your turn with `drover turn"), "{}", sends[1]);
    let kicks = calls
        .iter()
        .filter(|c| c.contains(" kick sendmeter 12 --why stop"))
        .count();
    assert_eq!(kicks, 2, "each turn's end kicks drive: {calls:?}");
    assert!(
        !calls.iter().any(|c| c.contains("--why push")),
        "a refused push kicks nothing: {calls:?}"
    );
    assert!(
        sends[0].contains("drover refused 1 call(s):\n- git push origin staging: pushing to `staging` is blocked"),
        "{}",
        sends[0]
    );
    let log = log(&c);
    assert!(
        !log.contains("\"acp-unasked\""),
        "the asked push is no unasked call: {log}"
    );
    let unguarded = log
        .lines()
        .find(|l| l.contains("\"lane-unguarded\""))
        .unwrap_or_else(|| panic!("{log}"));
    assert!(
        unguarded.contains("gh pr merge 5") && unguarded.contains("merge"),
        "{unguarded}"
    );
    assert!(
        log.contains("\"agent-guard\"") && log.contains("\"verdict\":\"unguarded\""),
        "{log}"
    );
}

#[test]
fn drover_set_puts_a_role_on_any_agent() {
    let b = FakeShed::new();
    let c = acp_ctx("acp-set-agent", &acp_config(&b.sock, "claude"));
    let (code, out) = run(
        &c,
        &Fake::default(),
        &["set", "sendmeter", "rev", "--agent", "fake-acp"],
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("sendmeter.rev agent: - -> fake-acp"), "{out}");
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let rev = &cfg.fleet["sendmeter"].role["rev"];
    assert_eq!((rev.agent.as_deref(), rev.harness.as_str()), (Some("fake-acp"), ""));
}
