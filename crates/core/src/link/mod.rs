pub mod frame;
pub mod intake;
pub mod view;
pub mod watch;

use crate::config::Config;
use crate::events::{fold, last_spawn, working, Event};
use crate::runner::Runner;
use crate::Ctx;
use anyhow::{Context, Result};
use frame::{Frame, Watch};
use std::io::Write;

/// Why a host's link is down while every report arrives: nothing holds a link open yet (#1309 step 2).
pub const NO_SERVICE: &str = "no service";

/// Each host drover reads, this one first (its `[hosts]` name, else `local`), then each with a `machine`.
pub fn hosts(cfg: &Config) -> Vec<(String, Option<String>)> {
    let here = cfg.hosts.iter().find(|h| h.1.machine.is_none());
    let here = (here.map_or("local", |h| h.0.as_str()).to_string(), None);
    let there = cfg
        .hosts
        .iter()
        .filter_map(|(n, h)| Some((n.clone(), Some(h.machine.clone()?))));
    std::iter::once(here).chain(there).collect()
}

fn watch(ctx: &Ctx, cfg: &Config, events: &[Event], (name, m): (&str, Option<&str>)) -> Watch {
    let live = fold(events).into_values().filter(|s| !s.done());
    let spawns = live
        .flat_map(|s| s.lanes.into_values())
        .filter_map(|a| last_spawn(events, &a));
    let hung = crate::roots::hung_now(events);
    let mut worktrees: Vec<String> = spawns
        .filter(|sp| sp.str("machine") == m && crate::roots::hung_under(&hung, Some(sp)).is_none())
        .filter_map(|sp| sp.str("worktree").map(String::from))
        .collect();
    worktrees.sort_unstable();
    worktrees.dedup();
    let home = crate::hosts::cached(ctx)
        .get(name)
        .map(|s| s.home.clone())
        .filter(|_| m.is_some());
    let set = cfg
        .hosts
        .get(name)
        .map(crate::config::HostCfg::roots)
        .unwrap_or_default();
    let roots = set
        .iter()
        .map(|p| crate::hosts::under(p, home.as_deref()).display().to_string());
    let l = &cfg.limits;
    Watch {
        roots: roots.collect(),
        worktrees,
        beat_secs: l.link_beat_secs,
        poll_millis: l.link_poll_millis,
        sample_secs: l.link_sample_secs,
    }
}

fn exe(ctx: &Ctx, name: &str) -> String {
    let home = crate::hosts::cached(ctx)
        .get(name)
        .map(|s| s.home.clone())
        .filter(|h| !h.is_empty());
    home.map_or("drover".into(), |h| crate::hosts::real_exe(ctx, &h))
}

fn report(ctx: &Ctx, cfg: &Config, r: &dyn Runner, (name, m): (&str, Option<&str>), ev: &[Event]) -> Result<Frame> {
    let w = watch(ctx, cfg, ev, (name, m));
    let Some(m) = m else { return watch::frame(r, &w) };
    let args = crate::runner::argv(&["hosts", "report"]);
    let o = crate::hosts::On::new(r, Some(m)).run_in(&exe(ctx, name), &args, &serde_json::to_string(&w)?)?;
    let why = || o.stderr.lines().next().map_or(format!("exit {}", o.code), String::from);
    anyhow::ensure!(o.code == 0, "{}", why());
    let line = o.stdout.lines().find(|l| l.starts_with('{')).context("no frame")?;
    Ok(serde_json::from_str(line)?)
}

/// The poll: one report from each host at once (every host with `all`, else those `View::stale` names), each applied in turn, lane states stamped; the view after it.
pub fn look(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, all: bool) -> Result<view::View> {
    let v = view::View::load(ctx, cfg)?;
    let due: Vec<&str> = v.stale();
    poll(ctx, cfg, r, out, &|name, _| all && !v.live(name) || due.contains(&name))
}

/// The host at `m` polled now: after a lane starts there, so the view holds its session.
pub fn look_at(ctx: &Ctx, cfg: &Config, r: &dyn Runner, m: Option<&str>) -> Result<view::View> {
    poll(ctx, cfg, r, &mut std::io::sink(), &|_, x| x == m)
}

fn poll(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    pick: &dyn Fn(&str, Option<&str>) -> bool,
) -> Result<view::View> {
    let events = working(&ctx.state)?;
    let off = crate::hosts::idle_off(cfg, &events);
    let all = (hosts(cfg).into_iter())
        .filter(|h| h.1.as_ref().is_none_or(|m| !off.contains(m)))
        .filter(|h| pick(&h.0, h.1.as_deref()));
    let all: Vec<_> = all.collect();
    if all.is_empty() {
        return view::View::load(ctx, cfg);
    }
    let one = |(name, m): (String, Option<String>)| {
        let f = report(ctx, cfg, r, (&name, m.as_deref()), &events);
        (name, m, f)
    };
    let (here, there): (Vec<_>, Vec<_>) = all.into_iter().partition(|h| h.1.is_none());
    let mut got: Vec<_> = here.into_iter().map(one).collect();
    got.extend(crate::hosts::each(there, one));
    for (name, m, f) in got {
        intake::apply(ctx, cfg, r, out, (&name, m.as_deref()), (f, false))?;
    }
    let v = view::View::load(ctx, cfg)?;
    crate::session::stamp::observe(ctx, cfg, r, &v.sessions())?;
    Ok(v)
}

/// The service's link to host `name` at `m`: `drover hosts watch` there kept open, each frame applied as it comes, dialled again after 1 s, doubling to `link_retry_max_secs`; it never returns.
pub fn run(ctx: &Ctx, r: &dyn Runner, (name, m): (&str, Option<&str>), clock: fn() -> u64) {
    let mut wait = 1;
    loop {
        let c = Ctx {
            now: clock(),
            ..ctx.clone()
        };
        let mut most = wait;
        if let Ok(cfg) = Config::load(&c.config) {
            most = cfg.limits.link_retry_max_secs;
            let off = working(&c.state).map(|ev| crate::hosts::idle_off(&cfg, &ev));
            if off.is_ok_and(|off| m.is_none_or(|m| !off.contains(m))) {
                let (heard, why) = dial(&c, &cfg, r, (name, m), clock);
                wait = if heard { 1 } else { wait };
                let c = Ctx { now: clock(), ..c };
                intake::apply(&c, &cfg, r, &mut std::io::sink(), (name, m), (Err(why), true)).ok();
            }
        }
        crate::clock::sleep(std::time::Duration::from_secs(wait));
        wait = (wait * 2).min(most.max(1));
    }
}

fn dial(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    (name, m): (&str, Option<&str>),
    clock: fn() -> u64,
) -> (bool, anyhow::Error) {
    let mut heard = false;
    let why = (|| -> Result<std::convert::Infallible> {
        let mut w = watch(ctx, cfg, &working(&ctx.state)?, (name, m));
        let args = crate::runner::argv(&["hosts", "watch"]);
        let (prog, args) = match m {
            Some(_) => crate::hosts::On::new(r, m).wrap(&exe(ctx, name), &args),
            None => (ctx.exe.clone(), args),
        };
        let (mut tx, rx) = r.pipe(&prog, &args)?;
        let (got, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let rx = std::io::BufRead::lines(std::io::BufReader::new(rx));
            rx.map_while(Result::ok).try_for_each(|l| got.send(l))
        });
        let mut send = |text: &str| -> Result<()> { Ok(writeln!(tx, "{text}").and_then(|()| tx.flush())?) };
        send(&serde_json::to_string(&w)?)?;
        let stale = std::time::Duration::from_secs(cfg.limits.link_stale_secs);
        let mut at: Option<(String, u64)> = None;
        loop {
            let line = lines.recv_timeout(stale).map_err(|e| match e {
                std::sync::mpsc::RecvTimeoutError::Timeout => anyhow::anyhow!("no frame for {} s", stale.as_secs()),
                std::sync::mpsc::RecvTimeoutError::Disconnected => anyhow::anyhow!("the link closed"),
            })?;
            let f: Frame = serde_json::from_str(&line).context("a line that is no frame")?;
            let report = matches!(f.body, frame::Body::Report(_));
            let next = at.as_ref().map_or(report, |(b, n)| *b == f.boot && f.n == n + 1);
            anyhow::ensure!(next, "frame {} of boot {} came out of order", f.n, f.boot);
            at = Some((f.boot.clone(), f.n));
            heard = true;
            let c = Ctx {
                now: clock(),
                ..ctx.clone()
            };
            for ack in intake::apply(&c, cfg, r, &mut std::io::sink(), (name, m), (Ok(f), true))? {
                send(&ack)?;
            }
            if report {
                let mut seen = view::View::load(&c, cfg)?.sessions();
                seen.0.retain(|h| h.0.as_deref() == m);
                crate::session::stamp::observe(&c, cfg, r, &seen)?;
                let now = watch(&c, cfg, &working(&c.state)?, (name, m));
                if now != w {
                    send(&serde_json::to_string(&now)?)?;
                    w = now;
                }
            }
        }
    })();
    let Err(why) = why;
    (heard, why)
}

/// Every host's sessions from `fresh`'s view (none: the database did not answer).
pub fn sessions(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> crate::session::Sessions {
    fresh(ctx, cfg, r).map(|v| v.sessions()).unwrap_or_default()
}

/// The view, every host with no fresh word polled first (a `drover` command's start; never serve's).
pub fn fresh(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> Result<view::View> {
    look(ctx, cfg, r, &mut std::io::sink(), false)
}

/// File `path` of spawn `sp`'s host: its verdict.md from the view when it has it, else read there.
pub fn read(ctx: &Ctx, cfg: &Config, r: &dyn Runner, sp: Option<&Event>, path: &str) -> Option<String> {
    let wt = sp.and_then(|e| e.str("worktree"));
    let named = wt.is_some_and(|wt| std::path::Path::new(wt).join("verdict.md") == std::path::Path::new(path));
    let seen = (sp.filter(|_| named)).and_then(|sp| {
        let v = view::View::load(ctx, cfg).ok()?;
        Some(v.worktree(sp)?.verdict.as_ref()?.text.clone())
    });
    seen.or_else(|| crate::hosts::On::of(r, sp).read(std::path::Path::new(path)))
}
