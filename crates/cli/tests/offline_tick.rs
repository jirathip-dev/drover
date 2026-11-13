mod common;
use common::*;
use drover::events::{append, Event};
use drover::runner::{Out, Runner};
use serde_json::json;
use std::time::Duration;

const NOW: u64 = 1_000_000;
const PROBE: &str = "ssh -oBatchMode=yes -oConnectTimeout=5 -oControlPath=none air -- true";

struct Dropped(Fake, Shared<Vec<String>>, Shared<u64>);

impl Dropped {
    fn new(f: Fake) -> Self {
        Dropped(f, Shared::default(), Shared::new(0))
    }
    fn timeouts(&self) -> Vec<String> {
        self.1.borrow().clone()
    }
}

impl Runner for Dropped {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.run_in(prog, args, "")
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.run_for(prog, args, input, Duration::from_secs(60))
    }
    fn run_for(&self, prog: &str, args: &[String], input: &str, _limit: Duration) -> anyhow::Result<Out> {
        let line = format!("{prog} {}", args.join(" "));
        let air = matches!(prog, "ssh" | "herdr") && args.iter().any(|a| a == "air") && args[0] != "-G";
        if !air || line == PROBE {
            return self.0.run_in(prog, args, input);
        }
        self.1.borrow_mut().push(line);
        self.2.set(self.2.get() + 10);
        Ok(Out {
            code: 255,
            stderr: "ssh: connect to host air port 22: Operation timed out\r\n".into(),
            ..Out::default()
        })
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
}

fn main_host() -> Fake {
    Fake::default()
        .reply(
            "herdr status",
            "client:\n  version: 0.9.5\n\nserver:\n  status: running\n  version: 0.9.5\n",
        )
        .reply(PROBE, "")
        .reply("herdr agent list", NO_AGENTS)
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 0.4 1 1 }")
        .reply("sysctl -n hw.ncpu", "8")
        .reply("vm_stat", "Swapouts: 5.\n")
        .reply("gh issue view", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply("git ", "")
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

fn setup(name: &str, air_cfg: &str, limits: &str) -> drover::Ctx {
    let base = CONFIG
        .replace("harness  =  \"hermes\"", "harness = \"claude\"")
        .replace("[limits]\n", &format!("[limits]\n{limits}"));
    let toml = format!("{base}\n[hosts.mini]\n\n[hosts.air]\nmachine = \"air\"\n{air_cfg}");
    let c = ctx(&tmpdir(name), &toml);
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "air": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    for n in 1..=5 {
        let wt = format!("/Users/me/.herdr/worktrees/sendmeter/issue-{n}");
        let d = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "harness": "claude",
                       "pane": format!("w9:p{n}"), "workspace": "w9", "worktree": wt, "branch": format!("issue-{n}"),
                       "host": "air", "machine": "air"});
        append(&c.state, &Event::new(NOW - 900, "sendmeter", Some(n), "spawn", d)).unwrap();
        append(
            &c.state,
            &Event::new(NOW - 800, "sendmeter", Some(n), "merged", json!({})),
        )
        .unwrap();
    }
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    c
}

#[test]
fn a_host_that_drops_after_its_probe_costs_one_timeout_and_drive_still_starts() {
    for (name, air_cfg) in [("drop-disabled", "enabled = false\n"), ("drop-enabled", "")] {
        let c = setup(name, air_cfg, "");
        let r = Dropped::new(main_host());
        let (code, out) = run(&c, &r, &["tick"]);
        assert_eq!(code, 0, "{name}: {out}");
        let hung = r.timeouts();
        assert!(
            hung.len() <= 1,
            "{name}: at most one timeout on the Air, not one per lane: {hung:#?}\n{out}"
        );
        assert!(r.2.get() <= 10, "{name}: {} s on the Air", r.2.get());
        assert!(
            r.0.called("herdr agent start sendmeter-impl-12"),
            "{name}: drive's full pass starts #12 on the main host: {out}"
        );
    }
}

#[test]
fn a_tick_past_its_budget_skips_the_rest_says_so_and_ends() {
    let c = setup("drop-budget", "", "slow_secs = 0\n");
    let r = Dropped::new(main_host());
    let (code, out) = run(&c, &r, &["tick"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        !r.0.called(PROBE),
        "nothing more goes to another host: {:#?}",
        r.0.calls.borrow()
    );
    assert!(r.timeouts().is_empty(), "{:#?}", r.timeouts());
    assert!(
        out.contains("tick: past its 0 s budget; skipped: clean, trim, lesson"),
        "{out}"
    );
    assert!(
        r.0.called("herdr agent start sendmeter-impl-12"),
        "drive still starts local work: {out}"
    );
}
