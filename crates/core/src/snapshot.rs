use crate::config::Config;
use crate::events::{Event, IssueState, States};
use crate::lanes::{self, Lane};
use crate::runner::{argv, Out, Runner};
use crate::session::Sessions;
use crate::waiting::Waiting;
use crate::Ctx;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// Each read's stdout by command line (None: it failed), and when the step took them.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub ts: u64,
    pub reads: BTreeMap<String, Option<String>>,
    /// The step's waits, and what they were worked out from (`wait_key`).
    pub waits: Vec<Waiting>,
    pub key: String,
}

pub fn key(prog: &str, args: &[String]) -> String {
    format!("{prog} {}", args.join(" "))
}

/// A remote read with stdin (`shed connect`) by its command line and its requests.
pub fn key_in(prog: &str, args: &[String], input: &str) -> String {
    format!("{}\n{input}", key(prog, args))
}

/// A read through another host (ssh).
pub fn remote(prog: &str) -> bool {
    prog == "ssh"
}

/// The `board` row of the database's snapshots.
pub fn load(ctx: &Ctx) -> Option<Snapshot> {
    crate::db::get(&ctx.state, "board").map(|s| s.1)
}

pub fn out(v: Option<String>) -> Out {
    let code = i32::from(v.is_none());
    Out {
        code,
        stdout: v.unwrap_or_default(),
        ..Out::default()
    }
}

/// What a read the snapshot lacks does: `Collect` (the step's: runs and is kept), `Live` (runs), `Local` (a remote one fails, the rest runs).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Miss {
    Collect,
    Live,
    Local,
}

/// Run `lines` (quoted command lines) in one `sh -c` on `r`; each one's stdout, None when it failed.
pub fn batch(r: &dyn Runner, lines: &[String]) -> Vec<Option<String>> {
    let script: String = (lines.iter())
        .map(|l| format!("{l} </dev/null 2>/dev/null; printf '\\036%s\\036' \"$?\"\n"))
        .collect();
    let text = r
        .run("sh", &argv(&["-c", &script]))
        .map(|o| o.stdout)
        .unwrap_or_default();
    let mut parts = text.split('\x1e');
    let mut each = || {
        let (o, code) = (parts.next(), parts.next());
        o.filter(|_| code == Some("0")).map(String::from)
    };
    lines.iter().map(|_| each()).collect()
}

/// A `batch` script answered probe by probe through `one`; any other command goes to `one` whole.
pub fn each_probe(
    prog: &str,
    args: &[String],
    one: &dyn Fn(&str, &[String]) -> anyhow::Result<Out>,
) -> anyhow::Result<Out> {
    let script = match (prog, args) {
        ("sh", [c, s]) if c == "-c" && s.contains("\\036") => s,
        _ => return one(prog, args),
    };
    let mut stdout = String::new();
    for l in script.lines() {
        let cmd = l.split(" </dev/null").next().unwrap_or_default().trim_matches('\'');
        let words: Vec<String> = cmd.split("' '").map(|w| w.replace("'\\''", "'")).collect();
        let Some((p, rest)) = words.split_first() else { continue };
        let o = one(p, rest)?;
        stdout += &format!("{}\x1e{}\x1e", o.stdout, o.code);
    }
    Ok(Out {
        stdout,
        ..Out::default()
    })
}

pub struct Snap<'a> {
    r: &'a dyn Runner,
    miss: Miss,
    pub reads: Mutex<BTreeMap<String, Option<String>>>,
    pub ts: Option<u64>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl<'a> Snap<'a> {
    /// `status` and `hosts`: the saved snapshot, a remote read it lacks failing; none saved: every read live.
    pub fn read(ctx: &Ctx, r: &'a dyn Runner) -> Self {
        let s = load(ctx);
        let miss = if s.is_some() { Miss::Local } else { Miss::Live };
        Snap::new(r, miss, s.as_ref().map(|s| s.reads.clone()), s.map(|s| s.ts))
    }
    fn new(r: &'a dyn Runner, miss: Miss, reads: Option<BTreeMap<String, Option<String>>>, ts: Option<u64>) -> Self {
        Snap {
            r,
            miss,
            reads: Mutex::new(reads.unwrap_or_default()),
            ts,
        }
    }
    fn missed(&self, k: String, prog: &str, call: impl FnOnce() -> anyhow::Result<Out>) -> anyhow::Result<Out> {
        if let Some(v) = lock(&self.reads).get(&k) {
            return Ok(out(v.clone()));
        }
        if self.miss == Miss::Local && remote(prog) {
            return Ok(out(None));
        }
        let o = call()?;
        if self.miss == Miss::Collect {
            lock(&self.reads).insert(k, (o.code == 0).then(|| o.stdout.clone()));
        }
        Ok(o)
    }
}

impl Runner for Snap<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.missed(key(prog, args), prog, || self.r.run(prog, args))
    }
    fn sleep(&self, secs: u64) {
        self.r.sleep(secs);
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.run_for(prog, args, input, crate::runner::limit(prog, args))
    }
    fn run_for(&self, prog: &str, args: &[String], input: &str, limit: std::time::Duration) -> anyhow::Result<Out> {
        if input.is_empty() {
            return self.run(prog, args);
        }
        if !remote(prog) {
            return self.r.run_for(prog, args, input, limit);
        }
        let k = key_in(prog, args, input);
        self.missed(k, prog, || self.r.run_for(prog, args, input, limit))
    }
    fn shed(&self) -> Option<crate::shed::Shed> {
        self.r.shed()
    }
}

/// The board's lanes: every host's shed sessions joined with the log.
pub fn lanes(ctx: &Ctx, cfg: &Config, (events, st): (&[Event], &States), sessions: &Sessions) -> Vec<Lane> {
    lanes::join(
        cfg,
        crate::link::view::View::load(ctx, cfg).ok().as_ref(),
        events,
        st,
        sessions,
    )
}

/// What waits are worked out from: the log's length and last row, and each lane's state.
pub fn wait_key(events: &[Event], all: &[Lane]) -> String {
    let last = events.last().map_or(0, |e| e.seq);
    let lanes: Vec<_> = all.iter().map(|l| format!("{}={}", l.name, l.state.word())).collect();
    format!("{last} {}", lanes.join(" "))
}

/// The step's waits while its snapshot saw this log and these lanes; else worked out now.
pub fn saved_waits(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    snap: &Snapshot,
    (events, all): (&[Event], &[Lane]),
) -> Vec<Waiting> {
    if snap.key == wait_key(events, all) {
        return snap.waits.clone();
    }
    waits(ctx, cfg, r, events, all)
}

/// Each lane's wait reason, from the cached PR reads.
pub fn waits(ctx: &Ctx, cfg: &Config, r: &dyn Runner, events: &[Event], all: &[Lane]) -> Vec<Waiting> {
    let live: Vec<_> = all.iter().filter_map(|l| l.session.clone()).collect();
    let names: std::collections::HashSet<&str> = all.iter().map(|x| x.name.as_str()).collect();
    let shown = |l: &String| names.contains(l.as_str());
    let keep = |s: &IssueState| !s.done() || s.lane.iter().chain(s.lanes.values()).any(shown);
    let w = crate::waiting::scan_where(ctx, cfg, r, (events, &live), false, keep);
    w.into_iter().map(|x| x.1).collect()
}

/// The step's snapshot: the reads the board makes (lanes, waits, now-lines, Host rows) from the view and this host, saved, and returned for the step's own scan (a read it lacks runs live).
pub fn take<'a>(ctx: &Ctx, cfg: &Config, r: &'a dyn Runner) -> Snap<'a> {
    let s = Snap::new(r, Miss::Collect, None, Some(ctx.now));
    crate::timer::services(ctx, cfg, &s, Some(String::new()));
    crate::hosts::cards(cfg, &s);
    let (mut key, mut ws) = (String::new(), vec![]);
    let sessions = crate::link::view::View::load(ctx, cfg)
        .map(|v| v.sessions())
        .unwrap_or_default();
    if sessions.listed() {
        let events = crate::events::working(&ctx.state).unwrap_or_default();
        let all = lanes(ctx, cfg, (&events, &crate::events::fold(&events)), &sessions);
        (key, ws) = (wait_key(&events, &all), waits(ctx, cfg, &s, &events, &all));
    }
    let reads = lock(&s.reads).clone();
    let snap = Snapshot {
        ts: ctx.now,
        reads,
        waits: ws,
        key,
    };
    crate::db::put(&ctx.state, "board", ctx.now, &snap).ok();
    Snap { miss: Miss::Live, ..s }
}
