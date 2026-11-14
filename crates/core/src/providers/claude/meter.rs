use super::super::{Limits, Usage, Window};
use super::CAP_PATTERN;
use crate::usage::epoch;
use serde_json::Value;

pub struct Meter;

impl Usage for Meter {
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
