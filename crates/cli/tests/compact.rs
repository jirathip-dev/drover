mod common;
use common::*;
use drover::config::Config;
use drover::events::{append, history, Event};
use drover::runner::{Out, Runner};
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

struct Git<'a>(&'a Fake);

impl Runner for Git<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.run_in(prog, args, "")
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        if prog != "git" && prog != "sh" {
            return self.0.run_in(prog, args, input);
        }
        self.0.calls.borrow_mut().push(format!("{prog} {}", args.join(" ")));
        let mut c = Command::new(prog)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        c.stdin.take().unwrap().write_all(input.as_bytes())?;
        let o = c.wait_with_output()?;
        Ok(Out {
            code: o.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&o.stdout).into(),
            stderr: String::from_utf8_lossy(&o.stderr).into(),
        })
    }
}

fn g(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "init.defaultBranch=staging",
            "-C",
        ])
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

fn commit(dir: &Path, file: &str, text: &str, msg: &str) -> String {
    std::fs::write(dir.join(file), text).unwrap();
    g(dir, &["add", "."]);
    g(dir, &["commit", "-qm", msg]);
    g(dir, &["rev-parse", "HEAD"])
}

const NOTE: &str = "Decided: count twice\nOwner: the meter counts two\n";

#[test]
fn compact_keeps_every_tree_answers_why_needs_a_fresh_review_and_undoes_exactly() {
    let dir = tmpdir("compact-sim");
    let (origin, archive, seed, local) = (
        dir.join("jirathip-dev/sendmeter"),
        dir.join("jirathip-dev/sendmeter-history"),
        dir.join("seed"),
        dir.join("local"),
    );
    for b in [&origin, &archive] {
        g(&dir, &["init", "-q", "--bare", b.to_str().unwrap()]);
    }
    g(&dir, &["init", "-q", seed.to_str().unwrap()]);
    g(&seed, &["remote", "add", "origin", origin.to_str().unwrap()]);
    commit(&seed, "a.rs", "one\ntwo\nthree\n", "base");
    g(&seed, &["checkout", "-qb", "old-wip"]);
    commit(&seed, "w.rs", "wip\n", "wip: never merged");
    g(&seed, &["checkout", "-q", "staging"]);
    g(&seed, &["checkout", "-qb", "issue-5"]);
    let c5 = commit(&seed, "a.rs", "ONE\ntwo\nthree\n", "a: old line");
    g(
        &seed,
        &["notes", "--ref", "why", "add", "-m", "Owner: the old note", &c5],
    );
    g(&seed, &["checkout", "-q", "staging"]);
    g(&seed, &["merge", "-q", "--no-ff", "-m", "a: old (#5) (#50)", "issue-5"]);
    let cut = g(&seed, &["rev-parse", "HEAD"]);
    g(&seed, &["checkout", "-qb", "issue-1"]);
    commit(&seed, "a.rs", "ONE\nTWO\nthree\n", "a: two");
    g(&seed, &["checkout", "-q", "staging"]);
    commit(&seed, "b.rs", "b\n", "direct: b");
    g(&seed, &["checkout", "-q", "issue-1"]);
    g(&seed, &["merge", "-q", "--no-edit", "staging"]);
    let l2 = commit(&seed, "c.rs", "c\n", "a: test it");
    g(&seed, &["notes", "--ref", "why", "add", "-m", NOTE.trim(), &l2]);
    g(&seed, &["checkout", "-q", "staging"]);
    g(&seed, &["merge", "-q", "--no-ff", "-m", "a: two (#1) (#11)", "issue-1"]);
    let m1 = g(&seed, &["rev-parse", "HEAD"]);
    g(&seed, &["checkout", "-qb", "issue-2"]);
    commit(&seed, "d.rs", "d\n", "d: add");
    g(&seed, &["checkout", "-q", "staging"]);
    commit(&seed, "e.rs", "e\n", "direct: e");
    g(&seed, &["checkout", "-q", "issue-2"]);
    g(&seed, &["merge", "-q", "--no-edit", "staging"]);
    let o2 = g(&seed, &["rev-parse", "HEAD"]);
    let all = ["staging", "issue-1", "issue-2", "issue-5", "old-wip", "refs/notes/why"];
    g(&seed, &[&["push", "-q", "origin"][..], &all].concat());
    g(&seed, &["push", "-q", "origin", "issue-2:refs/pull/22/head"]);
    g(
        &dir,
        &["clone", "-q", origin.to_str().unwrap(), local.to_str().unwrap()],
    );
    g(&local, &["fetch", "-q", "origin", "refs/notes/why:refs/notes/why"]);
    let config = CONFIG
        .replace("/tmp/does-not-matter/sendmeter", local.to_str().unwrap())
        .replace(
            "integration = \"staging\"   # never main",
            "integration = \"staging\"\npaused = true",
        );
    let c = ctx(&dir, &config);
    let ev = |n: u64, kind: &str, d: serde_json::Value| {
        append(&c.state, &Event::new(10, "sendmeter", Some(n), kind, d)).unwrap();
    };
    ev(5, "why", json!({"sha": c5, "round": 1, "files": ["a.rs"]}));
    ev(
        5,
        "merged",
        json!({"pr": "50", "merge_sha": cut, "verdict_sha": c5, "into": "staging"}),
    );
    ev(1, "why", json!({"sha": l2, "round": 1, "files": ["a.rs"]}));
    ev(1, "verdict", json!({"verdict": "PASS", "sha": l2}));
    ev(
        1,
        "merged",
        json!({"pr": "11", "merge_sha": m1, "verdict_sha": l2, "into": "staging"}),
    );
    ev(2, "verdict", json!({"verdict": "PASS", "sha": o2}));
    let refs = || g(&dir, &["ls-remote", origin.to_str().unwrap()]);
    let before = refs();
    let undo = before.lines().find(|l| l.ends_with("refs/heads/staging")).unwrap()[..40].to_string();
    let old_tree = g(&local, &["rev-parse", "origin/staging^{tree}"]);
    let body = format!("Closes #2\n\n## Evidence (head {o2:.9})");
    let fake = Fake::default()
        .reply(
            "gh pr list",
            &json!([{"number": 22, "headRefName": "issue-2", "isCrossRepository": false}]).to_string(),
        )
        .reply("gh repo view", &json!({"visibility": "PRIVATE"}).to_string())
        .reply("gh pr view 22", &json!({"body": body}).to_string())
        .reply("gh pr edit", "")
        .reply("gh issue view", &json!({"title": "Meter", "body": "x"}).to_string());
    let r = Git(&fake);

    let (code, out) = run(&c, &r, &["compact", "sendmeter", "--cut", &cut, "--dry-run"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("commits   staging 9 → 4 (1 root + 3)"), "{out}");
    assert!(out.contains("a: two (#1) (#11)  holds 3 old commits"), "{out}");
    assert!(
        out.contains("#22 refs/heads/issue-2") && out.contains("Carried"),
        "{out}"
    );
    assert!(out.contains("Detached 1 · Merged 2 · Notes 1"), "{out}");
    assert!(
        out.contains("events    2 issues to remap") && out.contains("1 lane heads held"),
        "{out}"
    );
    assert!(out.contains("checks    tip trees equal ✓ · ready ✓"), "{out}");
    assert_eq!(refs(), before, "a dry run pushes nothing");
    let tip = |o: &str| {
        o.lines()
            .find_map(|l| l.strip_prefix("new tip   "))
            .unwrap()
            .split(' ')
            .next()
            .unwrap()
            .to_string()
    };
    let new = tip(&out);
    let (_, again) = run(&c, &r, &["compact", "sendmeter", "--cut", &cut, "--dry-run"]);
    assert_eq!(tip(&again), new, "the plan is the same on a second run");

    let push = format!("drover compact sendmeter --push {new}");
    let (code, out) = run(&c, &r, &["compact", "sendmeter", "--push", &new]);
    assert_eq!((code, out.contains("COMPACT_UNAPPROVED")), (3, true), "{out}");
    let card = |id: &str, cmd: &str| {
        for (kind, d) in [
            ("run-card", json!({"id": id, "cmd": cmd, "thread": "t"})),
            ("run-settled", json!({"id": id, "approved": true})),
            ("run-started", json!({"id": id})),
        ] {
            append(&c.state, &Event::new(20, "sendmeter", None, kind, d)).unwrap();
        }
    };
    card("r1", &push);
    let (code, out) = run(&c, &r, &["compact", "sendmeter", "--push", &new]);
    assert_eq!(code, 0, "{out}");
    let on = |b: &str| g(&dir, &["--git-dir", origin.to_str().unwrap(), "rev-parse", b]);
    assert_eq!(on("staging"), new);
    assert_eq!(on("staging^{tree}"), old_tree, "the tip tree is identical");
    assert_eq!(
        g(
            &dir,
            &["--git-dir", origin.to_str().unwrap(), "rev-list", "--count", "staging"]
        ),
        "4"
    );
    assert!(
        !refs().contains("refs/heads/issue-5"),
        "a fully merged branch is deleted"
    );
    let arc = |b: &str| g(&dir, &["--git-dir", archive.to_str().unwrap(), "rev-parse", b]);
    assert_eq!(arc("staging"), undo, "the archive holds the old tip");
    assert_eq!(arc("pull/22"), o2, "and each PR head");
    assert_eq!(arc("drover-compact:compact-map.tsv").len(), 40);
    let o2_new = on("issue-2");
    assert_eq!(
        on("issue-2^{tree}"),
        g(&seed, &["rev-parse", &format!("{o2}^{{tree}}")])
    );
    assert!(fake.called(&format!(
        "gh pr edit 22 -R jirathip-dev/sendmeter --body Closes #2\n\n## Evidence (head {o2_new:.9})"
    )));
    assert_eq!(g(&local, &["rev-parse", "HEAD"]), new, "the fleet's clone is reset");

    let look = dir.join("look");
    g(&dir, &["clone", "-q", origin.to_str().unwrap(), look.to_str().unwrap()]);
    let (code, out) = run(&c, &r, &["why", &format!("{}:2", look.join("a.rs").display())]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("a: two (#1) (#11)") && out.contains("PR #11 · merged"),
        "{out}"
    );
    assert!(out.contains("  Owner: the meter counts two"), "{out}");
    let (code, out) = run(&c, &r, &["why", &format!("{}:1", look.join("a.rs").display())]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("a: old line") && out.contains("PR #50 · merged"), "{out}");
    assert!(out.contains("  Owner: the old note"), "{out}");

    let cfg = Config::load(&c.config).unwrap();
    let f = cfg.fleet("sendmeter").unwrap();
    let pr = json!({"number": 22, "state": "OPEN", "baseRefName": "staging", "headRefOid": o2_new,
        "statusCheckRollup": [{"__typename": "CheckRun", "name": "build", "status": "COMPLETED", "conclusion": "SUCCESS"}]});
    let st = |n| drover::events::fold(&history(&c.state).unwrap()).remove(&("sendmeter".to_string(), n));
    let refused = drover::gate::gate(f, &pr, st(2).as_ref(), false).unwrap_err();
    assert_eq!(refused.code, "STALE_VERDICT", "the old PASS does not carry");
    ev(2, "verdict", json!({"verdict": "PASS", "sha": o2_new}));
    assert_eq!(drover::gate::gate(f, &pr, st(2).as_ref(), false).unwrap(), o2_new);
    let events = history(&c.state).unwrap();
    drover::compact::rooted(&r, &events, ("sendmeter", f), &local, &o2_new).unwrap();
    let old = drover::compact::rooted(&r, &events, ("sendmeter", f), &local, &o2).unwrap_err();
    assert_eq!(old.code, "OLD_ROOT");

    card("r2", &format!("drover compact sendmeter --undo --push {undo}"));
    let (code, out) = run(&c, &r, &["compact", "sendmeter", "--undo", "--push", &undo]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(refs(), before, "undo restores the old tip and every ref exactly");
    let events = history(&c.state).unwrap();
    assert!(events.iter().any(|e| e.kind == "compact-undone"));
    drover::compact::rooted(&r, &events, ("sendmeter", f), &local, &o2).unwrap();
}
