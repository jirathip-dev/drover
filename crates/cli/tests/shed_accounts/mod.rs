use super::*;

pub fn acct(name: &str, state: &str) -> Value {
    json!({"provider": "claude", "name": name, "state": state})
}

fn shed_run(f: &Fake) -> Option<String> {
    f.calls.borrow().iter().find(|c| c.starts_with("shed run")).cloned()
}

#[test]
fn a_roles_account_rides_its_acp_start_and_the_fleets_covers_the_others() {
    let b = FakeShed::new();
    let cfg = acp_config(&b.sock, "claude")
        .replace("# never main\n", "# never main\naccount = \"work\"\n")
        .replace("model = \"opus\"\n", "model = \"opus\"\naccount = \"auto\"\n");
    let c = acp_ctx("acct-role", &cfg);
    let f = acp_fake(&["echo", "claude-agent-acp"]);
    let (code, out) = run(&c, &b.cli(&f), &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "{out}");
    let start = shed_run(&f).unwrap();
    assert!(start.contains(" --account auto --label "), "{start}");
    assert!(
        !start.contains("unset CLAUDE_CODE_OAUTH_TOKEN"),
        "shed sets the account's login: {start}"
    );
    let ev = drover::events::read(&c.state).unwrap();
    let sp = ev.iter().rfind(|e| e.kind == "spawn").unwrap();
    assert_eq!(sp.str("account"), Some("auto"));
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let work = drover::config::Account::Named("work".into());
    assert_eq!(cfg.account("sendmeter", "rev"), work);
    assert_eq!(
        cfg.account("sendmeter", "orchestrator"),
        work,
        "the default orchestrator: the fleet's, not impl's"
    );
    let bad = drover::config::Config::parse(&cfg_text(&c).replace("account = \"work\"", "account = \"Work Two\""));
    assert!(format!("{:#}", bad.unwrap_err()).contains("fleet.sendmeter.account"));
}

fn cfg_text(c: &drover::Ctx) -> String {
    std::fs::read_to_string(&c.config).unwrap()
}

#[test]
fn a_signed_out_account_holds_its_starts_with_one_alert_whose_sign_in_is_account_login() {
    let b = FakeShed::new();
    b.lock().accounts = vec![acct("work", "signed-out"), acct("default", "ready")];
    let c = acp_ctx("acct-out", &acp_config(&b.sock, "claude"));
    let f = acp_fake(&["echo", "claude-agent-acp"]);
    let (code, out) = run(&c, &b.cli(&f), &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("REFUSED LOGIN_NEEDED: claude account work is signed out"),
        "{out}"
    );
    assert!(shed_run(&f).is_none(), "{:?}", f.calls.borrow());
    for _ in 0..2 {
        assert_eq!(run(&c, &b.cli(&Fake::default()), &["check"]).0, 0);
    }
    let ev = drover::events::read(&c.state).unwrap();
    let alerts: Vec<_> = ev.iter().filter(|e| e.kind == "signed-out").collect();
    assert_eq!(alerts.len(), 1, "one card: {alerts:?}");
    assert_eq!(alerts[0].str("signin"), Some("claude/work"));

    let cfg = drover::config::Config::load(&c.config).unwrap();
    let pane = drover::setup::start(&c, &cfg, &b.on(&Fake::default()), "main", "claude/work", "web").unwrap();
    assert_eq!(b.last("account.login"), json!({"provider": "claude", "name": "work"}));
    assert_eq!(pane, "login-claude-work");
    let screen = drover::setup::screen(&c, &b.on(&Fake::default()), &pane).unwrap();
    assert_eq!(screen["closed"], json!(false), "{screen}");

    b.lock().accounts = vec![acct("work", "ready")];
    let f = acp_fake(&["echo", "claude-agent-acp"]);
    let (code, out) = run(&c, &b.cli(&f), &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "ready again: no `hosts on` needed: {out}");
}

#[test]
fn an_account_holding_a_setup_is_refused_acp_unguarded() {
    let b = FakeShed::new();
    let mut a = acct("work", "ready");
    a["setup"] = json!(["settings.json"]);
    b.lock().accounts = vec![a];
    let cfg = acp_config(&b.sock, "claude").replace("# never main\n", "# never main\naccount = \"work\"\n");
    let c = acp_ctx("acct-setup", &cfg);
    let f = acp_fake(&["echo", "claude-agent-acp"]);
    let (code, out) = run(&c, &b.cli(&f), &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("REFUSED ACP_UNGUARDED: claude account work holds a setup (settings.json)"),
        "{out}"
    );
    assert!(shed_run(&f).is_none());
}

#[test]
fn the_default_account_runs_when_named_and_auto_never_takes_it() {
    let b = FakeShed::new();
    b.lock().accounts = vec![acct("default", "ready")];
    let c = acp_ctx("acct-default-auto", &acp_config(&b.sock, "claude"));
    let f = acp_fake(&["echo", "claude-agent-acp"]);
    let (code, out) = run(&c, &b.cli(&f), &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 3, "auto skips default: {out}");
    assert!(out.contains("REFUSED LOGIN_NEEDED"), "{out}");
    let cfg = acp_config(&b.sock, "claude").replace("# never main\n", "# never main\naccount = \"default\"\n");
    let c = acp_ctx("acct-default-named", &cfg);
    let f = acp_fake(&["echo", "claude-agent-acp"]);
    let (code, out) = run(&c, &b.cli(&f), &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "{out}");
    assert!(shed_run(&f).unwrap().contains(" --account default --label "));
}
