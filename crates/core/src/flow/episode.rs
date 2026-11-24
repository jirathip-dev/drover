use crate::events::{last_spawn, Event, IssueState};
use std::collections::{BTreeMap, BTreeSet};

const HEADS: [&str; 4] = ["turn-end", "verdict", "verdict-seen", "verdict-refused"];

fn head_of(e: &Event) -> Option<&str> {
    HEADS
        .contains(&e.kind.as_str())
        .then(|| e.str("sha").or_else(|| e.str("head")))?
}

/// Who holds the issue's row now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Level {
    #[default]
    Auto,
    Orchestrator {
        ask: String,
    },
    Owner {
        card: String,
    },
}

/// The issue's episode as the log has it: what began it, this round's spawn, each row's failed tries, the row it is in and who holds it.
#[derive(Debug, Clone, Default)]
pub struct Episode {
    pub state: String,
    pub head: Option<String>,
    pub began: u64,
    pub spawn: Option<Event>,
    pub tries: BTreeMap<String, u8>,
    pub entered: Option<(String, u64)>,
    pub level: Level,
    pub answered: BTreeSet<String>,
}

impl Episode {
    /// Issue `s`'s episode: it begins at a new PR head, a move to another working state (a spawn) or the owner's decision.
    pub fn of(events: &[Event], s: &IssueState) -> Episode {
        let mine: Vec<&Event> = (events.iter())
            .filter(|e| e.fleet == s.fleet && e.issue == Some(s.issue))
            .collect();
        let head = mine.iter().rev().find_map(|e| head_of(e)).map(String::from);
        let orchestrator = |e: &Event| e.str("by").is_some_and(|b| b.starts_with("orchestrator-"));
        let begins = |(i, e): (usize, &&Event)| match e.kind.as_str() {
            "spawn" | "round-extended" | "unparked" => Some(i),
            "decision" | "row-retry" => (!orchestrator(e)).then_some(i),
            _ => head_of(e).filter(|h| Some(*h) != head.as_deref()).map(|_| i + 1),
        };
        let at = mine.iter().enumerate().rev().find_map(begins).unwrap_or(0);
        let began = mine.get(at).map_or(0, |e| e.ts);
        let mut ep = Episode {
            state: s.state.clone(),
            head,
            began,
            spawn: s.active_lane().and_then(|l| last_spawn(events, l.1)).cloned(),
            ..Episode::default()
        };
        for e in mine.iter().skip(at) {
            let row = e.str("row").unwrap_or_default().to_string();
            match e.kind.as_str() {
                "row-tried" if e.str("outcome") == Some("failed") => *ep.tries.entry(row).or_default() += 1,
                "row-entered" => ep.entered = Some((row, e.ts)),
                "row-escalated" => {
                    let ask = e.str("ask").unwrap_or_default().to_string();
                    ep.level = match e.str("level") {
                        Some("owner") => Level::Owner { card: ask },
                        _ => Level::Orchestrator { ask },
                    };
                }
                "row-retry" if orchestrator(e) => {
                    ep.tries.remove(&row);
                    ep.level = Level::Auto;
                    ep.answered.insert(row);
                }
                _ => {}
            }
        }
        ep
    }
}
