use super::episode::{Episode, Level};
use super::table::{Action, Escalate, Owner, Since, Table, Timeout};
use super::Cond;
use crate::config::Limits;
use crate::events::IssueState;

/// The times a row's clock may start from, as one pass's sample has them.
#[derive(Debug, Clone, Copy, Default)]
pub struct Clock {
    pub now: u64,
    pub heard: Option<u64>,
    pub frame: Option<u64>,
    pub reset: Option<u64>,
}

/// What `decide` says for an issue: its row, the try it is on, the action due now, the deadline and who holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub row: String,
    pub owner: Owner,
    pub act: Option<Action>,
    pub tries: u8,
    pub deadline: Option<u64>,
    pub expired: bool,
    pub level: Level,
    pub say: String,
}

fn secs(l: &Limits, t: Timeout) -> Option<u64> {
    let Timeout::Limit(k) = t else { return None };
    let v = serde_json::to_value(l).ok()?.get(k)?.as_u64()?;
    let unit = [("_hours", 3600), ("_minutes", 60)].iter().find(|u| k.ends_with(u.0));
    Some(v.saturating_mul(unit.map_or(1, |u| u.1)))
}

/// Issue `s`'s step under `cond`: the first matching row of its state's file, its try from `ep`, its deadline from `k`.
pub fn decide(t: &Table, s: &IssueState, cond: &Cond, ep: &Episode, k: &Clock, l: &Limits) -> Option<Step> {
    let name = s.state.split(['(', '+']).next().unwrap_or_default();
    let file = (t.flow("issue")?.files.iter()).find(|f| f.states.iter().any(|n| n == name))?;
    let row = file.row(cond)?;
    let entered = ep.entered.as_ref().filter(|e| e.0 == row.id).map_or(k.now, |e| e.1);
    let from = match row.since {
        Since::Entered => None,
        Since::Heard => k.heard,
        Since::HostHeard => k.frame,
        Since::Reset => k.reset,
    };
    let since = from.unwrap_or(entered).max(ep.last.get(&row.id).copied().unwrap_or(0));
    let deadline = secs(l, row.timeout).map(|t| since.saturating_add(t));
    let framed = |d: u64| row.since != Since::Heard || k.frame.is_some_and(|f| f >= d);
    let spent = ep.tries.get(&row.id).copied().unwrap_or(0);
    let tries = spent.saturating_add(1);
    let act = match tries {
        1 => Some(row.act),
        2 | 3 => Some(
            row.then
                .get(usize::from(spent).saturating_sub(1))
                .copied()
                .unwrap_or(row.act),
        ),
        _ => None,
    };
    let out = spent >= 3 || act == Some(Action::RaiseCard) || (row.escalate != Escalate::Ladder && spent >= 1);
    let late = ep
        .escalated
        .as_ref()
        .is_some_and(|e| k.now >= e.1.saturating_add(l.escalate_minutes * 60));
    let level = match &ep.level {
        Level::Orchestrator { .. } if late => Level::Owner { card: String::new() },
        _ if !out => ep.level.clone(),
        _ if row.escalate == Escalate::Owner || ep.answered.contains(&row.id) => Level::Owner { card: String::new() },
        x => x.clone().max(Level::Orchestrator { ask: String::new() }),
    };
    let owner = match level {
        Level::Auto => row.owner,
        Level::Orchestrator { .. } => Owner::Orchestrator,
        Level::Owner { .. } => Owner::Owner,
    };
    Some(Step {
        row: row.id.clone(),
        owner,
        act: act.filter(|a| *a != Action::Wait && level == Level::Auto),
        tries,
        deadline,
        expired: deadline.is_some_and(|d| k.now >= d && framed(d)),
        level,
        say: row.say.clone(),
    })
}
