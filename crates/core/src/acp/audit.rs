use super::{Kind, Location, Request, ToolCall};
use crate::events::Event;
use crate::hook::Call;
use crate::runner::Runner;
use crate::Ctx;
use serde_json::Value;

/// A tool call shed reported, as its `tool_call` event carries it.
pub struct Ran<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub kind: Option<&'a str>,
    pub locations: &'a [std::path::PathBuf],
    pub raw_input: Option<&'a Value>,
}

impl Ran<'_> {
    /// A call that changes something: an `execute`, `edit`, `delete` or `move`.
    pub fn acts(&self) -> bool {
        matches!(self.kind, Some("execute" | "edit" | "delete" | "move"))
    }

    /// The rules' call for it, judged in `cwd`; None when shed's record cannot be read.
    pub fn call(&self, cwd: &str) -> Option<Call> {
        let kind = self
            .kind
            .and_then(|k| serde_json::from_value::<Kind>(Value::from(k)).ok());
        let tool_call = ToolCall {
            tool_call_id: self.id.into(),
            title: Some(self.title.into()),
            kind,
            locations: self.locations.iter().map(|p| Location { path: p.clone() }).collect(),
            raw_input: self.raw_input.cloned(),
        };
        let req = Request {
            name: String::new(),
            labels: std::collections::BTreeMap::new(),
            cwd: cwd.into(),
            tool_call: Some(tool_call),
            options: vec![],
        };
        super::call::of(&req).ok()
    }
}

/// The rule an unasked call of lane spawn `sp` breaks, replayed on drover's rules; None when none does or it cannot be read.
pub fn replay(ctx: &Ctx, r: &dyn Runner, sp: &Event, ran: &Ran) -> Option<String> {
    let cwd = sp.str("worktree").unwrap_or(".");
    let call = ran.call(cwd)?;
    let lane = format!("{}:{}:{}", sp.fleet, sp.issue?, sp.str("role")?);
    let (policy, _) = crate::hook::policy(ctx, r, &lane, false, Some(cwd)).ok()?;
    let git = |d: &str, a: &[&str]| {
        crate::runner::ok(r, "git", &[&["-C", d][..], a].concat())
            .ok()
            .map(|s| s.trim().to_string())
    };
    crate::hook::decide(&call, &policy, &git)
}
