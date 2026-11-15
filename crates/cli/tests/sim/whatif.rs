use crate::model::{Ev, Model, FLEET};
use crate::scenario::T0;
use crate::{hunt, metrics, replay};
use drover::config::{Config, Serial};
use std::collections::BTreeMap;

/// The fleet.toml knobs a what-if carries into the sim's fleet, read (never written) from one config.
#[derive(Debug, Clone)]
pub struct Knobs {
    pub rounds: u32,
    pub spawns: usize,
    pub max_impl: Option<usize>,
    pub max_rev: Option<usize>,
    pub serial: Vec<Serial>,
    pub hosts: u64,
}

impl Knobs {
    /// `fleet`'s knobs in fleet.toml `text`; `hosts` counts the enabled `[hosts.*]` (none: the one main host).
    pub fn read(text: &str, fleet: &str) -> anyhow::Result<Knobs> {
        let c = Config::parse(text)?;
        let f = c.fleet(fleet)?;
        Ok(Knobs {
            rounds: c.limits.review_rounds,
            spawns: c.limits.max_parallel_spawns,
            max_impl: f.max_impl,
            max_rev: f.max_rev,
            serial: f.serial.clone(),
            hosts: c.hosts.values().filter(|h| h.enabled).count().max(1) as u64,
        })
    }

    /// The sim fleet's fleet.toml with these knobs.
    pub fn config(&self) -> String {
        let caps = [("max_impl", self.max_impl), ("max_rev", self.max_rev)];
        let caps: String = caps
            .iter()
            .filter_map(|(k, v)| v.map(|v| format!("{k} = {v}\n")))
            .collect();
        let at = "integration = \"staging\"\n";
        let mut cfg =
            crate::scenario::config(self.spawns as u64, self.rounds.into()).replacen(at, &format!("{at}{caps}"), 1);
        for g in &self.serial {
            cfg += &format!(
                "\n[[fleet.{FLEET}.serial]]\nname = {:?}\npaths = {:?}\nmax = {}\n",
                g.name, g.paths, g.max
            );
        }
        cfg
    }

    fn rows(&self) -> Vec<(String, String)> {
        let opt = |v: Option<usize>| v.map_or("-".into(), |v| v.to_string());
        let mut rows = vec![
            ("review_rounds".into(), self.rounds.to_string()),
            ("max_parallel_spawns".into(), self.spawns.to_string()),
            ("max_impl".into(), opt(self.max_impl)),
            ("max_rev".into(), opt(self.max_rev)),
            ("hosts".into(), self.hosts.to_string()),
        ];
        rows.extend(
            self.serial
                .iter()
                .map(|g| (format!("serial {} max", g.name), g.max.to_string())),
        );
        rows
    }
}

/// What both sides run: N seeds' random fleets, or one replayed window.
pub enum Source {
    Seeds(Vec<u64>),
    Replay(replay::Scenario),
}

/// One side's metrics (#685's, `waits` and `merges`) and every invariant failure.
#[derive(Debug, Default)]
pub struct Side {
    pub metrics: BTreeMap<String, f64>,
    pub failures: Vec<String>,
}

/// The model's load windows: where its agenda's `Load` events keep the host busy.
pub fn windows(m: &Model) -> Vec<(u64, u64, u64)> {
    let (mut out, mut since) = (vec![], None);
    for (&(t, _), e) in &m.agenda {
        match (since, e) {
            (None, Ev::Load(l)) if *l > 1 => since = Some(t),
            (Some(a), Ev::Load(1)) => {
                out.push((a, t, 1));
                since = None;
            }
            _ => {}
        }
    }
    out.extend(since.map(|a| (a, u64::MAX / 2, 1)));
    out
}

/// Re-place `m`'s load: each window (from, to, hosts it takes) is busy; spawns are refused only while they take all `hosts`.
pub fn place(m: &mut Model, ws: &[(u64, u64, u64)], hosts: u64) {
    m.agenda.retain(|_, e| !matches!(e, Ev::Load(_)));
    let mut edges: Vec<(u64, i64)> = (ws.iter())
        .flat_map(|&(a, b, n)| [(a, n.cast_signed()), (b, -n.cast_signed())])
        .collect();
    edges.sort_unstable();
    let (mut busy, mut on) = (0, false);
    for (t, d) in edges {
        busy += d;
        if (busy >= hosts.cast_signed()) != on {
            on = !on;
            m.at(t, Ev::Load(if on { 40 } else { 1 }));
        }
    }
}

fn with_waits(mut m: BTreeMap<String, f64>, merges: u64) -> BTreeMap<String, f64> {
    let waits: f64 = ["serial_busy", "host_busy", "caps"]
        .iter()
        .filter_map(|k| m.get(*k))
        .sum();
    m.insert("waits".into(), (waits * 100.0).round() / 100.0);
    m.insert("merges".into(), merges as f64);
    m
}

/// Run `src` under knobs `k`; `real_hosts` is how many hosts a replayed `HOST_BUSY` window had busy (the current config's).
pub fn side(src: &Source, k: &Knobs, real_hosts: u64) -> Side {
    let cfg = k.config();
    match src {
        Source::Seeds(seeds) => {
            let mut runs = vec![];
            for chunk in seeds.chunks(16) {
                let hs: Vec<_> = (chunk.iter())
                    .map(|&seed| {
                        let (cfg, hosts) = (cfg.clone(), k.hosts);
                        std::thread::spawn(move || {
                            let p = hunt::Plan::of(seed);
                            let mut m = p.model();
                            let ws = windows(&m);
                            place(&mut m, &ws, hosts);
                            crate::run_world(seed, (m, cfg), p.steps, None)
                        })
                    })
                    .collect();
                runs.extend(hs.into_iter().map(|h| h.join().expect("a run")));
            }
            let ms: Vec<_> = runs.iter().map(|o| o.metrics.clone()).collect();
            let s = metrics::summary(&ms).into_iter().map(|(n, v)| (n.to_string(), v));
            Side {
                metrics: with_waits(s.collect(), ms.iter().map(|m| m.merges).sum()),
                failures: (runs.iter())
                    .filter_map(|o| Some(format!("seed {}: {}", o.seed, o.failure.as_ref()?)))
                    .collect(),
            }
        }
        Source::Replay(s) => {
            let (mut m, _) = replay::model(s);
            let busy = s.busy.iter().map(|&(a, b)| (T0 + a, T0 + b, real_hosts));
            let ws: Vec<_> = busy
                .chain(s.offline.iter().map(|&(a, b)| (T0 + a, T0 + b, 1)))
                .collect();
            place(&mut m, &ws, k.hosts);
            let (o, run) = replay::run_in(s, m, &cfg);
            Side {
                metrics: with_waits(run.metrics, run.merged.len() as u64),
                failures: o.violations.into_iter().chain(o.failure).collect(),
            }
        }
    }
}

/// Both sides of a what-if, run at once.
pub struct WhatIf {
    pub knobs: (Knobs, Knobs),
    pub current: Side,
    pub candidate: Side,
}

pub fn run(src: Source, current: Knobs, candidate: Knobs) -> WhatIf {
    let src = std::sync::Arc::new(src);
    let real = current.hosts;
    let (s2, k2) = (src.clone(), candidate.clone());
    let cand = std::thread::spawn(move || side(&s2, &k2, real));
    let cur = side(&src, &current, real);
    WhatIf {
        knobs: (current, candidate),
        current: cur,
        candidate: cand.join().expect("the candidate's run"),
    }
}

impl WhatIf {
    /// Candidate minus current for metric `k`.
    pub fn delta(&self, k: &str) -> f64 {
        let v = |s: &Side| s.metrics.get(k).copied().unwrap_or(0.0);
        ((v(&self.candidate) - v(&self.current)) * 100.0).round() / 100.0
    }

    /// The knobs side by side, then each metric's current, candidate and delta, then the invariant failures.
    pub fn table(&self) -> String {
        let mut t = String::from("| knob | current | candidate |\n|---|---:|---:|\n");
        let (a, b) = (self.knobs.0.rows(), self.knobs.1.rows());
        let mut names: Vec<&String> = a.iter().map(|r| &r.0).collect();
        names.extend(b.iter().map(|r| &r.0).filter(|n| !a.iter().any(|r| &r.0 == *n)));
        let get = |rows: &[(String, String)], n: &str| {
            let r = rows.iter().find(|r| r.0 == n);
            r.map_or("-".into(), |r| r.1.clone())
        };
        for n in names {
            let (x, y) = (get(&a, n), get(&b, n));
            let mark = if x == y { "" } else { " **" };
            t += &format!("| {n}{mark} | {x} | {y} |\n");
        }
        t += "\n| metric | current | candidate | delta |\n|---|---:|---:|---:|\n";
        for (k, v) in &self.current.metrics {
            let c = self.candidate.metrics.get(k).copied().unwrap_or(0.0);
            t += &format!("| {k} | {v} | {c} | {:+} |\n", self.delta(k));
        }
        let (fa, fb) = (self.current.failures.len(), self.candidate.failures.len());
        t += &format!(
            "| invariant failures | {fa} | {fb} | {:+} |\n",
            i64::try_from(fb).unwrap() - i64::try_from(fa).unwrap()
        );
        for (side, s) in [("current", &self.current), ("candidate", &self.candidate)] {
            for f in s.failures.iter().take(5) {
                t += &format!("\n{side}: {f}");
            }
            if s.failures.len() > 5 {
                t += &format!("\n{side}: … {} more", s.failures.len() - 5);
            }
        }
        t
    }
}
