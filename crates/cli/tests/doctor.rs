//! #97: `herdsman doctor` is read-only, prints ✓/✗/! with a fix per line, exits 1 on any ✗, never echoes a secret.
mod common;
use common::*;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const TOKEN: &str = concat!("sk", "-ant-oat01-FAKEtokenFAKEtokenFAKEtoken0123456789");
const HERDR: &str = "client:\n  version: 0.9.5\n\nserver:\n  status: running\n  version: 0.9.5\n";
const PLIST: &str = "<plist><dict><key>ProgramArguments</key><array><string>/x/herdsman</string>\
<string>serve</string><string>--addr</string><string>127.0.0.1:7777</string></array></dict></plist>";
const SUB: &str = r#"[{"endpoint":"https://push.example/x","p256dh":"k","auth":"a","label":"phone","created":1}]"#;

fn config(d: &Path, harness: &str) -> String {
    format!(
        r#"[fleet.demo]
repo = "o/demo"
local = "{local}"
integration = "staging"

[fleet.demo.role.impl]
harness = "{harness}"
profile = "fleet-impl"

[accounts]
main = "TOKEN_MAIN"

[accounts.usage]
main = "{accounts}/main"

[serve]
allowed_hosts = ["mac.tail.ts.net"]
webhook_secret_file = "{hook}"

[[notify]]
type = "webpush"
sub = "mailto:a@b.c"
"#,
        local = d.join("local").display(),
        accounts = d.join("accounts").display(),
        hook = d.join("config/secrets/GITHUB_WEBHOOK_SECRET").display(),
    )
}

fn write(p: &Path, text: &str, mode: u32) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// A host where every check passes.
fn setup(name: &str, harness: &str) -> (herdsman::Ctx, PathBuf) {
    let d = tmpdir(name);
    let c = ctx(&d, &config(&d, harness));
    std::fs::create_dir_all(d.join("local")).unwrap();
    std::fs::create_dir_all(&c.worktrees).unwrap();
    // #683: account main's own login folder, signed in.
    write(&d.join("accounts/main/.claude.json"), r#"{"oauthAccount": {}}"#, 0o600);
    // #707: the webhook secret's 0600 file.
    write(&d.join("config/secrets/GITHUB_WEBHOOK_SECRET"), "hook", 0o600);
    write(
        &c.home.join("Library/LaunchAgents/dev.herdsman.serve.plist"),
        PLIST,
        0o644,
    );
    let st = c.state.parent().unwrap().to_path_buf();
    write(
        &st.join("serve/devices.json"),
        r#"[{"hash":"h","label":"phone"}]"#,
        0o600,
    );
    write(
        &st.join("webpush/vapid.key"),
        "S3CRETvapidKEYvalue0123456789abcdefABCDEF",
        0o600,
    );
    write(&st.join("webpush/subs.json"), SUB, 0o600);
    for w in ["check", "drive"] {
        write(&st.join(format!("{w}.at")), &(c.now - 60).to_string(), 0o644);
    }
    // #237: the tick and fast jobs as this build writes them.
    assert_eq!(run(&c, &good(), &["timer", "install", "--no-load"]).0, 0);
    (c, d)
}

fn good() -> Fake {
    Fake::default()
        .reply("herdr status", HERDR)
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1")
        .reply("vm_stat", "Swapouts: 5.\n")
        .reply("sysctl -n vm.loadavg", "{ 1.00 1.00 1.00 }")
        .reply("sysctl -n hw.ncpu", "8")
        .reply("gh auth status", "")
        .reply("gh repo view o/demo", "{}")
        .reply("git -C ", "abc\n")
        .reply("which ", "/bin/x\n")
        .reply("lsof ", "herdsman 1 u LISTEN\n")
        .reply("tailscale serve status", "|-- / proxy http://127.0.0.1:7777\n")
        .reply("launchctl print gui/", "state = running\n")
        .reply("systemctl --user is-active", "active\n")
}

fn line<'a>(out: &'a str, check: &str) -> &'a str {
    let pre = format!(" {check} — ");
    out.lines()
        .find(|l| l[l.find(' ').unwrap()..].starts_with(&pre))
        .unwrap_or_else(|| panic!("{check}: {out}"))
}

#[test]
fn all_good_exits_zero_and_never_writes() {
    let (c, _) = setup("doc-good", "claude");
    let r = good();
    let (code, out) = run(&c, &r, &["doctor"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains('✗') && !out.contains('!'), "{out}");
    for check in ["herdr", "host", "gh", "config", "worktrees", "gh demo", "local demo"] {
        assert!(line(&out, check).starts_with('✓'), "{out}");
    }
    for check in [
        "accounts",
        "harness",
        "serve hosts",
        "serve",
        "tailscale",
        "launchd",
        "phone",
        "webhook",
    ] {
        // The launchd job is macOS only; Linux has no row for it.
        if check == "launchd" && !cfg!(target_os = "macos") {
            assert!(!out.contains(&format!(" {check} — ")), "{out}");
            continue;
        }
        assert!(line(&out, check).starts_with('✓'), "{out}");
    }
    for check in ["vapid.key", "push", "watchdog check", "watchdog drive", "timer"] {
        assert!(line(&out, check).starts_with('✓'), "{out}");
    }
    // #707 (q855): one account-source row, no per-account rows, no Keychain read, no signature row.
    assert!(
        line(&out, "accounts").contains("sign in from each account's own folder") && !out.contains("TOKEN_MAIN"),
        "{out}"
    );
    assert!(
        !out.contains(" account main — ") && !out.contains(" signature — "),
        "{out}"
    );
    assert!(
        !r.calls
            .borrow()
            .iter()
            .any(|c| c.starts_with("security ") || c.starts_with("codesign ")),
        "{:?}",
        r.calls.borrow()
    );
    let writes = [
        "gh pr",
        "gh issue",
        "gh api",
        "git push",
        "launchctl bootstrap",
        "tailscale serve --bg",
    ];
    assert!(
        r.calls
            .borrow()
            .iter()
            .all(|c| !writes.iter().any(|w| c.starts_with(w))),
        "{:?}",
        r.calls.borrow()
    );

    let (code, json) = run(&c, &r, &["doctor", "--json"]);
    assert_eq!(code, 0, "{json}");
    let rows: Vec<String> = serde_json::from_str(&json).unwrap();
    assert_eq!(rows.join("\n") + "\n", out);

    // #155/#156: a leftover `driver` key still parses and is ignored; doctor no longer notes it.
    let text = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(
        &c.config,
        text.replace(
            "integration = \"staging\"",
            "integration = \"staging\"\ndriver = \"herdsman\"",
        ),
    )
    .unwrap();
    let (code, out) = run(&c, &r, &["doctor"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("driver"), "{out}");
}

#[test]
fn secrets_never_echo() {
    // #707: the webhook secret and VAPID key are files; doctor checks them, never shows them.
    let (c, _) = setup("doc-secret", "claude");
    for args in [&["doctor"][..], &["doctor", "--json"]] {
        let (code, out) = run(&c, &good(), args);
        assert_eq!(code, 0, "{out}");
        assert!(
            !out.contains(TOKEN) && !out.contains("FAKEtoken") && !out.contains("S3CRET"),
            "{out}"
        );
    }
    let (_, out) = run(&c, &good(), &["doctor"]);
    assert!(
        line(&out, "webhook").starts_with('✓') && !out.contains("— hook"),
        "{out}"
    );
}

/// Each failure: (what breaks, the check, its fix).
#[test]
fn each_check_fails_with_its_fix_and_exit_one() {
    type Break = fn(&herdsman::Ctx, &Path, Fake) -> Fake;
    let cases: Vec<(Break, &str, &str)> = vec![
        (|_, _, r| r.once("herdr status", 1, ""), "herdr", "→ herdr"),
        (|_, _, r| r.once("vm_stat", 1, ""), "host", "→ put /usr/sbin"),
        (|_, _, r| r.once("gh auth status", 1, ""), "gh", "→ gh auth login"),
        (|_, _, r| r.once("gh repo view", 1, ""), "gh demo", "→ gh auth refresh"),
        (|_, _, r| r.once("git -C ", 1, ""), "local demo", "fetch origin staging"),
        (
            |_, d, r| {
                std::fs::remove_dir_all(d.join("local")).unwrap();
                r.once("git -C ", 128, "")
            },
            "local demo",
            "|| git clone https://github.com/o/demo.git",
        ),
        (
            |c, _, r| {
                std::fs::remove_dir_all(&c.worktrees).unwrap();
                r
            },
            "worktrees",
            "→ mkdir -p",
        ),
        (|_, _, r| r.once("which claude", 1, ""), "harness", "→ install claude"),
        (
            |_, _, r| r.once("lsof ", 1, ""),
            "serve",
            "→ herdsman serve --addr 127.0.0.1:7777",
        ),
        (
            |_, _, r| r.once("tailscale serve status", 0, "No serve config\n"),
            "tailscale",
            "→ tailscale serve --bg",
        ),
        (
            |_, _, r| r.once("launchctl print", 113, ""),
            "launchd",
            "→ launchctl bootstrap gui/$(id -u)",
        ),
        (
            |c, _, r| {
                std::fs::remove_file(c.home.join("Library/LaunchAgents/dev.herdsman.serve.plist")).unwrap();
                r
            },
            "launchd",
            "no plist runs herdsman serve",
        ),
        (
            |c, _, r| {
                let k = c.state.with_file_name("webpush/vapid.key");
                std::fs::set_permissions(k, std::fs::Permissions::from_mode(0o644)).unwrap();
                r
            },
            "vapid.key",
            "&& chmod 600",
        ),
        (
            |c, _, r| {
                std::fs::remove_file(c.state.with_file_name("webpush/vapid.key")).unwrap();
                r
            },
            "vapid.key",
            "missing → herdsman push test",
        ),
        (
            |c, _, r| {
                std::fs::remove_file(c.state.with_file_name("check.at")).unwrap();
                r
            },
            "watchdog check",
            "ran never",
        ),
        (
            |c, _, r| {
                write(&c.state.with_file_name("drive.at"), &(c.now - 3600).to_string(), 0o644);
                r
            },
            "watchdog drive",
            "→ herdsman timer install",
        ),
        (
            // macOS: the serve job's probe passes, the tick job's fails; Linux probes systemd.
            |_, _, r| match cfg!(target_os = "macos") {
                true => r.once("launchctl print", 0, "").once("launchctl print", 1, ""),
                false => r.once("systemctl --user is-active", 3, ""),
            },
            "timer",
            "dev.herdsman.tick missing → herdsman timer install",
        ),
    ];
    for (i, (brk, check, fix)) in cases.into_iter().enumerate() {
        if check == "launchd" && !cfg!(target_os = "macos") {
            continue;
        }
        let (c, d) = setup(&format!("doc-fail-{i}"), "claude");
        let r = brk(&c, &d, good());
        let (code, out) = run(&c, &r, &["doctor"]);
        assert_eq!(code, 1, "{check}: {out}");
        let l = line(&out, check);
        assert!(l.starts_with('✗') && l.contains(fix), "{check}: {l}");
        assert_eq!(out.matches('✗').count(), 1, "{check}: {out}");
    }
}

#[test]
fn warnings_do_not_fail() {
    let (c, _) = setup("doc-warn", "claude");
    // #344: no paired phone, no push subscription (setup's files were imported into state.db).
    for t in ["devices", "push"] {
        herdsman::db::store::<serde_json::Value>(&c.state, t, &[]).unwrap();
    }
    let old = HERDR.replacen("version: 0.9.5\n\n", "version: 0.9.6\n\n", 1);
    let r = good().once("herdr status", 0, &old);
    let (code, out) = run(&c, &r, &["doctor"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        line(&out, "herdr").starts_with("! herdr — client 0.9.6, server 0.9.5 → herdr update --handoff"),
        "{out}"
    );
    for check in ["phone", "push"] {
        assert!(line(&out, check).starts_with('!'), "{out}");
    }
}

#[test]
fn tailscale_missing_is_a_warning() {
    struct NoTailscale(Fake);
    impl herdsman::runner::Runner for NoTailscale {
        fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<herdsman::runner::Out> {
            anyhow::ensure!(prog != "tailscale", "cannot run tailscale");
            self.0.run(prog, args)
        }
        fn sleep(&self, _: u64) {}
    }
    let (c, _) = setup("doc-nots", "claude");
    let (code, out) = run(&c, &NoTailscale(good()), &["doctor"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        line(&out, "tailscale").starts_with("! tailscale — serve maps to 127.0.0.1:7777 →"),
        "{out}"
    );
}

#[test]
fn bad_config_and_hermes_profiles() {
    let (c, _) = setup("doc-badcfg", "claude");
    std::fs::write(&c.config, "[fleet.x]\nrepo = 1\n").unwrap();
    let (code, out) = run(&c, &good(), &["doctor"]);
    assert_eq!(code, 1, "{out}");
    assert!(line(&out, "config").starts_with('✗'), "{out}");
    assert!(line(&out, "herdr").starts_with('✓'), "{out}");

    let (c, _) = setup("doc-hermes", "hermes");
    let (code, out) = run(&c, &good(), &["doctor"]);
    assert_eq!(code, 1, "{out}");
    assert!(
        line(&out, "hermes fleet-impl").contains("→ hermes profile create fleet-impl"),
        "{out}"
    );
    assert!(
        line(&out, "hermes hooks").starts_with('✓'),
        "no profile, nothing to check: {out}"
    );

    // #401: the default conductor (the impl role on `lane-conductor`) needs its Hermes profile too.
    assert!(line(&out, "hermes lane-orchestrator").starts_with('✗'), "{out}");
    for p in ["fleet-impl", "lane-orchestrator"] {
        write(&c.hermes.join(p).join("config.yaml"), "", 0o644);
    }
    let (code, out) = run(&c, &good(), &["doctor"]);
    assert_eq!(code, 1, "{out}");
    assert!(line(&out, "hermes fleet-impl").starts_with('✓'), "{out}");
    assert!(line(&out, "hermes hooks").contains("→ herdsman profiles sync"), "{out}");

    let hook = |role: &str| format!("'{}' hook --role {role} ✓ allowed", c.exe);
    let (code, out) = run(
        &c,
        &good()
            .reply("env HERDSMAN_LANE=demo:-:impl hermes", &hook("impl"))
            .reply("env HERDSMAN_LANE=demo:-:orchestrator hermes", &hook("orchestrator")),
        &["doctor"],
    );
    assert_eq!(code, 0, "{out}");
}

/// #539: a config still naming the retired `accounts_env` loads; doctor warns (exit 0) until the line is dropped
/// (#683: the account signs in with its own folder either way).
#[test]
fn retired_accounts_env_warns_until_the_line_is_dropped() {
    let (c, d) = setup("doc-retired", "claude");
    let env = d.join("tokens.env");
    write(&env, &format!("TOKEN_MAIN={TOKEN}\n"), 0o600);
    let text = std::fs::read_to_string(&c.config).unwrap();
    let line_ = format!("accounts_env = \"{}\"\n", env.display());
    let old = text.replace("[accounts]\n", &format!("[accounts]\n{line_}"));
    std::fs::write(&c.config, &old).unwrap();
    let warn = "! config — [accounts].accounts_env is retired (#441): drop the line (#683: accounts sign in with their own login)";

    let r = good();
    let (code, out) = run(&c, &r, &["doctor"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.lines().any(|l| l == warn), "{out}");
    assert!(line(&out, "accounts").starts_with('✓'), "{out}");
    assert!(!out.contains(TOKEN), "{out}");

    std::fs::write(&c.config, &text).unwrap();
    let (code, out) = run(&c, &good(), &["doctor"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains('!'), "{out}");
}

/// #737: a profile hook naming another binary fails doctor and `profiles sync --check`, both naming it.
#[test]
fn a_hook_naming_another_binary_is_named() {
    let (mut c, d) = setup("doc-stray-hook", "claude");
    let other = d.join("other/herdsman");
    write(&other, "", 0o755);
    let real = c.exe.clone();
    c.exe = other.display().to_string();
    assert_eq!(run(&c, &good(), &["profiles", "sync"]).0, 0);
    c.exe = real;
    let (code, out) = run(&c, &good(), &["doctor"]);
    let row = line(&out, "profile hooks");
    assert!(
        code == 1 && row.starts_with('✗') && row.contains(&*other.to_string_lossy()),
        "{out}"
    );
    let (code, out) = run(&c, &good(), &["profiles", "sync", "--check"]);
    assert!(code == 1 && out.contains(&*other.to_string_lossy()), "{out}");
    assert_eq!(run(&c, &good(), &["profiles", "sync"]).0, 0);
    let (code, out) = run(&c, &good(), &["doctor"]);
    assert!(code == 0 && line(&out, "profile hooks").starts_with('✓'), "{out}");
}

/// #708: the launchd row follows `[serve] launchd_label` (default `dev.herdsman.serve`), never another plist that
/// mentions `herdsman serve`; a missing one lists what it found.
#[test]
fn launchd_row_follows_the_configured_label() {
    if !cfg!(target_os = "macos") {
        return;
    }
    let (c, _) = setup("doc-launchd", "claude");
    let agents = c.home.join("Library/LaunchAgents");
    for old in ["com.a.herdsman.plist", "local.herdsman.plist", "zz.herdsman.old.plist"] {
        write(&agents.join(old), PLIST, 0o644);
    }
    let (code, out) = run(&c, &good(), &["doctor"]);
    assert_eq!(code, 0, "{out}");
    let l = line(&out, "launchd");
    assert!(l.starts_with('✓') && l.ends_with("/dev.herdsman.serve loaded"), "{l}");

    let text = std::fs::read_to_string(&c.config).unwrap();
    let mine = text.replace("[serve]\n", "[serve]\nlaunchd_label = \"dev.mine.serve\"\n");
    std::fs::write(&c.config, mine).unwrap();
    let (code, out) = run(&c, &good(), &["doctor"]);
    assert_eq!(code, 1, "{out}");
    let l = line(&out, "launchd");
    assert!(l.starts_with('✗') && l.contains("dev.mine.serve.plist missing"), "{l}");
    assert!(l.contains("found") && l.contains("dev.herdsman.serve.plist"), "{l}");
    write(&agents.join("dev.mine.serve.plist"), PLIST, 0o644);
    let (code, out) = run(&c, &good(), &["doctor"]);
    assert_eq!(code, 0, "{out}");
    assert!(line(&out, "launchd").ends_with("/dev.mine.serve loaded"), "{out}");
}

/// #708: the stale spawn markers removed in 24 h are listed once each, with a count, capped at `+N more`.
#[test]
fn spawns_row_dedupes_stale_names() {
    let (c, _) = setup("doc-spawns", "claude");
    let mut names: Vec<String> = (1..=8).map(|n| format!("spawn-demo-{n}.pending")).collect();
    names.extend(std::iter::repeat_n("spawn-demo-1.pending".to_string(), 299));
    for (i, f) in names.iter().enumerate() {
        let ev = herdsman::events::Event::new(
            c.now - 600 + i as u64,
            "demo",
            Some(1),
            "stale-marker",
            serde_json::json!({ "file": f }),
        );
        herdsman::events::append(&c.state, &ev).unwrap();
    }
    let (code, out) = run(&c, &good(), &["doctor"]);
    assert_eq!(code, 0, "{out}");
    let l = out.lines().find(|l| l.contains("spawns starting")).unwrap();
    assert!(l.starts_with("! spawns starting 0/"), "{l}");
    assert_eq!(l.matches("spawn-demo-1.pending").count(), 1, "{l}");
    assert!(l.contains("spawn-demo-1.pending ×300"), "{l}");
    assert!(l.contains("+3 more"), "{l}");
    assert!(l.len() < 300, "{} chars: {l}", l.len());
}

/// #708: `self-update --pause` shows in doctor and on the board until `--resume`.
#[test]
fn a_paused_self_update_shows_in_doctor_and_the_board() {
    let (c, _) = setup("doc-paused", "claude");
    let r = good();
    assert_eq!(run(&c, &r, &["self-update", "--pause"]).0, 0);
    let (code, out) = run(&c, &r, &["doctor"]);
    assert_eq!(code, 0, "{out}");
    let row = out
        .lines()
        .find(|l| l.contains(" self-update — "))
        .unwrap_or_else(|| panic!("{out}"));
    assert!(row.starts_with("! self-update — paused since "), "{row}");
    assert!(row.ends_with(" → herdsman self-update --resume"), "{row}");
    let (_, board) = run(&c, &r, &["board"]);
    assert!(board.lines().any(|l| l == row), "{board}");

    assert_eq!(run(&c, &r, &["self-update", "--resume"]).0, 0);
    let (_, out) = run(&c, &r, &["doctor"]);
    assert!(!out.contains("self-update"), "{out}");
    let (_, board) = run(&c, &r, &["board"]);
    assert!(!board.contains("self-update"), "{board}");
}

/// #708: a clean login shell that exports `CLAUDE_CODE_OAUTH_TOKEN` warns with the file to fix; neither the value nor
/// anything else the shell printed is shown.
#[test]
fn a_login_shell_exporting_the_token_warns() {
    let (c, _) = setup("doc-login", "claude");
    let warn = "! a login shell exports CLAUDE_CODE_OAUTH_TOKEN (it overrides each lane's account) → remove it from";
    let (_, out) = run(&c, &good(), &["doctor"]);
    assert!(!out.contains("login shell"), "{out}");
    let shell = format!("Welcome {TOKEN}\ntoken:set\nfile:.zshrc\nfile:{TOKEN}\n");
    let r = good().reply("sh -c t=$(env -i HOME=", &shell);
    let (code, out) = run(&c, &r, &["doctor"]);
    assert_eq!(code, 0, "{out}");
    let l = out
        .lines()
        .find(|l| l.starts_with(warn))
        .unwrap_or_else(|| panic!("{out}"));
    assert_eq!(l, format!("{warn} ~/.zshrc"), "{out}");
    assert!(!out.contains("FAKEtoken") && !out.contains("Welcome"), "{out}");
}

/// #791: a tick still running with a fresh heartbeat reads `running for Nm`, never stuck; with no heartbeat for 2
/// intervals (slow_secs 300) the stale stamp reads stuck again.
#[test]
fn a_long_tick_with_a_fresh_heartbeat_is_running_not_stuck() {
    let (c, _) = setup("doc-beat", "claude");
    let st = c.state.parent().unwrap().to_path_buf();
    write(&st.join("check.at"), &(c.now - 1080).to_string(), 0o644);
    write(&st.join("tick.beat"), &format!("{} {}", c.now - 360, c.now - 30), 0o644);
    let (code, out) = run(&c, &good(), &["doctor"]);
    assert_eq!(code, 0, "{out}");
    let l = line(&out, "watchdog check");
    assert!(l.starts_with('✓') && l.contains("running for 6m"), "{l}");
    write(
        &st.join("tick.beat"),
        &format!("{} {}", c.now - 1080, c.now - 660),
        0o644,
    );
    let (code, out) = run(&c, &good(), &["doctor"]);
    assert_eq!(code, 1, "{out}");
    let l = line(&out, "watchdog check");
    assert!(l.starts_with('✗') && l.contains("ran 18m"), "{l}");
}
