use crate::events::{self, Event};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub mod route;

/// Where a row goes: a thread by id, a repo's desk thread, or the owner alone.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "k", rename_all = "kebab-case")]
pub enum Target {
    Thread {
        id: String,
    },
    Desk {
        repo: String,
    },
    #[default]
    Owner,
}

/// What a row is; for filters and buttons, never for workflow.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum RowKind {
    Ask,
    Decision,
    Handoff,
    RunCard,
    ConfigCard,
    SecretCard,
    #[default]
    System,
    Merged,
    Rollout,
    Digest,
    Alert,
    Said,
}

/// A notification's words; a newer one with the same `tag` replaces the older.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Push {
    pub title: String,
    pub body: String,
    pub tag: String,
}

/// One thing a thread shows, made from one logged event (`seq`, `ts`), without its block.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(optional_fields = nullable, rename = "RowData"))]
pub struct Row {
    pub seq: u64,
    pub ts: u64,
    pub key: String,
    pub to: Target,
    pub kind: RowKind,
    pub fleet: String,
    pub issue: Option<u64>,
    pub text: String,
    pub open: bool,
    pub prompt: Option<String>,
    pub push: Option<Push>,
}

/// The threads as the log up to some seq has them: open ones by repo, each repo's desk, each issue's filing thread.
#[derive(Default)]
pub struct Registry {
    open: BTreeMap<String, String>,
    desks: BTreeMap<String, String>,
    homes: BTreeMap<(String, u64), String>,
}

impl Registry {
    /// The registry of the whole of `events`.
    pub fn read(events: &[Event]) -> Self {
        let mut r = Self::default();
        for e in events {
            r.add(e);
        }
        r
    }

    /// Fold in one more event.
    pub fn add(&mut self, e: &Event) {
        let id = e.str("thread").unwrap_or_default().to_string();
        match e.kind.as_str() {
            "thread-new" | "orchestrator-attached" | "thread-restore" => {
                self.open.insert(id, e.fleet.clone());
            }
            "thread-archive" | "orchestrator-detached" => {
                self.open.remove(&id);
            }
            "thread-facts" => {
                let repo = e.str("profile").unwrap_or_default().to_string();
                self.desks.retain(|_, d| *d != id);
                if e.val("archived") == true {
                    self.open.remove(&id);
                } else {
                    if e.val("desk") == true {
                        self.desks.insert(repo.clone(), id.clone());
                    }
                    self.open.insert(id, repo);
                }
            }
            "issue-filed" => {
                let (t, n) = (e.str("filed_in").unwrap_or_default(), e.issue.unwrap_or_default());
                self.homes.entry((e.fleet.clone(), n)).or_insert_with(|| t.into());
            }
            "epic" if e.str("op") == Some("add") && !id.is_empty() => {
                let p = e.val("parent").as_u64().unwrap_or_default();
                self.homes.entry((e.fleet.clone(), p)).or_insert_with(|| id.clone());
            }
            "queue" if e.str("op") == Some("add") && !id.is_empty() => {
                for n in e
                    .val("issues")
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(serde_json::Value::as_u64)
                {
                    self.homes.entry((e.fleet.clone(), n)).or_insert_with(|| id.clone());
                }
            }
            _ => {}
        }
    }

    /// Thread `id`'s repo (a fleet, `all` or `none`) while it is open.
    pub fn repo(&self, id: &str) -> Option<&str> {
        self.open.get(id).map(String::as_str)
    }

    /// The repo's desk thread, while one is open.
    pub fn desk(&self, repo: &str) -> Option<&str> {
        self.desks
            .get(repo)
            .map(String::as_str)
            .filter(|d| self.repo(d).is_some())
    }

    /// Where `fleet` #`issue`'s rows go: the open thread that filed or queued it (not a `none` one), else the desk.
    pub fn home(&self, fleet: &str, issue: Option<u64>) -> Target {
        let t = issue.and_then(|n| self.homes.get(&(fleet.into(), n)));
        match t.filter(|t| self.repo(t).is_some_and(|r| r != "none")) {
            Some(id) => Target::Thread { id: id.clone() },
            None => Target::Desk { repo: fleet.into() },
        }
    }
}

/// In an orchestrator's lane, the thread it talks in (`DROVER_THREAD`): every event it logs carries it (`db::insert`).
pub fn stamp() -> Option<String> {
    let lane = crate::config::env("DROVER_LANE")?;
    let on = crate::orchestrator::is_role(lane.split(':').nth(2)?);
    crate::config::env("DROVER_THREAD").filter(|t| on && !t.is_empty())
}

/// Every row the log makes, oldest first, each with the event it is about (an ask's, a card's).
pub fn rows(events: &[Event]) -> Vec<(&Event, Row)> {
    let mut reg = Registry::default();
    let mut out = vec![];
    for e in events {
        reg.add(e);
        if let Some(r) = row(events, &reg, e) {
            out.push(r);
        }
    }
    out
}

/// Every row whose key is still open, its newest row, with no `prompt` and no `push`.
pub fn open(events: &[Event]) -> Vec<(&Event, Row)> {
    let mut last = BTreeMap::new();
    for (i, (s, r)) in rows(events).into_iter().enumerate() {
        last.insert(r.key.clone(), (i, s, r));
    }
    let mut v: Vec<_> = last.into_values().filter(|r| r.2.open).collect();
    v.sort_by_key(|r| r.0);
    v.into_iter()
        .map(|(_, s, mut r)| {
            (r.prompt, r.push) = (None, None);
            (s, r)
        })
        .collect()
}

fn card_kind(kind: &str) -> RowKind {
    match kind {
        "run-card" => RowKind::RunCard,
        "config-propose" => RowKind::ConfigCard,
        "secret-request" => RowKind::SecretCard,
        _ => RowKind::Rollout,
    }
}

fn row<'a>(events: &'a [Event], reg: &Registry, e: &'a Event) -> Option<(&'a Event, Row)> {
    let s = |k: &str| e.str(k).unwrap_or_default().to_string();
    let thread = |id: &str| Target::Thread { id: id.into() };
    let home = e.str("thread").map_or_else(|| reg.home(&e.fleet, e.issue), thread);
    let ask = |kind: &str| events::ask_event(events, kind, &e.fleet, e.str("id").unwrap_or_default());
    let card = |kind: &str| events.iter().find(|c| c.kind == kind && c.str("id") == e.str("id"));
    let (mut subject, mut to, mut open, mut prompt) = (e, home, false, None);
    let kind = match (e.kind.as_str(), e.str("from")) {
        ("needs-decision" | "ask-owner", _) => {
            subject = if e.kind == "ask-owner" {
                ask("needs-decision")?
            } else {
                e
            };
            open = events::closed(events, subject).is_none();
            RowKind::Ask
        }
        ("decision", _) => {
            subject = ask("needs-decision")?;
            to = subject
                .str("thread")
                .map_or_else(|| reg.home(&e.fleet, subject.issue), thread);
            prompt = subject
                .str("thread")
                .map(|_| format!("DECISION {}: {}", s("id"), s("choice")));
            RowKind::Decision
        }
        ("thread-handoff", _) => {
            (to, prompt) = (
                thread(&s("to")),
                Some(format!("[{}] {}: {}", s("to"), s("from"), s("text"))),
            );
            RowKind::Handoff
        }
        ("run-card" | "config-propose" | "secret-request" | "rollout-open", _) => {
            open = true;
            card_kind(&e.kind)
        }
        ("run-settled" | "config-settled", _) => {
            subject = card(if e.kind == "run-settled" {
                "run-card"
            } else {
                "config-propose"
            })?;
            to = subject.str("thread").map_or(to, thread);
            card_kind(&subject.kind)
        }
        ("secret-set" | "secret-dismissed", _) => RowKind::SecretCard,
        ("rollout-done" | "rollout-dismissed", _) => RowKind::Rollout,
        ("nudge", _) => RowKind::Alert,
        ("merged" | "epic-landed", _) => RowKind::Merged,
        ("thread-msg", Some("system")) => {
            prompt = Some(s("text"));
            RowKind::System
        }
        ("thread-msg", Some(f)) if f != "orchestrator" && !crate::orchestrator::is_id(f) => {
            prompt = (e.str("via") != Some("client")).then(|| s("text"));
            RowKind::Said
        }
        _ if e.kind != "thread-msg" && e.is_push() => {
            let stuck = e.kind == "needs-human" && e.str("code") == Some("STUCK");
            to = if !stuck && (e.issue.is_some() || e.data.contains_key("thread")) {
                to
            } else {
                Target::Owner
            };
            RowKind::Alert
        }
        _ => return None,
    };
    let key = match kind {
        RowKind::Ask | RowKind::Decision => format!("ask-{}-{}", e.fleet, s("id")),
        RowKind::RunCard => format!("run-{}", s("id")),
        RowKind::ConfigCard => format!("config-{}", s("id")),
        RowKind::SecretCard => format!("secret-{}-{}", s("thread"), s("name")),
        RowKind::Rollout => format!("rollout-{}-{}", e.fleet, e.issue.unwrap_or_default()),
        RowKind::Merged if e.kind == "epic-landed" => format!("epic-{}-{}", e.fleet, e.issue.unwrap_or_default()),
        k => format!(
            "{}-{}",
            serde_json::to_value(k).unwrap_or_default().as_str().unwrap_or_default(),
            e.seq
        ),
    };
    let text = match kind {
        RowKind::Ask => format!("{} · {}", s("id"), subject.str("question").unwrap_or_default()),
        RowKind::System | RowKind::Said => s("text"),
        _ => events::plain(e),
    };
    let rings = match e.kind.as_str() {
        "needs-decision" => s("kind") != "routine" && e.str("routed").is_none(),
        "decision" => s("by") == "default",
        "ask-owner" | "run-card" | "config-propose" | "secret-request" => true,
        _ => e.is_push() && e.str("via") != Some("client"),
    };
    let push = rings.then(|| Push {
        title: e.issue.map_or(e.fleet.clone(), |n| format!("{} #{n}", e.fleet)),
        body: text.clone(),
        tag: key.clone(),
    });
    let row = Row {
        seq: e.seq,
        ts: e.ts,
        key,
        to,
        kind,
        fleet: e.fleet.clone(),
        issue: e.issue.or(subject.issue),
        text,
        open,
        prompt,
        push,
    };
    Some((subject, row))
}
