pub mod audit;
pub mod call;
pub mod probe;
pub mod start;

use crate::heavy::Heavy;
use crate::hook::{self, Call};
use crate::runner::Runner;
use crate::Ctx;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// One permission request shed's `judge` policy hands the hook's ACP mode on stdin.
#[derive(Debug, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub cwd: PathBuf,
    pub tool_call: Option<ToolCall>,
    #[serde(default)]
    pub options: Vec<Value>,
}

/// ACP's `toolCall`, as the agent sent it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    #[serde(default)]
    pub tool_call_id: String,
    pub title: Option<String>,
    pub kind: Option<Kind>,
    #[serde(default)]
    pub locations: Vec<Location>,
    pub raw_input: Option<Value>,
}

/// An ACP tool call's location.
#[derive(Debug, Clone, Deserialize)]
pub struct Location {
    pub path: PathBuf,
}

/// ACP's tool kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Read,
    Edit,
    Delete,
    Move,
    Search,
    Execute,
    Think,
    Fetch,
    SwitchMode,
    #[serde(other)]
    Other,
}

/// The judge's answer: one line on stdout.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Verdict {
    pub decision: Decision,
    pub reason: String,
}

/// Allow or deny.
#[derive(Debug, Serialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Allow,
    Deny,
}

impl Verdict {
    pub fn of(reason: Option<String>) -> Verdict {
        match reason {
            Some(r) => Verdict {
                decision: Decision::Deny,
                reason: format!("drover: {r}"),
            },
            None => Verdict {
                decision: Decision::Allow,
                reason: String::new(),
            },
        }
    }
}

/// How a lane's tool calls reach drover's rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guard {
    Judge,
    Hooked,
    Both,
}

impl Guard {
    pub fn word(self) -> &'static str {
        match self {
            Guard::Judge => "judge",
            Guard::Hooked => "hooked",
            Guard::Both => "both",
        }
    }
}

/// Agent `agent`'s guard under a runtime's `acp_policy`: judged unless its module has a hook format.
pub fn guard(policy: &str, agent: &str) -> Guard {
    match (crate::providers::of(agent).hook_dialect(), policy) {
        (None, _) => Guard::Judge,
        (Some(_), "judge") => Guard::Both,
        (Some(_), _) => Guard::Hooked,
    }
}

/// The judge command shed runs per request: this binary's hook in ACP mode.
pub fn command(ctx: &Ctx) -> String {
    format!("{} hook --acp", real(&ctx.exe))
}

fn real(path: &str) -> String {
    std::fs::canonicalize(path).map_or(path.into(), |p| p.to_string_lossy().into())
}

/// Why this host cannot start a judged lane: its shed has no `judge` policy, or drover's own path has a space.
pub fn ready(ctx: &Ctx, r: &dyn Runner) -> Result<(), String> {
    if !crate::shed::cli(r, &["help", "run"]).is_ok_and(|o| o.stdout.contains("--judge")) {
        return Err("this host's shed has no judge policy (`shed run --judge`); update shed".into());
    }
    let exe = real(&ctx.exe);
    if exe.contains(char::is_whitespace) {
        return Err(format!(
            "drover's path {exe} has a space; shed splits a judge command at spaces"
        ));
    }
    Ok(())
}

/// The hook's ACP mode on request `text`: Ok(None) allows, Ok(Some(reason)) denies, Err(cause) fails closed.
pub fn answer(
    ctx: &Ctx,
    r: &dyn Runner,
    text: &str,
    probe: Option<&std::path::Path>,
) -> Result<Option<String>, String> {
    let req: Request = serde_json::from_str(text).map_err(|e| format!("its input is not a judge request ({e})"))?;
    let call = call::of(&req);
    if let Some(dir) = probe {
        return Ok(Some(probe::seen(dir, call.as_ref().map_err(String::as_str))));
    }
    let call = call?;
    match call {
        Call::Free => return Ok(None),
        Call::Mode => return Ok(Some(hook::MODE_SWITCH.into())),
        _ => {}
    }
    let lane = crate::config::env("DROVER_LANE")
        .filter(|l| !l.is_empty())
        .ok_or("no DROVER_LANE")?;
    let cwd = req.cwd.to_string_lossy();
    let (policy, heavy) = hook::policy(ctx, r, &lane, false, Some(&cwd))?;
    let git = |d: &str, a: &[&str]| {
        crate::runner::ok(r, "git", &[&["-C", d][..], a].concat())
            .ok()
            .map(|s| s.trim().to_string())
    };
    Ok(hook::decide(&call, &policy, &git).or_else(|| heavy_line(&call, heavy.as_ref(), &ctx.exe, &git)))
}

fn heavy_line(
    call: &Call,
    h: Option<&Heavy>,
    exe: &str,
    git: &dyn Fn(&str, &[&str]) -> Option<String>,
) -> Option<String> {
    let (Call::Shell { cmd, cwd }, Some(h)) = (call, h) else {
        return None;
    };
    let mut w = cmd.split_whitespace().map(|w| w.trim_matches(['\'', '"']));
    if w.next().is_some_and(|x| x.ends_with("drover")) && w.next() == Some("heavy") {
        return None;
    }
    let kind = h.kind(cmd)?;
    let dir = (h.routes.contains_key(kind))
        .then(|| git(cwd, &["rev-parse", "--show-toplevel"]))
        .flatten()
        .filter(|t| !t.is_empty());
    let line = h.wrap(exe, kind, cmd, dir.as_deref());
    Some(format!(
        "REFUSED HEAVY_SLOT: a heavy command runs in a host slot; run it as: {line}"
    ))
}
