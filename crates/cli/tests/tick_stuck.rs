//! #561: a remote host that never answers (the Air, offline) costs a tick one 5 s probe: the tick stays in budget,
//! drive still starts work, the host shows offline; a disabled host's worktrees are never probed by clean; a call
//! through a dead shared ssh connection drops its socket and runs once more; a tick 10 min old raises `tick-stuck` once.
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use herdsman::runner::{Out, Runner};
use serde_json::json;
use std::time::Duration;

const NOW: u64 = 1_000_000;
const WT: &str = "/Users/me/.herdr/worktrees/sendmeter/issue-9";
const SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/herdsman-ssh-%C -oControlPersist=10m air -- ";

fn setup(name: &str, extra: &str) -> herdsman::Ctx {
    let base = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
    let toml = format!("{base}\n[hosts.mini]\n\n[hosts.air]\nmachine = \"air\"\n{extra}");
    let c = ctx(&tmpdir(name), &toml);
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "air": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    // #9's lane ran on the Air and is merged: clean's to remove.
    let d = json!({"role": "impl", "agent": "sendmeter-impl-9", "pane": "w9:p1", "workspace": "w9", "worktree": WT,
                   "branch": "issue-9", "host": "air", "machine": "air"});
    append(&c.state, &Event::new(NOW - 900, "sendmeter", Some(9), "spawn", d)).unwrap();
    append(
        &c.state,
        &Event::new(NOW - 800, "sendmeter", Some(9), "merged", json!({})),
    )
    .unwrap();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    c
}

fn local() -> Fake {
    Fake::default()
        .reply(
            "herdr status",
            "client:\n  version: 0.9.5\n\nserver:\n  status: running\n  version: 0.9.5\n",
        )
        .reply("herdr agent list", NO_AGENTS)
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

/// A fake whose every call to the Air never returns: a call without a hard limit panics (it would hang the tick); one
/// with a limit is killed at it (exit 124, as `Real` does), the limit added to a virtual clock. Each such call also
/// records the tick's `tick.step` as it stood then.
struct Hang {
    f: Fake,
    mark: std::path::PathBuf,
    spent: Shared<u64>,
    seen: Shared<Vec<String>>,
}

fn air(prog: &str, args: &[String]) -> bool {
    matches!(prog, "ssh" | "herdr") && args.iter().any(|a| a == "air")
}

impl Runner for Hang {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        assert!(!air(prog, args), "unbounded call to the Air: {prog} {args:?}");
        self.f.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        assert!(!air(prog, args), "unbounded call to the Air: {prog} {args:?}");
        self.f.run_in(prog, args, input)
    }
    fn run_for(&self, prog: &str, args: &[String], input: &str, limit: Duration) -> anyhow::Result<Out> {
        if !air(prog, args) {
            return self.f.run_in(prog, args, input);
        }
        self.f.calls.borrow_mut().push(format!("{prog} {}", args.join(" ")));
        self.seen
            .borrow_mut()
            .push(std::fs::read_to_string(&self.mark).unwrap_or_default());
        self.spent.set(self.spent.get() + limit.as_secs());
        Ok(Out {
            code: 124,
            ..Out::default()
        })
    }
    fn detach(&self, exe: &str, args: &[String], log: &std::path::Path) -> anyhow::Result<u32> {
        self.f.detach(exe, args, log)
    }
}

#[test]
fn a_host_that_never_answers_costs_one_probe_and_drive_still_starts() {
    let c = setup("stuck-air", "");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    // #11's lane is live on the Air.
    let d = json!({"role": "impl", "agent": "sendmeter-impl-11", "pane": "w8:p1", "workspace": "w8",
                   "worktree": "/Users/me/.herdr/worktrees/sendmeter/issue-11", "branch": "issue-11", "host": "air",
                   "machine": "air"});
    append(&c.state, &Event::new(NOW - 600, "sendmeter", Some(11), "spawn", d)).unwrap();
    let r = Hang {
        f: local(),
        mark: c.state.with_file_name("tick.step"),
        spent: Shared::new(0),
        seen: Shared::default(),
    };
    let (code, out) = run(&c, &r, &["tick"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        r.f.called("herdr agent start sendmeter-impl-12"),
        "drive still starts: {out}"
    );
    let calls = r.f.calls.borrow();
    let to_air: Vec<_> = calls.iter().filter(|l| l.contains(" air ")).collect();
    assert_eq!(
        to_air,
        ["ssh -oBatchMode=yes -oConnectTimeout=5 -oControlPath=none air -- true"],
        "one probe, nothing after it"
    );
    assert!(r.spent.get() <= 60, "{}s on the Air", r.spent.get());
    let mark = r.seen.borrow()[0].clone();
    assert!(mark.contains(r#""step":"check""#) && mark.contains("air"), "{mark}");
    assert!(!r.mark.exists(), "the tick's step marker goes when it ends");
    let cached = herdsman::hosts::cached(&c);
    assert!(!cached["air"].up, "the Air shows offline");
    assert!(
        out.contains(&format!("keep air:{WT}: host offline (not cleaned)")),
        "{out}"
    );
    let (_, status) = run(&c, &Fake::default(), &["status"]);
    assert!(status.contains("host offline: air"), "{status}");
}

#[test]
fn clean_never_probes_a_disabled_hosts_worktrees() {
    let c = setup("stuck-disabled", "enabled = false");
    let r = local().reply("herdr --machine air agent list", NO_AGENTS);
    let (code, out) = run(&c, &r, &["clean", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(&format!("keep air:{WT}: on a disabled host (not cleaned)")),
        "{out}"
    );
    let calls = r.calls.borrow();
    assert!(!calls.iter().any(|l| l.starts_with(SSH)), "{calls:?}");
    // Enabled again: probed and removed as before.
    let c2 = setup("stuck-enabled", "");
    let r = local()
        .reply("herdr --machine air agent list", NO_AGENTS)
        .reply(&format!("{SSH}'sh' '-c' 'test -e"), "");
    let (_, out) = run(&c2, &r, &["clean"]);
    assert!(r.called(&format!("{SSH}'sh' '-c' 'test -e")), "{out}");
    assert!(!out.contains("not cleaned"), "{out}");
}

#[test]
fn a_dead_shared_connection_drops_its_socket_and_runs_once_more_without_it() {
    let c = setup("stuck-socket", "");
    let sock = c.state.with_file_name("herdsman-ssh-abc");
    std::fs::write(&sock, "").unwrap();
    let probe = format!("{SSH}'sh' '-c' 'test -e");
    let r = local()
        .reply("herdr --machine air agent list", NO_AGENTS)
        .reply(
            "ssh -G -oControlPath=/tmp/herdsman-ssh-%C air",
            &format!("user me\ncontrolpath {}\n", sock.display()),
        )
        .once_err(
            &probe,
            255,
            "mux_client_request_session: read from master failed: Broken pipe",
        )
        .reply(
            "ssh -oControlPath=none -oBatchMode=yes -oConnectTimeout=10 air -- 'sh' '-c' 'test -e",
            "",
        );
    let (code, out) = run(&c, &r, &["clean"]);
    assert_eq!(code, 0, "{out}");
    assert!(!sock.exists(), "the stale socket is removed");
    let calls = r.calls.borrow();
    let (dead, again) = (
        calls.iter().position(|l| l.starts_with(&probe)),
        calls
            .iter()
            .position(|l| l.starts_with("ssh -oControlPath=none -oBatchMode=yes -oConnectTimeout=10 air --")),
    );
    assert!(dead.is_some() && dead < again, "{calls:?}");
    assert!(
        out.contains(&format!("would save air:{WT}")),
        "the retry's answer counts: {out}"
    );
}

#[test]
fn a_tick_ten_minutes_old_raises_tick_stuck_once_naming_its_step_and_child() {
    let c = setup("stuck-alert", "");
    let mark = c.state.with_file_name("tick.step");
    let child = "ssh -oBatchMode=yes air -- 'sh' '-c' 'test -e \"$1\"'";
    let stuck = |c: &herdsman::Ctx| -> Vec<Event> {
        assert_eq!(run(c, &local(), &["drive", "--fast"]).0, 0);
        read(&c.state)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == "tick-stuck")
            .collect()
    };
    std::fs::write(
        &mark,
        json!({"since": NOW - 300, "step": "clean", "child": child}).to_string(),
    )
    .unwrap();
    assert!(stuck(&c).is_empty(), "5 min is not stuck");
    std::fs::write(
        &mark,
        json!({"since": NOW - 700, "step": "clean", "child": child}).to_string(),
    )
    .unwrap();
    let later = herdsman::Ctx {
        now: NOW + 60,
        ..c.clone()
    };
    for c in [&c, &c, &later] {
        let e = stuck(c);
        assert_eq!(e.len(), 1, "once: {e:?}");
        assert_eq!((e[0].str("step"), e[0].str("child")), (Some("clean"), Some(child)));
        assert!(
            e[0].str("reason").unwrap().contains("tick running 11 min, in clean"),
            "{e:?}"
        );
    }
}
