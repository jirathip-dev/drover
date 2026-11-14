use crate::config::{expand, Config};
use crate::devices::save;

use crate::runner::{argv, Runner};
use crate::Ctx;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};

const POLL: u64 = 600;

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

/// `drover statusline`: the lane's `fleet #N` for its harness's status line (#923: no usage meter).
pub fn statusline(lane: &str, out: &mut dyn Write) {
    let label = match lane.split(':').collect::<Vec<_>>()[..] {
        [f, i, _] if i != "-" => format!("{f} #{i}"),
        _ => String::new(),
    };
    let _ = writeln!(out, "{label}");
}
