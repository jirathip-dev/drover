//! #254 cloud hosts (docs/hosts.md): a `[hosts.<n>] cloud` recipe's commands start a stopped host on demand, stop it
//! idle or at its spend limit (lane work saved to wip first) and price its running time; logged `cloud-*`.
use crate::config::{expand, write_atomic, Config};
use crate::events::{age, emit, read, Event};
use crate::runner::{argv, Runner};
use crate::Ctx;
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const DAY: u64 = 86_400;

/// Host `name`'s recipe (`<config>/recipes/<cloud>.toml`, else the built-in `boat`) and size.
fn recipe(ctx: &Ctx, cfg: &Config, name: &str) -> Result<(toml::Table, String)> {
    let (c, size) = (&cfg.hosts[name].cloud, cfg.hosts[name].size.clone().unwrap_or_default());
    let p = ctx
        .config
        .with_file_name(format!("recipes/{}.toml", c.as_deref().unwrap_or_default()));
    let text = std::fs::read_to_string(&p).or_else(|e| match c.as_deref() {
        Some("boat") => Ok(include_str!("../assets/recipes/boat.toml").into()),
        _ => Err(e).with_context(|| format!("cloud recipe {}", p.display())),
    })?;
    Ok((toml::from_str(&text)?, size))
}

/// Run host `name`'s recipe command `verb`, the provider key exported from stdin (never in argv or a log); its stdout.
fn cmd(ctx: &Ctx, cfg: &Config, r: &dyn Runner, name: &str, verb: &str) -> Result<String> {
    let (rc, size) = recipe(ctx, cfg, name)?;
    let s = |k: &str| rc.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let (var, line) = (s("key_env"), s(verb).replace("{name}", name).replace("{size}", &size));
    let file = || std::fs::read_to_string(expand(cfg.accounts.accounts_env.as_deref()?)).ok();
    let key = std::env::var(&var)
        .ok()
        .or_else(|| crate::usage::env_value(&file()?, &var));
    let key = key
        .filter(|k| !k.is_empty())
        .with_context(|| format!("{var} not set (env or accounts_env)"))?;
    let script = format!("IFS= read -r K; export {var}=\"$K\"; unset K; {line}");
    let o = r.run_in("sh", &argv(&["-c", &script]), &format!("{key}\n"))?;
    let err = o.stderr.trim().replace(&key, "•••");
    ensure!(o.code == 0, "{name} {verb} failed ({}): {err}", o.code);
    Ok(o.stdout)
}

fn log(ctx: &Ctx, cfg: &Config, kind: &str, name: &str, mut d: Value) -> Result<()> {
    d["host"] = json!(name);
    emit(&ctx.state, cfg, Event::new(ctx.now, "herdsman", None, kind, d))
}

fn last<'a>(events: &'a [Event], name: &str, kinds: &[&str]) -> Option<&'a Event> {
    let hit = |e: &&Event| kinds.contains(&e.kind.as_str()) && e.str("host") == Some(name);
    events.iter().rev().find(hit)
}

/// running | starting | stopped, from the last start, up or stop.
pub fn state(events: &[Event], name: &str) -> &'static str {
    match last(events, name, &["cloud-start", "cloud-up", "cloud-stop"]).map(|e| e.kind.as_str()) {
        Some("cloud-start") => "starting",
        Some("cloud-up") => "running",
        _ => "stopped",
    }
}

/// Host `name`'s spend this (UTC) month in USD and its running hours today: each `cloud-start` → `cloud-stop` span
/// (an open one up to now) × the size's `rate_usd_hour`.
pub fn spend(ctx: &Ctx, cfg: &Config, events: &[Event], name: &str) -> Result<(f64, f64)> {
    let (rc, size) = recipe(ctx, cfg, name)?;
    let rate = rc.get("rate_usd_hour").and_then(|t| t.get(&size)?.as_float());
    let rate = rate.with_context(|| format!("no rate_usd_hour for size {size}"))?;
    // The 0-based day of the month, from the civil calendar of the days since 1970 (H. Hinnant).
    let doe = (ctx.now / DAY + 719_468) % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let (today, dom) = (ctx.now - ctx.now % DAY, doy - (153 * ((5 * doy + 2) / 153) + 2) / 5);
    let hours = |from: u64| {
        let (mut on, mut t) = (None, 0);
        for e in events.iter().filter(|e| e.str("host") == Some(name)) {
            match e.kind.as_str() {
                "cloud-start" => on = on.or(Some(e.ts)),
                "cloud-stop" => t += on.take().map_or(0, |s: u64| e.ts.saturating_sub(s.max(from))),
                _ => {}
            }
        }
        let t = t + on.map_or(0, |s| ctx.now.saturating_sub(s.max(from)));
        f64::from(u32::try_from(t).unwrap_or(u32::MAX)) / 3600.0
    };
    Ok((hours(today - dom * DAY) * rate, hours(today)))
}

/// Why host `name` may not run now: its month's budget (none set: 0) or today's `max_hours_day` used up.
pub fn over(ctx: &Ctx, cfg: &Config, events: &[Event], name: &str) -> Option<String> {
    let h = &cfg.hosts[name];
    let (usd, hrs) = match spend(ctx, cfg, events, name) {
        Ok(s) => s,
        Err(e) => return Some(format!("{e:#}")),
    };
    let (b, m) = (h.budget_usd_month.unwrap_or(0.0), h.max_hours_day.filter(|m| hrs >= *m));
    let b = (usd >= b).then(|| format!("spent ${usd:.2} of ${b} this month"));
    b.or(m.map(|m| format!("{hrs:.1} h today of {m}")))
}

/// What keeps placement off cloud host `name` as it is: `CLOUD_BUDGET` at a limit, else `CLOUD_STOPPED` (startable).
pub fn held(ctx: &Ctx, cfg: &Config, name: &str) -> Option<(&'static str, String)> {
    cfg.hosts.get(name)?.cloud.as_ref()?;
    let events = read(&ctx.state).unwrap_or_default();
    let stopped = || (state(&events, name) == "stopped").then(|| ("CLOUD_STOPPED", "stopped".into()));
    over(ctx, cfg, &events, name)
        .map(|w| ("CLOUD_BUDGET", w))
        .or_else(stopped)
}

/// Start host `name`, wait (5 min at most) for `status` to print `running`, then refresh its address; a failure stops it.
pub fn start(ctx: &Ctx, cfg: &Config, r: &dyn Runner, name: &str) -> Result<()> {
    log(ctx, cfg, "cloud-start", name, json!({}))?;
    let up = cmd(ctx, cfg, r, name, "start").and_then(|_| {
        for _ in 0..30 {
            if cmd(ctx, cfg, r, name, "status")?.trim() == "running" {
                return refresh(ctx, cfg, r, name);
            }
            r.sleep(10);
        }
        bail!("{name} not running after 5 min")
    });
    if let Err(e) = &up {
        let _ = cmd(ctx, cfg, r, name, "stop");
        log(
            ctx,
            cfg,
            "cloud-stop",
            name,
            json!({"why": format!("start failed: {e:#}")}),
        )?;
    }
    up
}

/// Host `name`'s block in `~/.ssh/config.d/herdsman` (an `Include`d file) and its own `known_hosts`, rewritten from the
/// recipe's `address` (`user@host port`, then the host key); logged `cloud-up`.
fn refresh(ctx: &Ctx, cfg: &Config, r: &dyn Runner, name: &str) -> Result<()> {
    let out = cmd(ctx, cfg, r, name, "address")?;
    let (mut l, bad) = (out.lines().map(str::trim), || {
        format!("{name} address: not `user@host port` + key")
    });
    let (dest, port) = l.next().and_then(|a| a.split_once(' ')).with_context(bad)?;
    let ((user, host), key) = (dest.split_once('@').with_context(bad)?, l.next().with_context(bad)?);
    let (dir, at) = (
        ctx.home.join(".ssh/config.d"),
        [format!("[{host}]:{port}"), host.into()],
    );
    let known = dir.join(format!("herdsman-{name}.known"));
    std::fs::create_dir_all(&dir)?;
    write_atomic(&known, &format!("{} {key}\n", at[usize::from(port == "22")]))?;
    let (a, b, k) = (
        format!("# herdsman {name}\n"),
        format!("# end {name}\n"),
        known.display(),
    );
    let alias = cfg.hosts[name].machine.as_deref().unwrap_or(name);
    let block =
        format!("{a}Host {alias}\n  HostName {host}\n  User {user}\n  Port {port}\n  UserKnownHostsFile {k}\n{b}");
    let p = dir.join("herdsman");
    let old = std::fs::read_to_string(&p).unwrap_or_default();
    let rest = match (old.find(&a), old.find(&b)) {
        (Some(i), Some(j)) if i < j => format!("{}{}", &old[..i], &old[j + b.len()..]),
        _ => old,
    };
    write_atomic(&p, &(rest + &block))?;
    log(ctx, cfg, "cloud-up", name, json!({"address": format!("{dest} {port}")}))
}

/// Push each lane worktree on host `name` with unpushed or dirty work to `wip/<host>-<lane>` (a secret or a failed
/// push aborts, logged `cloud-abort`), then run the recipe's `verb` (`stop` | `destroy`); the wip branches pushed.
pub fn stop(ctx: &Ctx, cfg: &Config, r: &dyn Runner, name: &str, verb: &str, why: &str) -> Result<Vec<String>> {
    let (events, mut wip) = (read(&ctx.state)?, vec![]);
    let on = crate::hosts::On::new(r, cfg.hosts.get(name).context("no such host")?.machine.as_deref());
    let spawned = events
        .iter()
        .filter(|e| e.kind == "spawn" && e.str("host") == Some(name));
    let lanes: BTreeMap<_, _> = spawned
        .filter_map(|e| Some((e.str("agent")?, e.str("worktree")?)))
        .collect();
    for (lane, wt) in lanes.into_iter().filter(|l| on.exists(std::path::Path::new(l.1))) {
        let w = format!("wip/{name}-{lane}");
        match crate::check::save_wip(&on, wt, &w, &format!("wip: cloud host {verb}")) {
            Ok(saved) => wip.extend(saved.then_some(w)),
            Err(e) => {
                log(
                    ctx,
                    cfg,
                    "cloud-abort",
                    name,
                    json!({"lane": lane, "why": format!("{e:#}")}),
                )?;
                return Err(e.context(format!("{name}: {verb} aborted, {lane}'s work not saved")));
            }
        }
    }
    cmd(ctx, cfg, r, name, verb)?;
    log(
        ctx,
        cfg,
        "cloud-stop",
        name,
        json!({"verb": verb, "why": why, "wip": wip}),
    )?;
    Ok(wip)
}

/// `check`: stop each running cloud host at its spend limit, or once it has had no live agent for `idle_stop_minutes`.
pub fn tick(ctx: &Ctx, cfg: &Config, r: &dyn Runner, live: &BTreeMap<String, Option<Vec<Value>>>) -> Vec<String> {
    let (events, seen, mut out) = (read(&ctx.state).unwrap_or_default(), crate::hosts::cached(ctx), vec![]);
    for (name, h) in cfg.hosts.iter().filter(|h| h.1.cloud.is_some()) {
        let Some(Some(agents)) = live.get(name).filter(|_| state(&events, name) == "running") else {
            continue;
        };
        let (idle, up) = (
            h.idle_stop_minutes.unwrap_or(20) * 60,
            last(&events, name, &["cloud-up"]),
        );
        let busy = seen.get(name).map_or(0, |s| s.busy).max(up.map_or(0, |e| e.ts));
        let quiet = (agents.is_empty() && busy + idle <= ctx.now).then(|| format!("idle {} min", idle / 60));
        if let Some(why) = over(ctx, cfg, &events, name).or(quiet) {
            let res = stop(ctx, cfg, r, name, "stop", &why);
            out.push(res.map_or_else(|e| format!("{e:#}"), |w| format!("{name}: stopped ({why}); wip {w:?}")));
        }
    }
    out
}

/// Cloud host `name`'s Host card: its head words unless running, and the `cloud` row's tone (amber from 80 % of a
/// limit, red at it) and text: `boat small · spent $12 of $50 · 3.1 h today of 6 · address 5m ago · last start …`.
pub type Card = (Option<String>, &'static str, String);
pub fn card(ctx: &Ctx, cfg: &Config, events: &[Event], (name, lanes): (&str, usize)) -> Option<Card> {
    let h = cfg.hosts.get(name).filter(|h| h.cloud.is_some())?;
    let head = match state(events, name) {
        "running" => None,
        "starting" => Some("starting".into()),
        _ => Some(format!("stopped · {lanes} lane(s) paused: cloud host stopped")),
    };
    let (b, m) = (h.budget_usd_month.unwrap_or(0.0), h.max_hours_day.unwrap_or(24.0));
    let (usd, hrs) = spend(ctx, cfg, events, name).unwrap_or((f64::INFINITY, 0.0));
    let at = usize::from((usd / b).max(hrs / m) >= 0.8) + usize::from(over(ctx, cfg, events, name).is_some());
    let ago = |k| last(events, name, &[k]).map_or("never".into(), |e| format!("{} ago", age(ctx.now - e.ts)));
    let (rc, size) = (h.cloud.as_deref().unwrap_or_default(), h.size.as_deref().unwrap_or("?"));
    let (a, s, t) = (ago("cloud-up"), ago("cloud-start"), ago("cloud-stop"));
    let text =
        format!("{rc} {size} · spent ${usd:.0} of ${b} · {hrs:.1} h today of {m} · address {a} · last start {s}");
    Some((head, ["g", "a", "r"][at.min(2)], format!("{text} · last stop {t}")))
}
