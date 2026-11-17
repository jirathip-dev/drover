use crate::config::Config;
use crate::events::{age, names, Event};
use crate::Ctx;
use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::io::Write;

type Rule = fn(&Event) -> bool;

/// #962: what counts as an owner action: the event kind, its label, the rule as `--explain` prints it, the rule.
pub const OWNER: [(&str, &str, &str, Rule); 11] = [
    ("decision", "ask answered", "`by` is the owner's", owned),
    ("needs-human", "needs-human", "every one", |_| true),
    ("run-settled", "run card approved", "`approved` is true", |e| {
        e.val("approved") == &Value::Bool(true)
    }),
    ("spawn", "manual spawn", "`by` is the owner's", owned),
    ("verdict", "manual verdict", "`by` is the owner's", owned),
    (
        "lane-killed",
        "manual kill",
        "`by` is the owner's, or none (before #962)",
        |e| e.data.get("orchestrator").is_none() && (e.data.get("by").is_none() || owned(e)),
    ),
    ("round-extended", "extra round", "`by` is the owner's", owned),
    ("parked", "parked", "`by` is the owner's", owned),
    ("unparked", "unparked", "`by` is the owner's", owned),
    ("override", "override", "`by` is the owner's", owned),
    ("merged", "owner-override merge", "`by` is the owner's", owned),
];

fn owned(e: &Event) -> bool {
    let auto = |b: &str| b.starts_with("orchestrator-") || matches!(b, "default" | "drive" | "lane" | "external");
    e.data.get("orchestrator").is_none() && e.str("by").is_some_and(|b| !auto(b))
}

fn owner(e: &Event) -> Option<&'static str> {
    OWNER.iter().find(|o| o.0 == e.kind && (o.3)(e)).map(|o| o.1)
}

fn week(ts: u64) -> u64 {
    (ts / 86_400 + 3) / 7
}

#[derive(Default)]
struct Acc {
    merged: u32,
    autonomous: u32,
    actions: BTreeMap<&'static str, u32>,
    asks: BTreeMap<&'static str, u32>,
    rounds: Vec<u64>,
    pushbacks: u32,
    respawns: u32,
    restarts: u32,
    q2m: Vec<u64>,
    wait: u64,
    closed: u32,
    parked: u32,
}

/// #962: one week's (or the window's) autonomy numbers.
#[derive(Serialize)]
pub struct Row {
    pub week: String,
    pub merged: u32,
    pub autonomous: u32,
    pub autonomous_pct: u32,
    pub owner_actions: BTreeMap<&'static str, u32>,
    pub owner_actions_per_merge: f64,
    pub asks: BTreeMap<&'static str, u32>,
    pub rounds_median: u64,
    pub round3: u32,
    pub pushbacks: u32,
    pub respawns: u32,
    pub restarts: u32,
    pub queue_to_merge_median_secs: u64,
    pub queue_to_merge_p90_secs: u64,
    pub owner_wait_secs: u64,
    pub owner_wait_pct: u32,
    pub closed_unmerged: u32,
    pub parked: u32,
}

fn pct(a: u64, b: u64) -> u32 {
    u32::try_from(a * 100 / b.max(1)).unwrap_or(u32::MAX)
}

impl Acc {
    fn row(mut self, week: String) -> Row {
        self.rounds.sort_unstable();
        self.q2m.sort_unstable();
        let at = |v: &[u64], p: usize| v.get(v.len() * p / 100).copied().unwrap_or_default();
        let n: u32 = self.actions.values().sum();
        let total: u64 = self.q2m.iter().sum();
        Row {
            week,
            merged: self.merged,
            autonomous: self.autonomous,
            autonomous_pct: pct(self.autonomous.into(), self.merged.into()),
            owner_actions_per_merge: (f64::from(n) * 100.0 / f64::from(self.merged.max(1))).round() / 100.0,
            owner_actions: self.actions,
            asks: self.asks,
            rounds_median: at(&self.rounds, 50),
            round3: (self.rounds.iter().filter(|r| **r >= 3).count())
                .try_into()
                .unwrap_or(u32::MAX),
            pushbacks: self.pushbacks,
            respawns: self.respawns,
            restarts: self.restarts,
            queue_to_merge_median_secs: at(&self.q2m, 50),
            queue_to_merge_p90_secs: at(&self.q2m, 90),
            owner_wait_secs: self.wait,
            owner_wait_pct: pct(self.wait, total),
            closed_unmerged: self.closed,
            parked: self.parked,
        }
    }
}

fn line(r: &Row) -> String {
    let list = |m: &BTreeMap<&str, u32>| {
        let v: Vec<String> = m.iter().map(|(k, n)| format!("{k} {n}")).collect();
        if v.is_empty() {
            "none".into()
        } else {
            v.join(", ")
        }
    };
    format!(
        "{}: merged {} · no owner action {} ({}%) · owner actions {:.2}/merge ({}) · asks {} ({}) · \
         rounds median {}, round 3+ {} · pushbacks {} · respawns {} · restarts {} · queue → merge median {} p90 {}, \
         owner wait {} ({}%) · closed unmerged {} · parked {}",
        r.week,
        r.merged,
        r.autonomous,
        r.autonomous_pct,
        r.owner_actions_per_merge,
        list(&r.owner_actions),
        r.asks.values().sum::<u32>(),
        list(&r.asks),
        r.rounds_median,
        r.round3,
        r.pushbacks,
        r.respawns,
        r.restarts,
        age(r.queue_to_merge_median_secs),
        age(r.queue_to_merge_p90_secs),
        age(r.owner_wait_secs),
        r.owner_wait_pct,
        r.closed_unmerged,
        r.parked,
    )
}

/// #962: `--explain`: the owner-action table, one rule a line.
pub fn explain(out: &mut dyn Write) -> Result<()> {
    for (kind, label, rule, _) in OWNER {
        writeln!(out, "{label}: a `{kind}` event; {rule}")?;
    }
    let auto = "`by` is the owner's: set, and not `orchestrator-*`, `default`, `drive`, `lane` or `external`";
    Ok(writeln!(out, "{auto}, on an event no orchestrator lane logged")?)
}

fn waited(evs: &[&Event], ask: &HashMap<&str, Vec<&Event>>) -> u64 {
    let mut wait = 0;
    for (i, e) in evs.iter().enumerate() {
        let end = match e.kind.as_str() {
            "needs-decision" => (ask.get(e.str("id").unwrap_or_default()).into_iter().flatten())
                .find(|d| names(d, e) && owned(d))
                .map(|d| d.ts),
            "needs-human" | "parked" => {
                let back = ["round-extended", "unparked", "spawn", "decision", "merged", "closed"];
                (evs.iter().skip(i + 1))
                    .find(|x| back.contains(&x.kind.as_str()))
                    .map(|x| x.ts)
            }
            _ => None,
        };
        wait += end.map_or(0, |t| t.saturating_sub(e.ts));
    }
    wait
}

/// `drover stats`: numbers from the event log.
#[derive(clap::Subcommand)]
pub enum Stats {
    #[command(
        about = "#350: per landing mode, median PASS → landed, landings per day, reverts, check time; red minutes on integration"
    )]
    Land { fleet: String },
    #[command(
        about = "#962: per week and in total, merges with no owner action, owner actions per merge, asks, rounds, respawns, queue → merge time and its owner wait"
    )]
    Autonomy {
        fleet: String,
        #[arg(long, help = "Count from this date (YYYY-MM-DD)")]
        since: Option<String>,
        #[arg(long, help = "Count the last N weeks only, this one included")]
        weeks: Option<u64>,
        #[arg(long, help = "Machine-readable output")]
        json: bool,
        #[arg(long, help = "Print what counts as an owner action instead")]
        explain: bool,
    },
}

impl Stats {
    /// Run this `stats` command.
    pub fn run(self, ctx: &Ctx, cfg: &Config, out: &mut dyn Write) -> Result<()> {
        match self {
            Stats::Land { fleet } => crate::land::stats(ctx, cfg, out, &fleet),
            Stats::Autonomy { explain: true, .. } => explain(out),
            Stats::Autonomy {
                fleet,
                since,
                weeks,
                json,
                ..
            } => stats(ctx, cfg, out, &fleet, (since.as_deref(), weeks, json)),
        }
    }
}

/// #962: `drover stats autonomy <fleet>`: per week and in total, merges with no owner action, owner actions, asks, rounds, respawns, queue → merge time and its owner wait, closes and parks.
pub fn stats(
    ctx: &Ctx,
    cfg: &Config,
    out: &mut dyn Write,
    fleet: &str,
    (since, weeks, as_json): (Option<&str>, Option<u64>, bool),
) -> Result<()> {
    cfg.fleet(fleet)?;
    let from = match since {
        Some(d) => match crate::usage::epoch(&format!("{d}T00:00:00Z")).filter(|_| d.len() == 10) {
            Some(t) => t,
            None => bail!("--since {d}: a date is YYYY-MM-DD"),
        },
        None => 0,
    };
    let from = weeks.map_or(from, |n| {
        let start = (week(ctx.now) + 1).saturating_sub(n) * 7;
        from.max(start.saturating_sub(3) * 86_400)
    });
    let ev = crate::events::history(&ctx.state)?;
    let (mut issues, mut queued, mut ask) = (HashMap::new(), HashMap::new(), HashMap::new());
    let mut free: Vec<&Event> = vec![];
    for e in ev.iter().filter(|e| e.fleet == fleet) {
        if e.kind == "queue" && e.str("op") == Some("add") {
            for n in e
                .val("issues")
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_u64)
            {
                queued.entry(n).or_insert(e.ts);
            }
        }
        if e.kind == "decision" {
            ask.entry(e.str("id").unwrap_or_default())
                .or_insert_with(Vec::new)
                .push(e);
        }
        match e.issue {
            Some(n) => issues.entry(n).or_insert_with(Vec::new).push(e),
            None => free.push(e),
        }
    }
    let (mut rows, mut total) = (BTreeMap::<u64, Acc>::new(), Acc::default());
    let mut add = |ts: u64, f: &dyn Fn(&mut Acc)| {
        if ts >= from {
            f(rows.entry(week(ts)).or_default());
            f(&mut total);
        }
    };
    let by = |a: &Event| -> &'static str {
        let d = (ask.get(a.str("id").unwrap_or_default()).into_iter().flatten()).find(|d| names(d, a));
        match d.and_then(|d| d.str("by")) {
            None => "open",
            Some("default") => "default",
            Some(_) if d.is_some_and(|d| owned(d)) => "owner",
            Some(_) => "orchestrator",
        }
    };
    let all = issues.values().flatten().chain(free.iter());
    for e in all {
        if let Some(k) = owner(e) {
            add(e.ts, &|a| *a.actions.entry(k).or_default() += 1);
        }
        if e.kind == "needs-decision" {
            let k = by(e);
            add(e.ts, &|a| *a.asks.entry(k).or_default() += 1);
        }
    }
    for (n, evs) in &issues {
        let Some(m) = evs.iter().rfind(|e| e.kind == "merged") else {
            let gone = |k| evs.iter().filter(move |e| e.kind == k);
            gone("closed").for_each(|e| add(e.ts, &|a| a.closed += 1));
            gone("parked").for_each(|e| add(e.ts, &|a| a.parked += 1));
            continue;
        };
        let start = queued
            .get(n)
            .copied()
            .unwrap_or_else(|| evs.first().map_or(m.ts, |e| e.ts));
        let life: Vec<&Event> = evs.iter().copied().filter(|e| e.ts <= m.ts).collect();
        let count = |k: &str| u32::try_from(life.iter().filter(|e| e.kind == k).count()).unwrap_or(u32::MAX);
        let round = (life.iter().filter(|e| e.kind == "verdict"))
            .map(|e| e.val("round").as_u64().unwrap_or(1))
            .max()
            .unwrap_or_default();
        let auto = !life.iter().any(|e| owner(e).is_some());
        let (pushbacks, respawns, restarts) = (count("fix-done"), count("respawn"), count("supersede"));
        let (q2m, wait) = (m.ts.saturating_sub(start), waited(&life, &ask));
        add(m.ts, &|a| {
            a.merged += 1;
            a.autonomous += u32::from(auto);
            a.rounds.push(round);
            a.pushbacks += pushbacks;
            a.respawns += respawns;
            a.restarts += restarts;
            a.q2m.push(q2m);
            a.wait += wait;
        });
    }
    let rows: Vec<Row> = (rows.into_iter())
        .map(|(w, a)| a.row(crate::lesson::ymd((w * 7).saturating_sub(3))))
        .collect();
    let total = total.row("total".into());
    if as_json {
        let j = json!({"fleet": fleet, "since": from, "weeks": rows, "total": total});
        return Ok(writeln!(out, "{}", serde_json::to_string_pretty(&j)?)?);
    }
    for r in rows.iter().chain([&total]) {
        writeln!(out, "{fleet} {}", line(r))?;
    }
    Ok(())
}
