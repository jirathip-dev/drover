//! #680: a deterministic simulation of herdsman's concurrent passes (drive, drive --fast, check, kick and the owner's
//! verbs) running the real `main_with` against in-process fakes of herdr, gh and git and a fake clock, under one
//! seeded scheduler; the invariants are checked after every step (docs/design/sim-680).
//!
//! `HERDSMAN_SIM_SEED=<n> cargo test -p herdsman --test sim` replays one seed (`HERDSMAN_SIM_TRACE=full`: its whole
//! trace); `just sim [N]` runs N random seeds.
mod invariants;
mod model;
mod replay;
mod scenario;
mod sched;
mod seeds;

#[path = "../common/mod.rs"]
#[allow(unused_imports)]
mod common;

use invariants::Sabotage;
use sched::Sim;

/// One run: 6 sim hours or 4,000 steps, then a 30-minute settle with no new faults (I6).
const HOURS: u64 = 6;
const STEPS: u64 = 4000;

pub struct Outcome {
    pub seed: u64,
    pub failure: Option<String>,
    pub trace: Vec<String>,
    /// #686: every event the run appended, in order, and (a replay's `keep_going`) each violation with its step.
    pub events: Vec<herdsman::events::Event>,
    pub violations: Vec<String>,
}

impl Outcome {
    /// The failure with its seed, the replay line and the trace's last 200 lines plus every scenario event (all of
    /// it with `HERDSMAN_SIM_TRACE=full`).
    pub fn report(&self) -> String {
        let full = std::env::var("HERDSMAN_SIM_TRACE").is_ok_and(|v| v == "full");
        let tail = self.trace.len().saturating_sub(200);
        let lines: Vec<&str> = (self.trace.iter().enumerate())
            .filter(|(i, l)| full || *i >= tail || l.contains(" event "))
            .map(|(_, l)| l.as_str())
            .collect();
        format!(
            "{}\nFAIL seed={} {}\nreplay: HERDSMAN_SIM_SEED={} cargo test -p herdsman --test sim\n",
            lines.join("\n"),
            self.seed,
            self.failure.as_deref().unwrap_or("(passed)"),
            self.seed
        )
    }
}

/// Run `seed` (seed 1 is the #370 incident, any other a random fleet), `sabotage` breaking one invariant on purpose.
pub fn run(seed: u64, sabotage: Option<Sabotage>) -> Outcome {
    let (mut m, cfg) = if seed == seeds::INCIDENT {
        seeds::incident()
    } else {
        scenario::random(seed)
    };
    m.sabotage = sabotage;
    play(seed, m, &cfg, scenario::T0 + HOURS * 3600, STEPS, false)
}

/// Drive `m` from `T0` to `end` (or `steps`), then a 30-minute settle with no new faults (I6); `keep_going` records
/// each violation and runs on.
pub fn play(seed: u64, m: model::Model, cfg: &str, end: u64, steps: u64, keep_going: bool) -> Outcome {
    // A lane's own environment must not reach the simulated processes (they share this one).
    for k in [
        "HERDSMAN_LANE",
        "HERDSMAN_OUTBOX",
        "HERDSMAN_SERVICE",
        "HERDSMAN_TOKEN",
        "HERDSMAN_HOST",
    ] {
        std::env::remove_var(k);
    }
    let root = common::tmpdir(&format!("sim-{seed}"));
    scenario::setup(&root, cfg);
    let sim = Sim::new(seed, scenario::T0, m, root);
    sim.lock().keep_going = keep_going;
    let mut failure = sim.run(end, steps);
    if failure.is_none() {
        // The settle: faults stop, the passes go on.
        let mut w = sim.lock();
        let t = w.now;
        w.model
            .agenda
            .retain(|_, e| !matches!(e, model::Ev::Load(_) | model::Ev::Crash(_)));
        w.model.load = 1;
        drop(w);
        failure = sim.run(t + 1800, steps + 1000);
    }
    sim.stop();
    let mut w = sim.lock();
    let trace = std::mem::take(&mut w.trace);
    let events = w.wrote.drain(..).map(|x| x.ev).collect();
    Outcome {
        seed,
        failure,
        trace,
        events,
        violations: std::mem::take(&mut w.violations),
    }
}

/// The seeds this run checks: `HERDSMAN_SIM_SEED` alone, else the fixed set.
fn seeds() -> Vec<u64> {
    match std::env::var("HERDSMAN_SIM_SEED").ok().and_then(|s| s.parse().ok()) {
        Some(s) => vec![s],
        None => seeds::FIXED.iter().map(|s| s.0).collect(),
    }
}

/// Run `seeds` side by side (each run is its own world); the failures, each report printed.
fn check(seeds: &[u64]) -> Vec<String> {
    let runs: Vec<_> = seeds
        .iter()
        .map(|&s| std::thread::spawn(move || run(s, None)))
        .collect();
    let mut bad = vec![];
    for o in runs.into_iter().map(|h| h.join().expect("a run")) {
        if let Some(f) = &o.failure {
            eprintln!("{}", o.report());
            bad.push(format!("seed {}: {f}", o.seed));
        }
    }
    bad
}

#[test]
fn fixed_seeds_hold_every_invariant() {
    let bad = check(&seeds());
    assert!(bad.is_empty(), "{bad:#?}");
}

/// Seeds that found a cause of their own (owner, q835): each fails until its issue's fix lands with it unparked.
#[test]
#[ignore = "parked: each seed's cause is its own issue"]
fn parked_seeds() {
    let parked: Vec<u64> = seeds::PARKED.iter().map(|s| s.0).collect();
    let bad = check(&parked);
    assert!(bad.is_empty(), "{bad:#?}");
}

/// AC 2: seed 1 is the #370 incident; it reaches a verdict with one supersede, not a loop.
#[test]
fn the_incident_seed_records_the_verdict_with_one_supersede() {
    let o = run(seeds::INCIDENT, None);
    assert!(o.failure.is_none(), "{}", o.report());
    let sup = o.trace.iter().filter(|l| l.contains(" append supersede #370 ")).count();
    let told = o
        .trace
        .iter()
        .filter(|l| l.contains(" append proof-refused #370 "))
        .count();
    assert!(
        sup <= 3 && told >= 2,
        "supersedes {sup}, refusals {told}\n{}",
        o.report()
    );
    assert!(
        o.trace.iter().any(|l| l.contains(" append verdict #370 ")),
        "{}",
        o.report()
    );
}

/// AC 4: the same seed gives the same trace twice.
#[test]
fn a_seed_replays_to_the_same_trace() {
    let (a, b) = (run(7, None), run(7, None));
    assert!(a.trace.len() > 500, "{}", a.trace.len());
    let diff = a.trace.iter().zip(&b.trace).position(|(x, y)| x != y);
    assert!(
        diff.is_none() && a.trace.len() == b.trace.len(),
        "first difference at {diff:?}: {:?} vs {:?}",
        diff.map(|i| &a.trace[i]),
        diff.map(|i| &b.trace[i])
    );
}

/// AC 3: a deliberately broken invariant fails with its seed and trace.
#[test]
fn a_broken_invariant_fails_with_seed_and_trace() {
    for (s, code) in [(Sabotage::DoubleLane, "I1 "), (Sabotage::RedMerge, "I4 ")] {
        let o = (2..40)
            .map(|seed| run(seed, Some(s)))
            .find(|o| o.failure.is_some())
            .expect("a seed that trips it");
        let f = o.failure.clone().unwrap_or_default();
        assert!(f.starts_with(code), "{s:?}: {f}");
        let r = o.report();
        assert!(
            r.contains(&format!("FAIL seed={} {code}", o.seed)) && r.contains(&format!("HERDSMAN_SIM_SEED={}", o.seed)),
            "{r}"
        );
        assert!(r.lines().count() > 50, "{r}");
    }
}

/// `just sim [N]`: N random seeds from `HERDSMAN_SIM_FROM` (default 1000); every failing seed printed.
#[test]
#[ignore = "long: `just sim`"]
fn random_seeds() {
    let n: u64 = std::env::var("HERDSMAN_SIM_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);
    let from: u64 = std::env::var("HERDSMAN_SIM_FROM")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);
    let mut bad = vec![];
    let all: Vec<u64> = (from..from + n).collect();
    for chunk in all.chunks(16) {
        bad.extend(check(chunk));
    }
    assert!(bad.is_empty(), "{} of {n} seeds failed:\n{}", bad.len(), bad.join("\n"));
}

/// #686 AC 2: a small db (one issue straight through, one FAIL then fixed, one blocked on it, one whose lane dies)
/// replays to the same merge order and counts.
#[test]
fn a_fixture_db_replays_to_its_merge_order_and_counts() {
    let dir = common::tmpdir("replay-fixture");
    let log = dir.join("events.jsonl");
    let t = 1_790_000_000;
    let ev = |s: u64, n: u64, kind: &str, data: serde_json::Value| {
        herdsman::events::Event::new(t + s, "fx", Some(n), kind, data)
    };
    let spawn = |s, n, role: &str, agent: &str| {
        ev(
            s,
            n,
            "spawn",
            serde_json::json!({"role": role, "agent": agent, "sha": format!("{n}-{s}")}),
        )
    };
    let pass = serde_json::json!({"body": "PASS\n\nall good"});
    let fail = serde_json::json!({"body": "FAIL\n\n- blocking [correctness] x"});
    let driven = |s, n, step: &str| ev(s, n, "driven", serde_json::json!({"step": step, "code": null}));
    let pushed = |s, n| ev(s, n, "pushed", serde_json::json!({"sha": "a".repeat(40)}));
    let queue = |s, n| {
        herdsman::events::Event::new(
            t + s,
            "fx",
            None,
            "queue",
            serde_json::json!({"op": "add", "issues": [n]}),
        )
    };
    let evs = [
        queue(0, 11),
        queue(30, 12),
        spawn(60, 11, "impl", "fx-impl-11"),
        spawn(90, 12, "impl", "fx-impl-12"),
        pushed(1260, 11),
        driven(1500, 11, "review"),
        spawn(1500, 11, "rev", "fx-rev-11-r1"),
        ev(
            1800,
            12,
            "turn-end",
            serde_json::json!({"role": "impl", "state": "done"}),
        ),
        driven(2100, 12, "review"),
        spawn(2100, 12, "rev", "fx-rev-12-r1"),
        ev(2400, 11, "verdict", pass.clone()),
        driven(2400, 11, "merge"),
        ev(2400, 11, "merged", serde_json::json!({})),
        ev(3000, 12, "verdict", fail),
        spawn(3100, 12, "impl", "fx-impl-12"),
        queue(3600, 13),
        herdsman::events::Event::new(
            t + 3601,
            "fx",
            None,
            "queue",
            serde_json::json!({"op": "block", "issues": [13], "on": 12}),
        ),
        queue(3700, 14),
        spawn(3800, 14, "impl", "fx-impl-14"),
        ev(4400, 14, "lane-dead", serde_json::json!({"key": "1:fx-impl-14"})),
        pushed(5000, 12),
        driven(5300, 12, "review"),
        spawn(5300, 12, "rev", "fx-rev-12-r2"),
        ev(5900, 12, "verdict", pass.clone()),
        ev(5900, 12, "merged", serde_json::json!({})),
        spawn(6000, 13, "impl", "fx-impl-13"),
        pushed(6600, 13),
        driven(6900, 13, "review"),
        spawn(6900, 13, "rev", "fx-rev-13-r1"),
        ev(7500, 13, "verdict", pass),
        ev(7500, 13, "merged", serde_json::json!({})),
        ev(4 * 3600, 0, "drive-pass", serde_json::json!({})),
    ];
    for e in &evs {
        herdsman::events::append(&log, e).unwrap();
    }
    let (evs, from, to) = replay::read(&dir.join("state.db"), "fx", 1).unwrap();
    let s = replay::build(&evs, "fx", from, to);
    assert_eq!(s.real.merged, [11, 12, 13]);
    let (o, sim) = replay::run(&s);
    eprintln!("{}", replay::table(&s.real, &sim));
    assert!(
        o.failure.is_none() && o.violations.is_empty(),
        "{:?}\n{}",
        o.violations,
        o.report()
    );
    assert_eq!(sim.merged, s.real.merged, "{}", o.report());
    assert_eq!(
        (sim.reviewed_heads, sim.verdicts, sim.deaths, sim.rounds_per_merge),
        (
            s.real.reviewed_heads,
            s.real.verdicts,
            s.real.deaths,
            s.real.rounds_per_merge
        ),
        "{}",
        o.report()
    );
    // The scenario carries no issue text or verdict body: only ids, timings and outcomes.
    let saved = serde_json::to_string(&s).unwrap();
    assert!(!saved.contains("all good") && !saved.contains("blocking"), "{saved}");
}

/// #686: `just sim --replay <state.db> [--days 7] [--fleet herdsman] [--save scenario.json]`: replay a real window of
/// the db (or a saved scenario), print the invariant result and the real-vs-sim table.
#[test]
#[ignore = "long: `just sim --replay`"]
fn replay_db() {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let src = var("HERDSMAN_SIM_REPLAY").expect("HERDSMAN_SIM_REPLAY");
    let s: replay::Scenario = if src.ends_with(".json") {
        serde_json::from_str(&std::fs::read_to_string(&src).unwrap()).unwrap()
    } else {
        let fleet = var("HERDSMAN_SIM_FLEET").unwrap_or_else(|| "herdsman".into());
        let days = var("HERDSMAN_SIM_DAYS").and_then(|d| d.parse().ok()).unwrap_or(7);
        let (evs, from, to) = replay::read(std::path::Path::new(&src), &fleet, days).unwrap();
        replay::build(&evs, &fleet, from, to)
    };
    if let Some(p) = var("HERDSMAN_SIM_SAVE") {
        std::fs::write(&p, serde_json::to_string_pretty(&s).unwrap()).unwrap();
        println!("scenario saved to {p}");
    }
    let started = std::time::Instant::now();
    let (o, sim) = replay::run(&s);
    let lanes: usize = s.lanes.values().map(Vec::len).sum();
    println!(
        "replay {} {}..{} ({} queue verbs, {} lanes, {} busy and {} offline windows): {} sim steps in {:.0?}",
        s.fleet,
        s.from,
        s.to,
        s.queue.len(),
        lanes,
        s.busy.len(),
        s.offline.len(),
        o.trace.len(),
        started.elapsed()
    );
    let end = o
        .trace
        .last()
        .map(|l| l.split_whitespace().nth(1).unwrap_or_default().to_string());
    println!(
        "ran to {} (window {} s + 1800 s settle)",
        end.unwrap_or_default(),
        s.to - s.from
    );
    match &o.violations[..] {
        [] => println!("invariants: all held (I1–I10)"),
        v => println!(
            "invariants: {} violation(s), each at its step:\n{}",
            v.len(),
            v.join("\n")
        ),
    }
    println!("{}", replay::table(&s.real, &sim));
    assert!(o.failure.is_none(), "{:?}", o.failure);
}
