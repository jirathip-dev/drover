//! #254 cloud hosts (docs/hosts.md): recipe commands start, stop and price a `[hosts.<n>] cloud` host; logs `cloud-*`.
use crate::config::{expand, write_atomic, Config};
use crate::events::{age, emit, read, Event};
use crate::runner::{argv, Runner};
use crate::Ctx;
use anyhow::{anyhow, bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Host `name`'s recipe (`<config>/recipes/<cloud>.toml`, else the built-in `boat`) and size.
fn recipe(ctx: &Ctx, cfg: &Config, name: &str) -> Result<(toml::Table, String)> {
    let (c, size) = (&cfg.hosts[name].cloud, cfg.hosts[name].size.clone().unwrap_or_default());
    let file = format!("recipes/{}.toml", c.as_deref().unwrap_or(""));
    let p = ctx.config.with_file_name(file);
    let boat = || (c.as_deref() == Some("boat")).then(|| include_str!("../assets/recipes/boat.toml").into());
    let text = std::fs::read_to_string(&p).ok().or_else(boat);
    let text = text.with_context(|| format!("cloud recipe {}", p.display()))?;
    Ok((toml::from_str(&text)?, size))
}

/// Run host `name`'s recipe command `verb`, the provider key exported from stdin (never in argv or a log); its stdout.
fn cmd(ctx: &Ctx, cfg: &Config, r: &dyn Runner, name: &str, verb: &str) -> Result<String> {
    let (rc, size) = recipe(ctx, cfg, name)?;
    let s = |k: &str| rc.get(k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
    let (var, line) = (s("key_env"), s(verb).replace("{name}", name).replace("{size}", &size));
    let file = || std::fs::read_to_string(expand(cfg.accounts.accounts_env.as_deref()?)).ok();
    let env = || crate::usage::env_value(&file()?, &var);
    let key = std::env::var(&var).ok().or_else(env).unwrap_or_default();
    ensure!(!key.is_empty(), "{var} not set (env or accounts_env)");
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

/// Host `name`'s USD this UTC month and hours today: its `cloud-start` → `cloud-stop` spans × `rate_usd_hour`.
pub fn spend(ctx: &Ctx, cfg: &Config, events: &[Event], name: &str) -> Result<(f64, f64)> {
    let (rc, size) = recipe(ctx, cfg, name)?;
    let rate = rc.get("rate_usd_hour").and_then(|t| t.get(&size)?.as_float());
    let rate = rate.with_context(|| format!("no rate_usd_hour for size {size}"))?;
    // The 0-based day of the month, from the civil calendar of the days since 1970 (H. Hinnant).
    let doe = (ctx.now / 86_400 + 719_468) % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let (today, dom) = (ctx.now - ctx.now % 86_400, doy - (153 * ((5 * doy + 2) / 153) + 2) / 5);
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
    Ok((hours(today - dom * 86_400) * rate, hours(today)))
}

/// Why host `name` may not run now: its month's budget (none set: 0) or today's `max_hours_day` used up.
pub fn over(ctx: &Ctx, cfg: &Config, events: &[Event], name: &str) -> Option<String> {
    let h = &cfg.hosts[name];
    let (b, m) = (h.budget_usd_month.unwrap_or(0.0), h.max_hours_day);
    let month = |usd: f64| (usd >= b).then(|| format!("spent ${usd:.2} of ${b} this month"));
    let day = |hrs: f64| m.filter(|m| hrs >= *m).map(|m| format!("{hrs:.1} h today of {m}"));
    spend(ctx, cfg, events, name).map_or_else(|e| Some(format!("{e:#}")), |(u, h)| month(u).or_else(|| day(h)))
}

/// What keeps placement off cloud host `name` as it is: `CLOUD_BUDGET` at a limit, else `CLOUD_STOPPED` (startable).
pub fn held(ctx: &Ctx, cfg: &Config, name: &str) -> Option<(&'static str, String)> {
    cfg.hosts.get(name)?.cloud.as_ref()?;
    let events = read(&ctx.state).unwrap_or_default();
    let stopped = || (state(&events, name) == "stopped").then(|| ("CLOUD_STOPPED", "stopped".into()));
    let budget = over(ctx, cfg, &events, name).map(|w| ("CLOUD_BUDGET", w));
    budget.or_else(stopped)
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
        let d = json!({"why": format!("start failed: {e:#}")});
        log(ctx, cfg, "cloud-stop", name, d)?;
    }
    up
}

/// Rewrite host `name`'s block in `~/.ssh/config.d/herdsman` and its `known_hosts` from `address`; logs `cloud-up`.
fn refresh(ctx: &Ctx, cfg: &Config, r: &dyn Runner, name: &str) -> Result<()> {
    let out = cmd(ctx, cfg, r, name, "address")?;
    let bad = || format!("{name} address: not `user@host port` + key");
    let mut l = out.lines().map(str::trim);
    let (dest, port) = l.next().and_then(|a| a.split_once(' ')).with_context(bad)?;
    let ((user, host), key) = (dest.split_once('@').with_context(bad)?, l.next().with_context(bad)?);
    // The reply goes into ssh config verbatim: a numeric port and no whitespace, so it can't add a directive.
    ensure!(
        port.parse::<u16>().is_ok() && !dest.contains(char::is_whitespace),
        bad()
    );
    let dir = ctx.home.join(".ssh/config.d");
    let at = [format!("[{host}]:{port}"), host.into()];
    let known = dir.join(format!("herdsman-{name}.known"));
    std::fs::create_dir_all(&dir)?;
    write_atomic(&known, &format!("{} {key}\n", at[usize::from(port == "22")]))?;
    let (a, b) = (format!("# herdsman {name}\n"), format!("# end {name}\n"));
    let m = cfg.hosts[name].machine.as_deref().unwrap_or(name);
    let k = known.display();
    let s = format!("{a}Host {m}\n  HostName {host}\n  User {user}\n  Port {port}\n  UserKnownHostsFile {k}\n{b}");
    let p = dir.join("herdsman");
    let old = std::fs::read_to_string(&p).unwrap_or_default();
    let (keep, tail) = old.split_once(&a).unwrap_or((&old, ""));
    // A block whose end marker is lost ends at the next block, never swallowing it.
    let next = || tail.find("# herdsman ").map_or("", |i| &tail[i..]);
    let rest = keep.to_string() + tail.split_once(&b).map_or_else(next, |x| x.1);
    write_atomic(&p, &(rest + &s))?;
    log(ctx, cfg, "cloud-up", name, json!({"address": format!("{dest} {port}")}))
}

/// Save host `name`'s lane work to `wip/<host>-<lane>` (a failed push or unreachable host: `cloud-abort`), then `verb`.
pub fn stop(ctx: &Ctx, cfg: &Config, r: &dyn Runner, name: &str, verb: &str, why: &str) -> Result<Vec<String>> {
    let (events, mut wip) = (read(&ctx.state)?, vec![]);
    let on = crate::hosts::On::new(r, cfg.hosts.get(name).context("no such host")?.machine.as_deref());
    let mut lanes = BTreeMap::new();
    let mine = |e: &&Event| e.kind == "spawn" && e.str("host") == Some(name);
    for e in events.iter().filter(mine) {
        lanes.extend(e.str("agent").zip(e.str("worktree")));
    }
    for (lane, wt) in lanes {
        let w = format!("wip/{name}-{lane}");
        let saved = match on.run("test", &argv(&["-e", wt])).map(|o| o.code) {
            Ok(c) if c == 1 || c == 0 && crate::spawn::dirty(&on, wt.as_ref()).is_none() => continue,
            Ok(0) => crate::check::save_wip(&on, wt, &w, &format!("wip: cloud host {verb}")),
            o => o.and_then(|c| Err(anyhow!("can't reach {name} to check {wt} (exit {c})"))),
        };
        if let Err(e) = saved {
            let d = json!({"lane": lane, "why": format!("{e:#}")});
            log(ctx, cfg, "cloud-abort", name, d)?;
            return Err(e.context(format!("{name}: {verb} aborted, {lane}'s work not saved")));
        }
        wip.push(w);
    }
    cmd(ctx, cfg, r, name, verb)?;
    let d = json!({"verb": verb, "why": why, "wip": wip});
    log(ctx, cfg, "cloud-stop", name, d)?;
    Ok(wip)
}

/// `check`: stop each running cloud host at its spend limit, or once it has had no live agent for `idle_stop_minutes`.
pub type Live = BTreeMap<String, Option<Vec<Value>>>;
pub fn tick(ctx: &Ctx, cfg: &Config, r: &dyn Runner, live: &Live, out: &mut dyn std::io::Write) -> Result<()> {
    let (events, seen) = (read(&ctx.state).unwrap_or_default(), crate::hosts::cached(ctx));
    for (name, h) in cfg.hosts.iter().filter(|h| h.1.cloud.is_some()) {
        let Some(Some(agents)) = live.get(name).filter(|_| state(&events, name) == "running") else {
            continue;
        };
        let idle = h.idle_stop_minutes.unwrap_or(20) * 60;
        let up = last(&events, name, &["cloud-up"]);
        let busy = seen.get(name).map_or(0, |s| s.busy).max(up.map_or(0, |e| e.ts));
        let quiet = (agents.is_empty() && busy + idle <= ctx.now).then(|| format!("idle {} min", idle / 60));
        if let Some(why) = over(ctx, cfg, &events, name).or(quiet) {
            let res = stop(ctx, cfg, r, name, "stop", &why);
            let said = res.map_or_else(|e| format!("{e:#}"), |w| format!("{name}: stopped ({why}); wip {w:?}"));
            writeln!(out, "cloud {said}")?;
        }
    }
    Ok(())
}

/// Host card: head words (none when running); the `cloud` row's tone (amber from 80 %, red at a limit) and text.
pub fn card(ctx: &Ctx, cfg: &Config, ev: &[Event], (name, n): (&str, usize)) -> Option<[String; 3]> {
    let h = cfg.hosts.get(name).filter(|h| h.cloud.is_some())?;
    let st = state(ev, name);
    let paused = format!("stopped · {n} lane(s) paused: cloud host stopped");
    let head = [st.replace("running", ""), paused][usize::from(st == "stopped")].clone();
    let (b, m) = (h.budget_usd_month.unwrap_or(0.0), h.max_hours_day.unwrap_or(24.0));
    let (usd, hrs) = spend(ctx, cfg, ev, name).unwrap_or((f64::INFINITY, 0.0));
    let at = usize::from((usd / b).max(hrs / m) >= 0.8) + usize::from(over(ctx, cfg, ev, name).is_some());
    let ago = |k| last(ev, name, &[k]).map_or("never".into(), |e| format!("{} ago", age(ctx.now - e.ts)));
    let (rc, size) = (h.cloud.as_deref().unwrap_or_default(), h.size.as_deref().unwrap_or("?"));
    let (a, s, t) = (ago("cloud-up"), ago("cloud-start"), ago("cloud-stop"));
    let text =
        format!("{rc} {size} · spent ${usd:.0} of ${b} · {hrs:.1} h today of {m} · address {a} · last start {s}");
    let tone = ["g", "a", "r"][at.min(2)].to_string();
    Some([head, tone, format!("{text} · last stop {t}")])
}
