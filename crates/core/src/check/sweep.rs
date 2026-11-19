use super::{build_dirs, building, du, gb};
use crate::config::Config;
use crate::events::{emit, fold, lanes, last_spawn, working, Event};
use crate::host;
use crate::hosts::On;
use crate::runner::Runner;
use crate::session::Session;
use crate::Ctx;
use anyhow::Result;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::Write;

pub(super) type Off<'c> = Vec<(&'c str, &'c str, Option<Vec<Session>>)>;

pub(super) fn disabled<'c>(cfg: &'c Config, r: &dyn Runner) -> Off<'c> {
    let hosts = cfg.hosts.iter().filter(|h| !h.1.enabled);
    let hosts = hosts.filter_map(|(n, h)| Some((n.as_str(), h.machine.as_deref()?)));
    hosts
        .map(|(n, m)| (n, m, crate::session::live_on(&On::new(r, Some(m))).ok()))
        .collect()
}

pub(super) fn sweep_off(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    off: &Off,
    dry: bool,
) -> Result<u64> {
    let mut freed = 0;
    for (name, m, agents) in off {
        let Some(agents) = agents else {
            writeln!(out, "scratch: {name}: host offline (not cleaned)")?;
            continue;
        };
        let on = On::new(r, Some(m));
        for b in build_dirs(ctx, cfg, &on) {
            let b = format!("{b}/{}", crate::spawn::SCRATCH);
            let res = scratch(&on, &b, &[]).and_then(|v| {
                let held = |n: &String| agents.iter().any(|a| a.name == *n);
                let gone: Vec<String> = v.into_iter().map(|n| n.0).filter(|n| !held(n)).collect();
                if dry || gone.is_empty() {
                    Ok(gone.into_iter().map(|n| (n, 0)).collect())
                } else {
                    scratch(&on, &b, &gone)
                }
            });
            let gone = match res {
                Ok(g) => g,
                Err(e) => {
                    writeln!(out, "scratch: {name}: {e:#}")?;
                    continue;
                }
            };
            for (n, bytes) in gone {
                let p = format!("{b}/{n}");
                let line = if dry {
                    format!("would remove scratch {name}:{p}")
                } else {
                    format!("scratch: {name}: removed {p} {:.2} GB (no live lane there)", gb(bytes))
                };
                freed += bytes;
                writeln!(out, "{line}")?;
            }
        }
    }
    Ok(freed)
}

/// How a host's lane folders are swept: the tick's (a host short of disk drops idle lanes' scratch too), or `reclaim`'s (finished lanes only), previewed or applied.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sweep {
    Tick,
    Preview,
    Apply,
}

/// Host `host`'s finished lanes' build and scratch folders removed (`Preview`: listed); bytes freed.
pub fn sweep(ctx: &Ctx, cfg: &Config, on: &On, host: &str, out: &mut dyn Write, how: Sweep) -> Result<u64> {
    reported(ctx, cfg, on)?;
    let mut freed = 0;
    for b in build_dirs(ctx, cfg, on) {
        freed += sweep_in(
            ctx,
            cfg,
            on,
            (host, &format!("{b}/{}", crate::spawn::SCRATCH)),
            out,
            how,
        )?;
        freed += sweep_in(ctx, cfg, on, (host, &b), out, how)?;
    }
    Ok(freed)
}

pub(super) fn reported(ctx: &Ctx, cfg: &Config, on: &On) -> Result<()> {
    let view = on.machine.and_then(|_| crate::link::view::View::load(ctx, cfg).ok());
    match view.and_then(|v| v.failing(on.machine)) {
        Some(why) => anyhow::bail!("its report fails ({why}): no folder removed"),
        None => Ok(()),
    }
}

fn finished(events: &[Event], states: &crate::events::States, sp: &Event) -> Option<&'static str> {
    let (agent, role) = (sp.str("agent")?, sp.str("role").unwrap_or_default());
    let s = states.get(&(sp.fleet.clone(), sp.issue?))?;
    if s.done() {
        return Some("lane finished");
    }
    if s.lanes.get(role).map(String::as_str) != Some(agent) {
        return Some("superseded");
    }
    let ends = |e: &Event| match e.kind.as_str() {
        "verdict" | "design-review" => true,
        "turn-end" => e.str("state") == Some("done") && e.str("role") == Some(role),
        _ => false,
    };
    let mine = crate::events::of_issue(events, &sp.fleet, sp.issue?);
    mine.into_iter()
        .filter(|e| e.seq > sp.seq)
        .any(ends)
        .then_some("lane finished")
}

fn sweep_in(ctx: &Ctx, cfg: &Config, on: &On, (host, b): (&str, &str), out: &mut dyn Write, how: Sweep) -> Result<u64> {
    let names = scratch(on, b, &[])?;
    if names.is_empty() {
        return Ok(0);
    }
    let scr = b.ends_with(crate::spawn::SCRATCH);
    let (agents, events) = (crate::session::live_on(on)?, working(&ctx.state)?);
    let states = fold(&events);
    let agent = |n: &str| agents.iter().find(|a| a.name == n);
    let lanes = lanes(&events);
    let spawn = |n: &str| last_spawn(&events, n);
    let spawned = |n: &str| (lanes.iter()).any(|e| e.kind.starts_with("spawn") && e.str("agent") == Some(n));
    let low = how == Sweep::Tick && host::free_gb(on).is_some_and(|g| g < cfg.limits.min_free_gb);
    let (mut why, total) = (BTreeMap::new(), names.len());
    for (n, _) in names {
        if agent(&n).is_some_and(Session::busy) {
            continue;
        }
        let ended = spawn(&n).and_then(|sp| finished(&events, &states, sp));
        let w = match ended {
            Some(w) => w,
            None if agent(&n).is_none() && (scr || spawned(&n)) => "lane ended",
            None if low && scr => "disk low",
            None => continue,
        };
        why.insert(n, w);
    }
    let gone: Vec<String> = why.keys().cloned().collect();
    let what = if scr { "scratch" } else { "builds" };
    if how == Sweep::Preview {
        for (n, w) in &why {
            writeln!(out, "would remove {what} {host}:{b}/{n} ({w})")?;
        }
        return Ok(0);
    }
    let removed = if gone.is_empty() {
        vec![]
    } else {
        scratch(on, b, &gone)?
    };
    for (n, bytes) in &removed {
        let w = why.get(n).copied().unwrap_or_default();
        writeln!(out, "{what}: {host}: removed {b}/{n} {:.2} GB ({})", gb(*bytes), w)?;
        if let Some(sp) = spawn(n) {
            let d = json!({"agent": n, "bytes": bytes, "why": w, "host": host, "path": format!("{b}/{n}")});
            emit(
                &ctx.state,
                cfg,
                Event::new(ctx.now, &sp.fleet, sp.issue, "build-freed", d),
            )?;
        }
    }
    if on.machine.is_some() && scr {
        writeln!(
            out,
            "scratch: {host}: {} kept, {} removed",
            total - removed.len(),
            removed.len()
        )?;
    }
    Ok(removed.iter().map(|r| r.1).sum())
}

/// Lane `sp`'s `.scratch/<agent>` folder removed, here or on its host, once its verdict is recorded.
pub fn drop_scratch(r: &dyn Runner, sp: &Event, out: &mut dyn Write) -> Result<()> {
    let (Some(agent), Some(d)) = (sp.str("agent"), crate::hosts::build_dirs(sp).pop()) else {
        return Ok(());
    };
    let Some(b) = d.parent().map(|b| b.display().to_string()) else {
        return Ok(());
    };
    match scratch(&On::of(r, Some(sp)), &b, &[agent.into()]) {
        Ok(gone) => gone.iter().try_for_each(|(n, bytes)| {
            writeln!(out, "scratch: removed {b}/{n} {:.2} GB (verdict recorded)", gb(*bytes))
        })?,
        Err(e) => writeln!(out, "scratch: {b}/{agent}: {e:#}")?,
    }
    Ok(())
}

fn scratch(on: &On, b: &str, rm: &[String]) -> Result<Vec<(String, u64)>> {
    if on.machine.is_some() {
        let args = crate::runner::argv(&["-c", SCRATCH, "sh", b]);
        let args = [args, rm.to_vec()].concat();
        let o = on.run("sh", &args)?;
        anyhow::ensure!(o.code == 0, "sweep failed ({}): {}", o.code, o.stderr.trim());
        let row = |l: &str| match l.split_once(' ') {
            Some((kb, n)) if !rm.is_empty() => Some((n.into(), kb.parse::<u64>().ok()? * 1024)),
            _ => (rm.is_empty() && !l.is_empty()).then(|| (l.into(), 0)),
        };
        return Ok(o.stdout.lines().filter_map(row).collect());
    }
    let root = crate::config::expand(b);
    let ls = std::fs::read_dir(&root).into_iter().flatten().flatten();
    let all = ls
        .filter(|e| e.path().is_dir() && !e.file_name().as_encoded_bytes().starts_with(b"."))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), 0));
    if rm.is_empty() {
        return Ok(all.collect());
    }
    let mut v: Vec<_> = rm.iter().map(|n| (du(&root.join(n)), n)).collect();
    v.sort_by(|a, b| b.cmp(a));
    let gone = |(s, n): (u64, &String)| {
        let d = root.join(n);
        (!building(&d, 3) && std::fs::remove_dir_all(&d).is_ok()).then(|| (n.clone(), s))
    };
    Ok(v.into_iter().filter_map(gone).collect())
}

const SCRATCH: &str = r#"b=$1; shift; case $b in "~/"*) b=$HOME/${b#"~/"};; esac
cd "$b" 2>/dev/null || exit 0
[ $# -eq 0 ] && { for d in */; do [ -d "$d" ] && echo "${d%/}"; done; exit 0; }
du -sk -- "$@" 2>/dev/null | sort -rn | while read -r s d; do
  l=$(find "$d" -maxdepth 4 -name .cargo-lock 2>/dev/null)
  [ -n "$l" ] && command -v lsof >/dev/null && [ -n "$(lsof $l 2>/dev/null)" ] && continue
  rm -rf "./${d:?}" && echo "$s $d"
done
exit 0"#;
