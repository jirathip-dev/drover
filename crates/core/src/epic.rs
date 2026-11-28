use crate::config::Config;
use crate::events::{fold, of_kind, working, Event, Json, States};
use crate::gate::refused;
use crate::runner::{argv, ok, Runner};
use crate::Ctx;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

/// Each open parent's children, in issue order: the `epic` events of one fleet, replayed.
pub type Members = BTreeMap<u64, BTreeSet<u64>>;

/// A child, by the state its own issue has; `t` is when it got there.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "is", rename_all = "kebab-case")]
pub enum Member {
    Merged { issue: u64, pr: Option<u64>, t: u64 },
    Dropped { issue: u64, t: u64 },
    Open { issue: u64, state: String, t: u64 },
}

impl Member {
    /// Its issue number.
    pub fn issue(&self) -> u64 {
        match self {
            Member::Merged { issue, .. } | Member::Dropped { issue, .. } | Member::Open { issue, .. } => *issue,
        }
    }
}

/// One open epic as every view shows it: the parent, its title, when its first child was added, its children.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Epic {
    pub fleet: String,
    pub parent: u64,
    pub title: String,
    pub since: u64,
    pub children: Vec<Member>,
}

/// A merged child as `epic-landed` carries it.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kid {
    pub issue: u64,
    pub pr: Option<u64>,
    pub title: String,
}

/// What `epic-landed` carries; the comment and the row's block are both written from it.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Landed {
    pub title: String,
    pub merged: Vec<Kid>,
    pub dropped: Vec<u64>,
}

/// An `epic` verb's change: children added or removed.
#[derive(clap::Subcommand)]
pub enum Op {
    #[command(about = "Add child issues to the epic")]
    Add { issues: Vec<u64> },
    #[command(about = "Remove child issues from the epic (they stay queued)")]
    Rm { issues: Vec<u64> },
}

/// `drover epic <fleet> <parent> add|rm <n>…`.
#[derive(clap::Args)]
pub struct Args {
    pub fleet: String,
    pub parent: u64,
    #[command(subcommand)]
    pub op: Op,
}

/// Run `drover epic`: the line `change` returns.
pub fn cli(ctx: &Ctx, cfg: &Config, r: &dyn Runner, a: Args) -> Result<String> {
    change(ctx, cfg, r, &a.fleet, a.parent, a.op)
}

/// `queue add`'s epic refusals: none of `issues` is an open epic's parent, and with `--epic` the add would pass `check`.
pub fn queueable(ctx: &Ctx, fleet: &str, issues: &[u64], epic: Option<u64>) -> Result<()> {
    let events = working(&ctx.state)?;
    let st = fold(&events);
    if let Some(n) = issues.iter().find(|n| is_parent(&events, &st, fleet, **n)) {
        return refused(
            "EPIC_PARENT",
            format!("{fleet}#{n} is an open epic's parent: drover never builds it"),
        );
    }
    epic.map_or(Ok(()), |p| {
        check(
            &events,
            &st,
            fleet,
            p,
            &Op::Add {
                issues: issues.to_vec(),
            },
        )
    })
}

impl Epic {
    fn count(&self, f: impl Fn(&Member) -> bool) -> usize {
        self.children.iter().filter(|c| f(c)).count()
    }

    /// `k`: the merged children.
    pub fn merged(&self) -> usize {
        self.count(|c| matches!(c, Member::Merged { .. }))
    }

    /// The children closed unmerged.
    pub fn dropped(&self) -> usize {
        self.count(|c| matches!(c, Member::Dropped { .. }))
    }

    /// `n`: the children not dropped.
    pub fn total(&self) -> usize {
        self.children.len().saturating_sub(self.dropped())
    }

    /// No child open and one or more merged.
    pub fn lands(&self) -> bool {
        self.merged() > 0 && self.merged() == self.total()
    }

    fn open_in(&self, f: impl Fn(&str) -> bool) -> usize {
        self.count(|c| matches!(c, Member::Open { state, .. } if f(state)))
    }

    /// The counts after `k/n merged`, zero ones left out: `2 in progress · 1 dropped`.
    pub fn counts(&self) -> Vec<String> {
        let queued = |s: &str| s == "queued";
        let not = |s: &str| s == "not queued";
        [
            (self.open_in(|s| !queued(s) && !not(s)), "in progress"),
            (self.open_in(queued), "queued"),
            (self.open_in(not), "not queued"),
            (self.dropped(), "dropped"),
        ]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, w)| format!("{n} {w}"))
        .collect()
    }

    /// The one line `drover waiting` and the compact board print.
    pub fn line(&self) -> String {
        let hash = |c: &Member| format!("#{}", c.issue());
        let open: Vec<String> = (self.children.iter().filter(|c| matches!(c, Member::Open { .. })))
            .map(hash)
            .collect();
        let gone: Vec<String> = (self.children.iter().filter(|c| matches!(c, Member::Dropped { .. })))
            .map(hash)
            .collect();
        let mut words = vec![format!(
            "epic {} #{} {} · {}/{} merged",
            self.fleet,
            self.parent,
            self.title,
            self.merged(),
            self.total()
        )];
        words.extend(self.counts());
        let mut tail = vec![];
        if !open.is_empty() {
            tail.push(format!("open {}", open.join(" ")));
        }
        if !gone.is_empty() {
            tail.push(format!("({} dropped)", gone.join(" ")));
        }
        if !tail.is_empty() {
            words.push(tail.join(" "));
        }
        words.join(" · ")
    }

    fn landed(&self, ctx: &Ctx) -> Landed {
        let kid = |c: &Member| match c {
            Member::Merged { issue, pr, .. } => Some(Kid {
                issue: *issue,
                pr: *pr,
                title: crate::gh::cached_title(ctx, &self.fleet, *issue).unwrap_or_default(),
            }),
            _ => None,
        };
        Landed {
            title: self.title.clone(),
            merged: self.children.iter().filter_map(kid).collect(),
            dropped: (self.children.iter().filter(|c| matches!(c, Member::Dropped { .. })))
                .map(Member::issue)
                .collect(),
        }
    }
}

fn adds<'a>(events: &'a [Event], fleet: &'a str) -> impl Iterator<Item = &'a Event> {
    of_kind(events, &["epic"]).into_iter().filter(move |e| e.fleet == fleet)
}

/// The fleet's `epic` events folded: `add` inserts, `rm` removes, an empty parent drops out.
pub fn replay(events: &[Event], fleet: &str) -> Members {
    let mut m = Members::new();
    for e in adds(events, fleet) {
        let Some(p) = e.val("parent").as_u64() else { continue };
        let kids = crate::queue::nums(e.val("issues"));
        let set = m.entry(p).or_default();
        match e.str("op") {
            Some("add") => set.extend(kids),
            Some("rm") => set.retain(|n| !kids.contains(n)),
            _ => {}
        }
        m.retain(|_, s| !s.is_empty());
    }
    m
}

fn done(st: &States, fleet: &str, n: u64) -> bool {
    st.get(&(fleet.to_string(), n))
        .is_some_and(crate::events::IssueState::done)
}

fn opened(events: &[Event], st: &States, fleet: &str) -> Members {
    let mut m = replay(events, fleet);
    m.retain(|p, _| !done(st, fleet, *p));
    m
}

/// The open epic `n` is a child of.
pub fn parent_of(events: &[Event], st: &States, fleet: &str, n: u64) -> Option<u64> {
    opened(events, st, fleet)
        .into_iter()
        .find(|(_, s)| s.contains(&n))
        .map(|x| x.0)
}

/// `n` is an open epic's parent.
pub fn is_parent(events: &[Event], st: &States, fleet: &str, n: u64) -> bool {
    opened(events, st, fleet).contains_key(&n)
}

fn child(events: &[Event], st: &States, fleet: &str, n: u64) -> Member {
    let queued = || crate::queue::replay(events, fleet).iter().any(|i| i.issue == n);
    match st.get(&(fleet.to_string(), n)) {
        Some(s) if s.state == "merged" => Member::Merged {
            issue: n,
            pr: s.pr,
            t: s.last_ts,
        },
        Some(s) if s.state == "closed" => Member::Dropped { issue: n, t: s.last_ts },
        Some(s) => Member::Open {
            issue: n,
            state: s.shown(),
            t: s.last_ts,
        },
        None => Member::Open {
            issue: n,
            state: if queued() { "queued" } else { "not queued" }.into(),
            t: 0,
        },
    }
}

/// Each open epic of `fleet`, its children classed by their own issues' states.
pub fn open(ctx: &Ctx, events: &[Event], st: &States, fleet: &str) -> Vec<Epic> {
    let first = |p: u64| adds(events, fleet).filter(move |e| e.val("parent").as_u64() == Some(p));
    let title = |p: u64| {
        first(p)
            .filter_map(|e| e.str("title"))
            .filter(|t| !t.is_empty())
            .last()
            .map(String::from)
    };
    let epic = |(p, kids): (u64, BTreeSet<u64>)| Epic {
        fleet: fleet.into(),
        parent: p,
        title: title(p)
            .or_else(|| crate::gh::cached_title(ctx, fleet, p))
            .unwrap_or_default(),
        since: first(p).next().map_or(0, |e| e.ts),
        children: kids.into_iter().map(|n| child(events, st, fleet, n)).collect(),
    };
    opened(events, st, fleet).into_iter().map(epic).collect()
}

/// The refusals of `epic add` that need no GitHub read.
pub fn check(events: &[Event], st: &States, fleet: &str, parent: u64, op: &Op) -> Result<()> {
    let Op::Add { issues: kids } = op else { return Ok(()) };
    let s = st.get(&(fleet.to_string(), parent));
    let queued = crate::queue::replay(events, fleet).iter().any(|i| i.issue == parent);
    let lane = s.is_some_and(|s| s.done() || s.lane.is_some() || !s.lanes.is_empty());
    if lane || queued || parent_of(events, st, fleet, parent).is_some() || kids.contains(&parent) {
        let why = "is closed, merged, queued, has had a lane, is an epic's child or is named as its own child";
        return refused("EPIC_PARENT", format!("{fleet}#{parent} {why}"));
    }
    if let Some(n) = kids.iter().find(|n| is_parent(events, st, fleet, **n)) {
        return refused("EPIC_PARENT", format!("{fleet}#{n} is an open epic's parent"));
    }
    for n in kids {
        if let Some(p) = parent_of(events, st, fleet, *n).filter(|p| *p != parent) {
            let free = format!("drover epic {fleet} {p} rm {n}");
            return refused("EPIC_CHILD", format!("{fleet}#{n} is in epic #{p}: `{free}` frees it"));
        }
    }
    let mut all = replay(events, fleet).remove(&parent).unwrap_or_default();
    all.extend(kids);
    if !all
        .iter()
        .any(|n| matches!(child(events, st, fleet, *n), Member::Open { .. }))
    {
        let why = "no child would be open (all merged or closed): there is nothing to track";
        return refused("EPIC_DONE", format!("{fleet}#{parent}: {why}"));
    }
    Ok(())
}

/// `drover epic <fleet> <parent> add|rm <n>…`: checks, one spec read per new issue, then the `epic` event; the line to print.
pub fn change(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str, parent: u64, op: Op) -> Result<String> {
    let f = cfg.fleet(fleet)?;
    let events = working(&ctx.state)?;
    let st = fold(&events);
    check(&events, &st, fleet, parent, &op)?;
    let now = replay(&events, fleet).remove(&parent).unwrap_or_default();
    let (verb, kids): (&str, Vec<u64>) = match op {
        Op::Add { issues } => ("add", issues.into_iter().filter(|n| !now.contains(n)).collect()),
        Op::Rm { issues } => ("rm", issues.into_iter().filter(|n| now.contains(n)).collect()),
    };
    let mut d = json!({"op": verb, "parent": parent, "issues": kids});
    if verb == "add" && !kids.is_empty() {
        for n in std::iter::once(parent).chain(kids.iter().copied()) {
            if crate::gh::is_pr(&crate::gh::spec(ctx, r, f, fleet, n, false)?) {
                return refused("IS_PR", format!("#{n} is a pull request, not an issue"));
            }
        }
        let title = crate::gh::cached_title(ctx, fleet, parent).unwrap_or_default();
        d.put("title", json!(title));
    }
    if !kids.is_empty() {
        crate::events::emit(&ctx.state, cfg, Event::new(ctx.now, fleet, None, "epic", d))?;
    }
    let events = working(&ctx.state)?;
    let e = open(ctx, &events, &st, fleet).into_iter().find(|e| e.parent == parent);
    let list: Vec<u64> = e
        .as_ref()
        .map_or(vec![], |e| e.children.iter().map(Member::issue).collect());
    let title = e.map(|e| e.title).unwrap_or_default();
    let n = list.len();
    Ok(format!(
        "{fleet} #{parent} {title}: {n} {} ({})",
        if n == 1 { "child" } else { "children" },
        crate::queue::hashes(&list).replace(',', "")
    ))
}

/// Open epics' parents and their children with no state: the closed-issues check asks GitHub about them.
pub fn watched(events: &[Event], st: &States, fleet: &str) -> Vec<u64> {
    let unknown = |n: &u64| !st.contains_key(&(fleet.to_string(), *n));
    let m = opened(events, st, fleet);
    let all = m.iter().flat_map(|(p, kids)| std::iter::once(p).chain(kids));
    all.copied()
        .filter(unknown)
        .collect::<BTreeSet<u64>>()
        .into_iter()
        .collect()
}

/// The parent's one comment: each merged child with its PR, then the dropped ones.
pub fn summary(l: &Landed) -> String {
    let k = l.merged.len();
    let mut out = format!("Landed: {k} of {k} issues merged.");
    for c in &l.merged {
        let head = format!("#{} {}", c.issue, c.title);
        out += &match c.pr {
            Some(pr) => format!("\n- {}: PR #{pr}", head.trim_end()),
            None => format!("\n- {}", head.trim_end()),
        };
    }
    if !l.dropped.is_empty() {
        let d: Vec<String> = l.dropped.iter().map(|n| format!("#{n}")).collect();
        out += &format!("\nDropped (closed unmerged): {}", d.join(" "));
    }
    out
}

/// Land every open epic of `fleet` whose `lands()` is true: `epic-landed` once (pushed), one comment, then the close.
pub fn settle(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<()> {
    let f = cfg.fleet(fleet)?;
    let events = working(&ctx.state)?;
    let st = fold(&events);
    for e in open(ctx, &events, &st, fleet).into_iter().filter(Epic::lands) {
        let l = e.landed(ctx);
        let Value::Object(d) = serde_json::to_value(&l)? else {
            continue;
        };
        let ev = Event::new(ctx.now, fleet, Some(e.parent), "epic-landed", Value::Object(d)).push();
        let id = format!("{fleet}#{}:epic-land", e.parent);
        if !crate::events::emit_once(&ctx.state, cfg, &id, &ev)? {
            continue;
        }
        let (n, body) = (e.parent.to_string(), summary(&l));
        writeln!(out, "{fleet} #{n} landed: {} issues", l.merged.len())?;
        let comment = ["issue", "comment", &n, "-R", &f.repo, "--body", &body];
        let sent = crate::hygiene::args(ctx, r, cfg, fleet, &comment);
        if let Err(err) = sent.and_then(|()| ok(r, "gh", &comment).or_else(|_| ok(r, "gh", &comment))) {
            let reason = format!("epic #{n} landed, but its comment failed twice: {err:#}");
            let d = json!({"reason": reason});
            crate::events::emit(
                &ctx.state,
                cfg,
                Event::new(ctx.now, fleet, Some(e.parent), "evidence-missing", d),
            )?;
            writeln!(out, "{reason}; issue #{n} left open")?;
            continue;
        }
        r.run(
            "gh",
            &argv(&["issue", "close", &n, "-R", &f.repo, "--reason", "completed"]),
        )
        .ok();
    }
    Ok(())
}
