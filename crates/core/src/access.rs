//! What a lane may use: its MCP servers.
use crate::config::{Config, Fleet, Role};
use crate::gate::refused;
use anyhow::{anyhow, Context, Result};
use serde_json::{Map, Value};

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
