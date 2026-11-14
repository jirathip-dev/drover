//! #5: per-role Anthropic accounts with cap fallback; per-role MCP allowlist.
mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::{json, Value};
use std::path::Path;

const TOKEN_A: &str = concat!("sk", "-ant-oat01-AAAA-secret");
const TOKEN_B: &str = concat!("sk", "-ant-oat01-BBBB-secret");

/// Claude impl role with `role_extra` lines, `[accounts]` A/B/C, local repo `<dir>/repo`.
fn config(d: &Path, role_extra: &str) -> String {
    let repo = d.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let mcp = r#"{"mcpServers":{"supabase-dev":{"command":"npx","args":["dev"]},"supabase-prod":{"command":"npx","args":["prod"]},"other":{"url":"http://x"}}}"#;
    std::fs::write(repo.join(".mcp.json"), mcp).unwrap();
    CONFIG
        .replace("harness  =  \"hermes\"    # toggled in tests", &format!("harness = \"claude\"\n{role_extra}"))
        .replace("/tmp/does-not-matter/sendmeter", repo.to_str().unwrap())
        + "\n[accounts]\nA = \"CLAUDE_CODE_OAUTH_TOKEN_A\"\nB = \"CLAUDE_CODE_OAUTH_TOKEN_B\"\nC = \"CLAUDE_CODE_OAUTH_TOKEN_C\"\n"
}

/// #683: `config` with the accounts' folders `<dir>/accounts/<name>` as `[accounts.usage]`.
fn folders(d: &Path, role_extra: &str) -> String {
    let dirs = format!(
        "A = \"{0}/A\"\nB = \"{0}/B\"\nC = \"{0}/C\"\n",
        d.join("accounts").display()
    );
    config(d, role_extra) + "\n[accounts.usage]\n" + &dirs
}

fn shell(sh: &FakeShed, lane: &str, var: &str) -> String {
    let typed = sh.sent(lane);
    let line = typed.iter().find(|t| t.starts_with("export ")).expect("no export line");
    let line = format!("{}; printf %s \"${var}\"", line.trim_end_matches('\r'));
    let mut c = std::process::Command::new("sh");
    c.env_remove("CLAUDE_CODE_OAUTH_TOKEN").env_remove("CLAUDE_CONFIG_DIR");
    String::from_utf8_lossy(&c.arg("-c").arg(line).output().unwrap().stdout).into()
}

fn keep_secrets(c: &drover::Ctx, n: u64) {
    use std::os::unix::fs::PermissionsExt;
    let f = format!("{}.env", c.worktrees.join(format!("sendmeter/issue-{n}")).display());
    std::fs::create_dir_all(Path::new(&f).parent().unwrap()).unwrap();
    std::fs::write(&f, "").unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::hard_link(&f, format!("{f}.keep")).unwrap();
}

fn spawn(c: &drover::Ctx, sh: &FakeShed, n: u64) -> (Fake, String) {
    spawn_var(c, sh, n, SIGN_IN)
}

fn spawn_var(c: &drover::Ctx, sh: &FakeShed, n: u64, var: &'static str) -> (Fake, String) {
    spawn_with(c, sh, n, var, |r| r)
}

/// As `spawn_var`, its runner given more replies by `more`.
fn spawn_with(
    c: &drover::Ctx,
    sh: &FakeShed,
    n: u64,
    var: &'static str,
    more: impl Fn(Fake) -> Fake,
) -> (Fake, String) {
    let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
    std::fs::create_dir_all(&wt).unwrap();
    let r = more(Fake::default())
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply(&format!("git -C {} rev-list", wt.display()), "0\n")
        .reply(&format!("gh issue view {n}"), r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "");
    let lane = format!("sendmeter-impl-{n}");
    let (code, out) = run(c, &r, &["spawn", "sendmeter", &n.to_string(), "impl"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    assert_eq!(sh.last("run")["name"], lane, "{:?}", sh.methods());
    assert!(
        !out.contains(TOKEN_A) && !out.contains(TOKEN_B),
        "token in stdout: {out}"
    );
    let secrets = format!("{}.env", wt.display());
    assert!(!Path::new(&secrets).exists(), "the secrets file is left behind");
    let kept = format!("{secrets}.keep");
    if Path::new(&kept).exists() {
        std::fs::hard_link(&kept, &secrets).unwrap();
    }
    let token = shell(sh, &lane, var);
    for f in [&secrets, &kept] {
        let _ = std::fs::remove_file(f);
    }
    (r, token)
}

fn lines(sh: &FakeShed, r: &Fake, n: u64) -> Vec<String> {
    let mut all = sh.sent(&format!("sendmeter-impl-{n}"));
    all.extend(r.calls.borrow().iter().cloned());
    all
}

fn dry_account(c: &drover::Ctx, n: u64) -> Value {
    let (code, out) = run(
        c,
        &Fake::default().reply("which shed", "/opt/shed/bin/shed\n"),
        &["spawn", "sendmeter", &n.to_string(), "impl", "--dry-run", "--json"],
    );
    assert_eq!(code, 0, "{out}");
    serde_json::from_str::<Value>(&out).unwrap()["account"].clone()
}

fn no_token_in_log(c: &drover::Ctx) {
    let log = drover::events::jsonl(&c.state).unwrap_or_default();
    assert!(
        !log.contains(TOKEN_A) && !log.contains(TOKEN_B),
        "token in events: {log}"
    );
}

#[test]
fn role_account_b_signs_in_with_its_folder_and_no_token_with_no_helper() {
    let d = tmpdir("acct-b");
    let c = ctx(&d, &folders(&d, "account = \"B\""));
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let (r, got) = spawn(&c, &sh, 12);
    // #683: with no `account_env`, B's folder as CLAUDE_CONFIG_DIR; no token, no secrets file.
    let calls = lines(&sh, &r, 12);
    assert!(
        !calls.iter().any(|l| l.contains(TOKEN_B)),
        "token passed through argv or the pane"
    );
    assert_eq!(got, format!("{}/accounts/B|", d.display()));
    assert!(!calls.iter().any(|l| l.contains("&& . '")), "a secrets file: {calls:?}");
    let typed = sh.sent("sendmeter-impl-12");
    let at = |p: &str| typed.iter().position(|t| t.starts_with(p));
    assert!(at("export ").unwrap() < at("exec 'claude'").unwrap(), "{typed:?}");
    let ev = read(&c.state).unwrap();
    assert_eq!(ev.last().unwrap().str("account"), Some("B"));
    no_token_in_log(&c);

    // The binary's stdout and stderr (dry run, `accounts`) never show a token either.
    for args in [
        &["spawn", "sendmeter", "13", "impl", "--dry-run", "--json"][..],
        &["accounts"],
    ] {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_drover"))
            .args(args)
            .env("FLEET_CONFIG", &c.config)
            .env("FLEET_STATE", &c.state)
            .env("FLEET_WORKTREES", &c.worktrees)
            .env("PATH", FakeShed::path_in(&d))
            .output()
            .unwrap();
        let (so, se) = (String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        assert!(o.status.success(), "{args:?}: {so} {se}");
        assert!(so.contains('B'), "{so}");
        for t in [TOKEN_A, TOKEN_B] {
            assert!(!so.contains(t) && !se.contains(t), "token leaked: {so} {se}");
        }
    }
}

#[test]
fn capped_account_is_skipped_until_reset() {
    let d = tmpdir("acct-cap");
    let c = ctx(&d, &config(&d, "accounts = [\"A\", \"B\"]"));
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, &sh, 12);
    assert_eq!(read(&c.state).unwrap().last().unwrap().str("account"), Some("A"));

    let wt = c.worktrees.join("sendmeter/issue-12");
    sh.lane("sendmeter-impl-12", "idle", wt.to_str().unwrap());
    sh.screen(
        "sendmeter-impl-12",
        "  ⎿  You've hit your limit · resets 3pm (Asia/Bangkok)\n",
    );
    let r = Fake::default();
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("account-capped: A:"), "{out}");
    let capped: Vec<_> = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == "account-capped")
        .collect();
    assert_eq!((capped.len(), capped[0].str("account")), (1, Some("A")));
    assert_eq!(run(&c, &r, &["check"]).1, "", "fires once per lane");

    assert_eq!(dry_account(&c, 13), "B");
    let (_, list) = run(&c, &Fake::default(), &["accounts"]);
    assert!(
        list.lines()
            .any(|l| l.starts_with("A ") && l.contains("capped until ") && l.contains("1 lanes sendmeter-impl-12")),
        "{list}"
    );
    assert!(list.lines().any(|l| l.starts_with("B ") && l.contains(" ok")), "{list}");

    let (code, out) = run(&c, &Fake::default(), &["accounts", "reset", "A"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(dry_account(&c, 14), "A");
    // The capped lane still shows the banner, but its cap was already reported: no re-cap.
    assert_eq!(run(&c, &r, &["check"]).1, "");
    assert_eq!(dry_account(&c, 14), "A");
    // #683: A capped again, a real spawn signs in with B's (default) folder, never a token.
    let d = json!({"account": "A", "agent": "sendmeter-impl-12", "until": c.now + 3600});
    append(&c.state, &Event::new(c.now, "sendmeter", Some(12), "account-capped", d)).unwrap();
    let (_, got) = spawn(&c, &sh, 15);
    let b = drover::config::home().join(".config/drover/accounts/B");
    assert_eq!(got, format!("{}|", b.display()), "B's default folder, no token");
    no_token_in_log(&c);
}

/// #683: what the pane's shell printed of the account env: `$CLAUDE_CONFIG_DIR|$CLAUDE_CODE_OAUTH_TOKEN`.
const SIGN_IN: &str = "CLAUDE_CONFIG_DIR|$CLAUDE_CODE_OAUTH_TOKEN";

/// #683: Claude Code's own login in folder `dir` (`.claude.json`'s `oauthAccount`).
fn sign_in(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(".claude.json"), r#"{"oauthAccount": {"emailAddress": "a@x"}}"#).unwrap();
}

#[test]
fn the_profile_hook_still_refuses_a_guarded_command_on_the_accounts_folder() {
    let d = tmpdir("acct-hook");
    let mut c = ctx(&d, &folders(&d, "account = \"A\""));
    c.exe = env!("CARGO_BIN_EXE_drover").into();
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let (_, got) = spawn_var(&c, &sh, 12, SIGN_IN);
    let a = d.join("accounts/A");
    assert_eq!(got, format!("{}|", a.display()));
    // The profile goes on top of the account's login.
    let profile = c.config.with_file_name("claude").join("fleet-impl");
    let typed = sh.sent("sendmeter-impl-12");
    let start = typed.iter().find(|l| l.starts_with("exec 'claude'")).unwrap();
    assert!(
        start.contains(&format!("'--settings' '{}'", profile.join("settings.json").display())),
        "{start}"
    );
    // That profile's `PreToolUse` hook, run as Claude Code runs it in the lane's env, refuses a guarded command.
    let s: Value = serde_json::from_str(&std::fs::read_to_string(profile.join("settings.json")).unwrap()).unwrap();
    let hook = s["hooks"]["PreToolUse"][0]["hooks"][0]["command"].as_str().unwrap();
    let wt = c.worktrees.join("sendmeter/issue-12");
    let call = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": "git push origin staging"}, "cwd": wt});
    let mut ch = std::process::Command::new("sh")
        .args(["-c", hook])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap())
        .env("DROVER_LANE", "sendmeter:12:impl")
        .env("CLAUDE_CONFIG_DIR", &a)
        .env("FLEET_CONFIG", &c.config)
        .env("FLEET_STATE", &c.state)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write as _;
    ch.stdin.take().unwrap().write_all(call.to_string().as_bytes()).unwrap();
    let o = String::from_utf8(ch.wait_with_output().unwrap().stdout).unwrap();
    let v: Value = serde_json::from_str(&o).unwrap_or_else(|_| panic!("{o}"));
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny", "{o}");
}

/// `config` with `account_env` set to a fake helper (`fake-cooee`, answered by the test's runner).
fn helper_config(d: &Path) -> String {
    config(d, "account = \"A\"").replace(
        "[accounts]\n",
        "[accounts]\naccount_env = \"fake-cooee accounts env {provider} {name}\"\n",
    )
}

#[test]
fn a_helpers_json_env_reaches_the_lane_and_nothing_else_signs_it_in() {
    let d = tmpdir("acct-helper");
    let c = ctx(&d, &helper_config(&d));
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let env = r#"{"CLAUDE_CONFIG_DIR": "/cooee/accounts/claude/A", "COOEE_ACCOUNT": "it's A"}"#;
    let f = |r: Fake| r.reply("fake-cooee accounts env claude A", env);
    let (r, got) = spawn_with(
        &c,
        &sh,
        12,
        "CLAUDE_CONFIG_DIR|$COOEE_ACCOUNT|$CLAUDE_CODE_OAUTH_TOKEN",
        f,
    );
    assert_eq!(
        got, "/cooee/accounts/claude/A|it's A|",
        "the helper's env exactly, no token"
    );
    assert_eq!(r.count("fake-cooee accounts env claude A"), 1);
}

#[test]
fn a_failing_or_non_json_helper_refuses_the_spawn_with_its_stderr() {
    let d = tmpdir("acct-helper-bad");
    let c = ctx(&d, &helper_config(&d));
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let fails = [
        Fake::default().once_err("fake-cooee", 2, "cooee: no account claude/A"),
        Fake::default().reply("fake-cooee", "CLAUDE_CONFIG_DIR=/x"),
        Fake::default().reply("fake-cooee", r#"{"BAD KEY; rm -rf /": "x"}"#),
    ];
    for (i, r) in fails.into_iter().enumerate() {
        let r = r.reply("which shed", "/opt/shed/bin/shed\n").reply("git -C", "");
        let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl"]);
        assert_eq!(code, 3, "{i}: {out}");
        assert!(
            out.contains("REFUSED ACCOUNT_ENV: account A: `fake-cooee accounts env claude A`"),
            "{i}: {out}"
        );
        assert!(
            i != 0 || out.contains("exited 2") && out.contains("cooee: no account claude/A"),
            "{out}"
        );
        assert!(
            !r.calls.borrow().iter().any(|l| l.contains(" worktree add ")) && !sh.methods().iter().any(|m| m == "run"),
            "{i}: {:?} {:?}",
            r.calls.borrow(),
            sh.methods()
        );
    }
}

#[test]
fn doctor_shows_one_account_source_row_and_accounts_ask_each_folders_cli_never_the_keychain() {
    let d = tmpdir("acct-doctor");
    let c = ctx(&d, &folders(&d, "account = \"A\""));
    let (a, b) = (d.join("accounts/A"), d.join("accounts/B"));
    sign_in(&a);
    std::fs::create_dir_all(&b).unwrap(); // B's folder, signed out
                                          // #707: no secret store at all; #785: each folder's own CLI says whether it is signed in.
    let r = Fake::default()
        .reply(&auth_status(&d, "A"), r#"{"loggedIn": true}"#)
        .reply(&auth_status(&d, "B"), r#"{"loggedIn": false}"#);
    let out = run(&c, &r, &["doctor"]).1;
    // #707 (q855): one account-source row; the per-account rows are retired.
    let src: Vec<&str> = out.lines().filter(|l| l.contains(" account")).collect();
    assert_eq!(
        src,
        ["✓ accounts — sign in from each account's own folder (`/login` in it, docs/accounts.md)"],
        "{out}"
    );
    let helper = c.config.with_file_name("helper.toml");
    let text = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(
        &helper,
        text.replace(
            "[accounts]\n",
            "[accounts]\naccount_env = \"cooee accounts env {provider} {name}\"\n",
        ),
    )
    .unwrap();
    let h = drover::Ctx {
        config: helper,
        ..c.clone()
    };
    let out_h = run(&h, &r, &["doctor"]).1;
    assert!(
        out_h.contains("✓ accounts — sign in from the account_env helper `cooee accounts env {provider} {name}`"),
        "{out_h}"
    );
    assert_eq!(
        run(&c, &r, &["doctor", "--tokens"]).0,
        2,
        "`doctor --tokens` is retired"
    );
    let list = run(&c, &r, &["accounts"]).1;
    assert!(list.contains(&format!("signed in ({})\n", a.display())), "{list}");
    assert!(
        list.contains(&format!("✗ signed in ({}) → cooee", b.display())),
        "{list}"
    );
    assert!(!out.contains("Keychain") && !out.contains("partition-list"), "{out}");
    let calls = r.calls.borrow();
    assert!(!calls.iter().any(|l| l.starts_with("security ")), "{calls:?}");
    assert!(calls.contains(&auth_status(&d, "B")), "{calls:?}");
}

#[test]
fn cap_expires_after_cap_reset_minutes() {
    let d = tmpdir("acct-expire");
    let mut c = ctx(
        &d,
        &config(&d, "accounts = [\"A\", \"B\"]").replace("[accounts]\n", "[accounts]\ncap_reset_minutes = 10\n"),
    );
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, &sh, 12);
    let wt = c.worktrees.join("sendmeter/issue-12");
    sh.lane("sendmeter-impl-12", "idle", wt.to_str().unwrap());
    sh.screen("sendmeter-impl-12", "Claude usage limit reached.");
    let r = Fake::default();
    assert!(run(&c, &r, &["check"]).1.contains("account-capped"));
    assert_eq!(dry_account(&c, 13), "B");
    c.now += 601;
    assert_eq!(dry_account(&c, 13), "A");
}

fn cap(c: &drover::Ctx, sh: &FakeShed, banner: &str) {
    assert!(check_shows(c, sh, banner).contains("account-capped"));
}

#[test]
fn cap_until_parses_the_banner_reset_in_local_time() {
    let cfg = drover::config::Config::parse("").unwrap();
    let bkk = Fake::default().reply("date -r", "+0700\n");
    let now = 1_793_502_000; // 2026-11-01 10:00 +07
    let until = |s: &str| drover::access::cap_until(&cfg, &bkk, s, now);
    // The banner shapes `cap_pattern` matches.
    assert_eq!(
        until("  ⎿  You've hit your limit · resets 3pm (Asia/Bangkok)"),
        1_793_520_000
    );
    assert_eq!(
        until("5-hour limit reached ∙ resets 9am"),
        1_793_584_800,
        "9am already passed: tomorrow"
    );
    assert_eq!(
        until("Weekly limit reached ∙ resets Nov 4, 8am (Asia/Bangkok)"),
        1_793_754_000
    );
    assert_eq!(
        until("Claude usage limit reached. Your limit will reset at 3:30pm."),
        1_793_521_800
    );
    // Unparseable: cap_reset_minutes (300) for a session limit, a day for a weekly one.
    assert_eq!(until("Claude usage limit reached."), now + 300 * 60);
    assert_eq!(until("Weekly limit reached"), now + 86400);
}

#[test]
fn capped_account_is_used_again_after_its_banner_reset() {
    let d = tmpdir("acct-until");
    let mut c = ctx(&d, &config(&d, "accounts = [\"A\", \"B\"]"));
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, &sh, 12);
    // now = 1_000_000 (1970-01-12 13:46:40; `date` unscripted: UTC): 3pm is 1_004_400.
    cap(&c, &sh, "You've hit your limit · resets 3pm (Asia/Bangkok)");
    let ev = read(&c.state).unwrap();
    assert_eq!(ev.last().unwrap().data["until"], 1_004_400);
    assert_eq!(dry_account(&c, 13), "B");
    c.now = 1_004_399;
    assert_eq!(dry_account(&c, 13), "B");
    c.now = 1_004_400;
    assert_eq!(dry_account(&c, 13), "A");
}

#[test]
fn every_account_capped_refuses_all_capped() {
    let d = tmpdir("acct-all");
    let c = ctx(&d, &config(&d, "accounts = [\"A\"]"));
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, &sh, 12);
    cap(&c, &sh, "You've hit your limit · resets 3pm");
    let r = Fake::default().reply("which shed", "/opt/shed/bin/shed\n");
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "13", "impl", "--dry-run"]);
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("REFUSED ALL_CAPPED") && out.contains("[\"A\"] is capped; the first resets in 73m"),
        "{out}"
    );
}

#[test]
fn capped_lane_waits_on_drive_to_respawn_on_a_free_account() {
    let d = tmpdir("acct-respawn");
    let c = ctx(&d, &config(&d, "accounts = [\"A\", \"B\"]"));
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, &sh, 12);
    cap(&c, &sh, "You've hit your limit · resets 3pm");
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let agents = [serde_json::json!({"name": "sendmeter-impl-12", "agent_status": "idle"})];
    let ev = read(&c.state).unwrap();
    let waits = drover::waiting::scan(&c, &cfg, &Fake::default(), &ev, &agents, false);
    let w = &waits.iter().find(|(s, _)| s.issue == 12).unwrap().1;
    assert_eq!(w.who, "drive", "{w:?}");
    let cmd = w.next_cmd.clone().unwrap();
    assert_eq!(cmd, "drover spawn sendmeter 12 impl --supersede");
    // That respawn (as drive runs it) picks the free account.
    let mut args: Vec<&str> = cmd.split_whitespace().skip(1).collect();
    args.extend(["--dry-run", "--json"]);
    let (code, out) = run(&c, &Fake::default().reply("which shed", "/opt/shed/bin/shed\n"), &args);
    assert_eq!(code, 0, "{out}");
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["account"], "B");
}

#[test]
fn capped_ready_lane_is_not_told_to_respawn() {
    let d = tmpdir("acct-ready");
    let c = ctx(&d, &config(&d, "accounts = [\"A\", \"B\"]"));
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, &sh, 12);
    cap(&c, &sh, "You've hit your limit · resets 3pm");
    let pass = json!({"verdict": "PASS", "sha": "1".repeat(40)});
    append(&c.state, &Event::new(c.now, "sendmeter", Some(12), "verdict", pass)).unwrap();
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let agents = [json!({"name": "sendmeter-impl-12", "agent_status": "idle"})];
    let ev = read(&c.state).unwrap();
    let waits = drover::waiting::scan(&c, &cfg, &Fake::default(), &ev, &agents, false);
    let w = &waits.iter().find(|(s, _)| s.issue == 12).unwrap().1;
    assert!(
        !w.next_cmd.as_deref().unwrap_or_default().contains("--supersede"),
        "{w:?}"
    );
    assert!(!w.what.contains("account capped"), "{w:?}");
}

#[test]
fn mcp_allowlist_is_the_only_mcp_and_prod_is_refused() {
    let d = tmpdir("mcp");
    let c = ctx(&d, &config(&d, "mcp = [\"supabase-dev\"]"));
    let sh = FakeShed::attached(&c);
    let (code, out) = run(
        &c,
        &Fake::default().reply("which shed", "/opt/shed/bin/shed\n"),
        &["spawn", "sendmeter", "12", "impl", "--dry-run", "--json"],
    );
    assert_eq!(code, 0, "{out}");
    let p: Value = serde_json::from_str(&out).unwrap();
    let start = &p["commands"][4];
    assert_eq!(
        (start[0].as_str(), start[1].as_str()),
        (Some("shed"), Some("send")),
        "{start}"
    );
    let typed: Value = serde_json::from_str(start[2].as_str().unwrap()).unwrap();
    let typed = typed["text"].as_str().unwrap();
    let file = c.worktrees.join("sendmeter/issue-12/.mcp.lane.json");
    assert!(typed.starts_with("exec 'claude'"), "{typed}");
    assert!(
        typed.contains(&format!("'--mcp-config' '{}' '--strict-mcp-config'", file.display())),
        "{typed}"
    );

    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, &sh, 12);
    let lane: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let names: Vec<&String> = lane["mcpServers"].as_object().unwrap().keys().collect();
    assert_eq!(names, ["supabase-dev"]);
    assert_eq!(lane["mcpServers"]["supabase-dev"]["args"][0], "dev");

    // Default: no MCP at all, still strict.
    let d2 = tmpdir("mcp-none");
    let c2 = ctx(&d2, &config(&d2, ""));
    let sh2 = FakeShed::attached(&c2);
    assert_eq!(run(&c2, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c2, &sh2, 12);
    let lane: Value =
        serde_json::from_str(&std::fs::read_to_string(c2.worktrees.join("sendmeter/issue-12/.mcp.lane.json")).unwrap())
            .unwrap();
    assert_eq!(lane["mcpServers"], serde_json::json!({}));

    for bad in ["supabase-prod", "Supabase-PROD-ro"] {
        let c = ctx(&d, &config(&d, &format!("mcp = [\"supabase-dev\", \"{bad}\"]")));
        let (code, out) = run(&c, &Fake::default(), &["spawn", "sendmeter", "13", "impl", "--dry-run"]);
        assert_eq!(code, 3, "{out}");
        assert!(out.starts_with("REFUSED MCP_FORBIDDEN"), "{out}");
        let (code, out) = run(&c, &Fake::default(), &["profiles", "sync"]);
        assert_eq!((code, out.contains("MCP_FORBIDDEN")), (3, true), "{out}");
    }
}

#[test]
fn hermes_profile_gets_only_the_listed_mcp_servers() {
    let d = tmpdir("mcp-hermes");
    let cfg = config(&d, "mcp = [\"supabase-dev\"]").replace("harness = \"claude\"\nmcp", "harness = \"hermes\"\nmcp");
    let c = ctx(&d, &cfg);
    let yaml = c.homes["hermes"].join("fleet-impl/config.yaml");
    std::fs::create_dir_all(yaml.parent().unwrap()).unwrap();
    std::fs::write(
        &yaml,
        "model:\n  default: opus\nmcp_servers:\n  supabase-prod:\n    command: npx\nagent:\n  max_turns: 90\n",
    )
    .unwrap();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let text = std::fs::read_to_string(&yaml).unwrap();
    assert!(
        text.contains("mcp_servers:\n  \"supabase-dev\": {\"args\":[\"dev\"],\"command\":\"npx\"}\n"),
        "{text}"
    );
    assert!(
        !text.contains("prod") && text.contains("agent:\n  max_turns: 90"),
        "{text}"
    );
}

fn check_shows(c: &drover::Ctx, sh: &FakeShed, screen: &str) -> String {
    let wt = c.worktrees.join("sendmeter/issue-12");
    sh.lane("sendmeter-impl-12", "idle", wt.to_str().unwrap());
    sh.screen("sendmeter-impl-12", screen);
    let (code, out) = run(c, &Fake::default(), &["check"]);
    assert_eq!(code, 0, "{out}");
    out
}

/// #640: account A's latest status-line reading (`usage.jsonl`), read `ago` seconds before now.
fn reading(c: &drover::Ctx, five: Value, seven: Value, ago: u64) {
    let row = json!({"ts": c.now - ago, "account": "A", "five_hour_pct": five, "five_hour_reset": null,
        "seven_day_pct": seven, "seven_day_reset": 1_200_000});
    std::fs::write(c.state.with_file_name("usage.jsonl"), format!("{row}\n")).unwrap();
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

#[test]
fn a_banner_the_meter_contradicts_does_not_cap_and_a_lane_still_picks_the_account() {
    let d = tmpdir("acct-meter-ok");
    let c = ctx(&d, &config(&d, "accounts = [\"A\", \"B\"]"));
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, &sh, 12);
    reading(&c, Value::Null, json!(23.0), 60);
    let out = check_shows(&c, &sh, "  ⎿  You've hit your limit · resets 3pm (Asia/Bangkok)\n");
    assert!(!out.contains("account-capped"), "{out}");
    let ignored = kinds(&c, "cap-banner-ignored");
    assert_eq!(ignored.len(), 1, "{out}");
    assert!(
        ignored[0].str("reason").unwrap().contains("5h —, 7d 23%"),
        "{ignored:?}"
    );
    assert_eq!(dry_account(&c, 13), "A");
    let (_, list) = run(&c, &Fake::default(), &["accounts"]);
    assert!(
        list.lines()
            .any(|l| l.starts_with("A ") && l.contains("ok: 5h —, 7d 23%")),
        "{list}"
    );
}

#[test]
fn a_banner_with_the_week_at_100_caps_until_the_reset() {
    let d = tmpdir("acct-meter-full");
    let c = ctx(&d, &config(&d, "accounts = [\"A\", \"B\"]"));
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, &sh, 12);
    reading(&c, Value::Null, json!(100.0), 60);
    let out = check_shows(&c, &sh, "  ⎿  You've hit your limit · resets 3pm (Asia/Bangkok)\n");
    assert!(
        out.contains("account-capped: A:") && !out.contains("signed out"),
        "{out}"
    );
    let capped = kinds(&c, "account-capped");
    assert_eq!(capped[0].data["until"], 1_200_000, "the week's reset");
    assert!(kinds(&c, "account-signed-out").is_empty());
    assert_eq!(dry_account(&c, 13), "B");
    let (_, list) = run(&c, &Fake::default(), &["accounts"]);
    let a = list.lines().find(|l| l.starts_with("A ")).unwrap();
    assert!(
        a.contains("capped until ") && a.contains("banner ") && a.contains("7d 100%"),
        "{list}"
    );
    assert!(!a.contains("signed out"), "{list}");
}

#[test]
fn a_banner_with_no_fresh_reading_still_caps() {
    let d = tmpdir("acct-meter-stale");
    let c = ctx(&d, &config(&d, "accounts = [\"A\", \"B\"]"));
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, &sh, 12);
    reading(&c, Value::Null, json!(23.0), 601);
    assert!(check_shows(&c, &sh, "You've hit your limit · resets 3pm").contains("account-capped: A:"));
}

#[test]
fn the_pattern_in_a_reply_or_tool_output_is_no_banner() {
    let d = tmpdir("acct-banner-src");
    let c = ctx(&d, &config(&d, "accounts = [\"A\", \"B\"]"));
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, &sh, 12);
    for screen in [
        "⏺ Bash(rg -n 'hit your limit' crates)\n  ⎿  claude.rs:19: usage limit reached|hit your limit\n",
        "⏺ The pane said \"You've hit your limit · resets 3pm\", so I matched it.\n",
        "❯ why did it say usage limit reached?\n",
    ] {
        assert!(!check_shows(&c, &sh, screen).contains("account-capped"), "{screen}");
    }
    assert_eq!(dry_account(&c, 13), "A");
}

#[test]
fn signed_out_is_its_own_state_never_a_cap() {
    let d = tmpdir("acct-signed-out");
    let c = ctx(&d, &config(&d, "accounts = [\"A\", \"B\"]"));
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, &sh, 12);
    let out = check_shows(&c, &sh, "  ⎿  API Error: 401 · Please run /login\n");
    assert!(
        out.contains("account-signed-out") && !out.contains("account-capped"),
        "{out}"
    );
    let s = kinds(&c, "account-signed-out");
    assert!(
        s[0].data.get("until").is_none_or(Value::is_null),
        "no reset time: {s:?}"
    );
    assert!(s[0].str("reason").unwrap().contains("sign in"), "{s:?}");
    assert_eq!(dry_account(&c, 13), "B");
    let (_, list) = run(&c, &Fake::default(), &["accounts"]);
    let a = list.lines().find(|l| l.starts_with("A ")).unwrap();
    assert!(a.contains("signed out: ") && !a.contains("capped"), "{list}");
    assert_eq!(run(&c, &Fake::default(), &["accounts", "reset", "A"]).0, 0);
    assert_eq!(dry_account(&c, 13), "A");
}

/// #707: `config` with account A's hand-placed `token-file:` at `<dir>/tokens/A` (mode `mode`).
fn token_file(d: &Path, mode: u32) -> (String, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let f = d.join("tokens/A");
    std::fs::create_dir_all(f.parent().unwrap()).unwrap();
    std::fs::write(&f, format!("{TOKEN_A}\n")).unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(mode)).unwrap();
    let cfg = config(d, "account = \"A\"").replace(
        "A = \"CLAUDE_CODE_OAUTH_TOKEN_A\"",
        &format!("A = \"token-file:{}\"", f.display()),
    );
    (cfg, f)
}

#[test]
fn a_token_file_account_reaches_only_its_lanes_shell_as_the_oauth_token() {
    let d = tmpdir("acct-token-file");
    let (cfg, _) = token_file(&d, 0o600);
    let c = ctx(&d, &cfg);
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    keep_secrets(&c, 12);
    let (r, got) = spawn(&c, &sh, 12);
    assert_eq!(got, format!("|{TOKEN_A}"));
    let calls = lines(&sh, &r, 12);
    assert!(calls.iter().any(|l| l.contains("&& . '")), "{calls:?}");
    assert!(
        !calls.iter().any(|l| l.contains(TOKEN_A)),
        "token in argv or the pane: {calls:?}"
    );
    no_token_in_log(&c);
    let (_, out) = run(&c, &Fake::default(), &["doctor"]);
    assert!(
        out.contains("✓ accounts — sign in from each account's own folder"),
        "{out}"
    );
    assert!(out.contains("✓ account A token file — "), "{out}");
    assert!(!out.contains(TOKEN_A), "{out}");
}

#[test]
fn a_token_file_others_can_read_refuses_the_spawn() {
    let d = tmpdir("acct-token-0644");
    let (cfg, f) = token_file(&d, 0o644);
    let c = ctx(&d, &cfg);
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let r = Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply(&format!("git -C {} rev-list", wt.display()), "0\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "");
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains(&format!(
            "REFUSED SECRET_FILE_OPEN: {} is readable by others",
            f.display()
        )),
        "{out}"
    );
    let started = |m: &String| ["run", "send", "keys"].contains(&m.as_str());
    assert!(!sh.methods().iter().any(started), "no lane started: {:?}", sh.methods());
    assert!(!out.contains(TOKEN_A), "{out}");
    let (_, out) = run(&c, &Fake::default(), &["doctor"]);
    assert!(
        out.contains("! account A token file — REFUSED SECRET_FILE_OPEN"),
        "{out}"
    );
}

/// #785: Claude Code's own answer for folder `<d>/accounts/<name>`: `claude auth status` there, the env's token unset.
fn auth_status(d: &Path, name: &str) -> String {
    let dir = d.join("accounts").join(name);
    format!(
        "env -u CLAUDE_CODE_OAUTH_TOKEN CLAUDE_CONFIG_DIR={} claude auth status",
        dir.display()
    )
}

#[test]
fn a_signed_out_folder_account_is_skipped_with_one_alert_in_standalone() {
    let d = tmpdir("acct-785-folder");
    let c = ctx(&d, &folders(&d, "accounts = [\"A\", \"B\"]"));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    // A's folder holds a stale login file, but its CLI says it is signed out; B's says signed in.
    sign_in(&d.join("accounts/A"));
    let sh = FakeShed::attached(&c);
    let status = |r: Fake| {
        r.reply(&auth_status(&d, "A"), r#"{"loggedIn": false}"#)
            .reply(&auth_status(&d, "B"), r#"{"loggedIn": true}"#)
    };
    let (r, got) = spawn_with(&c, &sh, 12, SIGN_IN, status);
    assert!(r.called(&auth_status(&d, "A")), "{:?}", r.calls.borrow());
    assert_eq!(got, format!("{}/accounts/B|", d.display()), "the lane is on B");
    assert_eq!(kinds(&c, "spawn")[0].str("account"), Some("B"));
    let out = kinds(&c, "account-signed-out");
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(out[0].str("account"), Some("A"));
    assert!(
        out[0].str("reason").unwrap().contains("claude /login"),
        "names the sign-in: {out:?}"
    );
    // The next spawn skips A without asking again: still one alert.
    let (r, got) = spawn_with(&c, &sh, 13, SIGN_IN, status);
    assert_eq!(got, format!("{}/accounts/B|", d.display()));
    assert!(!r.called(&auth_status(&d, "A")));
    assert_eq!(kinds(&c, "account-signed-out").len(), 1);
}

#[test]
fn an_account_env_exit_3_is_signed_out_and_skipped() {
    let d = tmpdir("acct-785-helper");
    let cfg = config(&d, "accounts = [\"A\", \"B\"]").replace(
        "[accounts]\n",
        "[accounts]\naccount_env = \"fake-cooee accounts env {provider} {name} --require-signed-in\"\n",
    );
    let c = ctx(&d, &cfg);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let sh = FakeShed::attached(&c);
    let f = |r: Fake| {
        r.once_err("fake-cooee accounts env claude A", 3, "cooee: claude/A is signed out")
            .reply(
                "fake-cooee accounts env claude B",
                r#"{"CLAUDE_CONFIG_DIR": "/cooee/B"}"#,
            )
    };
    let (_, got) = spawn_with(&c, &sh, 12, SIGN_IN, f);
    assert_eq!(got, "/cooee/B|", "the lane is on B");
    let out = kinds(&c, "account-signed-out");
    assert_eq!((out.len(), out[0].str("account")), (1, Some("A")), "{out:?}");
    assert!(out[0].str("reason").unwrap().contains("signed out"), "{out:?}");
}

#[test]
fn accounts_lists_live_lanes_only() {
    let d = tmpdir("acct-785-live");
    let c = ctx(&d, &config(&d, "accounts = [\"A\", \"B\"]"));
    let ev = |issue: Option<u64>, kind: &str, d: Value| {
        append(&c.state, &Event::new(c.now - 100, "sendmeter", issue, kind, d)).unwrap();
    };
    for (n, agent) in [
        (12, "sendmeter-impl-12"),
        (13, "sendmeter-impl-13"),
        (14, "sendmeter-impl-14"),
    ] {
        ev(
            Some(n),
            "spawn",
            json!({"agent": agent, "role": "impl", "account": "A"}),
        );
    }
    ev(
        None,
        "spawn",
        json!({"agent": "conductor-t1", "role": "conductor", "account": "A"}),
    );
    // 13's lane died, 14's ACP session is gone, the conductor's was superseded: only 12 still runs.
    ev(
        Some(13),
        "lane-dead",
        json!({"key": "1:sendmeter-impl-13", "reason": "gone"}),
    );
    ev(
        Some(14),
        "lane-state",
        json!({"agent": "sendmeter-impl-14", "state": "gone", "cause": "killed"}),
    );
    ev(
        None,
        "supersede",
        json!({"agent": "conductor-t1", "killed": ["conductor-t1"]}),
    );
    let (code, list) = run(&c, &Fake::default(), &["accounts"]);
    assert_eq!(code, 0, "{list}");
    let a = list.lines().find(|l| l.starts_with("A ")).unwrap();
    assert!(a.ends_with("1 lanes sendmeter-impl-12"), "{list}");
}
