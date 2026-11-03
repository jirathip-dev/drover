//! Claude Code's usage meter (#258): an `[accounts.usage]` login's Anthropic windows, read through its own `claude`
//! login in the macOS Keychain, and its status-line JSON. Secrets reach `curl` and `security` on stdin only.
use super::{Part, UsageMeter};
use crate::devices::sha256;
use crate::runner::{argv, Runner};
use crate::usage::{curl, epoch, q, Poll, Window};
use serde_json::{json, Value};
use std::path::Path;

pub struct Meter;

impl UsageMeter for Meter {
    fn poll(&self, r: &dyn Runner, dir: &Path, now: u64) -> Poll {
        poll(r, dir, now)
    }

    /// `rate_limits.five_hour` / `seven_day`: `used_percentage` and `resets_at` (epoch or RFC 3339).
    fn status(&self, v: &Value) -> (Part, Part) {
        let w = |k: &str| {
            let x = &v["rate_limits"][k];
            let reset = x["resets_at"].as_u64().or_else(|| epoch(x["resets_at"].as_str()?));
            (x["used_percentage"].as_f64(), reset)
        };
        (w("five_hour"), w("seven_day"))
    }
}

/// Claude Code's OAuth client id (its token endpoint wants it on a refresh).
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

/// The macOS Keychain service of a `CLAUDE_CONFIG_DIR=<dir>` login, as derived from Claude Code 2.1.308 (unverified):
/// `Claude Code-credentials-<first 8 hex of sha256(dir)>`. Never the default login's plain `Claude Code-credentials`.
pub fn keychain_service(dir: &Path) -> String {
    format!(
        "Claude Code-credentials-{}",
        &sha256(dir.to_string_lossy().as_bytes())[..8]
    )
}

/// The item's account attribute (`"acct"<blob>="…"`), so a write-back goes to the item that was read.
fn keychain_account(r: &dyn Runner, svc: &str) -> Option<String> {
    let o = r
        .run("security", &argv(&["find-generic-password", "-s", svc]))
        .ok()
        .filter(|o| o.code == 0)?;
    o.stdout.lines().find_map(|l| {
        Some(
            l.trim()
                .strip_prefix("\"acct\"<blob>=\"")?
                .strip_suffix('"')?
                .to_string(),
        )
    })
}

/// The login JSON stored in the item.
fn keychain_read(r: &dyn Runner, svc: &str, acct: &str) -> Option<Value> {
    let o = r
        .run(
            "security",
            &argv(&["find-generic-password", "-w", "-a", acct, "-s", svc]),
        )
        .ok()
        .filter(|o| o.code == 0)?;
    serde_json::from_str(o.stdout.trim()).ok()
}

/// Read the login's token from the Keychain (refreshing an expired one), then `/api/oauth/usage`.
fn poll(r: &dyn Runner, dir: &Path, now: u64) -> Poll {
    let svc = keychain_service(dir);
    let Some(acct) = keychain_account(r, &svc) else {
        return Poll::Login;
    };
    let Some(mut creds) = keychain_read(r, &svc, &acct) else {
        return Poll::Login;
    };
    let oa = &creds["claudeAiOauth"];
    let token = if oa["expiresAt"].as_u64().unwrap_or(0) / 1000 > now + 60 {
        oa["accessToken"].as_str().map(String::from).ok_or(Poll::Login)
    } else {
        refresh(r, &mut creds, (&svc, &acct), now)
    };
    let token = match token {
        Ok(t) => t,
        Err(p) => return p,
    };
    let conf = format!(
        "header = \"Authorization: Bearer {}\"\nheader = \"anthropic-beta: oauth-2025-04-20\"\n",
        q(&token)
    );
    match curl(r, "https://api.anthropic.com/api/oauth/usage", &conf) {
        Some((200, _, body)) => parse_usage(&body).map_or(Poll::Failed, |(s, w)| Poll::Got(s, w)),
        Some((401 | 403, ..)) => Poll::Login,
        Some((429, head, _)) => {
            let after = head
                .lines()
                .find_map(|l| l.strip_prefix("retry-after:"))
                .and_then(|v| v.trim().parse().ok());
            Poll::Busy(after.unwrap_or(0))
        }
        _ => Poll::Failed,
    }
}

/// Trade the refresh token for a new access token and store the rotated login back in the same item, verified by a
/// re-read. Err(Login) leaves the old login intact; Err(Lost) means it was rotated but not stored.
fn refresh(r: &dyn Runner, creds: &mut Value, (svc, acct): (&str, &str), now: u64) -> Result<String, Poll> {
    // The `security -i` script quotes both: refuse anything a quote or escape could break out of, before rotating.
    let safe = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || " ._@+-".contains(c));
    let oa = creds
        .get_mut("claudeAiOauth")
        .filter(|_| safe(svc) && safe(acct))
        .ok_or(Poll::Login)?;
    let body = json!({"grant_type": "refresh_token", "refresh_token": oa["refreshToken"].as_str().ok_or(Poll::Login)?, "client_id": CLIENT_ID});
    let conf = format!(
        "header = \"Content-Type: application/json\"\ndata = \"{}\"\n",
        q(&body.to_string())
    );
    let (status, _, out) = curl(r, "https://platform.claude.com/v1/oauth/token", &conf).ok_or(Poll::Login)?;
    let v: Value = serde_json::from_str(&out)
        .ok()
        .filter(|_| status == 200)
        .ok_or(Poll::Login)?;
    let token = v["access_token"].as_str().ok_or(Poll::Login)?.to_string();
    oa["accessToken"] = json!(token);
    if let Some(rt) = v["refresh_token"].as_str() {
        oa["refreshToken"] = json!(rt);
    }
    oa["expiresAt"] = json!((now + v["expires_in"].as_u64().unwrap_or(3600)) * 1000);
    let hex: String = creds.to_string().bytes().map(|b| format!("{b:02x}")).collect();
    let stored = r.run_in(
        "security",
        &argv(&["-i"]),
        &format!("add-generic-password -U -a \"{acct}\" -s \"{svc}\" -X \"{hex}\"\n"),
    );
    let back = stored
        .is_ok_and(|o| o.code == 0)
        .then(|| keychain_read(r, svc, acct))
        .flatten();
    if back.is_some_and(|b| b["claudeAiOauth"]["accessToken"] == json!(token)) {
        Ok(token)
    } else {
        Err(Poll::Lost)
    }
}

/// `five_hour` and `seven_day` of `/api/oauth/usage`: (utilization %, reset epoch).
pub fn parse_usage(body: &str) -> Option<(Window, Window)> {
    let v: Value = serde_json::from_str(body).ok()?;
    let w = |k: &str| Some((v[k]["utilization"].as_f64()?, epoch(v[k]["resets_at"].as_str()?)?));
    Some((w("five_hour"), w("seven_day")))
}
