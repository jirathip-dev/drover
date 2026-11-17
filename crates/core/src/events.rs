use crate::config::Config;
use crate::runner::{Real, Runner};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;

use crate::db::Endings;

mod passive;
mod push;

#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(optional_fields = nullable))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// The log's monotonic number, set on append (an old row's is its line number); 0 before.
    #[serde(default, skip_serializing_if = "is_zero")]
    #[cfg_attr(feature = "ts", ts(as = "Option<u64>", optional))]
    pub seq: u64,
    pub ts: u64,
    pub fleet: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<u64>,
    #[serde(rename = "type")]
    pub kind: String,
    /// Not activity (an alert, a note, a prompt): the fold and the idle-pane reader skip it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    pub passive: bool,
    /// Reaches a phone whose notifier sets no `on`; kinds pushed before the flag are in `push::LEGACY`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(feature = "ts", ts(as = "Option<bool>", optional))]
    pub push: bool,
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
            passive: false,
            push: false,
            data,
        }
    }
    /// This event is not activity.
    #[must_use]
    pub fn passive(mut self) -> Event {
        self.passive = true;
        self
    }
    /// Flagged passive, or a kind logged passive before the flag.
    pub fn is_passive(&self) -> bool {
        self.passive || passive::LEGACY.contains(&self.kind.as_str())
    }
    /// This event reaches a phone whose notifier sets no `on`.
    #[must_use]
    pub fn push(mut self) -> Event {
        self.push = true;
        self
    }
    /// Flagged push, or a kind pushed before the flag.
    pub fn is_push(&self) -> bool {
        self.push || push::LEGACY.contains(&self.kind.as_str())
    }
    pub fn str(&self, k: &str) -> Option<&str> {
        self.data.get(k).and_then(Value::as_str)
    }
    /// The value at `k`, `Null` when the event has none.
    pub fn val(&self, k: &str) -> &Value {
        self.data.get(k).unwrap_or(&Value::Null)
    }
    /// A `kind` event about `fleet` #`issue`.
    pub fn is(&self, kind: &str, fleet: &str, issue: u64) -> bool {
        self.kind == kind && self.fleet == fleet && self.issue == Some(issue)
    }
}

/// Panic-free `Value` access: `at` reads like `v[i]` (`Null` when missing), `put` writes like `v[k] = x`.
pub trait Json {
    fn at<I: serde_json::value::Index>(&self, i: I) -> &Value;
    fn put(&mut self, k: &str, v: Value) -> &mut Value;
}

impl Json for Value {
    fn at<I: serde_json::value::Index>(&self, i: I) -> &Value {
        self.get(i).unwrap_or(&Value::Null)
    }
    fn put(&mut self, k: &str, v: Value) -> &mut Value {
        if !self.is_object() {
            *self = Value::Object(Map::new());
        }
        if let Value::Object(m) = self {
            m.insert(k.into(), v);
        }
        self
    }
}

pub(crate) fn aliased(mut ev: Event) -> Event {
    if let Some(k) = ev.kind.strip_prefix("conductor-") {
        ev.kind = format!("orchestrator-{k}");
    }
    if ev.str("from") == Some("conductor") {
        ev.data.insert("from".into(), "orchestrator".into());
    }
    if let Some(b) = ev.str("by").filter(|b| b.starts_with("conductor-")) {
        ev.data.insert("by".into(), crate::orchestrator::by(b).into());
    }
    ev
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if passes a reference"
)]
fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// One event, numbered next, into the database beside `path` (`db::append`): every writer goes through here.
pub fn append(path: &Path, ev: &Event) -> Result<()> {
    crate::db::append(path, ev)
}

/// The live log (without the events a rotation archived), by seq.
pub fn read(path: &Path) -> Result<Arc<Log>> {
    shared(path)
}

/// The log in the legacy JSONL format, one event a line with its seq (`drover events --jsonl`).
pub fn jsonl(path: &Path) -> Result<String> {
    history(path)?
        .iter()
        .map(|e| Ok(serde_json::to_string(e)? + "\n"))
        .collect()
}

pub(crate) fn read_file(path: &Path) -> Result<Vec<Event>> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(parse(&t, path)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.into()),
    }
}

pub(crate) fn parse(text: &str, path: &Path) -> Vec<Event> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::with_capacity(lines.len());
    for (i, l) in lines.iter().enumerate() {
        match serde_json::from_str::<Event>(l) {
            Ok(ev) if ev.seq == 0 => out.push(aliased(Event {
                seq: u64::try_from(i + 1).unwrap_or(u64::MAX),
                ..ev
            })),
            Ok(ev) => out.push(aliased(ev)),
            Err(_) if l.trim().is_empty() || i + 1 == lines.len() => {}
            Err(e) => crate::stderr!(
                "drover: {}: line {}: skipping corrupt record: {e}",
                path.display(),
                i + 1
            ),
        }
    }
    out
}

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

/// Every event, the archived ones too (and the archive files'), by seq.
pub fn history(path: &Path) -> Result<Vec<Event>> {
    let mut all = [crate::db::archived(path)?, crate::db::events(path, "seq > ?1", 0)?].concat();
    all.sort_by_key(|e| e.seq);
    all.dedup_by_key(|e| e.seq);
    Ok(all)
}

/// The current per-issue state: the `issues` table, kept by each append.
pub fn states(path: &Path) -> Result<States> {
    crate::db::issues(path)
}

/// The most bytes of JSON one event is stored in.
pub const EVENT_MAX: usize = 16 << 10;

/// `ev` in at most `EVENT_MAX` bytes: long strings keep a head and a tail, long arrays their first items.
pub fn capped(ev: &Event) -> Event {
    let size = |e: &Event| serde_json::to_string(e).map_or(0, |s| s.len());
    let mut keep = EVENT_MAX / 2;
    let mut out = ev.clone();
    while size(&out) > EVENT_MAX && keep >= 64 {
        out.data.clone_from(&ev.data);
        out.data.values_mut().for_each(|v| cut(v, keep));
        keep /= 2;
    }
    out
}

fn cut(v: &mut Value, keep: usize) {
    match v {
        Value::String(s) if s.len() > keep => {
            let (h, t) = (
                s.floor_char_boundary(keep / 2),
                s.ceil_char_boundary(s.len() - keep / 2),
            );
            *s = format!("{}… {} bytes cut …{}", &s[..h], t - h, &s[t..]);
        }
        Value::Array(a) => {
            let n = (keep / 256).max(1);
            if a.len() > n {
                let more = a.len() - n;
                a.truncate(n);
                a.push(format!("… {more} more").into());
            }
            for x in a.iter_mut() {
                cut(x, keep);
            }
        }
        Value::Object(m) => m.values_mut().for_each(|x| cut(x, keep)),
        _ => {}
    }
}

pub fn offload(state: &Path, mut d: Map<String, Value>) -> Result<Map<String, Value>> {
    let Some(errs) = d.get("errors").and_then(Value::as_array).filter(|a| !a.is_empty()) else {
        return Ok(d);
    };
    let text = errs.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n");
    let hash = crate::devices::sha256(text.as_bytes());
    let rel = format!("cache/build-cloned/{}.txt", &hash[..32]);
    let file = path_in(state, &rel);
    std::fs::create_dir_all(file.parent().unwrap_or(Path::new(".")))?;
    std::fs::write(file, text)?;
    d.remove("errors");
    d.insert("errors_ref".into(), rel.into());
    Ok(d)
}

/// `rel` under the state dir of the log at `state`.
pub fn path_in(state: &Path, rel: &str) -> PathBuf {
    state.parent().unwrap_or(Path::new(".")).join(rel)
}

/// Append, then run notifiers. Notifier failures never fail the caller.
#[expect(clippy::needless_pass_by_value, reason = "callers hand over the event they built")]
pub fn emit(state: &Path, cfg: &Config, ev: Event) -> Result<()> {
    append(state, &ev)?;
    notify(state, cfg, &ev)
}

/// `db::ask`, then notify each event logged: they, the asks numbered, or none when `once` is logged already.
pub fn ask(state: &Path, cfg: &Config, evs: Vec<Event>, once: Option<&str>) -> Result<Vec<Event>> {
    let evs = crate::db::ask(state, evs, once)?.unwrap_or_default();
    evs.iter().try_for_each(|ev| notify(state, cfg, ev))?;
    Ok(evs)
}

fn notify(state: &Path, cfg: &Config, ev: &Event) -> Result<()> {
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
        "ask-owner" => "needs-decision",
        "decision" if ev.str("by").is_some_and(|b| b.starts_with("orchestrator-")) => "",
        "thread-msg" if ev.str("from") != Some("orchestrator") => "",
        k => k,
    };
    let phone = (!push.is_empty()).then(|| Event {
        kind: push.into(),
        ..ev.clone()
    });
    let wanted = |n: &&crate::config::Notify| match (n.webpush(), &phone) {
        (false, _) => n.wants(ev),
        (true, p) => p.as_ref().is_some_and(|p| n.wants(p)),
    };
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
            crate::stderr!("drover: notifier `{cmd}` failed: {e:#}");
            let d = serde_json::json!({"cmd": cmd, "event": ev.kind, "error": format!("{e:#}")});
            append(
                state,
                &Event::new(ev.ts, &ev.fleet, ev.issue, "notify-failed", d).passive(),
            )
            .ok();
        }
    }
    Ok(())
}

/// One human-readable line, e.g. `api #N merged → staging 8ed4a06 (PR #M)`.
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
            let rec = ev.val("recommend").as_u64();
            let opts: String = ev
                .val("options")
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
            let n: Vec<String> = ev
                .val("issues")
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
    let mut args: Vec<String> = crate::config::envs()
        .filter_map(|(k, _)| k.into_string().ok())
        .filter(|k| crate::providers::all().any(|p| p.scrubs_env(k)))
        .flat_map(|k| ["-u".into(), k])
        .collect();
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
    /// Extra rounds the owner allowed (`round-extended`); the latest grants the round after any stage's.
    pub extra: u32,
    pub granted: u32,
    /// Passed `--update` rounds, apart from `rounds`; `updating` while one is open (state `fix(n)+uk`).
    pub updates: u32,
    pub updating: bool,
    pub last_ts: u64,
    pub last_event: String,
    /// Latest agent per role (impl, rev).
    pub lanes: BTreeMap<String, String>,
    /// (sha, PASS|FAIL, the check stage whose rounds it counts) in log order; older logs: `rev`.
    pub verdicts: Vec<(String, String, String)>,
    /// The pipeline stage of the latest spawn, the latest check and build roles, review rounds per check stage.
    pub stage: String,
    pub check: Option<String>,
    pub build: Option<String>,
    pub stage_rounds: BTreeMap<String, u32>,
    /// `FAIL (form-only)` verdicts that did not count toward the rounds.
    pub form: u32,
}

/// Per-issue state, keyed by (fleet, issue).
pub type States = BTreeMap<(String, u64), IssueState>;

/// The newest `spawn` of `agent` (the lane fold's, for a cached log).
pub fn last_spawn<'a>(events: &'a [Event], agent: &str) -> Option<&'a Event> {
    let spawn = |e: &&Event| e.kind == "spawn" && e.str("agent") == Some(agent);
    match cached(events) {
        Some(l) => l.lane_at.iter().rev().filter_map(|i| events.get(*i)).find(spawn),
        None => events.iter().rfind(spawn),
    }
}

const LANE: [&str; 14] = [
    "spawn",
    "spawn-started",
    "spawn-adopted",
    "lane-killed",
    "lane-closed",
    "lane-dead",
    "stale-pane-killed",
    "lane-kept-dirty",
    "wip-blocked",
    "supersede",
    "clean",
    "stale-marker",
    "root-hung",
    "root-answers",
];

pub(crate) fn lane_id(e: &Event) -> Option<String> {
    let at = [
        "host",
        "agent",
        "machine",
        "pane",
        "worktree",
        "key",
        "stale_host",
        "stale_pane",
        "file",
        "root",
    ];
    LANE.contains(&e.kind.as_str())
        .then(|| format!("{} {:?} {:?}", e.kind, e.issue, at.map(|k| e.str(k))))
}

/// The lane fold of `events` (each lane's newest lane event), in log order: the cached log's, else one pass.
pub fn lanes(events: &[Event]) -> Vec<&Event> {
    if let Some(l) = cached(events) {
        return l.lane_at.iter().filter_map(|i| events.get(*i)).collect();
    }
    let mut m = HashMap::new();
    for (i, e) in events.iter().enumerate() {
        m.extend(lane_id(e).map(|k| (k, i)));
    }
    let mut at: Vec<usize> = m.into_values().collect();
    at.sort_unstable();
    at.into_iter().filter_map(|i| events.get(i)).collect()
}

/// The issue `s`'s lane is held on (`held`, no `released` or `spawn` since). the issues it waits on (`on`: one number in older logs, else a list); empty when not held.
pub fn held_on(events: &[Event], s: &IssueState) -> Vec<u64> {
    let kinds = ["held", "released", "spawn"];
    let pick = |e: &&Event| kinds.contains(&&*e.kind) && !design(e);
    let e = issue_events(events, &s.fleet, s.issue).into_iter().rfind(pick);
    e.filter(|e| e.kind == "held")
        .map_or(vec![], |e| crate::queue::nums(e.val("on")))
}

/// A design (or architect) lane's spawn: never the issue's build or check lane.
pub fn design(e: &Event) -> bool {
    e.kind == "spawn" && e.str("role").is_some_and(|r| STAGES.contains(&r))
}

/// The owner-reviewed pre-impl roles, in the order they run.
pub const STAGES: [&str; 2] = ["architect", "design"];

/// Lane end `e` ends spawn `sp` of its agent: only that pane's when `e` names one; a supersede logged without a pane (older logs, after the new spawn) ends none.
pub fn ends(e: &Event, sp: &Event) -> bool {
    let pane = e.str("pane").map(|p| sp.str("pane") == Some(p));
    e.str("agent") == sp.str("agent") && pane.unwrap_or(e.str("reason") != Some("superseded"))
}

/// A check stage's spawn (older logs: a `rev` one).
pub fn checks(e: &Event) -> bool {
    e.data
        .get("check")
        .and_then(Value::as_bool)
        .unwrap_or(e.str("role") == Some("rev"))
}

/// An ask's endings, the first that applies naming why it closed.
pub const ORDER: [&str; 9] = [
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

/// (ask, event) pairs checked: the ask endings' work, counted.
pub static VISITS: AtomicUsize = AtomicUsize::new(0);

#[derive(Default, Clone)]
pub struct Threads {
    pub new: Vec<usize>,
    of: HashMap<String, Vec<usize>>,
    asks: HashMap<(String, String), usize>,
    decisions: HashMap<(String, String), usize>,
    pub forwards: std::collections::BTreeSet<u64>,
}

impl Threads {
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

/// The `Log` whose events `events` are, the newest or one a reader still holds (none: read some other way).
pub fn cached(events: &[Event]) -> Option<Arc<Log>> {
    let live = LIVE.lock().unwrap_or_else(PoisonError::into_inner);
    live.iter()
        .filter_map(Weak::upgrade)
        .find(|l| std::ptr::eq(&raw const *l.events, events))
}

/// The first `needs-decision` (`ask`) or the last `decision` of `fleet`'s ask `id`: the index's when `events` is a cached log, else one pass.
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
    map.get(&(fleet.into(), id.into())).and_then(|i| events.get(*i))
}

/// `e` repeats an ask logged before it (two passes raced to post it): every view shows the first alone.
pub fn repeat(events: &[Event], e: &Event) -> bool {
    closed(events, e) == Some("repeat")
}

/// An event kind that logs an ask or may end one (`closes`).
pub fn ends_asks(kind: &str) -> bool {
    matches!(
        kind,
        "needs-decision" | "decision" | "merged" | "closed" | "closed-other" | "clean" | "spawn" | "lane-killed"
    ) || kind.starts_with("ask-")
}

pub(crate) fn twin(e: &Event) -> Option<String> {
    let k = (&e.fleet, e.issue, e.str("thread"), e.str("question"));
    e.str("id").map(|id| format!("{id} {k:?}"))
}

/// Ask endings projected from a slice that is no cached `Log` (`index`'s fallback): serve's reads build none.
pub static REBUILDS: AtomicUsize = AtomicUsize::new(0);

fn index<T>(events: &[Event], f: impl FnOnce(&Endings, &dyn Fn(usize) -> u64) -> T) -> T {
    if let Some(l) = cached(events) {
        return f(&l.asks, &|i| events.get(i).map_or(0, |e| e.seq));
    }
    REBUILDS.fetch_add(1, Relaxed);
    let x = crate::db::endings(events).unwrap_or_default();
    f(&x, &|i| u64::try_from(i + 1).unwrap_or(u64::MAX))
}

/// The live log (`read`), its ask index and fold, cached per log by its last seq: a new event is read and folded alone (a rotation or another database file since: all again).
#[derive(Default, Clone)]
pub struct Log {
    pub events: Events,
    pub asks: Endings,
    lanes: HashMap<String, usize>,
    lane_at: std::collections::BTreeSet<usize>,
    pub threads: Threads,
    pub states: States,
    issues: HashMap<(String, u64), Vec<usize>>,
    kinds: HashMap<String, Vec<usize>>,
    agents: HashMap<String, Vec<usize>>,
    copies: Arc<Mutex<HashMap<(String, u64), Events>>>,
    queues: Queues,
    seq: u64,
    key: (Option<u64>, Option<u64>),
}

impl std::ops::Deref for Log {
    type Target = [Event];
    fn deref(&self) -> &[Event] {
        &self.events
    }
}

/// A log's events, in a buffer its later versions share and append to in place.
pub type Events = drover_sys::Append<Event>;

type Gated = (Arc<Mutex<()>>, Arc<Log>);

type Queues = Arc<Mutex<HashMap<String, (usize, Vec<crate::queue::Item>)>>>;

pub(crate) fn queues(events: &[Event]) -> Option<Queues> {
    cached(events).map(|l| l.queues.clone())
}

static LOGS: Mutex<BTreeMap<(PathBuf, bool), Gated>> = Mutex::new(BTreeMap::new());

static LIVE: Mutex<Vec<Weak<Log>>> = Mutex::new(Vec::new());

/// The cached `Log` of `path` (by its database: a demo's scratch one apart), brought up to its last seq: each call reads only the events appended since (serve's requests and watcher).
pub fn shared(path: &Path) -> Result<Arc<Log>> {
    log(path, false)
}

/// The `WORKING` events and the saved fold, cached like `shared` (whose log a process holding it reads instead).
pub fn working(path: &Path) -> Result<Arc<Log>> {
    let full = (LOGS.lock().unwrap_or_else(PoisonError::into_inner))
        .get(&(crate::db::file(path), false))
        .is_some_and(|l| l.1.seq > 0);
    log(path, !full)
}

const WORKING: &str = concat!(
    "archived = 0 AND seq <= ?1 AND type NOT IN ('build-cloned', 'build-reused') AND (seq > ?1 - 10000",
    " OR (fleet, issue) IN (SELECT fleet, issue FROM states WHERE done = 0)",
    " OR seq IN (SELECT MAX(seq) FROM events WHERE seq <= ?1 AND type IN ('driven', 'kick', 'drive-pass', 'lane-state', 'conflict-predicted', 'intent', 'push-sent', 'pass-slow')",
    " GROUP BY type, fleet, issue, json_extract(json, '$.agent'), json_extract(json, '$.step'))",
    " OR type = 'lane-state' AND json_extract(json, '$.state') = 'gone'",
    " OR type NOT IN ('driven', 'kick', 'drive-pass', 'lane-state', 'conflict-predicted', 'intent', 'push-sent', 'pass-slow') AND (issue IS NULL",
    " OR (fleet, issue) IN (SELECT fleet, issue FROM events WHERE seq > ?1 - 10000) OR type GLOB 'ask-*' OR type IN (",
    "'needs-decision', 'decision', 'queue', 'merged', 'closed', 'closed-other', 'why', ",
    "'premerge-failed', 'landed', 'revert-opened', ",
    "'revert-failed', 'reverted', 'land-fix', 'land-bisect', 'lesson-skipped', 'compacted', 'compact-undone'",
    ")))"
);

/// The live events of `kinds` logged at `ts` or later, by seq: a reader over more history than `working` holds.
pub fn since(path: &Path, kinds: &[&str], ts: u64) -> Result<Vec<Event>> {
    let kinds: Vec<String> = kinds.iter().map(|k| format!("'{k}'")).collect();
    crate::db::events(
        path,
        &format!("archived = 0 AND ts >= ?1 AND type IN ({})", kinds.join(", ")),
        ts,
    )
}

/// At most `limit` live events numbered after `since`, up to `until`, by seq: the board's event feed.
pub fn between(path: &Path, (since, until): (u64, u64), limit: usize) -> Result<Vec<Event>> {
    let (until, limit) = (
        i64::try_from(until).unwrap_or(i64::MAX),
        i64::try_from(limit).unwrap_or(i64::MAX),
    );
    crate::db::events(
        path,
        &format!(
            "seq IN (SELECT seq FROM events WHERE archived = 0 AND seq > ?1 AND seq <= {until} ORDER BY seq LIMIT {limit})"
        ),
        since,
    )
}

fn log(path: &Path, live: bool) -> Result<Arc<Log>> {
    let (file, logs) = ((crate::db::file(path), live), || {
        LOGS.lock().unwrap_or_else(PoisonError::into_inner)
    });
    let gate = logs().entry(file.clone()).or_default().0.clone();
    let _one = gate.lock().unwrap_or_else(PoisonError::into_inner);
    let (last, key) = (crate::db::last_seq(path), crate::db::version(path));
    let mut l = {
        let mut m = logs();
        let l = &mut m.entry(file.clone()).or_default().1;
        if key == l.key && last == l.seq && last > 0 {
            return Ok(l.clone());
        }
        std::mem::take(l)
    };
    let got = catch_up(path, &mut l, last, key, live);
    logs().entry(file).or_default().1 = l.clone();
    let mut live = LIVE.lock().unwrap_or_else(PoisonError::into_inner);
    live.retain(|w| w.strong_count() > 0 && !std::ptr::eq(w.as_ptr(), Arc::as_ptr(&l)));
    live.push(Arc::downgrade(&l));
    got.map(|()| l)
}

fn catch_up(path: &Path, l: &mut Arc<Log>, last: u64, key: (Option<u64>, Option<u64>), live: bool) -> Result<()> {
    if key != l.key {
        *l = Arc::new(Log { key, ..Log::default() });
    }
    let fresh = l.seq == 0 && l.events.is_empty();
    let (mut new, states, asks) = crate::db::since(path, l.seq, fresh, (live && fresh).then_some(WORKING))?;
    if fresh {
        new.extend(crate::db::lanes(path)?);
        new.sort_by_key(|e| e.seq);
        new.dedup_by_key(|e| e.seq);
    }
    let m = Arc::make_mut(l);
    m.seq = new.last().map_or(last, |e| e.seq.max(last));
    m.states.extend(states);
    m.asks.extend(asks);
    m.events.reserve(new.len());
    for e in new {
        if let Some(n) = e.issue {
            m.issues.entry((e.fleet.clone(), n)).or_default().push(m.events.len());
        }
        if let Some(k) = lane_id(&e) {
            let old = m.lanes.insert(k, m.events.len());
            old.map(|i| m.lane_at.remove(&i));
            m.lane_at.insert(m.events.len());
        }
        m.kinds.entry(e.kind.clone()).or_default().push(m.events.len());
        if let Some(a) = e.str("agent") {
            m.agents.entry(a.into()).or_default().push(m.events.len());
        }
        m.events.push(e);
        m.threads.add(&m.events);
    }
    Ok(())
}

/// Why ask `a` closed (every ending): answered, withdrawn, delivered or failed (a handoff card), expired, its issue merged, or its lane killed, cleaned or respawned; `None` while open. The one place that decides "open".
pub fn closed(events: &[Event], a: &Event) -> Option<&'static str> {
    ending(events, a).map(|e| e.0)
}

fn ending(events: &[Event], a: &Event) -> Option<(&'static str, String)> {
    let at = std::ptr::from_ref(a).addr().wrapping_sub(events.as_ptr().addr()) / size_of::<Event>();
    let at = events.get(at).filter(|e| std::ptr::eq(*e, a)).map(|_| at)?;
    let (r, row) = index(events, |x, key| x.get(&key(at)).cloned())??;
    Some((*ORDER.get(r)?, row))
}

/// `closed` as `drover asks` shows it: `withdrawn → q<new>`, `delivered to t<n>`.
pub fn closed_row(events: &[Event], a: &Event) -> Option<String> {
    ending(events, a).map(|e| e.1)
}

pub(crate) fn ending_row(why: &str, e: &Event) -> String {
    match (why, e.str("for"), e.str("to")) {
        ("withdrawn", Some(q), _) => format!("withdrawn → {q}"),
        ("delivered", _, Some(t)) => format!("delivered to {t}"),
        _ => why.into(),
    }
}

/// Decision or `ask-*` ending `e` is about ask `a` (by `ask_seq`, else by id on its issue).
pub fn names(e: &Event, a: &Event) -> bool {
    let seq = e.data.get("ask_seq").and_then(Value::as_u64);
    e.str("id") == a.str("id") && seq.map_or(e.issue.is_none() || e.issue == a.issue, |s| s == a.seq)
}

pub(crate) fn closes(a: &Event, e: &Event, own: impl FnOnce() -> bool) -> Option<&'static str> {
    VISITS.fetch_add(1, Relaxed);
    let on = |k: &str| e.kind == k && a.issue.is_some() && e.issue == a.issue;
    let mine = matches!(e.kind.as_str(), "spawn" | "lane-killed") && e.str("agent") == a.str("agent");
    let id = names(e, a);
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

/// `needs-decision` events not yet closed, oldest first (the `asks` table's, for a cached log).
pub fn open_asks(events: &[Event]) -> Vec<&Event> {
    index(events, |x, key| {
        let open =
            |(i, e): &(usize, &Event)| e.kind == "needs-decision" && x.get(&key(*i)).is_some_and(Option::is_none);
        events.iter().enumerate().filter(open).map(|(_, e)| e).collect()
    })
}

/// `fleet` #`n`'s events in order: the index's when `events` is a cached log, else one pass.
pub fn of_issue<'a>(events: &'a [Event], fleet: &str, n: u64) -> Vec<&'a Event> {
    let Some(l) = cached(events) else {
        REBUILDS.fetch_add(1, Relaxed);
        return events
            .iter()
            .filter(|e| e.fleet == fleet && e.issue == Some(n))
            .collect();
    };
    let at = l.issues.get(&(fleet.to_string(), n));
    at.map_or_else(Vec::new, |v| v.iter().filter_map(|i| events.get(*i)).collect())
}

pub(crate) fn issue_events<'a>(events: &'a [Event], fleet: &str, n: u64) -> Vec<&'a Event> {
    match cached(events) {
        Some(_) => of_issue(events, fleet, n),
        None => (events.iter())
            .filter(|e| e.fleet == fleet && e.issue == Some(n))
            .collect(),
    }
}

pub(crate) fn last_at<'a>(events: &'a [Event], kind: &str, (fleet, n): (&str, u64), ts: u64) -> Option<&'a Event> {
    issue_events(events, fleet, n)
        .into_iter()
        .rfind(|e| e.kind == kind && e.ts == ts)
}

/// The events of any of `kinds`, in order: the index's when `events` is a cached log, else one pass.
pub fn of_kind<'a>(events: &'a [Event], kinds: &[&str]) -> Vec<&'a Event> {
    let Some(l) = cached(events) else {
        return events.iter().filter(|e| kinds.contains(&e.kind.as_str())).collect();
    };
    let mut at: Vec<usize> = kinds
        .iter()
        .filter_map(|k| l.kinds.get(*k))
        .flatten()
        .copied()
        .collect();
    at.sort_unstable();
    at.into_iter().filter_map(|i| events.get(i)).collect()
}

/// `fleet` #`n`'s events as one slice: the cached log's copy, extended by the events since (else copied now).
pub fn issue_log(events: &[Event], fleet: &str, n: u64) -> Events {
    let mut out = Events::default();
    let Some(l) = cached(events) else {
        (events.iter().filter(|e| e.fleet == fleet && e.issue == Some(n))).for_each(|e| out.push(e.clone()));
        return out;
    };
    let at = l.issues.get(&(fleet.to_string(), n)).map_or(&[][..], Vec::as_slice);
    let mut copies = l.copies.lock().unwrap_or_else(PoisonError::into_inner);
    let c = copies.entry((fleet.to_string(), n)).or_default();
    at.iter()
        .skip(c.len())
        .filter_map(|i| events.get(*i))
        .for_each(|e| c.push(e.clone()));
    c.prefix(at.len())
}

/// The events naming `agent`, in order: the index's when `events` is a cached log, else one pass.
pub fn of_agent<'a>(events: &'a [Event], agent: &str) -> Vec<&'a Event> {
    let Some(l) = cached(events) else {
        return events.iter().filter(|e| e.str("agent") == Some(agent)).collect();
    };
    let at = l.agents.get(agent);
    at.map_or_else(Vec::new, |v| v.iter().filter_map(|i| events.get(*i)).collect())
}

/// Current per-issue state (the states in `roles/usage/20-states.md`); the cached log's fold when `events` is it.
pub fn fold(events: &[Event]) -> States {
    if let Some(l) = cached(events) {
        return l.states.clone();
    }
    fold_into(States::new(), events)
}

/// `f` on the per-issue states: the cached log's, never copied, when `events` is it; else folded now.
pub fn with_states<T>(events: &[Event], f: impl FnOnce(&States) -> T) -> T {
    match cached(events) {
        Some(l) => f(&l.states),
        None => f(&fold_into(States::new(), events)),
    }
}

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
        let back = matches!(e.kind.as_str(), "reverted" | "land-fix");
        if e.is_passive() || s.state == "merged" && !back || s.state == "closed" && e.kind != "spawn" {
            continue;
        }
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
                let new = u32::from(!s.reviewed(sha));
                let lp = e.str("loop").or(e.str("stage")).unwrap_or("rev").to_string();
                let mine = |x: &(String, String, String)| x.0.eq_ignore_ascii_case(sha) && x.2 == lp;
                let new_here = u32::from(!s.verdicts.iter().any(mine));
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
            "verify-skipped" | "verify-no-recipe" => {
                let (sha, stage) = (e.str("sha").unwrap_or_default(), e.str("stage").unwrap_or("verify"));
                s.verdicts.push((sha.into(), "PASS".into(), stage.into()));
            }
            "merged" | "closed" | "blocked" | "needs-human" | "parked" => s.state.clone_from(&e.kind),
            "closed-other" => s.state = "closed".into(),
            "design-review" => s.state = "design-review".into(),
            "design-approved" | "design-revise" => (s.state, s.lane) = ("queued".into(), None),
            "design-rejected" => (s.state, s.lane) = ("needs-human".into(), None),
            "repaired" | "unparked" => {
                s.state = e.str("state").map_or(s.state.clone(), String::from);
                if let Some(b) = s.build.take().filter(|_| s.state == "queued") {
                    s.lanes.remove(&b);
                }
            }
            "reverted" | "land-fix" => s.state = format!("fix({})", s.rounds),
            "round-extended" if s.state == "needs-human" && s.card() == "start lane" => s.state = "queued".into(),
            "round-extended" => {
                (s.extra, s.granted) = (s.extra + 1, s.stage_rounds.values().max().unwrap_or(&0) + 1);
                if s.state == "needs-human" && e.data.get("queued") == Some(&Value::Bool(true)) {
                    s.state = "queued".into();
                } else if s.state == "needs-human" {
                    s.state = if s.verdicts.is_empty() {
                        "impl".into()
                    } else {
                        format!("fix({})", s.rounds)
                    };
                }
            }
            crate::pending::REQUEUED if s.state != "parked" => s.state = "queued".into(),
            "clean" if e.str("why") != Some("merged") => s.state = "closed".into(),
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

/// Every state `fold` gives an issue (a round's `(n)`, `·f` and `+uN` aside), each with its ways out.
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
            ("merged", "squash-merge into one commit carrying its why notes, with PASS at head and CI green, no STAGING_WOULD_BREAK"),
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

/// `state`'s `STATES` entry, matched on its name before any `(` or `+`.
pub fn state(state: &str) -> Option<&'static (&'static str, &'static [(&'static str, &'static str)])> {
    let base = state.split(['(', '+']).next();
    STATES.iter().find(|(name, _)| name.split('(').next() == base)
}

/// The states a design spawn moves into `design` (before impl).
pub const DESIGN: &[&str] = &["queued", "design", "design-review"];

/// A form-only FAIL's suffix in `IssueState::verdicts` (`FAIL (form-only)`).
pub const FORM: &str = " (form-only)";

impl IssueState {
    fn form_mark(&self) -> &'static str {
        if self.verdicts.last().is_some_and(|v| v.1.ends_with(FORM)) {
            "·f"
        } else {
            ""
        }
    }

    /// A needs-human card's main action: [start lane] when no build lane ever spawned and no round was reviewed.
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

    /// The check lane (the latest check role's agent).
    pub fn check_lane(&self) -> Option<&String> {
        self.lanes.get(self.check.as_deref()?)
    }

    /// The shown state: a check stage other than `rev` shows `<stage>(n)` for `review(n)`.
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
