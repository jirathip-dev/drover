use crate::config::Limits;
use crate::events::{age, emit, Event, IssueState};
use crate::waiting::Waiting;
use crate::Ctx;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// What drive promised: its deadline is `[limits] promise_passes` drive passes for the kind (default 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    NextPass,
    Merging,
    SpawnPending,
    WaitingOn,
}

/// A waiting or next-step line, typed: `line` is the only place its words are built.
#[derive(Clone, Debug, PartialEq)]
pub enum Say {
    Drive { reason: String, step: String },
    CatchUp(String),
    Record(String),
    Merge(String),
    MergeFast(String),
    MergeOnGreen(String),
    SpawnSlot(Option<String>),
    ReviewerSlot(String),
    ReviewsAhead(usize),
    Pass(String),
    Starts(String),
}

/// The spawn-slot wait phrase; the board matches on it.
pub const SLOT: &str = "waiting for a spawn slot";
const CATCH_UP: &str = "catching up next pass (≤ 5 min)";

impl Say {
    pub fn kind(&self) -> Kind {
        match self {
            Say::Merge(_) | Say::MergeFast(_) => Kind::Merging,
            Say::MergeOnGreen(_) => Kind::WaitingOn,
            Say::SpawnSlot(_) | Say::ReviewerSlot(_) | Say::ReviewsAhead(_) => Kind::SpawnPending,
            _ => Kind::NextPass,
        }
    }

    pub fn line(&self) -> String {
        match self {
            Say::Drive { reason, step } => format!("{reason}, waiting for drive (next pass ≤ 5 min) to {step}"),
            Say::CatchUp(r) => format!("{r} · {CATCH_UP}"),
            Say::Record(r) => format!("{r} · recording within 1 min"),
            Say::Merge(r) => format!("{r} · merging next pass"),
            Say::MergeFast(r) => format!("{r} · merging within 1 min"),
            Say::MergeOnGreen(r) => format!("{r} · merging when CI green"),
            Say::SpawnSlot(None) => SLOT.into(),
            Say::SpawnSlot(Some(by)) => format!("{SLOT}: {by}"),
            Say::ReviewerSlot(why) => format!("waiting for a reviewer slot: {why}"),
            Say::ReviewsAhead(n) => format!(
                "review waits behind {n} earlier review{}",
                ["s", ""][usize::from(*n == 1)]
            ),
            Say::Pass(r) => format!("drive's next pass{r}"),
            Say::Starts(r) => format!("{r}; starts next pass"),
        }
    }
}

/// A line that ends catching up, without that ending.
pub fn strip_catch_up(line: &str) -> Option<&str> {
    line.strip_suffix(CATCH_UP)
}

/// A promise drive made: what, why, and by when.
#[derive(Clone, Debug, PartialEq)]
pub struct Promise {
    pub fleet: String,
    pub issue: u64,
    pub kind: Kind,
    pub reason: String,
    pub made_at: u64,
    pub deadline: u64,
    state: String,
    head: String,
}

impl Promise {
    fn of(e: &Event) -> Option<Promise> {
        Some(Promise {
            fleet: e.fleet.clone(),
            issue: e.issue?,
            kind: serde_json::from_value(e.data.get("kind")?.clone()).ok()?,
            reason: e.str("reason").unwrap_or_default().into(),
            made_at: e.ts,
            deadline: e.data.get("deadline")?.as_u64()?,
            state: e.str("state").unwrap_or_default().into(),
            head: e.str("head").unwrap_or_default().into(),
        })
    }
    fn moved(&self, s: &IssueState, head: &str) -> bool {
        self.state != s.state || self.head != head
    }
}

impl Limits {
    /// How long a promise of `kind` may stay open.
    pub fn promise_secs(&self, kind: Kind) -> u64 {
        self.promise_passes.get(&kind).copied().unwrap_or(2) * self.slow_secs
    }
}

fn open(mine: &[&Event]) -> Option<(Promise, Option<String>)> {
    let at = mine.iter().rposition(|e| e.kind == "promise")?;
    let p = Promise::of(mine[at])?;
    let missed = mine[at..].iter().rfind(|e| e.kind == "promise-missed");
    Some((p, missed.map(|e| e.str("why").unwrap_or_default().into())))
}

fn why_now(mine: &[&Event], p: &Promise, now: &Waiting) -> String {
    let refused = mine
        .iter()
        .rev()
        .take_while(|e| e.ts >= p.made_at)
        .find(|e| matches!(e.kind.as_str(), "driven" | "blocked") && e.str("code").is_some());
    let said = refused.map(|e| {
        let why = e.str("reason").or(e.str("result")).unwrap_or_default();
        format!("{} {why}", e.str("code").unwrap_or_default())
            .trim()
            .to_string()
    });
    let host = (now.what.starts_with("host offline")).then(|| now.what.clone());
    match (said, host) {
        (_, Some(h)) => h,
        (Some(c), _) if now.what == p.reason => c,
        (Some(c), _) => format!("{c} · now: {}", now.what),
        (None, _) if now.what == p.reason => "drive took no step".into(),
        (None, _) => format!("now: {}", now.what),
    }
}

/// A drive pass's promises: each new one logged `promise`, each one missed logged `promise-missed` once.
pub fn pass(
    ctx: &Ctx,
    cfg: &crate::config::Config,
    fleet: &str,
    events: &[Event],
    rows: &[(IssueState, Waiting)],
) -> Result<()> {
    for (s, w) in rows
        .iter()
        .filter(|(s, w)| s.fleet == fleet && s.lane.as_ref() == Some(&w.lane))
    {
        let mine: Vec<&Event> = events
            .iter()
            .filter(|e| e.fleet == fleet && e.issue == Some(s.issue))
            .collect();
        let head = crate::gh::cached_pr(ctx, fleet, s.issue)["headRefOid"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let was = open(&mine);
        let fresh = was
            .as_ref()
            .is_none_or(|(p, _)| p.moved(s, &head) || Some(p.kind) != w.promise);
        if let Some(kind) = w.promise.filter(|_| fresh) {
            let deadline = ctx.now + cfg.limits.promise_secs(kind);
            let d = json!({"kind": kind, "reason": w.what, "deadline": deadline, "state": s.state, "head": head});
            crate::events::append(&ctx.state, &Event::new(ctx.now, fleet, Some(s.issue), "promise", d))?;
            continue;
        }
        let Some((p, None)) = was else { continue };
        if ctx.now < p.deadline || p.moved(s, &head) || matches!(s.state.as_str(), "merged" | "closed") {
            continue;
        }
        let why = why_now(&mine, &p, w);
        let d = json!({"kind": p.kind, "promised": p.reason, "age": ctx.now - p.made_at, "why": why});
        emit(
            &ctx.state,
            cfg,
            Event::new(ctx.now, fleet, Some(s.issue), "promise-missed", d),
        )?;
    }
    Ok(())
}

/// #855: the issue's open promise once missed, with drive's real reason.
pub fn open_missed(events: &[Event], s: &IssueState, head: &str, kind: Option<Kind>) -> Option<(Promise, String)> {
    let mine: Vec<&Event> = events
        .iter()
        .filter(|e| e.fleet == s.fleet && e.issue == Some(s.issue))
        .collect();
    let (p, why) = open(&mine)?;
    let why = why.filter(|_| !p.moved(s, head) && kind.is_none_or(|k| k == p.kind))?;
    Some((p, why))
}

/// The row's line once its open promise was missed: `⚠ missed <age>: <real reason>`.
pub fn missed(events: &[Event], s: &IssueState, head: &str, kind: Option<Kind>, now: u64) -> Option<String> {
    let (p, why) = open_missed(events, s, head, kind)?;
    Some(format!("⚠ missed {}: {why}", age(now.saturating_sub(p.made_at))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(state: &str) -> IssueState {
        IssueState {
            fleet: "f".into(),
            issue: 7,
            state: state.into(),
            lane: Some("f-impl-7".into()),
            ..Default::default()
        }
    }

    fn row(s: &IssueState, promise: Option<Kind>, what: &str) -> (IssueState, Waiting) {
        let w = Waiting {
            fleet: "f".into(),
            issue: 7,
            lane: "f-impl-7".into(),
            who: "drive",
            what: what.into(),
            since: 0,
            next_cmd: None,
            promise,
        };
        (s.clone(), w)
    }

    fn run(dir: &std::path::Path, now: u64, rows: &[(IssueState, Waiting)]) -> Vec<Event> {
        let state = dir.join("events.jsonl");
        let ctx = Ctx {
            config: dir.join("fleet.toml"),
            state: state.clone(),
            worktrees: dir.join("w"),
            homes: std::collections::BTreeMap::default(),
            exe: "drover".into(),
            now,
            home: dir.into(),
            hints: vec![],
        };
        let cfg = crate::config::Config::default();
        let ev = crate::events::working(&state).unwrap_or_default();
        pass(&ctx, &cfg, "f", &ev, rows).unwrap();
        crate::events::working(&state).unwrap().to_vec()
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("drover-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn count(ev: &[Event], kind: &str) -> usize {
        ev.iter().filter(|e| e.kind == kind).count()
    }

    #[test]
    fn a_promise_past_its_deadline_with_no_state_change_is_missed_exactly_once() {
        let d = tmp("promise-missed");
        let ready = st("ready");
        let line = Say::Merge("PASS · verify skipped".into()).line();
        run(&d, 1000, &[row(&ready, Some(Kind::Merging), &line)]);
        let ev = run(&d, 1300, &[row(&ready, None, "PASS · CI green · PR is a draft")]);
        assert_eq!(count(&ev, "promise-missed"), 0, "inside its 2 passes");
        let ev = run(&d, 1600, &[row(&ready, None, "PASS · CI green · PR is a draft")]);
        let ev2 = run(&d, 1900, &[row(&ready, None, "PASS · CI green · PR is a draft")]);
        assert_eq!((count(&ev, "promise-missed"), count(&ev2, "promise-missed")), (1, 1));
        let m = ev2.iter().find(|e| e.kind == "promise-missed").unwrap();
        assert_eq!(m.str("why"), Some("now: PASS · CI green · PR is a draft"));
        assert_eq!(m.data["age"], 600);
        assert_eq!(
            missed(&ev2, &ready, "", None, 1900).as_deref(),
            Some("⚠ missed 15m: now: PASS · CI green · PR is a draft")
        );
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_promise_the_issue_moved_past_is_never_missed() {
        let d = tmp("promise-kept");
        let line = Say::Drive {
            reason: "pushed abc1234".into(),
            step: "spawn review".into(),
        }
        .line();
        run(&d, 1000, &[row(&st("impl"), Some(Kind::NextPass), &line)]);
        let ev = run(&d, 5000, &[row(&st("review(1)"), None, "waiting on f-rev-7 (working)")]);
        assert_eq!((count(&ev, "promise"), count(&ev, "promise-missed")), (1, 0));
        assert_eq!(missed(&ev, &st("review(1)"), "", None, 5000), None);
        std::fs::remove_dir_all(&d).unwrap();
    }
}
