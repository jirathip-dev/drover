//! #254: cloud hosts with a fake recipe runner: on-demand start → address refresh → spawn, overflow after the local
//! hosts were held `overflow_after_minutes`, idle stop and `hosts destroy` with the wip save (aborted on a failed push
//! or an unreachable host), the budget
//! refusal and the 80 % warning, an idempotent ssh include, and the provider key never in argv or the log.
mod common;
use common::*;
use herdsman::config::Config;
use herdsman::events::{append, read, Event};
use herdsman::hosts::place;
use herdsman::runner::{Out, Runner};
use serde_json::json;
use std::collections::BTreeMap;

const NOW: u64 = 1_000_000;
const SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 cloud1 -- ";
const WT: &str = "/home/me/.herdr/worktrees/sendmeter/issue-12";
const KEY: &str = "sekrit-key-123";
const RECIPE: &str = r#"key_env = "FAKE_CLOUD_KEY"
start = "fakecloud start {name} {size}"
stop = "fakecloud stop {name}"
status = "fakecloud status {name}"
address = "fakecloud address {name}"
destroy = "fakecloud destroy {name}"
[rate_usd_hour]
small = 1.0
"#;

/// The `sh -c` line a recipe command runs as.
fn rc(verb: &str) -> String {
    format!("sh -c IFS= read -r K; export FAKE_CLOUD_KEY=\"$K\"; unset K; fakecloud {verb} cloud1")
}

/// mini (this host) and cloud1 (the fake recipe, $10 a month, 20 h a day, a `docker` hand tag); the key in accounts_env.
fn setup(name: &str) -> herdsman::Ctx {
    let dir = tmpdir(name);
    let env = dir.join("keys.env");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&env, format!("FAKE_CLOUD_KEY={KEY}\n")).unwrap();
    let base = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
    let hosts = "[hosts.mini]\n\n[hosts.cloud1]\nmachine = \"cloud1\"\ncloud = \"fake\"\nsize = \"small\"\n\
budget_usd_month = 10\nmax_hours_day = 20\ntags = [\"docker\"]\n";
    let cfg = format!("{base}\n[accounts]\naccounts_env = \"{}\"\n\n{hosts}", env.display());
    let c = ctx(&dir, &cfg);
    std::fs::create_dir_all(c.config.with_file_name("recipes")).unwrap();
    std::fs::write(c.config.with_file_name("recipes/fake.toml"), RECIPE).unwrap();
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "cloud1": {"tags": ["os:linux"], "home": "/home/me", "probed": NOW, "seen": NOW - 9000, "up": false,
                   "busy": NOW - 2000},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    c
}

fn ev(c: &herdsman::Ctx, ts: u64, kind: &str) {
    append(
        &c.state,
        &Event::new(ts, "herdsman", None, kind, json!({"host": "cloud1"})),
    )
    .unwrap();
}

/// mini at load/cpu `mini`; cloud1 reachable, idle, and its recipe answering.
fn fake(mini: f64) -> Fake {
    Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("herdr --machine cloud1 agent list", NO_AGENTS)
        .reply("sysctl -n vm.loadavg", &format!("{{ {} 1 1 }}", mini * 4.0))
        .reply("sysctl -n hw.ncpu", "4")
        .reply(&format!("{SSH}'cat' '/proc/loadavg'"), "0.4 1 1 1/9 42")
        .reply(&format!("{SSH}'nproc'"), "4")
        .reply(&rc("start"), "")
        .reply(&rc("status"), "running\n")
        .reply(&rc("address"), "ubuntu@10.0.0.7 2222\nssh-ed25519 AAAAC3fakekey\n")
        .reply(&rc("stop"), "")
}

/// Where a lane on #12 goes: the host's name, or the refusal (no start: `place` only picks).
fn placed(c: &herdsman::Ctx, r: &Fake, force: Option<&str>) -> Result<String, String> {
    let cfg = Config::load(&c.config).unwrap();
    match place(c, &cfg, r, &read(&c.state).unwrap(), ("sendmeter", 12, "impl"), force) {
        Ok(p) => {
            let p = p.unwrap();
            p.refusal
                .map_or(Ok(p.name), |(code, why)| Err(format!("{code}: {why}")))
        }
        Err(e) => Err(format!("{e:#}")),
    }
}

/// A fresh lane on cloud1 answers every step.
fn spawn_fake() -> Fake {
    let ws = r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#;
    fake(0.1)
        .reply("gh issue view 12", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply(&format!("{SSH}'gh' 'repo' 'clone' 'jirathip-dev/sendmeter'"), "")
        .reply(&format!("{SSH}'git' '-C'"), "")
        .reply(&format!("{SSH}'sh' '-c' 'mkdir"), "")
        .reply("herdr --machine cloud1 worktree create", ws)
        .reply(
            "herdr --machine cloud1 pane list --workspace w9",
            r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#,
        )
        .reply("herdr --machine cloud1 pane run w9:p1 export", "{}")
        .reply("herdr --machine cloud1 agent start sendmeter-impl-12", "{}")
        .reply("herdr --machine cloud1 agent wait sendmeter-impl-12", "{}")
}

#[test]
fn a_lane_only_the_cloud_fits_starts_it_refreshes_its_address_then_spawns() {
    let c = setup("cloud-start");
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    run(
        &c,
        &Fake::default(),
        &["queue", "sendmeter", "add", "12", "--needs", "docker"],
    );
    // A dry run never starts it.
    let r = spawn_fake();
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl", "--dry-run"]);
    assert!(!r.called(&rc("start")), "{code} {out}");
    let r = spawn_fake();
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    let i = |p: &str| r.index(p).unwrap_or_else(|| panic!("no {p}: {:?}", r.calls.borrow()));
    assert!(i(&rc("start")) < i(&rc("status")) && i(&rc("status")) < i(&rc("address")));
    assert!(i(&rc("address")) < i("herdr --machine cloud1 worktree create"));
    let inc = std::fs::read_to_string(c.home.join(".ssh/config.d/herdsman")).unwrap();
    assert!(
        inc.contains("Host cloud1\n  HostName 10.0.0.7\n  User ubuntu\n  Port 2222\n"),
        "{inc}"
    );
    let known = std::fs::read_to_string(c.home.join(".ssh/config.d/herdsman-cloud1.known")).unwrap();
    assert_eq!(known, "[10.0.0.7]:2222 ssh-ed25519 AAAAC3fakekey\n");
    let events = read(&c.state).unwrap();
    let kinds: Vec<_> = events
        .iter()
        .map(|e| e.kind.as_str())
        .filter(|k| k.starts_with("cloud-"))
        .collect();
    assert_eq!(kinds, ["cloud-start", "cloud-up"]);
    let s = events.iter().rfind(|e| e.kind == "spawn").unwrap();
    assert_eq!(s.str("host"), Some("cloud1"));
}

#[test]
fn a_stopped_cloud_host_takes_overflow_only_after_the_locals_were_held_long_enough() {
    let c = setup("cloud-overflow");
    // mini admits: mini (cloud1 is stopped and never looked at over SSH).
    let r = fake(0.1);
    assert_eq!(placed(&c, &r, None).as_deref(), Ok("mini"));
    assert!(!r.called("herdr --machine cloud1"), "{:?}", r.calls.borrow());
    // mini held: its refusal until it has been held 10 minutes, then the stopped cloud host.
    let e = placed(&c, &fake(3.0), None).unwrap_err();
    assert!(e.starts_with("HOST_BUSY: mini"), "{e}");
    let later = herdsman::Ctx {
        now: NOW + 601,
        ..c.clone()
    };
    assert_eq!(
        placed(&later, &fake(3.0), None).unwrap_err(),
        "CLOUD_STOPPED: cloud1: stopped"
    );
    // mini admits again: the clock resets.
    assert_eq!(placed(&later, &fake(0.1), None).as_deref(), Ok("mini"));
    assert!(!c.state.with_file_name("held-since").exists());
    let e = placed(&herdsman::Ctx { now: NOW + 700, ..c }, &fake(3.0), None).unwrap_err();
    assert!(e.starts_with("HOST_BUSY: mini"), "{e}");
}

/// cloud1 running since 3000 s ago with a lane whose worktree is 1 commit ahead.
fn running(name: &str) -> (herdsman::Ctx, Config) {
    let c = setup(name);
    ev(&c, NOW - 3000, "cloud-start");
    ev(&c, NOW - 2990, "cloud-up");
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "w9:p1", "worktree": WT,
                   "branch": "issue-12", "host": "cloud1", "machine": "cloud1"});
    append(&c.state, &Event::new(NOW - 2900, "sendmeter", Some(12), "spawn", d)).unwrap();
    let cfg = Config::load(&c.config).unwrap();
    (c, cfg)
}

fn lane_fake() -> Fake {
    let git = format!("{SSH}'git' '-C' '{WT}'");
    fake(0.1)
        .reply(&format!("{SSH}'test' '-e'"), "")
        .reply(&format!("{git} 'rev-list'"), "1\n")
        .reply(&git, "")
}

/// `check`'s cloud step: its output lines.
fn tick(
    c: &herdsman::Ctx,
    cfg: &Config,
    r: &dyn Runner,
    live: &BTreeMap<String, Option<Vec<serde_json::Value>>>,
) -> Vec<String> {
    let mut out = vec![];
    herdsman::cloud::tick(c, cfg, r, live, &mut out).unwrap();
    String::from_utf8(out).unwrap().lines().map(String::from).collect()
}

fn idle() -> BTreeMap<String, Option<Vec<serde_json::Value>>> {
    BTreeMap::from([("cloud1".to_string(), Some(vec![]))])
}

#[test]
fn an_idle_cloud_host_saves_each_lane_to_wip_then_stops() {
    let (c, cfg) = running("cloud-idle");
    let r = lane_fake();
    let out = tick(&c, &cfg, &r, &idle());
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(out[0].contains("stopped (idle 20 min)"), "{out:?}");
    let push = format!("{SSH}'git' '-C' '{WT}' 'push' 'origin' 'HEAD:refs/heads/wip/cloud1-sendmeter-impl-12'");
    assert!(
        r.index(&push).unwrap() < r.index(&rc("stop")).unwrap(),
        "{:?}",
        r.calls.borrow()
    );
    let events = read(&c.state).unwrap();
    let stop = events.iter().rfind(|e| e.kind == "cloud-stop").unwrap();
    assert_eq!(stop.data["wip"], json!(["wip/cloud1-sendmeter-impl-12"]));
    assert_eq!(herdsman::cloud::state(&events, "cloud1"), "stopped");
    // A live agent there, or a recent one, keeps it running.
    let (c, cfg) = running("cloud-busy");
    let live = BTreeMap::from([("cloud1".to_string(), Some(vec![json!({"name": "x"})]))]);
    assert!(tick(&c, &cfg, &lane_fake(), &live).is_empty());
    let soon = herdsman::Ctx { now: NOW - 1000, ..c };
    assert!(tick(&soon, &cfg, &lane_fake(), &idle()).is_empty());
}

#[test]
fn a_failed_wip_push_aborts_the_stop() {
    let (c, cfg) = running("cloud-abort");
    let push = format!("{SSH}'git' '-C' '{WT}' 'push'");
    let r = lane_fake().once_err(&push, 1, "rejected");
    let out = tick(&c, &cfg, &r, &idle());
    assert!(
        out[0].contains("stop aborted, sendmeter-impl-12's work not saved"),
        "{out:?}"
    );
    assert!(!r.called(&rc("stop")), "{:?}", r.calls.borrow());
    let events = read(&c.state).unwrap();
    assert!(events.iter().any(|e| e.kind == "cloud-abort"));
    assert_eq!(herdsman::cloud::state(&events, "cloud1"), "running");
}

#[test]
fn an_unreachable_host_aborts_the_stop_and_destroy_saves_first() {
    // ssh fails (255) probing the worktree: never "nothing to save", so neither stop nor destroy runs.
    let (c, cfg) = running("cloud-unreachable");
    let r = lane_fake().once_err(&format!("{SSH}'test' '-e'"), 255, "Connection timed out");
    let (code, out) = run(&c, &r, &["hosts", "destroy", "cloud1"]);
    assert_ne!(code, 0, "{out}");
    assert!(!r.called(&rc("destroy")), "{:?}", r.calls.borrow());
    let abort = read(&c.state)
        .unwrap()
        .into_iter()
        .rfind(|e| e.kind == "cloud-abort")
        .unwrap();
    assert!(abort.str("why").unwrap().starts_with("can't reach cloud1"), "{abort:?}");
    let tick_r = lane_fake().once_err(&format!("{SSH}'test' '-e'"), 255, "Connection timed out");
    assert!(tick(&c, &cfg, &tick_r, &idle())[0].contains("stop aborted"));
    assert!(!tick_r.called(&rc("stop")), "{:?}", tick_r.calls.borrow());
    let events = read(&c.state).unwrap();
    assert_eq!(events.iter().filter(|e| e.kind == "cloud-abort").count(), 2);
    assert_eq!(herdsman::cloud::state(&events, "cloud1"), "running");
    // Reachable: the wip push, then `destroy`.
    let r = lane_fake().reply(&rc("destroy"), "");
    let (code, out) = run(&c, &r, &["hosts", "destroy", "cloud1"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("cloud1: destroyed; wip wip/cloud1-sendmeter-impl-12"),
        "{out}"
    );
    let push = format!("{SSH}'git' '-C' '{WT}' 'push' 'origin' 'HEAD:refs/heads/wip/cloud1-sendmeter-impl-12'");
    assert!(r.index(&push).unwrap() < r.index(&rc("destroy")).unwrap());
}

#[test]
fn the_budget_warns_at_80_percent_and_refuses_when_used_up() {
    let card = |c: &herdsman::Ctx| {
        let cfg = Config::load(&c.config).unwrap();
        herdsman::cloud::card(c, &cfg, &read(&c.state).unwrap(), ("cloud1", 0)).unwrap()
    };
    // 2 h at $1/h: green, stopped.
    let c = setup("cloud-budget");
    ev(&c, NOW - 7200, "cloud-start");
    ev(&c, NOW, "cloud-stop");
    let [head, tone, text] = card(&c);
    assert_eq!(head, "stopped · 0 lane(s) paused: cloud host stopped");
    assert_eq!(tone, "g");
    assert!(
        text.starts_with("fake small · spent $2 of $10 · 2.0 h today of 20 · address never"),
        "{text}"
    );
    // 8.5 h: amber (85 %), still placeable.
    let c = setup("cloud-budget-80");
    ev(&c, NOW - 30_600, "cloud-start");
    ev(&c, NOW - 30_000, "cloud-up");
    let [head, tone, text] = card(&c);
    assert_eq!((head.as_str(), tone.as_str()), ("", "a"), "{text}");
    assert!(text.contains("spent $8 of $10 · 8.5 h today"), "{text}");
    // 11 h: red, and placement refuses CLOUD_BUDGET even when forced.
    let c = setup("cloud-budget-over");
    ev(&c, NOW - 39_600, "cloud-start");
    ev(&c, NOW - 39_000, "cloud-up");
    assert_eq!(card(&c)[1], "r");
    assert_eq!(
        placed(&c, &fake(0.1), Some("cloud1")).unwrap_err(),
        "CLOUD_BUDGET: cloud1: spent $11.00 of $10 this month"
    );
    // A running host at its limit is stopped (after the wip save).
    let cfg = Config::load(&c.config).unwrap();
    let out = tick(&c, &cfg, &fake(0.1), &BTreeMap::from([("cloud1".into(), Some(vec![]))]));
    assert!(out[0].contains("stopped (spent $11.00 of $10 this month)"), "{out:?}");
    // Last month's spend does not count: on 10 Feb 1970, a span from 31 Jan 02:13 to 1 Feb 01:00 counts 1 h.
    let c = herdsman::Ctx {
        now: 40 * 86_400,
        ..setup("cloud-budget-month")
    };
    ev(&c, 2_600_000, "cloud-start");
    ev(&c, 31 * 86_400 + 3600, "cloud-stop");
    assert!(card(&c)[2].contains("spent $1 of $10 · 0.0 h today"), "{}", card(&c)[2]);
}

#[test]
fn the_ssh_include_rewrite_is_idempotent_and_keeps_other_blocks() {
    let c = setup("cloud-include");
    let cfg = Config::load(&c.config).unwrap();
    let dir = c.home.join(".ssh/config.d");
    std::fs::create_dir_all(&dir).unwrap();
    let other = "# herdsman other\nHost other\n  HostName 1.2.3.4\n# end other\n";
    std::fs::write(dir.join("herdsman"), other).unwrap();
    herdsman::cloud::start(&c, &cfg, &fake(0.1), "cloud1").unwrap();
    let once = std::fs::read_to_string(dir.join("herdsman")).unwrap();
    herdsman::cloud::start(&c, &cfg, &fake(0.1), "cloud1").unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("herdsman")).unwrap(), once);
    assert!(
        once.starts_with(other) && once.matches("Host cloud1").count() == 1,
        "{once}"
    );
    // A new address replaces the block.
    let r = fake(0.1).once(&rc("address"), 0, "ubuntu@10.0.0.9 22\nssh-ed25519 BBBB\n");
    herdsman::cloud::start(&c, &cfg, &r, "cloud1").unwrap();
    let now = std::fs::read_to_string(dir.join("herdsman")).unwrap();
    assert!(now.contains("HostName 10.0.0.9") && !now.contains("10.0.0.7"), "{now}");
    let known = std::fs::read_to_string(dir.join("herdsman-cloud1.known")).unwrap();
    assert_eq!(known, "10.0.0.9 ssh-ed25519 BBBB\n");
}

#[test]
fn a_lost_end_marker_keeps_later_blocks_and_a_bad_address_is_refused() {
    let c = setup("cloud-include-bad");
    let cfg = Config::load(&c.config).unwrap();
    let dir = c.home.join(".ssh/config.d");
    std::fs::create_dir_all(&dir).unwrap();
    let other = "# herdsman other\nHost other\n# end other\n";
    std::fs::write(dir.join("herdsman"), format!("# herdsman cloud1\nHost cloud1\n{other}")).unwrap();
    herdsman::cloud::start(&c, &cfg, &fake(0.1), "cloud1").unwrap();
    let now = std::fs::read_to_string(dir.join("herdsman")).unwrap();
    assert!(
        now.starts_with(other) && now.matches("Host cloud1").count() == 1,
        "{now}"
    );
    // A reply that would add an ssh directive (`Port 22 ProxyCommand …`) is refused; the file is untouched.
    let r = fake(0.1).once(
        &rc("address"),
        0,
        "ubuntu@10.0.0.9 22 ProxyCommand sh\nssh-ed25519 BBBB\n",
    );
    let e = herdsman::cloud::start(&c, &cfg, &r, "cloud1").unwrap_err();
    assert!(format!("{e:#}").contains("not `user@host port`"), "{e:#}");
    assert_eq!(std::fs::read_to_string(dir.join("herdsman")).unwrap(), now);
}

/// Records each call's stdin.
struct Stdin(Fake, Shared<Vec<String>>);

impl Runner for Stdin {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.0.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.1.borrow_mut().push(input.into());
        self.0.run(prog, args)
    }
}

#[test]
fn the_provider_key_never_reaches_argv_or_the_log() {
    let (c, cfg) = running("cloud-key");
    let r = Stdin(
        lane_fake().once_err(&rc("stop"), 1, &format!("bad key {KEY}")),
        Shared::default(),
    );
    let e = herdsman::cloud::stop(&c, &cfg, &r, "cloud1", "stop", "test").unwrap_err();
    assert!(format!("{e:#}").contains("bad key •••"), "{e:#}");
    herdsman::cloud::stop(&c, &cfg, &r, "cloud1", "stop", "test").unwrap();
    herdsman::cloud::start(&c, &cfg, &r, "cloud1").unwrap();
    assert!(
        r.1.borrow().iter().any(|i| i == &format!("{KEY}\n")),
        "the key goes on stdin"
    );
    assert!(
        !r.0.calls.borrow().iter().any(|c| c.contains(KEY)),
        "{:?}",
        r.0.calls.borrow()
    );
    let log = std::fs::read_to_string(&c.state).unwrap();
    assert!(!log.contains(KEY) && log.contains("cloud-up"), "{log}");
}
