use super::episode::{Episode, Level};
use super::{decide, facts::Seen, table};
use crate::config::{Config, Engine, Fleet};
use crate::drive::Pass;
use crate::events::{age, append, fold, Event, IssueState};
use crate::runner::Runner;
use crate::waiting::Waiting;
use crate::Ctx;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Write;

/// `drover flow`: the flow table's shadow run.
#[derive(clap::Args)]
pub struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Subcommand)]
enum Cmd {
    #[command(
        about = "Its disagreements with today's drive by kind, row and today's step or code: count, first and last seen, the median lead or lag of its escalation, three example issues"
    )]
    Report {
        fleet: Option<String>,
        #[arg(help = "The window in days")]
        #[arg(long, default_value_t = 7)]
        days: u64,
    },
}

/// Run `drover flow`.
pub fn cli(ctx: &Ctx, a: Args, out: &mut dyn Write) -> Result<()> {
    let Cmd::Report { fleet, days } = a.cmd;
    report(ctx, fleet.as_deref(), days, out)
}

fn today(f: &Fleet, s: &IssueState, w: Option<&Waiting>) -> (&'static str, String, String) {
    let Some(w) = w else {
        let act = if s.state == "queued" { "start" } else { "wait" };
        return ("drive", act.into(), format!("no waiting line ({})", s.state));
    };
    let owner = match w.who {
        "drive" => "drive",
        "owner" => "owner",
        _ => "lane",
    };
    let cmd: Vec<&str> = w.next_cmd.as_deref().unwrap_or_default().split(' ').collect();
    let has = |x: &str| cmd.contains(&x);
    let check = cmd.get(4).is_some_and(|r| f.flow().is_check(r));
    let act = match (w.who, cmd.get(1).copied()) {
        ("drive", Some("verdict")) => "record",
        ("drive", Some("merge")) => "merge",
        ("drive", Some("done")) => "no-change",
        ("drive", Some("spawn")) if has("--update") => "update",
        ("drive", Some("spawn")) if has("--supersede") && check => "respawn-check",
        ("drive", Some("spawn")) if has("--supersede") => "respawn",
        ("drive", Some("spawn")) if check => "spawn-check",
        ("drive", Some("spawn")) if s.state == "queued" => "start",
        ("drive", Some("spawn")) => "spawn-build",
        ("drive", Some(v)) if !v.is_empty() => v,
        ("drive", _) => "nudge",
        _ => "wait",
    };
    (owner, act.into(), w.what.clone())
}

fn log(pass: &Pass, s: &IssueState, kind: &str, d: Value) -> Result<()> {
    let ctx = pass.ctx();
    let e = Event::new(ctx.now, &s.fleet, Some(s.issue), kind, d).passive();
    append(&ctx.state, &e)
}

fn once(events: &[Event], (s, ep): (&IssueState, &Episode), kind: &str, row: &str) -> bool {
    let mine = |e: &Event| e.is("flow-disagree", &s.fleet, s.issue) && e.str("row") == Some(row);
    let began = |e: &&Event| e.ts >= ep.began;
    !events
        .iter()
        .rev()
        .take_while(began)
        .any(|e| mine(e) && e.str("kind") == Some(kind))
}

/// After today's pass body, under its lease: the table decides each open issue of the pass (a kick's alone) on the same sample today's `waiting` reads, and logs its `row-*` (marked `shadow`) and each `flow-disagree`; nothing acts, asks or cards. `acts`: today's pass's actions.
pub fn run(pass: &Pass, cfg: &Config, r: &dyn Runner, acts: &[Value]) -> Result<()> {
    let (ctx, f) = (pass.ctx(), cfg.fleet(pass.fleet())?);
    if f.engine == Engine::Legacy {
        return Ok(());
    }
    let (t, seen, events) = (table()?, Seen::load(ctx, cfg)?, crate::events::working(&ctx.state)?);
    let agents = crate::link::view::View::load(ctx, cfg)?.sessions().checked()?.running();
    let waits = crate::waiting::scan(ctx, cfg, r, &events, &agents, false, Some(pass.fleet()));
    let act_secs = cfg.limits.act_minutes * 60;
    let open = fold(&events)
        .into_values()
        .filter(|s| s.fleet == pass.fleet() && !s.done());
    for s in open.filter(|s| pass.kick().is_none_or(|n| n == s.issue)) {
        let (c, clock, mut ep) = (seen.cond(&s).0, seen.clock(&s), Episode::of(&events, &s));
        let Some(step) = decide(t, &s, &c, &ep, &clock, &cfg.limits) else {
            continue;
        };
        let w = (waits.iter()).find(|(x, w)| x.issue == s.issue && x.lane.as_ref() == Some(&w.lane));
        let (who, did, what) = today(f, &s, w.map(|x| &x.1));
        let (due, cond) = (step.act.map_or("wait", |a| a.name()), c.to_string());
        if ep.entered.as_ref().is_none_or(|e| e.0 != step.row) {
            let d = json!({"row": step.row, "cond": cond, "deadline": step.deadline, "shadow": true});
            log(pass, &s, "row-entered", d)?;
            ep.entered = Some((step.row.clone(), ctx.now));
        }
        let code = (acts.iter())
            .filter(|a| a["issue"] == s.issue && did == due)
            .find_map(|a| a["code"].as_str());
        let class = code.map(|c| super::table::outcomes().map(|o| o.class(c))).transpose()?;
        let spent = match class {
            Some(c @ ("failed" | "final")) => Some((c, code.unwrap_or_default(), "today's drive")),
            _ if step.expired && ep.level == Level::Auto => Some(("failed", "TIMEOUT", "deadline passed")),
            _ => None,
        };
        if let Some((outcome, code, why)) = spent {
            let d = json!({"row": step.row, "try": step.tries, "action": due, "outcome": outcome, "code": code, "why": why, "shadow": true});
            log(pass, &s, "row-tried", d)?;
            let n = ep.tries.entry(step.row.clone()).or_default();
            *n = if outcome == "final" { 3 } else { n.saturating_add(1) };
            ep.last.insert(step.row.clone(), ctx.now);
        }
        let now = decide(t, &s, &c, &ep, &clock, &cfg.limits).unwrap_or_else(|| step.clone());
        if now.level > ep.level {
            let d = json!({"row": now.row, "level": now.level.name(), "shadow": true});
            log(pass, &s, "row-escalated", d)?;
            ep.escalated = Some((now.row.clone(), ctx.now));
            ep.level = now.level.clone();
        }
        let (owner, due) = (now.owner.name(), now.act.map_or("wait", |a| a.name()));
        if (owner != who || due != did) && once(&events, (&s, &ep), "action", &now.row) {
            let d = json!({"kind": "action", "row": now.row, "cond": cond,
                "table": {"action": due, "owner": owner, "try": now.tries, "deadline": now.deadline},
                "drive": {"step": did, "who": who, "what": what}});
            log(pass, &s, "flow-disagree", d)?;
        }
        let card =
            (events.iter().rev().take_while(|e| e.ts >= ep.began)).find(|e| e.is("needs-human", &s.fleet, s.issue));
        let (mine, theirs) = (ep.escalated.as_ref().map(|e| e.1), card.map(|e| e.ts));
        let late = |t: Option<u64>| t.is_some_and(|t| ctx.now >= t.saturating_add(act_secs));
        let apart = (mine.zip(theirs)).is_some_and(|(a, b)| a.abs_diff(b) > act_secs || ep.level.name() != "owner");
        let one_side = (mine.is_none() && late(theirs)) || (theirs.is_none() && late(mine));
        let row = ep.escalated.as_ref().map_or(&now.row, |e| &e.0);
        if (apart || one_side) && once(&events, (&s, &ep), "escalation", row) {
            let d = json!({"kind": "escalation", "row": row, "cond": cond,
                "table_at": mine, "table_level": ep.level.name(),
                "drive_at": theirs, "drive_code": card.and_then(|e| e.str("code"))});
            log(pass, &s, "flow-disagree", d)?;
        }
    }
    Ok(())
}

fn median(mut v: Vec<i64>) -> Option<i64> {
    v.sort_unstable();
    v.get(v.len() / 2).copied()
}

/// `drover flow report`: the last `days` of `flow-disagree`, grouped by kind, row and today's step or code.
pub fn report(ctx: &Ctx, fleet: Option<&str>, days: u64, out: &mut dyn Write) -> Result<()> {
    let events = crate::events::read(&ctx.state)?;
    let from = ctx.now.saturating_sub(days * 86400);
    let mut groups: BTreeMap<(String, String, String), Vec<&Event>> = BTreeMap::new();
    for e in events.iter().filter(|e| e.kind == "flow-disagree" && e.ts >= from) {
        if fleet.is_some_and(|f| f != e.fleet) {
            continue;
        }
        let today = match e.str("kind") {
            Some("action") => e.val("drive").get("step").and_then(Value::as_str),
            _ => e.str("drive_code"),
        };
        let name = |x: Option<&str>| x.unwrap_or("none").to_string();
        let key = (name(e.str("kind")), name(e.str("row")), name(today));
        groups.entry(key).or_default().push(e);
    }
    if groups.is_empty() {
        writeln!(out, "no flow-disagree in the last {days} days")?;
    }
    for ((kind, row, today), es) in &groups {
        let at = |k: &str, e: &Event| e.val(k).as_i64();
        let lead = median(
            es.iter()
                .filter_map(|e| Some(at("table_at", e)? - at("drive_at", e)?))
                .collect(),
        );
        let lead = lead.map_or(String::new(), |l| {
            let side = if l <= 0 { "lead" } else { "lag" };
            format!(" · median {side} {}", age(l.unsigned_abs()))
        });
        let first = es.first().map_or(0, |e| e.ts);
        let last = es.last().map_or(0, |e| e.ts);
        let mut eg: Vec<String> = es
            .iter()
            .map(|e| format!("{} #{}", e.fleet, e.issue.unwrap_or(0)))
            .collect();
        eg.dedup();
        eg.truncate(3);
        let (first, last) = (age(ctx.now.saturating_sub(first)), age(ctx.now.saturating_sub(last)));
        writeln!(
            out,
            "{kind} {row} · today {today} · {} · first {first} ago · last {last} ago{lead} · e.g. {}",
            es.len(),
            eg.join(", ")
        )?;
    }
    Ok(())
}
