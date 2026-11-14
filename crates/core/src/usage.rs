use crate::config::{expand, Config};
use crate::devices::save;
use crate::events::{age, Event};
use crate::runner::{argv, Runner};
use crate::Ctx;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

const POLL: u64 = 600;

/// When spawn `events[i]`'s lane ended (None: it runs): its issue merged, closed or cleaned, its lane-dead, its ACP session gone (#785), a supersede of it, or its next spawn.
pub fn lane_end(events: &[Event], i: usize) -> Option<u64> {
    let s = &events[i];
    let a = s.str("agent").unwrap_or_default();
    let ends = |e: &&Event| match e.kind.as_str() {
        "merged" | "clean" | "closed" | "closed-other" => e.fleet == s.fleet && s.issue.is_some() && e.issue == s.issue,
        "lane-dead" => e.fleet == s.fleet && e.str("key").is_some_and(|k| k.ends_with(&format!(":{a}"))),
        "lane-state" => e.str("agent") == Some(a) && e.str("state") == Some("gone"),
        "supersede" => {
            e.str("agent") == Some(a) || e.data["killed"].as_array().is_some_and(|k| k.iter().any(|n| n == a))
        }
        "spawn" => e.str("agent") == Some(a),
        _ => false,
    };
    events[i + 1..].iter().find(ends).map(|e| e.ts)
}

/// Seconds `account`'s lanes ran in the `window` before `now`, each spawn until its `lane_end` (else `now`).
pub fn lane_secs(events: &[Event], account: &str, now: u64, window: u64) -> u64 {
    let from = now.saturating_sub(window);
    let spawns = events
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind == "spawn" && e.str("account") == Some(account));
    let end = |i| lane_end(events, i).unwrap_or(now).min(now);
    spawns.map(|(i, s)| end(i).saturating_sub(s.ts.max(from))).sum()
}

/// `5h 1.5 · 7d 12.0 drover lane-hours`: what drover ran, not the provider's quota.
pub fn lane_hours(events: &[Event], account: &str, now: u64) -> String {
    let h = |w: u64| lane_secs(events, account, now, w) as f64 / 3600.0;
    format!("5h {:.1} · 7d {:.1} drover lane-hours", h(5 * 3600), h(7 * 86400))
}

/// Local `date -r ts <fmt>` (UTC hh:mm when `date` cannot tell).
pub fn date(r: &dyn Runner, ts: u64, fmt: &str) -> String {
    match r.run("date", &argv(&["-r", &ts.to_string(), fmt])) {
        Ok(o) if o.code == 0 => o.stdout.trim().to_string(),
        _ => format!("{:02}:{:02} UTC", ts % 86400 / 3600, ts % 3600 / 60),
    }
}

fn dir(ctx: &Ctx) -> PathBuf {
    ctx.state.parent().unwrap_or(&ctx.state).join("usage")
}

pub fn load<T: DeserializeOwned>(p: &Path) -> Option<T> {
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

pub(crate) fn q(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(crate) fn curl(r: &dyn Runner, url: &str, conf: &str) -> Option<(u32, String, String)> {
    let o = r
        .run_in("curl", &argv(&["-sS", "-m", "15", "-D", "-", "-K", "-", url]), conf)
        .ok()
        .filter(|o| o.code == 0)?;
    let (head, body) = o.stdout.split_once("\r\n\r\n")?;
    Some((
        head.split_whitespace().nth(1)?.parse().ok()?,
        head.to_lowercase(),
        body.into(),
    ))
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Balance {
    pub ts: u64,
    pub currency: String,
    pub total: f64,
}

/// The first `balance_infos` entry of `DeepSeek`'s `/user/balance` reply.
pub fn parse_balance(body: &str, ts: u64) -> Option<Balance> {
    let v: Value = serde_json::from_str(body).ok()?;
    let b = &v["balance_infos"][0];
    Some(Balance {
        ts,
        currency: b["currency"].as_str()?.into(),
        total: b["total_balance"].as_str()?.parse().ok()?,
    })
}

pub(crate) fn env_value(text: &str, var: &str) -> Option<String> {
    let kv = text
        .lines()
        .filter_map(|l| l.trim().trim_start_matches("export ").split_once('='));
    kv.into_iter()
        .find(|(k, _)| k.trim() == var)
        .map(|(_, v)| v.trim().trim_matches(['"', '\'']).to_string())
}

/// The `DeepSeek` balance and whether it is under `warn_below`, read at most every 10 min; None without a key, Err(`key not found in <file>`) when the configured env file is unreadable or lacks it.
pub fn deepseek(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> Option<Result<(Balance, bool), String>> {
    let p = cfg.providers.deepseek.as_ref()?;
    let key = match p.env_file.as_deref() {
        Some(f) => match crate::secret::file(&expand(f)) {
            Err(e) => return Some(Err(format!("{e:#}"))),
            Ok(t) => match t.and_then(|t| env_value(&t, &p.key_env)).filter(|k| !k.is_empty()) {
                Some(k) => k,
                None => return Some(Err(format!("key not found in {f}"))),
            },
        },
        None => std::env::var(&p.key_env).ok().filter(|k| !k.is_empty())?,
    };
    let path = dir(ctx).join("deepseek.json");
    let cached: Option<Balance> = load(&path);
    let b = match cached {
        Some(b) if ctx.now < b.ts + POLL => b,
        _ => {
            let got = curl(
                r,
                "https://api.deepseek.com/user/balance",
                &format!("header = \"Authorization: Bearer {}\"\n", q(&key)),
            );
            match got.filter(|g| g.0 == 200).and_then(|g| parse_balance(&g.2, ctx.now)) {
                Some(b) => {
                    let _ = save(&path, &b);
                    b
                }
                None => cached?,
            }
        }
    };
    let low = b.total < p.warn_below;
    Some(Ok((b, low)))
}

/// (utilization %, reset epoch) of one usage window, if read.
pub type Window = Option<(f64, u64)>;

/// One account's usage reading: when it was read, and the (utilization %, reset epoch) of the 5-hour session and the 7-day week.
#[derive(Debug, Default, Clone)]
pub struct Meter {
    pub ts: u64,
    pub session: Window,
    pub week: Window,
}

/// #707: `row`, as the frozen board (#552) still calls it.
pub fn meter(ctx: &Ctx, _cfg: &Config, _r: &dyn Runner, name: &str) -> Option<Meter> {
    row(ctx, name)
}

/// RFC 3339 (`2026-10-31T12:59:59.5+07:00`, or `Z`) as epoch seconds.
pub fn epoch(s: &str) -> Option<u64> {
    let n = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0, 4)?, n(5, 7)?, n(8, 10)?, n(11, 13)?, n(14, 16)?, n(17, 19)?);
    let off = match s.get(19..)?.find(['+', '-']).map(|i| 19 + i) {
        Some(i) => (n(i + 1, i + 3)? * 3600 + n(i + 4, i + 6)? * 60) * if &s[i..=i] == "-" { -1 } else { 1 },
        None => 0,
    };
    let (y, m) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let (era, yoe) = (y.div_euclid(400), y.rem_euclid(400));
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + (153 * m + 2) / 5 + d - 1 - 719_468;
    u64::try_from(days * 86400 + h * 3600 + mi * 60 + se - off).ok()
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn out_at((pct, reset): (f64, u64), now: u64) -> Option<u64> {
    let start = reset.checked_sub(7 * 86400)?;
    let out = start + (now.checked_sub(start)? as f64 * 100.0 / pct.max(0.01)) as u64;
    (out < reset).then_some(out)
}

/// The meter's (text, percent) parts and the pace warning.
pub type Shown = (Vec<(String, f64)>, Option<String>);

/// The meter's (text, percent) parts and the pace warning (#707: never Err now the login poll is gone; `_login` and the Result stay for the frozen board's call).
#[allow(clippy::unnecessary_wraps)]
pub fn shown(r: &dyn Runner, m: &Meter, _login: &str, now: u64) -> Result<Shown, String> {
    let mut parts = vec![];
    let read = Some(now.saturating_sub(m.ts)).filter(|s| *s >= 60);
    let read = read.map_or(String::new(), |s| format!(", read {} ago", age(s)));
    let live = |(p, t): (f64, u64), when: String| (t > now).then(|| (p, format!("resets {when}")));
    let live = |w, when| live(w, when).unwrap_or((0.0, "reset".into()));
    if let Some((p, s)) = m.session.map(|w| live(w, age(w.1.saturating_sub(now)))) {
        parts.push((format!("session {p:.0}% ({s}{read})"), p));
    }
    if let Some((p, s)) = m.week.map(|w| live(w, date(r, w.1, "+%a %H:%M"))) {
        parts.push((format!("week {p:.0}% ({s})"), p));
    }
    let pace = m.week.filter(|w| w.1 > now).and_then(|w| out_at(w, now)).map(|t| {
        let s = date(r, t, "+%a %H");
        let part = |h: u32| {
            ["night", "morning", "afternoon", "evening"][match h {
                5..=11 => 1,
                12..=16 => 2,
                17..=21 => 3,
                _ => 0,
            }]
        };
        let when = s
            .split_once(' ')
            .and_then(|(d, h)| Some(format!("{d} {}", part(h.parse().ok()?))))
            .unwrap_or(s);
        format!("at this pace: out {when}")
    });
    Ok((parts, pace))
}

/// One account's usage, plain: `session 9% (resets 10m) · week 22% (resets Fri 11:59)`, or the login hint.
pub fn meter_line(r: &dyn Runner, m: &Meter, login: &str, now: u64) -> String {
    match shown(r, m, login, now) {
        Ok((parts, pace)) if !parts.is_empty() => parts
            .into_iter()
            .map(|p| p.0)
            .chain(pace)
            .collect::<Vec<_>>()
            .join(" · "),
        Ok(_) => "usage: not read yet".into(),
        Err(hint) => hint,
    }
}

/// One `drover statusline` reading of an account's limits, a line of `<state>/usage.jsonl`.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Row {
    pub ts: u64,
    pub account: String,
    pub five_hour_pct: Option<f64>,
    pub five_hour_reset: Option<u64>,
    pub seven_day_pct: Option<f64>,
    pub seven_day_reset: Option<u64>,
}

/// The newest row of each account.
pub fn rows(ctx: &Ctx) -> std::collections::BTreeMap<String, Row> {
    let text = std::fs::read_to_string(ctx.state.with_file_name("usage.jsonl")).unwrap_or_default();
    let rows = text.lines().filter_map(|l| serde_json::from_str::<Row>(l).ok());
    rows.map(|r| (r.account.clone(), r)).collect()
}

/// `row`, else `old` (#707: as the frozen board still calls it).
pub fn reading(ctx: &Ctx, name: &str, old: impl FnOnce() -> Option<Meter>) -> Option<Meter> {
    row(ctx, name).or_else(old)
}

/// #707: the account's newest status-line reading (`drover statusline`), fresh or stale; no `/api/oauth/usage` poll, so an account with no live lane keeps its last row.
pub fn row(ctx: &Ctx, name: &str) -> Option<Meter> {
    rows(ctx).remove(name).map(|w| Meter {
        ts: w.ts,
        session: w.five_hour_pct.zip(w.five_hour_reset),
        week: w.seven_day_pct.zip(w.seven_day_reset),
    })
}

/// `drover statusline`: the accounts harness's status-line JSON on stdin (`UsageMeter::status`); appends at most one row per account per minute and prints `myapp #12 · 5h 41% · 7d 63%`. Never fails the session: errors are swallowed.
pub fn statusline(ctx: &Ctx, account: &str, lane: &str, input: &str, out: &mut dyn Write) {
    let v: Value = serde_json::from_str(input).unwrap_or_default();
    let how = crate::providers::accounts().and_then(|p| p.usage());
    let (five, seven) = how
        .map(|u| u.reading(&v))
        .map(|l| (l.session, l.week))
        .unwrap_or_default();
    let (fp, sp) = (five.used, seven.used);
    let fresh = |r: &Row| ctx.now < r.ts + 60;
    let mut all = rows(ctx);
    if !account.is_empty() && (fp.is_some() || sp.is_some()) && !all.get(account).is_some_and(fresh) {
        let row = Row {
            ts: ctx.now,
            account: account.into(),
            five_hour_pct: fp,
            five_hour_reset: five.resets,
            seven_day_pct: sp,
            seven_day_reset: seven.resets,
        };
        let p = ctx.state.with_file_name("usage.jsonl");
        if std::fs::metadata(&p).is_ok_and(|m| m.len() > 1 << 20) {
            all.insert(account.into(), row);
            let _ = std::fs::write(&p, all.values().map(|r| format!("{}\n", json!(r))).collect::<String>());
        } else {
            let _ = std::fs::create_dir_all(ctx.state.parent().unwrap_or(&ctx.state));
            let f = std::fs::OpenOptions::new().create(true).append(true).open(p);
            let _ = f.map(|mut f| writeln!(f, "{}", json!(row)));
        }
    }
    let mut parts: Vec<String> = match lane.split(':').collect::<Vec<_>>()[..] {
        [f, i, _] if i != "-" => vec![format!("{f} #{i}")],
        _ => vec![],
    };
    parts.extend(fp.map(|p| format!("5h {p:.0}%")));
    parts.extend(sp.map(|p| format!("7d {p:.0}%")));
    let _ = writeln!(out, "{}", parts.join(" · "));
}
