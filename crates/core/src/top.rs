use crate::config::Config;
use crate::events::{age, Event};
use crate::host;
use crate::runner::{ok, Runner};
use crate::session::Session;
use crate::Ctx;
use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::io::Write;
use std::path::Path;

/// One process as `ps` sees it: its depth in its lane's tree, CPU %, seconds up and heavy kind.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Proc {
    pub pid: String,
    pub ppid: String,
    pub depth: usize,
    pub cmd: String,
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    pub cpu: f64,
    pub secs: u64,
    pub background: bool,
}

/// A heavy check in line for a slot or holding one here, and for how long.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Wait {
    pub kind: String,
    pub cmd: String,
    pub secs: u64,
    /// Why a routed check holds a slot here instead of running on its build machine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub here: Option<String>,
}

/// A live lane: its agent, `<fleet>:<issue>:<role>`, process tree and heavy-slot waits.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Lane {
    pub agent: String,
    pub lane: String,
    pub procs: Vec<Proc>,
    pub waits: Vec<Wait>,
    /// Its heavy checks holding a slot on this host.
    pub held: Vec<Wait>,
    /// An impl or rev lane, whose processes all belong at background priority.
    pub build: bool,
}

/// `drover top`: the host's CPU idle against its floor, heavy slots, held respawns and every live lane.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Top {
    pub cpu_idle: Option<f64>,
    /// Why the CPU idle is unknown: `<tool> not found`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_unknown: Option<String>,
    pub limit: f64,
    pub heavy_used: usize,
    pub heavy_max: usize,
    pub respawns_held: usize,
    /// drover's own processes' CPU %, summed (100 = one core).
    pub drover_cpu: f64,
    /// Every process's CPU %, summed: the host's busy CPU.
    pub busy_cpu: f64,
    pub lanes: Vec<Lane>,
    /// `drover hold` jobs (holder `ci:<repo>`) running or waiting in the heavy line.
    pub holds: Vec<crate::heavy::Ticket>,
}

fn ps(r: &dyn Runner, cfg: &Config) -> Vec<Proc> {
    let kinds = cfg.fleet.values().flat_map(crate::config::Fleet::heavy_kinds).collect();
    let heavy = crate::heavy::Heavy {
        kinds,
        ..Default::default()
    };
    let out = ok(r, "ps", &["-axo", "pid=,ppid=,pcpu=,etime=,pri=,nice=,command="]).unwrap_or_default();
    let row = |l: &str| {
        let w: Vec<&str> = l.split_whitespace().collect();
        let cmd = w.get(6..)?.join(" ");
        let (pri, nice): (i32, i32) = (w.get(4)?.parse().ok()?, w.get(5)?.parse().ok()?);
        let (prog, rest) = cmd.split_once(' ').unwrap_or((&cmd, ""));
        let bare = format!("{} {rest}", prog.rsplit('/').next().unwrap_or(prog));
        let tag = (bare.starts_with("runners run "))
            .then(|| rest.split_once("--tag "))
            .flatten();
        Some(Proc {
            pid: w.first()?.to_string(),
            ppid: w.get(1)?.to_string(),
            depth: 0,
            kind: heavy.kind(bare.trim_end()).map(String::from),
            remote: tag.and_then(|(_, t)| t.split_whitespace().next()).map(String::from),
            cpu: w.get(2)?.parse().ok()?,
            secs: host::etime_secs(w.get(3)?),
            background: pri <= 4 || nice >= 10,
            cmd,
        })
    };
    out.lines().filter_map(row).collect()
}

fn walk(mine: &[&Proc], at: &Proc, depth: usize, out: &mut Vec<Proc>) {
    out.push(Proc { depth, ..at.clone() });
    for c in mine.iter().filter(|c| c.ppid == at.pid && depth < 64) {
        walk(mine, c, depth + 1, out);
    }
}

fn lanes(
    ctx: &Ctx,
    cfg: &Config,
    events: &[Event],
    agents: &[Session],
    all: &[Proc],
    cwds: &[(String, String, String)],
) -> Vec<Lane> {
    let queue = crate::heavy::line(&ctx.state.with_file_name("heavy"));
    let lane = |a: &Session| {
        let agent = a.name.as_str();
        let sp = (events.iter()).rfind(|e| e.kind == "spawn" && e.str("agent") == Some(agent))?;
        let issue = sp.issue.map_or("-".into(), |n| n.to_string());
        let role = sp.str("role").unwrap_or_default();
        let key = format!("{}:{issue}:{role}", sp.fleet);
        let family = cfg.fleet.get(&sp.fleet).map(|f| f.family(role));
        let dir = sp.str("worktree").map_or(a.cwd.as_path(), Path::new);
        let pids: Vec<String> = host::inside(cwds, dir).into_iter().map(|p| p.0).collect();
        let mine: Vec<&Proc> = all.iter().filter(|p| pids.contains(&p.pid)).collect();
        let mut procs = vec![];
        for root in mine.iter().filter(|p| !mine.iter().any(|q| q.pid == p.ppid)) {
            walk(&mine, root, 0, &mut procs);
        }
        let of = |running: bool| {
            (queue.iter().filter(|(_, t)| t.running == running && t.lane == key))
                .map(|(_, t)| Wait {
                    kind: t.kind.clone(),
                    cmd: t.cmd.clone(),
                    secs: ctx.now.saturating_sub(t.since),
                    here: t.here.clone(),
                })
                .collect()
        };
        Some(Lane {
            agent: agent.into(),
            waits: of(false),
            held: of(true),
            lane: key,
            procs,
            build: matches!(family, Some("impl" | "rev")),
        })
    };
    agents.iter().filter_map(lane).collect()
}

fn snapshot(ctx: &Ctx, cfg: &Config, r: &dyn Runner, agents: &[Session]) -> Result<(Vec<Proc>, Vec<Lane>)> {
    let events = crate::events::working(&ctx.state)?;
    let all = ps(r, cfg);
    let lanes = lanes(ctx, cfg, &events, agents, &all, &host::cwds(r));
    Ok((all, lanes))
}

/// The `n` busiest processes by CPU as `cpu% command (lane agent | other)`, for `host-overloaded`.
pub fn busiest(ctx: &Ctx, cfg: &Config, r: &dyn Runner, agents: &[Session], n: usize) -> Result<Vec<String>> {
    let (mut all, lanes) = snapshot(ctx, cfg, r, agents)?;
    all.sort_by(|a, b| b.cpu.total_cmp(&a.cpu));
    let owner = |p: &Proc| {
        let l = lanes.iter().find(|l| l.procs.iter().any(|q| q.pid == p.pid));
        l.map_or("other".into(), |l| format!("lane {}", l.agent))
    };
    Ok(all
        .iter()
        .take(n)
        .map(|p| format!("{:.0}% {} ({})", p.cpu, p.cmd, owner(p)))
        .collect())
}

/// Each live build lane process not at background priority, as `(agent, pid)`, for doctor's `lane priority`.
pub fn normal(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> Result<Vec<(String, String)>> {
    let (_, lanes) = snapshot(ctx, cfg, r, &crate::session::live_on(r)?)?;
    let procs = (lanes.iter().filter(|l| l.build)).flat_map(|l| l.procs.iter().map(move |p| (l, p)));
    let normal = procs.filter(|(_, p)| !p.background);
    Ok(normal.map(|(l, p)| (l.agent.clone(), p.pid.clone())).collect())
}

/// Sample the host and build `drover top`'s rows.
pub fn top(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> Result<Top> {
    let agents = crate::session::live_on(r)?;
    let (all, lanes) = snapshot(ctx, cfg, r, &agents)?;
    let exe = Path::new(&ctx.exe).file_name().unwrap_or_default();
    let ours = |p: &&Proc| {
        p.cmd
            .split(' ')
            .next()
            .is_some_and(|c| Path::new(c).file_name() == Some(exe))
    };
    let main = crate::hosts::cards(cfg, r).into_iter().next().and_then(|c| c.1);
    let line = crate::heavy::line(&ctx.state.with_file_name("heavy"));
    let passes = crate::events::since(&ctx.state, &["drive-pass"], ctx.now.saturating_sub(3600))?;
    let last = |f: &String| passes.iter().rfind(|e| e.fleet == *f);
    let acts = |e: &Event| {
        e.data
            .get("actions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let held = cfg.fleet.keys().filter_map(last).flat_map(acts);
    let h = host::sample(r);
    Ok(Top {
        cpu_idle: h.cpu_idle,
        idle_unknown: h.not_found(&["iostat", "vmstat"]).filter(|_| h.cpu_idle.is_none()),
        limit: cfg.limits_on(None).min_cpu_idle,
        heavy_used: line.iter().filter(|(_, t)| t.running).count(),
        heavy_max: main.and_then(|h| h.heavy_max).unwrap_or(3),
        respawns_held: held.filter(|a| a["code"] == "RESPAWN_PACED").count(),
        drover_cpu: all.iter().filter(ours).map(|p| p.cpu).sum(),
        busy_cpu: all.iter().map(|p| p.cpu).sum(),
        lanes,
        holds: (line.iter())
            .filter(|(_, t)| t.lane.starts_with("ci:"))
            .map(|(_, t)| t.clone())
            .collect(),
    })
}

/// `drover top`'s plain text: the header line, then each lane and its indented process tree.
pub fn print(t: &Top, out: &mut dyn Write) -> Result<()> {
    let idle = match (t.cpu_idle, &t.idle_unknown) {
        (Some(i), _) => format!("cpu idle {i:.0}%"),
        (None, Some(why)) => format!("? cpu idle — {why}"),
        (None, None) => "cpu idle ?".into(),
    };
    let share = if t.busy_cpu > 0.0 {
        100.0 * t.drover_cpu / t.busy_cpu
    } else {
        0.0
    };
    writeln!(
        out,
        "{idle} (floor {}%) · heavy slots {}/{} · respawns held {} · drover {:.0}% of busy CPU ({:.0}% of {:.0}%)",
        t.limit, t.heavy_used, t.heavy_max, t.respawns_held, share, t.drover_cpu, t.busy_cpu
    )?;
    for l in &t.lanes {
        writeln!(out, "{} ({})", l.agent, l.lane)?;
        for p in &l.procs {
            let mut kind = p.kind.as_ref().map_or(String::new(), |k| format!(" [{k}]"));
            kind.extend(p.remote.as_ref().map(|t| format!(" [remote {t}]")));
            if l.build && !p.background {
                kind.push_str(" [normal priority]");
            }
            let pad = "  ".repeat(p.depth + 1);
            writeln!(out, "{pad}{} {:.0}% {} {}{kind}", p.pid, p.cpu, age(p.secs), p.cmd)?;
        }
        for w in &l.waits {
            writeln!(out, "  waiting for heavy slot {} {}: {}", w.kind, age(w.secs), w.cmd)?;
        }
        for w in &l.held {
            let why = w.here.as_ref().map_or(String::new(), |h| format!(" ({h})"));
            writeln!(out, "  heavy slot {} here{why} {}: {}", w.kind, age(w.secs), w.cmd)?;
        }
    }
    for h in &t.holds {
        let at = if h.running { "running" } else { "waiting" };
        writeln!(out, "{} {at}: {}", h.lane, h.cmd)?;
    }
    Ok(())
}
