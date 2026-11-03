//! Harness adapters (profile + hook install, launch argv, kick, screen readers, transcript, logins): a new harness is
//! one file plus its `NAMES` entry; core never names one, and any other name gets the generic fallbacks (#316).
mod claude;
mod dsh;
mod hermes;

use crate::herdr;
use crate::runner::Runner;
use crate::spawn::Plan;
use crate::Ctx;
use anyhow::Result;
use std::io::Write;
use std::path::{Path, PathBuf};

pub use claude::root as profiles_root;
pub use dsh::{install_pre_push, DSH_TUI_VERSION};
pub use hermes::set_mcp;

pub const NAMES: [&str; 3] = ["claude", "hermes", "dsh"];

/// Effort levels a role may set (`effort = "…"`).
pub const EFFORTS: &[&str] = &["none", "low", "medium", "high", "xhigh"];

/// The levels `harness` takes: Claude `--effort` (`none` omits it), Hermes `--reasoning`; dsh none yet.
pub fn efforts(harness: &str) -> &'static [&'static str] {
    match harness {
        "dsh" => &["none"],
        _ => EFFORTS,
    }
}

pub trait Harness {
    /// Write this harness's files for role `role`'s `profile` (prompts, hooks).
    fn sync(&self, _ctx: &Ctx, _role: &str, _profile: &str, _usage: &str, _out: &mut dyn Write) -> Result<()> {
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
    /// Lanes run on an `[accounts]` token (`access::TOKEN_VAR`); else on the harness's own login.
    fn accounts(&self) -> bool {
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
    /// A blocked pane's question and numbered options (answered by digit); None: a screen excerpt and reply box.
    fn pending_menu(&self, _screen: &str) -> Option<(String, Vec<String>)> {
        None
    }
    /// The newest local session transcript (`.jsonl`) of worktree `wt` under `home`; None: the chat reads the pane.
    fn transcript(&self, _home: &Path, _wt: &str) -> Option<PathBuf> {
        None
    }
    /// The login a host's herdr pane checks: (tool, command, fix).
    fn health_check(&self) -> Option<(&'static str, &'static str, &'static str)> {
        None
    }
    /// The herdr agent labels whose pane trees the Host tab's memory row sums; none: no row.
    fn process_names(&self) -> &'static [&'static str] {
        &[]
    }
}

struct Generic;
impl Harness for Generic {}

pub fn get(name: &str) -> &'static dyn Harness {
    match name {
        "claude" => &claude::Claude,
        "hermes" => &hermes::Hermes,
        "dsh" => &dsh::Dsh,
        _ => &Generic,
    }
}

/// The herdsman hook's command line, as Claude settings and Hermes config/allowlist name it.
pub fn hook_command(ctx: &Ctx, role: &str) -> String {
    format!("'{}' hook --role {role}", ctx.exe)
}

/// Try numbers `0..n`, sleeping `secs` before every try but the first.
fn attempts(r: &dyn Runner, n: u32, secs: u64) -> impl Iterator<Item = u32> + '_ {
    (0..n).inspect(move |&i| {
        if i > 0 {
            r.sleep(secs);
        }
    })
}
