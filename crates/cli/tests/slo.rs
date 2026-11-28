mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const NOW: u64 = 1_000_000;

fn setup(name: &str) -> (drover::Ctx, std::path::PathBuf, FakeShed) {
    let dir = tmpdir(name);
    let sink = dir.join("push.txt");
    let cfg = format!(
        "{CONFIG}worktree_budget_gb = 0.000004\n\n[[notify]]\ncmd = \"cat >> '{}'\"\non = [\"stuck\", \"slo-idle\"]\n",
        sink.display()
    );
    let c = ctx(&dir, &cfg);
    let sh = FakeShed::attached(&c);
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let lane = c.worktrees.join("sendmeter/issue-9");
    std::fs::create_dir_all(&lane).unwrap();
    std::fs::write(lane.join("big.bin"), vec![1u8; 8192]).unwrap();
    (c, sink, sh)
}

fn at(c: &drover::Ctx, secs: u64) -> drover::Ctx {
    drover::Ctx {
        now: NOW + secs,
        ..c.clone()
    }
}

fn pass(c: &drover::Ctx, sh: &FakeShed, args: &[&str]) -> Fake {
    let r = Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("git ", "")
        .reply("sh -c nohup", "4321");
    let (code, out) = run_only(c, &sh.on(&r), args);
    assert_eq!(code, 0, "{out}");
    r
}

fn fast(c: &drover::Ctx, sh: &FakeShed) -> Fake {
    pass(c, sh, &["drive"])
}

fn kinds(c: &drover::Ctx) -> Vec<String> {
    let ours = ["slo-idle", "self-heal", "stuck", "slo-ok"];
    let ev = read(&c.state).unwrap().to_vec().into_iter().map(|e| e.kind);
    ev.filter(|k| ours.contains(&k.as_str())).collect()
}

fn screen(c: &drover::Ctx, sh: &FakeShed, path: &str, now: u64) -> Value {
    let f = Fake::default();
    let r = sh.on(&f);
    let s = Server::new(c, &r, Some("127.0.0.1:7777".into()));
    let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n\r\n");
    let res = s.respond(&req, now);
    let res = String::from_utf8_lossy(&res);
    serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap()
}

fn health_line(c: &drover::Ctx, sh: &FakeShed, now: u64) -> Option<String> {
    let v = screen(c, sh, "/screens/cards", now);
    let first = v.as_array().unwrap().first()?;
    (first["id"] == "drive-health-list").then(|| first["rows"][0]["title"].as_str().unwrap().to_string())
}

fn card(c: &drover::Ctx, sh: &FakeShed, now: u64) -> Option<Value> {
    screen(c, sh, "/screens/cards", now)
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["title"] == "Drive is stuck")
        .cloned()
}

#[test]
fn idle_21_minutes_with_spawnable_work_heals_and_never_makes_a_card() {
    let (c, sink, sh) = setup("slo-idle-stuck");
    pass(&c, &sh, &["check"]);
    fast(&c, &sh);
    assert!(kinds(&c).is_empty(), "idle starts now: no breach yet");
    fast(&at(&c, 19 * 60), &sh);
    assert!(kinds(&c).is_empty(), "19 min is inside the SLO");
    let r = fast(&at(&c, 21 * 60), &sh);
    assert_eq!(kinds(&c), ["slo-idle", "self-heal"]);
    assert!(
        r.called("sh -c nohup")
            && r.calls
                .borrow()
                .iter()
                .any(|l| l.contains("kick sendmeter 12 --why slo-idle"))
    );
    let ev = read(&c.state).unwrap();
    let heal = ev.iter().find(|e| e.kind == "self-heal").unwrap();
    assert_eq!(
        heal.str("did"),
        Some("recounted worktrees 0.0 → 0.0 GB; retried #12's spawn")
    );
    for m in [22, 26, 27, 40] {
        fast(&at(&c, m * 60), &sh);
    }
    assert_eq!(kinds(&c), ["slo-idle", "self-heal"], "a wait: no stuck");
    assert_eq!(std::fs::read_to_string(&sink).unwrap_or_default(), "");
    assert_eq!(card(&c, &sh, NOW + 40 * 60), None);
    assert_eq!(
        health_line(&c, &sh, NOW + 40 * 60).as_deref(),
        Some("idle: 1 fleet · 40m")
    );
    sh.lane("sendmeter-impl-12", "working", "/x");
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12", "runtime": "shed", "worktree": "/x", "branch": "issue-12"});
    append(&c.state, &Event::new(NOW + 41 * 60, "sendmeter", Some(12), "spawn", d)).unwrap();
    fast(&at(&c, 42 * 60), &sh);
    assert_eq!(kinds(&c), ["slo-idle", "self-heal", "slo-ok"]);
    assert_eq!(health_line(&c, &sh, NOW + 42 * 60), None);
}

#[test]
fn idle_held_by_host_busy_is_a_wait_not_a_card() {
    let (c, sink, sh) = setup("slo-idle-busy");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/issue-9")).unwrap();
    let d = json!({"code": "HOST_BUSY", "reason": "load 9.1 per cpu"});
    append(&c.state, &Event::new(NOW, "sendmeter", Some(12), "driven", d)).unwrap();
    for m in [0, 21, 30, 60] {
        fast(&at(&c, m * 60), &sh);
    }
    assert_eq!(kinds(&c), ["slo-idle"]);
    assert_eq!(std::fs::read_to_string(&sink).unwrap_or_default(), "");
    assert_eq!(card(&c, &sh, NOW + 60 * 60), None);
}

fn quiet(name: &str) -> (drover::Ctx, std::path::PathBuf, FakeShed) {
    let (c, sink, sh) = setup(name);
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/issue-9")).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "rm", "12"]).0, 0);
    (c, sink, sh)
}

fn ev(c: &drover::Ctx, n: u64, ts: u64, kind: &str, d: Value) {
    append(&c.state, &Event::new(NOW + ts, "sendmeter", Some(n), kind, d)).unwrap();
}

fn spawned(c: &drover::Ctx, n: u64) {
    let agent = format!("sendmeter-impl-{n}");
    let d = json!({"role": "impl", "agent": agent, "pane": agent, "runtime": "shed", "worktree": "/x", "branch": format!("issue-{n}")});
    ev(c, n, 0, "spawn", d);
}

#[test]
fn a_missed_promise_with_its_lane_gone_makes_no_card_and_a_live_one_clears() {
    let (c, sink, sh) = quiet("slo-promise");
    spawned(&c, 7);
    let line = "PASS · verify skipped · merging next pass";
    let p = json!({"kind": "merging", "reason": line, "deadline": NOW + 620, "state": "impl", "head": ""});
    ev(&c, 7, 20, "promise", p);
    let m = json!({"kind": "merging", "promised": line, "age": 680, "why": "drive took no step"});
    ev(&c, 7, 700, "promise-missed", m);
    let old = json!({"key": format!("promise:sendmeter#7:{}", NOW + 20), "reason": "⚠ missed 11m: drive took no step"});
    ev(&c, 7, 710, "stuck", old);
    assert!(card(&c, &sh, NOW + 710).is_some(), "the card live before deploy");
    fast(&at(&c, 720), &sh);
    assert_eq!(card(&c, &sh, NOW + 720), None, "cleared on the first pass");
    fast(&at(&c, 1100), &sh);
    assert_eq!(kinds(&c), ["stuck", "slo-ok"]);
    assert_eq!(std::fs::read_to_string(&sink).unwrap_or_default(), "");
}

fn pass_error(c: &drover::Ctx, fleet: &str, ts: u64, error: bool) {
    let outcome = if error { "error" } else { "ok" };
    let d = json!({"by": "timer", "actions": [{"step": "pass", "outcome": outcome, "words": "failed: config gone"}]});
    append(&c.state, &Event::new(NOW + ts, fleet, None, "drive-pass", d)).unwrap();
}

#[test]
fn late_and_erroring_drive_in_two_fleets_is_one_top_line_never_a_card_or_a_push() {
    let dir = tmpdir("slo-health");
    let sink = dir.join("push.txt");
    let kiln = "\n[fleet.kiln]\nrepo = \"jirathip-dev/kiln\"\nlocal = \"/tmp/does-not-matter/kiln\"\nintegration = \"staging\"\n";
    let cfg = format!(
        "{CONFIG}\n[[notify]]\ncmd = \"cat >> '{}'\"\non = [\"stuck\", \"slo-idle\"]\n{kiln}",
        sink.display()
    );
    let c = ctx(&dir, &cfg);
    let sh = FakeShed::attached(&c);
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    for f in ["sendmeter", "kiln"] {
        std::fs::write(c.state.with_file_name(format!("drive-{f}.last")), NOW.to_string()).unwrap();
    }
    pass_error(&c, "kiln", 0, true);
    fast(&at(&c, 4 * 60), &sh);
    assert_eq!(
        health_line(&c, &sh, NOW + 4 * 60).as_deref(),
        Some("pass error: 1 fleet · 4m")
    );
    fast(&at(&c, 6 * 60), &sh);
    fast(&at(&c, 11 * 60), &sh);
    assert_eq!(std::fs::read_to_string(&sink).unwrap_or_default(), "", "never a push");
    assert_eq!(card(&c, &sh, NOW + 11 * 60), None, "never a card");
    assert_eq!(
        health_line(&c, &sh, NOW + 11 * 60).as_deref(),
        Some("drive late: 2 fleets · pass error: 1 fleet · 11m")
    );
    let cards = screen(&c, &sh, "/screens/cards", NOW + 11 * 60);
    assert_eq!(cards[0]["rows"][0]["tap"], json!({"sheet": "/screens/drive-health"}));
    let sheet = screen(&c, &sh, "/screens/drive-health", NOW + 11 * 60);
    let rows: Vec<(String, String, String)> = (sheet["blocks"][0]["rows"].as_array().unwrap().iter())
        .map(|r| {
            let s = |k: &str| r[k].as_str().unwrap().to_string();
            (s("title"), s("subtitle"), s("value"))
        })
        .collect();
    let row = |a: &str, b: &str| (a.to_string(), b.to_string(), "11m".to_string());
    assert_eq!(
        rows,
        [
            row("kiln · drive late", "no drive pass for 6 min"),
            row("sendmeter · drive late", "no drive pass for 6 min"),
            row("kiln · pass error", "drive's pass failed: config gone"),
        ]
    );
    assert_eq!(sheet["blocks"][1]["label"], "Drive now");
    assert_eq!(sheet["blocks"][1]["action"], "drive-all");
    for f in ["sendmeter", "kiln"] {
        std::fs::write(
            c.state.with_file_name(format!("drive-{f}.last")),
            (NOW + 12 * 60).to_string(),
        )
        .unwrap();
    }
    pass_error(&c, "kiln", 12 * 60, false);
    fast(&at(&c, 12 * 60), &sh);
    assert_eq!(health_line(&c, &sh, NOW + 12 * 60), None, "healthy: no line");
}

fn failing(c: &drover::Ctx, n: u64) {
    spawned(c, n);
    for ts in [100, 400] {
        let d = json!({"step": "respawn", "error": "shed run: exit 1", "head": null});
        ev(c, n, ts, "spawn-error", d);
    }
}

#[test]
fn three_issues_drive_cannot_retry_push_once_and_show_one_card_with_a_force_each() {
    let (c, sink, sh) = quiet("slo-fold");
    for n in [7, 8, 9] {
        failing(&c, n);
    }
    let d = json!({"step": "respawn", "error": "shed run: exit 1", "head": null});
    ev(
        &c,
        10,
        0,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-10", "pane": "p", "worktree": "/x", "branch": "issue-10"}),
    );
    ev(&c, 10, 400, "spawn-error", d);
    fast(&at(&c, 500), &sh);
    fast(&at(&c, 800), &sh);
    assert_eq!(kinds(&c), ["stuck", "stuck", "stuck"], "one error is retried: no card");
    let pushes = std::fs::read_to_string(&sink).unwrap();
    assert_eq!(pushes.lines().count(), 1, "one push per fleet: {pushes}");
    assert!(
        pushes.contains("respawn fails the same way 2 times: shed run: exit 1"),
        "{pushes}"
    );
    let k = card(&c, &sh, NOW + 800).expect("one card");
    let shown = k.to_string();
    for n in [7, 8, 9] {
        let force = k["buttons"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["label"] == format!("Force #{n}"));
        let force = force.unwrap_or_else(|| panic!("{shown}"));
        assert_eq!(
            (force["action"].as_str(), &force["values"]),
            (Some("force"), &json!({"fleet": "sendmeter", "issue": n}))
        );
    }
    assert!(!shown.contains("drover why"), "{shown}");
    assert!(shown.contains("`drover waiting sendmeter`"), "{shown}");
    let (code, out) = run(&at(&c, 800), &sh.on(&Fake::default()), &["waiting", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn force_on_a_stuck_issue_spawns_it_past_the_load_limit_by_the_owner_and_its_line_clears() {
    std::env::remove_var("DROVER_LANE");
    std::env::remove_var("DROVER_ORCHESTRATOR");
    let (c, _sink, sh) = quiet("slo-force");
    failing(&c, 7);
    failing(&c, 8);
    fast(&at(&c, 500), &sh);
    assert!(card(&c, &sh, NOW + 500).is_some());
    let r = Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("iostat", &iostat(5))
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("git -C", "")
        .reply("gh issue view 7", r#"{"title":"t","body":"b"}"#);
    let on = sh.on(&r);
    let c5 = at(&c, 500);
    let s = Server::new(&c5, &on, Some("127.0.0.1:7777".into()));
    let cookie = pair(&s, &c, "", NOW + 500);
    let body = json!({"screen": "cards", "action": "force", "values": {"fleet": "sendmeter", "issue": 7}}).to_string();
    let req = format!(
        "POST /action HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&req, NOW + 500)).into_owned();
    assert!(res.starts_with("HTTP/1.1 200"), "{res}");
    assert!(
        res.contains("drover spawn sendmeter 7 impl") && res.contains("--force"),
        "{res}"
    );
    let ev = read(&c.state).unwrap();
    let o: Vec<_> = ev.iter().filter(|e| e.kind == "override").collect();
    assert_eq!(o.len(), 1, "{o:?}");
    assert_eq!(
        (o[0].issue, o[0].str("code"), o[0].str("by")),
        (Some(7), Some("HOST_BUSY"), Some("owner"))
    );
    let shown = card(&c, &sh, NOW + 500).expect("#8 still stuck").to_string();
    assert!(!shown.contains("Force #7") && shown.contains("Force #8"), "{shown}");
}

#[test]
fn a_missed_promise_a_hold_took_over_shows_no_stuck_card() {
    let (c, sink, sh) = setup("slo-missed-held");
    std::fs::remove_dir_all(c.worktrees.join("sendmeter/issue-9")).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "rm", "12"]).0, 0);
    let ev = |ts, kind: &str, d: Value| append(&c.state, &Event::new(NOW + ts, "sendmeter", Some(7), kind, d)).unwrap();
    let d = json!({"role": "impl", "agent": "sendmeter-impl-7", "pane": "sendmeter-impl-7", "runtime": "shed", "worktree": "/x", "branch": "issue-7"});
    ev(0, "spawn", d);
    let line = "pushed abc1234, waiting for drive (next pass ≤ 5 min) to spawn review";
    ev(
        20,
        "promise",
        json!({"kind": "next-pass", "reason": line, "deadline": NOW + 620, "state": "impl", "head": ""}),
    );
    ev(
        700,
        "promise-missed",
        json!({"kind": "next-pass", "promised": line, "age": 680, "why": "drive took no step"}),
    );
    ev(710, "held", json!({"on": [8]}));
    fast(&at(&c, 720), &sh);
    fast(&at(&c, 1020), &sh);
    assert!(!kinds(&c).contains(&"stuck".to_string()), "{:?}", kinds(&c));
    assert_eq!(card(&c, &sh, NOW + 1020), None);
    assert_eq!(std::fs::read_to_string(&sink).unwrap_or_default(), "");
}

const H2: u64 = 2 * 3600;

fn spawner(c: &drover::Ctx, sh: &FakeShed) -> Fake {
    pass_with(c, sh, None)
}

fn pass_with(c: &drover::Ctx, sh: &FakeShed, pr: Option<&str>) -> Fake {
    let r = pr
        .map_or_else(Fake::default, |p| Fake::default().reply("gh pr view issue-7", p))
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n vm.loadavg", "{ 1.0 1.0 1.0 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("git ", "")
        .reply("gh issue view 7", r#"{"title":"t","body":"b"}"#)
        .reply("sh -c nohup", "4321");
    if pr.is_some() {
        let cfg = drover::config::Config::load(&c.config).unwrap();
        let f = cfg.fleet("sendmeter").unwrap();
        assert_eq!(
            drover::gh::pr(c, &r, f, ("sendmeter", 7), "issue-7", c.now).unwrap()["number"],
            34
        );
    }
    let (code, out) = run_only(c, &sh.on(&r), &["drive"]);
    assert_eq!(code, 0, "{out}");
    r
}

fn of_kind(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

#[test]
fn a_lane_silent_for_the_stall_window_restarts_once_and_a_second_stall_is_a_stuck_card() {
    std::env::remove_var("DROVER_LANE");
    std::env::remove_var("DROVER_ORCHESTRATOR");
    let (c, _sink, sh) = quiet("slo-stall-restart");
    spawned(&c, 7);
    spawner(&at(&c, H2 - 60), &sh);
    assert!(of_kind(&c, "stall-restart").is_empty(), "inside the window");
    spawner(&at(&c, H2 + 60), &sh);
    let r = of_kind(&c, "stall-restart");
    assert_eq!(r.len(), 1, "{r:?}");
    assert_eq!(
        (r[0].issue, r[0].str("role"), r[0].str("by")),
        (Some(7), Some("impl"), Some("drover"))
    );
    assert!(
        r[0].str("reason").unwrap().contains("no push, verdict or lane event"),
        "{r:?}"
    );
    let spawns = of_kind(&c, "spawn");
    assert_eq!(spawns.len(), 2, "the same role respawned: {spawns:?}");
    assert_eq!((spawns[1].ts, spawns[1].str("role")), (NOW + H2 + 60, Some("impl")));
    spawner(&at(&c, 2 * H2 + 120), &sh);
    assert_eq!(of_kind(&c, "stall-restart").len(), 1, "restarted once");
    assert_eq!(of_kind(&c, "spawn").len(), 2);
    let shown = card(&c, &sh, NOW + 2 * H2 + 120)
        .expect("the second stall's card")
        .to_string();
    assert!(shown.contains("#7") && shown.contains("stalled"), "{shown}");
}

#[test]
fn named_waits_never_restart_or_move_a_quiet_lane() {
    type Wait = fn(&drover::Ctx);
    std::env::remove_var("DROVER_LANE");
    std::env::remove_var("DROVER_ORCHESTRATOR");
    let waits: [(&str, Wait); 7] = [
        ("held", |c| {
            assert_eq!(run(c, &Fake::default(), &["queue", "sendmeter", "add", "7"]).0, 0);
            assert_eq!(
                run(c, &Fake::default(), &["queue", "sendmeter", "block", "--on", "8", "7"]).0,
                0
            );
        }),
        ("ci", |_| {}),
        ("slot", |c| {
            let d = json!({"step": "respawn", "result": "REFUSED SPAWN_PENDING: a spawn is starting", "code": "SPAWN_PENDING"});
            ev(c, 7, 3600, "driven", d);
        }),
        ("review-queue", |c| {
            let p = json!({"kind": "spawn-pending", "reason": "review waits behind 1 earlier review",
                "deadline": NOW + 9 * H2, "state": "impl", "head": ""});
            ev(c, 7, 600, "promise", p);
        }),
        ("ask", |c| {
            ev(c, 7, 600, "needs-decision", json!({"id": "q1", "question": "which?"}));
        }),
        ("working", |c| {
            ev(c, 7, 3600, "turn-end", json!({"state": "working", "why": "long build"}));
        }),
        ("load-young", |c| {
            let d = json!({"step": "respawn", "result": "REFUSED HOST_BUSY: mini: load", "code": "HOST_BUSY"});
            ev(c, 7, 3600, "driven", d);
        }),
    ];
    for (name, wait) in waits {
        let (c, _sink, sh) = quiet(&format!("slo-stall-wait-{name}"));
        spawned(&c, 7);
        wait(&c);
        let ci = r#"{"number": 34, "state": "OPEN", "headRefOid": "abc", "statusCheckRollup": [{"__typename": "CheckRun", "name": "test", "status": "IN_PROGRESS", "conclusion": ""}]}"#;
        pass_with(&at(&c, H2 + 60), &sh, (name == "ci").then_some(ci));
        assert!(of_kind(&c, "stall-restart").is_empty(), "{name}: no restart");
        assert!(of_kind(&c, "stall-move").is_empty(), "{name}: no move");
        assert_eq!(of_kind(&c, "spawn").len(), 1, "{name}: no respawn");
        assert_eq!(card(&c, &sh, NOW + H2 + 60), None, "{name}: no card");
    }
}

#[test]
fn a_pass_error_clears_on_the_next_successful_pass_once_kicks_and_10k_events_bury_it() {
    let c = ctx(&tmpdir("slo-pass-ok"), CONFIG);
    let sh = FakeShed::attached(&c);
    let pass =
        |ts: u64, d: Value| serde_json::to_string(&Event::new(NOW + ts, "sendmeter", None, "drive-pass", d)).unwrap();
    let words = "failed in spawns: database or disk is full: Error code 13";
    let failed = json!({"by": "auto", "step": "spawns", "kick": null, "actions": [{"step": "pass", "outcome": "error", "words": words}]});
    let ok = json!({"by": "auto", "step": "PR reads, gates, steps", "kick": null, "actions": []});
    let kick =
        json!({"by": "auto", "step": "PR reads, gates, steps", "kick": 7, "actions": [{"step": "review", "issue": 7}]});
    let mut log = vec![pass(0, failed), pass(300, ok), pass(400, kick)];
    log.extend(
        (0..10_000u64).map(|i| {
            serde_json::to_string(&Event::new(NOW + 400, "sendmeter", None, "note", json!({"i": i}))).unwrap()
        }),
    );
    rewrite_log(&c.state, &(log.join("\n") + "\n"));
    std::fs::write(c.state.with_file_name("drive-sendmeter.last"), (NOW + 300).to_string()).unwrap();
    fast(&at(&c, 600), &sh);
    let stuck: Vec<Event> = of_kind(&c, "stuck")
        .into_iter()
        .filter(|e| e.str("key") == Some("pass-error:sendmeter"))
        .collect();
    assert!(stuck.is_empty(), "{stuck:?}");
}
