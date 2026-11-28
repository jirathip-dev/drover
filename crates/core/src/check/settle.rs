use crate::config::Config;
use crate::events::{emit, Event};
use crate::Ctx;
use anyhow::Result;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

#[derive(Default)]
pub(super) struct Settled {
    cleaned: BTreeMap<String, u64>,
    roots: BTreeMap<String, usize>,
    hosts: BTreeMap<(String, &'static str), (usize, usize)>,
}

fn last<'e>(marks: impl Iterator<Item = &'e Event>) -> BTreeMap<String, u64> {
    let mut m: BTreeMap<String, u64> = BTreeMap::new();
    for c in marks {
        let t = m.entry(c.str("worktree").unwrap_or_default().into()).or_default();
        *t = (*t).max(c.ts);
    }
    m
}

fn root_of(wt: &str) -> String {
    let p = Path::new(wt);
    p.parent().and_then(Path::parent).unwrap_or(p).display().to_string()
}

pub(super) fn load(
    ctx: &Ctx,
    events: &[Event],
    (fleet, issue): (Option<&str>, Option<u64>),
    orphans: bool,
) -> Result<(Vec<Event>, Settled)> {
    let parked: Vec<Event> = (crate::events::since(&ctx.state, &["clean-settled"], 0)?.into_iter())
        .filter(|c| fleet.is_none_or(|f| f == c.fleet) && c.issue.is_some_and(|n| issue.is_none_or(|i| i == n)))
        .map(|c| Event::new(c.ts, &c.fleet, c.issue, "spawn", Value::Object(c.data)))
        .collect();
    let gone = last(events.iter().filter(|c| c.kind == "clean"));
    let settled = (events.iter().filter(|c| c.kind == "clean-settled").chain(&parked)).filter(|_| !orphans);
    let mut s = Settled {
        cleaned: last(events.iter().filter(|c| c.kind == "clean").chain(settled)),
        ..Settled::default()
    };
    for c in parked.iter().filter(|_| !orphans) {
        let wt = c.str("worktree").unwrap_or_default();
        if gone.get(wt).is_none_or(|t| *t < c.ts) && Path::new(wt).exists() {
            *s.roots.entry(root_of(wt)).or_default() += 1;
        }
    }
    Ok((parked, s))
}

impl Settled {
    pub(super) fn fresh(&self, e: &Event) -> bool {
        e.str("worktree")
            .and_then(|w| self.cleaned.get(w))
            .is_none_or(|t| *t < e.ts)
    }

    pub(super) fn checked(&mut self, host: Option<&str>, done: bool) {
        let c = self.hosts.entry((host.unwrap_or("this host").into(), "")).or_default();
        *if done { &mut c.1 } else { &mut c.0 } += 1;
    }

    pub(super) fn hold(
        &mut self,
        (ctx, cfg, out): (&Ctx, &Config, &mut dyn Write),
        (e, host, why): (&Event, Option<&str>, &'static str),
        (verbose, yes): (bool, bool),
    ) -> Result<()> {
        let wt = e.str("worktree").unwrap_or_default();
        if verbose {
            let shown = host.map_or(wt.into(), |h| format!("{h}:{wt}"));
            writeln!(out, "keep {shown}: {why} (not cleaned)")?;
        }
        if why != "off-root" {
            let host = host.unwrap_or("this host");
            let why = match why {
                "host unreachable" if cfg.hosts.get(host).is_some_and(|h| !h.enabled) => "host off",
                "host unreachable" => "host offline",
                w => w,
            };
            self.hosts.entry((host.into(), why)).or_default().0 += 1;
            return Ok(());
        }
        *self.roots.entry(root_of(wt)).or_default() += 1;
        if yes {
            let mut d = e.data.clone();
            d.insert("why".into(), why.into());
            let ev = Event::new(ctx.now, &e.fleet, e.issue, "clean-settled", Value::Object(d));
            emit(&ctx.state, cfg, ev)?;
        }
        Ok(())
    }

    pub(super) fn summary(&self, out: &mut dyn Write, yes: bool) -> Result<()> {
        let did = if yes { "removed" } else { "would remove" };
        for ((host, why), (n, done)) in &self.hosts {
            match *why {
                "" if *done == 0 => writeln!(out, "{host}: {n} lanes checked")?,
                "" => writeln!(out, "{host}: {n} lanes checked, {done} {did}")?,
                why => writeln!(out, "{host}: {n} lanes, {why} — not checked")?,
            }
        }
        for (root, n) in &self.roots {
            writeln!(out, "orphan root {root}: {n} worktrees off-root; `drover clean --orphans --yes` saves them to wip/, then removes them")?;
        }
        Ok(())
    }
}
