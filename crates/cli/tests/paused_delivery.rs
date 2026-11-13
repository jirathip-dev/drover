//! #714: pause holds back spawns and passes, never delivery: a paused fleet's live remote lane still has its outbox
//! drained (its ask and turn land) and an answer that never reached it is typed again on the next fast step.
//! One test: the lane's env vars and cwd are process-wide.
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use herdsman::runner::{Out, Runner};
use serde_json::json;

const AIR_SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/herdsman-ssh-%C -oControlPersist=10m air -- ";

/// The main host's runner: a shell line over ssh to `air` runs here (the lane's worktree is a local folder).
struct Air(Fake);

impl Runner for Air {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        let line = format!("{prog} {}", args.join(" "));
        let Some(sh) = line.strip_prefix(AIR_SSH) else {
            return self.0.run(prog, args);
        };
        let o = std::process::Command::new("sh").args(["-c", sh]).output()?;
        Ok(Out {
            code: o.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&o.stdout).into(),
            stderr: String::from_utf8_lossy(&o.stderr).into(),
        })
    }
}

#[test]
fn a_paused_fleets_live_remote_lane_still_gets_its_outbox_drained_and_its_answers() {
    let (md, ld) = (tmpdir("paused-main"), tmpdir("paused-lane"));
    let (main, lane) = (ctx(&md, CONFIG), ctx(&ld, CONFIG));
    let wt = ld.join("worktree");
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(wt.join(".git"), "gitdir: elsewhere\n").unwrap();
    std::env::set_current_dir(&wt).unwrap();
    std::env::set_var("HERDSMAN_LANE", "sendmeter:12:impl");
    std::env::set_var("HERDSMAN_OUTBOX", "1");
    let ask = ["ask", "sendmeter", "12", "Which way?", "--option", "a", "--option", "b"];
    assert_eq!(run(&lane, &Fake::default(), &ask).0, 0);
    assert_eq!(run(&lane, &Fake::default(), &["turn", "need-input"]).0, 0);
    let ob = wt.join(".herdsman/outbox.jsonl");
    assert_eq!(std::fs::read_to_string(&ob).unwrap().lines().count(), 2);
    std::env::remove_var("HERDSMAN_OUTBOX");
    std::env::remove_var("HERDSMAN_LANE");

    // The main host: the fleet is paused, #12's lane lives on air, and #13 waits in the queue.
    assert_eq!(run(&main, &Fake::default(), &["queue", "sendmeter", "add", "13"]).0, 0);
    assert_eq!(run(&main, &Fake::default(), &["pause", "sendmeter"]).0, 0);
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
    assert_eq!(asks[0].str("agent"), Some("sendmeter-impl-12"));
    let turns: Vec<_> = ev.iter().filter(|e| e.kind == "turn-end").collect();
    assert!(turns.len() == 1 && turns[0].issue == Some(12), "{ev:?}");
    assert!(!ob.exists() && !wt.join(".herdsman/outbox.jsonl.draining").exists());
    let (_, asked) = run(&main, &r, &["asks", "sendmeter"]);
    assert!(asked.contains("q1 #12") && asked.contains("Which way?"), "{asked}");

    // The answer can't reach the lane at first: the next fast step types it again, paused or not.
    assert_eq!(run(&main, &Fake::default(), &["answer", "sendmeter", "q1", "1"]).0, 0);
    assert_eq!(ev_count(&main, "answer-undelivered"), 1);
    let up = Air(Fake::default()
        .reply("herdr --machine air agent prompt", "")
        .reply("herdr --machine air pane send-keys", "")
        .reply("herdr --machine air agent wait", ""));
    run(&main, &up, &["drive", "--fast"]);
    assert!(
        up.0.called("herdr --machine air agent prompt sendmeter-impl-12 DECISION q1: a"),
        "{:?}",
        up.0.calls.borrow()
    );
    assert_eq!(ev_count(&main, "answer-delivered"), 1);

    // Pause still holds: a slow pass starts nothing, and no pass is logged for the fleet.
    run(&main, &Air(Fake::default()), &["drive"]);
    assert_eq!(ev_count(&main, "spawn"), 1, "only #12's seeded spawn");
    assert_eq!(ev_count(&main, "drive-pass"), 0);
    assert_eq!(herdsman::drive::last_pass(&main, "sendmeter"), None);
}

fn ev_count(c: &herdsman::Ctx, kind: &str) -> usize {
    read(&c.state).unwrap().iter().filter(|e| e.kind == kind).count()
}
