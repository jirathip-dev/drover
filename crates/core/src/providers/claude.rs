use super::{hook_command, Dialect, Header, Provider, Proxy};
use crate::term;
use crate::runner::Runner;
use crate::spawn::{role_prompt, Plan};
use crate::Ctx;
use anyhow::{bail, Result};
use serde_json::json;
use std::io::Write;
use std::path::PathBuf;

const SETTINGS: &str = include_str!("../../hooks/claude-settings.json");

pub struct Claude;
pub static PROVIDER: Claude = Claude;

const BETA_HEADER: &str = "anthropic-beta";
const OAUTH_BETA: &str = "oauth-2025-04-20";
/// #770: what `acp_profile` copies into a worktree (the lane's own files, never its work) and the folder it makes.
pub const LOCAL_FILES: [&str; 2] = ["CLAUDE.local.md", ".claude/settings.local.json"];
pub const LOCAL_DIR: &str = ".claude/";

static PROXY: Proxy = Proxy {
    upstream: "https://api.anthropic.com",
    base_url_env: "ANTHROPIC_BASE_URL",
    auth_token_env: "ANTHROPIC_AUTH_TOKEN",
    setup: "claude setup-token",
    headers,
};

fn headers(kept: &[Header]) -> Vec<Header> {
    let sent = kept.iter().find(|h| h.0 == BETA_HEADER).map_or("", |h| h.1.as_str());
    let mut beta: Vec<&str> = sent.split(',').map(str::trim).filter(|b| !b.is_empty()).collect();
    if !beta.contains(&OAUTH_BETA) {
        beta.push(OAUTH_BETA);
    }
    let head = (BETA_HEADER.to_string(), beta.join(","));
    let rest = kept.iter().filter(|h| h.0 != BETA_HEADER).cloned();
    std::iter::once(head).chain(rest).collect()
}

static DIALECT: Dialect = Dialect {
    sent: |v| {
        let ev = v["hook_event_name"].as_str().unwrap_or_default();
        ["PreToolUse", "PostToolUse", "Stop"].contains(&ev) && v.get("args").is_none()
    },
    input: &["tool_input"],
    shell: "Bash",
    edits: &["Edit", "Write", "MultiEdit", "NotebookEdit"],
    pre: "PreToolUse",
    post: "PostToolUse",
    stop: "Stop",
    blocked: "Notification",
    deny: |reason| json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason}}),
    prompt: "UserPromptSubmit",
    again: Some(|reason| json!({"decision": "block", "reason": reason})),
    rewrite: Some(|ti| json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "allow", "updatedInput": ti}})),
};

/// `<config dir>/claude`, the generated per-role profiles.
pub fn root(ctx: &Ctx) -> PathBuf {
    ctx.config.with_file_name("claude")
}

impl Provider for Claude {
    fn vendor(&self) -> Option<&'static str> {
        Some("anthropic")
    }
    fn brands(&self) -> &'static [&'static str] {
        &["claude", "anthropic"]
    }
    fn proxy(&self) -> Option<&'static Proxy> {
        Some(&PROXY)
    }
    fn sandbox(&self, ctx: &Ctx, profile: &str) -> super::Needs {
        let h = crate::config::home();
        super::Needs {
            read: vec![root(ctx).join(profile), h.join(".local/share/claude")],
            write: vec![h.join(".claude"), h.join(".claude.json")],
            hosts: vec!["*.anthropic.com", "claude.ai", "*.claude.ai"],
        }
    }
    fn sync(&self, ctx: &Ctx, role: &str, prompt: &str, profile: &str, usage: &str, out: &mut dyn Write) -> Result<()> {
        let dir = root(ctx).join(profile);
        std::fs::create_dir_all(&dir)?;
        let hook_cmd = serde_json::to_string(&hook_command(ctx, role))?;
        let days = crate::config::Config::load(&ctx.config).map_or(crate::config::limits().transcript_days, |c| c.limits.transcript_days);
        let line = serde_json::to_string(&format!("'{}' statusline", ctx.exe))?;
        let settings = SETTINGS
            .replace("\"{{FLEET_HOOK}}\"", &hook_cmd)
            .replace("\"{{FLEET_STATUSLINE}}\"", &line)
            .replace("\"{{TRANSCRIPT_DAYS}}\"", &days.to_string());
        std::fs::write(dir.join("settings.json"), settings)?;
        let role_md = format!("{}\n\n{usage}", role_prompt(prompt));
        std::fs::write(dir.join("role.md"), role_md)?;
        std::fs::write(dir.join("fleet.md"), usage)?;
        writeln!(out, "synced {} ({prompt})", dir.display())?;
        Ok(())
    }

    fn ready(&self, ctx: &Ctx, _r: &dyn Runner, p: &Plan) -> Result<()> {
        if !root(ctx).join(&p.profile).join("settings.json").exists() {
            bail!("Claude profile {} missing; run `drover profiles sync`", p.profile);
        }
        Ok(())
    }

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
        a.extend(["--name".into(), p.title.clone()]);
        a.extend([
            "--mcp-config".into(),
            p.mcp_file.to_string_lossy().into(),
            "--strict-mcp-config".into(),
        ]);
        if let Some((id, resume)) = &p.session {
            a.extend([if *resume { "--resume" } else { "--session-id" }.into(), id.clone()]);
        }
        a.push(p.kick.clone());
        term::start_argv("claude", pane, &a)
    }

    fn resumes(&self) -> bool {
        true
    }

    fn takes_model(&self) -> bool {
        true
    }

    fn kick_cmds(&self, p: &Plan, _pane: &str) -> Vec<Vec<String>> {
        vec![term::wait_working_argv(&p.agent, "20000")]
    }

    fn kick(&self, r: &dyn Runner, p: &Plan, pane: &str) -> Result<bool> {
        self.kick_cmds(p, pane)
            .first()
            .map_or(Ok(false), |c| Ok(term::exec(r, c)?.code == 0))
    }

    fn env(&self) -> &'static [&'static str] {
        &["CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1", "DISABLE_AUTOUPDATER=1"]
    }
    fn acp(&self) -> Option<&'static str> {
        Some("claude-acp")
    }
    fn acp_env(&self, model: Option<&str>, _effort: Option<&str>) -> Vec<(&'static str, String)> {
        model.map(|m| ("ANTHROPIC_MODEL", m.to_string())).into_iter().collect()
    }
    fn acp_profile(&self, settings: &str, role: &str) -> String {
        let [md, json] = LOCAL_FILES;
        format!(" && mkdir -p .claude && cp {settings} {json} && cp {role} {md}")
    }
    fn chat_settings(&self, profile: &std::path::Path) -> std::path::PathBuf {
        profile.into()
    }
    fn acp_meta(&self) -> Option<serde_json::Value> {
        let global = crate::config::home().join(".claude/CLAUDE.md");
        let settings = json!({"claudeMdExcludes": [global]});
        let options = json!({"settingSources": ["project", "local"], "settings": settings, "strictMcpConfig": true});
        Some(json!({"claudeCode": {"options": options}}))
    }
    fn login(&self) -> Option<&'static str> {
        Some("claude /login")
    }
    fn install(&self) -> Option<&'static str> {
        Some("curl -fsSL https://claude.ai/install.sh | bash")
    }
    fn hook_dialect(&self) -> Option<&'static Dialect> {
        Some(&DIALECT)
    }
    fn token_env(&self) -> Option<&'static str> {
        Some("CLAUDE_CODE_OAUTH_TOKEN")
    }
    fn health_check(&self) -> Option<(&'static str, &'static str, &'static str)> {
        Some(("claude", "claude auth status", "claude auth login in a pane there"))
    }
    fn process_names(&self) -> &'static [&'static str] {
        &["claude"]
    }
}

/// #536: removes Claude Code's temp dir for worktree `wt`, `<root>/claude-<uid>/<wt, each non-alphanumeric as ->`.
pub fn remove_tmp(roots: &[PathBuf], wt: &str) {
    let key: String = wt
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    for root in roots {
        let ls = std::fs::read_dir(root).into_iter().flatten().flatten();
        for d in ls.filter(|e| e.file_name().to_string_lossy().starts_with("claude-")) {
            std::fs::remove_dir_all(d.path().join(&key)).ok();
        }
    }
}
