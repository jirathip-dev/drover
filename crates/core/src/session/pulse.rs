use crate::Ctx;
use serde::{Deserialize, Serialize};
use shed_proto::EventKind;
use std::collections::BTreeMap;

/// An ACP session's pulse from shed's events only (#785, #792, #819, #946), kept in `shed-out.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pulse {
    #[serde(rename = "turn")]
    pub turn_open: bool,
    #[serde(rename = "start", skip_serializing_if = "Option::is_none")]
    pub first_turn: Option<u64>,
    #[serde(rename = "at")]
    pub heard: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub said: Option<u64>,
    #[serde(rename = "tools")]
    pub open_tools: BTreeMap<String, String>,
    #[serde(rename = "cpu", skip_serializing_if = "Option::is_none")]
    pub cpu_pct: Option<u64>,
}

fn path(ctx: &Ctx) -> std::path::PathBuf {
    ctx.state.with_file_name("shed-out.json")
}

fn load(ctx: &Ctx) -> BTreeMap<String, Pulse> {
    let t = std::fs::read_to_string(path(ctx)).unwrap_or_default();
    let all: BTreeMap<String, serde_json::Value> = serde_json::from_str(&t).unwrap_or_default();
    all.into_iter()
        .filter_map(|(k, v)| Some((k, serde_json::from_value(v).ok()?)))
        .collect()
}

/// Lane `agent`'s pulse as drive's intake last saw it; None before its first ACP event.
pub fn of(ctx: &Ctx, agent: &str) -> Option<Pulse> {
    load(ctx).remove(agent)
}

/// #785: the ACP lanes with no event back `mins` after their first turn: (agent, when that turn went in).
pub fn silent(ctx: &Ctx, mins: u64) -> Vec<(String, u64)> {
    let start = |(a, p): (String, Pulse)| Some((a, p.first_turn?));
    load(ctx)
        .into_iter()
        .filter_map(start)
        .filter(|s| ctx.now >= s.1 + mins * 60)
        .collect()
}

/// Change lane `agent`'s pulse (`None`: forget it); the one before.
pub fn update(ctx: &Ctx, agent: &str, f: impl FnOnce(&mut Option<Pulse>)) -> std::io::Result<Option<Pulse>> {
    let mut all = load(ctx);
    let was = all.remove(agent);
    let mut now = was.clone();
    f(&mut now);
    all.extend(now.map(|p| (agent.to_string(), p)));
    std::fs::write(path(ctx), serde_json::to_string(&all).unwrap_or_default())?;
    Ok(was)
}

/// One ACP event of `agent`'s into its pulse (a `replay` moves only the turn).
pub fn hear(ctx: &Ctx, agent: &str, kind: &EventKind, replay: bool) -> std::io::Result<()> {
    let now = ctx.now;
    update(ctx, agent, |p| {
        if let EventKind::Resources(u) = kind {
            if let Some(p) = p {
                p.cpu_pct = Some(u.cpu_pct);
            }
            return;
        }
        let turn = matches!(
            kind,
            EventKind::Turn { .. } | EventKind::Text { .. } | EventKind::Reply { .. }
        );
        if !turn && (replay || !kind.is_acp()) {
            return;
        }
        let p = p.get_or_insert_with(Pulse::default);
        if kind.is_acp() && !replay {
            p.heard = now;
            match kind {
                EventKind::Turn { .. } => {}
                EventKind::ToolCall { id, status, .. } if matches!(status.as_str(), "pending" | "in_progress") => {
                    p.said = Some(now);
                    p.open_tools.insert(id.clone(), status.clone());
                }
                EventKind::ToolCall { id, .. } => {
                    p.said = Some(now);
                    p.open_tools.remove(id);
                }
                EventKind::Reply { .. } => {
                    p.said = Some(now);
                    p.open_tools.clear();
                }
                _ => p.said = Some(now),
            }
        }
        match kind {
            EventKind::Turn { .. } => (p.turn_open, p.first_turn) = (true, Some(p.first_turn.unwrap_or(now))),
            EventKind::Text { .. } => (p.turn_open, p.first_turn) = (true, None),
            EventKind::Reply { .. } => (p.turn_open, p.first_turn) = (false, None),
            _ => {}
        }
    })
    .map(drop)
}
