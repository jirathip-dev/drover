//! What a lane may use: its account (first uncapped of an ordered list; tokens never read out) and MCP servers.
use crate::config::{Config, Fleet, Role};
use crate::events::{emit, fold, last_spawn, read, Event};
use crate::gate::refused;
use crate::harness::Credential;
use crate::runner::Runner;
use crate::usage;
use crate::Ctx;
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::Write;

/// When the account's (#302: or account-less harness's) latest cap or rest, since any reset, ends: its logged
/// `until`, else `cap_reset_minutes` after it.
pub fn capped_until(cfg: &Config, events: &[Event], name: &str, now: u64) -> Option<u64> {
    let kinds = ["account-capped", "account-reset", "agent-unavailable"];
    let mine = |e: &&Event| kinds.contains(&e.kind.as_str()) && e.str("account").or(e.str("harness")) == Some(name);
    let last = events.iter().rev().find(mine)?;
    let until = last.data.get("until").and_then(Value::as_u64);
    let until = until.unwrap_or(last.ts + cfg.accounts.cap_reset_minutes * 60);
    (last.kind != "account-reset" && until > now).then_some(until)
}

/// #302: role `r` on the first of its `fallback` entries (`harness[:account]`) whose harness and account both are not
/// resting (the earlier ones that are, for the card); `r` itself when it has none.
pub fn fallback(cfg: &Config, events: &[Event], r: &Role, now: u64) -> Result<(Role, Option<(String, u64)>)> {
    let rest = |e: &&String| e.split(':').filter_map(|k| capped_until(cfg, events, k, now)).max();
    if r.fallback.is_empty() {
        return Ok((r.clone(), None));
    }
    let Some(i) = r.fallback.iter().position(|e| rest(&e).is_none()) else {
        return refused("ALL_CAPPED", format!("every fallback {:?} is resting", r.fallback));
    };
    let (h, a) = r.fallback[i].split_once(':').unwrap_or((&r.fallback[i], ""));
    let mut to = r.clone();
    (to.account, to.accounts) = (Some(a.to_string()).filter(|a| !a.is_empty()), vec![]);
    if to.harness != h {
        (to.harness, to.model, to.effort) = (h.into(), None, None);
    }
    let (resting, entry) = (r.fallback[..i].join(" "), &r.fallback[i]);
    let until = r.fallback[..i].iter().filter_map(|e| rest(&e)).max();
    Ok((to, until.map(|t| (format!("on {entry} ({resting} resting until"), t))))
}

/// The role's first uncapped account (the lowest weekly usage when every one was read in the last 30 min) and its
/// token's secret name (#441: `spawn::deliver` reads it from the store); None unless the role's harness runs on an
/// `[accounts]` token (`Credential::Token`).
pub fn pick(ctx: &Ctx, cfg: &Config, events: &[Event], role: &Role) -> Result<Option<(String, String)>> {
    let now = ctx.now;
    let mut names = role.account_list();
    let Credential::Token(_) = crate::harness::get(&role.harness).credential() else {
        return Ok(None);
    };
    if names.is_empty() {
        return Ok(None);
    }
    let week = |n: &String| {
        crate::usage::cached(ctx, n)
            .filter(|m| m.ts + 1800 > now)
            .and_then(|m| m.week)
            .map(|w| w.0)
    };
    let free: Vec<&String> = names
        .iter()
        .filter(|n| capped_until(cfg, events, n, now).is_none())
        .collect();
    // Headroom: the lowest live 5h/7d use first; an account never read goes last.
    let used = |n: &String| {
        let m = usage::reading(ctx, n, || usage::cached(ctx, n));
        let live = m.map(|m| [m.session, m.week].into_iter().flatten().filter(|w| w.1 > now));
        live.map_or(f64::INFINITY, |w| w.map(|w| w.0).fold(0.0, f64::max))
    };
    if cfg.accounts.pick.as_deref() == Some("headroom") {
        names.sort_by(|a, b| used(a).total_cmp(&used(b)));
    } else if let Some(mut w) = free
        .iter()
        .map(|n| week(n).map(|w| (w, (*n).clone())))
        .collect::<Option<Vec<_>>>()
    {
        w.sort_by(|a, b| a.0.total_cmp(&b.0));
        names = w.into_iter().map(|w| w.1).collect();
    }
    let Some(name) = names.iter().find(|n| capped_until(cfg, events, n, now).is_none()) else {
        let all = role.account_list();
        let first = all.iter().filter_map(|n| capped_until(cfg, events, n, now)).min();
        let at = crate::events::age(first.unwrap_or(now).saturating_sub(now));
        let why = format!("every account {all:?} is capped; the first resets in {at}");
        return refused("ALL_CAPPED", why);
    };
    Ok(Some((name.clone(), cfg.accounts.names[name].clone())))
}

/// The banner's `resets 9am` / `resets Nov 4, 8am` (host local time); else `cap_reset_minutes` on (a day if weekly).
pub fn cap_until(cfg: &Config, r: &dyn Runner, screen: &str, now: u64) -> u64 {
    let s = screen.to_lowercase().replace([',', '(', '.'], " ");
    let tail = &s[s.rfind("reset").unwrap_or(s.len())..];
    let words: Vec<&str> = tail.split_whitespace().skip(1).collect();
    let date = words.first().zip(words.get(1)).and_then(|(m, d)| month_day(m, d));
    let secs = words.iter().find_map(|w| clock(w)).or(date.map(|_| 0));
    let off = local_offset(r, now);
    let today = now.saturating_add_signed(off) / 86400;
    let at = |secs: u64| {
        (today..today + 400)
            .filter(|&d| date.is_none_or(|md| md == civil_month_day(d)))
            .map(|d| (d * 86400 + secs).saturating_add_signed(-off))
            .find(|&t| t > now)
    };
    let session = cfg.accounts.cap_reset_minutes * 60;
    let fallback = if s.contains("week") { 86400 } else { session };
    secs.and_then(at).unwrap_or(now + fallback)
}

/// Seconds past midnight of `3pm` / `3:30pm`.
fn clock(word: &str) -> Option<u64> {
    let hm = word.strip_suffix("am").or(word.strip_suffix("pm"))?;
    let (h, m) = hm.split_once(':').unwrap_or((hm, "0"));
    let pm = if word.ends_with("pm") { 12 } else { 0 };
    Some((h.parse::<u64>().ok()? % 12 + pm) * 3600 + m.parse::<u64>().ok()? * 60)
}

/// `(month 1-12, day)` of `nov 4`.
fn month_day(month: &str, day: &str) -> Option<(u64, u64)> {
    let months = "jan feb mar apr may jun jul aug sep oct nov dec".split(' ');
    let (_, m) = months.zip(1..).find(|(name, _)| month.starts_with(name))?;
    Some((m, day.parse().ok()?))
}

/// `(month 1-12, day)` of a day number since 1970-01-01 (Hinnant's `civil_from_days`).
pub fn civil_month_day(days: u64) -> (u64, u64) {
    let doe = (days + 719_468) % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    ((mp + 2) % 12 + 1, doy - (153 * mp + 2) / 5 + 1)
}

/// #343: the year of a day number since 1970-01-01 (as `civil_month_day`).
pub fn civil_year(days: u64) -> u64 {
    let (z, (m, _)) = (days + 719_468, civil_month_day(days));
    let doe = z % 146_097;
    z / 146_097 * 400 + (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365 + u64::from(m <= 2)
}

/// The host's UTC offset in seconds (`date +%z`; UTC if unreadable).
fn local_offset(r: &dyn Runner, now: u64) -> i64 {
    let z = usage::date(r, now, "+%z").parse::<i64>().unwrap_or(0);
    z / 100 * 3600 + z % 100 * 60
}

/// The screen holds one of `pat`'s `|`-separated, case-insensitive substrings (a usage-limit banner).
pub fn banner(pat: &str, screen: &str) -> bool {
    let (screen, pat) = (screen.to_lowercase(), pat.to_lowercase());
    pat.split('|').any(|p| !p.is_empty() && screen.contains(p))
}

/// `herdsman accounts`: each account's state and the open lanes on it. Names only, never tokens.
pub fn list(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write) -> Result<()> {
    let events = read(&ctx.state)?;
    let states = fold(&events);
    // Latest spawn per agent, kept while its issue (if any) is not merged.
    let mut on: BTreeMap<&str, &str> = BTreeMap::new();
    for e in events.iter().filter(|e| e.kind == "spawn") {
        if let Some(a) = e.str("agent") {
            on.insert(a, e.str("account").unwrap_or_default());
        }
    }
    let merged = |e: &Event| {
        e.issue
            .is_some_and(|n| states.get(&(e.fleet.clone(), n)).is_some_and(|s| s.state == "merged"))
    };
    let open = |a: &str| last_spawn(&events, a).is_some_and(|e| !merged(e));
    if cfg.accounts.names.is_empty() {
        writeln!(out, "no [accounts] configured")?;
    }
    for name in cfg.accounts.names.keys() {
        let lanes: Vec<&str> = on
            .iter()
            .filter(|(a, n)| *n == name && open(a))
            .map(|(a, _)| *a)
            .collect();
        writeln!(
            out,
            "{name:<8} {:<28} {} lanes {}",
            cap_line(cfg, r, &events, name, ctx.now),
            lanes.len(),
            lanes.join(" ")
        )?;
        if let Some(m) = usage::reading(ctx, name, || usage::meter(ctx, cfg, r, name)) {
            let login = cfg.accounts.usage.get(name).map_or("", String::as_str);
            writeln!(out, "{:<8} {}", "", usage::meter_line(r, &m, login, ctx.now))?;
        }
        writeln!(out, "{:<8} {}", "", usage::lane_hours(&events, name, ctx.now))?;
    }
    match usage::deepseek(ctx, cfg, r) {
        Some(Ok((b, low))) => writeln!(
            out,
            "deepseek · {} {:.2} · {}",
            b.currency,
            b.total,
            if low { "low" } else { "ok" }
        )?,
        Some(Err(hint)) => writeln!(out, "deepseek · {hint}")?,
        None => {}
    }
    Ok(())
}

/// `herdsman accounts reset <name>`: spawns may use a capped account again.
pub fn reset(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, name: &str) -> Result<()> {
    anyhow::ensure!(cfg.accounts.names.contains_key(name), "unknown account '{name}'");
    let events = read(&ctx.state)?;
    if capped_until(cfg, &events, name, ctx.now).is_none() {
        writeln!(out, "{name} is not capped")?;
        return Ok(());
    }
    let cap = events
        .iter()
        .rev()
        .find(|e| e.kind == "account-capped" && e.str("account") == Some(name))
        .unwrap();
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, &cap.fleet, None, "account-reset", json!({"account": name})),
    )?;
    writeln!(out, "{name}: reset; spawns use it again")?;
    Ok(())
}

/// `capped until Nov 4 08:00` while the account's latest cap holds, else `ok`.
pub fn cap_line(cfg: &Config, r: &dyn Runner, events: &[Event], name: &str, now: u64) -> String {
    let until = |t| usage::date(r, t, "+%b %-d %H:%M");
    capped_until(cfg, events, name, now).map_or("ok".into(), |t| format!("capped until {}", until(t)))
}

/// `*`-wildcard match on lowercased bytes (callers lowercase both sides).
fn glob(g: &[u8], s: &[u8]) -> bool {
    match g {
        [] => s.is_empty(),
        [b'*', rest @ ..] => (0..=s.len()).any(|i| glob(rest, &s[i..])),
        [c, rest @ ..] => s.first() == Some(c) && glob(rest, &s[1..]),
    }
}

/// The role's MCP servers, copied by name from the repo's `.mcp.json` (`MCP_FORBIDDEN` if one matches `mcp_deny`).
pub fn mcp_servers(cfg: &Config, f: &Fleet, role: &Role) -> Result<Map<String, Value>> {
    for n in &role.mcp {
        if let Some(d) = cfg
            .mcp_deny
            .iter()
            .find(|d| glob(d.to_lowercase().as_bytes(), n.to_lowercase().as_bytes()))
        {
            return refused(
                "MCP_FORBIDDEN",
                format!("MCP server `{n}` matches mcp_deny `{d}`; lanes never get it"),
            );
        }
    }
    if role.mcp.is_empty() {
        return Ok(Map::new());
    }
    let src = f.local_path().join(".mcp.json");
    let text = std::fs::read_to_string(&src).with_context(|| format!("cannot read {}", src.display()))?;
    let all: Value = serde_json::from_str(&text).with_context(|| format!("{}: not JSON", src.display()))?;
    let get = |n: &String| all["mcpServers"].get(n).cloned().map(|v| (n.clone(), v));
    role.mcp
        .iter()
        .map(|n| get(n).ok_or_else(|| anyhow!("MCP server `{n}` is not in {}", src.display())))
        .collect()
}
