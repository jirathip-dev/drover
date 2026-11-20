mod common;
use common::*;
use drover::events::{append, history, Event};
use serde_json::json;

const T: u64 = 1_000_000;
const FILES: [&str; 4] = [
    "ci-sendmeter-12.md",
    "drive-sendmeter.last",
    "premerge-sendmeter-12.md",
    "turn-sendmeter-12-impl",
];

fn fixture(name: &str, paused: bool) -> (drover::Ctx, FakeShed) {
    let cfg = format!(
        "{}\n[orchestrator.ops]\nfleets = [\"sendmeter\"]\n",
        CONFIG.replace("# never main", &format!("# never main\npaused = {paused}"))
    );
    let c = ctx(&tmpdir(name), &cfg);
    let sh = FakeShed::attached(&c);
    let d =
        json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude", "pane": "w1:1", "worktree": "/wt/a"});
    for (kind, d) in [
        ("queue", json!({"op": "add", "issues": [12, 13]})),
        ("spawn", d),
        (
            "verdict",
            json!({"verdict": "FAIL", "sha": "a".repeat(40), "body": "FAIL"}),
        ),
        (
            "needs-decision",
            json!({"id": "q1", "question": "trim?", "options": ["yes", "no"], "kind": "owner"}),
        ),
        ("merged", json!({})),
    ] {
        append(&c.state, &Event::new(T, "sendmeter", Some(12), kind, d)).unwrap();
    }
    for (kind, d) in [
        ("thread-new", json!({"id": "t1", "title": "Ops"})),
        ("secret-set", json!({"thread": "t1", "name": "API_KEY", "by": "owner"})),
    ] {
        append(&c.state, &Event::new(T, "sendmeter", None, kind, d)).unwrap();
    }
    for f in FILES {
        std::fs::write(c.state.with_file_name(f), f).unwrap();
    }
    (c, sh)
}

fn fleets(c: &drover::Ctx) -> Vec<String> {
    history(&c.state).unwrap().iter().map(|e| e.fleet.clone()).collect()
}

#[test]
fn rename_moves_everything_and_the_old_name_resolves() {
    let (c, _sh) = fixture("rename-ok", true);
    let before = std::fs::read_to_string(&c.config).unwrap();
    let (code, out) = run(&c, &Fake::default(), &["fleet", "rename", "sendmeter", "meter"]);
    assert_eq!(code, 0, "{out}");
    for l in [
        "fleet.toml: [fleet.sendmeter] → [fleet.meter]",
        "fleet.toml: [fleet.sendmeter.role.impl] → [fleet.meter.role.impl]",
        "state.db: 7 events",
        "secret API_KEY: sendmeter → meter",
        "file turn-sendmeter-12-impl → turn-meter-12-impl",
        "dry run",
    ] {
        assert!(out.contains(l), "{l}: {out}");
    }
    assert_eq!(
        std::fs::read_to_string(&c.config).unwrap(),
        before,
        "a dry run changes nothing"
    );
    assert!(fleets(&c).iter().all(|f| f == "sendmeter"));

    let (code, out) = run(
        &c,
        &Fake::default(),
        &["fleet", "rename", "sendmeter", "meter", "--yes"],
    );
    assert_eq!(code, 0, "{out}");
    let after = std::fs::read_to_string(&c.config).unwrap();
    assert!(after.contains("[fleet.meter]\nrepo = \"jirathip-dev/sendmeter\"") && !after.contains("fleet.sendmeter"));
    assert!(
        after.contains("fleets = [\"meter\"]") && after.contains("paused = true"),
        "{after}"
    );
    let (old, new): (toml::Table, toml::Table) = (before.parse().unwrap(), after.parse().unwrap());
    assert_eq!(
        old["fleet"]["sendmeter"], new["fleet"]["meter"],
        "every value unchanged"
    );
    let evs = history(&c.state).unwrap();
    assert!(evs.iter().all(|e| e.fleet == "meter"));
    let rows = drover::db::issues(&c.state).unwrap();
    assert!(!rows.is_empty() && rows.iter().all(|(k, s)| k.0 == "meter" && s.fleet == "meter"));
    let renamed: Vec<_> = evs.iter().filter(|e| e.kind == "fleet-renamed").collect();
    assert_eq!(renamed.len(), 1);
    assert_eq!(renamed[0].val("old"), "sendmeter");
    assert_eq!(renamed[0].val("by"), "owner");
    for f in FILES {
        let moved = c.state.with_file_name(f.replace("sendmeter", "meter"));
        assert_eq!(std::fs::read_to_string(moved).unwrap(), f);
        assert!(!c.state.with_file_name(f).exists());
    }
    let bak = c.state.with_file_name(format!("rename-sendmeter-{T}"));
    assert!(bak.join("state.db").exists() && bak.join("fleet.toml").exists() && bak.join(FILES[0]).exists());

    let (_, ls) = run(&c, &Fake::default(), &["queue", "sendmeter", "ls"]);
    assert!(ls.contains("#13"), "{ls}");
    let (_, asks) = run(&c, &Fake::default(), &["asks", "sendmeter", "--all", "--json"]);
    assert!(asks.contains("\"q1\"") && asks.contains("\"meter\""), "{asks}");
    let (_, status) = run(&c, &Fake::default(), &["status", "sendmeter"]);
    assert!(status.contains("meter") && status.contains("12"), "{status}");
    let (_, board) = run(&c, &Fake::default(), &["board", "sendmeter"]);
    assert!(board.contains("meter") && board.contains("13"), "{board}");
    let (_, secrets) = run(&c, &Fake::default(), &["secret", "ls"]);
    assert_eq!(secrets, "meter API_KEY\n");
}

#[test]
fn a_failure_mid_move_leaves_the_original_state() {
    let (c, _sh) = fixture("rename-fail", true);
    let before = std::fs::read_to_string(&c.config).unwrap();
    std::fs::create_dir_all(c.state.with_file_name("turn-meter-12-impl").join("x")).unwrap();
    let (code, out) = run(
        &c,
        &Fake::default(),
        &["fleet", "rename", "sendmeter", "meter", "--yes"],
    );
    assert_ne!(code, 0, "{out}");
    assert_eq!(std::fs::read_to_string(&c.config).unwrap(), before);
    assert!(fleets(&c).iter().all(|f| f == "sendmeter"), "the database rolled back");
    for f in FILES {
        assert_eq!(std::fs::read_to_string(c.state.with_file_name(f)).unwrap(), f);
    }
    assert!(!c.state.with_file_name("ci-meter-12.md").exists() && !c.state.with_file_name("drive-meter.last").exists());
    let (_, ls) = run(&c, &Fake::default(), &["queue", "sendmeter", "ls"]);
    assert!(ls.contains("#13"), "{ls}");
}

#[test]
fn rename_refuses_unpaused_live_or_taken() {
    let (c, _sh) = fixture("rename-refuse", false);
    let (code, out) = run(
        &c,
        &Fake::default(),
        &["fleet", "rename", "sendmeter", "meter", "--yes"],
    );
    assert_eq!(code, 3);
    assert!(out.contains("NOT_PAUSED"), "{out}");

    let (c, sh) = fixture("rename-live", true);
    append(
        &c.state,
        &Event::new(
            T,
            "sendmeter",
            Some(13),
            "spawn",
            json!({"role": "impl", "agent": "sendmeter-impl-13"}),
        ),
    )
    .unwrap();
    sh.lane("sendmeter-impl-13", "working", "/wt/b");
    let (code, out) = run(
        &c,
        &Fake::default(),
        &["fleet", "rename", "sendmeter", "meter", "--yes"],
    );
    assert!(code == 3 && out.contains("LANE_LIVE"), "{out}");
    for bad in ["Meter", "_x", "a b", "sendmeter"] {
        let (code, out) = run(&c, &Fake::default(), &["fleet", "rename", "sendmeter", bad]);
        assert!(code == 3 && out.contains("BAD_NAME"), "{bad}: {out}");
    }
    assert!(fleets(&c).iter().all(|f| f == "sendmeter"));
}

fn repo(c: &drover::Ctx, name: &str) {
    let text = std::fs::read_to_string(&c.config).unwrap();
    let text = text.replace("jirathip-dev/sendmeter", &format!("jirathip-dev/{name}"));
    std::fs::write(&c.config, text).unwrap();
}

#[test]
fn a_stray_event_under_the_repo_name_joins_the_renamed_fleet() {
    let (c, _sh) = fixture("rename-stray", true);
    repo(&c, "shed");
    let d = json!({"agent": "sendmeter-impl-130"});
    append(&c.state, &Event::new(T, "shed", Some(130), "fix-done", d)).unwrap();
    let (code, out) = run(&c, &Fake::default(), &["fleet", "rename", "sendmeter", "shed", "--yes"]);
    assert_eq!(code, 0, "{out}");
    let evs = history(&c.state).unwrap();
    assert!(evs.iter().all(|e| e.fleet == "shed"), "{evs:?}");
    let rows = drover::db::issues(&c.state).unwrap();
    assert!(rows.contains_key(&("shed".to_string(), 130)));
    assert!(rows.iter().all(|(k, s)| k.0 == "shed" && s.fleet == "shed"));
}

#[test]
fn rename_onto_drover_ignores_its_system_events() {
    let (c, _sh) = fixture("rename-system", true);
    for (kind, d) in [
        ("pass-slow", json!({"secs": 40})),
        ("job", json!({"name": "tick"})),
        ("push-sent", json!({"kind": "stall"})),
        ("ask-waiting", json!({"id": "q9"})),
        ("slo-tick", json!({})),
        ("self-updated", json!({"version": "1.2.3"})),
    ] {
        append(&c.state, &Event::new(T, "drover", None, kind, d)).unwrap();
    }
    let (code, out) = run(
        &c,
        &Fake::default(),
        &["fleet", "rename", "sendmeter", "drover", "--yes"],
    );
    assert_eq!(code, 0, "{out}");
    assert!(history(&c.state).unwrap().iter().all(|e| e.fleet == "drover"));
    let rows = drover::db::issues(&c.state).unwrap();
    let issues: Vec<u64> = rows.keys().map(|k| k.1).collect();
    assert_eq!(issues, vec![12], "{rows:?}");
    let (code, status) = run(&c, &Fake::default(), &["status", "drover"]);
    assert_eq!(code, 0, "{status}");
    let rows: Vec<&str> = status.lines().filter(|l| l.contains(" #")).collect();
    assert!(
        rows.len() == 1 && rows[0].starts_with("drover ") && rows[0].contains("#12"),
        "{status}"
    );
    for kind in [
        "pass-slow",
        "job",
        "push-sent",
        "ask-waiting",
        "slo-tick",
        "self-updated",
        "#0",
    ] {
        assert!(!status.contains(kind), "{kind}: {status}");
    }
    let (_, asks) = run(&c, &Fake::default(), &["asks", "drover", "--all", "--json"]);
    assert!(asks.contains("\"q1\"") && !asks.contains("q9"), "{asks}");
    let (code, board) = run(&c, &Fake::default(), &["board", "drover"]);
    assert_eq!(code, 0, "{board}");
    assert!(board.contains("13"), "{board}");
    for kind in [
        "pass-slow",
        "job",
        "push-sent",
        "ask-waiting",
        "slo-tick",
        "self-updated",
        "#0",
    ] {
        assert!(!board.contains(kind), "{kind}: {board}");
    }
}

#[test]
fn rename_onto_another_real_fleet_is_refused() {
    let (c, _sh) = fixture("rename-other", true);
    let text = std::fs::read_to_string(&c.config).unwrap();
    let other = "\n[fleet.other]\nrepo = \"jirathip-dev/other\"\nlocal = \"/tmp/does-not-matter/other\"\nintegration = \"staging\"\n";
    std::fs::write(&c.config, format!("{text}{other}")).unwrap();
    let d = json!({"role": "impl", "agent": "past-impl-5"});
    append(&c.state, &Event::new(T, "past", Some(5), "spawn", d)).unwrap();
    for taken in ["other", "past"] {
        let (code, out) = run(&c, &Fake::default(), &["fleet", "rename", "sendmeter", taken, "--yes"]);
        assert!(code == 3 && out.contains("BAD_NAME"), "{taken}: {out}");
    }
    assert!(history(&c.state).unwrap().iter().all(|e| e.fleet != "other"));
}
