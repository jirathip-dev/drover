use crate::runner::{Out, Runner};
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

pub(super) fn guard<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(super) struct Memo<'a>(
    pub(super) &'a dyn Runner,
    pub(super) &'a std::sync::Mutex<BTreeMap<String, Out>>,
);

impl Memo<'_> {
    fn once(&self, k: String, call: impl FnOnce() -> Result<Out>) -> Result<Out> {
        if let Some(o) = guard(self.1).get(&k) {
            return Ok(o.clone());
        }
        let o = call()?;
        guard(self.1).insert(k, o.clone());
        Ok(o)
    }
}

impl Runner for Memo<'_> {
    fn run(&self, prog: &str, args: &[String]) -> Result<Out> {
        self.once(crate::snapshot::key(prog, args), || self.0.run(prog, args))
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> Result<Out> {
        self.once(crate::snapshot::key_in(prog, args, input), || {
            self.0.run_in(prog, args, input)
        })
    }
    fn run_for(&self, prog: &str, args: &[String], input: &str, limit: std::time::Duration) -> Result<Out> {
        let k = crate::snapshot::key_in(prog, args, input);
        self.once(k, || self.0.run_for(prog, args, input, limit))
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
    fn shed(&self) -> Option<crate::shed::Shed> {
        self.0.shed()
    }
}

pub(super) struct Tally<'a>(
    pub(super) &'a dyn Runner,
    pub(super) std::sync::Mutex<BTreeMap<String, (u64, f64)>>,
);

impl Tally<'_> {
    fn time(&self, prog: &str, args: &[String], call: impl FnOnce() -> Result<Out>) -> Result<Out> {
        let Some(m) = crate::hosts::Pass::host(prog, args) else {
            return call();
        };
        let t = crate::clock::instant();
        let o = call();
        let mut by = guard(&self.1);
        let e = by.entry(m.into()).or_default();
        (e.0, e.1) = (e.0 + 1, e.1 + t.elapsed().as_secs_f64());
        o
    }
    pub(super) fn json(&self) -> Value {
        let r1 = |x: f64| (x * 10.0).round() / 10.0;
        let by = guard(&self.1);
        by.iter()
            .map(|(m, (n, secs))| (m.clone(), json!({"calls": n, "secs": r1(*secs)})))
            .collect::<serde_json::Map<_, _>>()
            .into()
    }
}

impl Runner for Tally<'_> {
    fn run(&self, prog: &str, args: &[String]) -> Result<Out> {
        self.time(prog, args, || self.0.run(prog, args))
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> Result<Out> {
        self.time(prog, args, || self.0.run_in(prog, args, input))
    }
    fn run_for(&self, prog: &str, args: &[String], input: &str, limit: std::time::Duration) -> Result<Out> {
        self.time(prog, args, || self.0.run_for(prog, args, input, limit))
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
    fn detach(&self, exe: &str, args: &[String], log: &Path) -> Result<u32> {
        self.0.detach(exe, args, log)
    }
    fn shed(&self) -> Option<crate::shed::Shed> {
        self.0.shed()
    }
}
