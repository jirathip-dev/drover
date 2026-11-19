use super::{lock, mtime, refused, shown, BTreeMap, Config, Ctx, Refusal, Relaxed, Result, Sender, Server};
use crate::api::Job;
use crate::events::Event;
use serde_json::json;

pub(super) const OPS: [&str; 7] = ["drive", "override", "park", "unpark", "answer", "card", "config"];
const KEEP: u64 = 900;

pub(super) type Tap = (Ctx, BTreeMap<String, String>, String);

#[derive(Default)]
pub(super) struct Jobs(BTreeMap<String, (String, Job)>, u64);

impl Jobs {
    pub(super) fn busy(&self) -> bool {
        self.0.values().any(|j| j.1.state == "running")
    }

    fn add(&mut self, ctx: &Ctx, f: &BTreeMap<String, String>, (state, code, message): (&str, &str, String)) -> Job {
        let g = |k: &str| f.get(k).map_or("", String::as_str);
        self.1 += 1;
        let j = Job {
            id: format!("j{}-{}", ctx.now, self.1),
            op: g("op").into(),
            fleet: g("fleet").into(),
            issue: g("issue").parse().ok(),
            state: state.into(),
            code: code.into(),
            message,
            ts: ctx.now,
        };
        self.0.insert(j.id.clone(), (key(f), j.clone()));
        j
    }
}

fn key(f: &BTreeMap<String, String>) -> String {
    let card = ["card", "config"].contains(&f.get("op").map_or("", String::as_str));
    let keys: &[&str] = if card {
        &["op", "id"]
    } else {
        &["op", "fleet", "issue", "id"]
    };
    keys.iter()
        .map(|k| f.get(*k).map_or("", String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

fn ended(j: &Job) -> bool {
    !matches!(j.state.as_str(), "running" | "waiting")
}

pub(super) fn ready(ctx: &Ctx, f: &BTreeMap<String, String>) -> Result<()> {
    let g = |k: &str| f.get(k).map_or("", String::as_str);
    if !["card", "config"].contains(&g("op")) && g("mtime") != mtime(ctx) {
        return refused(
            "STALE",
            "fleet.toml changed since the page was rendered; reload and retry",
        );
    }
    Ok(())
}

fn host_wait(reason: &str) -> bool {
    reason.starts_with("HOST_BUSY") || reason == "MEM_LOW"
}

impl Server<'_> {
    pub(super) fn flying(&self, kv: &[(&str, &str)]) -> bool {
        let k = key(&kv.iter().map(|(a, b)| ((*a).into(), (*b).into())).collect());
        lock(&self.tapped).0.values().any(|(x, j)| *x == k && !ended(j))
    }

    pub(super) fn refusal(&self, kv: &[(&str, &str)]) -> Option<String> {
        let k = key(&kv.iter().map(|(a, b)| ((*a).into(), (*b).into())).collect());
        let jobs = lock(&self.tapped);
        let last = jobs.0.values().filter(|(x, _)| *x == k).max_by_key(|(_, j)| j.ts);
        last.filter(|(_, j)| j.state == "refused")
            .map(|(_, j)| j.message.clone())
    }

    pub(super) fn refused_tap(&self, ctx: &Ctx, f: &BTreeMap<String, String>, e: &anyhow::Error) {
        if let Some(r) = e.downcast_ref::<Refusal>() {
            let mut jobs = lock(&self.tapped);
            jobs.0.retain(|_, (_, j)| !ended(j) || ctx.now < j.ts + KEEP);
            jobs.add(ctx, f, ("refused", r.code, shown(e)));
        }
    }

    pub(super) fn tap(&self, ctx: &Ctx, f: &BTreeMap<String, String>) -> Option<Result<Job>> {
        let op = f.get("op").map_or("", String::as_str);
        let tx = self.jobs.as_ref().filter(|_| OPS.contains(&op))?;
        Some(self.accept(ctx, f, tx))
    }

    fn accept(&self, ctx: &Ctx, f: &BTreeMap<String, String>, tx: &Sender<Tap>) -> Result<Job> {
        let (k, g) = (key(f), |k: &str| f.get(k).map_or("", String::as_str));
        let job = {
            let mut jobs = lock(&self.tapped);
            jobs.0.retain(|_, (_, j)| !ended(j) || ctx.now < j.ts + KEEP);
            let again = jobs
                .0
                .values()
                .find(|(x, j)| *x == k && !["refused", "error"].contains(&j.state.as_str()));
            if let Some((_, j)) = again {
                return Ok(j.clone());
            }
            jobs.add(ctx, f, ("running", "ACCEPTED", format!("{} accepted", g("op"))))
        };
        if let Err(e) = ready(ctx, f) {
            lock(&self.tapped).0.remove(&job.id);
            return Err(e);
        }
        drop(tx.send((ctx.clone(), f.clone(), job.id.clone())));
        Ok(job)
    }

    pub(super) fn work(&self, ctx: &Ctx, f: &BTreeMap<String, String>) -> Result<String> {
        let g = |k: &str| f.get(k).map_or("", String::as_str);
        let (id, act) = (g("id"), g("act"));
        match g("op") {
            "card" => {
                let sha = Some(g("sha")).filter(|s| act == "approve" && !s.is_empty());
                let via = f.get("via").map(String::as_str);
                crate::card::settle_via(ctx, &Config::kept(ctx)?, &self.r, id, act == "approve", sha, via)
            }
            "config" => crate::propose::settle(ctx, &Config::kept(ctx)?, &self.r, id, act == "apply"),
            _ => self.write(ctx, f),
        }
    }

    fn set(&self, id: &str, state: &str, code: &str, message: String) -> Option<Job> {
        let mut jobs = lock(&self.tapped);
        let j = &mut jobs.0.get_mut(id)?.1;
        (j.state, j.code, j.message) = (state.into(), code.into(), message);
        Some(j.clone())
    }

    pub(super) fn job(&self, id: &str) -> Option<Job> {
        lock(&self.tapped).0.get(id).map(|j| j.1.clone())
    }

    fn logged(&self, j: &Job) {
        let d = json!({"id": j.id, "op": j.op, "state": j.state, "code": j.code, "message": j.message});
        let fleet = Some(j.fleet.as_str()).filter(|f| !f.is_empty()).unwrap_or("drover");
        drop(crate::events::append(
            &self.ctx.state,
            &Event::new(crate::now(), fleet, j.issue, "job", d),
        ));
    }

    pub(super) fn queued(&self, t: Tap) -> Option<String> {
        let g = |k: &str| t.1.get(k).map_or("", String::as_str);
        let k = if ["card", "config"].contains(&g("op")) {
            format!("{} {}", g("op"), g("id"))
        } else {
            format!("{} {}", g("fleet"), g("issue"))
        };
        let mut q = lock(&self.running);
        let first = !q.contains_key(&k);
        q.entry(k.clone()).or_default().push_back(t);
        first.then_some(k)
    }

    pub(super) fn drain(&self, k: &str) {
        loop {
            let t = {
                let mut q = lock(&self.running);
                let Some(t) = q.get_mut(k).and_then(std::collections::VecDeque::pop_front) else {
                    q.remove(k);
                    return;
                };
                t
            };
            self.run_job(t);
        }
    }

    pub(super) fn run_job(&self, (ctx, f, id): Tap) {
        if let Some(j) = self.job(&id) {
            self.logged(&j);
        }
        let ctx = Ctx {
            now: crate::now(),
            ..ctx
        };
        let (res, later) = crate::orchestrator::deferred(|| self.work(&ctx, &f));
        self.later(&ctx, later);
        self.deliver();
        let (state, code, msg) = match &res {
            Ok(m) => ("done", "OK".to_string(), m.clone()),
            Err(e) => match e.downcast_ref::<Refusal>() {
                Some(r) => ("refused", r.code.to_string(), shown(e)),
                None => ("error", "ERROR".into(), shown(e)),
            },
        };
        let every = Config::kept(&ctx).map_or(10, |c| c.limits.serve_host_secs).max(1);
        let (fleet, n) = (
            f.get("fleet").cloned().unwrap_or_default(),
            f.get("issue").and_then(|n| n.parse().ok()),
        );
        while let Some(p) = n.filter(|_| res.is_ok()).and_then(|n| {
            let ev = crate::events::working(&ctx.state).ok()?;
            crate::pending::of(&ev, &fleet, n).filter(|p| !p.failed && p.reason.as_deref().is_some_and(host_wait))
        }) {
            let line = p.line(crate::now()).unwrap_or_default();
            self.set(&id, "waiting", "HOST_WAIT", format!("{msg} · {line}"));
            self.kick.store(true, Relaxed);
            drover_core::clock::sleep(std::time::Duration::from_secs(every));
        }
        if let Some(j) = self.set(&id, state, &code, msg.clone()) {
            self.logged(&j);
        }
        self.kick.store(true, Relaxed);
        let (seen, msg) = (lock(&self.watch).seen.unwrap_or(0), json!(msg));
        lock(&self.clients).retain(|c| c.0.send(format!("id: {seen}\nevent: toast\ndata: {msg}\n\n")).is_ok());
    }
}
