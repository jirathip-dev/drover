//! #389: a lane on another host whose ssh forward to the main host fails queues its command in the worktree's
//! outbox; drive's fast step drains it over main → host ssh, as the lane its spawn names. One test: the lane's env
//! vars and cwd are process-wide.
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use herdsman::runner::{Out, Runner};
use serde_json::json;

const MAIN_SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/herdsman-ssh-%C -oControlPersist=10m mini -- ";
const AIR_SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/herdsman-ssh-%C -oControlPersist=10m air -- ";

/// The main host's runner: a shell line over ssh to `air` runs here (the lane's worktree is a local folder).
struct Air(Fake);

impl Runner for Air {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        let line = format!("{prog} {}", args.join(" "));
        let Some(sh) = line.strip_prefix(AIR_SSH) else {
            return self.0.run(prog, args);
        };
        self.0.calls.borrow_mut().push(line.clone());
        let o = std::process::Command::new("sh").args(["-c", sh]).output()?;
        Ok(Out {
            code: o.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&o.stdout).into(),
            stderr: String::from_utf8_lossy(&o.stderr).into(),
        })
    }
}

#[test]
fn a_lane_without_a_route_queues_and_drive_drains_it_once() {
    let (md, ld) = (tmpdir("outbox-main"), tmpdir("outbox-lane"));
    let (main, lane) = (ctx(&md, CONFIG), ctx(&ld, CONFIG));
    let wt = ld.join("worktree");
    std::fs::create_dir_all(wt.join("src")).unwrap();
    std::fs::write(wt.join(".git"), "gitdir: elsewhere\n").unwrap();
    std::env::set_current_dir(wt.join("src")).unwrap();
    std::env::set_var("HERDSMAN_LANE", "sendmeter:12:impl");
    std::env::set_var("HERDSMAN_MAIN", "mini:/opt/herdsman/bin/herdsman");
    let no_route = "ssh: Could not resolve hostname mini: nodename nor servname provided";
    let r = Fake::default()
        .once_err(MAIN_SSH, 255, no_route)
        .once_err(MAIN_SSH, 255, no_route);
    let (code, out) = run(
        &lane,
        &r,
        &["ask", "sendmeter", "12", "Which way?", "--option", "a", "--option", "b"],
    );
    assert_eq!(
        (code, out.as_str()),
        (0, herdsman::outbox::QUEUED),
        "{:?}",
        r.calls.borrow()
    );
    // A command for another issue is queued too: drive refuses it.
    let (code, _) = run(&lane, &r, &["done", "sendmeter", "13", "--no-change", "nothing"]);
    assert_eq!(code, 0);
    assert!(!lane.state.exists(), "the lane host's log is never written");
    let ob = wt.join(".herdsman/outbox.jsonl");
    let mut lines = std::fs::read_to_string(&ob).unwrap();
    assert_eq!(lines.lines().count(), 2, "{lines}");
    // A line that names another role of the issue is refused as well.
    let forged = json!({"args": ["done", "sendmeter", "12", "--no-change", "x"], "lane": "sendmeter:12:rev", "ts": 1});
    lines += &format!("{forged}\n");
    std::fs::write(&ob, &lines).unwrap();

    // The main host: the lane's spawn names its host and worktree; its ssh session has no `HERDSMAN_MAIN`.
    std::env::remove_var("HERDSMAN_MAIN");
    std::env::remove_var("HERDSMAN_LANE");
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude", "pane": "w9:p1", "workspace": "w9",
                   "worktree": wt, "branch": "issue-12", "host": "air", "machine": "air"});
    append(
        &main.state,
        &Event::new(1_000_000 - 60, "sendmeter", Some(12), "spawn", d),
    )
    .unwrap();
    let r = Air(Fake::default());
    run(&main, &r, &["drive", "--fast"]);
    let ev = read(&main.state).unwrap();
    let asks: Vec<_> = ev.iter().filter(|e| e.kind == "needs-decision").collect();
    assert_eq!(asks.len(), 1, "{ev:?}");
    assert_eq!(
        (asks[0].issue, asks[0].str("agent"), asks[0].str("question")),
        (Some(12), Some("sendmeter-impl-12"), Some("Which way?"))
    );
    assert!(
        !ev.iter().any(|e| e.kind == "fix-done"),
        "no line ran for #13 or as rev"
    );
    let refused: Vec<_> = ev
        .iter()
        .filter(|e| e.kind == "outbox" && e.data.get("refused").is_some())
        .collect();
    assert_eq!(refused.len(), 2, "{ev:?}");
    assert!(
        refused[0].str("refused").unwrap().contains("sendmeter#13"),
        "{:?}",
        refused[0]
    );
    assert!(
        refused[1].str("refused").unwrap().contains("sendmeter:12:rev"),
        "{:?}",
        refused[1]
    );
    assert!(!ob.exists() && !wt.join(".herdsman/outbox.jsonl.draining").exists());
    let (_, board) = run(&main, &r, &["board", "sendmeter"]);
    let row = board.lines().find(|l| l.contains("#12")).unwrap_or_default();
    assert!(
        row.trim_end().ends_with("needs-"),
        "the ask is #12's last event: {board}"
    );
    let (_, asked) = run(&main, &r, &["asks", "sendmeter"]);
    assert!(asked.contains("q1 #12") && asked.contains("Which way?"), "{asked}");

    // A drain cut short leaves its lines behind: the next one applies none of them again.
    std::fs::write(wt.join(".herdsman/outbox.jsonl.draining"), &lines).unwrap();
    run(&main, &r, &["drive", "--fast"]);
    let ev = read(&main.state).unwrap();
    assert_eq!(ev.iter().filter(|e| e.kind == "needs-decision").count(), 1);
    assert_eq!(ev.iter().filter(|e| e.kind == "outbox").count(), 3);
    assert!(!wt.join(".herdsman/outbox.jsonl.draining").exists());
}
