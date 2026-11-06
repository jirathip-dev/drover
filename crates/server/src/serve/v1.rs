//! #265: the JSON API v1 under `/api/v1/` (docs/serve.md), the board's data: `state`, `events?since=N`, `host`,
//! `queue`, #269 `needs`, `lanes`, `chat` and `thread/<t>`, `lane/<agent>` (`crate::api::Chat`) and a POST per form
//! op. Its types are in `crate::api`, beside the board's. A breaking change is `/v2`. #555: the drover plugins' routes
//! (`herd`, `roles`, `setup/<pane>`; POST `send`, `menu`, `act`, `setup`, `chat-about`, `chat`, `close`) run the old
//! board's code; every POST takes JSON (or the board's form) and answers `Reply`.
use super::{act, board, busy, coat, gh, moment, words, Auth, BTreeMap, Event, JSON, PAIRED_OPS};
use super::{fold, form, lanes, lock, refused, shared, shown, view, Config, Ctx, Refusal, Result, Server};
use crate::api::{Ask, ChatDefaults, Ev, Events, Fleet, Harness, Lane, Queued, Reply, Roles, Row, SetupScreen, State};
use crate::events::plain;
use anyhow::bail;
use serde::Serialize;
use serde_json::Value;

/// #555: the paired-only POSTs beside `PAIRED_OPS`: the old board's `/send`, `/answer` (`menu`), `/act`, `/setup` and
/// a backlog row's [chat about] (`/thread`'s `new`), then `[chat]`'s defaults and a backlog issue's [close].
const LANE_OPS: &[&str] = &["send", "menu", "act", "setup", "chat-about", "chat", "close"];

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
    pub(super) fn api(
        &self,
        ctx: &Ctx,
        head: &str,
        (method, p): (&str, &str),
        query: &str,
        body: &str,
        auth: &Auth,
    ) -> Res {
        let f = form(query);
        let (paired, lane) = (auth.is_ok(), p.strip_prefix("/api/v1/lane/"));
        let setup = p.strip_prefix("/api/v1/setup/").filter(|_| method == "GET");
        let paired_op = |op: &str| PAIRED_OPS.contains(&op) || LANE_OPS.contains(&op);
        match (method, p.strip_prefix("/api/v1/")) {
            ("POST", _) if self.addr.is_none() => reply("405 Method Not Allowed", "READ_ONLY", "GET only"),
            _ if !self.allowed(ctx, head) => reply("403 Forbidden", "FORBIDDEN", "foreign Host or Origin"),
            // #501: the devices unread: no unpaired view or 401, the page keeps its last draw and retries.
            (_, Some(x))
                if busy(auth)
                    && (lane.or(setup).is_some() || ["needs", "lanes", "chat"].contains(&x) || paired_op(x)) =>
            {
                reply("503 Service Unavailable", "BUSY", "board busy, retrying")
            }
            ("GET", Some("state")) => self.state(ctx).map_or_else(
                |e| reply("500 Internal Server Error", "ERROR", shown(&e)),
                |s| out("200 OK", &s),
            ),
            ("GET", Some("host")) => out("200 OK", &self.host_tab(ctx)),
            ("GET", Some("queue")) => out("200 OK", &self.queue_tab(ctx)),
            // #269: the needs cards, Lanes and [chat], as this device sees them; a thread's older messages.
            ("GET", Some("needs")) => out("200 OK", &self.needs_tab(ctx, paired)),
            ("GET", Some("lanes")) => out("200 OK", &self.lanes_tab(ctx, paired)),
            ("GET", Some("chat")) => out("200 OK", &self.chat_tab(ctx, paired && self.addr.is_some())),
            ("GET", Some(t)) if t.starts_with("thread/") => {
                let ev = shared(&ctx.state).unwrap_or_default();
                super::chat::older(ctx, &ev, &t[7..], &f).map_or_else(
                    || reply("404 Not Found", "NOT_FOUND", "no such thread"),
                    |p| out("200 OK", &p),
                )
            }
            ("GET", Some("herd")) => self.herd(ctx, None).map_or_else(
                |e| reply("500 Internal Server Error", "ERROR", shown(&e)),
                |h| out("200 OK", &h),
            ),
            ("GET", Some("roles")) => roles(ctx, f.get("fleet").map_or("", String::as_str)).map_or_else(
                |e| reply("404 Not Found", "NOT_FOUND", format!("{e:#}")),
                |r| out("200 OK", &r),
            ),
            ("GET", Some("events")) => {
                let n = |k: &str| f.get(k).and_then(|n| n.parse().ok());
                out(
                    "200 OK",
                    &events(ctx, n("since").unwrap_or(0), n("until").unwrap_or(u64::MAX), 500),
                )
            }
            ("GET", _) if lane.or(setup).is_some() && !paired => {
                reply("401 Unauthorized", "UNPAIRED", "pair this device first")
            }
            // #555: a setup pane's screen (the old board's `/lane?setup=`).
            ("GET", _) if setup.is_some() => crate::setup::screen(ctx, self.r, setup.unwrap_or_default()).map_or_else(
                |e| reply("404 Not Found", "NOT_FOUND", format!("{e:#}")),
                |j| out("200 OK", &serde_json::from_value::<SetupScreen>(j).unwrap_or_default()),
            ),
            ("GET", _) if lane.is_some() => {
                let q = form(&format!("{query}&agent={}", lane.unwrap_or_default())); // the path's agent wins
                let t = self.transcript(ctx, &q).filter(|_| !q.contains_key("setup"));
                t.map_or_else(
                    || reply("404 Not Found", "NOT_FOUND", "no such agent pane"),
                    |t| ("200 OK", JSON, t.into_bytes()),
                )
            }
            ("POST", Some(op)) if paired_op(op) && !paired => {
                reply("401 Unauthorized", "UNPAIRED", "pair this device first")
            }
            // #555: the old board's own path (its rate limit and refusals), `Reply`'s code from its line.
            ("POST", Some(op @ ("send" | "menu" | "act" | "setup" | "chat-about"))) => {
                let mut f = fields(body);
                if op == "chat-about" {
                    f = about(ctx, &f);
                }
                let path = ["/send", "/answer", "/act", "/setup", "/thread"]
                    [LANE_OPS.iter().position(|o| *o == op).unwrap_or(0)];
                let (status, msg) = self.send(ctx, head, &f, path, &auth.clone().into());
                let refusal = msg.strip_prefix("REFUSED ").and_then(|m| m.split_once(": "));
                let code = match &status[..3] {
                    "200" => "OK",
                    "429" => "RATE_LIMIT",
                    _ => refusal.map_or("ERROR", |r| r.0),
                };
                let status = if code == "ERROR" { "400 Bad Request" } else { status };
                reply(status, code, refusal.map_or(msg.as_str(), |r| r.1))
            }
            ("POST", Some("chat")) => done(self.chat_defaults(ctx, &fields(body))),
            ("POST", Some("close")) => done(self.close(ctx, &fields(body))),
            ("POST", Some(op)) => {
                let f = fields(body).into_iter().chain([("op".into(), op.into())]).collect();
                done(self.op(ctx, f))
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

    /// #555: `[chat]`'s defaults from the fleet's [config] sheet (`mtime` as the form ops send it).
    fn chat_defaults(&self, ctx: &Ctx, f: &BTreeMap<String, String>) -> Result<String> {
        if f.get("mtime").map(String::as_str) != Some(&super::mtime(ctx)) {
            return refused(
                "STALE",
                "fleet.toml changed since the page was rendered; reload and retry",
            );
        }
        let wanted: Vec<_> = ["harness", "model", "effort"]
            .into_iter()
            .filter_map(|k| Some((k, f.get(k).filter(|v| !v.is_empty())?.clone())))
            .collect();
        if wanted.is_empty() {
            return refused("BAD_VALUE", "set harness, model or effort");
        }
        let line = crate::conductor::chat(ctx, self.r, &wanted)?;
        self.kick.store(true, super::Relaxed);
        Ok(line)
    }

    /// #555: a backlog row's [close]: `gh issue close` on an issue neither queued nor worked on, then the backlog read
    /// again (it drops out); logs `closed`.
    fn close(&self, ctx: &Ctx, f: &BTreeMap<String, String>) -> Result<String> {
        let (cfg, events) = (Config::load(&ctx.config)?, shared(&ctx.state)?);
        let fleet = f.get("fleet").map_or("", String::as_str);
        let Some(fc) = cfg.fleet.get(fleet) else {
            return refused("BAD_VALUE", format!("no fleet `{fleet}`"));
        };
        let Some(n) = f.get("issue").and_then(|n| n.parse::<u64>().ok()) else {
            return refused("BAD_VALUE", "no issue number");
        };
        let st = fold(&events);
        if st.contains_key(&(fleet.to_string(), n))
            || crate::queue::view(&events, &st, fleet).iter().any(|i| i.issue == n)
        {
            return refused(
                "NOT_BACKLOG",
                format!("#{n} is queued or has a lane: only a backlog issue closes here"),
            );
        }
        let o = self.r.run(
            "gh",
            &crate::runner::argv(&["issue", "close", &n.to_string(), "-R", &fc.repo]),
        )?;
        if o.code != 0 {
            bail!("gh issue close #{n}: {}", o.stderr.trim());
        }
        let d = serde_json::json!({"why": "backlog", "via": "web"});
        crate::events::append(&ctx.state, &Event::new(ctx.now, fleet, Some(n), "closed", d))?;
        let _ = gh::backlog(ctx, self.r, fc, fleet);
        Ok(format!("closed #{n}"))
    }

    pub(super) fn state(&self, ctx: &Ctx) -> Result<State> {
        let (cfg, events) = (Config::load(&ctx.config)?, shared(&ctx.state)?);
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
            repo: fc.repo.clone(),
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
            now: ctx.now,
            tz: crate::usage::date(self.r, ctx.now, "+%z")
                .parse::<i64>()
                .map_or(0, |z| z / 100 * 3600 + z % 100 * 60),
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

/// #555: a POST body: a JSON object (a list joined with `,`, `true` as `1`), else the board's form.
fn fields(body: &str) -> BTreeMap<String, String> {
    fn flat(v: Value) -> String {
        match v {
            Value::String(s) => s,
            Value::Bool(b) => if b { "1" } else { "" }.into(),
            Value::Null => String::new(),
            Value::Array(a) => a.into_iter().map(flat).collect::<Vec<_>>().join(","),
            v => v.to_string(),
        }
    }
    serde_json::from_str::<serde_json::Map<String, Value>>(body.trim()).map_or_else(
        |_| form(body.trim()),
        |m| m.into_iter().map(|(k, v)| (k, flat(v))).collect(),
    )
}

/// A form op's result as `Reply`: OK, a refusal's code (409), else ERROR (400).
fn done(r: Result<String>) -> Res {
    r.map_or_else(
        |e| match e.downcast_ref::<Refusal>() {
            Some(r) => reply("409 Conflict", r.code, r.msg.clone()),
            None => reply("400 Bad Request", "ERROR", format!("{e:#}")),
        },
        |m| reply("200 OK", "OK", m),
    )
}

/// #555: [chat about] backlog issue `issue` of `fleet`: a new thread in its repo whose first message names it.
fn about(ctx: &Ctx, f: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let (fleet, n) = (
        f.get("fleet").cloned().unwrap_or_default(),
        f.get("issue").cloned().unwrap_or_default(),
    );
    let title = n.parse().ok().and_then(|n| gh::cached_title(ctx, &fleet, n));
    let text = format!(
        "Let's talk about #{n}{}",
        title.map_or(String::new(), |t| format!(": {t}"))
    );
    BTreeMap::from([
        ("op".into(), "new".into()),
        ("repo".into(), fleet),
        ("text".into(), text),
    ])
}

/// #555: `roles?fleet=`: the fleet's [config] sheet and `[chat]`'s defaults.
fn roles(ctx: &Ctx, fleet: &str) -> Result<Roles> {
    let (cfg, events) = (Config::load(&ctx.config)?, shared(&ctx.state)?);
    let fc = cfg.fleet(fleet)?;
    let c = &cfg.chat;
    let harness = |h: &&str| Harness {
        name: (*h).into(),
        efforts: crate::harness::efforts(h).iter().map(ToString::to_string).collect(),
    };
    Ok(Roles {
        fleet: fleet.into(),
        config: super::config_sheet(ctx, &cfg, fleet, fc, &fold(&events)),
        chat: ChatDefaults {
            harness: c.harness.clone(),
            model: c.model.clone(),
            effort: c.effort.clone(),
            harnesses: crate::harness::NAMES.iter().map(harness).collect(),
        },
    })
}

/// An open ask (`needs-decision`) as `state` lists it; #268: a Queue row's [change] form.
pub(super) fn ask(a: &Event) -> Ask {
    Ask {
        id: a.str("id").unwrap_or_default().into(),
        fleet: a.fleet.clone(),
        issue: a.issue,
        question: a.str("question").unwrap_or_default().into(),
        options: serde_json::from_value(a.data["options"].clone()).unwrap_or_default(),
        recommend: a.data["recommend"].as_u64(),
        ts: a.ts,
    }
}

/// #259: the events with `since < seq <= until`; over `cap`, none and `refresh`. #266: each as the Events tab says it.
pub(super) fn events(ctx: &Ctx, since: u64, until: u64, cap: usize) -> Events {
    let (ev, cfg) = (shared(&ctx.state).unwrap_or_default(), Config::load(&ctx.config).ok());
    let after: Vec<_> = ev.iter().filter(|e| e.seq > since && e.seq <= until).collect();
    let (seq, refresh) = (ev.last().map_or(0, |e| e.seq), after.len() > cap);
    let mut prs = BTreeMap::new();
    let list = (after.into_iter().filter(|_| !refresh))
        .map(|e| {
            let fc = cfg.as_ref().and_then(|c| c.fleet.get(&e.fleet));
            let (glyph, tone, text, imp) = words(e, fc.map_or("serial", |f| f.serial_label()));
            let pr = e
                .data
                .get("pr")
                .and_then(|p| p.as_u64().or_else(|| p.as_str()?.parse().ok()));
            let (pr, run) = e.issue.filter(|_| fc.is_some()).map_or((None, None), |n| {
                let pr = pr.or_else(|| {
                    *prs.entry(n)
                        .or_insert_with(|| gh::cached_pr(ctx, &e.fleet, n)["number"].as_u64())
                });
                (pr, pr.is_none().then(|| gh::cached_run(ctx, &e.fleet, n)).flatten())
            });
            let act = match (e.kind.as_str(), tone) {
                ("spawn", _) if e.data.get("check") == Some(&serde_json::json!(true)) => "rev",
                ("spawn", _) => act(e.str("role").unwrap_or_default(), "working"),
                (_, "r") => "blocked",
                (_, "g") => "done",
                _ => "idle",
            };
            Ev {
                seq: e.seq,
                ts: e.ts,
                fleet: e.fleet.clone(),
                issue: e.issue,
                kind: e.kind.clone(),
                data: e.data.clone(),
                k: moment(e).map(String::from),
                glyph: glyph.into(),
                tone: tone.into(),
                text,
                imp,
                act: act.into(),
                coat: e.str("agent").filter(|a| !a.is_empty()).map_or(String::new(), coat),
                plain: plain(e),
                pr,
                run,
            }
        })
        .collect();
    Events { seq, refresh, list }
}
