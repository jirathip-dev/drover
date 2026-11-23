mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::runner::Out;
use serde_json::json;

const NOW: u64 = 1_000_000;
const SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C -oControlPersist=10m bazzite -- ";
const WT: &str = "/home/me/.local/share/drover/work/sendmeter/issue-12";
const LANE: &str = "sendmeter-rev-12-r1";
const DOWN: &str = "ssh: connect to host bazzite port 22: Connection timed out";

fn head() -> String {
    "a".repeat(40)
}

fn setup(name: &str) -> (drover::Ctx, String) {
    let rev = tmpdir(&format!("{name}-rev")).display().to_string();
    let base = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
    let c = ctx(
        &tmpdir(name),
        &format!("{base}\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n"),
    );
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "bazzite": {"tags": ["os:linux"], "home": "/home/me", "probed": NOW, "seen": NOW, "up": true,
                    "load": 0.1, "mem": 12.0, "disk": 200.0},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    let ev = |ts, d| append(&c.state, &Event::new(ts, "sendmeter", Some(12), "spawn", d)).unwrap();
    ev(
        NOW - 900,
        json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude", "pane": "sendmeter-impl-12",
               "workspace": "", "worktree": WT, "branch": "issue-12", "host": "bazzite", "machine": "bazzite"}),
    );
    ev(
        NOW - 600,
        json!({"role": "rev", "agent": LANE, "harness": "claude", "pane": LANE, "workspace": "", "worktree": rev,
               "branch": "rev-12-r1", "sha": head(), "host": "bazzite", "machine": "bazzite"}),
    );
    (c, rev)
}

fn boat(rev: &str, verdict: Result<&str, &str>) -> Fake {
    let pr = json!({"number": 34, "state": "OPEN", "baseRefName": "staging", "headRefOid": head(),
                    "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "IN_PROGRESS"}]});
    let mut f = Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("gh pr view issue-12", &pr.to_string())
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{}\n", head()),
        )
        .reply(
            &format!("{SSH}'git' '--no-replace-objects' '-C' '{WT}' 'log'"),
            &format!("{} 20", head()),
        )
        .reply(&format!("{SSH}'sh' '-c' 'test -e"), "")
        .reply(
            &format!("{SSH}'git' '--no-replace-objects' '-C' '{rev}' 'rev-list'"),
            "0\n",
        )
        .reply(&format!("{SSH}'git' '--no-replace-objects' '-C' '{rev}' 'status'"), "")
        .reply(
            &format!("{SSH}'git' '--no-replace-objects' '-C' '{rev}' 'worktree' 'remove'"),
            "",
        )
        .reply(&format!("{SSH}'rm'"), "");
    match verdict {
        Ok(text) => std::fs::write(format!("{rev}/verdict.md"), text).unwrap(),
        Err(why) => {
            let out = Out {
                code: 255,
                stderr: why.into(),
                ..Out::default()
            };
            f.replies
                .insert(0, (format!("{SSH}'/opt/drover/bin/drover' 'hosts' 'report'"), out));
        }
    }
    f
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == kind)
        .cloned()
        .collect()
}

#[test]
fn a_remote_reviewer_that_exited_leaving_verdict_md_is_recorded_next_pass_after_a_failed_pickup() {
    let (c, rev) = setup("remote-verdict-pickup");
    let (_sh, bazzite) = (FakeShed::attached(&c), FakeShed::new());
    bazzite.lane(LANE, "idle", &rev);
    bazzite.exit(LANE);
    let verdict = format!("PASS\n\nReviewed head: {}\n", head());

    let down = boat(&rev, Err(DOWN));
    let (code, out) = run(&c, &two(&down, &bazzite), &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(kinds(&c, "verdict").is_empty(), "{out}");
    let rev_spawns = |c: &drover::Ctx| {
        kinds(c, "spawn")
            .iter()
            .filter(|e| e.str("role") == Some("rev"))
            .count()
    };
    assert_eq!(rev_spawns(&c), 1, "a finished review is not thrown away: {out}");
    let reviewed = |c: &drover::Ctx| kinds(c, "driven").iter().any(|e| e.str("step") == Some("review"));
    assert!(!reviewed(&c), "no re-review: {out}");
    let failed = kinds(&c, "verdict-pickup-failed");
    assert_eq!(failed.len(), 1, "{out}");
    assert!(
        failed[0]
            .str("why")
            .unwrap_or_default()
            .contains("bazzite: not sent (host offline this pass)"),
        "{failed:?}"
    );
    let (_, waiting) = run(&c, &two(&down, &bazzite), &["waiting", "sendmeter"]);
    assert!(waiting.contains("verdict pickup failed"), "{waiting}");
    assert!(!waiting.contains("recording within 1 min"), "{waiting}");

    let up = boat(&rev, Ok(&verdict));
    let next = drover::Ctx {
        now: c.now + 60,
        ..c.clone()
    };
    let (code, out) = run(&next, &two(&up, &bazzite), &["drive"]);
    assert_eq!(code, 0, "{out}");
    let v = kinds(&c, "verdict");
    assert_eq!(v.len(), 1, "{out}");
    assert_eq!(
        (v[0].str("verdict"), v[0].str("sha")),
        (Some("PASS"), Some(head().as_str()))
    );
    assert!(!reviewed(&c), "{out}");
}

#[test]
fn a_verdict_md_in_the_hosts_report_is_recorded_in_the_same_pass() {
    let (c, rev) = setup("remote-verdict-fetch");
    let (_sh, bazzite) = (FakeShed::attached(&c), FakeShed::new());
    bazzite.lane(LANE, "idle", &rev);
    let verdict = format!(
        "FAIL\n\nReviewed head: {}\n- blocking [code] a.rs:1: x\n  Proof: spec\n  > AC 1\n",
        head()
    );
    let f = boat(&rev, Ok(&verdict));
    let (code, out) = run(&c, &two(&f, &bazzite), &["drive"]);
    assert_eq!(code, 0, "{out}");
    let v = kinds(&c, "verdict");
    assert_eq!(v.len(), 1, "{out}");
    assert_eq!(
        (v[0].str("verdict"), v[0].str("sha")),
        (Some("FAIL"), Some(head().as_str()))
    );
    assert!(kinds(&c, "verdict-pickup-failed").is_empty(), "{out}");
}

fn fix_due(c: &drover::Ctx, n: u64) {
    let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
    std::fs::create_dir_all(&wt).unwrap();
    let d = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "pane": format!("sendmeter-impl-{n}"),
        "worktree": wt, "branch": format!("issue-{n}")});
    let at = |kind, d| Event::new(NOW - 1200, "sendmeter", Some(n), kind, d);
    append(&c.state, &at("spawn", d)).unwrap();
    let v = json!({"verdict": "FAIL", "sha": "c".repeat(40), "body": "FAIL\nsrc/x.rs:3 off by one"});
    append(&c.state, &at("verdict", v)).unwrap();
}

#[test]
fn a_later_step_failing_on_a_full_disk_leaves_the_verdict_and_the_fix_round_done() {
    let (c, rev) = setup("remote-verdict-disk-full");
    fix_due(&c, 21);
    for a in [
        &["queue", "sendmeter", "add", "40"][..],
        &["queue", "sendmeter", "block", "--on", "41", "40"],
    ] {
        assert_eq!(run(&c, &Fake::default(), a).0, 0);
    }
    let db = rusqlite::Connection::open(c.state.with_file_name("state.db")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER full BEFORE INSERT ON events WHEN NEW.type = 'blocker-closed' \
         BEGIN SELECT RAISE(ABORT, 'database or disk is full'); END;",
    )
    .unwrap();
    let (_sh, bazzite) = (FakeShed::attached(&c), FakeShed::new());
    bazzite.lane(LANE, "idle", &rev);
    bazzite.exit(LANE);
    let verdict = format!("PASS\n\nReviewed head: {}\n", head());
    let f = boat(&rev, Ok(&verdict)).reply("gh issue view 41", r#"{"state":"CLOSED"}"#);
    let (_, out) = run(&c, &two(&f, &bazzite), &["drive"]);
    let v = kinds(&c, "verdict");
    assert!(
        v.iter()
            .any(|e| e.issue == Some(12) && e.str("verdict") == Some("PASS")),
        "the remote verdict is recorded: {out}"
    );
    let fix = kinds(&c, "driven");
    assert!(
        fix.iter().any(|e| e.issue == Some(21)
            && e.str("cmd")
                .is_some_and(|c| c.starts_with("drover spawn sendmeter 21 impl"))),
        "the fix round is started: {out}"
    );
    assert!(
        out.contains("database or disk is full"),
        "the failed step is logged: {out}"
    );
}

fn two<'a>(f: &'a Fake, bazzite: &'a FakeShed) -> OverSsh<'a> {
    OverSsh {
        f,
        hosts: vec![("bazzite", bazzite)],
    }
}

#[test]
fn with_an_admission_pending_a_ready_pr_merges_and_a_remote_verdict_is_recorded_in_the_same_pass() {
    let (c, rev) = setup("remote-verdict-1625-admission");
    let cfg = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(
        &c.config,
        cfg.replace("max_parallel_spawns = 0", "max_parallel_spawns = 6"),
    )
    .unwrap();
    let (sh, bazzite) = (FakeShed::attached(&c), FakeShed::new());
    bazzite.lane(LANE, "idle", &rev);
    bazzite.exit(LANE);
    let wt = c.worktrees.join("sendmeter/issue-5");
    std::fs::create_dir_all(&wt).unwrap();
    let wt = wt.display().to_string();
    let ready = "b".repeat(40);
    let at = |kind, d| append(&c.state, &Event::new(NOW - 800, "sendmeter", Some(5), kind, d)).unwrap();
    at(
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-5", "harness": "claude", "pane": "sendmeter-impl-5",
               "worktree": wt, "branch": "issue-5"}),
    );
    at("verdict", json!({"verdict": "PASS", "sha": ready, "body": "PASS"}));
    sh.lane("sendmeter-impl-5", "idle", &wt);
    for k in ["admit-local.lock", "admit-mini.lock", "admit-bazzite.lock"] {
        lease_as(&c, k, 4242, drover::now(), 600);
    }
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "7"]).0, 0);
    let green = json!({"number": 105, "state": "OPEN", "baseRefName": "staging", "headRefOid": ready,
                       "mergeable": "MERGEABLE",
                       "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}]});
    let f = boat(&rev, Ok(&format!("PASS\n\nReviewed head: {}\n", head())))
        .reply("kill -0 4242", "")
        .reply("ps -o etime= -p 4242", "  10:00\n")
        .reply("gh pr view issue-5", &green.to_string())
        .reply("gh pr view 105", r#"{"mergeCommit":{"oid":"m3rg3"}}"#)
        .reply("gh pr merge 105", "")
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-5",
            &format!("{ready}\n"),
        )
        .reply("gh api -X POST repos/jirathip-dev/sendmeter/statuses/", "{}")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("gh issue", "")
        .reply("sh -c nohup", "4321\n")
        .reply("git -C", "");
    let (code, out) = run(&c, &two(&f, &bazzite), &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("REFUSED SPAWN_PENDING"),
        "the start waits on the admission: {out}"
    );
    let kicks: Vec<String> = f
        .calls
        .borrow()
        .iter()
        .filter(|l| l.contains(" --why plan "))
        .cloned()
        .collect();
    let mut ran = String::new();
    for k in &kicks {
        let a: Vec<&str> = k.split(' ').skip(4).take_while(|w| *w != ">>").collect();
        let (code, o) = run_only(&c, &two(&f, &bazzite), &a);
        assert_eq!(code, 0, "{o}");
        ran += &o;
    }
    let v = kinds(&c, "verdict");
    assert!(
        v.iter()
            .any(|e| e.issue == Some(12) && e.str("verdict") == Some("PASS")),
        "{out}"
    );
    assert!(kinds(&c, "merged").iter().any(|e| e.issue == Some(5)), "{out}\n{ran}");
    assert!(
        lease_on(&c, "admit-bazzite.lock").is_some_and(|l| l.pid == 4242),
        "still pending"
    );
}
