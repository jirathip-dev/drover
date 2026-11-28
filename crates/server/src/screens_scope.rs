use super::super::{lock, Later};
use super::{confirm, cut, fields, plain, rows_list, titled, wire, Config, Ctx, Event, Kind, Reply, Result, Server};
use crate::events::Json;
use blocks as k;
use serde_json::Value;
use std::sync::atomic::Ordering::Relaxed;

pub(super) fn card(now: u64, cfg: &Config, ev: &[Event]) -> Value {
    let mut rows = vec![];
    let mut folds = vec![];
    for fleet in cfg.fleet.keys() {
        let open = crate::scope::open(ev, fleet);
        let waits = open.iter().filter(|q| !crate::scope::started(ev, q)).count();
        folds.extend(match (open.len(), waits) {
            (0, _) => None,
            (n, w) if n == w => Some(format!("{fleet}: waits")),
            (_, 0) => Some(format!("{fleet}: scoping")),
            (_, w) => Some(format!("{fleet}: scoping, {w} waits")),
        });
        rows.extend(open.into_iter().map(|q| row(now, ev, fleet, q)));
    }
    if !rows.is_empty() {
        rows = vec![rows_list("scopes", rows)];
    }
    rows.push(composer(now, cfg, ev));
    let line = if folds.is_empty() {
        "+ new issue · a line or a screenshot, for any repo".into()
    } else {
        folds.join(" · ")
    };
    let card = k::Card::new()
        .id("new-issue")
        .blocks(rows)
        .fold(k::Fold { line, main: None });
    let mut v = wire(&card.block());
    v.put("kind", "new-issue".into());
    v
}

fn row(now: u64, ev: &[Event], fleet: &str, q: &Event) -> k::Block {
    let req = q.str("req").unwrap_or_default();
    let line = q.str("text").filter(|t| !t.is_empty()).unwrap_or("a screenshot");
    let sub = if crate::scope::started(ev, q) {
        format!("scoping · {}", crate::events::age(now.saturating_sub(q.ts)))
    } else {
        "waits · starts once the one above is filed or dropped".into()
    };
    let drop = k::Act {
        tone: Some(k::Tone::Bad),
        confirm: Some(k::Confirm {
            message: Some("Nothing is filed. Its scope lane exits.".into()),
            ..confirm("Drop this new issue?", "Drop")
        }),
        ..super::act(
            "Drop",
            "scope-drop",
            fields([("fleet", fleet.into()), ("req", req.into())]),
        )
    };
    titled(format!("scope-{fleet}-{req}"), format!("{fleet} · {}", cut(line, 60)))
        .subtitle(plain(sub))
        .swipe(vec![drop])
        .block()
}

fn composer(now: u64, cfg: &Config, ev: &[Event]) -> k::Block {
    let repos: Vec<String> = cfg.fleet.keys().cloned().collect();
    let last = (ev.iter().rev())
        .find(|e| e.kind == "scope-request")
        .map(|e| e.fleet.clone());
    let chip = k::ComposerChip {
        name: "repo".into(),
        cur: last.or_else(|| repos.first().cloned()),
        all: repos,
    };
    k::Composer::new("new-issue")
        .id("new-issue-line")
        .placeholder("one line: the bug or the feature")
        .chips(vec![chip])
        .values(fields([("req", format!("new-{now}").into())]))
        .block()
}

impl Server<'_> {
    pub(super) fn scope_op(&self, ctx: &Ctx, by: &str, name: &str, v: &Value) -> Result<Value> {
        let cfg = Config::kept(ctx)?;
        let s = |k: &str| v.at(k).as_str().unwrap_or_default();
        let fleet = if name == "new-issue" { s("repo") } else { s("fleet") };
        let toast = if name == "new-issue" {
            let files = super::super::chat::attachments(s("name"), s("data"))?;
            crate::scope::request(ctx, &cfg, fleet, (s("text"), &files), (s("req"), by))?
        } else {
            crate::scope::drop_request(ctx, &cfg, &self.r, fleet, s("req"))?
        };
        lock(&self.later).push((ctx.clone(), Later::Scope(fleet.into())));
        self.kick.store(true, Relaxed);
        Ok(Reply::done(vec![], toast, false))
    }
}
