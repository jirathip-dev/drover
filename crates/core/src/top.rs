use crate::config::Config;
use crate::events::{age, Event};
use crate::host;
use crate::runner::{ok, Runner};
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
    pub cpu: f64,
    pub secs: u64,
}

/// A heavy check in line for a slot, and how long it has waited.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Wait {
    pub kind: String,
    pub cmd: String,
    pub secs: u64,
}

/// A live lane: its agent, `<fleet>:<issue>:<role>`, process tree and heavy-slot waits.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Lane {
    pub agent: String,
    pub lane: String,
    pub procs: Vec<Proc>,
    pub waits: Vec<Wait>,
}

/// `drover top`: the host's load against its limit, heavy slots, held respawns and every live lane.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Top {
    pub load_per_cpu: Option<f64>,
    pub limit: f64,
    pub heavy_used: usize,
    pub heavy_max: usize,
    pub respawns_held: usize,
    pub lanes: Vec<Lane>,
}

fn ps(r: &dyn Runner, cfg: &Config) -> Vec<Proc> {
    let kinds = cfg.fleet.values().flat_map(|f| f.heavy.clone()).collect();
    let heavy = crate::heavy::Heavy {
        kinds,
        ..Default::default()
    };
    let out = ok(r, "ps", &["-axo", "pid=,ppid=,pcpu=,etime=,command="]).unwrap_or_default();
    let row = |l: &str| {
        let w: Vec<&str> = l.split_whitespace().collect();
        let cmd = w.get(4..)?.join(" ");
        let (prog, rest) = cmd.split_once(' ').unwrap_or((&cmd, ""));
        let bare = format!("{} {rest}", prog.rsplit('/').next().unwrap_or(prog));
        Some(Proc {
            pid: w[0].into(),
            ppid: w[1].into(),
            depth: 0,
            kind: heavy.kind(bare.trim_end()).map(String::from),
            cpu: w[2].parse().ok()?,
            secs: host::etime_secs(w[3]),
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

fn lanes(ctx: &Ctx, events: &[Event], agents: &[Value], all: &[Proc], cwds: &[(String, String, String)]) -> Vec<Lane> {
    let queue = crate::heavy::line(&ctx.state.with_file_name("heavy"));
    let lane = |a: &Value| {
        let agent = a["name"].as_str()?;
        let sp = (events.iter()).rfind(|e| e.kind == "spawn" && e.str("agent") == Some(agent))?;
        let issue = sp.issue.map_or("-".into(), |n| n.to_string());
        let key = format!("{}:{issue}:{}", sp.fleet, sp.str("role").unwrap_or_default());
        let dir = Path::new(sp.str("worktree").or(a["cwd"].as_str()).unwrap_or("\0"));
        let pids: Vec<String> = host::inside(cwds, dir).into_iter().map(|p| p.0).collect();
        let mine: Vec<&Proc> = all.iter().filter(|p| pids.contains(&p.pid)).collect();
        let mut procs = vec![];
        for root in mine.iter().filter(|p| !mine.iter().any(|q| q.pid == p.ppid)) {
            walk(&mine, root, 0, &mut procs);
        }
        let waits = (queue.iter().filter(|(_, t)| !t.running && t.lane == key)).map(|(_, t)| Wait {
            kind: t.kind.clone(),
            cmd: t.cmd.clone(),
            secs: ctx.now.saturating_sub(t.since),
        });
        let waits = waits.collect();
        Some(Lane {
            agent: agent.into(),
            lane: key,
            procs,
            waits,
        })
    };
    agents.iter().filter_map(lane).collect()
}

fn snapshot(ctx: &Ctx, cfg: &Config, r: &dyn Runner, agents: &[Value]) -> Result<(Vec<Proc>, Vec<Lane>)> {
    let events = crate::events::working(&ctx.state)?;
    let all = ps(r, cfg);
    let lanes = lanes(ctx, &events, agents, &all, &host::cwds(r));
    Ok((all, lanes))
}

/// The `n` busiest processes by CPU as `cpu% command (lane agent | other)`, for `host-overloaded`.
pub fn busiest(ctx: &Ctx, cfg: &Config, r: &dyn Runner, agents: &[Value], n: usize) -> Result<Vec<String>> {
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

/// Sample the host and build `drover top`'s rows.
pub fn top(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> Result<Top> {
    let agents = host::agents(r)?;
    let (_, lanes) = snapshot(ctx, cfg, r, &agents)?;
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
    Ok(Top {
        load_per_cpu: host::sample(r).load_per_cpu,
        limit: cfg.limits.load_per_cpu,
        heavy_used: line.iter().filter(|(_, t)| t.running).count(),
        heavy_max: main.and_then(|h| h.heavy_max).unwrap_or(3),
        respawns_held: held.filter(|a| a["code"] == "RESPAWN_PACED").count(),
        lanes,
    })
}

/// `drover top`'s plain text: the header line, then each lane and its indented process tree.
pub fn print(t: &Top, out: &mut dyn Write) -> Result<()> {
    let load = t.load_per_cpu.map_or("?".into(), |l| format!("{l:.2}"));
    writeln!(
        out,
        "load/cpu {load} (limit {}) · heavy slots {}/{} · respawns held {}",
        t.limit, t.heavy_used, t.heavy_max, t.respawns_held
    )?;
    for l in &t.lanes {
        writeln!(out, "{} ({})", l.agent, l.lane)?;
        for p in &l.procs {
            let kind = p.kind.as_ref().map_or(String::new(), |k| format!(" [{k}]"));
            let pad = "  ".repeat(p.depth + 1);
            writeln!(out, "{pad}{} {:.0}% {} {}{kind}", p.pid, p.cpu, age(p.secs), p.cmd)?;
        }
        for w in &l.waits {
            writeln!(out, "  waiting for heavy slot {} {}: {}", w.kind, age(w.secs), w.cmd)?;
        }
    }
    Ok(())
}
