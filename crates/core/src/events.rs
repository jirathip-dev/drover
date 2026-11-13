//! The append-only event log (#344: in `state.db`, `db`), the state fold over it, and notifiers.
use crate::config::Config;
use crate::runner::{Real, Runner};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::cell::UnsafeCell;
use std::collections::{BTreeMap, HashMap};
use std::mem::MaybeUninit;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(optional_fields = nullable))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// #259: the log's monotonic number, set on append (an old row's is its line number); 0 before.
    #[serde(default, skip_serializing_if = "is_zero")]
    #[cfg_attr(feature = "ts", ts(as = "Option<u64>", optional))]
    pub seq: u64,
    pub ts: u64,
    pub fleet: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<u64>,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    pub data: Map<String, Value>,
}

impl Event {
    pub fn new(ts: u64, fleet: &str, issue: Option<u64>, kind: &str, data: Value) -> Event {
        let data = match data {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        Event {
            seq: 0,
            ts,
            fleet: fleet.into(),
            issue,
            kind: kind.into(),
            data,
        }
    }
    pub fn str(&self, k: &str) -> Option<&str> {
        self.data.get(k).and_then(Value::as_str)
    }
    /// A `kind` event about `fleet` #`issue`.
    pub fn is(&self, kind: &str, fleet: &str, issue: u64) -> bool {
        self.kind == kind && self.fleet == fleet && self.issue == Some(issue)
    }
}

pub(crate) fn aliased(mut ev: Event) -> Event {
    if let Some(k) = ev.kind.strip_prefix("conductor-") {
        ev.kind = format!("orchestrator-{k}");
    }
    if ev.str("from") == Some("conductor") {
        ev.data["from"] = "orchestrator".into();
    }
    if let Some(b) = ev.str("by").filter(|b| b.starts_with("conductor-")) {
        ev.data["by"] = crate::orchestrator::by(b).into();
    }
    ev
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's skip_serializing_if hands a reference
fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// #344: one event, numbered next, into the database beside `path` (`db::append`): every writer goes through here.
pub fn append(path: &Path, ev: &Event) -> Result<()> {
    crate::db::append(path, ev)
}

/// The live log (#343: without the events a rotation archived), by seq.
pub fn read(path: &Path) -> Result<Vec<Event>> {
    Ok(shared(path)?.to_vec())
}

/// #344: the log in the JSONL format before #344, one event a line with its seq (`drover events --jsonl`).
pub fn jsonl(path: &Path) -> Result<String> {
    history(path)?
        .iter()
        .map(|e| Ok(serde_json::to_string(e)? + "\n"))
        .collect()
}

/// A JSONL log from before #344 (`db::import`): every complete record; a torn last line is skipped silently, any other
/// bad line with a warning.
pub(crate) fn read_file(path: &Path) -> Result<Vec<Event>> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(parse(&t, path)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.into()),
    }
}

fn parse(text: &str, path: &Path) -> Vec<Event> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::with_capacity(lines.len());
    for (i, l) in lines.iter().enumerate() {
        match serde_json::from_str::<Event>(l) {
            Ok(ev) if ev.seq == 0 => out.push(aliased(Event {
                seq: i as u64 + 1,
                ..ev
            })),
            Ok(ev) => out.push(aliased(ev)),
            Err(_) if l.trim().is_empty() || i + 1 == lines.len() => {}
            Err(e) => eprintln!(
                "drover: {}: line {}: skipping corrupt record: {e}",
                path.display(),
                i + 1
            ),
        }
    }
    out
}

/// #343: the fold up to `seq` (the `states` snapshot row): a cache, the log rebuilds it.
#[derive(Serialize, Deserialize, Default)]
struct Checkpoint {
    seq: u64,
    states: Vec<IssueState>,
}

/// #343: the rotated archives (`events-YYYYMM.jsonl.gz`) of a log from before #344 (`db::import`).
pub(crate) fn archives(path: &Path) -> Result<Vec<Event>> {
    let dir = std::fs::read_dir(path.parent().unwrap_or(Path::new(".")));
    let mut gz: Vec<PathBuf> = dir.into_iter().flatten().filter_map(|e| Some(e.ok()?.path())).collect();
    gz.retain(|p| {
        p.file_name()
            .is_some_and(|n| n.to_string_lossy().ends_with(".jsonl.gz"))
    });
    gz.sort();
    let mut all = vec![];
    for g in gz {
        let o = Real.run("gzip", &["-dc".into(), g.to_string_lossy().into_owned()])?;
        all.extend(parse(&o.stdout, &g));
    }
    Ok(all)
}

/// #343: every event, the archived ones too, by seq.
pub fn history(path: &Path) -> Result<Vec<Event>> {
    crate::db::events(path, "seq > ?1", 0)
}

/// #343: the checkpoint's states and seq, and the events after it (no checkpoint: every event), read by seq.
fn load(path: &Path) -> Result<(States, u64, Vec<Event>)> {
    let cp = crate::db::get::<Checkpoint>(path, "states")
        .map(|c| c.1)
        .unwrap_or_default();
    let m = cp.states.into_iter().map(|s| ((s.fleet.clone(), s.issue), s)).collect();
    Ok((m, cp.seq, crate::db::events(path, "seq > ?1", cp.seq)?))
}

/// #343: the current per-issue state, from the checkpoint and the events after it: `fold` of every event.
pub fn states(path: &Path) -> Result<States> {
    let (m, _, ev) = load(path)?;
    Ok(fold_into(m, &ev))
}

/// #343: save the checkpoint (each slow pass); returns its seq.
pub fn checkpoint(path: &Path) -> Result<u64> {
    let (m, seq, ev) = load(path)?;
    let seq = ev.last().map_or(seq, |e| e.seq);
    let states = fold_into(m, &ev).into_values().collect();
    crate::db::put(path, "states", seq, &Checkpoint { seq, states })?;
    Ok(seq)
}

/// #343: over `max` bytes of live log, the events up to the checkpoint and older than 30 days are archived (#344: kept
/// in the database for `history`, out of `read`). The live log keeps what its readers need: queue changes, every event
/// of an issue still open or active in the 30 days, and a done issue's merge or close.
pub fn rotate(path: &Path, max: u64, now: u64) -> Result<()> {
    if crate::db::live_bytes(path)? <= max {
        return Ok(());
    }
    let (cut, st, all) = (checkpoint(path)?, states(path)?, read(path)?);
    let old = |e: &Event| e.seq <= cut && e.ts + 30 * 86_400 < now && e.kind != "queue";
    let mut live_issue: BTreeMap<(&str, u64), bool> = BTreeMap::new();
    for e in &all {
        if let Some(n) = e.issue {
            let done = st.get(&(e.fleet.clone(), n)).is_some_and(IssueState::done);
            *live_issue.entry((&e.fleet, n)).or_default() |= !done || !old(e);
        }
    }
    // A merged issue keeps its merge; a closed one (it never merged) what closed it.
    let ends = |e: &Event, s: &IssueState| match s.state.as_str() {
        "merged" => e.kind == "merged",
        _ => {
            matches!(e.kind.as_str(), "closed" | "closed-other") || e.kind == "clean" && e.str("why") != Some("merged")
        }
    };
    let moved = |e: &&Event| match e.issue {
        Some(n) => !live_issue[&(e.fleet.as_str(), n)] && !ends(e, &st[&(e.fleet.clone(), n)]),
        None => old(e),
    };
    let seqs: Vec<u64> = all.iter().filter(moved).map(|e| e.seq).collect();
    crate::db::archive(path, &seqs)
}

/// Append, then run notifiers. Notifier failures never fail the caller.
#[allow(clippy::needless_pass_by_value)] // public API: callers and tests hand the event over
pub fn emit(state: &Path, cfg: &Config, ev: Event) -> Result<()> {
    append(state, &ev)?;
    notify(state, cfg, &ev)
}

/// #900: `db::ask`, then notify each event logged: they, the asks numbered, or none when `once` is logged already.
pub fn ask(state: &Path, cfg: &Config, evs: Vec<Event>, once: Option<&str>) -> Result<Vec<Event>> {
    let evs = crate::db::ask(state, evs, once)?.unwrap_or_default();
    evs.iter().try_for_each(|ev| notify(state, cfg, ev))?;
    Ok(evs)
}

fn notify(state: &Path, cfg: &Config, ev: &Event) -> Result<()> {
    // #99: phones hear of owner asks and of each default taken, never of a routine ask merely waiting.
    // #407: a thread open on a board (its stream stamped `watch-<thread>.at` in the last 5 s) pushes nothing.
    let at = |t: &str| std::fs::read_to_string(state.with_file_name(format!("watch-{t}.at"))).ok();
    let watched = ev
        .str("thread")
        .and_then(at)
        .and_then(|a| a.trim().parse::<u64>().ok())
        .is_some_and(|a| a + 5 >= ev.ts);
    let push = match ev.kind.as_str() {
        _ if watched => "",
        "needs-decision" if ev.str("kind") == Some("routine") || ev.str("routed").is_some() => "",
        "decision" if ev.str("by") == Some("default") => "needs-decision",
        // #541: a lane ask put to the owner pushes as an ask; a conductor's answer to one never pushes.
        "ask-owner" => "needs-decision",
        "decision" if ev.str("by").is_some_and(|b| b.starts_with("orchestrator-")) => "",
        // #386: a conductor's reply pushes; the owner's own words and system lines never do.
        "thread-msg" if ev.str("from") != Some("orchestrator") => "",
        k => k,
    };
    let wanted = |n: &&crate::config::Notify| n.wants(if n.webpush() { push } else { &ev.kind });
    for n in cfg.notifiers(&ev.fleet).into_iter().filter(wanted) {
        let payload = if n.format.as_deref() == Some("json") {
            serde_json::to_string(ev)?
        } else {
            plain(ev) + "\n"
        };
        let res = if n.webpush() {
            crate::webpush::send(state, n.sub.as_deref().unwrap_or_default(), ev, None).map(drop)
        } else {
            run_notifier(&n.cmd, &payload)
        };
        if let Err(e) = res {
            let cmd = if n.webpush() { "webpush" } else { &n.cmd };
            eprintln!("drover: notifier `{cmd}` failed: {e:#}");
            let d = serde_json::json!({"cmd": cmd, "event": ev.kind, "error": format!("{e:#}")});
            let _ = append(state, &Event::new(ev.ts, &ev.fleet, ev.issue, "notify-failed", d));
        }
    }
    Ok(())
}

/// One human-readable line, e.g. `api #1037 merged → staging 8ed4a06 (PR #1041)`.
pub fn plain(ev: &Event) -> String {
    let who = ev.issue.map_or(ev.fleet.clone(), |n| format!("{} #{n}", ev.fleet));
    let s = |k: &str| ev.str(k).unwrap_or("?");
    match ev.kind.as_str() {
        "merged" => {
            let sha: String = s("merge_sha").chars().take(7).collect();
            format!("{who} merged → {} {sha} (PR #{})", s("into"), s("pr"))
        }
        "blocked" => format!("{who} blocked {}: {}", s("code"), s("reason")),
        "needs-decision" | "ask-owner" => {
            let rec = ev.data["recommend"].as_u64();
            let opts: String = ev.data["options"]
                .as_array()
                .into_iter()
                .flatten()
                .zip(1..)
                .map(|(o, n)| {
                    format!(
                        "\n  {n}) {}{}",
                        o.as_str().unwrap_or("?"),
                        if rec == Some(n) { " (recommended)" } else { "" }
                    )
                })
                .collect();
            format!(
                "{who} needs you ({} asks {}): {}{opts}\n  drover answer {} {} <n|\"text\">",
                s("agent"),
                s("id"),
                s("question"),
                ev.fleet,
                s("id")
            )
        }
        "host-busy" => format!("host-busy {}: {}", s("host"), s("reason")),
        "spawn-failing" => format!("spawn-failing {}#{}: {}", ev.fleet, ev.issue.unwrap_or(0), s("error")),
        "decision" => format!("{who} decision {} by {}: {}", s("id"), s("by"), s("choice")),
        "queue" => {
            let n: Vec<String> = ev.data["issues"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|n| format!("#{n}"))
                .collect();
            format!("{who} queue {} {}", s("op"), n.join(" "))
        }
        k => match ev.str("reason") {
            Some(r) => format!("{who} {k}: {r}"),
            None => format!("{who} {k}"),
        },
    }
}

fn run_notifier(cmd: &str, payload: &str) -> Result<()> {
    let mut args: Vec<String> = std::env::vars()
        .map(|(k, _)| k)
        .filter(|k| crate::providers::all().any(|p| p.scrubs_env(k)))
        .flat_map(|k| ["-u".into(), k])
        .collect();
    // Both streams go nowhere, as before: a sink that backgrounds a child must not hold the reply open.
    args.extend(crate::runner::argv(&[
        "sh",
        "-c",
        &format!("exec >/dev/null 2>&1\n{cmd}"),
    ]));
    let secs = crate::config::limits().notify_secs;
    let o = Real.run_for("env", &args, payload, Duration::from_secs(secs))?;
    anyhow::ensure!(o.code != 124, "timed out after {secs}s");
    anyhow::ensure!(o.code == 0, "exit {}", o.code);
    Ok(())
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(optional_fields = nullable))]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct IssueState {
    pub fleet: String,
    pub issue: u64,
    pub state: String,
    pub lane: Option<String>,
    pub branch: Option<String>,
    /// The latest lane's reasoning effort, as spawned.
    pub effort: Option<String>,
    pub rounds: u32,
    /// Extra rounds the owner allowed (`round-extended`, #130); #309: the latest grants the round after any stage's.
    pub extra: u32,
    pub granted: u32,
    /// #123: passed `--update` rounds, apart from `rounds`; `updating` while one is open (state `fix(n)+uk`).
    pub updates: u32,
    pub updating: bool,
    pub last_ts: u64,
    pub last_event: String,
    /// Latest agent per role (impl, rev).
    pub lanes: BTreeMap<String, String>,
    /// (sha, PASS|FAIL, the check stage whose rounds it counts) in log order; logs before #173: `rev`.
    pub verdicts: Vec<(String, String, String)>,
    /// #173: the pipeline stage of the latest spawn, the latest check and build roles, review rounds per check stage.
    pub stage: String,
    pub check: Option<String>,
    pub build: Option<String>,
    pub stage_rounds: BTreeMap<String, u32>,
    /// #228: `FAIL (form-only)` verdicts that did not count toward the rounds.
    pub form: u32,
}

/// Per-issue state, keyed by (fleet, issue).
pub type States = BTreeMap<(String, u64), IssueState>;

/// The newest `spawn` of `agent`.
pub fn last_spawn<'a>(events: &'a [Event], agent: &str) -> Option<&'a Event> {
    events
        .iter()
        .rev()
        .find(|e| e.kind == "spawn" && e.str("agent") == Some(agent))
}

/// #425: the issue `s`'s lane is held on (`held`, no `released` or `spawn` since).
/// #505: the issues it waits on (`on`: one number before #505, a list since); empty when not held.
pub fn held_on(events: &[Event], s: &IssueState) -> Vec<u64> {
    let kinds = ["held", "released", "spawn"];
    let e = events
        .iter()
        .rev()
        .find(|e| e.fleet == s.fleet && e.issue == Some(s.issue) && kinds.contains(&&*e.kind) && !design(e));
    e.filter(|e| e.kind == "held")
        .map_or(vec![], |e| crate::queue::nums(&e.data["on"]))
}

/// #500: a design (#730: or architect) lane's spawn: never the issue's build or check lane.
pub fn design(e: &Event) -> bool {
    e.kind == "spawn" && e.str("role").is_some_and(|r| STAGES.contains(&r))
}

/// #730: the owner-reviewed pre-impl roles, in the order they run.
pub const STAGES: [&str; 2] = ["architect", "design"];

/// #424: lane end `e` ends spawn `sp` of its agent: only that pane's when `e` names one; a supersede logged
/// without a pane (before #424, after the new spawn) ends none.
pub fn ends(e: &Event, sp: &Event) -> bool {
    let pane = e.str("pane").map(|p| sp.str("pane") == Some(p));
    e.str("agent") == sp.str("agent") && pane.unwrap_or(e.str("reason") != Some("superseded"))
}

/// #173: a check stage's spawn (logs before #173: a `rev` one).
pub fn checks(e: &Event) -> bool {
    e.data
        .get("check")
        .and_then(Value::as_bool)
        .unwrap_or(e.str("role") == Some("rev"))
}

/// Events `drover check` and notifiers write about an issue; they are not activity.
pub const PASSIVE: &[&str] = &[
    // #778: a tapped start still waiting; the decision before it moved the issue.
    "pending-action",
    "lane-state",
    "lesson-prompted",
    // #511: a lane's typed turn end and drive's prompt on it; neither moves the issue.
    "turn-end",
    "turn-prompted",
    "promise",
    "promise-missed",
    "self-heal",
    "stuck",
    "slo-ok",
    // #677: a why note put on a round head, one that failed to push, and drive's one nudge on a refused note.
    "why",
    "why-failed",
    "why-nudged",
    // #666: an ACP lane's first prompt sent again.
    "start-resent",
    "nudged",
    "respawned-silent",
    "rev-stale-replaced",
    "budget-freed",
    // #798: a finished lane's build folder or scratch removed.
    "build-freed",
    "wip-saved",
    "wip-blocked",
    // #662: a gone ACP lane's local commits drive pushed before resuming it.
    "lane-pushed",
    "stall",
    "orphan-lane",
    "orphan-issue",
    "outbox-stale",
    "lane-dead",
    "ready-unmerged",
    "ci-missing",
    "notify-failed",
    "push-sent",
    "account-capped",
    "agent-unavailable",
    "account-signed-out",
    "lane-failed-start",
    "cap-banner-ignored",
    "spawn-failed",
    // #724: a failed spawn drive retries after a backoff, and the alert on a third identical one.
    "spawn-error",
    "spawn-failing",
    "spawn-started",
    // #500: an adoption is logged with its `spawn`; a design lane's alone must not end the issue's verdict wait.
    "spawn-adopted",
    "fallback",
    "ask-waiting",
    "driven",
    "respawn",
    "drive-pass",
    "kick",
    "amended",
    "conflict-predicted",
    "heads-up",
    "lane-closed",
    "lane-kept",
    "lane-kept-dirty",
    "lane-reaped",
    "lane-trimmed",
    "stale-pane-killed",
    "decision-redelivered",
    "labelled",
    "build-cloned",
    "build-reused",
    "unlabelled",
    "form-only-rejected",
    "form-rounds-exhausted",
    "stale-marker",
    "spawn-held",
    "host-busy",
    "outbox",
    // #584: an ask's other endings, and the one-time sweep.
    "ask-withdrawn",
    "ask-delivered",
    "ask-failed",
    "ask-expired",
    "asks-swept",
    // #509: a side effect's log around it; the step's own event (spawn, merged) is the activity.
    "intent",
    "done",
    "failed",
];

/// #584: an ask's endings, the first that applies naming why it closed.
const ORDER: [&str; 9] = [
    "answered",
    "withdrawn",
    "delivered",
    "failed",
    "expired",
    "closed: merged",
    "closed: issue closed",
    "closed: lane gone",
    "repeat",
];

/// #632: events indexed and (ask, event) pairs checked: the ask endings' work, counted.
pub static VISITS: AtomicUsize = AtomicUsize::new(0);

/// #632: every ask's ending (`closes`' rules, `ORDER`'s choice), one pass over the log, one event at a time (`add`).
#[derive(Default, Clone)]
pub struct Asks {
    /// By the ask's position: its ending (rank in `ORDER`, the closing event's position); `None` while open.
    ends: BTreeMap<usize, Option<(usize, usize)>>,
    /// The asks an event naming (0 id | 1 issue | 2 agent, fleet, value) can still end better.
    by: HashMap<(u8, String, Option<String>), Vec<usize>>,
    /// The first `spawn-started` of each (agent, pane).
    started: HashMap<(Option<String>, Option<String>), usize>,
    ids: std::collections::HashSet<(String, String)>,
}

impl Asks {
    /// Index `events`' last event.
    pub fn add(&mut self, events: &[Event]) {
        let Some((e, i)) = events.last().map(|e| (e, events.len() - 1)) else {
            return;
        };
        VISITS.fetch_add(1, Relaxed);
        let s = |k: &str| e.str(k).map(String::from);
        let key = |t: u8, v: Option<String>| (t, e.fleet.clone(), v);
        let issue = e.issue.map(|n| n.to_string());
        if e.kind == "spawn-started" {
            self.started.entry((s("agent"), s("pane"))).or_insert(i);
        } else if e.kind == "needs-decision" && s("id").is_some_and(|id| !self.ids.insert((e.fleet.clone(), id))) {
            self.ends.insert(i, Some((ORDER.len() - 1, i)));
        } else if e.kind == "needs-decision" {
            self.ends.insert(i, None);
            let keys = [key(0, s("id")), key(2, s("agent"))]
                .into_iter()
                .chain(issue.clone().map(|n| key(1, Some(n))));
            keys.for_each(|k| self.by.entry(k).or_default().push(i));
        }
        let (t, v) = match e.kind.as_str() {
            "decision" | "ask-withdrawn" | "ask-delivered" | "ask-failed" | "ask-expired" => (0, s("id")),
            "merged" | "closed" | "closed-other" | "clean" if issue.is_some() => (1, issue),
            "spawn" | "lane-killed" => (2, s("agent")),
            _ => return,
        };
        let Some(list) = self.by.get_mut(&key(t, v)) else {
            return;
        };
        let first = self.started.get(&(s("agent"), s("pane"))).copied();
        for &a in list.iter() {
            let w = closes(&events[a], e, || first.is_some_and(|f| f < a));
            let r = ORDER.iter().position(|o| Some(*o) == w).unwrap_or(ORDER.len());
            let end = self.ends.entry(a).or_default();
            if r < end.map_or(ORDER.len(), |x| x.0) {
                *end = Some((r, i));
            }
        }
        // An id can still answer, an issue still merge (over a lane gone), an agent only end an open ask.
        let floor = [0, 5, 7][usize::from(t)];
        list.retain(|a| self.ends[a].is_none_or(|x| x.0 > floor));
    }

    /// The asks of `events` (this index's) still open, oldest first.
    fn open<'a>(&self, events: &'a [Event]) -> Vec<&'a Event> {
        self.ends
            .iter()
            .filter(|x| x.1.is_none())
            .map(|x| &events[*x.0])
            .collect()
    }
}

/// #701: the chat's index, one event at a time (`add`, beside `Asks`): the threads, each thread's events, the first
/// ask and the last decision of each (fleet, id), and the handoff rows sent on (`conductor::forwarded`); by position.
#[derive(Default, Clone)]
pub struct Threads {
    pub new: Vec<usize>,
    of: HashMap<String, Vec<usize>>,
    asks: HashMap<(String, String), usize>,
    decisions: HashMap<(String, String), usize>,
    pub forwards: std::collections::BTreeSet<u64>,
}

impl Threads {
    /// Index `events`' last event.
    fn add(&mut self, events: &[Event]) {
        let Some((e, i)) = events.last().map(|e| (e, events.len() - 1)) else {
            return;
        };
        if let Some(t) = e.str("thread") {
            self.of.entry(t.into()).or_default().push(i);
        }
        self.forwards.extend(e.data.get("forwards").and_then(Value::as_u64));
        let id = (e.fleet.clone(), e.str("id").unwrap_or_default().to_string());
        match e.kind.as_str() {
            "thread-new" => self.new.push(i),
            "needs-decision" => {
                self.asks.entry(id).or_insert(i);
            }
            "decision" => {
                self.decisions.insert(id, i);
            }
            _ => {}
        }
    }

    /// Thread `id`'s events.
    pub fn of(&self, id: &str) -> &[usize] {
        self.of.get(id).map_or(&[], Vec::as_slice)
    }
}

/// #701, #944: the `Log` whose events `events` are, the newest or one a reader still holds (none: read some other way).
pub fn cached(events: &[Event]) -> Option<Arc<Log>> {
    let live = LIVE.lock().unwrap_or_else(PoisonError::into_inner);
    live.iter()
        .filter_map(Weak::upgrade)
        .find(|l| std::ptr::eq(&raw const *l.events, events))
}

/// #701: the first `needs-decision` (`ask`) or the last `decision` of `fleet`'s ask `id`: the index's when `events` is
/// a cached log, else one pass.
pub fn ask_event<'a>(events: &'a [Event], kind: &str, fleet: &str, id: &str) -> Option<&'a Event> {
    let is = |e: &&Event| e.kind == kind && e.fleet == fleet && e.str("id").unwrap_or_default() == id;
    let Some(l) = cached(events) else {
        return if kind == "decision" {
            events.iter().rfind(is)
        } else {
            events.iter().find(is)
        };
    };
    let x = &l.threads;
    let map = if kind == "decision" { &x.decisions } else { &x.asks };
    map.get(&(fleet.into(), id.into())).map(|i| &events[*i])
}

/// #900: `e` repeats an ask id logged before it (two passes raced to post it): every view shows the first alone.
pub fn repeat(events: &[Event], e: &Event) -> bool {
    let first = |id| ask_event(events, "needs-decision", &e.fleet, id);
    e.kind == "needs-decision" && e.str("id").and_then(first).is_some_and(|a| !std::ptr::eq(a, e))
}

/// #859: ask indexes built from a slice that is no cached `Log` (`index`'s one-pass fallback): serve's reads build none.
pub static REBUILDS: AtomicUsize = AtomicUsize::new(0);

/// #632: `f` on `events`' ask index: the cached `log`'s when `events` is it, else built in one pass.
fn index<T>(events: &[Event], f: impl FnOnce(&Asks) -> T) -> T {
    if let Some(l) = cached(events) {
        return f(&l.asks);
    }
    REBUILDS.fetch_add(1, Relaxed);
    let mut x = Asks::default();
    (1..=events.len()).for_each(|n| x.add(&events[..n]));
    f(&x)
}

/// #632: the live log (`read`), its ask index and fold, cached per log by its last seq: a new event is read and
/// folded alone (a rotation or another database file since: all again).
#[derive(Default, Clone)]
pub struct Log {
    pub events: Events,
    pub asks: Asks,
    pub threads: Threads,
    pub states: States,
    issues: HashMap<(String, u64), Vec<usize>>,
    seq: u64,
    key: (Option<u64>, Option<u64>),
}

impl std::ops::Deref for Log {
    type Target = [Event];
    fn deref(&self) -> &[Event] {
        &self.events
    }
}

/// #944: a log's events, in a buffer its later versions share and append to in place: each sees only its own `len`.
#[derive(Default, Clone)]
pub struct Events {
    buf: Arc<Buf>,
    len: usize,
}

#[derive(Default)]
struct Buf {
    len: Mutex<usize>,
    cells: Box<[UnsafeCell<MaybeUninit<Event>>]>,
}

// SAFETY: a cell is written once, under `len`'s lock, before `len` passes it; only cells below `len` are ever read.
unsafe impl Sync for Buf {}

impl Drop for Buf {
    fn drop(&mut self) {
        let n = *self.len.get_mut().unwrap_or_else(PoisonError::into_inner);
        for c in &mut self.cells[..n] {
            // SAFETY: the first `n` cells were written (`push`) and are dropped only here.
            unsafe { c.get_mut().assume_init_drop() };
        }
    }
}

impl std::ops::Deref for Events {
    type Target = [Event];
    fn deref(&self) -> &[Event] {
        // SAFETY: the first `len` cells are written and never written again; a cell is laid out as its `Event`.
        unsafe { std::slice::from_raw_parts(self.buf.cells.as_ptr().cast(), self.len) }
    }
}

impl Events {
    fn push(&mut self, e: Event) {
        loop {
            let mut at = self.buf.len.lock().unwrap_or_else(PoisonError::into_inner);
            if *at == self.len && self.len < self.buf.cells.len() {
                // SAFETY: cell `len` is past what every handle sees and `at`'s lock is held: nothing else touches it.
                unsafe { (*self.buf.cells[self.len].get()).write(e) };
                (*at, self.len) = (self.len + 1, self.len + 1);
                return;
            }
            drop(at);
            self.grow(1);
        }
    }

    fn reserve(&mut self, n: usize) {
        if self.len + n > self.buf.cells.len() {
            self.grow(n);
        }
    }

    fn grow(&mut self, n: usize) {
        let cap = (self.len + n).next_power_of_two().max(64);
        let cells = (0..cap).map(|_| UnsafeCell::new(MaybeUninit::uninit())).collect();
        let mut next = Events {
            buf: Arc::new(Buf {
                len: Mutex::new(0),
                cells,
            }),
            len: 0,
        };
        self.iter().for_each(|e| next.push(e.clone()));
        *self = next;
    }
}

type Gated = (Arc<Mutex<()>>, Arc<Log>);

static LOGS: Mutex<BTreeMap<PathBuf, Gated>> = Mutex::new(BTreeMap::new());

static LIVE: Mutex<Vec<Weak<Log>>> = Mutex::new(Vec::new());

/// #560, #632: the cached `Log` of `path` (by its database: a demo's scratch one apart), brought up to its last seq:
/// each call reads only the events appended since (serve's requests and watcher).
pub fn shared(path: &Path) -> Result<Arc<Log>> {
    let (file, logs) = (crate::db::file(path), || {
        LOGS.lock().unwrap_or_else(PoisonError::into_inner)
    });
    let gate = logs().entry(file.clone()).or_default().0.clone();
    let _one = gate.lock().unwrap_or_else(PoisonError::into_inner);
    let (last, key) = (crate::db::last_seq(path), crate::db::version(path));
    let mut l = {
        let mut m = logs();
        let l = &mut m.get_mut(&file).expect("added above").1;
        if key == l.key && last == l.seq && last > 0 {
            return Ok(l.clone());
        }
        std::mem::take(l)
    };
    let got = catch_up(path, &mut l, last, key);
    logs().get_mut(&file).expect("added above").1 = l.clone();
    let mut live = LIVE.lock().unwrap_or_else(PoisonError::into_inner);
    live.retain(|w| w.strong_count() > 0 && !std::ptr::eq(w.as_ptr(), Arc::as_ptr(&l)));
    live.push(Arc::downgrade(&l));
    got.map(|()| l)
}

fn catch_up(path: &Path, l: &mut Arc<Log>, last: u64, key: (Option<u64>, Option<u64>)) -> Result<()> {
    if key != l.key {
        *l = Arc::new(Log { key, ..Log::default() });
    }
    let new = crate::db::events(path, "archived = 0 AND seq > ?1", l.seq)?;
    let m = Arc::make_mut(l);
    m.seq = new.last().map_or(last, |e| e.seq.max(last));
    m.states = fold_into(std::mem::take(&mut m.states), &new);
    m.events.reserve(new.len());
    for e in new {
        if let Some(n) = e.issue {
            m.issues.entry((e.fleet.clone(), n)).or_default().push(m.events.len());
        }
        m.events.push(e);
        m.asks.add(&m.events);
        m.threads.add(&m.events);
    }
    Ok(())
}

/// Why ask `a` closed (#584: every ending): answered, withdrawn, delivered or failed (a handoff card), expired, its
/// issue merged, or its lane killed, cleaned or respawned; `None` while open. The one place that decides "open".
pub fn closed(events: &[Event], a: &Event) -> Option<&'static str> {
    ending(events, a).map(|e| e.0)
}

/// `closed`, and the event that closed it.
fn ending<'a>(events: &'a [Event], a: &Event) -> Option<(&'static str, &'a Event)> {
    let at = (std::ptr::from_ref(a) as usize).wrapping_sub(events.as_ptr() as usize) / size_of::<Event>();
    let at = events.get(at).filter(|e| std::ptr::eq(*e, a)).map(|_| at)?;
    let end = index(events, |x| x.ends.get(&at).copied())??;
    Some((ORDER[end.0], &events[end.1]))
}

/// #584: `closed` as `drover asks` shows it: `withdrawn → q<new>`, `delivered to t<n>`.
pub fn closed_row(events: &[Event], a: &Event) -> Option<String> {
    let (why, e) = ending(events, a)?;
    Some(match (why, e.str("for"), e.str("to")) {
        ("withdrawn", Some(q), _) => format!("withdrawn → {q}"),
        ("delivered", _, Some(t)) => format!("delivered to {t}"),
        _ => why.into(),
    })
}

/// #447: what later event `e` does to ask `a` (`closed`'s reasons, one event at a time). `own`: `e`'s
/// `spawn-started` came before the ask, so a `spawn` `e` is the asking lane itself (#446).
fn closes(a: &Event, e: &Event, own: impl FnOnce() -> bool) -> Option<&'static str> {
    VISITS.fetch_add(1, Relaxed);
    let on = |k: &str| e.kind == k && a.issue.is_some() && e.issue == a.issue;
    let mine = matches!(e.kind.as_str(), "spawn" | "lane-killed") && e.str("agent") == a.str("agent");
    let id = e.str("id") == a.str("id");
    if e.fleet != a.fleet {
        None
    } else if e.kind == "decision" && id {
        Some(if a.data.get("handoff").is_some() {
            "delivered"
        } else {
            "answered"
        })
    } else if let (true, Some(w)) = (id, e.kind.strip_prefix("ask-")) {
        ["withdrawn", "delivered", "failed", "expired"]
            .into_iter()
            .find(|k| *k == w)
    } else if on("merged") {
        Some("closed: merged")
    } else if on("closed") || on("closed-other") {
        Some("closed: issue closed")
    } else if mine && !(e.kind == "spawn" && own()) || on("clean") {
        Some("closed: lane gone")
    } else {
        None
    }
}

/// The first finding of `fleet` #`issue`'s last verdict (the line after `PASS`/`FAIL`).
pub fn finding<'a>(events: &'a [Event], fleet: &str, issue: u64) -> Option<&'a str> {
    let v = events.iter().rev().find(|e| e.is("verdict", fleet, issue))?;
    v.str("body")?.lines().skip(1).map(str::trim).find(|l| !l.is_empty())
}

/// `needs-decision` events not yet closed, oldest first (#632: the ask index's).
pub fn open_asks(events: &[Event]) -> Vec<&Event> {
    index(events, |x| x.open(events))
}

/// #859: `fleet` #`n`'s events in order: the index's when `events` is a cached log, else one pass.
pub fn of_issue<'a>(events: &'a [Event], fleet: &str, n: u64) -> Vec<&'a Event> {
    let Some(l) = cached(events) else {
        REBUILDS.fetch_add(1, Relaxed);
        return events
            .iter()
            .filter(|e| e.fleet == fleet && e.issue == Some(n))
            .collect();
    };
    let at = l.issues.get(&(fleet.to_string(), n));
    at.map_or_else(Vec::new, |v| v.iter().map(|i| &events[*i]).collect())
}

/// Current per-issue state (the states in `roles/usage-tail.md`); #859: the cached log's fold when `events` is it.
pub fn fold(events: &[Event]) -> States {
    if let Some(l) = cached(events) {
        return l.states.clone();
    }
    fold_into(States::new(), events)
}

/// #343: `fold` on from the states `m` (a checkpoint's).
pub(crate) fn fold_into(mut m: States, events: &[Event]) -> States {
    for e in events {
        let Some(n) = e.issue else { continue };
        let new = || IssueState {
            fleet: e.fleet.clone(),
            issue: n,
            state: "queued".into(),
            ..Default::default()
        };
        let s = m.entry((e.fleet.clone(), n)).or_insert_with(new);
        // A merged issue never leaves `merged`, whatever is logged after it (#350: but for its landing reverted or
        // fixed forward); a closed one only for a new spawn.
        let back = matches!(e.kind.as_str(), "reverted" | "land-fix");
        if PASSIVE.contains(&e.kind.as_str())
            || s.state == "merged" && !back
            || s.state == "closed" && e.kind != "spawn"
        {
            continue;
        }
        // Design lanes are the owner's: they never move the issue through review or merge; #412: one on the issue
        // branch (a `--design` issue's, before impl) is the issue's design stage.
        if design(e) {
            if let (Some(r), Some(a)) = (e.str("role"), e.str("agent")) {
                s.lanes.insert(r.into(), a.into());
            }
            if !DESIGN.contains(&s.state.as_str())
                || e.str("branch")
                    .is_none_or(|b| b.starts_with(&format!("{}-", e.str("role").unwrap_or_default())))
            {
                continue;
            }
            (s.state, s.lane) = ("design".into(), e.str("agent").map(String::from));
        }
        s.last_ts = e.ts;
        s.last_event.clone_from(&e.kind);
        match e.kind.as_str() {
            "spawn" if design(e) => {}
            "spawn" => {
                s.lane = e.str("agent").map(String::from);
                s.effort = e.str("effort").map(String::from);
                if let (Some(r), Some(a)) = (e.str("role"), e.str("agent")) {
                    s.lanes.insert(r.into(), a.into());
                }
                // #173: a check stage's spawn says so; before, only `rev` reviewed.
                let role = e.str("role").unwrap_or_default();
                let rev = checks(e);
                s.stage = e.str("stage").unwrap_or(role).into();
                *if rev { &mut s.check } else { &mut s.build } = Some(role.into());
                if !rev {
                    s.branch = e.str("branch").map(String::from).or(s.branch.take());
                    s.updating = e.data.get("update") == Some(&Value::Bool(true));
                }
                let u = if s.updating {
                    format!("+u{}", s.updates + 1)
                } else {
                    String::new()
                };
                let n = s.stage_rounds.get(&s.stage).copied().unwrap_or_default();
                s.state = match (rev, s.rounds) {
                    (true, _) => format!("review({}){u}", n + u32::from(!s.updating)),
                    (false, 0) => "impl".into(),
                    (false, n) => format!("fix({n}{}){u}", s.form_mark()),
                };
            }
            "verdict" => {
                let sha = e.str("sha").unwrap_or_default();
                let form = e.data.get("form_only") == Some(&Value::Bool(true));
                let v = e.str("verdict").unwrap_or_default().to_string() + if form { FORM } else { "" };
                // A round is a distinct reviewed sha; re-recording one does not burn another.
                // #123: a PASSed update round is an update; a FAILed merge-only check is a normal round.
                let new = u32::from(!s.reviewed(sha));
                // #173: a check stage counts its own reviews; a gate's (CI red) those of the check its fix goes to.
                let lp = e.str("loop").or(e.str("stage")).unwrap_or("rev").to_string();
                let mine = |x: &(String, String, String)| x.0.eq_ignore_ascii_case(sha) && x.2 == lp;
                let new_here = u32::from(!s.verdicts.iter().any(mine));
                // #228: a form-only FAIL starts a fix round but does not count toward the stage's rounds.
                if !(form || s.updating && v == "PASS") {
                    *s.stage_rounds.entry(lp.clone()).or_default() += new_here;
                }
                s.form += new_here * u32::from(form);
                *if s.updating && v == "PASS" {
                    &mut s.updates
                } else {
                    &mut s.rounds
                } += new;
                s.updating = false;
                s.verdicts.push((sha.into(), v.clone(), lp));
                s.state = if v == "PASS" {
                    "ready".into()
                } else {
                    format!("fix({}{})", s.rounds, s.form_mark())
                };
            }
            // #417: a verify stage that doesn't run at a head passes it there, in no round.
            "verify-skipped" | "verify-no-recipe" => {
                let (sha, stage) = (e.str("sha").unwrap_or_default(), e.str("stage").unwrap_or("verify"));
                s.verdicts.push((sha.into(), "PASS".into(), stage.into()));
            }
            "merged" | "closed" | "blocked" | "needs-human" | "parked" => s.state.clone_from(&e.kind),
            // #639: an issue closed on GitHub by any other route (`check`).
            "closed-other" => s.state = "closed".into(),
            // #412: the owner's decision on the design: impl (or the design again) starts from the queue.
            "design-review" => s.state = "design-review".into(),
            "design-approved" | "design-revise" => (s.state, s.lane) = ("queued".into(), None),
            "design-rejected" => (s.state, s.lane) = ("needs-human".into(), None),
            // #500: a false `LANE_ENDS` park undone: the state before it. #639: back in the queue, its build lane ends.
            // #712: the owner's `unpark`: the state before the park, the same way.
            "repaired" | "unparked" => {
                s.state = e.str("state").map_or(s.state.clone(), String::from);
                if let Some(b) = s.build.take().filter(|_| s.state == "queued") {
                    s.lanes.remove(&b);
                }
            }
            // #350: the FAIL verdict logged right after it starts the fix round.
            "reverted" | "land-fix" => s.state = format!("fix({})", s.rounds),
            "round-extended" if s.state == "needs-human" && s.card() == "start lane" => s.state = "queued".into(),
            "round-extended" => {
                (s.extra, s.granted) = (s.extra + 1, s.stage_rounds.values().max().unwrap_or(&0) + 1);
                // #151: `extend --no-spawn` spawns nothing, so the extra round itself leaves needs-human. #639: a build
                // lane never reviewed goes back to impl, not to a fix round of a FAIL that never was.
                if s.state == "needs-human" {
                    s.state = if s.verdicts.is_empty() {
                        "impl".into()
                    } else {
                        format!("fix({})", s.rounds)
                    };
                }
            }
            // #128: `clean` removes only merged or GitHub-closed lanes (logs before #128 carry no `why`).
            "clean" if e.str("why") != Some("merged") => s.state = "closed".into(),
            // Back to the state before the killed lane: queued / fix(n) for an impl, impl / fix(n) for a review.
            "lane-killed" if e.str("role").and_then(|r| s.lanes.get(r)).map(String::as_str) == e.str("agent") => {
                let role = e.str("role").unwrap_or_default();
                let chk = s.check.as_deref() == Some(role);
                s.lanes.remove(role);
                s.lane = None;
                if s.build.as_deref() == Some(role) && s.state == "impl" || chk && s.state.starts_with("review") {
                    s.state = if s.rounds > 0 {
                        format!("fix({})", s.rounds)
                    } else if chk {
                        "impl".into()
                    } else {
                        "queued".into()
                    };
                }
            }
            _ => {}
        }
    }
    debug_assert!(m.values().all(|s| state(&s.state).is_some()), "a state not in STATES");
    m
}

/// #845: every state `fold` gives an issue (a round's `(n)`, `·f` and `+uN` aside), each with its ways out.
pub const STATES: &[(&str, &[(&str, &str)])] = &[
    (
        "queued",
        &[
            ("design", "queued with an architect or design mark, that lane first"),
            (
                "impl",
                "impl spawned, refused while SERIAL_BUSY, HOST_BUSY, DISK_BUDGET, LANE_LIVE or CAP",
            ),
            ("closed", "issue closed on GitHub"),
        ],
    ),
    ("design", &[("design-review", "design pushed, the owner's ask")]),
    (
        "design-review",
        &[("queued", "approve or revise"), ("needs-human", "reject")],
    ),
    (
        "impl",
        &[
            ("review(n)", "turn done with a push, review spawned"),
            ("needs-human", "turn blocked, or stalled past stall_minutes"),
            ("queued", "lane killed, or repaired while blocked"),
        ],
    ),
    (
        "review(n)",
        &[
            ("ready", "PASS at head"),
            ("fix(n)", "FAIL at head"),
            ("needs-human", "ROUND_LIMIT after review_rounds"),
        ],
    ),
    (
        "fix(n)",
        &[
            ("review(n)", "fix pushed, next review round"),
            ("needs-human", "turn blocked, or stalled past stall_minutes"),
        ],
    ),
    (
        "ready",
        &[
            ("review(n)", "verify stage at head for ui_paths, a verify or repro mark"),
            (
                "fix(n)",
                "update round +uN when behind or conflicting, UPDATE_LIMIT after update_rounds",
            ),
            ("merged", "merge with PASS at head and CI green, no STAGING_WOULD_BREAK"),
            ("blocked", "merge refused, or PR closed outside drover"),
        ],
    ),
    (
        "blocked",
        &[
            ("fix(n)", "fix round spawned"),
            ("review(n)", "review spawned at a new head"),
        ],
    ),
    (
        "needs-human",
        &[
            ("queued", "extend with no lane yet, start lane"),
            ("fix(n)", "extend, one more round"),
            ("parked", "park"),
        ],
    ),
    ("parked", &[("needs-human", "unpark, the state before it")]),
    ("merged", &[]),
    ("closed", &[]),
];

/// #845: `state`'s `STATES` entry, matched on its name before any `(` or `+`.
pub fn state(state: &str) -> Option<&'static (&'static str, &'static [(&'static str, &'static str)])> {
    let base = state.split(['(', '+']).next();
    STATES.iter().find(|(name, _)| name.split('(').next() == base)
}

/// #412: the states a design spawn moves into `design` (before impl).
pub const DESIGN: &[&str] = &["queued", "design", "design-review"];

/// #228: a form-only FAIL's suffix in `IssueState::verdicts` (`FAIL (form-only)`).
pub const FORM: &str = " (form-only)";

impl IssueState {
    /// #228: `·f` while the latest verdict is a form-only FAIL (`fix(2·f)`).
    fn form_mark(&self) -> &'static str {
        if self.verdicts.last().is_some_and(|v| v.1.ends_with(FORM)) {
            "·f"
        } else {
            ""
        }
    }

    /// #831: a needs-human card's main action: [start lane] when no build lane ever spawned and no round was reviewed.
    pub fn card(&self) -> &'static str {
        if self.build.is_none() && self.verdicts.is_empty() {
            "start lane"
        } else {
            "one more round"
        }
    }

    /// A verdict was already recorded for this sha.
    pub fn reviewed(&self, sha: &str) -> bool {
        self.verdicts.iter().any(|(s, ..)| s.eq_ignore_ascii_case(sha))
    }

    /// Merged, or closed on GitHub: no lane, alert or queue slot any more.
    pub fn done(&self) -> bool {
        self.state == "merged" || self.state == "closed"
    }

    /// The lane that should be working now: impl in impl/fix, rev in review; none once ready, blocked or merged.
    pub fn active_lane(&self) -> Option<(&str, &str)> {
        let role = if self.state.starts_with("review") {
            self.check.as_deref()?
        } else if self.state == "impl" || self.state.starts_with("fix") {
            self.build.as_deref()?
        } else {
            return None;
        };
        self.lanes.get_key_value(role).map(|(r, a)| (r.as_str(), a.as_str()))
    }

    /// The event log still has an open lane for this role (`check`: a check stage's).
    pub fn unfinished(&self, role: &str, check: bool) -> bool {
        if check {
            self.state.starts_with("review")
        } else {
            self.lanes.contains_key(role) && !self.done()
        }
    }

    /// #173: the check lane (the latest check role's agent).
    pub fn check_lane(&self) -> Option<&String> {
        self.lanes.get(self.check.as_deref()?)
    }

    /// #173: the shown state: a check stage other than `rev` shows `<stage>(n)` for `review(n)`.
    pub fn shown(&self) -> String {
        match self.state.strip_prefix("review") {
            Some(n) if self.check.is_some() && self.stage != "rev" => format!("{}{n}", self.stage),
            _ => self.state.clone(),
        }
    }
}

pub fn age(secs: u64) -> String {
    match secs {
        s if s < 120 => format!("{s}s"),
        s if s < 7200 => format!("{}m", s / 60),
        s if s < 172_800 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86400),
    }
}

pub fn status_line(s: &IssueState, now: u64) -> String {
    let lane = s.lane.as_deref().unwrap_or("-");
    format!(
        "{:<12} #{:<5} {:<12} {lane:<24} {}",
        s.fleet,
        s.issue,
        s.shown(),
        age(now.saturating_sub(s.last_ts))
    )
}
