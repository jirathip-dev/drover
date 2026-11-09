//! Claude Code's usage meter (#258): its status-line JSON's rate limits (#707: no Keychain read, no usage poll).
use super::{Part, UsageMeter};
use crate::usage::epoch;
use serde_json::Value;

pub struct Meter;

impl UsageMeter for Meter {
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
