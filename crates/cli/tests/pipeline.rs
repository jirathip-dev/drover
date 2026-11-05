//! #173: the configurable pipeline. The built-in default replays both fleets' recorded histories with today's decisions.
mod common;
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
        let log = herdsman::waiting::Log::new(&events[..=i]);
        let a = waiting_on(
            &log,
            &events[..=i],
            s,
            Some((&last, 1)),
            &pr(&last),
            f,
            Some(("/v.md", "PASS")),
        );
        let b = waiting_on(&log, &events[..=i], s, Some((&moved, 1)), &pr(&moved), f, None);
        let g = gate(f, &pr(&last), Some(s), false).map_or_else(|r| r.code.to_string(), |_| "ok".into());
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

/// `[fleet.atlas]` with roles impl, rev, sec and `stages` as its pipeline, plus a plain `[fleet.other]`.
fn atlas(stages: &str) -> String {
    let mut t = String::from("[fleet.atlas]\nrepo = \"o/atlas\"\nlocal = \"/tmp/a\"\nintegration = \"staging\"\n");
    for r in ["impl", "rev", "sec"] {
        t += &format!("[fleet.atlas.role.{r}]\nharness = \"claude\"\n");
    }
    t += "[fleet.other]\nrepo = \"o/other\"\nlocal = \"/tmp/o\"\nintegration = \"staging\"\n";
    t + stages
}

/// One `[[fleet.atlas.pipeline.stage]]`.
fn st(name: &str, kind: &str, role: &str, rounds: Option<u32>, next: &str) -> String {
    let role = if role.is_empty() {
        String::new()
    } else {
        format!("role = \"{role}\"\n")
    };
    let rounds = rounds.map_or(String::new(), |r| format!("rounds = {r}\n"));
    format!("[[fleet.atlas.pipeline.stage]]\nname = \"{name}\"\nkind = \"{kind}\"\n{role}{rounds}next = {{ {next} }}\n")
}

const IMPL: (&str, &str, &str, Option<u32>, &str) = ("impl", "build", "impl", None, "push = \"rev\"");
const GATES: (&str, &str, &str, Option<u32>, &str) = (
    "gates",
    "gate",
    "",
    Some(3),
    "ok = \"merge\", ci_red = \"impl\", conflict = \"impl:update\"",
);

fn stages(s: &[(&str, &str, &str, Option<u32>, &str)]) -> String {
    s.iter().map(|s| st(s.0, s.1, s.2, s.3, s.4)).collect()
}

/// The custom example: impl → rev → sec → gates → merge, both checks FAIL back to impl.
fn two_checks() -> String {
    atlas(&stages(&[
        IMPL,
        ("rev", "check", "rev", Some(3), "PASS = \"sec\", FAIL = \"impl\""),
        ("sec", "check", "sec", Some(2), "PASS = \"gates\", FAIL = \"impl\""),
        GATES,
    ]))
}

fn rules(cfg: &Config) -> Vec<&'static str> {
    cfg.fleet["atlas"].bad.iter().map(|p| p.rule).collect()
}

#[test]
fn the_brief_and_custom_pipelines_are_valid() {
    let brief = atlas(&stages(&[
        IMPL,
        ("rev", "check", "rev", Some(3), "PASS = \"gates\", FAIL = \"impl\""),
        GATES,
    ]));
    for t in [brief, two_checks()] {
        let cfg = Config::parse(&t).unwrap();
        assert_eq!(cfg.fleet["atlas"].bad, vec![], "{t}");
        assert!(cfg.fleet("atlas").is_ok());
    }
}

/// Each rule rejects a crafted pipeline, refused `BAD_PIPELINE` for that fleet only, with the stage's fleet.toml line.
#[test]
fn each_rule_rejects_a_crafted_pipeline() {
    let rev = ("rev", "check", "rev", Some(3), "PASS = \"gates\", FAIL = \"impl\"");
    let cases: Vec<(&str, String)> = vec![
        ("start", stages(&[rev, IMPL, GATES])),
        (
            "unreachable",
            stages(&[
                IMPL,
                rev,
                ("lost", "check", "sec", Some(1), "PASS = \"gates\", FAIL = \"impl\""),
                GATES,
            ]),
        ),
        (
            "no end",
            stages(&[
                IMPL,
                ("rev", "check", "rev", Some(3), "PASS = \"impl\", FAIL = \"impl\""),
                GATES,
            ]),
        ),
        (
            "no limit",
            stages(&[
                IMPL,
                ("rev", "check", "rev", None, "PASS = \"gates\", FAIL = \"impl\""),
                GATES,
            ]),
        ),
        (
            "unmapped",
            stages(&[IMPL, ("rev", "check", "rev", Some(3), "PASS = \"gates\""), GATES]),
        ),
        (
            "no role",
            stages(&[
                IMPL,
                ("rev", "check", "writer", Some(3), "PASS = \"gates\", FAIL = \"impl\""),
                GATES,
            ]),
        ),
        (
            "self-review",
            stages(&[
                IMPL,
                ("rev", "check", "impl", Some(3), "PASS = \"gates\", FAIL = \"impl\""),
                GATES,
            ]),
        ),
        (
            "no gate",
            stages(&[
                IMPL,
                ("rev", "check", "rev", Some(3), "PASS = \"merge\", FAIL = \"impl\""),
            ]),
        ),
    ];
    for (rule, s) in cases {
        let cfg = Config::parse(&atlas(&s)).unwrap();
        assert!(rules(&cfg).contains(&rule), "{rule}: got {:?}\n{s}", rules(&cfg));
        let err = cfg.fleet("atlas").unwrap_err().to_string();
        assert!(err.starts_with("REFUSED BAD_PIPELINE: fleet atlas"), "{err}");
        assert!(cfg.fleet("other").is_ok(), "{rule}: the other fleet keeps running");
        assert!(
            cfg.fleet["atlas"].bad.iter().all(|p| p.line > 0),
            "{:?}",
            cfg.fleet["atlas"].bad
        );
    }
}

/// The design's refused example: rev's PASS now goes to a `qa` check without rounds, `docs` uses an undefined role.
/// Its four errors, plus `unmapped`: drive has no step for a PASS that leads to a build.
#[test]
fn the_design_error_example_breaks_its_rules() {
    let cfg = Config::parse(&atlas(&stages(&[
        IMPL,
        ("rev", "check", "rev", Some(3), "PASS = \"qa\", FAIL = \"impl\""),
        ("qa", "check", "sec", None, "PASS = \"docs\", FAIL = \"impl\""),
        ("docs", "build", "writer", None, "push = \"qa\""),
        GATES,
    ])))
    .unwrap();
    assert_eq!(
        rules(&cfg),
        ["unreachable", "no end", "no limit", "unmapped", "no role"]
    );
    let bad = &cfg.fleet["atlas"].bad;
    assert_eq!(bad[0].msg, "stage gates can't be reached: no outcome leads to it");
    assert_eq!(bad[2].msg, "loop qa → docs → qa has no round limit: set rounds on qa");
    assert_eq!(
        bad[3].msg,
        "stage qa: PASS goes to docs; it must go to a check or gate or merge"
    );
    assert_eq!(
        bad[4].msg,
        "stage docs: role writer is not defined; add [fleet.atlas.role.writer]"
    );
    assert_eq!((bad[0].stage.as_str(), bad[0].line), ("gates", 36));
}

fn ev(ts: u64, kind: &str, d: serde_json::Value) -> Event {
    Event::new(ts, "atlas", Some(12), kind, d)
}

fn spawn(ts: u64, role: &str, check: bool) -> Event {
    ev(
        ts,
        "spawn",
        json!({"role": role, "stage": role, "check": check, "agent": format!("atlas-{role}-12-{ts}")}),
    )
}

fn verdict(ts: u64, stage: &str, v: &str, sha: &str) -> Event {
    ev(
        ts,
        "verdict",
        json!({"verdict": v, "sha": sha, "stage": stage, "loop": stage}),
    )
}

fn pr(head: &str) -> serde_json::Value {
    json!({"state": "OPEN", "number": 7, "headRefOid": head, "baseRefName": "staging", "mergeable": "MERGEABLE",
        "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}]})
}

/// What `waiting_on` has drive run next, and the merge gate's answer, after `events`.
fn next(cfg: &Config, events: &[Event], head: &str) -> (Option<String>, Result<(), &'static str>) {
    let f = cfg.fleet("atlas").unwrap();
    let s = &fold(events)[&("atlas".to_string(), 12)];
    let w = waiting_on(
        &herdsman::waiting::Log::new(events),
        events,
        s,
        Some((head, 1)),
        &pr(head),
        f,
        None,
    );
    (
        w.next_cmd,
        gate(f, &pr(head), Some(s), false).map(drop).map_err(|r| r.code),
    )
}

/// A custom 2-check pipeline: rev's PASS spawns sec on the same head, merge needs both PASSes at the head, and a FAIL
/// in either routes to a fix in impl.
#[test]
fn two_checks_merge_on_both_passes_and_either_fail_routes_to_fix() {
    let cfg = Config::parse(&two_checks()).unwrap();
    let (a, b) = ("a".repeat(40), "b".repeat(40));
    let mut e = vec![
        spawn(1, "impl", false),
        spawn(2, "rev", true),
        verdict(3, "rev", "PASS", &a),
    ];
    assert_eq!(
        next(&cfg, &e, &a),
        (Some("herdsman spawn atlas 12 sec".into()), Err("STALE_VERDICT"))
    );
    e.push(spawn(4, "sec", true));
    assert_eq!(fold(&e)[&("atlas".to_string(), 12)].shown(), "sec(1)");
    let mut pass = e.clone();
    pass.push(verdict(5, "sec", "PASS", &a));
    assert_eq!(next(&cfg, &pass, &a), (Some("herdsman merge atlas 12".into()), Ok(())));
    // A head with only rev's PASS does not merge, nor one whose sec PASS is for an older head.
    let mut moved = pass.clone();
    moved.extend([
        spawn(6, "impl", false),
        spawn(7, "rev", true),
        verdict(8, "rev", "PASS", &b),
    ]);
    assert_eq!(next(&cfg, &moved, &b).1, Err("STALE_VERDICT"));
    // sec FAIL → fix in impl; then rev FAIL → fix in impl.
    e.push(verdict(5, "sec", "FAIL", &a));
    let fix = Some("herdsman spawn atlas 12 impl --supersede".into());
    assert_eq!(next(&cfg, &e, &a), (fix.clone(), Err("VERDICT_FAIL")));
    e.extend([
        spawn(6, "impl", false),
        spawn(7, "rev", true),
        verdict(8, "rev", "FAIL", &b),
    ]);
    assert_eq!(next(&cfg, &e, &b), (fix, Err("VERDICT_FAIL")));
}

/// Per-stage round limits: sec (≤2) runs out on its 2nd FAIL while rev (≤3) has used 2 of its own.
#[test]
fn round_limits_are_per_check_stage() {
    let d = common::tmpdir("pipeline-rounds");
    let c = common::ctx(&d, &two_checks());
    let (a, b) = ("a".repeat(40), "b".repeat(40));
    let file = d.join("v.md");
    std::fs::write(&file, "FAIL\n\nnope\n").unwrap();
    let file = file.to_string_lossy().to_string();
    let log = |e: Event| herdsman::events::append(&c.state, &e).unwrap();
    let verdict = |v: &str, sha: &str| {
        let r = common::Fake::default().reply("gh api", sha);
        common::run(&c, &r, &["verdict", "atlas", "12", v, "--sha", sha, "--file", &file])
    };
    log(spawn(1, "impl", false));
    log(spawn(2, "rev", true));
    assert!(verdict("PASS", &a).1.contains("(round 1/3)"));
    log(spawn(3, "sec", true));
    let (_, out) = verdict("FAIL", &a);
    assert!(out.contains("(round 1/2)") && !out.contains("needs-human"), "{out}");
    log(spawn(4, "impl", false));
    log(spawn(5, "rev", true));
    assert!(verdict("PASS", &b).1.contains("(round 2/3)"));
    log(spawn(6, "sec", true));
    let (_, out) = verdict("FAIL", &b);
    assert!(
        out.contains("(round 2/2)") && out.contains("round limit reached → needs-human"),
        "{out}"
    );
    let s = &fold(&herdsman::events::read(&c.state).unwrap())[&("atlas".to_string(), 12)];
    assert_eq!(
        (s.state.as_str(), s.stage_rounds["rev"], s.stage_rounds["sec"]),
        ("needs-human", 2, 2)
    );
}

/// `BAD_PIPELINE` stops only its fleet's commands; doctor lists the rule.
#[test]
fn a_bad_pipeline_refuses_spawn_and_doctor_lists_it() {
    let bad = atlas(&stages(&[
        IMPL,
        ("rev", "check", "rev", Some(3), "PASS = \"merge\", FAIL = \"impl\""),
    ]));
    let d = common::tmpdir("pipeline-bad");
    let c = common::ctx(&d, &bad);
    let (code, out) = common::run(&c, &common::Fake::default(), &["spawn", "atlas", "12", "impl"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED BAD_PIPELINE") && out.contains("no gate"), "{out}");
    let (_, out) = common::run(&c, &common::Fake::default(), &["doctor"]);
    assert!(out.contains("✗ pipeline atlas — BAD_PIPELINE, 1 error(s)"), "{out}");
    assert!(
        out.contains("rev's PASS goes straight to merge: route it through a gate"),
        "{out}"
    );
    assert!(out.contains("✓ pipeline other — valid (built-in)"), "{out}");
}

/// Red CI after the last round's PASS is the gate's FAIL at the same head: no new round, and needs-human at once.
#[test]
fn red_ci_in_the_last_round_goes_to_park() {
    let d = common::tmpdir("pipeline-ci-red");
    let c = common::ctx(&d, common::CONFIG);
    let file = d.join("v.md");
    std::fs::write(&file, "FAIL\n\nCI red\n").unwrap();
    let file = file.to_string_lossy().to_string();
    let log =
        |ts, kind, d| herdsman::events::append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
    for (i, sha) in ["a", "b", "c"].iter().enumerate() {
        let t = 10 * i as u64;
        log(t, "spawn", json!({"role": "impl", "agent": "sendmeter-impl-12"}));
        log(
            t + 1,
            "spawn",
            json!({"role": "rev", "agent": format!("sendmeter-rev-12-r{i}")}),
        );
        let v = if i == 2 { "PASS" } else { "FAIL" };
        log(t + 2, "verdict", json!({"verdict": v, "sha": sha.repeat(40)}));
    }
    let head = "c".repeat(40);
    let r = common::Fake::default().reply("gh api", &head);
    let (_, out) = common::run(
        &c,
        &r,
        &["verdict", "sendmeter", "12", "FAIL", "--sha", &head, "--file", &file],
    );
    assert!(out.contains("(round 3/3)") && out.contains("needs-human"), "{out}");
    let events = herdsman::events::read(&c.state).unwrap();
    let v = events.iter().rfind(|e| e.kind == "verdict").unwrap();
    assert_eq!((v.str("stage"), v.str("loop")), (Some("gates"), Some("rev")));
}
