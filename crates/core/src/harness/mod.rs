//! Harness adapters (profile + hook install, launch argv, kick, screen readers, transcript, logins): a new harness is
//! one file plus its `NAMES` entry; core never names one, and any other name gets the generic fallbacks (#316).
mod claude;
mod claude_meter;
mod codex;
mod dsh;
mod hermes;

use crate::herdr;
use crate::runner::Runner;
use crate::spawn::Plan;
use crate::Ctx;
use anyhow::Result;
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};

pub use claude::{
    remove_tmp, root as profiles_root, AUTH_TOKEN_ENV, BASE_URL_ENV, BETA_HEADER, CAP_PATTERN, OAUTH_BETA,
    PROXY_UPSTREAM,
};
pub use dsh::{install_pre_push, DSH_TUI_VERSION};
pub use hermes::set_mcp;

pub const NAMES: [&str; 3] = ["claude", "hermes", "dsh"];

/// #732: harnesses a role reaches only through its ACP `agent` (no terminal lane, not offered as a `harness`).
pub const ACP_ONLY: [&str; 1] = ["codex"];

/// #732: every harness name, `NAMES` then `ACP_ONLY`.
pub fn known() -> impl Iterator<Item = &'static str> {
    NAMES.into_iter().chain(ACP_ONLY)
}

/// #732: each known ACP agent's model provider; a fleet's `rev` runs on another provider than its `impl`.
pub const PROVIDERS: &[(&str, &str)] = &[
    ("claude-acp", "anthropic"),
    ("codex-acp", "openai"),
    ("gemini-acp", "google"),
];

/// #732: ACP agent `id`'s model provider, if known.
pub fn provider(id: &str) -> Option<&'static str> {
    PROVIDERS.iter().find(|p| p.0 == id).map(|p| p.1)
}

/// #732: the harness whose adapter ACP agent `id` is (the one that guards it).
pub fn of_agent(id: &str) -> Option<&'static str> {
    known().find(|h| get(h).acp() == Some(id))
}

/// Effort levels a role may set (`effort = "…"`).
pub const EFFORTS: &[&str] = &["none", "low", "medium", "high", "xhigh"];

/// The levels `harness` takes: Claude `--effort` (`none` omits it), Hermes `--reasoning`; dsh none yet.
pub fn efforts(harness: &str) -> &'static [&'static str] {
    match harness {
        "dsh" => &["none"],
        _ => EFFORTS,
    }
}

/// How a harness's lanes sign in: unknown, an `[accounts]` account of this provider (`access::pick`; #683: its env from
/// `access::account_env`), or the harness's own login on the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Credential {
    None,
    Account(&'static str),
    Own,
}

/// A harness's hook protocol, as `herdsman hook` reads and answers it: whether it `sent` a call, the keys of a call's
/// tool `input` (the first object wins), its `shell` and file-writing tools, its `hook_event_name` after a tool call
/// and at a turn's end, and its `deny` answer.
pub struct Dialect {
    pub sent: fn(&Value) -> bool,
    pub input: &'static [&'static str],
    pub shell: &'static str,
    pub edits: &'static [&'static str],
    /// #508: its `hook_event_name` before a tool call.
    pub pre: &'static str,
    pub post: &'static str,
    pub stop: &'static str,
    /// #298: its `hook_event_name` while it waits on a person (none: "").
    pub blocked: &'static str,
    pub deny: fn(&str) -> Value,
    /// #511: its `hook_event_name` when a prompt is submitted, and its answer that ends no turn but goes on with
    /// `reason` (none: "", `None`).
    pub prompt: &'static str,
    pub again: Option<fn(&str) -> Value>,
}

/// Every harness by name, in `NAMES` order.
pub fn all() -> impl Iterator<Item = (&'static str, &'static dyn Harness)> {
    NAMES.into_iter().map(|n| (n, get(n)))
}

/// The hook dialects of every harness that has one, in `known` order.
pub fn dialects() -> impl Iterator<Item = &'static Dialect> {
    known().filter_map(|h| get(h).hook_dialect())
}

/// (used %, reset epoch) of a usage window, either part unread.
pub type Part = (Option<f64>, Option<u64>);

/// A harness's usage meter for an `[accounts]` login (#258; #707: its status line only, no poll).
pub trait UsageMeter: Sync {
    /// The (5-hour, 7-day) parts of its status-line JSON (`herdsman statusline`).
    fn status(&self, v: &Value) -> (Part, Part);
}

/// The harness `[accounts]` are for: the first that runs on one.
pub fn accounts() -> Option<&'static dyn Harness> {
    NAMES
        .iter()
        .map(|n| get(n))
        .find(|h| matches!(h.credential(), Credential::Account(_)))
}

pub trait Harness: Sync {
    /// Write this harness's files for a `role`-family `profile` (hooks) with `prompt`'s role prompt.
    fn sync(
        &self,
        _ctx: &Ctx,
        _role: &str,
        _prompt: &str,
        _profile: &str,
        _usage: &str,
        _out: &mut dyn Write,
    ) -> Result<()> {
        Ok(())
    }
    /// Fail unless the profile and install are ready to spawn plan `p`.
    fn ready(&self, _ctx: &Ctx, _r: &dyn Runner, _p: &Plan) -> Result<()> {
        Ok(())
    }
    /// Lane setup in the worktree before the harness starts.
    fn prepare(&self, _ctx: &Ctx, _on: &crate::hosts::On, _p: &Plan) -> Result<()> {
        Ok(())
    }
    /// herdr argv that launches the harness in `pane` (generic: its name typed into the lane's shell).
    fn start_cmd(&self, _ctx: &Ctx, p: &Plan, pane: &str) -> Vec<String> {
        herdr::run_argv(pane, &[&p.harness])
    }
    /// #355: `start_cmd` honours `Plan::session` (start it, or resume it with its conversation).
    fn resumes(&self) -> bool {
        false
    }
    /// #404: `start_cmd` passes `Plan::model` (else its profile owns the model).
    fn takes_model(&self) -> bool {
        false
    }
    /// herdr commands that settle the harness, deliver the kick and confirm it landed.
    fn kick_cmds(&self, p: &Plan, pane: &str) -> Vec<Vec<String>> {
        vec![herdr::send_text_argv(pane, &p.kick), herdr::keys_argv(pane, "enter")]
    }
    /// Run `kick_cmds`; true once the kick is confirmed landed.
    fn kick(&self, r: &dyn Runner, p: &Plan, pane: &str) -> Result<bool> {
        Ok(self
            .kick_cmds(p, pane)
            .iter()
            .all(|c| herdr::exec(r, c).is_ok_and(|o| o.code == 0)))
    }
    /// `VAR=value` words the lane's shell exports before the launch.
    fn env(&self) -> &'static [&'static str] {
        &[]
    }
    /// #565: the ACP adapter (its `shed agents` id) known to run this harness's hooks (generic: none, unguarded).
    fn acp(&self) -> Option<&'static str> {
        None
    }
    /// #565: the env its ACP adapter reads its model (#732: and effort) from (#683: its config folder is the account's, `Plan::lane_cmd`).
    fn acp_env(&self, _model: Option<&str>, _effort: Option<&str>) -> Vec<(&'static str, String)> {
        Vec::new()
    }
    /// #683: whether config folder `dir` holds its CLI's own login, read from its files (never a Keychain or a CLI
    /// call; generic: unknown, false).
    fn signed_in(&self, _dir: &Path) -> bool {
        false
    }
    /// #565: the shell words that put a lane's profile (`settings.json`, `role.md`, both quoted) where its ACP
    /// adapter reads them in the worktree (generic: none).
    fn acp_profile(&self, _settings: &str, _role: &str) -> String {
        String::new()
    }
    /// How its lanes sign in (generic: unknown).
    fn credential(&self) -> Credential {
        Credential::None
    }
    /// #708: the env var a token in the shell overrides its CLI's login with (generic: none).
    fn token_env(&self) -> Option<&'static str> {
        None
    }
    /// The interactive login a person runs for it (setup panes, login hints).
    fn login(&self) -> Option<&'static str> {
        None
    }
    /// The shell line that installs it on a fresh host (`hosts bootstrap`; generic: none).
    fn install(&self) -> Option<&'static str> {
        None
    }
    /// Its `herdsman hook` protocol (generic: none, no tool guard but git's pre-push).
    fn hook_dialect(&self) -> Option<&'static Dialect> {
        None
    }
    /// Its accounts' usage meter (generic: none).
    fn usage_meter(&self) -> Option<&'static dyn UsageMeter> {
        None
    }
    /// The pane shows a usage-limit banner: until when.
    fn usage_limited(&self, _cfg: &crate::config::Config, _r: &dyn Runner, _screen: &str, _now: u64) -> Option<u64> {
        None
    }
    /// The pane shows a signed-out error (#302).
    fn signed_out(&self, _screen: &str) -> bool {
        false
    }
    /// The live spinner's word and, past a minute, its elapsed time; None when idle or without one.
    fn status_word(&self, _screen: &str) -> Option<(String, Option<String>)> {
        None
    }
    /// A transcript line is an action, the latest one the now-line (generic: any non-chrome line).
    fn now_line(&self, line: &str) -> bool {
        !crate::lanes::chrome(line)
    }
    /// A pane line's tool call as (tool, argument) for the `now:` line (generic: a `$ ` shell line).
    fn tool_line(&self, line: &str) -> Option<(String, String)> {
        Some(("$".into(), line.strip_prefix("$ ")?.trim_start().into()))
    }
    /// A blocked pane's question and numbered options (answered by `answer_keys`); None: a screen excerpt and reply box.
    fn pending_menu(&self, _screen: &str) -> Option<(String, Vec<String>)> {
        None
    }
    /// #463: a first-start prompt on a pane that would not start: its name and the keys that answer it, in order.
    fn startup_prompt(&self, _screen: &str) -> Option<(&'static str, Vec<String>)> {
        None
    }
    /// The keys that pick menu option `n` (1-based).
    fn answer_keys(&self, n: usize) -> String {
        n.to_string()
    }
    /// The newest local session transcript (`.jsonl`) of worktree `wt` under `home`; None: the chat reads the pane.
    fn transcript(&self, _home: &Path, _wt: &str) -> Option<PathBuf> {
        None
    }
    /// The login a host's herdr pane checks: (tool, command, fix).
    fn health_check(&self) -> Option<(&'static str, &'static str, &'static str)> {
        None
    }
    /// #359: what its startup terminal title (OSC 0) contains, confirming a lane came up (generic: none).
    fn startup_title(&self) -> Option<&'static str> {
        None
    }
    /// The herdr agent labels of its panes, for the Host tab's memory row (generic: an agent's own label is its row).
    fn process_names(&self) -> &'static [&'static str] {
        &[]
    }
}

struct Generic;
impl Harness for Generic {}

/// #683 (q846): the one provider -> config-folder env table, the account source when no `account_env` is set.
pub const CONFIG_ENV: &[(&str, &str)] = &[("claude", "CLAUDE_CONFIG_DIR"), ("codex", "CODEX_HOME")];

pub fn get(name: &str) -> &'static dyn Harness {
    match name {
        "claude" => &claude::Claude,
        "hermes" => &hermes::Hermes,
        "dsh" => &dsh::Dsh,
        "codex" => &codex::Codex,
        _ => &Generic,
    }
}

/// #565: a `shed-acp` runtime's default adapter: the first harness's that runs its hooks.
pub fn default_acp() -> Option<&'static str> {
    all().find_map(|h| h.1.acp())
}

/// The herdsman hook's command line, as Claude settings and Hermes config/allowlist name it.
pub fn hook_command(ctx: &Ctx, role: &str) -> String {
    format!("'{}' hook --role {role}", real(&ctx.exe))
}

/// #737: `path` with its symlinks resolved (unresolvable: as given), so `~/.local/bin/herdsman` and the binary agree.
fn real(path: &str) -> String {
    std::fs::canonicalize(path).map_or(path.into(), |p| p.to_string_lossy().into())
}

/// #737: the program of a herdsman hook command (`'<exe>' hook --role <r>`), any role; None for another hook.
pub fn hook_program(cmd: &str) -> Option<&str> {
    cmd.strip_prefix('\'')?.split_once("' hook --role ").map(|(exe, _)| exe)
}

/// #737: herdsman hook command `cmd` runs the running binary, through a symlink or not.
pub fn runs_this(ctx: &Ctx, cmd: &str) -> bool {
    hook_program(cmd).is_some_and(|exe| real(exe) == real(&ctx.exe))
}

/// #737: hook command `cmd` is this binary's hook for `role`.
pub fn guards(ctx: &Ctx, cmd: &str, role: &str) -> bool {
    runs_this(ctx, cmd) && cmd.ends_with(&format!("' hook --role {role}"))
}

/// Try numbers `0..n`, sleeping `secs` before every try but the first.
fn attempts(r: &dyn Runner, n: u32, secs: u64) -> impl Iterator<Item = u32> + '_ {
    (0..n).inspect(move |&i| {
        if i > 0 {
            r.sleep(secs);
        }
    })
}
