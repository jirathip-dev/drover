mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::json;

const T: u64 = 1_000_000;
const LANE: &str = "sendmeter-impl-12";

fn ev(c: &drover::Ctx, n: u64, ts: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(n), kind, d)).unwrap();
}

fn logged(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    let all = read(&c.state).unwrap();
    all.iter().filter(|e| e.kind == kind).cloned().collect()
}

fn notified(c: &drover::Ctx) -> String {
    std::fs::read_to_string(c.state.with_file_name("notified.txt")).unwrap_or_default()
}

fn alerted(name: &str) -> drover::Ctx {
    let d = tmpdir(name);
    let to = d.join("state/notified.txt");
    let cfg = format!(
        "[[notify]]\ncmd = \"cat >> '{}'\"\non = [\"lane-dead\", \"lane-wave\"]\n\n{CONFIG}",
        to.display()
    );
    ctx(&d, &cfg)
}

fn at(c: &drover::Ctx, now: u64) -> drover::Ctx {
    drover::Ctx { now, ..c.clone() }
}

fn dying_adapter(name: &str) -> (drover::Ctx, FakeShed) {
    let c = alerted(name);
    let sp = json!({"role": "impl", "agent": LANE, "harness": "claude", "pane": LANE, "runtime": "shed-acp"});
    ev(&c, 12, T - 600, "spawn", sp);
    let sh = FakeShed::attached(&c);
    sh.lane(LANE, "working", "/x");
    let said = json!([
        {"seq": 1, "event": "turn", "prompt": "Read .brief.md"},
        {"seq": 2, "event": "tool_call", "id": "t1", "title": "cargo test", "status": "in_progress"}
    ]);
    sh.set(LANE, |t| (t["acp"], t["events"]) = (json!("claude-acp"), said));
    sh.screen(
        LANE,
        "> Read .brief.md\n[tool] cargo test (in_progress)\nnode:internal/process: uncaught\nError: socket hang up\n\n",
    );
    sh.set(LANE, |t| (t["state"], t["exit"]) = (json!("exited"), json!(137)));
    (c, sh)
}

fn card(e: &Event) -> String {
    drover::serve::words(e, "").2
}

#[test]
fn an_adapter_killed_by_a_signal_names_the_signal_its_last_stderr_line_and_acp_message() {
    let (c, _sh) = dying_adapter("adapter-signal");
    let (code, out) = run(&at(&c, T), &Fake::default(), &["check"]);
    assert_eq!(code, 0, "{out}");
    let dead = logged(&c, "lane-dead");
    assert_eq!(dead.len(), 1, "{out}");
    let why = dead[0].str("reason").unwrap();
    assert!(why.contains("adapter crash, signal 9"), "{why}");
    assert!(why.contains("Error: socket hang up"), "{why}");
    assert!(why.contains("cargo test (in_progress)"), "{why}");
    let shown = card(&dead[0]);
    assert!(
        shown.contains("signal 9") && shown.contains("Error: socket hang up"),
        "{shown}"
    );
}

#[test]
fn an_exit_heard_on_shed_events_carries_the_same_last_words() {
    let (c, sh) = dying_adapter("adapter-stream");
    let labels = json!({"fleet": "sendmeter", "issue": "12", "role": "impl", "harness": "claude"});
    sh.lock()
        .events
        .push(json!({"seq": 1, "name": LANE, "labels": labels, "event": "exited", "code": 137}));
    let cfg = drover::config::Config::load(&c.config).unwrap();
    drover::session::stamp::intake(&at(&c, T), &cfg, &cfg.shed().unwrap().wire).unwrap();
    let gone = logged(&c, "lane-state").pop().unwrap();
    assert_eq!(gone.str("state"), Some("gone"), "{gone:?}");
    let (code, out) = run(&at(&c, T + 60), &Fake::default(), &["check"]);
    assert_eq!(code, 0, "{out}");
    let dead = logged(&c, "lane-dead");
    let why = dead[0].str("reason").unwrap();
    assert!(
        why.contains("signal 9") && why.contains("Error: socket hang up"),
        "{why}"
    );
}

fn host(load: &str) -> Fake {
    Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", &format!("{{ {load} 1.20 1.00 }}\n"))
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "")
        .reply("which shed", "/opt/shed/bin/shed\n")
}

fn crashed_together(c: &drover::Ctx, k: u64) {
    for n in 1..=k {
        let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
        std::fs::create_dir_all(&wt).unwrap();
        let lane = |role: &str, agent: &str| {
            json!({"role": role, "agent": agent, "pane": agent, "workspace": "", "runtime": "shed",
                "worktree": wt, "branch": format!("issue-{n}"), "confirmed": true})
        };
        let agent = format!("sendmeter-impl-{n}");
        ev(c, n, T - 900, "spawn", lane("impl", &agent));
        let gone = json!({"agent": agent, "state": "gone", "why": "exited", "code": 1, "cause": "adapter crash",
            "since": T - 120 + n * 8});
        ev(c, n, T - 120 + n * 8, "lane-state", gone);
    }
}

fn respawned(c: &drover::Ctx) -> Vec<u64> {
    let all = read(&c.state).unwrap();
    let late = all
        .iter()
        .filter(|e| e.kind == "spawn" && e.ts >= T && e.str("role") == Some("impl"));
    late.filter_map(|e| e.issue).collect()
}

#[test]
fn six_lanes_of_one_host_dying_in_a_minute_come_back_in_one_pass_with_one_alert() {
    let c = alerted("adapter-wave");
    let _sh = FakeShed::attached(&c);
    crashed_together(&c, 6);
    let (code, out) = run(&at(&c, T), &host("1.50"), &["check"]);
    assert_eq!(code, 0, "{out}");
    let (code, out) = run(&at(&c, T + 30), &host("50.00"), &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(respawned(&c), Vec::<u64>::new(), "held while the host is busy: {out}");
    let (code, out) = run(&at(&c, T + 90), &host("1.50"), &["drive"]);
    assert_eq!(code, 0, "{out}");
    let mut back = respawned(&c);
    back.sort_unstable();
    assert_eq!(back, (1..=6).collect::<Vec<_>>(), "all six in one pass: {out}");
    let all = read(&c.state).unwrap();
    assert!(all.iter().all(|e| e.kind != "override"), "admission is never forced");
    let wave = logged(&c, "lane-wave");
    assert_eq!(wave.len(), 1, "{wave:?}");
    let why = wave[0].str("reason").unwrap();
    assert!(why.contains("6 lanes") && why.contains("adapter crash"), "{why}");
    let sent = notified(&c);
    assert_eq!(sent.lines().count(), 1, "one alert, not one per lane: {sent}");
    assert!(sent.contains("6 lanes"), "{sent}");
}
