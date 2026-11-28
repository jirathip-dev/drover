mod common;
use common::*;
use drover::config::Limits;
use drover::events::{append, read, Event, IssueState};
use drover::flow::episode::Level;
use drover::flow::facts::{Ask, Host, Lane, Pr, Turn, Verdict};
use drover::flow::table::{outcomes, table, Action, Owner};
use drover::flow::{decide, Clock, Cond, Episode, Step};
use serde_json::json;

const NOW: u64 = 1_000_000;
const BASE: Cond = Cond {
    lane: Lane::Running,
    host: Host::Up,
    pr: Pr::None,
    verdict: Verdict::None,
    ask: Ask::None,
    turn: Turn::None,
};

fn tried(row: &str, n: u8) -> Episode {
    Episode {
        tries: [(row.to_string(), n)].into(),
        ..Episode::default()
    }
}

fn step(state: &str, c: Cond, ep: &Episode, k: Clock) -> Step {
    let s = IssueState {
        fleet: "sendmeter".into(),
        issue: 12,
        state: state.into(),
        ..IssueState::default()
    };
    decide(table().unwrap(), &s, &c, ep, &k, &Limits::default()).unwrap()
}

fn at(heard: u64, frame: u64) -> Clock {
    Clock {
        now: NOW,
        heard: Some(heard),
        frame: Some(frame),
        reset: None,
    }
}

#[test]
fn the_acceptance_walk_of_usage_md_runs_against_decide() {
    let l = Limits::default();
    let fresh = Episode::default();
    let now = at(NOW, NOW);
    let q = step("queued", BASE, &fresh, now);
    assert_eq!((q.row.as_str(), q.act), ("issue/queued/start", Some(Action::Start)));

    let ended = Cond {
        lane: Lane::Done,
        ..BASE
    };
    let quiet = at(NOW - l.stall_minutes * 60, NOW);
    let s = step("impl", ended, &fresh, quiet);
    assert_eq!(
        (s.row.as_str(), s.act, s.expired),
        ("issue/build/lane-ended", None, true)
    );
    let row = "issue/build/lane-ended";
    assert_eq!(step("impl", ended, &tried(row, 1), quiet).act, Some(Action::Nudge));
    assert_eq!(step("fix(1)", ended, &tried(row, 2), quiet).act, Some(Action::Respawn));
    let out = step("impl", ended, &tried(row, 3), quiet);
    assert_eq!((out.act, out.owner), (None, Owner::Orchestrator));

    let pushed = Cond {
        turn: Turn::Done,
        pr: Pr::Open,
        ..BASE
    };
    let s = step("impl", pushed, &fresh, now);
    assert_eq!(
        (s.row.as_str(), s.act),
        ("issue/build/pushed", Some(Action::SpawnCheck))
    );

    let written = Cond {
        lane: Lane::None,
        verdict: Verdict::Written,
        pr: Pr::Open,
        ..BASE
    };
    let s = step("review(1)", written, &fresh, now);
    assert_eq!((s.row.as_str(), s.act), ("issue/review/record", Some(Action::Record)));

    let refused = Cond {
        verdict: Verdict::Refused,
        ..BASE
    };
    let row = "issue/review/refused";
    assert_eq!(step("review(1)", refused, &fresh, now).act, Some(Action::TellCheck));
    assert_eq!(
        step("review(1)", refused, &tried(row, 1), now).act,
        Some(Action::RespawnCheck)
    );
    assert_eq!(
        step("review(1)", refused, &tried(row, 3), now).owner,
        Owner::Orchestrator
    );

    let green = Cond {
        verdict: Verdict::Pass,
        pr: Pr::Green,
        ..BASE
    };
    let s = step("ready", green, &fresh, now);
    assert_eq!((s.row.as_str(), s.act), ("issue/ready/merge", Some(Action::Merge)));

    let offline = Cond {
        host: Host::Offline,
        ..BASE
    };
    let silent = at(NOW, NOW - l.host_offline_minutes * 60);
    let s = step("review(2)", offline, &fresh, silent);
    assert_eq!(
        (s.row.as_str(), s.act, s.expired),
        ("issue/review/host-offline", None, true)
    );
    let row = "issue/review/host-offline";
    assert_eq!(
        step("review(2)", offline, &tried(row, 1), silent).act,
        Some(Action::Move)
    );

    let loaded = Cond {
        host: Host::Loaded,
        ..BASE
    };
    let waited = at(NOW - l.host_wait_minutes * 60, NOW);
    let s = step("impl", loaded, &fresh, waited);
    assert_eq!((s.row.as_str(), s.expired), ("issue/build/working-loaded", true));
    let row = "issue/build/working-loaded";
    assert_eq!(step("impl", loaded, &tried(row, 1), waited).act, Some(Action::Move));

    let late_frame = at(NOW - l.silent_minutes * 60 - 60, NOW - 120);
    let s = step("impl", BASE, &fresh, late_frame);
    assert_eq!((s.row.as_str(), s.act, s.expired), ("issue/build/working", None, false));

    let s = step("impl", offline, &fresh, silent);
    assert_eq!((s.row.as_str(), s.expired), ("issue/build/host-offline", true));
    assert_eq!(
        step("impl", offline, &tried("issue/build/host-offline", 1), silent).act,
        Some(Action::Move)
    );

    let row = "issue/build/lane-ended";
    let again = Episode {
        answered: [row.to_string()].into(),
        ..tried(row, 3)
    };
    let s = step("impl", ended, &again, quiet);
    assert_eq!((s.owner, s.level.name()), (Owner::Owner, "owner"));
}

#[test]
fn the_orchestrator_holds_a_row_for_escalate_minutes_then_the_owner_has_it() {
    let l = Limits::default();
    let row = "issue/build/lane-ended";
    let ended = Cond {
        lane: Lane::Done,
        ..BASE
    };
    let with = |at: u64| Episode {
        level: Level::Orchestrator { ask: String::new() },
        escalated: Some((row.into(), at)),
        ..tried(row, 3)
    };
    let k = at(NOW - l.stall_minutes * 60, NOW);
    assert_eq!(step("impl", ended, &with(NOW - 60), k).owner, Owner::Orchestrator);
    let late = step("impl", ended, &with(NOW - l.escalate_minutes * 60), k);
    assert_eq!(late.owner, Owner::Owner);
}

#[test]
fn every_classed_refusal_code_is_a_known_code_in_one_class() {
    let o = outcomes().unwrap();
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let mut src = String::new();
    let mut dirs = vec![std::path::PathBuf::from(root).join("crates/core/src")];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                src += &std::fs::read_to_string(p).unwrap();
            }
        }
    }
    let all: Vec<&String> = o.queued.iter().chain(&o.held).chain(&o.ends).collect();
    for c in &all {
        let doc = std::path::Path::new(root).join(format!("roles/usage/30-refusals/{c}.md"));
        assert!(
            doc.exists() || src.contains(&format!("\"{c}\"")),
            "{c}: no refusal code of that name"
        );
    }
    let mut once = all.clone();
    once.sort();
    once.dedup();
    assert_eq!(once.len(), all.len(), "a code in two classes");
    let classes = ["SERIAL_BUSY", "HOST_BUSY", "STUCK", "EXIT", "NO_REPRO"].map(|c| o.class(c));
    assert_eq!(classes, ["queued", "held", "final", "failed", "failed"]);
}

fn ready(name: &str, config: &str) -> (drover::Ctx, Fake, FakeShed) {
    let c = ctx(&tmpdir(name), config);
    let sh = FakeShed::new();
    sh.attach(&c);
    let wt = c.worktrees.join("sendmeter").join("issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let ev = |ts, kind: &str, d| append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
    let head = format!("abc{}", "0".repeat(37));
    ev(
        10,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12", "workspace": "",
            "runtime": "shed", "worktree": wt, "branch": "issue-12"}),
    );
    ev(30, "verdict", json!({"verdict": "PASS", "sha": head, "body": "PASS"}));
    let check = json!({"__typename": "CheckRun", "name": "actionlint", "status": "IN_PROGRESS",
        "conclusion": null, "databaseId": 4242, "startedAt": "1970-01-12T13:00:00Z",
        "detailsUrl": "https://github.com/jirathip-dev/sendmeter/actions/runs/777/job/4242"});
    let pr = json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": head,
        "statusCheckRollup": [check]});
    let r = Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("gh pr view issue-12", &pr.to_string())
        .reply(
            "gh api repos/jirathip-dev/sendmeter/actions/runs/777 --jq",
            "completed 1970-01-12T13:40:00Z\n",
        )
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply("gh run rerun", "")
        .reply("git -C", "");
    (c, r, sh)
}

const SHADOW: [&str; 4] = ["row-entered", "row-tried", "row-escalated", "flow-disagree"];

fn drive(c: &drover::Ctx, r: &Fake) -> (Vec<String>, Vec<Event>) {
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    let all = read(&c.state).unwrap().to_vec();
    let (shadow, today): (Vec<Event>, Vec<Event>) = all.into_iter().partition(|e| SHADOW.contains(&e.kind.as_str()));
    (today.into_iter().map(|e| e.kind).collect(), shadow)
}

#[test]
fn shadow_logs_the_tables_step_beside_todays_drive_and_changes_nothing_it_does() {
    let legacy_cfg = CONFIG.replace(
        "integration = \"staging\"   # never main",
        "integration = \"staging\"\nflow = \"legacy\"",
    );
    let (lc, lr, _ls) = ready("flow-legacy", &legacy_cfg);
    let (today_legacy, none) = drive(&lc, &lr);
    assert!(none.is_empty(), "legacy logs no shadow event: {none:?}");

    let (c, r, _sh) = ready("flow-shadow", CONFIG);
    let (today, shadow) = drive(&c, &r);
    assert_eq!(today, today_legacy, "today's drive logged the same events");
    let dir = |r: &Fake, c: &drover::Ctx| {
        let root = c.state.parent().unwrap().parent().unwrap().display().to_string();
        let calls = r.calls.borrow();
        let mine = calls.iter().filter(|x| !x.starts_with("date -r "));
        mine.map(|x| x.replace(&root, "")).collect::<Vec<_>>()
    };
    assert_eq!(dir(&r, &c), dir(&lr, &lc), "today's drive ran the same commands");

    let entered = shadow.iter().find(|e| e.kind == "row-entered").unwrap();
    assert_eq!(entered.str("row"), Some("issue/ready/ci-running"));
    assert_eq!(entered.data["shadow"], true);
    assert!(entered.is_passive());
    let action: Vec<&Event> = shadow.iter().filter(|e| e.kind == "flow-disagree").collect();
    assert_eq!(action.len(), 1, "{shadow:?}");
    assert_eq!(action[0].str("kind"), Some("action"));
    assert_eq!(action[0].data["table"]["owner"], "drive");

    let (_, again) = drive(&c, &r);
    let n = |v: &[Event], k: &str| v.iter().filter(|e| e.kind == k).count();
    assert_eq!(n(&again, "flow-disagree"), 1, "once per issue, row and episode");
    assert_eq!(n(&again, "row-entered"), 1, "the row did not change");

    let (code, out) = run(&c, &r, &["flow", "report", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("action issue/ready/ci-running · today wait · 1 ·"),
        "{out}"
    );
    assert!(out.contains("e.g. sendmeter #12"), "{out}");
}

#[test]
fn a_deadline_passed_spends_a_shadow_try_and_three_reach_the_orchestrator() {
    let (c, r, _sh) = ready("flow-tries", CONFIG);
    drive(&c, &r);
    let l = Limits::default();
    let mut c = c;
    for _ in 0..3 {
        c.now += l.ci_stuck_minutes * 60 + 1;
        drive(&c, &r);
    }
    let (_, shadow) = drive(&c, &r);
    let tries: Vec<&Event> = shadow.iter().filter(|e| e.kind == "row-tried").collect();
    assert_eq!(tries.len(), 3, "{shadow:?}");
    assert!(tries
        .iter()
        .all(|e| e.str("code") == Some("TIMEOUT") && e.data["shadow"] == true));
    assert_eq!(tries[1].str("action"), Some("unstick-ci"));
    let up: Vec<&Event> = shadow.iter().filter(|e| e.kind == "row-escalated").collect();
    assert_eq!(up.len(), 1);
    assert_eq!(up[0].str("level"), Some("orchestrator"));
    assert!(
        !read(&c.state).unwrap().iter().any(|e| e.kind == "needs-human"),
        "a shadow escalation raises no card"
    );
    c.now += l.act_minutes * 60;
    let (_, shadow) = drive(&c, &r);
    let esc: Vec<&Event> = (shadow.iter())
        .filter(|e| e.kind == "flow-disagree" && e.str("kind") == Some("escalation"))
        .collect();
    assert_eq!(esc.len(), 1, "the table reached a person and today did not");
    assert_eq!(
        (esc[0].str("table_level"), esc[0].data["drive_at"].is_null()),
        (Some("orchestrator"), true)
    );
}
