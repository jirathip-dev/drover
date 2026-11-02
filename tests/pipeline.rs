//! #173: the configurable pipeline. The built-in default replays both fleets' recorded histories with today's decisions.
use herdsman::config::Config;
use herdsman::events::{fold, Event};
use herdsman::gate::gate;
use herdsman::waiting::waiting_on;
use serde_json::json;

const FLEETS: &str = r#"
[fleet.herdsman]
repo = "o/herdsman"
local = "/tmp/h"
integration = "staging"
[fleet.herdsman.role.impl]
harness = "claude"
[fleet.herdsman.role.rev]
harness = "claude"
[fleet.atlas]
repo = "o/atlas"
local = "/tmp/a"
integration = "staging"
[fleet.atlas.role.impl]
harness = "claude"
[fleet.atlas.role.rev]
harness = "claude"
"#;

/// After every event of the recorded log (sanitized: `tests/golden/pipeline/events.jsonl`), the issue's fold, its
/// wait reason for a PR at the last verdict's head and at a moved head, and the merge gate.
#[test]
fn default_pipeline_replays_recorded_histories() {
    let cfg = Config::parse(FLEETS).unwrap();
    let text = include_str!("golden/pipeline/events.jsonl");
    let events: Vec<Event> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let mut out = String::new();
    for (i, e) in events.iter().enumerate() {
        let Some(n) = e.issue else { continue };
        let st = fold(&events[..=i]);
        let s = &st[&(e.fleet.clone(), n)];
        let f = cfg.fleet(&s.fleet).unwrap();
        let last = s.verdicts.last().map_or("f".repeat(40), |v| v.0.clone());
        let pr = |head: &str| {
            json!({"state": "OPEN", "number": 7, "headRefOid": head, "baseRefName": "staging", "mergeable": "MERGEABLE",
                "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}]})
        };
        let moved = "e".repeat(40);
        let a = waiting_on(&events[..=i], s, Some((&last, 1)), &pr(&last), &[], Some(("/v.md", "PASS")));
        let b = waiting_on(&events[..=i], s, Some((&moved, 1)), &pr(&moved), &[], None);
        let g = gate(f, &pr(&last), Some(s), 3).map_or_else(|r| r.code.to_string(), |_| "ok".into());
        out += &format!(
            "{i} {} #{n} {} r{} u{} x{} | {} {:?} | {} {:?} | {g}\n",
            s.fleet, s.state, s.rounds, s.updates, s.extra, a.who, a.next_cmd, b.who, b.next_cmd
        );
    }
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/pipeline/decisions.txt");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(path, &out).unwrap();
    }
    assert_eq!(out, std::fs::read_to_string(path).unwrap(), "decisions changed");
}
