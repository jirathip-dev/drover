mod common;
use common::*;
use herdsman::config::{Limits, LIMIT_DOCS};
use herdsman::events::{append, fold, Event, STATES};
use serde_json::json;

const TIMING: [&str; 5] = ["_secs", "_millis", "_minutes", "_hours", "_days"];

fn limit_line(out: &str, key: &str, value: &str) -> bool {
    out.contains(&format!("{key} = {value}"))
}

#[test]
fn the_diagram_is_docs_workflow_md_with_every_state_and_timing_limit() {
    let c = ctx(&tmpdir("diagram"), "");
    let (code, out) = run(&c, &Fake::default(), &["diagram"]);
    assert_eq!(code, 0, "{out}");
    let doc = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/workflow.md")).unwrap();
    assert_eq!(
        out, doc,
        "docs/workflow.md is stale: `herdsman diagram > docs/workflow.md`"
    );
    for s in [
        "queued",
        "design",
        "design-review",
        "impl",
        "review(n)",
        "fix(n)",
        "ready",
        "blocked",
        "needs-human",
        "parked",
        "merged",
        "closed",
    ] {
        assert!(out.contains(&format!("state \"{s}\" as ")), "{s} missing");
    }
    assert!(STATES.iter().all(|(s, _)| out.contains(&format!("state \"{s}\" as "))));
    let l = serde_json::to_value(Limits::default()).unwrap();
    let timed = LIMIT_DOCS.iter().filter(|(k, _)| TIMING.iter().any(|t| k.ends_with(t)));
    let stepped = (herdsman::drive::STEPS.iter().chain(&herdsman::timer::TICK_STEPS)).flat_map(|s| s.1.iter());
    for k in timed.map(|d| d.0).chain(stepped.copied()) {
        assert!(limit_line(&out, k, &l[k].to_string()), "{k} missing");
    }
    for (step, _) in herdsman::drive::STEPS.iter().chain(&herdsman::timer::TICK_STEPS) {
        assert!(out.contains(&format!("[\"{step}\"]")), "{step} missing");
    }
}

#[test]
fn live_counts_issues_per_state_at_the_configs_limits() {
    let c = ctx(
        &tmpdir("diagram-live"),
        &CONFIG.replace("stall_minutes = 30", "stall_minutes = 45"),
    );
    let ev = |n, kind: &str, d| append(&c.state, &Event::new(1_000, "sendmeter", Some(n), kind, d)).unwrap();
    ev(1, "spawn", json!({"role": "impl", "agent": "sendmeter-impl-1"}));
    ev(2, "spawn", json!({"role": "impl", "agent": "sendmeter-impl-2"}));
    ev(2, "spawn", json!({"role": "rev", "agent": "sendmeter-rev-2-r1"}));
    ev(3, "spawn", json!({"role": "impl", "agent": "sendmeter-impl-3"}));
    ev(3, "spawn", json!({"role": "rev", "agent": "sendmeter-rev-3-r1"}));
    ev(3, "verdict", json!({"verdict": "PASS", "sha": "a".repeat(40)}));
    let (code, out) = run(&c, &Fake::default(), &["diagram", "--live", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    for s in [
        "state \"impl · 1\" as impl",
        "state \"review(n) · 1\" as review",
        "state \"ready · 1\" as ready",
    ] {
        assert!(out.contains(s), "{s} in {out}");
    }
    assert!(out.contains("state \"merged · 0\" as merged"), "{out}");
    assert!(
        limit_line(&out, "stall_minutes", "45") && !limit_line(&out, "stall_minutes", "30"),
        "{out}"
    );
    let (code, out) = run(&c, &Fake::default(), &["diagram", "--live", "nope"]);
    assert_ne!(code, 0, "{out}");
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "a state not in STATES")]
fn a_fold_state_missing_from_the_table_fails() {
    let d = json!({"state": "limbo"});
    fold(&[Event::new(1, "sendmeter", Some(1), "repaired", d)]);
}
