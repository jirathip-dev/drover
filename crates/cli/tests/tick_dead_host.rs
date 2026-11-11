mod common;
use common::*;
use herdsman::events::{append, Event};
use herdsman::runner::{Out, Runner};
use serde_json::json;
use std::time::Duration;

const NOW: u64 = 1_000_000;
const PROBE: &str = "ssh -oBatchMode=yes -oConnectTimeout=5 -oControlPath=none air -- true";

struct Dead {
    f: Fake,
    probe_ok: bool,
    spent: Shared<u64>,
    sent: Shared<Vec<String>>,
}

impl Runner for Dead {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.run_for(prog, args, "", Duration::from_secs(600))
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.run_for(prog, args, input, Duration::from_secs(600))
    }
    fn run_for(&self, prog: &str, args: &[String], input: &str, limit: Duration) -> anyhow::Result<Out> {
        let line = format!("{prog} {}", args.join(" "));
        let air = matches!(prog, "ssh" | "herdr") && args.iter().any(|a| a == "air") && args[0] != "-G";
        if !air || (self.probe_ok && line == PROBE) {
            return self.f.run_in(prog, args, input);
        }
        let wait = (args.iter())
            .find_map(|a| a.strip_prefix("-oConnectTimeout="))
            .map_or(75, |s| s.parse().unwrap());
        self.spent.set(self.spent.get() + wait.min(limit.as_secs()));
        self.sent.borrow_mut().push(line);
        Ok(Out {
            code: 255,
            stdout: String::new(),
            stderr: "Connection timed out during banner exchange\r\nConnection to air port 22 timed out\r\n".into(),
        })
    }
    fn sleep(&self, secs: u64) {
        self.f.sleep(secs);
    }
    fn detach(&self, exe: &str, args: &[String], log: &std::path::Path) -> anyhow::Result<u32> {
        self.f.detach(exe, args, log)
    }
}

fn setup(name: &str, air_cfg: &str) -> herdsman::Ctx {
    let base = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
    let toml = format!("{base}\n[hosts.mini]\n\n[hosts.air]\nmachine = \"air\"\n{air_cfg}");
    let c = ctx(&tmpdir(name), &toml);
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "air": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    for n in 100..150u64 {
        let wt = format!("/Users/me/.herdr/worktrees/sendmeter/issue-{n}");
        let d = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "pane": format!("w9:p{n}"),
                       "workspace": "w9", "worktree": wt, "branch": format!("issue-{n}"), "host": "air",
                       "machine": "air"});
        append(&c.state, &Event::new(NOW - 9000, "sendmeter", Some(n), "spawn", d)).unwrap();
        append(
            &c.state,
            &Event::new(NOW - 8000, "sendmeter", Some(n), "merged", json!({})),
        )
        .unwrap();
    }
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    c
}

fn main_host() -> Fake {
    Fake::default()
        .reply(
            "herdr status",
            "client:\n  version: 0.9.5\n\nserver:\n  status: running\n  version: 0.9.5\n",
        )
        .reply(PROBE, "")
        .reply("herdr agent list", NO_AGENTS)
        .reply("herdr --machine air agent list", NO_AGENTS)
        .reply("sysctl -n vm.loadavg", "{ 0.4 1 1 }")
        .reply("sysctl -n hw.ncpu", "4")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("vm_stat", "Swapouts: 5.\n")
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

#[test]
fn a_host_whose_ssh_never_connects_with_50_worktrees_costs_the_tick_under_a_minute() {
    for (name, air_cfg, probe_ok) in [("dead-on", "", true), ("dead-off", "enabled = false\n", false)] {
        let c = setup(name, air_cfg);
        let r = Dead {
            f: main_host(),
            probe_ok,
            spent: Shared::new(0),
            sent: Shared::default(),
        };
        let (code, out) = run(&c, &r, &["tick"]);
        assert_eq!(code, 0, "{name}: {out}");
        assert!(
            r.f.called("herdr agent start sendmeter-impl-12"),
            "{name}: drive's step runs: {out}"
        );
        let sent = r.sent.borrow();
        assert!(r.spent.get() < 60, "{name}: {}s on the Air: {sent:#?}", r.spent.get());
        assert!(
            sent.len() <= 2,
            "{name}: one probe or one failed call, not one per worktree: {sent:#?}"
        );
        if probe_ok {
            continue;
        }
        for n in 100..150u64 {
            let wt = format!("air:/Users/me/.herdr/worktrees/sendmeter/issue-{n}");
            assert!(
                out.contains(&format!("keep {wt}: host unreachable")),
                "{name} {n}: {out}"
            );
        }
    }
}

#[test]
fn a_reachable_off_host_is_still_cleaned_and_killed_when_asked_directly() {
    let c = setup("dead-off-asked", "enabled = false\n");
    let wt = "/Users/me/.herdr/worktrees/sendmeter/issue-100";
    let test_e = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/herdsman-ssh-%C -oControlPersist=10m air -- 'sh' '-c' 'test -e";
    let r = main_host().reply(test_e, "");
    let (code, out) = run(&c, &r, &["clean"]);
    assert_eq!(code, 0, "{out}");
    assert!(r.called(&format!("{test_e} \"$1\"' 'sh' '{wt}'")), "{out}");
    assert!(out.contains(&format!("would save air:{wt}")), "{out}");
    let git = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/herdsman-ssh-%C -oControlPersist=10m air -- 'git'";
    let r = main_host().reply(test_e, "").reply(git, "0\n");
    let (code, out) = run(&c, &r, &["kill", "sendmeter-impl-100"]);
    assert_eq!(code, 0, "{out}");
    assert!(r.called(&format!("{test_e} \"$1\"' 'sh' '{wt}'")), "{out}");
}
