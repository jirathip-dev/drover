use crate::config::Config;
use crate::events::{append, Event};
use crate::hosts::On;
use crate::runner::{argv, Runner};
use crate::Ctx;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use shed_proto::{Method, Target};
use std::io::Write;
use std::path::Path;

/// The nightly soak's hosts (`[hosts]` names) and targets: spawn time, verdict read back, stuck lanes.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Soak {
    pub hosts: Vec<String>,
    pub spawn_secs: u64,
    pub verdict_minutes: u64,
    #[serde(default)]
    pub stuck: u64,
}

const DIR: &str = "/tmp/drover-soak";

/// `drover soak <fleet>`.
#[derive(clap::Args)]
pub struct Args {
    pub fleet: String,
}

struct Took {
    host: String,
    spawn: Option<u64>,
    verdict: Option<u64>,
    stuck: bool,
}

fn ms(since: std::time::Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn lane(cfg: &Config, r: &dyn Runner, fleet: &str, host: &str, t: &Soak) -> Took {
    let machine = cfg.hosts.get(host).and_then(|h| h.machine.as_deref());
    let on = On::new(r, machine);
    let name = format!("{fleet}-soak-{host}");
    let file = format!("{DIR}/{name}.md");
    let mut took = Took {
        host: host.into(),
        spawn: None,
        verdict: None,
        stuck: true,
    };
    let Some(wire) = on.shed().map(|s| s.wire) else {
        return took;
    };
    let call = |m, p: Value| crate::shed::call(&on, &wire, m, p);
    let target = || json!(Target { name: name.clone() });
    call(Method::Kill, target()).ok();
    call(Method::Rm, target()).ok();
    on.remove(Path::new(&file)).ok();
    let mut p = crate::term::Shed::run_params(&name, &[("fleet", fleet.into()), ("role", "soak".into())], "/");
    let script = format!(
        "mkdir -p {DIR} && echo PASS > {file} && exec sleep {}",
        2 * t.verdict_minutes * 60
    );
    p.cmd = argv(&["sh", "-c", &script]);
    let start = crate::clock::instant();
    if call(Method::Run, json!(p)).is_err() {
        return took;
    }
    took.spawn = Some(ms(start));
    let up = crate::clock::instant();
    for _ in 0..=2 * t.verdict_minutes * 60 {
        if on.read(Path::new(&file)).is_some_and(|v| v.trim() == "PASS") {
            took.verdict = Some(ms(up));
            break;
        }
        r.sleep(1);
    }
    call(Method::Kill, target()).ok();
    let ended = call(Method::Rm, target()).is_ok();
    on.remove(Path::new(&file)).ok();
    took.stuck = !ended || took.verdict.is_none() || call(Method::Get, target()).is_ok();
    took
}

fn missed(t: &Soak, all: &[Took]) -> Vec<(&'static str, String)> {
    let over = |v: Option<u64>, max: u64| v.is_none_or(|v| v > max);
    let named = |f: &dyn Fn(&Took) -> bool| all.iter().filter(|x| f(x)).map(|x| x.host.clone()).collect::<Vec<_>>();
    let spawn = named(&|x| over(x.spawn, t.spawn_secs * 1000));
    let verdict = named(&|x| over(x.verdict, t.verdict_minutes * 60_000));
    let stuck = named(&|x| x.stuck);
    let mut out = vec![];
    if !spawn.is_empty() {
        out.push((
            "spawn",
            format!("spawn over {} s on {}", t.spawn_secs, spawn.join(", ")),
        ));
    }
    if !verdict.is_empty() {
        let hosts = verdict.join(", ");
        out.push((
            "verdict",
            format!("verdict not read back within {} min on {hosts}", t.verdict_minutes),
        ));
    }
    if u64::try_from(stuck.len()).unwrap_or(u64::MAX) > t.stuck {
        out.push((
            "stuck",
            format!("{} stuck lanes (target {}): {}", stuck.len(), t.stuck, stuck.join(", ")),
        ));
    }
    out
}

/// One soak of `fleet`: a throwaway lane per `[fleet.<n>.soak] hosts` host, its numbers logged as `soak`, one issue per missed target per night.
pub fn run(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<i32> {
    let f = cfg.fleet(fleet)?;
    let t = f
        .soak
        .as_ref()
        .with_context(|| format!("{fleet} has no [fleet.{fleet}.soak] targets"))?;
    let all = crate::hosts::each(t.hosts.iter(), |h| lane(cfg, r, fleet, h, t));
    let mut hosts = serde_json::Map::new();
    for x in &all {
        let row = json!({"spawn_ms": x.spawn, "verdict_ms": x.verdict, "stuck": x.stuck});
        writeln!(out, "{fleet} soak {}: {row}", x.host)?;
        hosts.insert(x.host.clone(), row);
    }
    let misses = missed(t, &all);
    let d = json!({"hosts": hosts, "missed": misses.iter().map(|m| m.0).collect::<Vec<_>>()});
    append(&ctx.state, &Event::new(ctx.now, fleet, None, "soak", d.clone()))?;
    let day = ctx.now / 86400;
    let (m, dd) = crate::access::civil_month_day(day);
    let date = format!("{}-{m:02}-{dd:02}", crate::access::civil_year(day));
    for (target, why) in &misses {
        let title = format!("soak: {fleet} {target} target missed {date}");
        if crate::gh::titled(cfg, r, fleet, &title)?.is_some() {
            continue;
        }
        let body = format!("The nightly soak missed its `{target}` target: {why}.\n\n```json\n{d:#}\n```\n");
        let args = ["issue", "create", "-R", &f.repo, "--title", &title, "--body", &body];
        let url = crate::runner::ok(r, "gh", &args)?;
        writeln!(out, "{fleet} soak: filed {}", url.trim())?;
    }
    Ok(i32::from(!misses.is_empty()))
}

/// `tick`'s soak: each fleet with `[fleet.<n>.soak]` (a paused one waits) starts `drover soak <fleet>` once a night, detached so the tick never waits on it.
pub fn nightly(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> Result<()> {
    for (name, _) in cfg.fleet.iter().filter(|(_, f)| f.soak.is_some() && !f.paused) {
        if crate::timer::due(ctx, &format!("soak-{name}"), 86400)? {
            let log = ctx.state.with_file_name(format!("soak-{name}.log"));
            r.detach(&ctx.exe, &argv(&["soak", name]), &log)?;
        }
    }
    Ok(())
}
