//! Claude Code's usage meter (#258): its status-line JSON's rate limits (#707: no Keychain read, no usage poll).
use super::super::{Limits, Usage, Window};
use super::CAP_PATTERN;
use crate::usage::epoch;
use serde_json::Value;

pub struct Meter;

impl Usage for Meter {
    /// `rate_limits.five_hour` / `seven_day`: `used_percentage` and `resets_at` (epoch or RFC 3339).
    fn reading(&self, v: &Value) -> Limits {
        let w = |k: &str| {
            let x = &v["rate_limits"][k];
            let reset = x["resets_at"].as_u64().or_else(|| epoch(x["resets_at"].as_str()?));
            Window {
                used: x["used_percentage"].as_f64(),
                resets: reset,
            }
        };
        Limits {
            session: w("five_hour"),
            week: w("seven_day"),
        }
    }
    fn cap_banner(&self) -> &'static str {
        CAP_PATTERN
    }
}
