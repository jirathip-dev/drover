//! M2 spawn gates: host load (HOST_BUSY), duplicate guard (LANE_LIVE,
//! WORKTREE_DIRTY, --supersede), the Hermes kick retry (#9) and the NO_ORCH refusal (#155). #771: generic spawn
//! behaviour runs on the shared fake shed (`shed_happy`); herdr's own start path (soft starts, pane ids, startup
//! prompts, `agent_pane_busy`, `agent_name_taken`) stays on `happy` for #721 to delete.
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use herdsman::runner::{Out, Runner};
use serde_json::json;
use std::path::PathBuf;

const WS: &str = r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#;
const PANES: &str = r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#;

fn wt(c: &herdsman::Ctx) -> PathBuf {
    c.worktrees.join("sendmeter/issue-12")
}

/// A Fake on which `herdsman spawn sendmeter 12 impl` (hermes) succeeds end to end.
/// Prepend `once` replies or pass `agent_list` to steer it.
fn happy(c: &herdsman::Ctx, agent_list: &str) -> Fake {
    let w = wt(c);
    std::fs::create_dir_all(&w).unwrap();
    Fake::default()
        .reply("herdr agent list", agent_list)
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply(&format!("git -C {} rev-list", w.display()), "0\n")
        .reply("git -C", "")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("herdr worktree", WS)
        .reply("herdr workspace create", WS)
        .reply("herdr pane list", PANES)
        .reply("herdr pane run", "{}")
        .reply("herdr agent start", "{}")
        .reply("herdr pane", "{}")
        .reply("herdr agent prompt", "{}")
        .reply("herdr agent wait", "{}")
}

/// #771: `happy` on a shed host: ctx `c`'s lanes on a fresh fake shed (returned; keep it alive), no herdr replies.
/// The worktree exists, clean and pushed.
fn shed_happy(c: &herdsman::Ctx) -> (FakeShed, Fake) {
    let w = wt(c);
    std::fs::create_dir_all(&w).unwrap();
    let sh = FakeShed::new();
    // A second call on the same ctx moves its lanes to this server (the last one's `[runtime]` dropped).
    let base = std::fs::read_to_string(&c.config).unwrap();
    let base = base.split("\n[runtime]\n").next().unwrap();
    std::fs::write(&c.config, sh.config(base)).unwrap();
    let f = Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply(&format!("git -C {} rev-list", w.display()), "0\n")
        .reply("git -C", "")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#);
    (sh, f)
}

fn spawn(c: &herdsman::Ctx, r: &dyn Runner, extra: &[&str]) -> (i32, String) {
    let mut a = vec!["spawn", "sendmeter", "12", "impl"];
    a.extend(extra);
    run(c, r, &a)
}

/// A spawn on shed `sh` refused `code`: no worktree made, no terminal run, nothing logged.
fn assert_refused(c: &herdsman::Ctx, sh: &FakeShed, r: &Fake, extra: &[&str], code: &str) -> String {
    let before = read(&c.state).unwrap().len();
    let (exit, out) = spawn(c, r, extra);
    assert_eq!(exit, 3, "{out} {:?}", r.calls.borrow());
    assert!(out.contains(&format!("REFUSED {code}")), "{out}");
    assert!(
        !r.called("git -C /tmp/does-not-matter/sendmeter worktree add"),
        "nothing created: {:?}",
        r.calls.borrow()
    );
    assert!(sh.all("run").is_empty(), "{:?}", sh.methods());
    assert_eq!(read(&c.state).unwrap().len(), before, "nothing logged");
    out
}

// --- host-load gate ---

#[test]
fn host_busy_on_memory_pressure() {
    for level in ["2", "4"] {
        let d = tmpdir("host-pressure");
        let c = ctx(&d, CONFIG);
        let (sh, r) = shed_happy(&c);
        let r = r.once("sysctl -n kern.memorystatus_vm_pressure_level", 0, level);
        let out = assert_refused(&c, &sh, &r, &[], "HOST_BUSY");
        assert!(out.contains("memory pressure"), "{out}");
    }
}

/// #168: one swap spike with normal pressure admits; a sustained median with warn pressure refuses.
#[test]
fn swap_refuses_on_sustained_median_with_pressure_not_one_spike() {
    let swap = |prev: &str, level: &str| {
        let d = tmpdir("host-swap");
        let c = ctx(&d, CONFIG);
        std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
        std::fs::write(c.state.with_file_name("swapouts"), prev).unwrap();
        let (sh, r) = shed_happy(&c);
        let r = r
            .once("sysctl -n kern.memorystatus_vm_pressure_level", 0, level)
            .once("vm_stat", 0, "Swapouts: 100.\n")
            .once("vm_stat", 0, "Swapouts: 5100.\n");
        (c, sh, r)
    };
    let (c, sh, r) = swap("0 0 0 0", "1");
    assert_eq!(spawn(&c, &r, &[]).0, 0, "a spike with normal pressure admits");
    assert!(r.called("sleep 2"), "two samples 2 s apart");
    assert_eq!(sh.all("run").len(), 1, "the lane started");
    let (c, sh, r) = swap("0 0 0 0", "2");
    let out = assert_refused(&c, &sh, &r, &[], "HOST_BUSY");
    assert!(!out.contains("swap-out"), "a spike's median is low: {out}");
    let (c, sh, r) = swap("4000 5000 6000 3000", "2");
    let out = assert_refused(&c, &sh, &r, &[], "HOST_BUSY");
    assert!(out.contains("swap-out median 5000 pages/2s > 2000"), "{out}");
    let w = std::fs::read_to_string(c.state.with_file_name("swapouts")).unwrap();
    assert_eq!(w, "4000 5000 6000 3000 5000", "the window keeps the last 5");
}

/// #168: free + inactive memory under `[limits] min_free_mem_gb` (default 2) refuses MEM_LOW.
#[test]
fn mem_low_under_the_floor() {
    let d = tmpdir("mem-low");
    let c = ctx(&d, CONFIG);
    let vm = |pages: u64| {
        format!("Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free: {pages}.\nPages inactive: {pages}.\nSwapouts: 100.\n")
    };
    let (sh, r) = shed_happy(&c);
    let r = r.once("vm_stat", 0, &vm(32768)).once("vm_stat", 0, &vm(32768));
    let out = assert_refused(&c, &sh, &r, &[], "MEM_LOW");
    assert!(out.contains("only 1.0 GB of memory free (floor 2 GB)"), "{out}");
    let (sh, r) = shed_happy(&c);
    let r = r.once("vm_stat", 0, &vm(65536)).once("vm_stat", 0, &vm(98304));
    assert_eq!(spawn(&c, &r, &[]).0, 0, "3 GB free admits");
    assert_eq!(sh.all("run").len(), 1);
}

/// #103: a fake `df` under `[limits] min_free_gb` (default 10) refuses DISK_LOW; at or over it admits.
#[test]
fn disk_low_under_min_free_gb() {
    let d = tmpdir("disk-low");
    let c = ctx(&d, CONFIG);
    let df = |kb: u64| {
        format!("Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 239362496 9 {kb} 50% /\n")
    };
    let (sh, r) = shed_happy(&c);
    let r = r.once("df -k /", 0, &df(9 * 1_048_576));
    let out = assert_refused(&c, &sh, &r, &[], "DISK_LOW");
    assert!(
        out.contains("9.0 GB free < 10;") && out.contains("herdsman clean --yes"),
        "{out}"
    );
    let (sh, r) = shed_happy(&c);
    let r = r.once("df -k /", 0, &df(19 * 1_048_576));
    let (exit, out) = spawn(&c, &r, &[]);
    assert_eq!(exit, 0, "{out}");
    assert_eq!(sh.all("run").len(), 1);
}

#[test]
fn host_busy_on_load_per_cpu() {
    let d = tmpdir("host-load");
    let c = ctx(&d, CONFIG);
    let (sh, r) = shed_happy(&c);
    let r = r.once("sysctl -n vm.loadavg", 0, "{ 26.00 3.00 2.00 }\n");
    let out = assert_refused(&c, &sh, &r, &[], "HOST_BUSY");
    assert!(out.contains("load/cpu 2.60"), "{out}");
}

#[test]
fn many_live_agents_with_normal_signals_admit() {
    let d = tmpdir("host-many");
    let c = ctx(&d, &format!("{CONFIG}max_agents = 12\n"));
    let (sh, r) = shed_happy(&c);
    for i in 0..50 {
        sh.lane(&format!("other-{i}"), "idle", "/elsewhere");
    }
    let (exit, out) = spawn(&c, &r, &[]);
    assert_eq!(
        exit, 0,
        "a leftover max_agents key is ignored and there is no count cap: {out}"
    );
    assert_eq!(sh.last("run")["name"], "sendmeter-impl-12");
}

#[test]
fn host_under_every_threshold_admits() {
    let d = tmpdir("host-ok");
    let c = ctx(&d, CONFIG);
    let (sh, r) = shed_happy(&c);
    let (exit, out) = spawn(&c, &r, &[]);
    assert_eq!(exit, 0, "{out} {:?}", r.calls.borrow());
    assert!(
        r.called("sysctl -n kern.memorystatus_vm_pressure_level"),
        "the host was probed"
    );
    assert_eq!(sh.last("run")["name"], "sendmeter-impl-12");
    let ev = read(&c.state).unwrap();
    assert!(ev.iter().any(|e| e.kind == "spawn"));
    assert!(ev.iter().all(|e| e.kind != "override"));
}

#[test]
fn force_admits_busy_host_and_logs_override() {
    let d = tmpdir("host-force");
    let c = ctx(&d, CONFIG);
    let (sh, r) = shed_happy(&c);
    let r = r.once("sysctl -n kern.memorystatus_vm_pressure_level", 0, "4");
    let (exit, out) = spawn(&c, &r, &["--force"]);
    assert_eq!(exit, 0, "{out}");
    assert_eq!(sh.all("run").len(), 1);
    let ev = read(&c.state).unwrap();
    let o = ev.iter().find(|e| e.kind == "override").expect("override event");
    assert_eq!(o.str("code"), Some("HOST_BUSY"));
    assert!(
        o.str("reason").unwrap().contains("memory pressure critical"),
        "{:?}",
        o.data
    );
    assert!(ev.iter().any(|e| e.kind == "spawn"));
}

// --- duplicate guard ---

#[test]
fn live_agent_in_worktree_is_lane_live() {
    let d = tmpdir("dup-live");
    let c = ctx(&d, CONFIG);
    let w = wt(&c).join("src");
    let (sh, r) = shed_happy(&c);
    sh.lane("someone", "working", w.to_str().unwrap());
    let out = assert_refused(&c, &sh, &r, &[], "LANE_LIVE");
    assert!(out.contains("someone"), "{out}");
}

#[test]
fn unfinished_lane_in_log_is_lane_live() {
    let d = tmpdir("dup-log");
    let c = ctx(&d, CONFIG);
    let w = wt(&c).to_string_lossy().into_owned();
    let d0 = json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12", "worktree": w});
    append(&c.state, &Event::new(10, "sendmeter", Some(12), "spawn", d0)).unwrap();
    let (sh, r) = shed_happy(&c);
    let out = assert_refused(&c, &sh, &r, &[], "LANE_LIVE");
    assert!(out.contains("unfinished impl lane"), "{out}");
}

#[test]
fn dirty_unknown_worktree_is_worktree_dirty() {
    let d = tmpdir("dup-dirty");
    let c = ctx(&d, CONFIG);
    let w = wt(&c);
    let (sh, r) = shed_happy(&c);
    let r = r.once(&format!("git -C {} status", w.display()), 0, " M src/x.rs\n");
    let out = assert_refused(&c, &sh, &r, &[], "WORKTREE_DIRTY");
    assert!(out.contains("uncommitted"), "{out}");
    let (sh, r) = shed_happy(&c);
    let r = r.once(&format!("git -C {} rev-list", w.display()), 0, "3\n");
    let out = assert_refused(&c, &sh, &r, &[], "WORKTREE_DIRTY");
    assert!(out.contains("3 unpushed"), "{out}");
    // fleet's own brief does not make a worktree dirty.
    let (sh, r) = shed_happy(&c);
    let r = r.once(&format!("git -C {} status", w.display()), 0, "?? .brief.md\n");
    assert_eq!(spawn(&c, &r, &[]).0, 0);
    assert_eq!(sh.all("run").len(), 1);
}

#[test]
fn supersede_kills_old_agent_then_spawns() {
    let d = tmpdir("dup-supersede");
    let c = ctx(&d, CONFIG);
    let w = wt(&c).to_string_lossy().into_owned();
    let d0 = json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12", "worktree": w});
    append(&c.state, &Event::new(10, "sendmeter", Some(12), "spawn", d0)).unwrap();
    let (sh, r) = shed_happy(&c);
    sh.lane("sendmeter-impl-12", "idle", &w);
    let (exit, out) = spawn(&c, &r, &["--supersede"]);
    assert_eq!(exit, 0, "{out} {:?}", r.calls.borrow());
    let m = sh.methods();
    let kill = m.iter().position(|m| m == "kill").expect("old agent killed");
    assert_eq!(sh.all("kill")[0]["name"], "sendmeter-impl-12");
    assert!(
        kill < m.iter().position(|m| m == "run").unwrap(),
        "kill before start: {m:?}"
    );
    let ev = read(&c.state).unwrap();
    let s = ev.iter().find(|e| e.kind == "supersede").expect("supersede logged");
    assert_eq!(s.data["killed"], json!(["sendmeter-impl-12"]));
    assert_eq!(ev.last().unwrap().kind, "spawn");
}

// --- #9: Hermes kick ---

/// shed + Hermes model: until the first attempt's pause (`sleep 10`) the lane's waits time out and its screen shows
/// no context (`ctx --`), so the first prompt is dropped; after it the next prompt lands.
struct Hermes<'a> {
    fake: Fake,
    sh: &'a FakeShed,
}

impl Runner for Hermes<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.fake.run(prog, args)
    }
    fn sleep(&self, secs: u64) {
        if secs == 10 {
            self.sh.lock().waits.clear();
            self.sh.screen(IMPL, "fleet-impl ❯  ctx 52K");
        }
        self.fake.sleep(secs)
    }
}

#[test]
fn hermes_kick_retries_after_dropped_first_prompt() {
    let d = tmpdir("kick-retry");
    let c = ctx(&d, CONFIG);
    let (sh, fake) = shed_happy(&c);
    sh.wait_ends(IMPL, "timeout");
    let h = Hermes { fake, sh: &sh };
    let (exit, out) = spawn(&c, &h, &[]);
    assert_eq!(exit, 0, "{out} {:?}", h.fake.calls.borrow());
    let f = &h.fake;
    let seen = sh.lock().seen.clone();
    let at = |m: &str, p: &dyn Fn(&serde_json::Value) -> bool| seen.iter().position(|s| s.0 == m && p(&s.1));
    let waited = at("wait", &|p| p["output"] == "❯").expect("waits for the prompt");
    let prompt = |p: &serde_json::Value| p["name"] == IMPL && !p["text"].as_str().unwrap().ends_with('\r');
    assert!(waited < at("send", &prompt).unwrap(), "waits for input");
    let prompts = seen.iter().filter(|s| s.0 == "send" && prompt(&s.1)).count();
    assert_eq!(prompts, 2, "landed on attempt 2: {seen:?}");
    let enters = sh
        .all("keys")
        .into_iter()
        .filter(|p| p["name"] == IMPL && p["keys"] == json!(["Enter"]));
    assert_eq!(enters.count(), 2, "explicit Enter each attempt");
    assert_eq!(f.count("sleep 10"), 1);
    assert_eq!(read(&c.state).unwrap().last().unwrap().data["confirmed"], true);
}

// --- #155: no orch lane ---

#[test]
fn an_orch_spawn_is_refused_and_every_lane_takes_an_issue() {
    let c = ctx(&tmpdir("no-orch"), CONFIG);
    let (sh, r) = shed_happy(&c);
    for issue in ["-", "12"] {
        let (exit, out) = run(&c, &r, &["spawn", "sendmeter", issue, "orch"]);
        assert_eq!(exit, 3, "{out}");
        assert_eq!(
            out.trim(),
            "REFUSED NO_ORCH: herdsman drives fleets itself; see herdsman timer"
        );
        assert!(sh.methods().is_empty() && !r.called("herdr"), "{:?}", sh.methods());
    }
    assert!(read(&c.state).unwrap_or_default().is_empty());
    assert_eq!(run(&c, &r, &["spawn", "sendmeter", "-", "impl"]).0, 1);
    assert_eq!(run(&c, &r, &["spawn", "sendmeter", "12", "boss"]).0, 1);
    assert!(sh.all("run").is_empty());
}

#[test]
fn fix_round_brief_carries_fail_findings() {
    let d = tmpdir("fix-brief");
    let c = ctx(&d, CONFIG);
    let w = wt(&c).to_string_lossy().into_owned();
    let d0 = json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12", "worktree": w});
    append(&c.state, &Event::new(10, "sendmeter", Some(12), "spawn", d0)).unwrap();
    let v = json!({"verdict": "FAIL", "sha": "1".repeat(40), "body": "FAIL\n1. blocking: src/x.rs:3 off by one"});
    append(&c.state, &Event::new(20, "sendmeter", Some(12), "verdict", v)).unwrap();
    let (_sh, r) = shed_happy(&c);
    let (exit, out) = spawn(&c, &r, &["--supersede"]);
    assert_eq!(exit, 0, "{out}");
    let brief = std::fs::read_to_string(wt(&c).join(".brief.md")).unwrap();
    assert!(
        brief.contains("Review findings to fix") && brief.contains("src/x.rs:3 off by one"),
        "{brief}"
    );
}

#[test]
fn a_new_lane_brief_carries_the_issue_decisions() {
    let d = tmpdir("decision-brief");
    let c = ctx(&d, CONFIG);
    let dec = json!({"id": "q3", "choice": "per API key", "by": "owner"});
    append(&c.state, &Event::new(10, "sendmeter", Some(12), "decision", dec)).unwrap();
    let (_sh, r) = shed_happy(&c);
    let (exit, out) = spawn(&c, &r, &[]);
    assert_eq!(exit, 0, "{out}");
    let brief = std::fs::read_to_string(wt(&c).join(".brief.md")).unwrap();
    assert!(
        brief.contains("sendmeter #12 decision q3 by owner: per API key"),
        "{brief}"
    );
}

// --- #13: herdr start timeout under load ---

const TIMEOUT: &str = r#"{"error":{"code":"timeout","message":"timed out waiting for agent startup"}}"#;
const PANE_HERMES: &str =
    r#"{"result":{"pane":{"pane_id":"w9:p1","workspace_id":"w9","agent":"hermes","agent_status":"idle"}}}"#;
const PANE_SHELL: &str = r#"{"result":{"pane":{"pane_id":"w9:p1","workspace_id":"w9","agent":null}}}"#;

#[test]
fn start_timeout_with_running_harness_registers_lane() {
    let d = tmpdir("timeout-adopt");
    let c = ctx(&d, CONFIG);
    let r = happy(&c, NO_AGENTS)
        .once_err("herdr agent start", 1, TIMEOUT)
        .once("herdr pane get w9:p1", 0, PANE_HERMES)
        .once("herdr agent rename w9:p1 sendmeter-impl-12", 0, "{}");
    let (exit, out) = spawn(&c, &r, &[]);
    assert_eq!(exit, 0, "{out} {:?}", r.calls.borrow());
    assert!(
        r.called("herdr agent rename w9:p1 sendmeter-impl-12"),
        "{:?}",
        r.calls.borrow()
    );
    let ev = read(&c.state).unwrap();
    let s = ev.iter().find(|e| e.kind == "spawn").expect("spawn logged");
    assert_eq!(
        (s.str("agent"), s.str("pane")),
        (Some("sendmeter-impl-12"), Some("w9:p1"))
    );
    assert_eq!(s.data["confirmed"], true);
    let (_, st) = run(&c, &Fake::default(), &["status"]);
    assert!(st.contains("sendmeter-impl-12"), "{st}");
}

/// #359: a pane that never confirms within the window is closed and started once more on a fresh pane; a second
/// failure closes that one too: no stale pane, nothing logged.
#[test]
fn start_timeout_without_harness_closes_pane_and_retries_once() {
    let d = tmpdir("timeout-none");
    let c = ctx(&d, CONFIG);
    let r = happy(&c, NO_AGENTS)
        .once_err("herdr agent start", 1, TIMEOUT)
        .once_err("herdr agent start", 1, TIMEOUT)
        .once("herdr pane get w9:p1", 0, PANE_SHELL)
        .once(
            "herdr pane split w9:p1",
            0,
            r#"{"result":{"pane":{"pane_id":"w9:p2"}}}"#,
        );
    let (exit, out) = spawn(&c, &r, &[]);
    assert_eq!(exit, 1, "{out}");
    assert_eq!(r.count("herdr agent start"), 2, "retried once");
    assert!(r.called("herdr pane close w9:p1") && r.called("herdr pane close w9:p2"));
    assert_eq!(r.count("sleep 5"), 2 * 24, "a 120 s window each, never one read");
    assert!(!r.called("herdr agent rename"));
    assert!(
        read(&c.state)
            .unwrap()
            .iter()
            .all(|e| matches!(e.kind.as_str(), "spawn-started" | "intent" | "failed")), // #509: its try
        "nothing but the intent (#376) logged"
    );
}

/// #718: a pane whose title changes on every read (a busy spinner) but never confirms still gives up at 120 s: a
/// title change never extends confirm's window.
#[test]
fn a_title_that_changes_every_read_never_extends_the_confirm_window() {
    let d = tmpdir("timeout-spinner");
    let c = ctx(&d, CONFIG);
    let mut r = happy(&c, NO_AGENTS)
        .once_err("herdr agent start", 1, TIMEOUT)
        .once_err("herdr agent start", 1, TIMEOUT)
        .once(
            "herdr pane split w9:p1",
            0,
            r#"{"result":{"pane":{"pane_id":"w9:p2"}}}"#,
        );
    for i in 0..200 {
        let pane = json!({"result": {"pane": {"pane_id": "w9:p1", "agent": null, "terminal_title": format!("⠋ {i}")}}});
        r = r.once("herdr pane get w9:p", 0, &pane.to_string());
    }
    let (exit, out) = spawn(&c, &r, &[]);
    assert_eq!(exit, 1, "{out}");
    assert_eq!(r.count("herdr agent start"), 2, "retried once");
    assert_eq!(r.count("sleep 5"), 2 * 24, "each confirm gives up at 120 s");
    assert!(r.called("herdr pane close w9:p1") && r.called("herdr pane close w9:p2"));
}

/// #359: the retried start succeeds: the lane runs on the fresh pane.
#[test]
fn a_never_confirming_pane_is_replaced_by_one_more_start() {
    let d = tmpdir("timeout-retry");
    let c = ctx(&d, CONFIG);
    let r = happy(&c, NO_AGENTS).once_err("herdr agent start", 1, TIMEOUT).once(
        "herdr pane split w9:p1",
        0,
        r#"{"result":{"pane":{"pane_id":"w9:p2"}}}"#,
    );
    let (exit, out) = spawn(&c, &r, &[]);
    assert_eq!(exit, 0, "{out} {:?}", r.calls.borrow());
    assert!(r.called("herdr pane close w9:p1"));
    let ev = read(&c.state).unwrap();
    assert_eq!(
        ev.iter().find(|e| e.kind == "spawn").unwrap().str("pane"),
        Some("w9:p2")
    );
}

/// #359: Claude's startup title arrives 40 s after its start gave up: the pane is confirmed, named and adopted.
#[test]
fn a_late_startup_title_confirms_and_adopts_the_lane() {
    let d = tmpdir("late-title");
    let c = ctx(&d, &CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\""));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let pane = |title: &str| {
        json!({"result": {"pane": {"pane_id": "w9:p1", "agent": "claude", "terminal_title": title}}}).to_string()
    };
    let mut r = happy(&c, NO_AGENTS).once_err("herdr agent start", 1, TIMEOUT);
    for _ in 0..8 {
        r = r.once("herdr pane get w9:p1", 0, &pane("zsh"));
    }
    let r = r
        .once("herdr pane get w9:p1", 0, &pane("✳ Claude Code"))
        .reply("herdr agent rename w9:p1 sendmeter-impl-12", "{}");
    let (exit, out) = spawn(&c, &r, &[]);
    assert_eq!(exit, 0, "{out} {:?}", r.calls.borrow());
    assert_eq!(r.count("sleep 5"), 8, "confirmed at 40 s");
    assert_eq!(r.count("herdr agent start"), 1);
    assert!(!r.called("herdr pane close"), "{:?}", r.calls.borrow());
    let rename = r.index("herdr agent rename").unwrap();
    assert!(
        rename
            > r.calls
                .borrow()
                .iter()
                .rposition(|c| c.starts_with("herdr pane get"))
                .unwrap()
    );
    let ev = read(&c.state).unwrap();
    let a = ev
        .iter()
        .find(|e| e.kind == "spawn-adopted")
        .expect("spawn-adopted logged");
    assert_eq!(a.str("pane"), Some("w9:p1"));
    assert_eq!(
        ev.iter().find(|e| e.kind == "spawn").unwrap().str("pane"),
        Some("w9:p1")
    );
}

/// #359: `agent_launch_pending` on the rename waits and retries; it is never a failure.
#[test]
fn agent_launch_pending_on_rename_is_retried_then_succeeds() {
    let d = tmpdir("launch-pending");
    let c = ctx(&d, CONFIG);
    let pending =
        r#"{"error":{"code":"agent_launch_pending","message":"agent name cannot change while startup is pending"}}"#;
    let r = happy(&c, NO_AGENTS)
        .once_err("herdr agent start", 1, TIMEOUT)
        .once("herdr pane get w9:p1", 0, PANE_HERMES)
        .once_err("herdr agent rename w9:p1 sendmeter-impl-12", 1, pending)
        .once_err("herdr agent rename w9:p1 sendmeter-impl-12", 1, pending)
        .reply("herdr agent rename w9:p1 sendmeter-impl-12", "{}");
    let (exit, out) = spawn(&c, &r, &[]);
    assert_eq!(exit, 0, "{out} {:?}", r.calls.borrow());
    assert_eq!(r.count("herdr agent rename"), 3);
    assert!(!r.called("herdr pane close"));
    let ev = read(&c.state).unwrap();
    assert!(ev.iter().any(|e| e.kind == "spawn-adopted"));
    assert_eq!(ev.iter().find(|e| e.kind == "spawn").unwrap().data["confirmed"], true);
}

/// herdr-only (#721): a spawn on herdr refused `code`, nothing created or logged.
fn herdr_refused(c: &herdsman::Ctx, r: &Fake, code: &str) -> String {
    let before = read(&c.state).unwrap().len();
    let (exit, out) = spawn(c, r, &[]);
    assert_eq!(exit, 3, "{out} {:?}", r.calls.borrow());
    assert!(out.contains(&format!("REFUSED {code}")), "{out}");
    assert!(
        !r.called("herdr worktree") && !r.called("herdr agent start"),
        "{:?}",
        r.calls.borrow()
    );
    assert_eq!(read(&c.state).unwrap().len(), before, "nothing logged");
    out
}

/// herdr-only (#721): an agent whose herdr start timed out has no name; shed terminals always have one.
#[test]
fn lane_live_names_pane_of_unnamed_agent() {
    let d = tmpdir("dup-unnamed");
    let c = ctx(&d, CONFIG);
    let w = wt(&c).to_string_lossy().into_owned();
    let list = json!({"result": {"agents": [{"agent_status": "working", "cwd": w, "pane_id": "w63T:p1"}]}}).to_string();
    let out = herdr_refused(&c, &happy(&c, &list), "LANE_LIVE");
    assert!(out.contains("w63T:p1"), "{out}");
}

#[test]
fn adopt_records_running_lane() {
    let d = tmpdir("adopt");
    let c = ctx(&d, CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
    sh.lane("stray", "idle", "/elsewhere");
    sh.set("stray", |t| t["labels"]["harness"] = json!("hermes"));
    let r = Fake::default();
    let (exit, out) = run(&c, &r, &["adopt", "sendmeter", "12", "impl", "--pane", "stray"]);
    assert_eq!(exit, 0, "{out} {:?}", r.calls.borrow());
    assert!(sh.called("get", "stray"), "{:?}", sh.methods());
    let ev = read(&c.state).unwrap();
    let s = ev.iter().find(|e| e.kind == "spawn").expect("spawn logged");
    assert_eq!(
        (s.str("agent"), s.str("pane")),
        (Some("sendmeter-impl-12"), Some("stray"))
    );
    assert_eq!(s.data["adopted"], true);
    let (_, st) = run(&c, &Fake::default(), &["status"]);
    assert!(st.contains("sendmeter-impl-12"), "{st}");
    // A terminal with no harness running (it exited) is not a lane.
    sh.exit("stray");
    assert_eq!(
        run(&c, &r, &["adopt", "sendmeter", "13", "impl", "--pane", "stray"]).0,
        1
    );
    assert!(read(&c.state).unwrap().iter().all(|e| e.issue != Some(13)));
}

// --- #107: no orphan lanes ---

#[test]
fn host_busy_creates_no_worktree_dir() {
    let d = tmpdir("host-busy-nothing");
    let c = ctx(&d, CONFIG);
    let (sh, r) = shed_happy(&c);
    let r = r.once("sysctl -n vm.loadavg", 0, "{ 26.90 3.00 2.00 }\n");
    std::fs::remove_dir_all(wt(&c)).unwrap();
    assert_refused(&c, &sh, &r, &[], "HOST_BUSY");
    assert!(!wt(&c).exists(), "no worktree dir");
}

/// #107: a kick that is never confirmed (every wait times out, the screen shows no context) still logs the
/// running lane's spawn, unconfirmed; the spawn exits 4 (spawned, prompt not confirmed).
#[test]
fn kick_failure_after_start_still_logs_unconfirmed_spawn() {
    let d = tmpdir("kick-fails");
    let c = ctx(&d, CONFIG);
    let (sh, r) = shed_happy(&c);
    sh.wait_ends(IMPL, "timeout");
    let (exit, out) = spawn(&c, &r, &[]);
    assert_eq!(exit, 4, "{out}");
    assert!(sh.has(IMPL), "the started lane is kept");
    let ev = read(&c.state).unwrap();
    let s = ev.iter().find(|e| e.kind == "spawn").expect("spawn logged");
    assert_eq!(
        (s.str("agent"), s.data["confirmed"].clone()),
        (Some("sendmeter-impl-12"), json!(false))
    );
    assert_eq!(r.count("sleep 10"), 2, "three attempts");
}

const TAKEN: &str = r#"{"error":{"code":"agent_name_taken","message":"agent name already in use"}}"#;
const PANE_P7: &str = r#"{"result":{"pane":{"pane_id":"w9:p7","workspace_id":"w9","agent":"hermes"}}}"#;

/// The guard's `agent list` sees nothing; after `agent_name_taken` the next one shows the orphan in `cwd`.
fn name_taken(c: &herdsman::Ctx, cwd: &str) -> Fake {
    let list = agents(&[("sendmeter-impl-12", "working", cwd, "w9:p7")]);
    happy(c, &list)
        .once("herdr agent list", 0, NO_AGENTS)
        .once_err("herdr agent start", 1, TAKEN)
        .once("herdr pane get w9:p7", 0, PANE_P7)
        .reply("herdr agent rename w9:p7 sendmeter-impl-12", "{}")
}

#[test]
fn agent_name_taken_in_lane_worktree_adopts_the_orphan() {
    let d = tmpdir("taken-adopt");
    let c = ctx(&d, CONFIG);
    let r = name_taken(&c, wt(&c).to_str().unwrap());
    let (exit, out) = spawn(&c, &r, &[]);
    assert_eq!(exit, 0, "{out} {:?}", r.calls.borrow());
    let ev = read(&c.state).unwrap();
    let s = ev.iter().find(|e| e.kind == "spawn").expect("spawn logged");
    assert_eq!(
        (s.str("agent"), s.str("pane")),
        (Some("sendmeter-impl-12"), Some("w9:p7"))
    );
    assert_eq!(s.data["adopted"], true);
}

#[test]
fn agent_name_taken_elsewhere_is_refused_as_before() {
    let d = tmpdir("taken-elsewhere");
    let c = ctx(&d, CONFIG);
    let r = name_taken(&c, "/elsewhere");
    let (exit, out) = spawn(&c, &r, &[]);
    assert_eq!(exit, 1, "{out}");
    assert!(!r.called("herdr agent rename"));
    assert!(
        read(&c.state)
            .unwrap()
            .iter()
            .all(|e| matches!(e.kind.as_str(), "spawn-started" | "intent" | "failed")), // #509: its try
        "nothing but the intent (#376) logged"
    );
}

#[test]
fn named_orphan_in_worktree_is_adopted_not_lane_live() {
    let d = tmpdir("dup-orphan");
    let c = ctx(&d, CONFIG);
    let (sh, r) = shed_happy(&c);
    sh.lane(IMPL, "working", wt(&c).to_str().unwrap());
    let (exit, out) = spawn(&c, &r, &[]);
    assert_eq!(exit, 0, "{out}");
    assert!(sh.all("run").is_empty(), "nothing started: {:?}", sh.methods());
    let ev = read(&c.state).unwrap();
    assert_eq!(ev.iter().filter(|e| e.kind == "spawn").count(), 1);
    assert_eq!((ev[0].str("pane"), &ev[0].data["adopted"]), (Some(IMPL), &json!(true)));
}

/// #128: a supersede closes the old lane's pane once the new one is confirmed; a working old lane is untouched.
#[test]
fn supersede_closes_old_pane_after_confirm_unless_working() {
    for working in [false, true] {
        let d = tmpdir("supersede-old-pane");
        let c = ctx(&d, CONFIG);
        let w = wt(&c).to_string_lossy().into_owned();
        // The old lane's terminal is `old-pane`: renamed and working elsewhere (a listed lane), or left over.
        let d0 = json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12", "worktree": w, "pane": "old-pane"});
        append(&c.state, &Event::new(10, "sendmeter", Some(12), "spawn", d0)).unwrap();
        let (sh, r) = shed_happy(&c);
        if working {
            sh.lane("old-pane", "working", "/elsewhere");
        } else {
            sh.add("old-pane", json!({})); // no `fleet` label: no live lane
        }
        let (exit, out) = spawn(&c, &r, &["--supersede"]);
        assert_eq!(exit, 0, "{out} {:?}", r.calls.borrow());
        let closed = read(&c.state).unwrap().iter().any(|e| e.kind == "lane-closed");
        let seen = sh.lock().seen.clone();
        let close = seen.iter().position(|s| s.0 == "kill" && s.1["name"] == "old-pane");
        if working {
            assert!(close.is_none() && !closed && sh.has("old-pane"), "{seen:?}");
        } else {
            let confirmed = seen.iter().rposition(|s| s.0 == "wait" && s.1["name"] == IMPL).unwrap();
            assert!(
                close.expect("old pane closed") > confirmed,
                "after the new lane is confirmed"
            );
            assert!(closed && !sh.has("old-pane"));
        }
    }
}

// --- #182: a failed spawn leaves no zombie pane ---

#[test]
fn a_failed_rename_closes_the_pane_so_the_next_spawn_is_not_lane_live() {
    let d = tmpdir("rename-fail");
    let c = ctx(&d, CONFIG);
    let r = happy(&c, NO_AGENTS)
        .once_err("herdr agent start", 1, TIMEOUT)
        .once("herdr pane get w9:p1", 0, PANE_HERMES)
        .once_err("herdr agent rename w9:p1 sendmeter-impl-12", 1, "agent rename failed");
    let (exit, out) = spawn(&c, &r, &[]);
    assert_eq!(exit, 1, "{out}");
    assert!(r.called("herdr pane close w9:p1"), "{:?}", r.calls.borrow());
    assert!(
        read(&c.state)
            .unwrap()
            .iter()
            .all(|e| matches!(e.kind.as_str(), "spawn-started" | "intent" | "failed")), // #509: its try
        "nothing but the intent (#376) logged"
    );
    // The pane is gone: the next pass spawns, no LANE_LIVE.
    let (exit, out) = spawn(&c, &happy(&c, NO_AGENTS), &[]);
    assert_eq!(exit, 0, "{out}");
}

/// herdr-only (#721): an unnamed idle pane (a timed-out herdr start); a named one is adopted instead.
#[test]
fn lane_live_on_an_idle_unlogged_pane_names_its_age_and_the_way_out() {
    let d = tmpdir("dup-stale");
    let c = ctx(&d, CONFIG);
    let d = json!({"code": "CI_NOT_GREEN", "reason": "r"});
    append(
        &c.state,
        &Event::new(c.now - 3 * 3600, "sendmeter", Some(12), "blocked", d),
    )
    .unwrap();
    let w = wt(&c).to_string_lossy().into_owned();
    let list = json!({"result": {"agents": [{"agent_status": "idle", "cwd": w, "pane_id": "w6N5:p1"}]}}).to_string();
    let out = herdr_refused(&c, &happy(&c, &list), "LANE_LIVE");
    assert!(
        out.contains("stale pane w6N5:p1 (idle 3h): herdsman adopt or --supersede"),
        "{out}"
    );
}

/// #168: `agent_pane_busy` closes a finished lane's pane and retries once on a fresh one; no error.
#[test]
fn agent_pane_busy_closes_a_finished_lane_pane_and_retries_once() {
    let d = tmpdir("pane-busy");
    let c = ctx(&d, CONFIG);
    let d0 = json!({"role": "impl", "agent": "sendmeter-impl-11", "worktree": "/elsewhere", "pane": "w9:p1"});
    append(&c.state, &Event::new(10, "sendmeter", Some(11), "spawn", d0)).unwrap();
    let busy = r#"{"error":{"code":"agent_pane_busy","message":"agent target pane w9:p1 is not an available shell"}}"#;
    let r = happy(&c, NO_AGENTS)
        .once_err("herdr agent start", 1, busy)
        .reply("herdr pane close w9:p1", "{}")
        .once(
            "herdr pane split w9:p1",
            0,
            r#"{"result":{"pane":{"pane_id":"w9:p2"}}}"#,
        );
    let (exit, out) = spawn(&c, &r, &[]);
    assert_eq!(exit, 0, "{out} {:?}", r.calls.borrow());
    assert_eq!(r.count("herdr agent start"), 2, "retried once");
    let ev = read(&c.state).unwrap();
    let b = ev.iter().find(|e| e.kind == "pane-busy").expect("logged");
    assert_eq!((b.str("pane"), b.data["closed"].clone()), (Some("w9:p1"), json!(true)));
    let s = ev.iter().rfind(|e| e.kind == "spawn").unwrap();
    assert_eq!(s.str("pane"), Some("w9:p2"));
}

// --- #463: a start that fails shows its screen; a known first-start prompt is answered ---

const NOT_READY: &str = r#"{"error":{"code":"agent_not_ready","message":"agent sendmeter-impl-12 is blocked during startup and is not ready for prompts"}}"#;

/// A one-entry fallback chain, so a failed start logs `spawn-failed` (its `error` the printed one).
fn chain(harness: &str) -> String {
    let cfg = CONFIG.replace("harness  =  \"hermes\"", &format!("harness = \"{harness}\""));
    cfg.replace(
        "model = \"opus\"",
        &format!("model = \"opus\"\nfallback = [\"{harness}\"]"),
    )
}

/// The error a failed start logged.
fn failed(c: &herdsman::Ctx) -> String {
    let ev = read(&c.state).unwrap();
    let f = ev
        .iter()
        .find(|e| e.kind == "spawn-failed")
        .expect("spawn-failed logged");
    f.str("error").unwrap().to_string()
}

/// A start that fails: the pane's last 40 lines (secrets redacted) are in the error and the `spawn-failed` event.
#[test]
fn a_start_failure_records_the_pane_screen() {
    let d = tmpdir("not-ready-screen");
    let c = ctx(&d, &chain("hermes"));
    let token = "ghp_0123456789abcdefghijklmnopqrstuv";
    let mut screen: String = (1..=50).map(|i| format!("old line {i}\n")).collect();
    screen += &format!("Hermes setup: paste your key\nkey: {token}\n");
    let r = happy(&c, NO_AGENTS)
        .once_err("herdr agent start", 1, NOT_READY)
        .once("herdr pane read w9:p1", 0, &screen);
    let (exit, out) = spawn(&c, &r, &[]);
    assert_eq!(exit, 1, "{out}");
    assert!(r.called("herdr pane close w9:p1"));
    let err = failed(&c);
    assert!(err.contains("agent_not_ready"), "{err}");
    assert!(err.contains("Hermes setup: paste your key"), "{err}");
    assert!(!err.contains(token) && err.contains("key: •••"), "redacted: {err}");
    assert!(
        !err.contains("old line 12\n") && err.contains("old line 13\n"),
        "the last 40 lines: {err}"
    );
}

fn claude_ctx(name: &str) -> (PathBuf, herdsman::Ctx) {
    let d = tmpdir(name);
    let c = ctx(&d, &chain("claude"));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    (d, c)
}

const TRUST: &str = " Accessing workspace:\n\n /tmp/cooee/design-1\n\n Quick safety check: Is this a project you created or one you trust?\n\n ❯ 1. Yes, I trust this folder\n   2. No, exit\n\n Enter to confirm · Esc to cancel\n";
/// #476: Claude Code's unnumbered trust menu, as cooee #10's pane showed it.
const TRUST_CURSOR: &str = " Accessing workspace:

 /Users/jkmacmini/.herdr/worktrees/cooee/issue-10

 Quick safety check: Is this a project you created or one you trust? (Like your own code, a well-known open source
 project, or work from your team). If not, take a moment to review what's in this folder first.

 Claude Code'll be able to read, edit, and execute files here.

 Security guide

 ❯ No, exit
   Yes, I trust this folder

 Enter to confirm · Esc to cancel
";
/// #484: the raw `herdr pane read --source visible` of a failed start: the shell's own `❯` prompts above the menu.
const TRUST_LIVE: &str = include_str!("golden/spawn/trust-live.txt");
const TRUST_1X: &str = "╭──────────────────────────────────────────╮\n│ Do you trust the files in this folder?   │\n│                                          │\n│ /tmp/cooee/design-1                      │\n│                                          │\n│ ❯ 1. Yes, proceed                        │\n│   2. No, exit                            │\n╰──────────────────────────────────────────╯\n   Enter to confirm · Esc to exit\n";
const BYPASS: &str = " WARNING: Claude Code running in Bypass Permissions mode\n\n By proceeding, you accept all responsibility?\n\n ❯ 1. No, exit\n   2. Yes, I accept\n\n Enter to confirm · Esc to cancel\n";

/// A new folder's trust dialog (and the bypass warning, "Yes" second) is answered, then the lane confirms and runs.
#[test]
fn a_known_first_start_prompt_is_answered_and_the_spawn_confirms() {
    for (screen, prompt, keys) in [
        (TRUST, "folder-trust", &["1"][..]),
        (TRUST_1X, "folder-trust", &["1"]),
        (BYPASS, "bypass-permissions", &["2"]),
        (TRUST_CURSOR, "folder-trust", &["Down", "Enter"]),
        (TRUST_LIVE, "folder-trust", &["Down", "Enter"]),
    ] {
        let (_d, c) = claude_ctx("first-start");
        let up =
            json!({"result": {"pane": {"pane_id": "w9:p1", "agent": "claude", "terminal_title": "✳ Claude Code"}}});
        let r = happy(&c, NO_AGENTS)
            .once_err("herdr agent start", 1, NOT_READY)
            .once("herdr pane read w9:p1", 0, screen)
            .once("herdr pane get w9:p1", 0, &up.to_string())
            .reply("herdr agent rename w9:p1 sendmeter-impl-12", "{}");
        let (exit, out) = spawn(&c, &r, &[]);
        assert_eq!(exit, 0, "{out} {:?}", r.calls.borrow());
        let sent: Vec<String> = r
            .calls
            .borrow()
            .iter()
            .filter_map(|c| c.strip_prefix("herdr pane send-keys w9:p1 ").map(String::from))
            .collect();
        assert_eq!(sent, keys, "{:?}", r.calls.borrow());
        let last = format!("herdr pane send-keys w9:p1 {}", keys[keys.len() - 1]);
        assert!(
            r.index(&last) < r.index("herdr pane get w9:p1"),
            "answered, then confirmed"
        );
        assert!(!r.called("herdr pane close"), "{:?}", r.calls.borrow());
        let ev = read(&c.state).unwrap();
        let sp = ev
            .iter()
            .find(|e| e.kind == "startup-prompt")
            .expect("startup-prompt logged");
        assert_eq!((sp.str("prompt"), sp.str("pane")), (Some(prompt), Some("w9:p1")));
        let s = ev.iter().find(|e| e.kind == "spawn").expect("spawn logged");
        assert_eq!(
            (s.str("pane"), s.data["confirmed"].clone()),
            (Some("w9:p1"), json!(true))
        );
    }
}

/// A prompt herdsman does not know is never answered: the spawn fails with that screen in its error.
#[test]
fn an_unknown_first_start_prompt_fails_with_its_screen() {
    let (_d, c) = claude_ctx("first-start-unknown");
    let screen = " Choose the text style that looks best with your terminal?\n\n ❯ 1. Dark mode\n   2. Light mode\n";
    let r = happy(&c, NO_AGENTS)
        .once_err("herdr agent start", 1, NOT_READY)
        .once("herdr pane read w9:p1", 0, screen);
    let (exit, out) = spawn(&c, &r, &[]);
    assert_eq!(exit, 1, "{out}");
    let err = failed(&c);
    assert!(
        err.contains("agent_not_ready") && err.contains("❯ 1. Dark mode"),
        "{err}"
    );
    assert!(!r.called("herdr pane send-keys"), "{:?}", r.calls.borrow());
    assert!(r.called("herdr pane close w9:p1"));
    assert!(read(&c.state).unwrap().iter().all(|e| e.kind != "startup-prompt"));
}
