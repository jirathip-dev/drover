mod common;
use common::*;
use drover::events::{append, Event};
use drover::runner::{Out, Runner};
use serde_json::json;
use std::time::Duration;

const NOW: u64 = 1_000_000;
const PROBE: &str = "ssh -oBatchMode=yes -oConnectTimeout=5 -oControlPath=none air -- true";
const CALL_SECS: u64 = 20;

struct Stall {
    f: Fake,
    far: &'static str,
    spent: Shared<u64>,
    sent: Shared<Vec<String>>,
}

impl Runner for Stall {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.run_for(prog, args, "", Duration::from_secs(3600))
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.run_for(prog, args, input, Duration::from_secs(3600))
    }
    fn run_for(&self, prog: &str, args: &[String], input: &str, limit: Duration) -> anyhow::Result<Out> {
        let line = format!("{prog} {}", args.join(" "));
        let remote = prog == "ssh" && args[0] != "-G" && args.iter().any(|a| a == self.far);
        if !remote || line == PROBE {
            return self.f.run_in(prog, args, input);
        }
        self.spent.set(self.spent.get() + limit.as_secs());
        self.sent.borrow_mut().push(line);
        Ok(Out {
            code: 124,
            ..Out::default()
        })
    }
    fn sleep(&self, secs: u64) {
        self.f.sleep(secs);
    }
    fn detach(&self, exe: &str, args: &[String], log: &std::path::Path) -> anyhow::Result<u32> {
        self.f.detach(exe, args, log)
    }
}

fn setup(name: &str, machine: &str) -> (drover::Ctx, FakeShed) {
    let base = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
    let toml = format!("{base}\n[hosts.mini]\n\n[hosts.air]\nmachine = \"air\"\n");
    let c = ctx(&tmpdir(name), &toml);
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "air": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    let wt = "/home/user/.herdr/worktrees/sendmeter/issue-49";
    let d = json!({"role": "impl", "agent": "sendmeter-impl-49", "pane": "sendmeter-impl-49",
                   "workspace": "", "worktree": wt, "branch": "issue-49", "host": machine,
                   "machine": machine});
    append(&c.state, &Event::new(NOW - 600, "sendmeter", Some(49), "spawn", d)).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let sh = FakeShed::attached(&c);
    (c, sh)
}

fn stall(far: &'static str) -> Stall {
    let f = Fake::default()
        .reply(
            "herdr status",
            "client:\n  version: 0.9.5\n\nserver:\n  status: running\n  version: 0.9.5\n",
        )
        .reply(PROBE, "")
        .reply("sysctl -n vm.loadavg", "{ 0.4 1 1 }")
        .reply("sysctl -n hw.ncpu", "4")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("vm_stat", "Swapouts: 5.\n")
        .reply("git ", "")
        .reply("which shed", "/opt/shed/bin/shed\n");
    Stall {
        f,
        far,
        spent: Shared::new(0),
        sent: Shared::default(),
    }
}

#[test]
fn a_lane_on_a_machine_outside_hosts_gets_no_ssh_and_shows_host_offline_not_configured() {
    let (c, _sh) = setup("stall-unconfigured", "boat1");
    let r = stall("boat1");
    let (code, out) = run(&c, &r, &["tick"]);
    assert_eq!(code, 0, "{out}");
    let sent = r.sent.borrow().clone();
    assert!(sent.is_empty(), "no ssh to a machine no [hosts] entry names: {sent:#?}");
    assert_eq!(r.spent.get(), 0, "{sent:#?}");
    let (code, out) = run(&c, &r, &["waiting", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("host offline (not configured): boat1"), "{out}");
}

#[test]
fn a_configured_host_whose_muxed_call_stalls_after_its_probe_is_skipped_for_the_rest_of_the_pass() {
    let (c, _sh) = setup("stall-configured", "air");
    let r = stall("air");
    let (code, out) = run(&c, &r, &["tick"]);
    assert_eq!(code, 0, "{out}");
    let sent = r.sent.borrow().clone();
    assert_eq!(
        sent.len(),
        1,
        "one stalled call, then the host is offline this pass: {sent:#?}"
    );
    assert!(
        r.spent.get() <= CALL_SECS,
        "{}s on the stalled host: {sent:#?}",
        r.spent.get()
    );
    assert!(
        out.contains("tick: air: over budget; skipped for the rest of this pass"),
        "{out}"
    );
}

#[test]
fn hosts_skipped_two_passes_running_raise_one_alert_with_the_calls_not_sent() {
    let (c, _sh) = setup("stall-alert", "air");
    let skipped = || {
        drover::events::read(&c.state)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "hosts-skipped")
            .cloned()
            .collect::<Vec<_>>()
    };
    let (code, out) = run(&c, &stall("air"), &["tick"]);
    assert_eq!(code, 0, "{out}");
    assert!(skipped().is_empty(), "one pass is no alert yet: {out}");
    for pass in 2..=3 {
        let (code, out) = run(&c, &stall("air"), &["tick"]);
        assert_eq!(code, 0, "pass {pass}: {out}");
        let got = skipped();
        assert_eq!(
            got.len(),
            1,
            "pass {pass}: one alert while the state lasts: {got:?}\n{out}"
        );
        let e = &got[0];
        assert_eq!(e.data["hosts"], json!(["air"]), "{e:?}");
        assert_eq!(e.data["passes"], json!(2), "{e:?}");
        assert!(e.data["not_sent"].as_u64().is_some(), "{e:?}");
        assert!(e.str("step").is_some_and(|s| !s.is_empty()), "{e:?}");
    }
}
