mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::runner::{Out, Runner};
use serde_json::json;
use std::time::Duration;

const NOW: u64 = 1_000_000;
const CONNECT: u64 = 10;

struct Dead {
    f: Fake,
    calls: Shared<u64>,
}

impl Dead {
    fn new() -> Self {
        Dead {
            f: local(),
            calls: Shared::new(0),
        }
    }
    fn waited(&self) -> u64 {
        self.calls.get() * CONNECT
    }
}

fn air(prog: &str, args: &[String]) -> bool {
    prog == "ssh" && args.first().is_none_or(|a| a != "-G") && args.iter().any(|a| a == "air")
}

impl Runner for Dead {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.run_for(prog, args, "", Duration::from_secs(60))
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.run_for(prog, args, input, Duration::from_secs(60))
    }
    fn run_for(&self, prog: &str, args: &[String], input: &str, _: Duration) -> anyhow::Result<Out> {
        if !air(prog, args) {
            return self.f.run_in(prog, args, input);
        }
        self.calls.set(self.calls.get() + 1);
        Ok(Out {
            code: 255,
            stderr: "ssh: connect to host jirathips-macbook-air port 22: Operation timed out\n".into(),
            ..Out::default()
        })
    }
    fn detach(&self, exe: &str, args: &[String], log: &std::path::Path) -> anyhow::Result<u32> {
        self.f.detach(exe, args, log)
    }
}

fn local() -> Fake {
    Fake::default()
        .reply(
            "herdr status",
            "client:\n  version: 0.9.5\n\nserver:\n  status: running\n  version: 0.9.5\n",
        )
        .reply("sysctl -n vm.loadavg", "{ 0.4 1 1 }")
        .reply("sysctl -n hw.ncpu", "4")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("git ", "")
        .reply("gh issue view", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply("which shed", "/opt/shed/bin/shed\n")
}

fn setup(name: &str, extra: &str) -> (drover::Ctx, FakeShed) {
    let base = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
    let other = base
        .split("[limits]")
        .next()
        .unwrap()
        .replace("# fleet config (test)", "")
        .replace("sendmeter", "cooee");
    let toml = format!("{base}\n{other}\n[hosts.mini]\n\n[hosts.air]\nmachine = \"air\"\n{extra}");
    let c = ctx(&tmpdir(name), &toml);
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "air": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    for n in 100..120u64 {
        let agent = format!("cooee-rev-{n}-r2");
        let d = json!({"role": "rev", "agent": agent, "pane": agent, "workspace": "",
                       "worktree": format!("/Users/me/.herdr/worktrees/cooee/{agent}"),
                       "branch": format!("issue-{n}"), "host": "air", "machine": "air"});
        append(&c.state, &Event::new(NOW - 900, "cooee", Some(n), "spawn", d)).unwrap();
    }
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let sh = FakeShed::attached(&c);
    (c, sh)
}

#[test]
fn a_kick_pays_one_connect_timeout_to_a_dead_host_with_twenty_old_lanes() {
    let (c, sh) = setup("down-kick", "");
    let r = Dead::new();
    let (code, out) = run(&c, &r, &["kick", "sendmeter", "12", "--why", "push", "--now"]);
    assert_eq!(code, 0, "{out}");
    assert!(sh.called("run", "sendmeter-impl-12"), "the kick still starts #12: {out}");
    assert!(r.calls.get() <= 1, "{} calls to air: {out}", r.calls.get());
    assert!(r.waited() <= CONNECT, "{} s simulated", r.waited());
}

#[test]
fn waiting_and_status_call_a_dead_host_once_then_not_while_its_mark_is_fresh() {
    for cmd in ["waiting", "status"] {
        let (c, _sh) = setup(&format!("down-{cmd}"), "");
        let r = Dead::new();
        let (code, out) = run(&c, &r, &[cmd]);
        assert_eq!(code, 0, "{cmd}: {out}");
        assert!(r.calls.get() <= 1, "{cmd}: {} calls to air: {out}", r.calls.get());
        let again = Dead::new();
        let c = drover::Ctx { now: c.now + 30, ..c };
        let (code, out) = run(&c, &again, &[cmd]);
        assert_eq!(code, 0, "{cmd}: {out}");
        assert_eq!(again.calls.get(), 0, "{cmd}: the shared offline mark is fresh: {out}");
    }
}

#[test]
fn a_disabled_host_with_old_lanes_gets_no_ssh_from_kick_tick_drive_waiting_or_status() {
    let (c, _sh) = setup("down-disabled", "enabled = false\n");
    let r = Dead::new();
    for args in [
        &["kick", "sendmeter", "12", "--why", "push", "--now"][..],
        &["tick"],
        &["drive"],
        &["waiting"],
        &["status"],
    ] {
        let (code, out) = run(&c, &r, args);
        assert_eq!(code, 0, "{args:?}: {out}");
        assert_eq!(r.calls.get(), 0, "{args:?}: {out}");
    }
}

#[test]
fn tick_ends_a_kick_past_its_limit_logs_it_and_still_runs_every_fleet() {
    let (c, _sh) = setup("down-kick-limit", "enabled = false\n");
    let (code, show) = run(&c, &Fake::default(), &["config", "show"]);
    assert_eq!(code, 0, "{show}");
    assert!(show.contains("kick_max_secs"), "{show}");
    lease_as(&c, "kick-cooee-7.lock", 4242, NOW - 900, 3600);
    let r = Dead::new();
    let r = Dead {
        f: r.f.reply("kill -0 4242", "").reply("ps -o etime= -p 4242", "15:00\n"),
        ..r
    };
    let (code, out) = run(&c, &r, &["tick"]);
    assert_eq!(code, 0, "{out}");
    assert!(r.f.count("kill -TERM -4242") == 1, "{:#?}", r.f.calls.borrow());
    let ev = read(&c.state).unwrap().to_vec();
    let ended = ev.iter().find(|e| e.kind == "kick-killed").expect("kick-killed logged");
    assert_eq!((ended.fleet.as_str(), ended.issue), ("cooee", Some(7)));
    assert!(lease_on(&c, "kick-cooee-7.lock").is_none());
    for f in ["sendmeter", "cooee"] {
        assert!(drover::drive::last_pass(&c, f).is_some(), "{f} had its pass: {out}");
    }
}

#[test]
fn a_second_kick_for_a_running_issue_exits_at_once_and_is_logged() {
    let (c, sh) = setup("down-kick-twice", "enabled = false\n");
    lease_as(&c, "kick-sendmeter-12.lock", 4242, c.now, 300);
    let r = local().reply("kill -0 4242", "").reply("ps -o etime= -p 4242", "00:01\n");
    let (code, out) = run(&c, &r, &["kick", "sendmeter", "12", "--why", "push", "--now"]);
    assert_eq!(code, 0, "{out}");
    assert!(!sh.called("run", "sendmeter-impl-12"), "the running kick does the work");
    let ev = read(&c.state).unwrap().to_vec();
    let busy: Vec<_> = ev.iter().filter(|e| e.kind == "kick-in-flight").collect();
    assert_eq!(busy.len(), 1, "{ev:?}");
    assert_eq!((busy[0].issue, busy[0].str("why")), (Some(12), Some("push")));
}

#[test]
fn doctor_shows_a_dead_host_as_one_line_with_how_long_it_is_down() {
    let (c, _sh) = setup("down-doctor", "");
    let r = Dead::new();
    assert_eq!(run(&c, &r, &["status"]).0, 0);
    let c = drover::Ctx { now: c.now + 600, ..c };
    let (_, out) = run(&c, &Dead::new(), &["doctor"]);
    let air: Vec<_> = out.lines().filter(|l| l.contains("host air")).collect();
    assert_eq!(air.len(), 1, "{out}");
    assert!(air[0].contains("down 10m"), "{out}");
}
