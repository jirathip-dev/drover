//! #680: a deterministic simulation of herdsman's concurrent passes (drive, drive --fast, check, kick and the owner's
//! verbs) running the real `main_with` against in-process fakes of herdr, gh and git and a fake clock, under one
//! seeded scheduler; the invariants are checked after every step (docs/design/sim-680).
//!
//! `HERDSMAN_SIM_SEED=<n> cargo test -p herdsman --test sim` replays one seed (`HERDSMAN_SIM_TRACE=full`: its whole
//! trace); `just sim [N]` runs N random seeds.
mod invariants;
mod model;
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
    let (mut m, cfg) = if seed == seeds::INCIDENT {
        seeds::incident()
    } else {
        scenario::random(seed)
    };
    m.sabotage = sabotage;
    scenario::setup(&root, &cfg);
    let sim = Sim::new(seed, scenario::T0, m, root);
    let end = scenario::T0 + HOURS * 3600;
    let mut failure = sim.run(end, STEPS);
    if failure.is_none() {
        // The settle: faults stop, the passes go on.
        let mut w = sim.lock();
        let t = w.now;
        w.model
            .agenda
            .retain(|_, e| !matches!(e, model::Ev::Load(_) | model::Ev::Crash(_)));
        w.model.load = 1;
        drop(w);
        failure = sim.run(t + 1800, STEPS + 1000);
    }
    sim.stop();
    let trace = std::mem::take(&mut sim.lock().trace);
    Outcome { seed, failure, trace }
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
