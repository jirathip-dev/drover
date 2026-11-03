//! Claude Code: per-role settings.json (`PreToolUse` hook) + role.md; the kick goes in as argv.
use super::{hook_command, Credential, Dialect, Harness, UsageMeter};
use crate::config::Config;
use crate::herdr;
use crate::lanes::{chat, mask, strip};
use crate::runner::Runner;
use crate::spawn::{role_prompt, Plan};
use crate::Ctx;
use anyhow::{bail, Result};
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};

const SETTINGS: &str = include_str!("../../hooks/claude-settings.json");

pub struct Claude;

/// `|`-separated, case-insensitive substrings of Claude Code's usage-limit banner (`[accounts].cap_pattern` overrides).
pub const CAP_PATTERN: &str = "usage limit reached|hit your limit|limit reached ∙ resets|limit reached · resets";

/// Claude Code's signed-out errors (#302), as `CAP_PATTERN`.
const AUTH_PATTERN: &str = "please run /login|invalid api key|oauth token has expired|not logged in";

/// Claude Code's `PreToolUse` / `PostToolUse` / `Stop` hooks.
static DIALECT: Dialect = Dialect {
    sent: |v| {
        let ev = v["hook_event_name"].as_str().unwrap_or_default();
        ["PreToolUse", "PostToolUse", "Stop"].contains(&ev) && v.get("args").is_none()
    },
    input: &["tool_input"],
    shell: "Bash",
    edits: &["Edit", "Write", "MultiEdit", "NotebookEdit"],
    post: "PostToolUse",
    stop: "Stop",
    deny: |reason| json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason}}),
};

/// `<config dir>/claude`, the generated per-role profiles.
pub fn root(ctx: &Ctx) -> PathBuf {
    ctx.config.with_file_name("claude")
}

impl Harness for Claude {
    fn sync(&self, ctx: &Ctx, role: &str, profile: &str, usage: &str, out: &mut dyn Write) -> Result<()> {
        let dir = root(ctx).join(profile);
        std::fs::create_dir_all(&dir)?;
        let hook_cmd = serde_json::to_string(&hook_command(ctx, role))?;
        let days = crate::config::Config::load(&ctx.config).map_or(30, |c| c.limits.transcript_days);
        let line = serde_json::to_string(&format!("'{}' statusline", ctx.exe))?;
        let settings = SETTINGS
            .replace("\"{{FLEET_HOOK}}\"", &hook_cmd)
            .replace("\"{{FLEET_STATUSLINE}}\"", &line)
            .replace("\"{{TRANSCRIPT_DAYS}}\"", &days.to_string());
        std::fs::write(dir.join("settings.json"), settings)?;
        std::fs::write(dir.join("role.md"), format!("{}\n\n{usage}", role_prompt(role)))?;
        std::fs::write(dir.join("fleet.md"), usage)?;
        writeln!(out, "synced {} ({role})", dir.display())?;
        Ok(())
    }

    fn ready(&self, ctx: &Ctx, _r: &dyn Runner, p: &Plan) -> Result<()> {
        if !root(ctx).join(&p.profile).join("settings.json").exists() {
            bail!("Claude profile {} missing; run `herdsman profiles sync`", p.profile);
        }
        Ok(())
    }

    /// The lane's MCP file: only the role's servers.
    fn prepare(&self, _ctx: &Ctx, on: &crate::hosts::On, p: &Plan) -> Result<()> {
        on.write(
            &p.mcp_file,
            &serde_json::to_string_pretty(&serde_json::json!({"mcpServers": p.mcp}))?,
        )
    }

    fn start_cmd(&self, ctx: &Ctx, p: &Plan, pane: &str) -> Vec<String> {
        let dir = root(ctx).join(&p.profile);
        let mut a = vec![
            "--dangerously-skip-permissions".to_string(),
            "--settings".into(),
            dir.join("settings.json").to_string_lossy().into(),
            "--append-system-prompt-file".into(),
            dir.join("role.md").to_string_lossy().into(),
        ];
        if let Some(m) = &p.model {
            a.extend(["--model".into(), m.clone()]);
        }
        if let Some(e) = p.effort.as_deref().filter(|e| *e != "none") {
            a.extend(["--effort".into(), e.into()]);
        }
        // Display name = terminal title on the herdr board (else Claude titles it from the kick).
        a.extend(["--name".into(), p.title.clone()]);
        // `--mcp-config` is variadic: `--strict-mcp-config` ends it before the kick.
        a.extend([
            "--mcp-config".into(),
            p.mcp_file.to_string_lossy().into(),
            "--strict-mcp-config".into(),
        ]);
        a.push(p.kick.clone());
        herdr::start_argv(&p.agent, "claude", pane, a)
    }

    /// Claude gets the kick as argv, so it only waits.
    fn kick_cmds(&self, p: &Plan, _pane: &str) -> Vec<Vec<String>> {
        vec![herdr::wait_working_argv(&p.agent, "20000")]
    }

    fn kick(&self, r: &dyn Runner, p: &Plan, pane: &str) -> Result<bool> {
        Ok(herdr::exec(r, &self.kick_cmds(p, pane)[0])?.code == 0)
    }

    /// #248: no updater, telemetry or other background traffic (names as of Claude Code 2.1.311).
    fn env(&self) -> &'static [&'static str] {
        &["CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1", "DISABLE_AUTOUPDATER=1"]
    }
    /// The env var Claude Code reads its OAuth token from.
    fn credential(&self) -> Credential {
        Credential::Token("CLAUDE_CODE_OAUTH_TOKEN")
    }
    fn login(&self) -> Option<&'static str> {
        Some("claude /login")
    }
    fn hook_dialect(&self) -> Option<&'static Dialect> {
        Some(&DIALECT)
    }
    fn usage_meter(&self) -> Option<&'static dyn UsageMeter> {
        Some(&super::claude_meter::Meter)
    }
    fn usage_limited(&self, cfg: &Config, r: &dyn Runner, screen: &str, now: u64) -> Option<u64> {
        let pat = cfg.accounts.cap_pattern.as_deref().unwrap_or(CAP_PATTERN);
        crate::access::banner(pat, screen).then(|| crate::access::cap_until(cfg, r, screen, now))
    }
    fn signed_out(&self, screen: &str) -> bool {
        AUTH_PATTERN.split('|').any(|p| screen.to_lowercase().contains(p))
    }

    /// #276: `✻ Herding… (4m 12s · ↓ 3k tokens)` is (`✻ Herding…`, `4m`).
    fn status_word(&self, screen: &str) -> Option<(String, Option<String>)> {
        let s = strip(chat(screen).1?);
        let (w, rest) = s.split_once(" (")?;
        let mins = |t: &&str| t.strip_suffix(['m', 'h']).is_some_and(|n| n.parse::<u32>().is_ok());
        let t = rest.split([' ', ')']).find(mins).map(String::from);
        w.trim().ends_with('…').then(|| (mask(w.trim()), t))
    }
    fn now_line(&self, line: &str) -> bool {
        line.starts_with('⏺')
    }
    /// `⏺ Bash(cargo test)` (or `•`).
    fn tool_line(&self, line: &str) -> Option<(String, String)> {
        let (n, a) = line.strip_prefix(['⏺', '•'])?.trim_start().split_once('(')?;
        let word = !n.is_empty() && n.chars().all(char::is_alphanumeric);
        word.then(|| (n.into(), a.rsplit_once(')').map_or(a, |x| x.0).into()))
    }
    fn pending_menu(&self, screen: &str) -> Option<(String, Vec<String>)> {
        pending(screen)
    }

    /// #260: `<home>/.claude/projects/<worktree, non-alphanumerics as `-`>/*.jsonl`.
    fn transcript(&self, home: &Path, wt: &str) -> Option<PathBuf> {
        let d = home.join(".claude/projects");
        let all = std::fs::read_dir(d.join(wt.replace(|c: char| !c.is_ascii_alphanumeric(), "-"))).ok()?;
        let paths = all.flatten().map(|e| e.path());
        let jsonl = paths.filter(|p| p.extension().is_some_and(|x| x == "jsonl"));
        jsonl.max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok())
    }
    fn health_check(&self) -> Option<(&'static str, &'static str, &'static str)> {
        Some(("claude", "claude auth status", "claude auth login in a pane there"))
    }
    fn process_names(&self) -> &'static [&'static str] {
        &["claude"]
    }
}

/// A blocked pane's pending prompt: the question and a Claude menu's options (a `❯`-marked run
/// numbered 1, 2, …; wrapped lines skipped), or no options for a free-text question. None: no prompt seen.
fn pending(screen: &str) -> Option<(String, Vec<String>)> {
    let lines: Vec<&str> = screen
        .lines()
        .map(|l| l.trim_matches(|c: char| c.is_whitespace() || c == '│'))
        .filter(|l| !l.is_empty())
        .collect();
    let opt = |l: &str| {
        let (n, t) = l.trim_start_matches(['❯', '›']).trim_start().split_once(". ")?;
        Some((n.parse::<usize>().ok()?, t.trim().to_string()))
    };
    let question = |ls: &[&str]| {
        ls.iter()
            .rev()
            .take(6)
            .find(|l| l.contains('?'))
            .map(std::string::ToString::to_string)
    };
    let (mut opts, mut i, mut gap) = (
        vec![],
        lines.iter().rposition(|l| opt(l).is_some()).map_or(0, |i| i + 1),
        0,
    );
    while i > 0 && gap <= 2 && opts.last().is_none_or(|o: &(usize, String)| o.0 > 1) {
        i -= 1;
        match opt(lines[i]) {
            Some(o) => (opts.push(o), gap = 0).1,
            None => gap += 1,
        }
    }
    opts.reverse();
    let menu = opts.len() >= 2
        && opts.iter().zip(1..).all(|(o, n)| o.0 == n)
        && lines[i..].iter().any(|l| l.starts_with('❯') && opt(l).is_some());
    if menu {
        Some((
            question(&lines[..i]).or(lines[..i].last().map(std::string::ToString::to_string))?,
            opts.into_iter().map(|o| o.1).collect(),
        ))
    } else {
        Some((question(&lines)?, vec![]))
    }
}
