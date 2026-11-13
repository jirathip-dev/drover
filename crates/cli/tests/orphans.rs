//! #376: lanes drover never logged: `spawn-started` before the harness starts, and each drive step adopts a
//! lane-shaped orphan pane of a tracked issue (its verdict then recorded as any lane's), closes a stale one, and leaves
//! an unmatched one to `check`'s `orphan-lane` alert.
mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::json;
use std::path::PathBuf;

const WS: &str = r#"{"result":{"workspace":{"workspace_id":"w9"}}}"#;

fn sha(p: &str) -> String {
    format!("{p}{}", "0".repeat(40 - p.len()))
}

fn ev(c: &drover::Ctx, ts: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
}

fn pr(head: &str) -> String {
    let check = json!({"__typename": "CheckRun", "name": "ci", "status": "IN_PROGRESS"});
    json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": head, "statusCheckRollup": [check]})
        .to_string()
}

/// The pane-get answer of a pane running a harness.
fn harness(pane: &str) -> String {
    json!({"result": {"pane": {"pane_id": pane, "agent": "hermes", "workspace_id": "w9"}}}).to_string()
}

fn fake(agents: &str) -> Fake {
    Fake::default()
        .reply("herdr agent list", agents)
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
}

fn fast(c: &drover::Ctx, r: &Fake) -> String {
    let (code, out) = run(c, r, &["drive", "--fast"]);
    assert_eq!(code, 0, "{out} {:?}", r.calls.borrow());
    out
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

/// #12's impl lane, logged, and a review worktree no spawn logged holding `verdict`.
fn review(c: &drover::Ctx, verdict: &str) -> PathBuf {
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "w9:p1", "workspace": "w9", "worktree": wt,
        "branch": "issue-12"});
    ev(c, 10, "spawn", d);
    let rev = c.worktrees.join("sendmeter/rev-12-r1");
    std::fs::create_dir_all(&rev).unwrap();
    std::fs::write(rev.join("verdict.md"), verdict).unwrap();
    rev
}

#[test]
fn spawn_logs_spawn_started_before_the_harness_starts() {
    let c = ctx(&tmpdir("orphan-started"), CONFIG);
    let r = fake(NO_AGENTS)
        .reply("git -C", "")
        .reply("herdr worktree", WS)
        .reply(
            "herdr pane list",
            r#"{"result":{"panes":[{"pane_id":"w9:p1","agent":null}]}}"#,
        )
        .reply("herdr pane run", "{}")
        .reply("herdr pane", "{}")
        .once_err("herdr agent start", 1, "hermes: command not found");
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 1, "{out}");
    let st = &kinds(&c, "spawn-started")[0];
    assert_eq!(
        (st.str("agent"), st.str("role"), st.str("pane")),
        (Some("sendmeter-impl-12"), Some("impl"), Some("w9:p1"))
    );
    assert_eq!(
        PathBuf::from(st.str("worktree").unwrap()),
        c.worktrees.join("sendmeter/issue-12")
    );
    assert!(kinds(&c, "spawn").is_empty(), "the start failed: no spawn");
    // Passive: the issue's state is untouched.
    assert_eq!(
        fold(&read(&c.state).unwrap())[&("sendmeter".into(), 12)].state,
        "queued"
    );
}

/// A crash between `spawn-started` and `spawn`: the next fast step adopts the running pane.
#[test]
fn a_crash_after_spawn_started_is_adopted_by_the_next_step() {
    let c = ctx(&tmpdir("orphan-crash"), CONFIG);
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    ev(
        &c,
        50,
        "spawn-started",
        json!({"agent": "sendmeter-impl-12", "role": "impl", "worktree": wt, "pane": "w9:p1"}),
    );
    let list = agents(&[("sendmeter-impl-12", "working", &wt.to_string_lossy(), "w9:p1")]);
    let r = fake(&list)
        .reply("herdr pane get w9:p1", &harness("w9:p1"))
        .reply("herdr agent rename w9:p1 sendmeter-impl-12", "{}");
    let out = fast(&c, &r);
    assert!(out.contains("adopted w9:p1 as sendmeter-impl-12"), "{out}");
    let sp = &kinds(&c, "spawn")[0];
    assert_eq!(
        (sp.str("pane"), sp.str("why"), sp.data["since"].as_u64()),
        (Some("w9:p1"), Some("orphan"), Some(50))
    );
    assert_eq!(kinds(&c, "spawn-adopted")[0].str("why"), Some("orphan"));
    assert_eq!(fold(&read(&c.state).unwrap())[&("sendmeter".into(), 12)].state, "impl");
    // Known now: the next step adopts nothing again.
    fast(&c, &r);
    assert_eq!(kinds(&c, "spawn").len(), 1);
}

/// A finished review no spawn logged, its verdict at the PR head: adopted, recorded, then its pane closed.
#[test]
fn a_finished_orphan_review_at_the_head_is_recorded_then_closed() {
    let c = ctx(&tmpdir("orphan-review"), CONFIG);
    let head = sha("abc");
    let rev = review(&c, &format!("PASS\nReviewed head: {head}\nall good\n"));
    let list = agents(&[("sendmeter-rev-12-r1", "done", &rev.to_string_lossy(), "w9:p2")]);
    let r = fake(&list)
        .reply("herdr pane get w9:p2", &harness("w9:p2"))
        .reply("herdr agent rename w9:p2 sendmeter-rev-12-r1", "{}")
        .reply("gh pr view issue-12", &pr(&head))
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{head}\n"),
        )
        .reply("herdr worktree", WS)
        .reply("herdr pane close w9:p2", "{}")
        .reply(&format!("git -C {} rev-list", rev.display()), "0\n")
        .reply("git -C", "");
    let out = fast(&c, &r);
    let adopted = &kinds(&c, "spawn")[1];
    assert_eq!((adopted.str("role"), adopted.str("why")), (Some("rev"), Some("orphan")));
    let v = &kinds(&c, "verdict")[0];
    assert_eq!(
        (v.str("verdict"), v.str("sha")),
        (Some("PASS"), Some(head.as_str())),
        "{out}"
    );
    let closed = kinds(&c, "lane-closed");
    assert_eq!(
        closed.first().and_then(|e| e.str("reason")),
        Some("verdict recorded"),
        "{out} {:?}",
        read(&c.state).unwrap()
    );
    let (close, rename) = (
        r.index("herdr pane close w9:p2").unwrap(),
        r.index("herdr agent rename").unwrap(),
    );
    assert!(close > rename, "{:?}", r.calls.borrow());
}

/// A finished review whose verdict is for an old head: its unpushed work goes to wip/, the pane closes, no verdict.
#[test]
fn a_stale_head_orphan_review_is_closed_not_recorded() {
    let c = ctx(&tmpdir("orphan-stale"), CONFIG);
    let rev = review(&c, &format!("PASS\nReviewed head: {}\n", sha("0d")));
    let list = agents(&[("sendmeter-rev-12-r1", "done", &rev.to_string_lossy(), "w9:p2")]);
    let r = fake(&list)
        .reply("gh pr view issue-12", &pr(&sha("bee")))
        .reply(&format!("git -C {} rev-list", rev.display()), "1\n")
        .reply("herdr pane close w9:p2", "{}")
        .reply("git -C", "");
    let out = fast(&c, &r);
    assert!(
        out.contains("closed stale orphan sendmeter-rev-12-r1 (verdict for 0d00000, head is bee0000)"),
        "{out}"
    );
    let closed = &kinds(&c, "lane-closed")[0];
    assert_eq!(
        (closed.str("why"), closed.str("pane")),
        (Some("stale-orphan"), Some("w9:p2"))
    );
    let push = format!(
        "git -C {} push origin HEAD:refs/heads/wip/sendmeter-rev-12-",
        rev.display()
    );
    assert!(r.index(&push).unwrap() < r.index("herdr pane close w9:p2").unwrap());
    assert!(
        kinds(&c, "verdict").is_empty() && kinds(&c, "spawn").len() == 1,
        "not recorded, not adopted"
    );
    assert!(!r.called("herdr agent rename"));
}

/// A merged issue's orphan pane is stale too.
#[test]
fn a_merged_issues_orphan_is_closed() {
    let c = ctx(&tmpdir("orphan-merged"), CONFIG);
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    ev(&c, 10, "merged", json!({}));
    let list = agents(&[("", "idle", &wt.to_string_lossy(), "w9:p7")]);
    let r = fake(&list).reply("herdr pane close w9:p7", "{}").reply("git -C", "");
    let out = fast(&c, &r);
    assert!(
        out.contains("closed stale orphan w9:p7 (sendmeter#12 is merged)"),
        "{out}"
    );
    assert_eq!(kinds(&c, "lane-closed")[0].str("why"), Some("stale-orphan"));
}

/// A lane-shaped pane of no tracked issue: drive leaves it, `check` alerts it once; a tracked one is not alerted.
#[test]
fn an_unmatched_pane_is_only_alerted() {
    let c = ctx(&tmpdir("orphan-unmatched"), CONFIG);
    let wt = c.worktrees.join("sendmeter/issue-12");
    ev(
        &c,
        50,
        "spawn-started",
        json!({"agent": "sendmeter-impl-12", "role": "impl", "pane": "w9:p1"}),
    );
    let list = agents(&[
        ("sendmeter-impl-99", "idle", "/tmp/elsewhere", "w9:p9"),
        ("sendmeter-impl-12", "working", &wt.to_string_lossy(), "w9:p1"),
    ]);
    let r = fake(&list).reply("git -C", "");
    assert_eq!(run(&c, &r, &["check"]).0, 0);
    let alerts = kinds(&c, "orphan-lane");
    assert_eq!(alerts.len(), 1);
    assert_eq!(
        (alerts[0].issue, alerts[0].str("key")),
        (Some(99), Some("sendmeter-impl-99"))
    );
    fast(&c, &r);
    assert!(!r.called("herdr agent rename w9:p9") && !r.called("herdr pane close w9:p9"));
    assert!(kinds(&c, "spawn").iter().all(|e| e.issue == Some(12)));
}

/// An orphan on another host (`herdr --machine`) is adopted there, and its spawn names the host.
#[test]
fn a_remote_orphan_is_adopted_on_its_host() {
    let cfg = format!("{CONFIG}\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n");
    let c = ctx(&tmpdir("orphan-remote"), &cfg);
    ev(&c, 10, "queued", json!({}));
    let wt = "/home/me/.herdr/worktrees/sendmeter/issue-12";
    let list = agents(&[("sendmeter-impl-12", "working", wt, "w3:p1")]);
    let r = fake(NO_AGENTS)
        .reply("herdr --machine bazzite agent list", &list)
        .reply("herdr --machine bazzite pane get w3:p1", &harness("w3:p1"))
        .reply("herdr --machine bazzite agent rename w3:p1 sendmeter-impl-12", "{}");
    let out = fast(&c, &r);
    assert!(out.contains("adopted w3:p1"), "{out} {:?}", r.calls.borrow());
    let sp = &kinds(&c, "spawn")[0];
    assert_eq!(
        (sp.str("host"), sp.str("machine"), sp.str("worktree")),
        (Some("bazzite"), Some("bazzite"), Some(wt))
    );
    assert!(!r.called("herdr agent rename"), "never renamed on this host");
}

/// #393: a runner that, while drive lists the remote host's agents, logs `spawn-started` as a concurrent spawn does
/// before it starts the harness (its `spawn` comes once the prompt is confirmed, after this pass).
struct Racing<'a>(&'a Fake, &'a drover::Ctx, &'a str);

impl drover::runner::Runner for Racing<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        let line = format!("{prog} {}", args.join(" "));
        if line.contains("bazzite") && line.contains("list") && kinds(self.1, "spawn-started").is_empty() {
            let d = json!({"agent": "sendmeter-impl-12", "role": "impl", "worktree": self.2, "pane": "w3:p1",
                "host": "bazzite", "machine": "bazzite"});
            ev(self.1, self.1.now, "spawn-started", d);
        }
        self.0.run(prog, args)
    }
}

/// #393: a remote lane whose spawn is still running when the sweep lists it is not adopted; an unlogged one is.
#[test]
fn a_lane_spawned_during_the_remote_listing_is_not_adopted() {
    let cfg = format!("{CONFIG}\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n");
    let c = ctx(&tmpdir("orphan-race"), &cfg);
    ev(&c, 10, "queued", json!({}));
    append(&c.state, &Event::new(10, "sendmeter", Some(13), "queued", json!({}))).unwrap();
    let (wt, wt13) = (
        "/home/me/.herdr/worktrees/sendmeter/issue-12",
        "/home/me/.herdr/worktrees/sendmeter/issue-13",
    );
    let list = agents(&[
        ("sendmeter-impl-12", "working", wt, "w3:p1"),
        ("sendmeter-impl-13", "working", wt13, "w3:p2"),
    ]);
    let f = fake(NO_AGENTS)
        .reply("herdr --machine bazzite agent list", &list)
        .reply("herdr --machine bazzite pane get w3:p1", &harness("w3:p1"))
        .reply("herdr --machine bazzite agent rename w3:p1 sendmeter-impl-12", "{}")
        .reply("herdr --machine bazzite pane get w3:p2", &harness("w3:p2"))
        .reply("herdr --machine bazzite agent rename w3:p2 sendmeter-impl-13", "{}");
    let (code, out) = run(&c, &Racing(&f, &c, wt), &["drive", "--fast"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(kinds(&c, "spawn-started").len(), 1, "logged during the listing");
    assert!(
        !f.called("herdr --machine bazzite agent rename w3:p1"),
        "{:?}",
        f.calls.borrow()
    );
    let spawns = kinds(&c, "spawn");
    assert_eq!(
        spawns.iter().map(|e| (e.issue, e.str("why"))).collect::<Vec<_>>(),
        [(Some(13), Some("orphan"))],
        "{out}"
    );
    let adopted = kinds(&c, "spawn-adopted");
    assert_eq!(adopted.iter().map(|e| e.issue).collect::<Vec<_>>(), [Some(13)]);
}

#[test]
fn a_rev_lane_on_a_reused_pane_id_is_adopted() {
    let c = ctx(&tmpdir("orphan-812-reused"), CONFIG);
    let rev = review(&c, "");
    std::fs::remove_file(rev.join("verdict.md")).unwrap();
    let list = agents(&[("sendmeter-rev-12-r1", "working", &rev.to_string_lossy(), "w9:p1")]);
    let r = fake(&list)
        .reply("herdr pane get w9:p1", &harness("w9:p1"))
        .reply("herdr agent rename w9:p1 sendmeter-rev-12-r1", "{}")
        .reply("git -C", "");
    let out = fast(&c, &r);
    assert!(out.contains("adopted w9:p1 as sendmeter-rev-12-r1"), "{out}");
    let sp = kinds(&c, "spawn");
    assert_eq!(
        (sp.len(), sp[1].str("role"), sp[1].str("why")),
        (2, Some("rev"), Some("orphan"))
    );
}

#[test]
fn an_orphan_rev_lane_of_the_wrong_round_is_closed() {
    let c = ctx(&tmpdir("orphan-812-round"), CONFIG);
    review(&c, "");
    let rev = c.worktrees.join("sendmeter/rev-12-r3");
    std::fs::create_dir_all(&rev).unwrap();
    let list = agents(&[("sendmeter-rev-12-r3", "idle", &rev.to_string_lossy(), "w9:p2")]);
    let r = fake(&list).reply("herdr pane close w9:p2", "{}").reply("git -C", "");
    let out = fast(&c, &r);
    assert!(
        out.contains("closed stale orphan sendmeter-rev-12-r3 (round r3, r1 is due)"),
        "{out}"
    );
    assert_eq!(kinds(&c, "lane-closed")[0].str("why"), Some("stale-orphan"));
    assert!(kinds(&c, "spawn").len() == 1 && !r.called("herdr agent rename"));
}
