//! #732: Codex, as its ACP adapter `codex-acp` runs it (shed-acp lanes only): its login is the account folder's own
//! (`CODEX_HOME`), its guard herdsman's hook in the session config (`codex-config.json`), its role `AGENTS.override.md`.
use super::{hook_command, Credential, Dialect, Harness};
use crate::Ctx;
use anyhow::Result;
use serde_json::json;
use std::io::Write;
use std::path::Path;

pub struct Codex;

/// Codex's hooks speak Claude Code's protocol; its file edits are `apply_patch`, the patch in `command`.
static DIALECT: Dialect = Dialect {
    sent: |_| false,
    input: &["tool_input"],
    shell: "Bash",
    edits: &["apply_patch"],
    pre: "PreToolUse",
    post: "PostToolUse",
    stop: "Stop",
    blocked: "",
    deny: |reason| json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason}}),
    prompt: "UserPromptSubmit",
    again: Some(|reason| json!({"decision": "block", "reason": reason})),
};

/// The session config `codex-acp` merges in (`CODEX_CONFIG`): herdsman's hook on every tool call, turn end and
/// prompt, run without Codex's interactive trust review (a lane has no one to answer it).
pub fn config(hook: &str) -> serde_json::Value {
    let h = json!([{"hooks": [{"type": "command", "command": hook}]}]);
    json!({"bypass_hook_trust": true, "hooks": {"PreToolUse": h, "PostToolUse": h, "Stop": h, "UserPromptSubmit": h}})
}

impl Harness for Codex {
    fn sync(
        &self,
        ctx: &Ctx,
        role: &str,
        _prompt: &str,
        profile: &str,
        _usage: &str,
        out: &mut dyn Write,
    ) -> Result<()> {
        let dir = super::profiles_root(ctx).join(profile);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join("codex-config.json"),
            config(&hook_command(ctx, role)).to_string(),
        )?;
        writeln!(out, "synced {} codex ({role})", dir.display())?;
        Ok(())
    }
    fn acp(&self) -> Option<&'static str> {
        Some("codex-acp")
    }
    /// The model and effort as the head of the session config `acp_profile` exports; no approvals asked.
    fn acp_env(&self, model: Option<&str>, effort: Option<&str>) -> Vec<(&'static str, String)> {
        let mut head = String::new();
        if let Some(m) = model {
            head += &format!("\"model\":{},", json!(m));
        }
        if let Some(e) = effort.filter(|e| *e != "none") {
            head += &format!("\"model_reasoning_effort\":{},", json!(e));
        }
        vec![
            ("HERDSMAN_CODEX", head),
            ("INITIAL_AGENT_MODE", "agent-full-access".into()),
        ]
    }
    /// #785: `codex login status` in the folder: `Logged in …` or `Not logged in`.
    fn signed_in(&self, r: &dyn crate::runner::Runner, dir: &Path) -> Option<bool> {
        let home = format!("CODEX_HOME={}", dir.display());
        let a = crate::runner::argv(&[&home, "codex", "login", "status"]);
        let o = r.run("env", &a).ok()?;
        let said = o.stdout + &o.stderr;
        said.contains("ogged in").then(|| !said.contains("Not logged in"))
    }
    /// The session config (model head + the profile's hooks) as `CODEX_CONFIG`; the role ahead of the repo's own
    /// `AGENTS.md` in `AGENTS.override.md`, which Codex reads in its place.
    fn acp_profile(&self, settings: &str, role: &str) -> String {
        let cfg = settings.replace("settings.json'", "codex-config.json'");
        format!(
            " && export CODEX_CONFIG=\"{{$HERDSMAN_CODEX$(tail -c +2 {cfg})\" && {{ cat {role}; cat AGENTS.md 2>/dev/null || true; }} > AGENTS.override.md"
        )
    }
    /// `[accounts]` of provider `codex` (`harness::CONFIG_ENV`: `CODEX_HOME`).
    fn credential(&self) -> Credential {
        Credential::Account("codex")
    }
    fn login(&self) -> Option<&'static str> {
        Some("codex login")
    }
    fn hook_dialect(&self) -> Option<&'static Dialect> {
        Some(&DIALECT)
    }
}
