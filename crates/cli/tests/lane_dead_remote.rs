mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::runner::{Out, Runner};
use serde_json::json;

const NOW: u64 = 1_000_000;
const WT: &str = "/home/me/.herdr/worktrees/sendmeter/issue-12";

fn setup(name: &str, limits: &str) -> (drover::Ctx, FakeShed) {
    spawned_ago(name, limits, 900)
}

fn spawned_ago(name: &str, limits: &str, ago: u64) -> (drover::Ctx, FakeShed) {
    let d = tmpdir(name);
    let base = CONFIG
        .replace("harness  =  \"claude\"", "harness = \"claude\"")
        .replace("[limits]\n", &format!("[limits]\n{limits}"));
    let toml = format!("{base}\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n");
    let c = ctx(&d, &toml);
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "bazzite": {"tags": ["os:linux"], "home": "/home/me", "probed": NOW, "seen": NOW, "up": true},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let sh = FakeShed::attached(&c);
    let d = json!({"role": "impl", "agent": IMPL, "harness": "claude", "pane": IMPL, "workspace": "",
                   "worktree": WT, "branch": "issue-12", "host": "bazzite", "machine": "bazzite"});
    append(&c.state, &Event::new(NOW - ago, "sendmeter", Some(12), "spawn", d)).unwrap();
    (c, sh)
}

fn main_host() -> Fake {
    Fake::default()
        .reply("sysctl -n vm.loadavg", "{ 0.4 1 1 }")
        .reply("sysctl -n hw.ncpu", "4")
        .reply("gh issue view", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply("git ", "")
        .reply("which shed", "/opt/shed/bin/shed\n")
}

fn dead(c: &drover::Ctx) -> usize {
    read(&c.state).unwrap().iter().filter(|e| e.kind == "lane-dead").count()
}

struct Unanswered<'a>(&'a Fake, i32);

impl Runner for Unanswered<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.run_in(prog, args, "")
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        let there = prog == "ssh" && args[0] != "-G" && args.iter().any(|a| a == "bazzite");
        let shed = args.last().is_some_and(|l| l.contains("'shed' 'connect'"));
        if there && (shed || self.1 == 124) {
            return Ok(Out {
                code: self.1,
                stderr: "shed: no answer".into(),
                ..Out::default()
            });
        }
        self.0.run_in(prog, args, input)
    }
    fn sleep(&self, _: u64) {}
}

#[test]
fn a_pass_that_cannot_read_a_remote_hosts_sessions_logs_no_lane_dead() {
    let (c, _sh) = setup("ldr-unread", "");
    let f = main_host();
    for _ in 0..2 {
        let (code, out) = run(&c, &Unanswered(&f, 1), &["check"]);
        assert_eq!(code, 0, "{out}");
    }
    assert_eq!(
        dead(&c),
        0,
        "a host whose session list failed says nothing about its lanes"
    );
}

#[test]
fn a_tick_that_skips_a_remote_host_over_budget_logs_no_lane_dead() {
    let (c, _sh) = setup("ldr-budget", "slow_secs = 0\n");
    let f = main_host();
    let (code, out) = run(&c, &Unanswered(&f, 124), &["tick"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("tick: bazzite: over budget; skipped for the rest of this pass"),
        "{out}"
    );
    assert_eq!(
        dead(&c),
        0,
        "a host the pass skipped says nothing about its lanes: {out}"
    );
}

#[test]
fn a_lane_logged_dead_and_then_seen_running_again_is_logged_dead_again_at_its_real_death() {
    let (c, _sh) = setup("ldr-back", "");
    let (f, bazzite) = (main_host(), FakeShed::new());
    let r = OverSsh {
        f: &f,
        hosts: vec![("bazzite", &bazzite)],
    };
    assert_eq!(run(&c, &r, &["check"]).0, 0);
    assert_eq!(dead(&c), 1);
    bazzite.lane(IMPL, "working", WT);
    look(&c, &r);
    for _ in 0..2 {
        let (code, out) = run(&c, &r, &["check"]);
        assert_eq!(code, 0, "{out}");
    }
    bazzite.exit(IMPL);
    look(&c, &r);
    assert_eq!(run(&c, &r, &["check"]).0, 0);
    assert_eq!(dead(&c), 2, "a later real death is logged again");
}

#[test]
fn a_lane_whose_session_really_exited_on_an_answering_host_is_dead() {
    let (c, _sh) = setup("ldr-exited", "");
    let (f, bazzite) = (main_host(), FakeShed::new());
    bazzite.lane(IMPL, "working", WT);
    bazzite.exit(IMPL);
    let r = OverSsh {
        f: &f,
        hosts: vec![("bazzite", &bazzite)],
    };
    for _ in 0..2 {
        assert_eq!(run(&c, &r, &["check"]).0, 0);
    }
    assert_eq!(dead(&c), 1);
}

#[test]
fn a_probe_that_times_out_once_ends_no_fresh_lane() {
    let (c, _sh) = spawned_ago("ldr-timeout", "", 120);
    let (code, out) = run(&c, &Unanswered(&main_host(), 124), &["check"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(dead(&c), 0, "a timed-out probe is unknown: {out}");
}

#[test]
fn a_fresh_lane_its_answering_host_does_not_list_yet_is_dead_only_after_its_grace() {
    let (mut c, _sh) = spawned_ago("ldr-grace", "", 120);
    let (f, bazzite) = (main_host(), FakeShed::new());
    let r = OverSsh {
        f: &f,
        hosts: vec![("bazzite", &bazzite)],
    };
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(dead(&c), 0, "a lane two minutes old is not judged dead: {out}");
    c.now = NOW + 600;
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        dead(&c),
        1,
        "past its grace, an answering host without it means gone: {out}"
    );
}
