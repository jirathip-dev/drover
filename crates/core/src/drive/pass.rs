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
}

pub(super) fn stash(ctx: &Ctx, cfg: &Config, fleet: &str, waits: &mut Vec<(IssueState, Waiting)>) {
    let kick = super::KICK.get();
    waits.retain(|(s, _)| kick.is_none_or(|k| k == s.issue));
    PLAN.set(Some(shadow::plan(ctx, cfg, fleet, kick, waits)));
}

pub(super) fn shadow(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, fleet: &str) -> Result<Vec<serde_json::Value>> {
    let acts = super::ACTS.with_borrow_mut(|m| m.remove(fleet)).unwrap_or_default();
    let Some(plan) = PLAN.take() else {
        return Ok(acts);
    };
    let pass = Pass::of(ctx, fleet, super::KICK.get());
    if let Err(e) = plan.and_then(|p| shadow::run(&pass, cfg, p, &acts)) {
        writeln!(out, "{fleet} shadow: {e:#}")?;
    }
    Ok(acts)
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
    #[cfg(test)]
    pub(crate) fn test(ctx: &'a Ctx, fleet: &'a str) -> Pass<'a> {
        Pass::of(ctx, fleet, None)
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
