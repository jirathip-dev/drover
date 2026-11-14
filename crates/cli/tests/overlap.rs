//! #129: auto-update after a merge, the pure-update gate, the `git merge-tree` preview, merge order, heads-up, cap-check.
//! Real temp git repos (origin, a seed clone that pushes, the fleet's `local` clone); everything else is the fake.
mod common;
use common::*;
use drover::config::Config;
use drover::events::{append, fold, read, Event};
use drover::runner::{Out, Real, Runner};
use serde_json::json;
use std::path::{Path, PathBuf};

struct Mixed(Fake);

impl Runner for Mixed {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        if prog == "git" {
            self.0.calls.borrow_mut().push(format!("git {}", args.join(" ")));
            return Real.run(prog, args);
        }
        self.0.run(prog, args)
    }
    fn sleep(&self, _: u64) {}
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn commit(seed: &Path, file: &str, text: &str, push: &str) -> String {
    std::fs::write(seed.join(file), text).unwrap();
    git(seed, &["add", "."]);
    git(seed, &["commit", "-qm", file]);
    git(seed, &["push", "-q", "origin", &format!("HEAD:{push}")]);
    git(seed, &["rev-parse", "HEAD"])
}

const A: &str = "1\n2\n3\n4\n5\n";

/// origin with `staging` and `issue-1` (a.txt line 1), `issue-2` (a.txt line 1 too: conflicts with 1),
/// `issue-3` (b.txt: clean); returns (dir, seed, ctx, fleet config).
fn repos(name: &str) -> (PathBuf, PathBuf, drover::Ctx, Config) {
    let d = tmpdir(name);
    let (origin, seed, local) = (d.join("origin.git"), d.join("seed"), d.join("local"));
    git(&d, &["init", "-q", "--bare", "-b", "staging", origin.to_str().unwrap()]);
    git(&d, &["clone", "-q", origin.to_str().unwrap(), seed.to_str().unwrap()]);
    git(&d, &["clone", "-q", origin.to_str().unwrap(), local.to_str().unwrap()]);
    for dir in [&seed, &local] {
        git(dir, &["config", "user.email", "t@example.com"]);
        git(dir, &["config", "user.name", "t"]);
    }
    git(&seed, &["checkout", "-qb", "staging"]);
    commit(&seed, "a.txt", A, "staging");
    for (n, file, text) in [
        (1, "a.txt", "one\n2\n3\n4\n5\n"),
        (2, "a.txt", "uno\n2\n3\n4\n5\n"),
        (3, "b.txt", "b\n"),
    ] {
        git(&seed, &["checkout", "-q", "-B", &format!("issue-{n}"), "staging"]);
        commit(&seed, file, text, &format!("issue-{n}"));
    }
    git(&seed, &["checkout", "-q", "staging"]);
    let config = CONFIG.replace("/tmp/does-not-matter/sendmeter", local.to_str().unwrap());
    let c = ctx(&d, &config);
    (d, seed, c, Config::parse(&config).unwrap())
}

fn head(seed: &Path, branch: &str) -> String {
    git(seed, &["fetch", "-q", "origin"]);
    git(seed, &["rev-parse", &format!("origin/{branch}")])
}

/// An impl lane for `n`, a PASS at its pushed head when `pass`, and its open PR #(10 + n) in the PR cache.
fn lane(c: &drover::Ctx, seed: &Path, n: u64, pass: bool) {
    let spawn = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "branch": format!("issue-{n}"), "pane": format!("sendmeter-impl-{n}")});
    append(&c.state, &Event::new(10, "sendmeter", Some(n), "spawn", spawn)).unwrap();
    if pass {
        let v = json!({"verdict": "PASS", "sha": head(seed, &format!("issue-{n}"))});
        append(&c.state, &Event::new(20, "sendmeter", Some(n), "verdict", v)).unwrap();
    }
    let cache = c.state.with_file_name("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let pr = json!({"ts": 0, "branch": format!("issue-{n}"), "pr": {"number": 10 + n, "state": "OPEN"}});
    std::fs::write(cache.join(format!("pr-sendmeter-{n}.json")), pr.to_string()).unwrap();
}

/// #285: integration's protection read (cached) as strict, so a behind ready PR is caught up.
fn strict(c: &drover::Ctx) {
    let p = c.state.with_file_name("cache").join("strict-sendmeter-0.json");
    std::fs::write(p, json!({"ts": c.now, "strict": true}).to_string()).unwrap();
}

fn green_pr(n: u64, head: &str) -> String {
    let ok = json!([{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}]);
    json!({"number": 10 + n, "state": "OPEN", "baseRefName": "staging", "headRefOid": head, "statusCheckRollup": ok})
        .to_string()
}

fn merger(n: u64, head: &str) -> Mixed {
    Mixed(
        Fake::default()
            .reply(&format!("gh pr view issue-{n} "), &green_pr(n, head))
            .reply("gh pr merge", "")
            .reply("gh pr view 1", &json!({"mergeCommit": {"oid": "m3rg3"}}).to_string())
            .reply("gh issue", ""),
    )
}

#[test]
fn clean_auto_update_keeps_the_pass_and_spawns_no_lane() {
    let (_d, seed, c, cfg) = repos("au-clean");
    let sh = FakeShed::attached(&c);
    lane(&c, &seed, 3, true);
    strict(&c);
    let from = head(&seed, "issue-3");
    commit(&seed, "c.txt", "c\n", "staging");
    let r = Mixed(Fake::default());
    let mut out = vec![];
    drover::drive::auto_update(&c, &cfg, &sh.on(&r), &mut out, "sendmeter").unwrap();
    let to = head(&seed, "issue-3");
    let ev = read(&c.state).unwrap().to_vec();
    let up = ev.iter().find(|e| e.kind == "auto-updated").expect("auto-updated");
    assert_eq!((up.str("from"), up.str("to")), (Some(from.as_str()), Some(to.as_str())));
    assert!(ev.iter().all(|e| e.kind != "spawn" || e.ts == 10), "no lane: {ev:?}");
    assert!(
        r.0.calls
            .borrow()
            .iter()
            .all(|l| l.starts_with("git ") || l.starts_with("herdr agent list")),
        "{:?}",
        r.0.calls.borrow()
    );
    assert!(
        sh.methods().iter().all(|m| m == "ls"),
        "no lane touched: {:?}",
        sh.methods()
    );
    // The gate takes the PASS of the head it updated: merged, no review round.
    let r = merger(3, &to);
    let (code, out) = run(&c, &r, &["merge", "sendmeter", "3"]);
    assert_eq!(code, 0, "{out}");
    assert!(r.0.called(&format!(
        "gh pr merge 13 -R jirathip-dev/sendmeter --merge --match-head-commit {to}"
    )));
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 3)];
    assert_eq!((st.state.as_str(), st.rounds), ("merged", 1));
}

/// Merge staging into origin's `issue-3` in a fresh clone, `extra` folded into b.txt (None: a pure merge); pushed.
fn merge_staging(d: &Path, extra: Option<&str>) {
    let w = d.join("w");
    git(
        d,
        &[
            "clone",
            "-q",
            "-b",
            "issue-3",
            d.join("origin.git").to_str().unwrap(),
            w.to_str().unwrap(),
        ],
    );
    git(
        &w,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "merge",
            "-q",
            "--no-commit",
            "origin/staging",
        ],
    );
    if let Some(text) = extra {
        std::fs::write(w.join("b.txt"), text).unwrap();
    }
    git(&w, &["add", "."]);
    git(
        &w,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "commit",
            "-qm",
            "merge",
        ],
    );
    git(&w, &["push", "-q", "origin", "HEAD:issue-3"]);
}

#[test]
fn gate_refuses_a_merge_head_that_adds_its_own_change() {
    let (d, seed, c, _) = repos("au-impure");
    let sh = FakeShed::attached(&c);
    lane(&c, &seed, 3, true);
    commit(&seed, "c.txt", "c\n", "staging");
    // A merge of integration with an extra edit folded in: not a pure update.
    merge_staging(&d, Some("sneaky\n"));
    let h = head(&seed, "issue-3");
    let r = merger(3, &h);
    let (code, out) = run(&c, &r, &["merge", "sendmeter", "3"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED STALE_VERDICT"), "{out}");
    assert!(!r.0.called("gh pr merge"));
    // #165: the next drive pass starts a review of the new head, not a dead block.
    let r = Mixed(
        Fake::default()
            .reply("gh pr view issue-3 ", &green_pr(3, &h))
            .reply("gh issue view 3", r#"{"title":"t","body":"b"}"#)
            .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
            .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
            .reply("sysctl -n hw.ncpu", "10\n")
            .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
            .reply("which shed", "/opt/shed/bin/shed\n"),
    );
    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("sendmeter #3 review"), "{out}");
    let calls = r.0.calls.borrow();
    assert!(
        calls
            .iter()
            .any(|l| l.contains(" worktree add -b rev-3-r2 ") && l.ends_with(" origin/issue-3")),
        "{calls:?}"
    );
    assert_eq!(sh.last("run")["name"], "sendmeter-rev-3-r2", "{:?}", sh.methods());
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 3)];
    assert_eq!(st.state, "review(2)");
    assert!(!st.reviewed(&h), "B has no verdict yet");
}

#[test]
fn an_auto_updated_head_waits_for_ci_then_merges_on_a_later_pass() {
    let (_d, seed, mut c, _) = repos("au-ci-pending");
    let _sh = FakeShed::attached(&c);
    lane(&c, &seed, 1, true);
    lane(&c, &seed, 3, true);
    strict(&c);
    let (h1, from) = (head(&seed, "issue-1"), head(&seed, "issue-3"));
    commit(&seed, "c.txt", "c\n", "staging"); // #1's merge lands on staging
    let st = |c: &drover::Ctx| {
        fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 3)]
            .state
            .clone()
    };
    let pass = |c: &drover::Ctx, pr3: &str| {
        let r = Mixed(
            Fake::default()
                .reply("gh pr view issue-1 ", &green_pr(1, &h1))
                .reply("gh pr view issue-3 ", pr3)
                .reply("gh pr merge", "")
                .reply("gh pr view 1", &json!({"mergeCommit": {"oid": "m3rg3"}}).to_string())
                .reply("gh issue", ""),
        );
        let (code, out) = run(c, &r, &["drive"]);
        assert_eq!(code, 0, "{out}");
        (r, out)
    };
    // #1 merges and #3 is auto-updated; #3's merge waits for the CI of its new head.
    let (r, out) = pass(&c, &green_pr(3, &from));
    let to = head(&seed, "issue-3");
    assert!(
        out.contains(&format!("sendmeter #3 ready (CI pending at {to:.7})")),
        "{out}"
    );
    assert_eq!(r.0.count("gh pr merge"), 1, "{:?}", r.0.calls.borrow());
    assert_eq!(st(&c), "ready");
    // No checks reported at the new head: still ready, a manual merge refused but not blocked.
    c.now += 120;
    let none =
        json!({"number": 13, "state": "OPEN", "baseRefName": "staging", "headRefOid": to, "statusCheckRollup": []});
    let (r, _) = pass(&c, &none.to_string());
    assert!(!r.0.called("gh pr merge"));
    let (code, out) = run(&c, &r, &["merge", "sendmeter", "3"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED CI_NOT_GREEN"), "{out}");
    assert_eq!(st(&c), "ready");
    // Green on the next pass: merged.
    c.now += 120;
    let (r, out) = pass(&c, &green_pr(3, &to));
    let merge = format!("gh pr merge 13 -R jirathip-dev/sendmeter --merge --match-head-commit {to}");
    assert!(r.0.called(&merge), "{out}");
    assert_eq!(st(&c), "merged");
}

#[test]
fn conflicting_auto_update_spawns_an_update_lane() {
    let (_d, seed, c, cfg) = repos("au-conflict");
    lane(&c, &seed, 1, true);
    let before = head(&seed, "issue-1");
    // #2's change lands on staging: #1 now conflicts on a.txt.
    git(&seed, &["merge", "-q", "--ff-only", "origin/issue-2"]);
    git(&seed, &["push", "-q", "origin", "HEAD:staging"]);
    let r = Mixed(Fake::default());
    drover::drive::auto_update(&c, &cfg, &r, &mut vec![], "sendmeter").unwrap();
    assert_eq!(head(&seed, "issue-1"), before, "nothing pushed");
    let ev = read(&c.state).unwrap();
    assert!(ev.iter().all(|e| e.kind != "auto-updated"));
    let d = ev.iter().find(|e| e.kind == "driven").expect("driven");
    assert_eq!(d.str("cmd"), Some("drover spawn sendmeter 1 impl --supersede --update"));
}

#[test]
fn merge_tree_finds_the_pair_and_its_files_once() {
    let (_d, seed, c, cfg) = repos("mt-pair");
    for n in 1..=3 {
        lane(&c, &seed, n, false);
    }
    let r = Mixed(Fake::default());
    let got = drover::overlap::preview(&c, &cfg, &r, "sendmeter", &read(&c.state).unwrap()).unwrap();
    assert_eq!(got[&1], vec![(2, vec!["a.txt".to_string()])]);
    assert_eq!(got[&2], vec![(1, vec!["a.txt".to_string()])]);
    assert!(!got.contains_key(&3), "{got:?}");
    let ev = read(&c.state).unwrap();
    let p: Vec<_> = ev.iter().filter(|e| e.kind == "conflict-predicted").collect();
    assert_eq!((p.len(), p[0].issue, &p[0].data["other"]), (1, Some(2), &json!(1)));
    let card = "may conflict with #1; keep going, drover updates you after it merges";
    assert_eq!(p[0].str("reason"), Some(card), "#320");
    // Same heads: cached, no merge-tree run and no second event.
    let trees = r.0.count("git -C");
    let r2 = Mixed(Fake::default());
    let again = drover::overlap::preview(&c, &cfg, &r2, "sendmeter", &read(&c.state).unwrap()).unwrap();
    assert_eq!(again, got);
    assert!(trees > 0 && !r2.0.calls.borrow().iter().any(|l| l.contains("merge-tree")));
    assert_eq!(
        read(&c.state)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "conflict-predicted")
            .count(),
        1
    );
    assert_eq!(drover::overlap::cached(&c, "sendmeter"), got);
    let (_, st) = run(&c, &r2, &["status", "sendmeter", "--json"]);
    let rows: serde_json::Value = serde_json::from_str(&st).unwrap();
    assert_eq!(rows[0]["conflicts"], json!([[2, ["a.txt"]]]));
    let (_, board) = run(&c, &r2, &["board", "sendmeter"]);
    assert!(board.contains("⚠ conflicts with #2 (a.txt)"), "{board}");
}

#[test]
fn drive_merges_the_ready_pr_that_conflicts_with_nothing_first() {
    let (_d, seed, c, _) = repos("order");
    let _sh = FakeShed::attached(&c);
    for n in 1..=3 {
        lane(&c, &seed, n, true);
    }
    let mut f = Fake::default()
        .reply("gh pr merge", "")
        .reply("gh pr view 1", &json!({"mergeCommit": {"oid": "m3rg3"}}).to_string())
        .reply("gh issue", "");
    for n in 1..=3 {
        f = f.reply(
            &format!("gh pr view issue-{n} "),
            &green_pr(n, &head(&seed, &format!("issue-{n}"))),
        );
    }
    let r = Mixed(f);
    let (code, out) = run(&c, &r, &["drive", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    let at = |n: u64| r.0.index(&format!("gh pr merge {}", 10 + n)).unwrap_or(usize::MAX);
    assert!(at(3) < at(1), "#3 first: {:?}", r.0.calls.borrow());
    assert!(at(1) < usize::MAX, "{out}");
}

#[test]
fn heads_up_reaches_a_working_lane_once() {
    let (_d, seed, c, cfg) = repos("heads-up");
    let sh = FakeShed::attached(&c);
    lane(&c, &seed, 1, false);
    lane(&c, &seed, 2, true);
    sh.lane("sendmeter-impl-1", "working", "/w/issue-1");
    sh.lane("sendmeter-impl-2", "idle", "/w/issue-2");
    let r = Mixed(Fake::default());
    for _ in 0..2 {
        drover::overlap::preview(&c, &cfg, &sh.on(&r), "sendmeter", &read(&c.state).unwrap()).unwrap();
    }
    let want = "HEADS-UP: #2 is ready to merge and touches a.txt: your branch may conflict \
                with #2; keep going, drover updates you after it merges.";
    assert_eq!(sh.sent("sendmeter-impl-1"), [want], "{:?}", sh.methods());
    assert!(sh.sent("sendmeter-impl-2").is_empty(), "a ready PR's lane is done");
    assert_eq!(
        read(&c.state).unwrap().iter().filter(|e| e.kind == "heads-up").count(),
        1
    );
}

#[test]
fn cap_check_reads_the_limit_from_config() {
    let d = tmpdir("cap");
    let (low, high) = (d.join("low.toml"), d.join("high.toml"));
    std::fs::write(&low, "[fleet.x]\nline_cap = 10\n").unwrap();
    std::fs::write(&high, "[fleet.y]\nline_cap = 100000\n").unwrap();
    let c = ctx(&d, CONFIG);
    let r = Fake::default();
    assert_eq!(run(&c, &r, &["cap-check", "--config", low.to_str().unwrap()]).0, 1);
    let (code, out) = run(&c, &r, &["cap-check", "--config", high.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("/ cap 100000"), "{out}");
    let ci =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/ci.yml")).unwrap();
    let step = ci
        .lines()
        .skip_while(|l| !l.contains("line cap"))
        .take(2)
        .collect::<String>();
    assert!(step.contains("drover cap-check --config ci/cap.toml"), "{step}");
    assert!(
        !step.chars().any(|c| c.is_ascii_digit()),
        "ci.yml holds no cap number: {step}"
    );
}

/// #207: `n` in review, its reviewer spawned at `sha` and done (`PASS` in verdict.md, not recorded yet).
fn reviewed(c: &drover::Ctx, n: u64, sha: &str) -> String {
    reviewed_with(c, n, sha, "PASS\nall good\n")
}

/// #293: the same with verdict.md's `body` (a `Reviewed head:` line names the sha the reviewer checked out).
fn reviewed_with(c: &drover::Ctx, n: u64, sha: &str, body: &str) -> String {
    let wt = c.worktrees.join(format!("rev-{n}"));
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(wt.join("verdict.md"), body).unwrap();
    let d = json!({"role": "rev", "agent": format!("sendmeter-rev-{n}-r1"), "pane": format!("sendmeter-rev-{n}-r1"), "worktree": wt, "sha": sha});
    append(&c.state, &Event::new(30, "sendmeter", Some(n), "spawn", d)).unwrap();
    wt.join("verdict.md").display().to_string()
}

fn pending_pr(n: u64, head: &str) -> String {
    green_pr(n, head)
        .replace("COMPLETED", "IN_PROGRESS")
        .replace(r#","conclusion":"SUCCESS""#, "")
}

#[test]
fn a_written_verdict_is_recorded_before_the_update_in_the_same_pass() {
    let (_d, seed, c, _) = repos("au-order");
    let _sh = FakeShed::attached(&c);
    lane(&c, &seed, 1, true);
    lane(&c, &seed, 3, false);
    strict(&c);
    let (h1, from) = (head(&seed, "issue-1"), head(&seed, "issue-3"));
    reviewed(&c, 3, &from);
    commit(&seed, "c.txt", "c\n", "staging"); // #1's merge lands on staging
    let r = Mixed(
        Fake::default()
            .reply("gh pr view issue-1 ", &green_pr(1, &h1))
            .reply("gh pr view issue-3 ", &pending_pr(3, &from))
            .reply("gh api repos/jirathip-dev/sendmeter/branches/issue-3", &from)
            .reply("gh pr merge", "")
            .reply("gh pr view 1", &json!({"mergeCommit": {"oid": "m3rg3"}}).to_string())
            .reply("gh issue", ""),
    );
    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    let ev = read(&c.state).unwrap();
    let at = |k: &str| {
        ev.iter()
            .position(|e| e.is(k, "sendmeter", 3))
            .unwrap_or_else(|| panic!("{k}: {out}"))
    };
    assert!(at("verdict") < at("auto-updated"), "{out}");
    assert_eq!(ev[at("verdict")].str("sha"), Some(from.as_str()));
    assert_ne!(head(&seed, "issue-3"), from, "updated");
}

#[test]
fn a_branch_in_review_is_not_updated() {
    let (_d, seed, c, cfg) = repos("au-review");
    lane(&c, &seed, 3, false);
    let from = head(&seed, "issue-3");
    reviewed(&c, 3, &from);
    commit(&seed, "c.txt", "c\n", "staging");
    // A reviewer still working, then one done whose verdict is not recorded yet.
    let sh = FakeShed::attached(&c);
    sh.lane("sendmeter-rev-3-r1", "working", "/wt");
    for live in [true, false] {
        if !live {
            sh.remove("sendmeter-rev-3-r1");
        }
        let r = Mixed(Fake::default());
        drover::drive::auto_update(&c, &cfg, &sh.on(&r), &mut vec![], "sendmeter").unwrap();
        assert_eq!(head(&seed, "issue-3"), from, "nothing pushed");
    }
    assert!(read(&c.state).unwrap().iter().all(|e| e.kind != "auto-updated"));
}

#[test]
fn a_verdict_at_the_head_a_pure_merge_moved_from_records_and_merges() {
    let (d, seed, c, _) = repos("au-carry");
    let _sh = FakeShed::attached(&c);
    lane(&c, &seed, 3, false);
    let x = head(&seed, "issue-3");
    let file = reviewed(&c, 3, &x);
    commit(&seed, "c.txt", "c\n", "staging");
    merge_staging(&d, None);
    let m = head(&seed, "issue-3");
    // Drive records it at the reviewed sha (no auto-updated event: the merge was the lane's own).
    let r = Mixed(
        Fake::default()
            .reply("gh pr view issue-3 ", &green_pr(3, &m))
            .reply("gh api repos/jirathip-dev/sendmeter/branches/issue-3", &m),
    );
    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 3)];
    let last = st.verdicts.last().map(|v| v.0.as_str());
    assert_eq!((st.state.as_str(), last), ("ready", Some(x.as_str())), "{out}");
    // The CLI takes it too, and the merge carries the PASS to the head: no re-review.
    let (code, out) = run(
        &c,
        &r,
        &["verdict", "sendmeter", "3", "PASS", "--sha", &x, "--file", &file],
    );
    assert_eq!(code, 0, "{out}");
    let r = merger(3, &m);
    let (code, out) = run(&c, &r, &["merge", "sendmeter", "3"]);
    assert_eq!(code, 0, "{out}");
    assert!(r.0.called(&format!(
        "gh pr merge 13 -R jirathip-dev/sendmeter --merge --match-head-commit {m}"
    )));
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 3)];
    assert_eq!((st.state.as_str(), st.rounds), ("merged", 1));
}

#[test]
fn a_verdict_at_a_head_an_impure_merge_moved_from_still_refuses() {
    let (d, seed, c, _) = repos("au-no-carry");
    let _sh = FakeShed::attached(&c);
    lane(&c, &seed, 3, false);
    let x = head(&seed, "issue-3");
    let file = reviewed(&c, 3, &x);
    commit(&seed, "c.txt", "c\n", "staging");
    merge_staging(&d, Some("sneaky\n"));
    let m = head(&seed, "issue-3");
    let r = Mixed(
        Fake::default()
            .reply("gh pr view issue-3 ", &pending_pr(3, &m))
            .reply("gh api repos/jirathip-dev/sendmeter/branches/issue-3", &m),
    );
    let (code, out) = run(
        &c,
        &r,
        &["verdict", "sendmeter", "3", "PASS", "--sha", &x, "--file", &file],
    );
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED STALE_SHA"), "{out}");
    let (_, out) = run(&c, &r, &["waiting", "sendmeter"]);
    let want = format!("review was for {x:.7}, head moved to {m:.7} (not a clean merge): review again");
    assert!(out.contains(&want), "{out}");
}

/// #285: the fleet config with `merge_label = "merge-ready"` for the `repos` in `d`.
fn labelled(d: &Path) -> Config {
    let config = CONFIG.replace("/tmp/does-not-matter/sendmeter", d.join("local").to_str().unwrap());
    Config::parse(&config.replace(
        "integration = \"staging\"",
        "integration = \"staging\"\nmerge_label = \"merge-ready\"",
    ))
    .unwrap()
}

fn pr3(head: &str, labels: &[&str]) -> serde_json::Value {
    let labels: Vec<_> = labels.iter().map(|l| json!({"name": l})).collect();
    json!({"number": 13, "state": "OPEN", "headRefOid": head, "labels": labels})
}

const PROTECTION: &str = "gh api repos/jirathip-dev/sendmeter/branches/staging/protection";
const RULES: &str = "gh api repos/jirathip-dev/sendmeter/rules/branches/staging";

#[test]
fn behind_under_a_non_strict_rule_is_not_updated_and_merges_as_it_is() {
    let (_d, seed, mut c, cfg) = repos("au-non-strict");
    let sh = FakeShed::attached(&c);
    lane(&c, &seed, 3, true);
    let from = head(&seed, "issue-3");
    commit(&seed, "c.txt", "c\n", "staging");
    let r = Mixed(merger(3, &from).0.reply(PROTECTION, "false\n"));
    drover::drive::auto_update(&c, &cfg, &sh.on(&r), &mut vec![], "sendmeter").unwrap();
    assert_eq!(head(&seed, "issue-3"), from, "nothing pushed");
    let ev = read(&c.state).unwrap().to_vec();
    assert!(
        ev.iter().all(|e| e.kind != "auto-updated" && e.kind != "driven"),
        "{ev:?}"
    );
    // The status says so, never a silent catch-up loop.
    let behind = json!({"number": 13, "state": "OPEN", "baseRefName": "staging", "headRefOid": from,
        "mergeable": "MERGEABLE", "mergeStateStatus": "BEHIND", "statusCheckRollup": []});
    let w = Mixed(Fake::default().reply("gh pr view issue-3 ", &behind.to_string()));
    let ws = drover::waiting::scan(&c, &cfg, &sh.on(&w), &read(&c.state).unwrap(), &[], true, None);
    let what = &ws.iter().find(|(s, _)| s.issue == 3).expect("#3").1.what;
    assert!(
        what.starts_with("PASS · behind staging (not required) · merging when CI green"),
        "{what}"
    );
    // Green while still behind: merged as it is.
    c.now += 120;
    let mut green: serde_json::Value = serde_json::from_str(&green_pr(3, &from)).unwrap();
    (green["mergeable"], green["mergeStateStatus"]) = (json!("MERGEABLE"), json!("BEHIND"));
    let r = Mixed(
        Fake::default()
            .reply("gh pr view issue-3 ", &green.to_string())
            .reply("gh pr view 1", &json!({"mergeCommit": {"oid": "m3rg3"}}).to_string()),
    );
    let r = Mixed(r.0.reply("gh pr ", "").reply("gh issue", ""));
    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    let merge = format!("gh pr merge 13 -R jirathip-dev/sendmeter --merge --match-head-commit {from}");
    assert!(r.0.called(&merge), "{out}");
    assert!(read(&c.state).unwrap().iter().all(|e| e.kind != "auto-updated"));
}

#[test]
fn strict_rule_auto_updates_and_puts_the_merge_label_back() {
    let (d, seed, c, _) = repos("au-strict");
    let sh = FakeShed::attached(&c);
    let cfg = labelled(&d);
    lane(&c, &seed, 3, true);
    commit(&seed, "c.txt", "c\n", "staging");
    let r = Mixed(Fake::default().reply(RULES, "true\n").reply("gh pr edit", ""));
    let mut out = vec![];
    drover::drive::auto_update(&c, &cfg, &sh.on(&r), &mut out, "sendmeter").unwrap();
    let out = String::from_utf8_lossy(&out);
    let to = head(&seed, "issue-3");
    assert!(out.contains("auto-updated"), "{out}");
    assert!(
        r.0.called("gh pr edit 13 -R jirathip-dev/sendmeter --add-label merge-ready"),
        "{out}"
    );
    let ev = read(&c.state).unwrap().to_vec();
    assert!(
        ev.iter()
            .any(|e| e.kind == "labelled" && e.str("sha") == Some(to.as_str())),
        "{ev:?}"
    );
    assert!(ev.iter().all(|e| e.kind != "unlabelled"));
    // The ruleset decides (synergy has no classic protection), read once.
    assert_eq!((r.0.count(RULES), r.0.count(PROTECTION)), (1, 0));
}

#[test]
fn a_pure_merge_head_keeps_the_label_and_a_new_change_loses_it() {
    let (d, seed, c, _) = repos("au-label");
    let cfg = labelled(&d);
    lane(&c, &seed, 3, true);
    commit(&seed, "c.txt", "c\n", "staging");
    merge_staging(&d, None);
    let m = head(&seed, "issue-3");
    let st = || fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 3)].clone();
    let r = Mixed(Fake::default().reply("gh pr edit", ""));
    drover::gate::sync_label(&c, &cfg, &r, &mut vec![], &st(), &pr3(&m, &["merge-ready"])).unwrap();
    assert!(!r.0.called("gh pr edit"), "kept: {:?}", r.0.calls.borrow());
    drover::gate::sync_label(&c, &cfg, &r, &mut vec![], &st(), &pr3(&m, &[])).unwrap();
    assert!(r
        .0
        .called("gh pr edit 13 -R jirathip-dev/sendmeter --add-label merge-ready"));
    // A merge that folds in its own change is new work: the label comes off.
    commit(&seed, "d.txt", "d\n", "staging");
    std::fs::remove_dir_all(d.join("w")).unwrap();
    merge_staging(&d, Some("sneaky\n"));
    let h = head(&seed, "issue-3");
    drover::gate::sync_label(&c, &cfg, &r, &mut vec![], &st(), &pr3(&h, &["merge-ready"])).unwrap();
    assert!(r
        .0
        .called("gh pr edit 13 -R jirathip-dev/sendmeter --remove-label merge-ready"));
    assert!(read(&c.state).unwrap().iter().any(|e| e.kind == "unlabelled"));
}

/// #293: issue-3's head moved past the rev spawn head by a commit of its own; returns (spawn head, new head).
fn moved(seed: &Path) -> (String, String) {
    let a = head(seed, "issue-3");
    git(seed, &["checkout", "-q", "issue-3"]);
    let b = commit(seed, "d.txt", "d\n", "issue-3");
    git(seed, &["checkout", "-q", "staging"]);
    (a, b)
}

#[test]
fn a_verdict_naming_the_current_head_records_there_whatever_the_spawn_head() {
    let (_d, seed, c, _) = repos("named-head");
    let _sh = FakeShed::attached(&c);
    let cfg = std::fs::read_to_string(&c.config).unwrap();
    let label = "integration = \"staging\"\nmerge_label = \"merge-ready\"";
    let cfg = cfg.replacen("integration = \"staging\"", label, 1);
    std::fs::write(&c.config, cfg).unwrap();
    lane(&c, &seed, 3, false);
    let (a, b) = moved(&seed);
    reviewed_with(&c, 3, &a, &format!("PASS\nReviewed head: {b} (PR #13)\nall good\n"));
    let r = Mixed(
        Fake::default()
            .reply("gh pr view issue-3 ", &green_pr(3, &b))
            .reply("gh api repos/jirathip-dev/sendmeter/branches/issue-3", &b)
            .reply("gh pr edit", ""),
    );
    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    let ev = read(&c.state).unwrap();
    let v = ev
        .iter()
        .rfind(|e| e.is("verdict", "sendmeter", 3))
        .unwrap_or_else(|| panic!("{out}"));
    assert_eq!((v.str("sha"), v.str("spawn_sha")), (Some(b.as_str()), Some(a.as_str())));
    assert!(
        r.0.called("gh pr edit 13 -R jirathip-dev/sendmeter --add-label merge-ready"),
        "{out}"
    );
}

#[test]
fn a_verdict_naming_a_head_a_commit_moved_from_is_stale_and_none_means_the_spawn_head() {
    for (i, body) in ["PASS\nReviewed head: {a}\n", "PASS\nall good\n"]
        .into_iter()
        .enumerate()
    {
        let (_d, seed, c, _) = repos(&format!("named-stale-{i}"));
        let _sh = FakeShed::attached(&c);
        lane(&c, &seed, 3, false);
        let (a, b) = moved(&seed);
        reviewed_with(&c, 3, &a, &body.replace("{a}", &a));
        let r = Mixed(
            Fake::default()
                .reply("gh pr view issue-3 ", &pending_pr(3, &b))
                .reply("gh api repos/jirathip-dev/sendmeter/branches/issue-3", &b),
        );
        let (_, out) = run(&c, &r, &["waiting", "sendmeter"]);
        let want = format!("review was for {a:.7}, head moved to {b:.7} (not a clean merge): review again");
        assert!(out.contains(&want), "{out}");
        assert!(read(&c.state).unwrap().iter().all(|e| e.kind != "verdict"));
    }
}

#[test]
fn a_verdict_naming_the_head_a_pure_merge_moved_from_is_carried() {
    let (d, seed, c, _) = repos("named-carry");
    let _sh = FakeShed::attached(&c);
    lane(&c, &seed, 3, false);
    let x = head(&seed, "issue-3");
    reviewed_with(&c, 3, &x, &format!("PASS\nReviewed head: {x}\n"));
    commit(&seed, "c.txt", "c\n", "staging");
    merge_staging(&d, None);
    let m = head(&seed, "issue-3");
    let r = Mixed(
        Fake::default()
            .reply("gh pr view issue-3 ", &green_pr(3, &m))
            .reply("gh api repos/jirathip-dev/sendmeter/branches/issue-3", &m),
    );
    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 3)];
    let last = st.verdicts.last().map(|v| v.0.as_str());
    assert_eq!((st.state.as_str(), last), ("ready", Some(x.as_str())), "{out}");
}

/// #429: under `warn_within` (default 500) lines of headroom cap-check warns and passes; over the cap it fails.
#[test]
fn cap_check_warns_within_reach_of_the_cap() {
    let d = tmpdir("cap-warn");
    let (c, r) = (ctx(&d, CONFIG), Fake::default());
    let cap = |toml: &str| {
        let p = d.join("cap.toml");
        std::fs::write(&p, toml).unwrap();
        run(&c, &r, &["cap-check", "--config", p.to_str().unwrap()])
    };
    let (_, out) = cap("[fleet.x]\nline_cap = 100000000\n");
    let n: i64 = out
        .split("formatted): ")
        .nth(1)
        .unwrap()
        .split(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    // As 24,100 and 24,500 lines against a cap of 24,500, then 24,600.
    for (left, code, warn) in [(400, 0, true), (0, 0, true), (-100, 1, false)] {
        let (got, out) = cap(&format!("[fleet.x]\nline_cap = {}\nwarn_within = 500\n", n + left));
        assert_eq!(got, code, "{out}");
        let line = format!("WARN: {left} lines of headroom (< 500)");
        assert_eq!(out.contains(&line), warn, "{out}");
    }
    let (got, out) = cap(&format!("[fleet.x]\nline_cap = {}\n", n + 499));
    assert!(
        got == 0 && out.contains("WARN: 499 lines of headroom (< 500)"),
        "default 500: {out}"
    );
    let (got, out) = cap(&format!("[fleet.x]\nline_cap = {}\n", n + 500));
    assert!(got == 0 && !out.contains("WARN"), "{out}");
    let (got, out) = cap(&format!("[fleet.x]\nline_cap = {}\nwarn_within = 10\n", n + 100));
    assert!(got == 0 && !out.contains("WARN"), "warn_within read: {out}");
}

/// #429: `check` measures integration's head by git and emits `cap-near` once per integration sha, only under
/// `warn_within`.
#[test]
fn check_emits_cap_near_once_per_integration_sha() {
    let (_d, seed, c, cfg) = repos("cap-near");
    let sh = FakeShed::attached(&c);
    let put = |files: &[(&str, &str)]| {
        for (f, t) in files {
            let p = seed.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, t).unwrap();
        }
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-qm", "cap"]);
        git(&seed, &["push", "-q", "origin", "HEAD:staging"]);
        git(&seed, &["rev-parse", "HEAD"])
    };
    let first = put(&[
        ("ci/cap.toml", "[fleet.sendmeter]\nline_cap = 504\n"),
        ("src/a.rs", "1\n2\n\n"),
        ("crates/x/src/m/b.rs", "1\n2"),
        ("crates/x/tests/t.rs", "not\nsrc\n"),
    ]);
    let r = Mixed(Fake::default());
    let near = || {
        let ev = read(&c.state).unwrap().to_vec();
        ev.into_iter().filter(|e| e.kind == "cap-near").collect::<Vec<_>>()
    };
    for _ in 0..2 {
        drover::check::check(&c, &cfg, &sh.on(&r), &mut vec![]).unwrap();
    }
    let got = near();
    assert_eq!(got.len(), 1, "once per sha, not per pass: {got:?}");
    assert_eq!(got[0].str("key"), Some(first.as_str()));
    let why = got[0].str("reason").unwrap();
    assert!(
        why.starts_with("499 lines of headroom (< 500)") && why.contains("5 / cap 504"),
        "{why}"
    );
    let page = || {
        let f = Fake::default();
        let s = drover::serve::Server::new(&c, &f, Some("127.0.0.1:7777".into()));
        queue_tab(&s, "127.0.0.1:7777", c.now) // #268: the board's Queue tab
    };
    assert!(
        page().contains("<p class=r>cap-near: 499 lines of headroom on staging</p>"),
        "board banner"
    );
    let second = put(&[("c.txt", "c\n")]);
    drover::check::check(&c, &cfg, &r, &mut vec![]).unwrap();
    let got = near();
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(got[1].str("key"), Some(second.as_str()));
    put(&[("ci/cap.toml", "[fleet.sendmeter]\nline_cap = 505\n")]);
    drover::check::check(&c, &cfg, &r, &mut vec![]).unwrap();
    assert_eq!(near().len(), 2, "500 lines of headroom is not near");
    assert!(!page().contains("cap-near"), "no banner after the raise");
}

/// #270: cap-check reports the web/src TypeScript lines as a soft budget: printed, never failing the check.
#[test]
fn cap_check_reports_web_lines_without_capping_them() {
    let d = tmpdir("cap-web");
    std::fs::create_dir_all(d.join("crates/a/src")).unwrap();
    std::fs::create_dir_all(d.join("web/src/tabs")).unwrap();
    std::fs::write(d.join("crates/a/src/lib.rs"), "fn a() {}\nfn b() {}\n").unwrap();
    std::fs::write(d.join("web/src/main.ts"), "let a = 1;\n".repeat(50)).unwrap();
    std::fs::write(d.join("web/src/tabs/q.ts"), "let b = 2;\n".repeat(7)).unwrap();
    std::fs::write(d.join("web/src/board.css"), "a {}\n".repeat(9)).unwrap();
    let cap = |n: u32| {
        std::fs::write(d.join("cap.toml"), format!("[fleet.x]\nline_cap = {n}\n")).unwrap();
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_drover"))
            .args(["cap-check", "--config", "cap.toml"])
            .current_dir(&d)
            .output()
            .unwrap();
        (
            o.status.code().unwrap(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
        )
    };
    for (n, code) in [(2, 0), (1, 1)] {
        let (got, out) = cap(n);
        assert_eq!(got, code, "{out}");
        assert!(out.contains(&format!("src lines (formatted): 2 / cap {n}")), "{out}");
        assert!(
            out.contains("web lines (TypeScript, soft budget, not capped): 57"),
            "{out}"
        );
    }
}

/// #668: `drover cap <fleet>` sums every open PR's `src/ net` (its body's, else its measured diff) onto integration's
/// total, lists the queued issues with no PR and proposes one raise to the projected total plus margin.
#[test]
fn cap_prints_the_queue_wide_view() {
    let (_d, seed, c, _) = repos("cap-view");
    std::fs::create_dir_all(seed.join("src")).unwrap();
    std::fs::create_dir_all(seed.join("ci")).unwrap();
    std::fs::write(
        seed.join("ci/cap.toml"),
        "[fleet.sendmeter]\nline_cap = 1000\nwarn_within = 100\n",
    )
    .unwrap();
    commit(&seed, "src/a.rs", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n", "staging");
    git(&seed, &["checkout", "-q", "-B", "issue-4", "staging"]);
    commit(&seed, "src/b.rs", "1\n2\n3\n", "issue-4");
    git(&seed, &["checkout", "-q", "staging"]);
    let pr = |n: u64, body: &str| {
        json!({"number": 10 + n, "state": "OPEN", "baseRefName": "staging", "body": body}).to_string()
    };
    let r = Mixed(
        Fake::default()
            .reply(
                "gh pr view issue-1",
                &pr(1, "## Lines\nsrc/ net +120 (budget +100): the parser"),
            )
            .reply("gh pr view issue-2", &pr(2, "src/ net +1,300 (budget +900)"))
            .reply("gh pr view issue-3", &pr(3, "Closes #3. src/ net -20 (budget +0)"))
            .reply("gh pr view issue-4", &pr(4, "no figure here")),
    );
    assert_eq!(
        run(&c, &r, &["queue", "sendmeter", "add", "1", "2", "3", "4", "9"]).0,
        0
    );
    let (code, out) = run(&c, &r, &["cap", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    for want in [
        "sendmeter: 10 / cap 1000 at staging",
        "headroom 990",
        "  #1 (PR #11): +120 (body)",
        "  #2 (PR #12): +1300 (body)",
        "  #3 (PR #13): -20 (body)",
        "  #4 (PR #14): +3 (measured)",
        "queued, no PR: #9\n",
        "sum of open PRs: +1403; projected total 1413 / cap 1000, headroom -413",
        "raise to 1513 in one orchestrator-filed issue (recommended) 2. trim",
    ] {
        assert!(out.contains(want), "{want:?} in\n{out}");
    }
}

/// git and `sh` run for real (a fleet's cap script), the rest is scripted.
struct Shell(Mixed);

impl Runner for Shell {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        if prog == "sh" {
            return Real.run(prog, args);
        }
        self.0.run(prog, args)
    }
    fn sleep(&self, _: u64) {}
}

/// #754: `repos` with `key` (a `premerge` or `cap_script` line) in the fleet, `script` as `scripts/line-cap.sh`,
/// `src/a.rs` (10 lines) and `web/ts/b.ts` (5) on staging, and `issue-4` adding `web/ts/c.ts` (3) as open PR #14.
fn script_repos(name: &str, key: &str, script: &str) -> (drover::Ctx, Shell) {
    let (d, seed, _, _) = repos(name);
    for dir in ["src", "web/ts", "scripts"] {
        std::fs::create_dir_all(seed.join(dir)).unwrap();
    }
    std::fs::write(seed.join("scripts/line-cap.sh"), script).unwrap();
    std::fs::write(seed.join("web/ts/b.ts"), "1\n2\n3\n4\n5\n").unwrap();
    commit(&seed, "src/a.rs", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n", "staging");
    git(&seed, &["checkout", "-q", "-B", "issue-4", "staging"]);
    commit(&seed, "web/ts/c.ts", "1\n2\n3\n", "issue-4");
    git(&seed, &["checkout", "-q", "staging"]);
    let config = CONFIG.replace("/tmp/does-not-matter/sendmeter", d.join("local").to_str().unwrap());
    let c = ctx(
        &d,
        &config.replace(
            "integration = \"staging\"",
            &format!("integration = \"staging\"\n{key}"),
        ),
    );
    let pr = json!({"number": 14, "state": "OPEN", "baseRefName": "staging", "body": "src/ net +0 (budget +0)"});
    let r = Shell(Mixed(Fake::default().reply("gh pr view issue-4", &pr.to_string())));
    assert_eq!(run(&c, &r, &["queue", "sendmeter", "add", "4"]).0, 0);
    (c, r)
}

/// #754: a repo whose `premerge` cap script also counts TypeScript: `drover cap` shows the script's total, each
/// open PR's net by the script on its merge, and projects and raises from that count, not the built-in Rust one.
#[test]
fn cap_uses_the_fleets_own_cap_script() {
    let script = "n=$(cat src/*.rs web/ts/*.ts | grep -c .)\necho \"line cap: $n / cap 100\"\n[ \"$n\" -le 100 ]\n";
    let (c, r) = script_repos("cap-script", "premerge = [\"sh scripts/line-cap.sh\"]", script);
    let (code, out) = run(&c, &r, &["cap", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    for want in [
        "sendmeter: line cap 15 / cap 100 at staging",
        "headroom 85\n",
        "  #4 (PR #14): +3 (script)",
        "sum of open PRs: +3; projected total 18 / cap 100, headroom 82",
        "ask options: 1. raise line cap to 518 in one orchestrator-filed issue (recommended) 2. trim",
    ] {
        assert!(out.contains(want), "{want:?} in\n{out}");
    }
}

/// #754: a `cap_script` that prints two caps: each is shown, projected and raised with its own headroom.
#[test]
fn cap_shows_each_cap_a_script_prints() {
    let script = "echo \"src: $(cat src/*.rs | grep -c .) / cap 100\"\necho \"apple_cap: $(cat web/ts/*.ts | grep -c .) / cap 6\"\n";
    let (c, r) = script_repos("cap-two", "cap_script = \"sh scripts/line-cap.sh\"", script);
    let (code, out) = run(&c, &r, &["cap", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    for want in [
        "sendmeter: src 10 / cap 100 at staging",
        "headroom 90\n",
        "sendmeter: apple_cap 5 / cap 6 at staging",
        "headroom 1\n",
        "  #4 (PR #14): +0 src, +3 apple_cap (script)",
        "src: sum of open PRs: +0; projected total 10 / cap 100, headroom 90",
        "apple_cap: sum of open PRs: +3; projected total 8 / cap 6, headroom -2",
        "ask options: 1. raise apple_cap to 508 in one orchestrator-filed issue (recommended) 2. trim",
    ] {
        assert!(out.contains(want), "{want:?} in\n{out}");
    }
}

#[test]
fn cap_runs_the_repos_line_cap_script_when_the_fleet_names_none() {
    let script = include_str!("support/cooee-line-cap.sh");
    let (c, r) = script_repos("cap-repo-script", "", script);
    let seed = c.worktrees.with_file_name("seed");
    for dir in ["crates/core/src", "web/src", "ci"] {
        std::fs::create_dir_all(seed.join(dir)).unwrap();
    }
    std::fs::write(
        seed.join("ci/cap.toml"),
        "[fleet.cooee]\nline_cap = 1000\napple_cap = 50\n",
    )
    .unwrap();
    std::fs::write(seed.join("web/src/main.rs"), "fn main() {}\n").unwrap();
    let lib = "fn a() {}\n\nfn b() {}\n#[cfg(test)]\nmod t {\n    fn x() {}\n}\n";
    commit(&seed, "crates/core/src/lib.rs", lib, "staging");
    let (code, out) = run(&c, &r, &["cap", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    for want in [
        "sendmeter: non-test lines 8 / cap 1000 at staging",
        "headroom 992\n",
        "sendmeter: apple lines 0 / cap 50 at staging",
        "counted by `sh scripts/line-cap.sh`",
        "  #4 (PR #14): +3 non-test lines, +0 apple lines (script)",
        "non-test lines: sum of open PRs: +3; projected total 11 / cap 1000, headroom 989",
    ] {
        assert!(out.contains(want), "{want:?} in\n{out}");
    }
}

/// #788: no `premerge` or `cap_script`, but the repo has `scripts/line-cap.sh` printing cooee's `label: N / M` lines
/// (no `cap` word) for two caps: `drover cap` counts by that script, extra paths included, each cap on its own.
#[test]
fn cap_finds_the_repos_own_line_cap_script() {
    let script =
        "echo \"non-test lines: $(cat src/*.rs web/ts/*.ts | grep -c .) / 100\"\necho \"apple lines: 0 / 50\"\n";
    let (c, r) = script_repos("cap-own", "", script);
    let (code, out) = run(&c, &r, &["cap", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    for want in [
        "sendmeter: non-test lines 15 / cap 100 at staging",
        "sendmeter: apple lines 0 / cap 50 at staging",
        "  #4 (PR #14): +3 non-test lines, +0 apple lines (script)",
        "non-test lines: sum of open PRs: +3; projected total 18 / cap 100, headroom 82",
    ] {
        assert!(out.contains(want), "{want:?} in\n{out}");
    }
}

/// #788: a cap script whose output has no `N / cap M` line is reported as that, with the line it printed.
#[test]
fn cap_reports_a_script_it_cannot_parse() {
    let script = "echo 'counting...'\necho 'total: 15 lines, limit 100'\n";
    let (c, r) = script_repos("cap-garbled", "", script);
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let err = drover::overlap::cap_view(&c, &cfg, &r, "sendmeter", &mut Vec::new()).unwrap_err();
    let want = "`sh scripts/line-cap.sh` at origin/staging printed no `N / cap M` line drover can parse: printed `total: 15 lines, limit 100`";
    assert_eq!(err.to_string(), want);
}
