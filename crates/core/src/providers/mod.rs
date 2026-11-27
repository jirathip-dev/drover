use crate::runner::Runner;
use crate::spawn::Plan;
use crate::Ctx;
use anyhow::Result;
use serde_json::Value;
use std::io::Write;
use std::ops::Deref;
use std::path::PathBuf;

pub use claude::{remove_tmp, root as profiles_root, LOCAL_DIR, LOCAL_FILES};

macro_rules! registry { ($($(#[$m:meta])* $n:ident),* $(,)?) => {
    $($(#[$m])* mod $n;)*
    static ALL: &[Entry] = &[$($(#[$m])* Entry { name: stringify!($n), provider: &$n::PROVIDER }),*];
} }
registry! { claude, codex, gemini, #[cfg(test)] fake }

/// A registry entry: its name (the registry's ident) and the provider it derefs to.
#[derive(Clone, Copy)]
pub struct Entry {
    pub name: &'static str,
    pub provider: &'static dyn Provider,
}

impl Deref for Entry {
    type Target = dyn Provider;
    fn deref(&self) -> &Self::Target {
        self.provider
    }
}

/// How a provider is reached.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    Terminal,
    Acp,
    Listed,
}

/// Every provider, in registry order.
pub fn all() -> impl Iterator<Item = Entry> {
    ALL.iter().copied()
}

fn on(s: Surface) -> impl Iterator<Item = Entry> {
    all().filter(move |p| p.surface() == s)
}

/// The lane providers a role's `harness` may name.
pub fn lanes() -> impl Iterator<Item = Entry> {
    on(Surface::Terminal)
}

/// Providers a role reaches only through its ACP `agent` (no terminal lane, not offered as a `harness`).
pub fn acp_only() -> impl Iterator<Item = Entry> {
    on(Surface::Acp)
}

/// The names a role's `harness` may take.
pub fn lane_names() -> Vec<&'static str> {
    lanes().map(|p| p.name).collect()
}

/// Every provider name `get` knows, the lanes then the ACP-only ones.
pub fn known() -> impl Iterator<Item = &'static str> {
    lanes().chain(acp_only()).map(|p| p.name)
}

/// ACP agent `id`'s model vendor, if known; a fleet's `rev` runs on another vendor than its `impl`.
pub fn vendor_of_agent(id: &str) -> Option<&'static str> {
    all().find(|p| p.acp() == Some(id)).and_then(|p| p.vendor())
}

/// The provider whose adapter ACP agent `id` is (the one that guards it).
pub fn of_agent(id: &str) -> Option<&'static str> {
    known().find(|h| get(h).acp() == Some(id))
}

/// The names an AI co-author trailer carries.
pub fn brands() -> impl Iterator<Item = &'static str> {
    all().flat_map(|p| p.brands().iter().copied())
}

/// Effort levels a role may set (`effort = "…"`).
pub const EFFORTS: &[&str] = &["none", "low", "medium", "high", "xhigh"];

/// What a harness needs in a lane sandbox: folders it reads, folders it writes, hosts it reaches.
pub struct Needs {
    pub read: Vec<PathBuf>,
    pub write: Vec<PathBuf>,
    pub hosts: Vec<&'static str>,
}

/// A `drover doctor` row: passed, its mark when not, its text.
pub type Row = (bool, char, String);

/// A harness's hook protocol, as `drover hook` reads and answers it: whether it `sent` a call, the keys of a call's tool `input` (the first object wins), its `shell` and file-writing tools, its `hook_event_name` after a tool call and at a turn's end, and its `deny` answer.
pub struct Dialect {
    pub sent: fn(&Value) -> bool,
    pub input: &'static [&'static str],
    pub shell: &'static str,
    pub edits: &'static [&'static str],
    /// Its `hook_event_name` before a tool call.
    pub pre: &'static str,
    pub post: &'static str,
    pub stop: &'static str,
    /// Its `hook_event_name` while it waits on a person (none: "").
    pub blocked: &'static str,
    pub deny: fn(&str) -> Value,
    /// Its `hook_event_name` when a prompt is submitted, and its answer that ends no turn but goes on with `reason` (none: "", `None`).
    pub prompt: &'static str,
    pub again: Option<fn(&str) -> Value>,
    /// Its answer that runs the tool call with this tool input instead (none: it can't).
    pub rewrite: Option<fn(Value) -> Value>,
}

/// The hook dialects of every harness that has one, in `known` order.
pub fn dialects() -> impl Iterator<Item = &'static Dialect> {
    known().filter_map(|h| get(h).hook_dialect())
}

pub trait Provider: Sync {
    fn surface(&self) -> Surface {
        Surface::Terminal
    }
    fn vendor(&self) -> Option<&'static str> {
        None
    }
    fn brands(&self) -> &'static [&'static str] {
        &[]
    }
    fn efforts(&self) -> &'static [&'static str] {
        EFFORTS
    }
    fn model_default(&self) -> Option<&'static str> {
        None
    }
    fn sessions(&self) -> Option<&'static str> {
        None
    }
    fn profile_files(&self, _ctx: &Ctx, _profile: &str) -> Vec<PathBuf> {
        vec![]
    }
    fn sandbox(&self, ctx: &Ctx, profile: &str) -> Needs {
        let files = self.profile_files(ctx, profile).into_iter();
        Needs {
            read: files.filter_map(|f| Some(f.parent()?.to_path_buf())).collect(),
            write: vec![],
            hosts: vec![],
        }
    }
    fn hooks_allowed(
        &self,
        _ctx: &Ctx,
        _cfg: &crate::config::Config,
        _r: &dyn Runner,
        _out: &mut dyn Write,
    ) -> Result<bool> {
        Ok(true)
    }
    fn doctor(&self, _ctx: &Ctx, _cfg: &crate::config::Config, _r: &dyn Runner) -> Vec<Row> {
        vec![]
    }
    fn scrubs_env(&self, _var: &str) -> bool {
        false
    }
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
    fn ready(&self, _ctx: &Ctx, _r: &dyn Runner, _p: &Plan) -> Result<()> {
        Ok(())
    }
    fn prepare(&self, _ctx: &Ctx, _on: &crate::hosts::On, _p: &Plan) -> Result<()> {
        Ok(())
    }
    fn takes_model(&self) -> bool {
        false
    }
    fn env(&self) -> &'static [&'static str] {
        &[]
    }
    fn acp(&self) -> Option<&'static str> {
        None
    }
    fn acp_env(&self, _model: Option<&str>, _effort: Option<&str>) -> Vec<(&'static str, String)> {
        Vec::new()
    }
    fn acp_profile(&self, _settings: &str, _role: &str) -> String {
        String::new()
    }
    fn chat_settings(&self, profile: &std::path::Path) -> std::path::PathBuf {
        profile.join("settings.json")
    }
    fn acp_meta(&self) -> Option<serde_json::Value> {
        None
    }
    fn login(&self) -> Option<&'static str> {
        None
    }
    fn install(&self) -> Option<&'static str> {
        None
    }
    fn hook_dialect(&self) -> Option<&'static Dialect> {
        None
    }
    fn token_env(&self) -> Option<&'static str> {
        None
    }
    fn health_check(&self) -> Option<(&'static str, &'static str, &'static str)> {
        None
    }
    fn process_names(&self) -> &'static [&'static str] {
        &[]
    }
}

struct Generic;
impl Provider for Generic {}

/// The provider a role's `harness` names; an unknown or listed-only one gets the generic fallbacks.
pub fn get(name: &str) -> &'static dyn Provider {
    all()
        .find(|p| p.name == name && p.surface() != Surface::Listed)
        .map_or(&Generic, |p| p.provider)
}

/// A `shed-acp` runtime's default adapter: the first lane provider's that runs its hooks.
pub fn default_acp() -> Option<&'static str> {
    lanes().find_map(|h| h.acp())
}

/// The drover hook's command line, as Claude settings name it.
pub fn hook_command(ctx: &Ctx, role: &str) -> String {
    format!("'{}' hook --role {role}", real(&ctx.exe))
}

fn real(path: &str) -> String {
    std::fs::canonicalize(path).map_or(path.into(), |p| p.to_string_lossy().into())
}

/// The program of a drover hook command (`'<exe>' hook --role <r>`), any role; None for another hook.
pub fn hook_program(cmd: &str) -> Option<&str> {
    cmd.strip_prefix('\'')?.split_once("' hook --role ").map(|(exe, _)| exe)
}

/// Drover hook command `cmd` runs the running binary, through a symlink or not.
pub fn runs_this(ctx: &Ctx, cmd: &str) -> bool {
    hook_program(cmd).is_some_and(|exe| real(exe) == real(&ctx.exe))
}

/// Hook command `cmd` is this binary's hook for `role`.
pub fn guards(ctx: &Ctx, cmd: &str, role: &str) -> bool {
    runs_this(ctx, cmd) && cmd.ends_with(&format!("' hook --role {role}"))
}
