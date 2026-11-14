mod common;
use common::*;
use drover::events::{read, Event};
use serde_json::Value;
use std::path::Path;

fn config(d: &Path, role_extra: &str) -> String {
    let repo = d.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let mcp = r#"{"mcpServers":{"supabase-dev":{"command":"npx","args":["dev"]},"supabase-prod":{"command":"npx","args":["prod"]},"other":{"url":"http://x"}}}"#;
    std::fs::write(repo.join(".mcp.json"), mcp).unwrap();
    CONFIG
        .replace(
            "harness  =  \"hermes\"    # toggled in tests",
            &format!("harness = \"claude\"\n{role_extra}"),
        )
        .replace("/tmp/does-not-matter/sendmeter", repo.to_str().unwrap())
}

fn shell(sh: &FakeShed, lane: &str, var: &str) -> String {
    let typed = sh.sent(lane);
    let line = typed.iter().find(|t| t.starts_with("export ")).expect("no export line");
    let line = format!("{}; printf %s \"${var}\"", line.trim_end_matches('\r'));
    let mut c = std::process::Command::new("sh");
    c.env_remove("CLAUDE_CODE_OAUTH_TOKEN")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("DROVER_ACCOUNT");
    String::from_utf8_lossy(&c.arg("-c").arg(line).output().unwrap().stdout).into()
}

fn spawn_run(c: &drover::Ctx, n: u64) -> (i32, String) {
    let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
    std::fs::create_dir_all(&wt).unwrap();
    let r = Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply(&format!("git -C {} rev-list", wt.display()), "0\n")
        .reply(&format!("gh issue view {n}"), r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "");
    run(c, &r, &["spawn", "sendmeter", &n.to_string(), "impl"])
}

fn spawn(c: &drover::Ctx, sh: &FakeShed, n: u64) {
    let (code, out) = spawn_run(c, n);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        sh.last("run")["name"],
        format!("sendmeter-impl-{n}"),
        "{:?}",
        sh.methods()
    );
}

#[test]
fn retired_account_keys_load_warn_once_and_the_lane_runs_on_the_hosts_login() {
    let d = tmpdir("acct-retired");
    let old = config(&d, "accounts = [\"A\", \"B\"]\nfallback = [\"codex\"]")
        + "\n[accounts]\nA = \"token-file:/nowhere/A\"\ncap_reset_minutes = 300\naccount_env = \"cooee accounts env {provider} {name}\"\n\n[accounts.usage]\nA = \"/nowhere\"\n";
    let c = ctx(&d, &old);
    let sh = FakeShed::attached(&c);
    let (code, out) = run(&c, &Fake::default(), &["check"]);
    assert_eq!(code, 0, "{out}");
    let warns: Vec<&str> = out.lines().filter(|l| l.contains("config-warning")).collect();
    assert_eq!(warns.len(), 1, "{out}");
    assert!(
        warns[0].contains("[accounts]")
            && warns[0].contains("fleet.sendmeter.role.impl.accounts")
            && warns[0].contains("fleet.sendmeter.role.impl.fallback"),
        "{out}"
    );
    assert!(
        !run(&c, &Fake::default(), &["check"]).1.contains("config-warning"),
        "once"
    );
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, &sh, 12);
    let lane = "sendmeter-impl-12";
    assert_eq!(shell(&sh, lane, "CLAUDE_CONFIG_DIR"), "", "the CLI's default folder");
    assert_eq!(shell(&sh, lane, "DROVER_ACCOUNT"), "");
    let sp = kinds(&c, "spawn");
    assert!(sp[0].val("account").is_null(), "{sp:?}");
}

#[test]
fn a_usage_limit_banner_alerts_once_and_holds_new_lanes_until_hosts_on() {
    let d = tmpdir("acct-banner-hold");
    let c = ctx(&d, &config(&d, ""));
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, &sh, 12);
    let banner = "  ⎿  You've hit your limit · resets 3pm (Asia/Bangkok)\n";
    let out = check_shows(&c, &sh, banner);
    let hit = kinds(&c, "usage-limit");
    assert_eq!(hit.len(), 1, "{out}");
    let why = hit[0].str("reason").unwrap();
    assert!(
        why.contains("sendmeter-impl-12") && why.contains("claude /login") && why.contains("hosts on main"),
        "{why}"
    );
    check_shows(&c, &sh, banner);
    assert_eq!(kinds(&c, "usage-limit").len(), 1, "one alert per hold");
    let (code, out) = spawn_run(&c, 13);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("LOGIN_NEEDED") && out.contains("claude /login"), "{out}");
    let log = drover::events::jsonl(&c.state).unwrap_or_default();
    assert!(!log.contains("until") && !log.contains("account"), "no rest: {log}");
    let help = run(&c, &Fake::default(), &["help", "--agent"]).1;
    assert!(!help.contains("drover accounts"), "no accounts verb: {help}");

    let (code, out) = run(&c, &Fake::default(), &["hosts", "on", "main"]);
    assert_eq!(code, 0, "{out}");
    spawn(&c, &sh, 13);
    check_shows(&c, &sh, banner);
    assert_eq!(kinds(&c, "usage-limit").len(), 2, "a new hold after the owner acted");
}

#[test]
fn a_sign_in_prompt_is_reported_as_signed_out_and_holds_the_host() {
    let d = tmpdir("acct-signed-out");
    let c = ctx(&d, &config(&d, ""));
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, &sh, 12);
    let out = check_shows(&c, &sh, "  ⎿  API Error: 401 · Please run /login\n");
    assert!(kinds(&c, "usage-limit").is_empty(), "{out}");
    let s = kinds(&c, "signed-out");
    assert_eq!(s.len(), 1, "{out}");
    assert!(s[0].str("reason").unwrap().contains("is signed out on main"), "{s:?}");
    let (code, out) = spawn_run(&c, 13);
    assert_eq!((code, out.contains("LOGIN_NEEDED")), (3, true), "{out}");
}

#[test]
fn the_pattern_in_a_reply_or_tool_output_is_no_banner() {
    let d = tmpdir("acct-banner-src");
    let c = ctx(&d, &config(&d, ""));
    let sh = FakeShed::attached(&c);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    spawn(&c, &sh, 12);
    for screen in [
        "⏺ Bash(rg -n 'hit your limit' crates)\n  ⎿  claude.rs:19: usage limit reached|hit your limit\n",
        "⏺ The pane said \"You've hit your limit · resets 3pm\", so I matched it.\n",
        "❯ why did it say usage limit reached?\n",
    ] {
        check_shows(&c, &sh, screen);
    }
    assert!(kinds(&c, "usage-limit").is_empty());
    spawn(&c, &sh, 13);
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

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}
