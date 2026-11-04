//! #265: the JSON API v1 under `/api/v1/` (docs/serve.md), beside the HTML board: `state`, `events?since=N`,
//! `lane/<agent>` (`crate::api::Chat`) and a POST per form op. Its types are in `crate::api`, beside the board's. A
//! breaking change is `/v2`.
use super::{board, moment, BTreeMap, Event, JSON, PAIRED_OPS};
use super::{fold, form, lanes, lock, read, refused, shown, view, Config, Ctx, Refusal, Result, Server};
use crate::api::{Ask, Ev, Events, Fleet, Lane, Queued, Reply, Row, State};
use serde::Serialize;

type Res = (&'static str, &'static str, Vec<u8>);

fn out(status: &'static str, v: &impl Serialize) -> Res {
    (status, JSON, serde_json::to_vec(v).unwrap_or_default())
}

fn reply(status: &'static str, code: &str, message: impl Into<String>) -> Res {
    let (ok, code, message) = (code == "OK", code.into(), message.into());
    out(status, &Reply { ok, code, message })
}

impl Server<'_> {
    /// `/api/v1/<p>`: Host/Origin-checked like `/live`; `lane/` and the paired ops for a paired device only.
    pub(super) fn api(&self, ctx: &Ctx, head: &str, method: &str, p: &str, query: &str, body: &str) -> Res {
        let (f, paired) = (form(query), self.device(ctx, head).is_some());
        let lane = p.strip_prefix("/api/v1/lane/");
        match (method, p.strip_prefix("/api/v1/")) {
            ("POST", _) if self.addr.is_none() => reply("405 Method Not Allowed", "READ_ONLY", "GET only"),
            _ if !self.allowed(ctx, head) => reply("403 Forbidden", "FORBIDDEN", "foreign Host or Origin"),
            ("GET", Some("state")) => self.state(ctx).map_or_else(
                |e| reply("500 Internal Server Error", "ERROR", shown(&e)),
                |s| out("200 OK", &s),
            ),
            ("GET", Some("events")) => {
                let ev = read(&ctx.state).unwrap_or_default();
                let since = f.get("since").and_then(|n| n.parse().ok()).unwrap_or(0);
                let after: Vec<_> = ev.iter().filter(|e| e.seq > since).collect();
                let (seq, refresh) = (ev.last().map_or(0, |e| e.seq), after.len() > 500);
                let list = after.into_iter().filter(|_| !refresh).map(ev_of).collect();
                out("200 OK", &Events { seq, refresh, list })
            }
            ("GET", _) if lane.is_some() && !paired => reply("401 Unauthorized", "UNPAIRED", "pair this device first"),
            ("GET", _) if lane.is_some() => {
                let q = form(&format!("{query}&agent={}", lane.unwrap_or_default())); // the path's agent wins
                let t = self.transcript(ctx, &q).filter(|_| !q.contains_key("setup"));
                t.map_or_else(
                    || reply("404 Not Found", "NOT_FOUND", "no such agent pane"),
                    |t| ("200 OK", JSON, t.into_bytes()),
                )
            }
            ("POST", Some(op)) if PAIRED_OPS.contains(&op) && !paired => {
                reply("401 Unauthorized", "UNPAIRED", "pair this device first")
            }
            ("POST", Some(op)) => {
                let f = form(body.trim())
                    .into_iter()
                    .chain([("op".into(), op.into())])
                    .collect();
                self.op(ctx, f).map_or_else(
                    |e| match e.downcast_ref::<Refusal>() {
                        Some(r) => reply("409 Conflict", r.code, r.msg.clone()),
                        None => reply("400 Bad Request", "ERROR", format!("{e:#}")),
                    },
                    |m| reply("200 OK", "OK", m),
                )
            }
            _ => reply("404 Not Found", "NOT_FOUND", format!("no {method} {p} in API v1")),
        }
    }

    /// #140: a form op that can spawn or merge goes to `serve_on`'s job thread (its result toasts on `/live`).
    pub(super) fn op(&self, ctx: &Ctx, f: BTreeMap<String, String>) -> Result<String> {
        let [op, fleet] = ["op", "fleet"].map(|k| f.get(k).cloned().unwrap_or_default());
        let Some(tx) = self
            .jobs
            .as_ref()
            .filter(|_| ["drive", "override", "extend", "park"].contains(&op.as_str()))
        else {
            return self.write(ctx, &f);
        };
        if !lock(&self.running).insert(fleet.clone()) {
            return refused("DRIVE_RUNNING", format!("{fleet} has a {op} running"));
        }
        drop(tx.send((ctx.clone(), f, fleet.clone())));
        Ok(format!("{op} started for {fleet}"))
    }

    fn state(&self, ctx: &Ctx) -> Result<State> {
        let (cfg, events) = (Config::load(&ctx.config)?, read(&ctx.state)?);
        let (st, r) = (fold(&events), &self.cached(ctx.now, 60));
        let all = self.lanes(ctx.now, &cfg, &events, &st).unwrap_or_default();
        let waits = self.waits(ctx, &cfg, &events, &all);
        let lane = |l: &lanes::Lane| {
            let (title, tone, shown, _) = view(ctx, l, &waits);
            Lane {
                name: l.name.clone(),
                fleet: l.fleet.clone(),
                issue: l.issue,
                title,
                role: l.role.clone(),
                harness: l.harness.clone(),
                model: l.model.clone(),
                state: l.state.clone(),
                shown,
                tone: tone.into(),
                since: l.since,
            }
        };
        let queued = |(f, i): (&String, crate::queue::Item)| Queued {
            fleet: f.clone(),
            issue: i.issue,
            title: crate::gh::cached_title(ctx, f, i.issue),
            state: st
                .get(&(f.clone(), i.issue))
                .map_or("queued".into(), |s| s.state.clone()),
            blocked_by: i.blocked_by,
        };
        let ask = |a: &Event| Ask {
            id: a.str("id").unwrap_or_default().into(),
            fleet: a.fleet.clone(),
            issue: a.issue,
            question: a.str("question").unwrap_or_default().into(),
            options: serde_json::from_value(a.data["options"].clone()).unwrap_or_default(),
            recommend: a.data["recommend"].as_u64(),
            ts: a.ts,
        };
        let seen = crate::hosts::cached(ctx);
        let host = |(name, h): (String, Option<&crate::config::HostCfg>)| {
            let n = crate::hosts::lanes_on(&cfg, &events, &name).len();
            let on = |s| crate::hosts::shown(h.is_none_or(|h| h.enabled), s, n, ctx.now);
            let (t, detail) = seen.get(&name).map_or(("g", "this host".into()), on);
            Row {
                name,
                tone: t.into(),
                detail,
                ..Row::default()
            }
        };
        let row = |(name, t, detail, fix): crate::timer::Row| Row {
            name,
            tone: t.into(),
            detail,
            fix,
        };
        let (t, d) = crate::timer::phone(ctx, Some(&cfg), &events);
        let fleet = |(f, fc): (&String, &crate::config::Fleet)| Fleet {
            name: f.clone(),
            color: fc.color(f).into(),
            paused: fc.paused,
        };
        let snap = self.snapshot().1.ts;
        let queue = cfg
            .fleet
            .keys()
            .flat_map(|f| crate::queue::view(&events, &st, f).into_iter().map(move |i| (f, i)));
        Ok(State {
            seq: events.last().map_or(0, |e| e.seq),
            age: (snap > 0).then(|| ctx.now.saturating_sub(snap)),
            mtime: super::mtime(ctx),
            theme: super::active(ctx),
            fleets: cfg.fleet.iter().map(fleet).collect(),
            queue: queue.map(queued).collect(),
            lanes: all.iter().map(lane).collect(),
            asks: crate::events::open_asks(&events).into_iter().map(ask).collect(),
            hosts: crate::hosts::cards(&cfg, r).into_iter().map(host).collect(),
            services: crate::timer::services(ctx, &cfg, r, Some(board(ctx)))
                .into_iter()
                .map(row)
                .collect(),
            alerts: vec![row(("push".into(), t, d, String::new()))],
        })
    }
}

fn ev_of(e: &Event) -> Ev {
    Ev {
        seq: e.seq,
        ts: e.ts,
        fleet: e.fleet.clone(),
        issue: e.issue,
        kind: e.kind.clone(),
        data: e.data.clone(),
        k: moment(e).map(String::from),
    }
}
