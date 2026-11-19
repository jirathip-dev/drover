use super::frame::{Body, Frame, Line, V};
use super::view::HostSeen;
use crate::config::Config;
use crate::events::{append, fold, last_spawn, working, Event, Json};
use crate::runner::Runner;
use crate::Ctx;
use anyhow::Result;
use serde_json::json;
use std::io::Write;

/// The lane a live spawn on the host at `m` made worktree `wt` for, and its `<fleet>:<issue>:<role>`.
pub fn lane_of(events: &[Event], m: Option<&str>, wt: &str) -> Option<(Event, String)> {
    let live = fold(events).into_values().filter(|s| !s.done());
    let spawns = live
        .flat_map(|s| s.lanes.into_values())
        .filter_map(|a| last_spawn(events, &a));
    let sp = spawns
        .filter(|sp| sp.str("machine") == m && sp.str("worktree") == Some(wt))
        .max_by_key(|sp| sp.seq)?;
    let lane = format!("{}:{}:{}", sp.fleet, sp.issue?, sp.str("role")?);
    Some((sp.clone(), lane))
}

/// Whether the log holds the `outbox` event of `lane`'s line sent at `sent`.
pub fn ran(events: &[Event], lane: &str, sent: u64) -> bool {
    let ran = |e: &Event| e.kind == "outbox" && e.str("lane") == Some(lane) && e.val("sent").as_u64() == Some(sent);
    events.iter().any(ran)
}

fn run(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (sp, lane): (&Event, &str),
    l: &Line,
) -> Result<()> {
    let events = working(&ctx.state)?;
    if ran(&events, lane, l.sent) {
        return Ok(());
    }
    let fleets: Vec<&str> = cfg.fleet.keys().map(String::as_str).collect();
    let why = match &l.lane {
        _ if l.args.is_empty() => Some("unreadable line".into()),
        named if named == lane => crate::cli::foreign(&l.args, lane, &events, &fleets),
        named => Some(format!("names lane {}, not {lane}", json!(named))),
    };
    let lag = (ctx.now.saturating_mul(1000)).saturating_sub(l.sent);
    let mut d = json!({"lane": lane, "sent": l.sent, "args": l.args, "host": sp.str("host"), "lag_ms": lag});
    if let Some(w) = &why {
        d.put("refused", json!(w));
        writeln!(out, "outbox {lane}: refused: {w}")?;
    } else {
        writeln!(out, "outbox {lane}: {}", l.args.first().map_or("", String::as_str))?;
        std::env::set_var("DROVER_LANE", lane);
        let res = crate::cli::run(&l.args.iter().map(String::as_str).collect::<Vec<_>>(), ctx, r, out);
        std::env::remove_var("DROVER_LANE");
        d.put("code", json!(res.as_ref().ok()))
            .put("error", json!(res.err().map(|e| format!("{e:#}"))));
    }
    append(&ctx.state, &Event::new(ctx.now, &sp.fleet, sp.issue, "outbox", d))
}

/// Host `host`'s frame applied, or why none came: the view's rows, then each outbox line not yet applied, run once as the lane its worktree's spawn names.
pub fn apply(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (host, m): (&str, Option<&str>),
    got: Result<Frame>,
) -> Result<()> {
    let old = (crate::db::view(&ctx.state)?.0.into_iter())
        .find(|h| h.host == host)
        .unwrap_or_default();
    let f = got.and_then(|f| {
        anyhow::ensure!(f.v == V, "it sends frames v{}, this build reads v{V}", f.v);
        Ok(f)
    });
    let f = match f {
        Ok(f) => f,
        Err(e) => {
            let why = format!("{e:#}").lines().next().unwrap_or_default().to_string();
            let h = HostSeen {
                host: host.into(),
                link: "down".into(),
                since: ctx.now,
                why: Some(why),
                ..old
            };
            return crate::db::view_put(&ctx.state, &h, None);
        }
    };
    if old.boot.as_deref() == Some(f.boot.as_str()) && old.n >= Some(f.n) {
        return Ok(());
    }
    let Body::Report(rep) = f.body;
    let key = |s: &super::frame::Seen| {
        (s.session.as_ref().map(|t| t.name.clone())).or_else(|| s.worktree.as_ref().map(|w| w.path.clone()))
    };
    let rows: Vec<(String, String)> = (rep.lanes.iter())
        .filter_map(|s| Some((key(s)?, serde_json::to_string(s).ok()?)))
        .collect();
    let h = HostSeen {
        host: host.into(),
        link: "down".into(),
        since: ctx.now,
        why: Some(rep.shed.unwrap_or_else(|| super::NO_SERVICE.into())),
        boot: Some(f.boot),
        n: Some(f.n),
        heard: Some(ctx.now),
        build: Some(rep.build),
    };
    crate::db::view_put(&ctx.state, &h, Some(&rows))?;
    let worktrees = rep.lanes.iter().filter_map(|s| s.worktree.as_ref());
    let mut events = None;
    for w in worktrees.filter(|w| !w.lines.is_empty()) {
        let ev = match &events {
            Some(ev) => ev,
            None => events.insert(working(&ctx.state)?),
        };
        let Some((sp, lane)) = lane_of(ev, m, &w.path) else {
            continue;
        };
        for l in &w.lines {
            run(ctx, cfg, r, out, (&sp, &lane), l)?;
        }
    }
    Ok(())
}
