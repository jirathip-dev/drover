mod common;
use common::*;
use herdsman::events::{append, read, Event};
use herdsman::runner::{Out, Runner};
use serde_json::json;
use std::time::Duration;

const NOW: u64 = 1_000_000;
const BANNER: &str = "@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\r\n\
@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\r\n\
@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\r\n\
IT IS POSSIBLE THAT SOMEONE IS DOING SOMETHING NASTY!\r\n\
Host key for boat1 has changed and you have requested strict checking.\r\n\
Host key verification failed.\r\n";

fn boat1(prog: &str, args: &[String]) -> bool {
    matches!(prog, "ssh" | "herdr") && args.iter().any(|a| a == "boat1")
}

fn setup(name: &str, extra: &str) -> herdsman::Ctx {
    let base = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
    let toml = format!("{base}\n[hosts.mini]\n\n[hosts.boat1]\nmachine = \"boat1\"\n{extra}");
    let c = ctx(&tmpdir(name), &toml);
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "boat1": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    let d = json!({"role": "impl", "agent": "sendmeter-impl-9", "pane": "w9:p1", "workspace": "w9",
                   "worktree": "/Users/me/.herdr/worktrees/sendmeter/issue-9", "branch": "issue-9", "host": "boat1",
                   "machine": "boat1"});
    append(&c.state, &Event::new(NOW - 900, "sendmeter", Some(9), "spawn", d)).unwrap();
    append(
        &c.state,
        &Event::new(NOW - 800, "sendmeter", Some(9), "merged", json!({})),
    )
    .unwrap();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    c
}

fn local() -> Fake {
    Fake::default()
        .reply(
            "herdr status",
            "client:\n  version: 0.9.5\n\nserver:\n  status: running\n  version: 0.9.5\n",
        )
        .reply("herdr agent list", NO_AGENTS)
        .reply("herdr --machine boat1 agent list", NO_AGENTS)
        .reply("sysctl -n vm.loadavg", "{ 0.4 1 1 }")
        .reply("sysctl -n hw.ncpu", "4")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("git ", "")
        .reply("gh issue view", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply(
            "herdr worktree create",
            r#"{"result":{"workspace":{"workspace_id":"w1"}}}"#,
        )
        .reply(
            "herdr pane list --workspace w1",
            r#"{"result":{"panes":[{"pane_id":"w1:p1","agent":null}]}}"#,
        )
        .reply("herdr pane run w1:p1 export", "{}")
        .reply("herdr agent start", "{}")
        .reply("herdr agent wait", "{}")
}

fn to_boat1(f: &Fake) -> Vec<String> {
    let calls = f.calls.borrow();
    calls.iter().filter(|l| l.contains(" boat1 ")).cloned().collect()
}

#[test]
fn a_disabled_host_with_no_lane_there_gets_no_ssh_from_a_tick_or_status() {
    let c = setup("spared-off", "enabled = false\n");
    let r = local();
    let (code, out) = run(&c, &r, &["tick"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(to_boat1(&r), Vec::<String>::new(), "{out}");
    assert!(
        r.called("herdr agent start sendmeter-impl-12"),
        "drive still starts: {out}"
    );
    let (_, status) = run(&c, &r, &["status"]);
    assert_eq!(to_boat1(&r), Vec::<String>::new(), "{status}");
    let ev = read(&c.state).unwrap();
    assert!(!ev.iter().any(|e| e.kind == "host-offline"), "{ev:?}");
}

#[test]
fn a_disabled_host_with_a_live_lane_there_is_still_looked_at() {
    let c = setup("spared-off-live", "enabled = false\n");
    let d = json!({"role": "impl", "agent": "sendmeter-impl-11", "pane": "w8:p1", "workspace": "w8",
                   "worktree": "/Users/me/.herdr/worktrees/sendmeter/issue-11", "branch": "issue-11", "host": "boat1",
                   "machine": "boat1"});
    append(&c.state, &Event::new(NOW - 600, "sendmeter", Some(11), "spawn", d)).unwrap();
    let r = local();
    let (code, out) = run(&c, &r, &["tick"]);
    assert_eq!(code, 0, "{out}");
    assert!(r.called("herdr --machine boat1 agent list"), "{out}");
}
struct KeyChanged(Fake);

impl Runner for KeyChanged {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.run_for(prog, args, "", Duration::from_secs(60))
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.run_for(prog, args, input, Duration::from_secs(60))
    }
    fn run_for(&self, prog: &str, args: &[String], input: &str, _: Duration) -> anyhow::Result<Out> {
        if !boat1(prog, args) {
            return self.0.run_in(prog, args, input);
        }
        self.0.calls.borrow_mut().push(format!("{prog} {}", args.join(" ")));
        Ok(Out {
            code: 255,
            stderr: BANNER.into(),
            ..Out::default()
        })
    }
    fn detach(&self, exe: &str, args: &[String], log: &std::path::Path) -> anyhow::Result<u32> {
        self.0.detach(exe, args, log)
    }
}

#[test]
fn a_changed_host_key_alerts_once_and_is_never_retried_in_a_loop() {
    let c = setup("spared-key", "");
    for (i, now) in [NOW, NOW + 300, NOW + 600].into_iter().enumerate() {
        let c = herdsman::Ctx { now, ..c.clone() };
        let r = KeyChanged(local());
        let (code, out) = run(&c, &r, &["tick"]);
        assert_eq!(code, 0, "pass {i}: {out}");
        let calls = to_boat1(&r.0);
        assert_eq!(calls.len(), 1, "pass {i}: one try, then skipped: {calls:#?}");
        let alerts: Vec<Event> = read(&c.state)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == "host-key-changed")
            .collect();
        assert_eq!(alerts.len(), 1, "pass {i}: {alerts:?}");
        assert_eq!(alerts[0].str("host"), Some("boat1"));
        assert!(
            alerts[0].str("reason").unwrap().starts_with("host-key-changed boat1"),
            "{alerts:?}"
        );
    }
    let ev = read(&c.state).unwrap();
    assert!(ev.iter().any(|e| e.kind == "host-offline"), "the host shows offline");
}
