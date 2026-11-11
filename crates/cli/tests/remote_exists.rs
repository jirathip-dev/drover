mod common;
use common::*;
use herdsman::events::{append, Event};
use herdsman::runner::{Out, Runner};
use serde_json::json;
use std::collections::BTreeSet;
use std::time::Duration;

const NOW: u64 = 1_000_000;
const PROBE: &str = "ssh -oBatchMode=yes -oConnectTimeout=5 -oControlPath=none air -- true";
const LANES: std::ops::Range<u64> = 1000..1500;

fn wt(n: u64) -> String {
    format!("/Users/me/.herdr/worktrees/sendmeter/issue-{n}")
}

struct Air {
    f: Fake,
    present: BTreeSet<String>,
    hang: bool,
    sent: Shared<Vec<String>>,
}

impl Air {
    fn exists(&self, args: &[String], input: &str) -> Out {
        let line = args.last().map_or("", String::as_str);
        let paths: Vec<&str> = match line.strip_suffix('\'').and_then(|l| l.rsplit_once(" '")) {
            Some((script, p)) if script.ends_with("'test -e \"$1\"' 'sh'") => vec![p],
            _ => input.lines().collect(),
        };
        let yes = |p: &&str| self.present.contains(*p);
        match line.contains("\"$1\"") {
            true => Out {
                code: i32::from(!paths.iter().all(yes)),
                ..Out::default()
            },
            false => Out {
                stdout: paths.iter().map(|p| if yes(p) { "y\n" } else { "n\n" }).collect(),
                ..Out::default()
            },
        }
    }
}

impl Runner for Air {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.run_for(prog, args, "", Duration::from_secs(600))
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.run_for(prog, args, input, Duration::from_secs(600))
    }
    fn run_for(&self, prog: &str, args: &[String], input: &str, _limit: Duration) -> anyhow::Result<Out> {
        let line = format!("{prog} {}", args.join(" "));
        let air = prog == "ssh" && args.iter().any(|a| a == "air") && args[0] != "-G" && line != PROBE;
        if !air {
            return self.f.run_in(prog, args, input);
        }
        self.sent.borrow_mut().push(line.clone());
        if self.hang {
            return Ok(Out {
                code: 124,
                ..Out::default()
            });
        }
        if line.contains("test -e") {
            return Ok(self.exists(args, input));
        }
        self.f.run_in(prog, args, input)
    }
    fn sleep(&self, secs: u64) {
        self.f.sleep(secs);
    }
    fn detach(&self, exe: &str, args: &[String], log: &std::path::Path) -> anyhow::Result<u32> {
        self.f.detach(exe, args, log)
    }
}

fn setup(name: &str) -> herdsman::Ctx {
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
    for n in LANES {
        let d = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "pane": format!("w9:p{n}"),
                       "workspace": "w9", "worktree": wt(n), "branch": format!("issue-{n}"), "host": "air",
                       "machine": "air"});
        append(&c.state, &Event::new(NOW - 9000, "sendmeter", Some(n), "spawn", d)).unwrap();
        append(
            &c.state,
            &Event::new(NOW - 8000, "sendmeter", Some(n), "merged", json!({})),
        )
        .unwrap();
    }
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
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

fn air(present: &[u64], hang: bool) -> Air {
    Air {
        f: main_host(),
        present: present.iter().map(|&n| wt(n)).collect(),
        hang,
        sent: Shared::default(),
    }
}

fn checks(r: &Air) -> usize {
    r.sent.borrow().iter().filter(|l| l.contains("test -e")).count()
}

#[test]
fn a_drive_pass_checks_500_remote_worktrees_with_one_ssh_call() {
    let c = setup("exists-drive");
    let r = air(&[], false);
    let (code, out) = run(&c, &r, &["drive"]);
    eprintln!("drive: {} existence ssh call(s) to the Air", checks(&r));
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        checks(&r),
        1,
        "one ssh call checks every worktree: {:#?}",
        r.sent.borrow()
    );
    let one = r.sent.borrow().iter().any(|l| l.contains("test -e \"$1\""));
    assert!(!one, "no per-path call: {:#?}", r.sent.borrow());
    assert!(!out.contains("clean"), "no worktree there, nothing to clean: {out}");
}

#[test]
fn a_clean_pass_reads_each_of_500_remote_worktrees_existence_from_one_ssh_call() {
    let c = setup("exists-clean");
    let present = [1000, 1177, 1499];
    let r = air(&present, false);
    let (code, out) = run(&c, &r, &["clean"]);
    eprintln!("clean: {} existence ssh call(s) to the Air", checks(&r));
    assert_eq!(code, 0, "{out}");
    assert_eq!(checks(&r), 1, "{:#?}", r.sent.borrow());
    let one = r.sent.borrow().iter().any(|l| l.contains("test -e \"$1\""));
    assert!(!one, "no per-path call: {:#?}", r.sent.borrow());
    for n in LANES {
        let shown = out.contains(&format!("air:{}\n", wt(n))) || out.contains(&format!("air:{} ", wt(n)));
        assert_eq!(shown, present.contains(&n), "{n}: {out}");
    }
}

#[test]
fn a_hung_host_is_logged_over_budget_and_the_other_hosts_spawn_goes_ahead() {
    let c = setup("exists-hung");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    let r = air(&[], true);
    let (code, out) = run(&c, &r, &["tick"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        r.f.called("herdr agent start sendmeter-impl-12"),
        "the spawn goes ahead: {out}"
    );
    assert!(
        out.contains("tick: air: over budget; skipped for the rest of this pass"),
        "{out}"
    );
    let sent = r.sent.borrow();
    assert!(
        sent.len() <= 2,
        "one call and its retry without the shared connection: {sent:#?}"
    );
}
