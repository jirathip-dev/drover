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
    drover::land::auto_update(&c, &cfg, &sh.on(&r), &mut out, "sendmeter").unwrap();
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
    let r = merger(3, &to);
    let (code, out) = run(&c, &r, &["merge", "sendmeter", "3"]);
    assert_eq!(code, 0, "{out}");
    assert!(r.0.called(&format!(
        "gh pr merge 13 -R jirathip-dev/sendmeter --squash --delete-branch --match-head-commit {to}"
    )));
    let st = &fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 3)];
    assert_eq!((st.state.as_str(), st.rounds), ("merged", 1));
}

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
    merge_staging(&d, Some("sneaky\n"));
    let h = head(&seed, "issue-3");
    let r = merger(3, &h);
    let (code, out) = run(&c, &r, &["merge", "sendmeter", "3"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED STALE_VERDICT"), "{out}");
    assert!(!r.0.called("gh pr merge"));
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
    assert!(sh.has("sendmeter-rev-3-r2"), "{:?}", sh.methods());
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
    commit(&seed, "c.txt", "c\n", "staging");
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
    let (r, out) = pass(&c, &green_pr(3, &from));
    let to = head(&seed, "issue-3");
    assert!(
        out.contains(&format!("sendmeter #3 ready (CI pending at {to:.7})")),
        "{out}"
    );
    assert_eq!(r.0.count("gh pr merge"), 1, "{:?}", r.0.calls.borrow());
    assert_eq!(st(&c), "ready");
    c.now += 120;
    let none =
        json!({"number": 13, "state": "OPEN", "baseRefName": "staging", "headRefOid": to, "statusCheckRollup": []});
    let (r, _) = pass(&c, &none.to_string());
    assert!(!r.0.called("gh pr merge"));
    let (code, out) = run(&c, &r, &["merge", "sendmeter", "3"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED CI_NOT_GREEN"), "{out}");
    assert_eq!(st(&c), "ready");
    c.now += 120;
    let (r, out) = pass(&c, &green_pr(3, &to));
    let merge = format!("gh pr merge 13 -R jirathip-dev/sendmeter --squash --delete-branch --match-head-commit {to}");
    assert!(r.0.called(&merge), "{out}");
    assert_eq!(st(&c), "merged");
}

#[test]
fn conflicting_auto_update_spawns_an_update_lane() {
    let (_d, seed, c, cfg) = repos("au-conflict");
    lane(&c, &seed, 1, true);
    let before = head(&seed, "issue-1");
    git(&seed, &["merge", "-q", "--ff-only", "origin/issue-2"]);
    git(&seed, &["push", "-q", "origin", "HEAD:staging"]);
    let r = Mixed(Fake::default());
    drover::land::auto_update(&c, &cfg, &r, &mut vec![], "sendmeter").unwrap();
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
    assert_eq!((p.len(), p[0].issue, p[0].val("other")), (1, Some(2), &json!(1)));
    let card = "may conflict with #1; keep going, drover updates you after it merges";
    assert_eq!(p[0].str("reason"), Some(card), "#320");
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

fn reviewed(c: &drover::Ctx, n: u64, sha: &str) -> String {
    reviewed_with(c, n, sha, "PASS\nall good\n")
}

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
    commit(&seed, "c.txt", "c\n", "staging");
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
    let sh = FakeShed::attached(&c);
    sh.lane("sendmeter-rev-3-r1", "working", "/wt");
    for live in [true, false] {
        if !live {
            sh.remove("sendmeter-rev-3-r1");
        }
        let r = Mixed(Fake::default());
        drover::land::auto_update(&c, &cfg, &sh.on(&r), &mut vec![], "sendmeter").unwrap();
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
        "gh pr merge 13 -R jirathip-dev/sendmeter --squash --delete-branch --match-head-commit {m}"
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

fn labelled(d: &Path) -> Config {
    let config = CONFIG.replace("/tmp/does-not-matter/sendmeter", d.join("local").to_str().unwrap());
    Config::parse(&config.replace("merge_label = \"off\"", "merge_label = \"merge-ready\"")).unwrap()
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
    drover::land::auto_update(&c, &cfg, &sh.on(&r), &mut vec![], "sendmeter").unwrap();
    assert_eq!(head(&seed, "issue-3"), from, "nothing pushed");
    let ev = read(&c.state).unwrap().to_vec();
    assert!(
        ev.iter().all(|e| e.kind != "auto-updated" && e.kind != "driven"),
        "{ev:?}"
    );
    let behind = json!({"number": 13, "state": "OPEN", "baseRefName": "staging", "headRefOid": from,
        "mergeable": "MERGEABLE", "mergeStateStatus": "BEHIND", "statusCheckRollup": []});
    let w = Mixed(Fake::default().reply("gh pr view issue-3 ", &behind.to_string()));
    let ws = drover::waiting::scan(&c, &cfg, &sh.on(&w), &read(&c.state).unwrap(), &[], true, None);
    let what = &ws.iter().find(|(s, _)| s.issue == 3).expect("#3").1.what;
    assert!(
        what.starts_with("PASS · behind staging (not required) · merging when CI green"),
        "{what}"
    );
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
    let merge = format!("gh pr merge 13 -R jirathip-dev/sendmeter --squash --delete-branch --match-head-commit {from}");
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
    drover::land::auto_update(&c, &cfg, &sh.on(&r), &mut out, "sendmeter").unwrap();
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
    let cfg = cfg.replacen("merge_label = \"off\"", "merge_label = \"merge-ready\"", 1);
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

#[test]
fn a_second_pass_with_nothing_moved_starts_no_worktree() {
    let (d, seed, c, cfg) = repos("au-idle");
    let sh = FakeShed::attached(&c);
    for n in 1..=3 {
        lane(&c, &seed, n, true);
    }
    commit(&seed, "c.txt", "c\n", "staging");
    merge_staging(&d, None);
    let heads: Vec<String> = (1..=3).map(|n| head(&seed, &format!("issue-{n}"))).collect();
    let r = Mixed(Fake::default().reply(PROTECTION, "false\n"));
    let t = std::time::Instant::now();
    drover::land::auto_update(&c, &cfg, &sh.on(&r), &mut vec![], "sendmeter").unwrap();
    let first = r.0.calls.borrow().len();
    drover::land::auto_update(&c, &cfg, &sh.on(&r), &mut vec![], "sendmeter").unwrap();
    let ms = t.elapsed().as_millis();
    let second: Vec<String> = r.0.calls.borrow()[first..]
        .iter()
        .filter(|l| l.starts_with("git "))
        .cloned()
        .collect();
    let tried = |w: &str| second.iter().filter(|l| l.contains(w)).count();
    assert_eq!(
        (tried(" worktree "), tried(" merge-tree "), tried(" merge ")),
        (0, 0, 0),
        "{} git calls in {ms} ms: {second:#?}",
        second.len()
    );
    let now: Vec<String> = (1..=3).map(|n| head(&seed, &format!("issue-{n}"))).collect();
    assert_eq!(now, heads, "nothing pushed");
    assert!(read(&c.state)
        .unwrap()
        .iter()
        .all(|e| e.kind != "auto-updated" && e.kind != "driven"));
}
