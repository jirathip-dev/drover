//! #5: per-role Anthropic accounts with cap fallback; per-role MCP allowlist.
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use herdsman::runner::Runner;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const TOKEN_A: &str = concat!("sk", "-ant-oat01-AAAA-secret");
const TOKEN_B: &str = concat!("sk", "-ant-oat01-BBBB-secret");

/// Claude impl role with `role_extra` lines, `[accounts]` A/B/C (#683: old tokens still in the store, `store`, never
/// read), local repo `<dir>/repo`.
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

/// #441: the accounts' old tokens in the fake store (#683: no lane gets them).
fn store(f: Fake) -> Fake {
    let svc = |v: &str| format!("herdsman/all/CLAUDE_CODE_OAUTH_TOKEN_{v}");
    f.stored(&svc("A"), TOKEN_A)
        .stored(&svc("B"), TOKEN_B)
        .stored(&svc("C"), "c")
}

/// A real (faked herdr/gh/git) impl spawn of issue `n` in pane `w9:p<n>`; its pane's shell's `SIGN_IN`.
fn spawn(c: &herdsman::Ctx, n: u64) -> (Fake, String) {
    spawn_var(c, n, SIGN_IN)
}

/// As `spawn`; `var` is what its pane's shell printed of `"$<var>"`.
fn spawn_var(c: &herdsman::Ctx, n: u64, var: &'static str) -> (Fake, String) {
    spawn_with(c, n, var, |r| r)
}

/// As `spawn_var`, its runner given more replies by `more`.
fn spawn_with(c: &herdsman::Ctx, n: u64, var: &'static str, more: impl Fn(Fake) -> Fake) -> (Fake, String) {
    let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
    std::fs::create_dir_all(&wt).unwrap();
    let pane = format!("w9:p{n}");
    let r = more(store(Fake::default()))
        .reply("herdr agent list", NO_AGENTS)
        .reply(&format!("git -C {} rev-list", wt.display()), "0\n")
        .reply(&format!("gh issue view {n}"), r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "")
        .reply(
            "herdr worktree open",
            r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#,
        )
        .reply(
            "herdr pane list --workspace w9",
            &format!(r#"{{"result":{{"panes":[{{"pane_id":"{pane}","agent":null}}]}}}}"#),
        )
        .reply(&format!("herdr pane run {pane} export"), "{}")
        .reply("herdr agent start", "{}")
        .reply("herdr agent wait", "{}");
    let sh = Shell(&r, var, Shared::default());
    let (code, out) = run(c, &sh, &["spawn", "sendmeter", &n.to_string(), "impl"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    assert!(
        !out.contains(TOKEN_A) && !out.contains(TOKEN_B),
        "token in stdout: {out}"
    );
    let token = sh.2.borrow().concat();
    assert!(
        !Path::new(&format!("{}.env", wt.display())).exists(),
        "the secrets file is left behind"
    );
    (r, token)
}

fn dry_account(c: &herdsman::Ctx, n: u64) -> Value {
    let (code, out) = run(
        c,
        &Fake::default(),
        &["spawn", "sendmeter", &n.to_string(), "impl", "--dry-run", "--json"],
    );
    assert_eq!(code, 0, "{out}");
    serde_json::from_str::<Value>(&out).unwrap()["account"].clone()
}

fn no_token_in_log(c: &herdsman::Ctx) {
    let log = herdsman::events::jsonl(&c.state).unwrap_or_default();
    assert!(
        !log.contains(TOKEN_A) && !log.contains(TOKEN_B),
        "token in events: {log}"
    );
}

#[test]
fn role_account_b_signs_in_with_its_folder_and_no_token_with_no_helper() {
    let d = tmpdir("acct-b");
    let c = ctx(&d, &folders(&d, "account = \"B\""));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let (r, got) = spawn(&c, 12);
    // #683: with no `account_env`, B's folder as CLAUDE_CONFIG_DIR; no token, no secrets file.
    let calls = r.calls.borrow();
    assert!(!calls.iter().any(|l| l.contains(TOKEN_B)), "token passed through argv");
    assert_eq!(got, format!("{}/accounts/B|", d.display()));
    assert!(!calls.iter().any(|l| l.contains("&& . '")), "a secrets file: {calls:?}");
    assert!(r.index("herdr pane run w9:p12 export").unwrap() < r.index("herdr agent start").unwrap());
    let ev = read(&c.state).unwrap();
    assert_eq!(ev.last().unwrap().str("account"), Some("B"));
    no_token_in_log(&c);

    // The binary's stdout and stderr (dry run, `accounts`) never show a token either.
    for args in [
        &["spawn", "sendmeter", "13", "impl", "--dry-run", "--json"][..],
        &["accounts"],
    ] {
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_herdsman"))
            .args(args)
            .env("FLEET_CONFIG", &c.config)
            .env("FLEET_STATE", &c.state)
            .env("FLEET_WORKTREES", &c.worktrees)
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
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, 12);
    assert_eq!(read(&c.state).unwrap().last().unwrap().str("account"), Some("A"));

    let wt = c.worktrees.join("sendmeter/issue-12");
    let r = store(Fake::default())
        .reply(
            "herdr agent list",
            &agents(&[("sendmeter-impl-12", "idle", wt.to_str().unwrap(), "w9:p12")]),
        )
        .reply(
            "herdr pane read w9:p12",
            "  ⎿  You've hit your limit · resets 3pm (Asia/Bangkok)\n",
        );
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("account-capped: A:"), "{out}");
    let capped: Vec<_> = read(&c.state)
        .unwrap()
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
    let (_, got) = spawn(&c, 15);
    let b = herdsman::config::home().join(".config/herdsman/accounts/B");
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
    c.exe = env!("CARGO_BIN_EXE_herdsman").into();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let (r, got) = spawn_var(&c, 12, SIGN_IN);
    let a = d.join("accounts/A");
    assert_eq!(got, format!("{}|", a.display()));
    // The profile goes on top of the account's login.
    let profile = c.config.with_file_name("claude").join("fleet-impl");
    let calls = r.calls.borrow();
    let start = calls.iter().find(|l| l.starts_with("herdr agent start")).unwrap();
    assert!(
        start.contains(&format!("--settings {}", profile.join("settings.json").display())),
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
        .env("HERDSMAN_LANE", "sendmeter:12:impl")
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
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let env = r#"{"CLAUDE_CONFIG_DIR": "/cooee/accounts/claude/A", "COOEE_ACCOUNT": "it's A"}"#;
    let f = |r: Fake| r.reply("fake-cooee accounts env claude A", env);
    let (r, got) = spawn_with(&c, 12, "CLAUDE_CONFIG_DIR|$COOEE_ACCOUNT|$CLAUDE_CODE_OAUTH_TOKEN", f);
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
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let fails = [
        Fake::default().once_err("fake-cooee", 2, "cooee: no account claude/A"),
        Fake::default().reply("fake-cooee", "CLAUDE_CONFIG_DIR=/x"),
        Fake::default().reply("fake-cooee", r#"{"BAD KEY; rm -rf /": "x"}"#),
    ];
    for (i, r) in fails.into_iter().enumerate() {
        let r = store(r).reply("herdr agent list", NO_AGENTS).reply("git -C", "");
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
            !r.called("herdr worktree open") && !r.called("herdr agent start"),
            "{i}: {:?}",
            r.calls.borrow()
        );
    }
}

#[test]
fn doctor_and_accounts_read_each_folders_sign_in_from_its_files_never_the_keychain_or_the_cli() {
    let d = tmpdir("acct-doctor");
    let c = ctx(&d, &folders(&d, "account = \"A\""));
    let (a, b) = (d.join("accounts/A"), d.join("accounts/B"));
    sign_in(&a);
    std::fs::create_dir_all(&b).unwrap(); // B's folder, signed out
                                          // No usable store: an account row that read the Keychain would fail.
    let r = Fake {
        no_store: true,
        ..Fake::default()
    };
    let out = run(&c, &r, &["doctor"]).1;
    assert!(
        out.lines()
            .any(|l| l == format!("✓ account A — signed in ({})", a.display())),
        "{out}"
    );
    let fix = format!(
        "cooee Settings › Providers, or CLAUDE_CONFIG_DIR={} claude /login",
        b.display()
    );
    let want = format!("✗ account B — signed in ({}) → {fix}", b.display());
    assert!(out.lines().any(|l| l == want), "{out}");
    let (code, gate) = run(&c, &r, &["doctor", "--tokens"]);
    assert_eq!(code, 1, "B is signed out: {gate}");
    assert!(
        gate.contains(&format!("✓ account A — signed in ({})", a.display())),
        "{gate}"
    );
    let list = run(&c, &r, &["accounts"]).1;
    assert!(list.contains(&format!("signed in ({})\n", a.display())), "{list}");
    assert!(
        list.contains(&format!("✗ signed in ({}) → cooee", b.display())),
        "{list}"
    );
    assert!(!out.contains("Keychain") && !out.contains("partition-list"), "{out}");
    let calls = r.calls.borrow();
    assert!(
        !calls.iter().any(|l| l.contains("auth status") && l.contains("claude")),
        "{calls:?}"
    );
}

#[test]
fn cap_expires_after_cap_reset_minutes() {
    let d = tmpdir("acct-expire");
    let mut c = ctx(
        &d,
        &config(&d, "accounts = [\"A\", \"B\"]").replace("[accounts]\n", "[accounts]\ncap_reset_minutes = 10\n"),
    );
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, 12);
    let wt = c.worktrees.join("sendmeter/issue-12");
    let r = store(Fake::default())
        .reply(
            "herdr agent list",
            &agents(&[("sendmeter-impl-12", "idle", wt.to_str().unwrap(), "w9:p12")]),
        )
        .reply("herdr pane read w9:p12", "Claude usage limit reached.");
    assert!(run(&c, &r, &["check"]).1.contains("account-capped"));
    assert_eq!(dry_account(&c, 13), "B");
    c.now += 601;
    assert_eq!(dry_account(&c, 13), "A");
}

/// Caps lane `sendmeter-impl-12` (pane `w9:p12`) with `banner` via `herdsman check`.
fn cap(c: &herdsman::Ctx, banner: &str) {
    let wt = c.worktrees.join("sendmeter/issue-12");
    let r = store(Fake::default())
        .reply(
            "herdr agent list",
            &agents(&[("sendmeter-impl-12", "idle", wt.to_str().unwrap(), "w9:p12")]),
        )
        .reply("herdr pane read w9:p12", banner);
    assert!(run(c, &r, &["check"]).1.contains("account-capped"));
}

#[test]
fn cap_until_parses_the_banner_reset_in_local_time() {
    let cfg = herdsman::config::Config::parse("").unwrap();
    let bkk = Fake::default().reply("date -r", "+0700\n");
    let now = 1_793_502_000; // 2026-11-01 10:00 +07
    let until = |s: &str| herdsman::access::cap_until(&cfg, &bkk, s, now);
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
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, 12);
    // now = 1_000_000 (1970-01-12 13:46:40; `date` unscripted: UTC): 3pm is 1_004_400.
    cap(&c, "You've hit your limit · resets 3pm (Asia/Bangkok)");
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
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, 12);
    cap(&c, "You've hit your limit · resets 3pm");
    let (code, out) = run(&c, &Fake::default(), &["spawn", "sendmeter", "13", "impl", "--dry-run"]);
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
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, 12);
    cap(&c, "You've hit your limit · resets 3pm");
    let cfg = herdsman::config::Config::load(&c.config).unwrap();
    let agents = [serde_json::json!({"name": "sendmeter-impl-12", "agent_status": "idle"})];
    let ev = read(&c.state).unwrap();
    let waits = herdsman::waiting::scan(&c, &cfg, &Fake::default(), &ev, &agents, false);
    let w = &waits.iter().find(|(s, _)| s.issue == 12).unwrap().1;
    assert_eq!(w.who, "drive", "{w:?}");
    let cmd = w.next_cmd.clone().unwrap();
    assert_eq!(cmd, "herdsman spawn sendmeter 12 impl --supersede");
    // That respawn (as drive runs it) picks the free account.
    let mut args: Vec<&str> = cmd.split_whitespace().skip(1).collect();
    args.extend(["--dry-run", "--json"]);
    let (code, out) = run(&c, &Fake::default(), &args);
    assert_eq!(code, 0, "{out}");
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["account"], "B");
}

#[test]
fn capped_ready_lane_is_not_told_to_respawn() {
    let d = tmpdir("acct-ready");
    let c = ctx(&d, &config(&d, "accounts = [\"A\", \"B\"]"));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, 12);
    cap(&c, "You've hit your limit · resets 3pm");
    let pass = json!({"verdict": "PASS", "sha": "1".repeat(40)});
    append(&c.state, &Event::new(c.now, "sendmeter", Some(12), "verdict", pass)).unwrap();
    let cfg = herdsman::config::Config::load(&c.config).unwrap();
    let agents = [json!({"name": "sendmeter-impl-12", "agent_status": "idle"})];
    let ev = read(&c.state).unwrap();
    let waits = herdsman::waiting::scan(&c, &cfg, &Fake::default(), &ev, &agents, false);
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
    let (code, out) = run(
        &c,
        &Fake::default(),
        &["spawn", "sendmeter", "12", "impl", "--dry-run", "--json"],
    );
    assert_eq!(code, 0, "{out}");
    let p: Value = serde_json::from_str(&out).unwrap();
    let start: Vec<&str> = p["commands"][3]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w.as_str().unwrap())
        .collect();
    let file = c.worktrees.join("sendmeter/issue-12/.mcp.lane.json");
    let i = start.iter().position(|w| *w == "--mcp-config").unwrap();
    assert_eq!(start[i + 1..i + 3], [file.to_str().unwrap(), "--strict-mcp-config"]);

    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, 12);
    let lane: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let names: Vec<&String> = lane["mcpServers"].as_object().unwrap().keys().collect();
    assert_eq!(names, ["supabase-dev"]);
    assert_eq!(lane["mcpServers"]["supabase-dev"]["args"][0], "dev");

    // Default: no MCP at all, still strict.
    let d2 = tmpdir("mcp-none");
    let c2 = ctx(&d2, &config(&d2, ""));
    assert_eq!(run(&c2, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c2, 12);
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
    let yaml = c.hermes.join("fleet-impl/config.yaml");
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

/// #640: lane `sendmeter-impl-12` on account A shows `screen`; `herdsman check`'s output.
fn check_shows(c: &herdsman::Ctx, screen: &str) -> String {
    let wt = c.worktrees.join("sendmeter/issue-12");
    let r = store(Fake::default())
        .reply(
            "herdr agent list",
            &agents(&[("sendmeter-impl-12", "idle", wt.to_str().unwrap(), "w9:p12")]),
        )
        .reply("herdr pane read w9:p12", screen);
    let (code, out) = run(c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    out
}

/// #640: account A's latest status-line reading (`usage.jsonl`), read `ago` seconds before now.
fn reading(c: &herdsman::Ctx, five: Value, seven: Value, ago: u64) {
    let row = json!({"ts": c.now - ago, "account": "A", "five_hour_pct": five, "five_hour_reset": null,
        "seven_day_pct": seven, "seven_day_reset": 1_200_000});
    std::fs::write(c.state.with_file_name("usage.jsonl"), format!("{row}\n")).unwrap();
}

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

#[test]
fn a_banner_the_meter_contradicts_does_not_cap_and_a_lane_still_picks_the_account() {
    let d = tmpdir("acct-meter-ok");
    let c = ctx(&d, &config(&d, "accounts = [\"A\", \"B\"]"));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, 12);
    reading(&c, Value::Null, json!(23.0), 60);
    let out = check_shows(&c, "  ⎿  You've hit your limit · resets 3pm (Asia/Bangkok)\n");
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
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, 12);
    reading(&c, Value::Null, json!(100.0), 60);
    let out = check_shows(&c, "  ⎿  You've hit your limit · resets 3pm (Asia/Bangkok)\n");
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
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, 12);
    reading(&c, Value::Null, json!(23.0), 601);
    assert!(check_shows(&c, "You've hit your limit · resets 3pm").contains("account-capped: A:"));
}

#[test]
fn the_pattern_in_a_reply_or_tool_output_is_no_banner() {
    let d = tmpdir("acct-banner-src");
    let c = ctx(&d, &config(&d, "accounts = [\"A\", \"B\"]"));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, 12);
    for screen in [
        "⏺ Bash(rg -n 'hit your limit' crates)\n  ⎿  claude.rs:19: usage limit reached|hit your limit\n",
        "⏺ The pane said \"You've hit your limit · resets 3pm\", so I matched it.\n",
        "❯ why did it say usage limit reached?\n",
    ] {
        assert!(!check_shows(&c, screen).contains("account-capped"), "{screen}");
    }
    assert_eq!(dry_account(&c, 13), "A");
}

#[test]
fn signed_out_is_its_own_state_never_a_cap() {
    let d = tmpdir("acct-signed-out");
    let c = ctx(&d, &config(&d, "accounts = [\"A\", \"B\"]"));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, 12);
    let out = check_shows(&c, "  ⎿  API Error: 401 · Please run /login\n");
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

#[test]
fn a_banner_with_a_401_usage_reading_is_signed_out() {
    let d = tmpdir("acct-meter-401");
    let cfg = config(&d, "accounts = [\"A\", \"B\"]") + "\n[accounts.usage]\nA = \"~/logins/A\"\n";
    let c = ctx(&d, &cfg);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, 12);
    let dir = c.state.parent().unwrap().join("usage");
    std::fs::create_dir_all(&dir).unwrap();
    let m = json!({"checked": c.now, "ts": c.now - 7200, "login_needed": true});
    std::fs::write(dir.join("A.json"), m.to_string()).unwrap();
    let out = check_shows(&c, "You've hit your limit · resets 3pm");
    assert!(
        out.contains("account-signed-out") && !out.contains("account-capped"),
        "{out}"
    );
    let (_, list) = run(&c, &Fake::default(), &["accounts"]);
    assert!(
        list.lines()
            .any(|l| l.starts_with("A ") && l.contains("signed out: usage 401 at")),
        "{list}"
    );
}
