mod common;
use common::*;
use drover::events::read;

fn fleet(name: &str, max: usize, issues: u64) -> (drover::Ctx, FakeShed) {
    fleet_with(name, max, "", issues, &[])
}

fn fleet_with(name: &str, max: usize, extra: &str, issues: u64, flags: &[&str]) -> (drover::Ctx, FakeShed) {
    let cfg = CONFIG
        .replace("max_parallel_spawns = 0", &format!("max_parallel_spawns = {max}"))
        .replace(
            "integration = \"staging\"",
            &format!("integration = \"staging\"\n{extra}"),
        );
    let c = ctx(&tmpdir(name), &cfg);
    let mut add = vec!["queue".to_string(), "sendmeter".into(), "add".into()];
    add.extend((1..=issues).map(|n| n.to_string()));
    add.extend(flags.iter().map(std::string::ToString::to_string));
    let add: Vec<&str> = add.iter().map(String::as_str).collect();
    assert_eq!(run(&c, &Fake::default(), &add).0, 0);
    let sh = FakeShed::new();
    sh.attach(&c);
    (c, sh)
}

fn started(sh: &FakeShed) -> bool {
    !sh.briefed().is_empty()
}

fn codes(d: &serde_json::Value) -> Vec<String> {
    let a = d["actions"].as_array().unwrap();
    a.iter().map(|a| a["code"].as_str().unwrap_or("").to_string()).collect()
}

fn fake(idle: u32) -> Fake {
    fake_with(idle, Fake::default())
}

fn fake_with(idle: u32, f: Fake) -> Fake {
    f.reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("iostat", &iostat(idle))
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "")
}

fn launched(r: &Fake, n: u64) -> usize {
    r.count(&format!(
        "sh -c nohup '/opt/drover/bin/drover' kick sendmeter {n} --why drive --now"
    ))
}

fn marker(c: &drover::Ctx, n: u64) -> Option<drover::db::Lease> {
    lease_on(c, &format!("spawn-sendmeter-{n}.pending"))
}

fn pass(c: &drover::Ctx, r: &Fake) -> serde_json::Value {
    let (code, out) = passes(c, r);
    assert_eq!(code, 0, "{out}");
    let ev = read(&c.state).unwrap();
    serde_json::Value::Object(ev.iter().rfind(|e| e.kind == "drive-pass").unwrap().data.clone())
}

fn mark(c: &drover::Ctx, n: u64, pid: u32, age: u64) {
    let e = serde_json::json!({"ts": 1, "fleet": "sendmeter", "issue": n, "type": "spawn", "role": "impl", "pid": pid});
    let key = format!("spawn-sendmeter-{n}.pending");
    let l = drover::db::take(&c.state, &key, c.now - age, 300, &e.to_string(), &|_| true).unwrap();
    assert!(drover::db::renew(&c.state, &l, &drover::db::Lease { pid, ..l.clone() }));
}

fn fix_due(c: &drover::Ctx, n: u64) {
    let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
    std::fs::create_dir_all(&wt).unwrap();
    let d = serde_json::json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "pane": format!("sendmeter-impl-{n}"),
        "worktree": wt, "branch": format!("issue-{n}")});
    let at = |kind, d| drover::events::Event::new(10, "sendmeter", Some(n), kind, d);
    drover::events::append(&c.state, &at("spawn", d)).unwrap();
    let v = serde_json::json!({"verdict": "FAIL", "sha": "a".repeat(40), "body": "FAIL\nsrc/x.rs:3 off by one"});
    drover::events::append(&c.state, &at("verdict", v)).unwrap();
}

fn act(d: &serde_json::Value, n: u64) -> serde_json::Value {
    d["actions"]
        .as_array()
        .unwrap()
        .iter()
        .rfind(|a| a["issue"] == n)
        .cloned()
        .unwrap()
}

#[test]
fn a_start_held_for_any_length_fires_no_spawn_held() {
    let (c, _sh) = fleet("kicks-held", 6, 1);
    let held = |ts| {
        let a = serde_json::json!([{"issue": 3, "step": "spawn", "outcome": "waiting", "code": "SPAWN_PENDING"}]);
        let e = drover::events::Event::new(ts, "sendmeter", None, "drive-pass", serde_json::json!({"actions": a}));
        drover::events::append(&c.state, &e).unwrap();
    };
    for ago in [86_000, 3600, 600, 60] {
        held(c.now - ago);
    }
    let (code, out) = run(&c, &Fake::default(), &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(!read(&c.state).unwrap().iter().any(|e| e.kind == "spawn-held"), "{out}");
}

fn one_slot_held(name: &str, issues: u64) -> (drover::Ctx, Fake, FakeShed) {
    let (c, sh) = fleet(name, 1, issues);
    mark(&c, 11, 4242, 10);
    let r = fake(80)
        .reply("kill -0 4242", "")
        .reply("ps -o etime= -p 4242", "  00:30\n");
    (c, r, sh)
}

fn kick(c: &drover::Ctx, r: &Fake, n: u64) -> String {
    let n = n.to_string();
    let (code, out) = run(c, r, &["kick", "sendmeter", &n, "--why", "webhook", "--now"]);
    assert_eq!(code, 0, "{out}");
    out
}

fn kicked_why(c: &drover::Ctx, n: u64) -> String {
    let ev = read(&c.state).unwrap();
    let p = ev
        .iter()
        .rfind(|e| e.kind == "drive-pass" && e.data["kick"] == n)
        .unwrap();
    act(&serde_json::Value::Object(p.data.clone()), n)["why"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn a_waiting_fix_round_gets_the_freed_slot_before_a_waiting_first_start() {
    let (c, held, sh) = one_slot_held("line-round-first", 1);
    fix_due(&c, 21);
    let d = pass(&c, &held);
    assert_eq!(
        (act(&d, 21)["code"].clone(), act(&d, 1)["code"].clone()),
        ("SPAWN_PENDING".into(), "SPAWN_PENDING".into()),
        "{d}"
    );
    let (code, board) = run(&c, &held, &["board", "sendmeter"]);
    assert_eq!(code, 0, "{board}");
    assert!(board.contains("line 2 SPAWN_PENDING"), "{board}");
    let (code, out) = run(&c, &held, &["waiting", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("line 1 on every host"), "{out}");
    let freed = fake(80);
    let out = kick(&c, &freed, 1);
    assert!(
        kicked_why(&c, 1).starts_with("waits its turn behind sendmeter #21"),
        "{out}"
    );
    assert!(!started(&sh) && marker(&c, 1).is_none(), "{out}");
    let d = pass(&c, &freed);
    assert_eq!((launched(&freed, 21), launched(&freed, 1)), (1, 0), "{d}");
}

#[test]
fn first_starts_refused_in_one_pass_are_admitted_in_queue_order() {
    let (c, held, sh) = one_slot_held("line-queue-order", 2);
    let d = pass(&c, &held);
    assert_eq!(codes(&d), ["SPAWN_PENDING", "SPAWN_PENDING"], "{d}");
    let freed = fake(80);
    let out = kick(&c, &freed, 2);
    assert_eq!(
        kicked_why(&c, 2),
        "waits its turn behind sendmeter #1 on local",
        "{out}"
    );
    assert!(!started(&sh), "{out}");
    let out = kick(&c, &freed, 1);
    assert!(started(&sh), "{out}");
}

#[test]
fn a_refused_start_never_sleeps_and_the_pass_goes_on() {
    let (c, _sh) = fleet("line-no-sleep", 6, 2);
    fix_due(&c, 21);
    for h in ["local", "mini"] {
        lease_as(&c, &format!("admit-{h}.lock"), 4242, drover::now() - 5, 60);
    }
    let r = fake(80)
        .reply("kill -0 4242", "")
        .reply("ps -o etime= -p 4242", "  00:05\n");
    let d = pass(&c, &r);
    for n in [21, 1, 2] {
        let a = act(&d, n);
        assert_eq!(a["code"], "SPAWN_PENDING", "{d}");
        assert!(
            a["why"].as_str().unwrap().contains("another pass is admitting a spawn"),
            "{d}"
        );
    }
    assert!(!r.called("sleep"), "{:?}", r.calls.borrow());
}
