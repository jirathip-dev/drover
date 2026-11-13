//! What a lane may use: its account (first uncapped of an ordered list; tokens never read out) and MCP servers.
use crate::config::{Config, Fleet, Role};
use crate::events::{emit, read, Event};
use crate::gate::refused;
use crate::providers::Credential;
use crate::runner::Runner;
use crate::usage;
use crate::Ctx;
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

/// When the account's (#302: or account-less harness's) latest cap or rest, since any reset, ends: its logged
/// `until`, else `cap_reset_minutes` after it (#640: a sign-out, until `accounts reset`).
pub fn capped_until(cfg: &Config, events: &[Event], name: &str, now: u64) -> Option<u64> {
    let last = latest(events, name)?;
    let out = (last.kind == "account-signed-out").then_some(u64::MAX);
    let until = last.data.get("until").and_then(Value::as_u64).or(out);
    let until = until.unwrap_or(last.ts + cfg.accounts.cap_reset_minutes * 60);
    (last.kind != "account-reset" && until > now).then_some(until)
}

/// An event that rests an account (else harness): a cap, a #302 rest, a #640 sign-out.
pub fn rests(kind: &str) -> bool {
    ["account-capped", "agent-unavailable", "account-signed-out"].contains(&kind)
}

/// The account's (else harness's) latest rest or reset.
fn latest<'a>(events: &'a [Event], name: &str) -> Option<&'a Event> {
    let who = |e: &Event| e.str("account").or(e.str("harness")) == Some(name);
    let mine = |e: &&Event| (rests(&e.kind) || e.kind == "account-reset") && who(e);
    events.iter().rev().find(mine)
}

/// #640: a limit banner on account `a` caps it only when its usage reading agrees: none fresh (10 min) or a
/// window it has at 100% (until the later reset). (kind, until, the reading) for `check`.
pub fn confirm(ctx: &Ctx, a: Option<&str>, until: u64) -> Seen {
    let stale = ["agent-unavailable", "account-capped"][usize::from(a.is_some())];
    let m = a.and_then(|n| usage::row(ctx, n));
    match m.filter(|m| ctx.now < m.ts + 600) {
        Some(m) => {
            let full = [m.session, m.week].into_iter().flatten().filter(|w| w.0 >= 100.0);
            let full = full.map(|w| w.1.max(until)).max();
            let kind = ["cap-banner-ignored", "account-capped"][usize::from(full.is_some())];
            (kind, full, pcts(&m, ctx.now))
        }
        None => (stale, Some(until), "no fresh reading".into()),
    }
}

pub type Seen = (&'static str, Option<u64>, String);

/// `5h —, 7d 23%`: a window the plan lacks is `—`, one past its reset 0% (#785).
fn pcts(m: &usage::Meter, now: u64) -> String {
    let p = |w: usage::Window| w.map_or("—".into(), |w| format!("{:.0}%", if w.1 > now { w.0 } else { 0.0 }));
    format!("5h {}, 7d {}", p(m.session), p(m.week))
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

/// #683: an account's name and the env its lane signs in with (`account_env`, filled in where the lane starts).
pub type Account = (String, BTreeMap<String, String>);

/// The role's first uncapped account (the lowest weekly usage when every one was read in the last 30 min), its env
/// still empty; None unless the role's harness runs on `[accounts]` (`Credential::Account`).
pub fn pick(ctx: &Ctx, cfg: &Config, events: &[Event], role: &Role) -> Result<Option<Account>> {
    let now = ctx.now;
    let mut names = role.account_list();
    let Credential::Account(_) = crate::providers::get(&role.harness).credential() else {
        return Ok(None);
    };
    if names.is_empty() {
        return Ok(None);
    }
    let week = |n: &String| {
        usage::row(ctx, n)
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
        let m = usage::row(ctx, n);
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
    Ok(Some((name.clone(), BTreeMap::new())))
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

/// `drover accounts`: each account's state and the open lanes on it. Names only, never tokens.
pub fn list(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write) -> Result<()> {
    let events = read(&ctx.state)?;
    // #785: the lanes still running (`usage::lane_end`), with their accounts.
    let running = (0..events.len()).filter(|&i| events[i].kind == "spawn" && usage::lane_end(&events, i).is_none());
    let on: Vec<(&str, &str)> = running
        .filter_map(|i| Some((events[i].str("agent")?, events[i].str("account")?)))
        .collect();
    if cfg.accounts.names.is_empty() {
        writeln!(out, "no [accounts] configured")?;
    }
    for name in cfg.accounts.names.keys() {
        let lanes: Vec<&str> = on.iter().filter(|l| l.1 == name).map(|l| l.0).collect();
        let m = usage::row(ctx, name);
        let mut state = cap_line(cfg, r, &events, name, ctx.now);
        if let (Some(m), "ok") = (&m, state.as_str()) {
            state = format!("ok: {}", pcts(m, ctx.now));
        }
        writeln!(out, "{name:<8} {state:<28} {} lanes {}", lanes.len(), lanes.join(" "))?;
        if let Some(m) = m {
            let login = cfg.accounts.usage.get(name).map_or("", String::as_str);
            writeln!(out, "{:<8} {}", "", usage::meter_line(r, &m, login, ctx.now))?;
        }
        writeln!(out, "{:<8} {}", "", usage::lane_hours(&events, name, ctx.now))?;
        let (ok, line) = sign_in(cfg, r, name);
        let shown = if ok {
            line.split(" → ").next().unwrap_or_default()
        } else {
            &line
        };
        writeln!(out, "{:<8} {}{shown}", "", if ok { "" } else { "✗ " })?;
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

/// #683 (q846): the env account `name` signs a lane of `harness` in with, exactly: `[accounts] account_env`'s JSON
/// object of env vars (else refused `ACCOUNT_ENV` with its stderr), or with none set its folder (`Accounts::folder`)
/// as its provider's config folder. Never a token; empty for a harness on no `[accounts]`.
pub fn account_env(cfg: &Config, r: &dyn Runner, harness: &str, name: &str) -> Result<BTreeMap<String, String>> {
    let Credential::Account(acct) = crate::providers::get(harness).credential() else {
        return Ok(BTreeMap::new());
    };
    let Some(cmd) = &cfg.accounts.account_env else {
        if token_file(cfg, harness, name).is_some() {
            return Ok(BTreeMap::new()); // #707: its token goes in the lane's secrets file (`spawn::deliver`)
        }
        let var = acct.config_env.to_string();
        return Ok(BTreeMap::from([(var, cfg.accounts.folder(name).display().to_string())]));
    };
    let fill = |w: &str| w.replace("{provider}", acct.provider).replace("{name}", name);
    let words: Vec<String> = cmd.split_whitespace().map(fill).collect();
    let fail = |why: String| refused("ACCOUNT_ENV", format!("account {name}: `{}` {why}", words.join(" ")));
    let Some((prog, args)) = words.split_first() else {
        return fail("is empty".into());
    };
    let o = match r.run_for(
        prog,
        args,
        "",
        std::time::Duration::from_secs(cfg.limits.access_probe_secs),
    ) {
        Ok(o) => o,
        Err(e) => return fail(format!("did not run: {e:#}")),
    };
    if o.code == 3 {
        let why = format!("account {name} is signed out (`{}` exit 3)", words.join(" "));
        return refused("SIGNED_OUT", why);
    }
    let var = |k: &String| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    let env = serde_json::from_str::<BTreeMap<String, String>>(&o.stdout).ok();
    match env.filter(|e| o.code == 0 && e.keys().all(var)) {
        Some(env) => Ok(env),
        None => fail(format!(
            "exited {} without a JSON object of env vars: {}",
            o.code,
            o.stderr.trim()
        )),
    }
}

/// #785: `p`'s account with its env and token, signed in: one signed out (`account_env` exit 3, else its CLI's status
/// command) rests `account-signed-out` (one owner alert) for the role's next. A dry run asks no CLI, logs nothing.
pub fn signed(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    (fleet, role, dry): (&str, &Role, bool),
    p: &mut crate::spawn::Plan,
) -> Result<()> {
    let (h, mut a) = (crate::providers::get(&role.harness), p.account.take());
    let ask = !dry && cfg.accounts.account_env.is_none();
    while let Some((name, env)) = a.as_mut() {
        let got = account_env(cfg, r, &role.harness, name);
        let out = match &got {
            Ok(got) => folder(&role.harness, got)
                .filter(|f| ask && h.signed_in(r, Path::new(f.1)) == Some(false))
                .map(|(var, d)| format!("account {name} is signed out: {var}={d} {}", h.login().unwrap_or(""))),
            Err(e) => (e.downcast_ref::<crate::gate::Refusal>())
                .filter(|f| f.code == "SIGNED_OUT" && !dry)
                .map(|f| f.msg.clone()),
        };
        let Some(why) = out else {
            *env = got?;
            break;
        };
        let why = format!("{why}; sign it in, then `drover accounts reset {name}`");
        let d = json!({"account": name, "harness": role.harness, "meter": "sign-in check", "reason": why});
        let e = Event::new(ctx.now, fleet, None, "account-signed-out", d);
        emit(&ctx.state, cfg, e)?;
        a = pick(ctx, cfg, &read(&ctx.state)?, role)?;
    }
    p.token = a.as_ref().and_then(|a| token_file(cfg, &p.harness, &a.0));
    p.account = a;
    Ok(())
}

/// #683: the provider's config-folder env var (`CONFIG_ENV`) and its value in `env`.
fn folder<'a>(harness: &str, env: &'a BTreeMap<String, String>) -> Option<(&'static str, &'a str)> {
    let Credential::Account(acct) = crate::providers::get(harness).credential() else {
        return None;
    };
    let var = acct.config_env;
    Some((var, env.get(var)?))
}

/// #707: with no `account_env`, account `name`'s `[accounts]` value `token-file:<path>` (hand-placed, 0600): the env var
/// its lane gets the token as (`Harness::token_env`) and the file.
pub fn token_file(cfg: &Config, harness: &str, name: &str) -> Option<(String, std::path::PathBuf)> {
    let h = crate::providers::get(harness);
    let Credential::Account(acct) = h.credential() else {
        return None;
    };
    let f = cfg.accounts.names.get(name)?.strip_prefix("token-file:")?;
    let var = acct.token_env?;
    cfg.accounts
        .account_env
        .is_none()
        .then(|| (var.to_string(), crate::config::expand(f)))
}

/// #683: account `name`'s sign-in for `drover accounts`: whether it passes, and its line (` → ` its fix): its env's
/// config folder signed in as its CLI's status command says (#785; never the Keychain), or why its env failed.
pub fn sign_in(cfg: &Config, r: &dyn Runner, name: &str) -> (bool, String) {
    let Some(hn) = crate::providers::accounts().map(|p| p.name) else {
        return (false, "no harness runs on [accounts]".into());
    };
    if let Some((var, f)) = token_file(cfg, hn, name) {
        let ok = crate::secret::file(&f).map(|v| v.is_some());
        let why = ok.as_ref().map_or_else(|e| format!(" ({e:#})"), |_| String::new());
        return (
            ok.unwrap_or(false),
            format!("{var} from token file {}{why} → place it, mode 600", f.display()),
        );
    }
    let (h, env) = (crate::providers::get(hn), account_env(cfg, r, hn, name));
    let env = match env {
        Ok(env) => env,
        Err(e) => return (false, format!("{e:#}")),
    };
    let Some((var, dir)) = folder(hn, &env) else {
        let keys: Vec<&String> = env.keys().collect();
        return (true, format!("env {keys:?} from account_env"));
    };
    let login = h.login().unwrap_or("its login");
    let fix = format!("cooee Settings › Providers, or {var}={dir} {login}");
    (
        h.signed_in(r, Path::new(dir)) == Some(true),
        format!("signed in ({dir}) → {fix}"),
    )
}

/// `drover accounts reset <name>`: spawns may use a capped account again.
pub fn reset(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, name: &str) -> Result<()> {
    anyhow::ensure!(cfg.accounts.names.contains_key(name), "unknown account '{name}'");
    let events = read(&ctx.state)?;
    if capped_until(cfg, &events, name, ctx.now).is_none() {
        writeln!(out, "{name} is not capped")?;
        return Ok(());
    }
    let cap = latest(&events, name).unwrap();
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, &cap.fleet, None, "account-reset", json!({"account": name})),
    )?;
    writeln!(out, "{name}: reset; spawns use it again")?;
    Ok(())
}

/// #640: `capped until Nov 4 08:00: banner 19:40 + 5h —, 7d 100%` / `signed out: usage 401 at 18:02`, else `ok`.
pub fn cap_line(cfg: &Config, r: &dyn Runner, events: &[Event], name: &str, now: u64) -> String {
    let (Some(t), Some(e)) = (capped_until(cfg, events, name, now), latest(events, name)) else {
        return "ok".into();
    };
    let (at, why) = (usage::date(r, e.ts, "+%H:%M"), e.str("meter").unwrap_or("no reading"));
    if e.kind == "account-signed-out" {
        return format!("signed out: {why} at {at}");
    }
    let until = usage::date(r, t, "+%b %-d %H:%M");
    format!("capped until {until}: banner {at} + {why}")
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
