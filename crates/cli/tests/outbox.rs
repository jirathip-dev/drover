mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::runner::{Out, Runner};
use serde_json::json;

const AIR_SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C -oControlPersist=10m air -- ";

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
fn a_remote_lane_only_queues_and_drive_drains_it_once() {
    let (md, ld) = (tmpdir("outbox-main"), tmpdir("outbox-lane"));
    let (main, lane) = (ctx(&md, CONFIG), ctx(&ld, CONFIG));
    let wt = ld.join("worktree");
    std::fs::create_dir_all(wt.join("src")).unwrap();
    std::fs::write(wt.join(".git"), "gitdir: elsewhere\n").unwrap();
    std::env::set_current_dir(wt.join("src")).unwrap();
    std::env::set_var("DROVER_LANE", "sendmeter:12:impl");
    std::env::set_var("DROVER_OUTBOX", "1");
    let r = Fake::default().reply("ssh", "q9\n");
    let (code, out) = run(
        &lane,
        &r,
        &["ask", "sendmeter", "12", "Which way?", "--option", "a", "--option", "b"],
    );
    assert_eq!((code, out.as_str()), (0, drover::outbox::QUEUED));
    let sha = "a".repeat(40);
    let verdict = [
        "verdict",
        "sendmeter",
        "12",
        "PASS",
        "--sha",
        &sha,
        "--file",
        "verdict.md",
    ];
    assert_eq!(run(&lane, &r, &verdict).0, 0);
    let (code, _) = run(&lane, &r, &["done", "sendmeter", "13", "--no-change", "nothing"]);
    assert_eq!(code, 0);
    assert!(
        !drover::db::file(&lane.state).exists(),
        "the lane host's log is never written"
    );
    assert!(
        r.calls.borrow().is_empty(),
        "no ssh, nothing run: {:?}",
        r.calls.borrow()
    );
    let ob = wt.join(".drover/outbox.jsonl");
    let mut lines = std::fs::read_to_string(&ob).unwrap();
    assert_eq!(lines.lines().count(), 3, "{lines}");
    let file = wt.canonicalize().unwrap().join("src/verdict.md").display().to_string();
    assert!(lines.contains(&format!("\"--file\",\"{file}\"")), "{lines}");
    let forged = json!({"args": ["done", "sendmeter", "12", "--no-change", "x"], "lane": "sendmeter:12:rev", "ts": 1});
    lines += &format!("{forged}\n");
    std::fs::write(&ob, &lines).unwrap();

    std::env::remove_var("DROVER_OUTBOX");
    std::env::remove_var("DROVER_LANE");
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude", "pane": "w9:p1", "workspace": "w9",
                   "worktree": wt, "branch": "issue-12", "host": "air", "machine": "air"});
    append(
        &main.state,
        &Event::new(1_000_000 - 60, "sendmeter", Some(12), "spawn", d),
    )
    .unwrap();
    let r = Air(Fake::default());
    run(&main, &r, &["drive", "--fast"]);
    let ev = read(&main.state).unwrap().to_vec();
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
    let ran: Vec<_> = ev
        .iter()
        .filter(|e| e.kind == "outbox" && e.val("args")[0] == "verdict")
        .collect();
    assert!(
        ran.len() == 1 && ran[0].data.get("refused").is_none(),
        "its own verdict runs: {ran:?}"
    );
    assert!(!ob.exists() && !wt.join(".drover/outbox.jsonl.draining").exists());
    let (_, board) = run(&main, &r, &["board", "sendmeter"]);
    let row = board.lines().find(|l| l.contains("#12")).unwrap_or_default();
    assert!(
        row.trim_end().ends_with("needs-"),
        "the ask is #12's last event: {board}"
    );
    let (_, asked) = run(&main, &r, &["asks", "sendmeter"]);
    assert!(asked.contains("q1 #12") && asked.contains("Which way?"), "{asked}");

    std::fs::write(wt.join(".drover/outbox.jsonl.draining"), &lines).unwrap();
    run(&main, &r, &["drive", "--fast"]);
    let ev = read(&main.state).unwrap();
    assert_eq!(ev.iter().filter(|e| e.kind == "needs-decision").count(), 1);
    assert_eq!(ev.iter().filter(|e| e.kind == "outbox").count(), 4);
    assert!(!wt.join(".drover/outbox.jsonl.draining").exists());

    let at = |s: u64| json!({"args": ["ask", "sendmeter", "12", "Late?"], "lane": "sendmeter:12:impl", "ts": s * 1000});
    std::fs::write(&ob, format!("{}\n", at(1_000_000 - 200))).unwrap();
    run(&main, &r, &["check"]);
    let stale = |c: &drover::Ctx| {
        read(&c.state)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "outbox-stale")
            .count()
    };
    assert_eq!(stale(&main), 0, "4 min old is not stale yet");
    std::fs::write(&ob, format!("{}\n{}\n", at(1_000_000 - 400), at(1_000_000 - 200))).unwrap();
    run(&main, &r, &["check"]);
    run(&main, &r, &["check"]);
    assert_eq!(stale(&main), 1);
    std::env::remove_var("DROVER_LANE");
}
