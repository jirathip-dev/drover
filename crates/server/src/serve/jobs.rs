use super::{lock, mtime, refused, shown, BTreeMap, Config, Ctx, Refusal, Relaxed, Result, Sender, Server};
use crate::api::Job;
use crate::events::Event;
use serde_json::json;

/// #977: the taps that run as jobs, each on its own thread (`Server::tap`).
pub(super) const OPS: [&str; 8] = [
    "drive", "override", "extend", "park", "unpark", "answer", "card", "config",
];
const KEEP: u64 = 900;

/// #977: a job to run: (ctx, form, job id).
pub(super) type Tap = (Ctx, BTreeMap<String, String>, String);

/// #977: the board's jobs by id, each with its button's key (op, fleet, issue, id); the last id number.
#[derive(Default)]
pub(super) struct Jobs(BTreeMap<String, (String, Job)>, u64);

impl Jobs {
    /// #977: a job is still running (a handover waits for it; one waiting for host does not hold it).
    pub(super) fn busy(&self) -> bool {
        self.0.values().any(|j| j.1.state == "running")
    }
}

fn key(f: &BTreeMap<String, String>) -> String {
    ["op", "fleet", "issue", "id"]
        .map(|k| f.get(k).map_or("", String::as_str))
        .join(" ")
}

fn ended(j: &Job) -> bool {
    !matches!(j.state.as_str(), "running" | "waiting")
}

fn extended(ctx: &Ctx, f: &BTreeMap<String, String>) -> Result<()> {
    let g = |k: &str| f.get(k).map_or("", String::as_str);
    if g("op") != "extend" {
        return Ok(());
    }
    let n = g("issue").parse().ok().ok_or_else(|| anyhow::anyhow!("no issue"))?;
    let state = crate::gate::held(ctx, g("fleet"), n)?;
    crate::gate::extended(ctx, (g("fleet"), n), (true, "owner"), &state)
}

/// #977: a tap's page is current (`STALE` unless its `mtime` is fleet.toml's; a card or config tap has none).
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
    /// #977: a job op's tap, accepted at once (a tap of the same button returns its pending or done job); None: no job op.
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
            jobs.1 += 1;
            let j = Job {
                id: format!("j{}-{}", ctx.now, jobs.1),
                op: g("op").into(),
                fleet: g("fleet").into(),
                issue: g("issue").parse().ok(),
                state: "running".into(),
                code: "ACCEPTED".into(),
                message: format!("{} accepted", g("op")),
                ts: ctx.now,
            };
            jobs.0.insert(j.id.clone(), (k, j.clone()));
            j
        };
        if let Err(e) = ready(ctx, f).and_then(|()| extended(ctx, f)) {
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

    /// #977: the job `id`, as it stands.
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

    /// #977: run job `id` on its own thread: its op, then, while its issue's start waits for host, `waiting`; toasted.
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
            std::thread::sleep(std::time::Duration::from_secs(every));
        }
        if let Some(j) = self.set(&id, state, &code, msg.clone()) {
            self.logged(&j);
        }
        self.kick.store(true, Relaxed);
        let (seen, msg) = (lock(&self.watch).seen.unwrap_or(0), json!(msg));
        lock(&self.clients).retain(|c| c.0.send(format!("id: {seen}\nevent: toast\ndata: {msg}\n\n")).is_ok());
    }
}
