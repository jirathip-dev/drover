//! #686: a real stretch of one fleet's state.db as a scenario: when issues were queued (and blocked, unblocked,
//! removed), what each lane's agent did and how long it took (push, verdict, death, usage cap), each push's CI time and
//! colour, and the host's busy and offline windows. Today's herdsman then drives it in the sim; its invariants hold or
//! fail as for any seed, and its metrics are set beside the real ones. Only ids, timings and outcomes enter the
//! scenario: no issue text, verdict body, path or secret.
use crate::model::{Ev, Model, Script, FLEET};
use crate::scenario::{args, config, timers, T0};
use crate::sched::Rng;
use herdsman::events::Event;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// What a replay drives, times in seconds from the window's start (sim time `T0`).
#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct Scenario {
    pub fleet: String,
    /// The real window, unix seconds.
    pub from: u64,
    pub to: u64,
    /// The owner's queue verbs: when, and the words after `queue <fleet>`.
    pub queue: Vec<(u64, Vec<String>)>,
    /// Each `<role>#<issue>`'s lanes in spawn order: what its agent did (the last one again for any later lane).
    pub lanes: BTreeMap<String, Vec<Script>>,
    /// Windows the host refused spawns HOST_BUSY, and a host was offline (the sim has one host: both are busy).
    pub busy: Vec<(u64, u64)>,
    pub offline: Vec<(u64, u64)>,
    /// What really happened that week.
    pub real: Metrics,
}

/// One run's numbers (#685's, those the log carries), from its events alone: the real log's or the sim's.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct Metrics {
    /// Issues in the order they merged.
    pub merged: Vec<u64>,
    /// Hours from queued (or the window's start) to merged: p50 and p90.
    pub queue_to_merge_h: (f64, f64),
    pub spawns: usize,
    /// Spawns whose lane handed nothing on before the next lane of its role: no push, `turn-end done` or reviewer
    /// after it (impl), no verdict (rev).
    pub wasted: usize,
    pub supersedes: usize,
    pub deaths: usize,
    /// Distinct heads a reviewer was spawned on.
    pub reviewed_heads: usize,
    pub verdicts: usize,
    /// Verdicts on merged issues per merge.
    pub rounds_per_merge: f64,
    pub host_busy: usize,
}

/// `fleet`'s events of the last `days` before its newest one, read-only.
pub fn read(db: &Path, fleet: &str, days: u64) -> anyhow::Result<(Vec<Event>, u64, u64)> {
    let c = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let to: u64 = c.query_row("SELECT max(ts) FROM events WHERE fleet = ?1", [fleet], |r| r.get(0))?;
    let from = to.saturating_sub(days * 86_400);
    let mut q = c.prepare("SELECT json FROM events WHERE fleet = ?1 AND ts >= ?2 ORDER BY ts, seq")?;
    let rows = q.query_map(rusqlite::params![fleet, from], |r| r.get::<_, String>(0))?;
    let evs = rows
        .map(|j| Ok(serde_json::from_str(&j?)?))
        .collect::<anyhow::Result<Vec<Event>>>()?;
    let from = evs.first().map_or(from, |e| e.ts);
    Ok((evs, from, to))
}

fn issues(e: &Event) -> Vec<u64> {
    let v = e.data.get("issues").and_then(|v| v.as_array());
    v.into_iter().flatten().filter_map(serde_json::Value::as_u64).collect()
}

fn role(e: &Event) -> String {
    e.str("role")
        .map(String::from)
        .or_else(|| e.str("agent").and_then(crate::invariants::lane_key).map(|k| k.1))
        .unwrap_or_default()
}

/// Merge overlapping `(start, end)` windows.
fn merge(mut w: Vec<(u64, u64)>) -> Vec<(u64, u64)> {
    w.sort_unstable();
    let mut out: Vec<(u64, u64)> = vec![];
    for (a, b) in w {
        match out.last_mut() {
            Some(l) if a <= l.1 => l.1 = l.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// The scenario `evs` (one fleet's, ts-ordered, from `from` to `to`) describe.
pub fn build(evs: &[Event], fleet: &str, from: u64, to: u64) -> Scenario {
    let at = |e: &Event| e.ts.saturating_sub(from);
    let mut s = Scenario {
        fleet: fleet.into(),
        from,
        to,
        ..Scenario::default()
    };
    let mut queued = BTreeSet::new();
    for e in evs.iter().filter(|e| e.kind == "queue") {
        let ns = issues(e);
        let words: Vec<Vec<String>> = match e.str("op") {
            Some("add") => {
                queued.extend(&ns);
                let top = e.data.get("top").and_then(serde_json::Value::as_bool) == Some(true);
                let mut w = vec!["add".to_string()];
                w.extend(top.then(|| "--top".to_string()));
                w.extend(ns.iter().map(ToString::to_string));
                vec![w]
            }
            Some("block") => match e.data.get("on").and_then(serde_json::Value::as_u64) {
                Some(on) => (ns.iter())
                    .map(|n| args(&["block", "--on", &on.to_string(), &n.to_string()]))
                    .collect(),
                None => vec![], // a blocker in another fleet or repo: outside the sim
            },
            Some(op @ ("unblock" | "rm")) => ns.iter().map(|n| args(&[op, &n.to_string()])).collect(),
            _ => vec![],
        };
        s.queue.extend(words.into_iter().map(|w| (at(e), w)));
    }
    // An issue already in flight when the window opens is queued at its start.
    let mut late = vec![];
    for e in evs
        .iter()
        .filter(|e| ["spawn", "pushed", "verdict", "merged"].contains(&e.kind.as_str()))
    {
        if let Some(n) = e.issue.filter(|n| queued.insert(*n)) {
            late.push((0, args(&["add", &n.to_string()])));
        }
    }
    s.queue.splice(0..0, late);
    let merged: BTreeSet<u64> = evs
        .iter()
        .filter(|e| e.kind == "merged")
        .filter_map(|e| e.issue)
        .collect();
    for (i, sp) in evs.iter().enumerate().filter(|(_, e)| e.kind == "spawn") {
        let (Some(n), r, agent) = (sp.issue, role(sp), sp.str("agent").unwrap_or_default()) else {
            continue;
        };
        if r != "impl" && r != "rev" {
            continue;
        }
        let secs = |e: &Event| e.ts.saturating_sub(sp.ts).max(1);
        let mine = |e: &Event| e.str("agent") == Some(agent) || e.str("key").is_some_and(|k| k.ends_with(agent));
        // An impl lane's push shows as its `turn-end done` or `pushed`, else as the next drive step that needed a new
        // head (a review, a red CI, a merge): its CI then counts as ended there.
        let end = evs[i + 1..].iter().enumerate().find_map(|(j, e)| {
            let same = e.issue == Some(n);
            let step = e.str("step").unwrap_or_default();
            match e.kind.as_str() {
                "pushed" if same && r == "impl" => Some(push(&evs[i + 1 + j..], n, secs(e))),
                "turn-end" if same && r == "impl" && e.str("state") == Some("done") => {
                    Some(push(&evs[i + 1 + j..], n, secs(e)))
                }
                "driven" if same && r == "impl" && ["review", "ci-red", "merge"].contains(&step) => {
                    Some(Script::Push {
                        secs: secs(e),
                        green: step != "ci-red",
                        ci: Some(60),
                    })
                }
                "verdict" if same && r == "rev" => Some(Script::Review {
                    secs: secs(e),
                    pass: e.str("body").is_some_and(|b| b.trim_start().starts_with("PASS")),
                    proof: true,
                    heeds: true,
                }),
                "lane-dead" | "account-capped" if mine(e) => Some(Script::Die { secs: secs(e) }),
                "spawn" | "supersede" if same && mine(e) => Some(Script::Hang),
                _ => None,
            }
        });
        let lanes = s.lanes.entry(format!("{r}#{n}")).or_default();
        lanes.push(end.unwrap_or(Script::Hang));
    }
    for &n in &queued {
        let imp = s.lanes.entry(format!("impl#{n}")).or_default();
        if imp.is_empty() {
            imp.push(Script::Push {
                secs: 600,
                green: true,
                ci: None,
            });
        }
        let rev = s.lanes.entry(format!("rev#{n}")).or_default();
        if rev.is_empty() {
            rev.push(match merged.contains(&n) {
                true => Script::Review {
                    secs: 600,
                    pass: true,
                    proof: true,
                    heeds: true,
                },
                false => Script::Hang,
            });
        }
    }
    let busy: Vec<(u64, u64)> = (evs.iter())
        .filter(|e| e.kind == "driven" && e.str("code") == Some("HOST_BUSY"))
        .map(|e| (at(e), at(e) + 300))
        .collect();
    s.busy = merge(busy);
    let mut offline = vec![];
    for (i, e) in evs.iter().enumerate().filter(|(_, e)| e.kind == "host-offline") {
        let on = (evs[i..].iter()).find(|x| x.kind == "host-online" && x.str("host") == e.str("host"));
        offline.push((at(e), on.map_or(to - from, at)));
    }
    s.offline = merge(offline);
    s.real = metrics(evs, from);
    s
}

/// The lane's push of `n` (`evs[0]`): its CI's time and colour, as the next drive step that needed it saw them.
fn push(evs: &[Event], n: u64, secs: u64) -> Script {
    let ci = (evs[1..].iter())
        .take_while(|e| !(e.kind == "pushed" && e.issue == Some(n)))
        .find(|e| {
            e.kind == "driven"
                && e.issue == Some(n)
                && ["review", "ci-red", "merge"].contains(&e.str("step").unwrap_or(""))
        });
    Script::Push {
        secs,
        green: ci.is_none_or(|e| e.str("step") != Some("ci-red")),
        ci: ci.map(|e| (e.ts - evs[0].ts).clamp(60, 3600)),
    }
}

/// The run's metrics from its events (`start`: when an issue never queued in them counts as queued).
pub fn metrics(evs: &[Event], start: u64) -> Metrics {
    let mut m = Metrics::default();
    let (mut queued, mut waits) = (BTreeMap::new(), vec![]);
    let mut open: BTreeMap<(u64, String), bool> = BTreeMap::new();
    let mut verdicts: BTreeMap<u64, usize> = BTreeMap::new();
    let mut heads = BTreeSet::new();
    for e in evs {
        let n = e.issue.unwrap_or(0);
        match e.kind.as_str() {
            "queue" if e.str("op") == Some("add") => {
                for n in issues(e) {
                    queued.entry(n).or_insert(e.ts);
                }
            }
            "spawn" => {
                m.spawns += 1;
                let r = role(e);
                if r == "rev" {
                    open.insert((n, "impl".into()), true);
                    if let Some(sha) = e.str("sha") {
                        heads.insert((n, sha.to_string()));
                    }
                }
                if open.insert((n, r), false) == Some(false) {
                    m.wasted += 1;
                }
            }
            "pushed" => drop(open.insert((n, "impl".into()), true)),
            "turn-end" if e.str("state") == Some("done") => drop(open.insert((n, "impl".into()), true)),
            "verdict" => {
                m.verdicts += 1;
                *verdicts.entry(n).or_default() += 1;
                open.insert((n, "rev".into()), true);
            }
            "supersede" => m.supersedes += 1,
            "lane-dead" => m.deaths += 1,
            "driven" if e.str("code") == Some("HOST_BUSY") => m.host_busy += 1,
            "merged" if !m.merged.contains(&n) => {
                m.merged.push(n);
                let q = queued.get(&n).copied().unwrap_or(start).max(start);
                waits.push(e.ts.saturating_sub(q) as f64 / 3600.0);
            }
            _ => {}
        }
    }
    m.wasted += open.values().filter(|done| !**done).count();
    m.reviewed_heads = heads.len();
    waits.sort_by(f64::total_cmp);
    let pct = |p: f64| {
        waits
            .get(((waits.len() as f64 - 1.0) * p).round() as usize)
            .copied()
            .unwrap_or(0.0)
    };
    m.queue_to_merge_h = (pct(0.5), pct(0.9));
    let rounds: usize = m.merged.iter().map(|n| verdicts.get(n).copied().unwrap_or(0)).sum();
    m.rounds_per_merge = rounds as f64 / m.merged.len().max(1) as f64;
    m
}

/// The sim's world for `s`: its timers, the queue verbs at their times, each lane's script, the busy windows.
pub fn model(s: &Scenario) -> (Model, String) {
    let mut rng = Rng::new(s.from);
    let mut m = Model::default();
    let sha = m.sha();
    m.origin.insert("staging".into(), sha);
    timers(&mut m, &mut rng, T0 + 1);
    // The owner's verbs one after another, as typed: one 5 s after the last at the earliest.
    let mut last = 0;
    for (t, w) in &s.queue {
        let mut a = args(&["queue", FLEET]);
        a.extend(w.iter().cloned());
        last = (*t).max(last + 5);
        m.at(T0 + last, Ev::Run(a));
    }
    for (a, b) in merge([&s.busy[..], &s.offline].concat()) {
        m.at(T0 + a, Ev::Load(40));
        m.at(T0 + b, Ev::Load(1));
    }
    m.scripts = s.lanes.clone();
    (m, config(4, 6))
}

/// Replay `s` to its end; the run and the sim's metrics.
pub fn run(s: &Scenario) -> (crate::Outcome, Metrics) {
    let (m, cfg) = model(s);
    let o = crate::play(s.from, m, &cfg, T0 + (s.to - s.from), u64::MAX / 2, true);
    let sim = metrics(&o.events, T0);
    (o, sim)
}

/// The real-vs-sim table.
pub fn table(real: &Metrics, sim: &Metrics) -> String {
    let rows: [(&str, String, String); 11] = [
        ("merged", real.merged.len().to_string(), sim.merged.len().to_string()),
        (
            "queue→merge p50 h",
            format!("{:.1}", real.queue_to_merge_h.0),
            format!("{:.1}", sim.queue_to_merge_h.0),
        ),
        (
            "queue→merge p90 h",
            format!("{:.1}", real.queue_to_merge_h.1),
            format!("{:.1}", sim.queue_to_merge_h.1),
        ),
        ("spawns", real.spawns.to_string(), sim.spawns.to_string()),
        ("wasted spawns", real.wasted.to_string(), sim.wasted.to_string()),
        ("supersedes", real.supersedes.to_string(), sim.supersedes.to_string()),
        ("lane deaths", real.deaths.to_string(), sim.deaths.to_string()),
        (
            "reviewed heads",
            real.reviewed_heads.to_string(),
            sim.reviewed_heads.to_string(),
        ),
        ("verdicts", real.verdicts.to_string(), sim.verdicts.to_string()),
        (
            "rounds per merge",
            format!("{:.2}", real.rounds_per_merge),
            format!("{:.2}", sim.rounds_per_merge),
        ),
        (
            "HOST_BUSY refusals",
            real.host_busy.to_string(),
            sim.host_busy.to_string(),
        ),
    ];
    let same = (real.merged.iter().zip(&sim.merged))
        .take_while(|(a, b)| a == b)
        .count();
    let mut t = String::from("| metric | real | sim |\n|---|---:|---:|\n");
    for (k, a, b) in rows {
        t += &format!("| {k} | {a} | {b} |\n");
    }
    t + &format!("| same merge order (leading issues) | {same} | |\n")
}
