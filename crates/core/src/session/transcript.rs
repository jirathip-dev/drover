use crate::runner::Runner;
use crate::Ctx;
use anyhow::{Context, Result};
use serde_json::json;
use shed_proto::{EventKind, Logged, Method, ReadParams, ReadResult};
use std::path::PathBuf;

/// A session's own log as shed keeps it: an ACP session's events, a PTY session's text.
#[derive(Clone, Debug, PartialEq)]
pub enum Transcript {
    Events(Vec<Logged>),
    Screen(String),
}

const LINES: usize = 5000;

fn file(ctx: &Ctx, agent: &str, ext: &str) -> Option<PathBuf> {
    let plain = !agent.starts_with('.') && agent.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    plain.then(|| ctx.state.with_file_name("transcripts").join(format!("{agent}.{ext}")))
}

/// Session `name`'s log from shed on `r` (events `before` that seq, the last `n`); None when shed has no such session.
pub fn read(r: &dyn Runner, name: &str, before: Option<u64>, n: Option<usize>) -> Result<Option<Transcript>> {
    match super::get(r, name)? {
        Some(s) => of(r, &s, before, n).map(Some),
        None => Ok(None),
    }
}

/// Session `s`'s log from shed on `r`, as `read` reads it.
pub fn of(r: &dyn Runner, s: &super::Session, before: Option<u64>, n: Option<usize>) -> Result<Transcript> {
    let name = &s.name;
    let w = r.shed().context("no shed runtime")?.wire;
    let p = ReadParams {
        name: name.clone(),
        lines: (!s.acp).then_some(LINES),
        history: !s.acp,
        events: s.acp,
        before: before.filter(|_| s.acp),
        n: n.filter(|_| s.acp),
    };
    let got: ReadResult = serde_json::from_value(crate::shed::call(r, &w, Method::Read, json!(p))?)?;
    Ok(if s.acp {
        Transcript::Events(got.events)
    } else {
        Transcript::Screen(got.text)
    })
}

/// Lane `agent`'s log: live from shed, else the copy `archive` kept; ACP events `before` that seq, the last `n`.
pub fn page(r: &dyn Runner, ctx: &Ctx, agent: &str, before: Option<u64>, n: usize) -> Option<Transcript> {
    match read(r, agent, before, Some(n)) {
        Ok(Some(t)) => Some(t),
        _ => match archived(ctx, agent)? {
            Transcript::Events(ev) => {
                let mut ev: Vec<Logged> = ev.into_iter().filter(|e| before.is_none_or(|b| e.seq < b)).collect();
                Some(Transcript::Events(ev.split_off(ev.len().saturating_sub(n))))
            }
            t @ Transcript::Screen(_) => Some(t),
        },
    }
}

/// Copy session `name`'s log to the state dir (mode 0600) before drover removes it from shed.
pub fn archive(r: &dyn Runner, ctx: &Ctx, name: &str) -> Result<()> {
    let (Some(t), Some(dir)) = (read(r, name, None, None)?, file(ctx, name, "txt")) else {
        return Ok(());
    };
    std::fs::create_dir_all(dir.parent().context("no transcripts dir")?)?;
    let (ext, body) = match &t {
        Transcript::Events(ev) => ("jsonl", ev.iter().map(|e| json!(e).to_string() + "\n").collect()),
        Transcript::Screen(text) => ("txt", text.clone()),
    };
    let path = file(ctx, name, ext).context("no transcript file")?;
    crate::devices::write0600(&path, body.as_bytes())?;
    Ok(())
}

/// The copy `archive` kept of lane `agent`'s log.
pub fn archived(ctx: &Ctx, agent: &str) -> Option<Transcript> {
    if let Some(t) = file(ctx, agent, "jsonl").and_then(|f| std::fs::read_to_string(f).ok()) {
        let ev = t.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        return Some(Transcript::Events(ev));
    }
    let t = std::fs::read_to_string(file(ctx, agent, "txt")?).ok()?;
    Some(Transcript::Screen(t))
}

/// Whether drover keeps a copy of lane `agent`'s log.
pub fn kept(ctx: &Ctx, agent: &str) -> bool {
    ["jsonl", "txt"]
        .iter()
        .any(|x| file(ctx, agent, x).is_some_and(|f| f.is_file()))
}

/// Drop the copy of lane `agent`'s log (its worktree went).
pub fn forget(ctx: &Ctx, agent: &str) {
    for x in ["jsonl", "txt"] {
        let _ = file(ctx, agent, x).map(std::fs::remove_file);
    }
}

/// A transcript's last reply line (ACP) or last text line (PTY), trimmed.
pub fn last_line(t: &Transcript) -> String {
    let reply = |e: &Logged| match &e.kind {
        EventKind::Reply { text, .. } => Some(text.clone()),
        _ => None,
    };
    let text = match t {
        Transcript::Events(ev) => ev.iter().rev().find_map(reply).unwrap_or_default(),
        Transcript::Screen(s) => s.clone(),
    };
    text.lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default()
        .into()
}

/// An ACP log's prompts, final replies and commands run (shell tool titles), masked: the next lane's handover.
pub fn said(t: &Transcript) -> [Vec<String>; 3] {
    let mut out: [Vec<String>; 3] = Default::default();
    let Transcript::Events(ev) = t else {
        return out;
    };
    let one = |s: &str| crate::lanes::mask(&s.split_whitespace().collect::<Vec<_>>().join(" "));
    let mut ids = std::collections::BTreeSet::new();
    for e in ev {
        match &e.kind {
            EventKind::Turn { prompt } => out[0].push(one(prompt)),
            EventKind::Reply { text, .. } if !text.trim().is_empty() => out[1].push(one(text)),
            EventKind::ToolCall { id, title, kind, .. } if kind.as_deref() == Some("execute") && ids.insert(id) => {
                out[2].push(one(title));
            }
            _ => {}
        }
    }
    out
}
