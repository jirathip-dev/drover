use crate::config::Config;
use crate::events::IssueState;
use crate::flow::shadow::{self, Plan};
use crate::waiting::Waiting;
use crate::Ctx;
use anyhow::Result;
use std::cell::RefCell;
use std::io::Write;

thread_local! {
    static PLAN: RefCell<Option<Result<Plan>>> = const { RefCell::new(None) };
    static ACTS: RefCell<Vec<serde_json::Value>> = const { RefCell::new(Vec::new()) };
}

/// Keep a kick's own issue in today's `waits`, and plan the table's steps beside them for the shadow run after the pass body.
pub(super) fn stash(ctx: &Ctx, cfg: &Config, fleet: &str, waits: &mut Vec<(IssueState, Waiting)>) {
    let kick = super::KICK.get();
    waits.retain(|(s, _)| kick.is_none_or(|k| k == s.issue));
    PLAN.set(Some(shadow::plan(ctx, cfg, fleet, kick, waits)));
}

/// The pass body's actions, for the shadow run that follows it.
pub(super) fn acted(acts: &[serde_json::Value]) {
    ACTS.set(acts.to_vec());
}

/// The shadow run of the pass that just ran, under its lease and after its last host sample; an error is printed, never the pass's.
pub(super) fn shadow(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, fleet: &str) -> Result<()> {
    let acts = ACTS.take();
    let Some(plan) = PLAN.take() else {
        return Ok(());
    };
    let pass = Pass::of(ctx, fleet, super::KICK.get());
    if let Err(e) = plan.and_then(|p| shadow::run(&pass, cfg, p, &acts)) {
        writeln!(out, "{fleet} shadow: {e:#}")?;
    }
    Ok(())
}

/// A held pass or kick lease; only `drive` builds one, so only a pass or kick writes the flow table's events.
pub struct Pass<'a> {
    ctx: &'a Ctx,
    fleet: &'a str,
    kick: Option<u64>,
}

impl<'a> Pass<'a> {
    fn of(ctx: &'a Ctx, fleet: &'a str, kick: Option<u64>) -> Pass<'a> {
        Pass { ctx, fleet, kick }
    }
    pub fn ctx(&self) -> &Ctx {
        self.ctx
    }
    pub fn fleet(&self) -> &str {
        self.fleet
    }
    /// The kicked issue, when this is a kick's lease.
    pub fn kick(&self) -> Option<u64> {
        self.kick
    }
}
