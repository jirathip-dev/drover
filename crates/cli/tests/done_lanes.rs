mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::json;

const NOW: u64 = 1_000_000;
const STALL: u64 = 30 * 60;
const SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C -oControlPersist=10m bazzite -- ";
const GIT: &str = "'git' '--no-replace-objects'";
const WT: &str = "/home/me/.local/share/drover/work/sendmeter/issue-12";
const IMPL: &str = "sendmeter-impl-12";

fn setup(name: &str) -> drover::Ctx {
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
    c
}

fn ev(c: &drover::Ctx, ts: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == kind)
        .cloned()
        .collect()
}

fn at(c: &drover::Ctx, now: u64) -> drover::Ctx {
    drover::Ctx { now, ..c.clone() }
}

fn two<'a>(f: &'a Fake, bazzite: &'a FakeShed) -> OverSsh<'a> {
    OverSsh {
        f,
        hosts: vec![("bazzite", bazzite)],
    }
}

fn host_fake(load: f64) -> Fake {
    Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n vm.loadavg", "{ 0.4 1 1 }")
        .reply("sysctl -n hw.ncpu", "4")
        .reply(
            &format!("{SSH}'cat' '/proc/loadavg'"),
            &format!("{} 1 1 1/9 42", load * 4.0),
        )
        .reply(&format!("{SSH}'nproc'"), "4")
        .reply(
            &format!("{SSH}'/opt/drover/bin/drover' 'profiles' 'sync' '--profile'"),
            "",
        )
        .reply("gh issue view 12", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
}

fn impl_lane(name: &str, t0: u64) -> (drover::Ctx, FakeShed, Fake) {
    let c = setup(name);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let d = json!({"role": "impl", "agent": IMPL, "harness": "claude", "pane": IMPL, "workspace": "",
                   "worktree": WT, "branch": "issue-12", "host": "bazzite", "machine": "bazzite",
                   "runtime": "shed", "confirmed": true});
    ev(&c, t0, "spawn", d);
    ev(
        &c,
        t0 + 60,
        "lane-state",
        json!({"agent": IMPL, "state": "done", "since": t0 + 60}),
    );
    let bazzite = FakeShed::new();
    bazzite.lane(IMPL, "done", WT);
    let f = host_fake(0.1)
        .reply(&format!("{SSH}'sh' '-c' 'test -e"), "")
        .reply(&format!("{SSH}{GIT} '-C'"), "")
        .reply(&format!("{SSH}'sh' '-c'"), "")
        .reply(&format!("{SSH}'rm' '-f'"), "");
    (c, bazzite, f)
}

fn drive(c: &drover::Ctx, r: &dyn drover::runner::Runner) -> String {
    let (code, out) = passes(c, r);
    assert_eq!(code, 0, "{out}");
    out
}

fn ended_once(c: &drover::Ctx) -> usize {
    kinds(c, "respawn").len() + kinds(c, "needs-human").len()
}

#[test]
fn a_remote_lane_done_in_shed_with_no_turn_end_is_respawned_once_not_left_idle() {
    let t0 = NOW - 2 * STALL;
    let (c, bazzite, f) = impl_lane("done-lane-quiet", t0);
    let _sh = FakeShed::attached(&c);
    let r = two(&f, &bazzite);
    let out = drive(&c, &r);
    assert_eq!(ended_once(&c), 1, "{out}");
    let rs = kinds(&c, "respawn");
    assert_eq!(rs[0].str("why"), Some("lane stalled"), "{out}");
    assert!(bazzite.called("run", IMPL), "respawned on its host: {out}");
    drive(&at(&c, NOW + 60), &r);
    assert_eq!(ended_once(&c), 1, "one respawn or one card, never a loop");
}

#[test]
fn a_remote_lane_done_in_shed_after_turn_working_is_ended_not_prompted_forever() {
    let t0 = NOW - 4 * STALL;
    let (c, bazzite, f) = impl_lane("done-lane-working", t0);
    let _sh = FakeShed::attached(&c);
    ev(
        &c,
        t0 + 60,
        "turn-end",
        json!({"role": "impl", "state": "working", "agent": IMPL}),
    );
    let r = two(&f, &bazzite);
    let first = drive(&at(&c, t0 + 60 + STALL + 1), &r);
    let prompts = |c: &drover::Ctx| kinds(c, "turn-prompted").len();
    assert_eq!(prompts(&c), 1, "{first}");
    let later = at(&c, t0 + 60 + 2 * STALL + 2);
    let out = drive(&later, &r);
    assert_eq!(ended_once(&c), 1, "{out}");
    let (_, w) = run(&later, &r, &["waiting", "sendmeter"]);
    assert!(!w.contains("prompted to continue"), "{w}");
    drive(&at(&c, later.now + 60), &r);
    assert_eq!((prompts(&c), ended_once(&c)), (1, 1), "never prompted again");
}

const REV: &str = "sendmeter-rev-12-r1";

fn head() -> String {
    "a".repeat(40)
}

fn rev_lane(name: &str) -> (drover::Ctx, String, FakeShed, Fake) {
    let c = setup(name);
    let rev = tmpdir(&format!("{name}-rev")).display().to_string();
    let d = json!({"role": "impl", "agent": IMPL, "harness": "claude", "pane": IMPL, "workspace": "",
                   "worktree": WT, "branch": "issue-12", "host": "bazzite", "machine": "bazzite"});
    ev(&c, NOW - 3 * STALL, "spawn", d);
    ev(&c, NOW - 3 * STALL + 60, "pushed", json!({"sha": head()}));
    let d = json!({"role": "rev", "agent": REV, "harness": "claude", "pane": REV, "workspace": "", "worktree": rev,
                   "branch": "rev-12-r1", "sha": head(), "host": "bazzite", "machine": "bazzite"});
    ev(&c, NOW - 2 * STALL, "spawn", d);
    std::fs::write(
        format!("{rev}/verdict.md"),
        format!("PASS\n\nReviewed head: {}\n", head()),
    )
    .unwrap();
    let bazzite = FakeShed::new();
    bazzite.lane(REV, "done", &rev);
    let pr = json!({"number": 34, "state": "OPEN", "baseRefName": "staging", "headRefOid": head(),
                    "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "IN_PROGRESS"}]});
    let f = host_fake(0.1)
        .reply("gh pr view issue-12", &pr.to_string())
        .reply(&format!("{SSH}{GIT} '-C' '{WT}' 'log'"), &format!("{} 20", head()))
        .reply(&format!("{SSH}'sh' '-c' 'test -e"), "")
        .reply(&format!("{SSH}{GIT} '-C' '{rev}' 'rev-list'"), "0\n")
        .reply(&format!("{SSH}{GIT} '-C' '{rev}' 'status'"), "")
        .reply(&format!("{SSH}{GIT} '-C' '{rev}' 'worktree' 'remove'"), "")
        .reply(&format!("{SSH}'rm'"), "");
    (c, rev, bazzite, f)
}

#[test]
fn a_done_remote_reviewers_verdict_md_is_recorded_in_one_pass_without_its_outbox_line() {
    let (c, _rev, bazzite, f) = rev_lane("done-rev-record");
    let f = f.reply(
        "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
        &format!("{}\n", head()),
    );
    let _sh = FakeShed::attached(&c);
    let out = drive(&c, &two(&f, &bazzite));
    let v = kinds(&c, "verdict");
    assert_eq!(v.len(), 1, "{out}");
    assert_eq!(
        (v[0].str("verdict"), v[0].str("sha")),
        (Some("PASS"), Some(head().as_str()))
    );
}

#[test]
fn a_done_remote_reviewers_verdict_that_fails_to_record_alerts_in_the_same_pass() {
    let (c, _rev, bazzite, f) = rev_lane("done-rev-alert");
    let f = f.reply("gh api repos/jirathip-dev/sendmeter/branches/issue-12", "");
    let _sh = FakeShed::attached(&c);
    let r = two(&f, &bazzite);
    let out = drive(&c, &r);
    assert!(kinds(&c, "verdict").is_empty(), "{out}");
    let failed = kinds(&c, "verdict-pickup-failed");
    assert_eq!(failed.len(), 1, "{out}");
    let (_, w) = run(&c, &r, &["waiting", "sendmeter"]);
    assert!(!w.contains("recording within 1 min"), "{w}");
}

#[test]
fn a_remote_spawn_makes_a_worktree_whose_branch_tracks_origin_issue_n() {
    let c = setup("done-upstream");
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]);
    let (_sh, bazzite) = (FakeShed::attached(&c), FakeShed::new());
    let f = host_fake(0.1)
        .reply(&format!("{SSH}'sh' '-c' 'git clone --single-branch"), "")
        .reply(&format!("{SSH}{GIT} '-C'"), "")
        .reply(&format!("{SSH}'sh' '-c' 'mkdir"), "")
        .reply(&format!("{SSH}'sh' '-c' 'umask 077"), "")
        .reply(&format!("{SSH}'rm' '-f'"), "");
    let (code, out) = run(
        &c,
        &two(&f, &bazzite),
        &["spawn", "sendmeter", "12", "impl", "--host", "bazzite"],
    );
    assert_eq!(code, 0, "{out}");
    let calls = f.calls.borrow();
    let set = |k: &str, v: &str| {
        let tail = format!("'config' 'branch.issue-12.{k}' '{v}'");
        calls.iter().rposition(|l| l.starts_with(SSH) && l.ends_with(&tail))
    };
    let add = calls
        .iter()
        .position(|l| l.starts_with(SSH) && l.contains("'worktree' 'add'"));
    let (add, remote, merge) = (
        add.expect("worktree add"),
        set("remote", "origin"),
        set("merge", "refs/heads/issue-12"),
    );
    assert!(
        remote.is_some_and(|i| i > add) && merge.is_some_and(|i| i > add),
        "{calls:?}"
    );
}
