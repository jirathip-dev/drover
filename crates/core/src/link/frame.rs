use crate::shed::proto::TerminalInfo;
use serde::{Deserialize, Serialize};

/// The frame format this build writes and reads.
pub const V: u32 = 1;

/// One line a host prints: `drover hosts report` prints exactly one, a `Report`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub v: u32,
    pub boot: String,
    pub n: u64,
    pub at: u64,
    pub body: Body,
}

/// What a frame carries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Body {
    Report(Report),
}

/// The whole truth of one host: it replaces everything the main host knew of it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Report {
    pub build: String,
    pub shed: Option<String>,
    pub lanes: Vec<Seen>,
}

/// One session, its worktree, or both.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Seen {
    pub session: Option<TerminalInfo>,
    pub worktree: Option<Worktree>,
}

/// A lane worktree's facts as its host sees them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Worktree {
    pub path: String,
    pub head: Option<(String, u64)>,
    pub dirty: Option<String>,
    pub verdict: Option<Verdict>,
    pub lines: Vec<Line>,
}

/// A reviewer's verdict.md: when it was written, and its text.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    pub mtime: u64,
    pub text: String,
}

/// One outbox line and the worktree it came from.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Line {
    pub worktree: String,
    pub sent: u64,
    pub lane: String,
    pub args: Vec<String>,
}

/// The main host's side of the call, as the reporting host's stdin: it has no fleet.toml.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Watch {
    pub roots: Vec<String>,
    pub worktrees: Vec<String>,
}
