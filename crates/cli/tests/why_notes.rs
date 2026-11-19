mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::runner::{Out, Runner};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Git<'a> {
    fake: &'a Fake,
    before_push: std::sync::Mutex<Option<Box<dyn FnOnce() + Send + 'a>>>,
}

impl<'a> Git<'a> {
    fn new(fake: &'a Fake) -> Self {
        Git {
            fake,
            before_push: std::sync::Mutex::new(None),
        }
    }
}

impl Runner for Git<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.run_in(prog, args, "")
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        if prog != "git" {
            return self.fake.run_in(prog, args, input);
        }
        self.fake.calls.borrow_mut().push(format!("git {}", args.join(" ")));
        if args.iter().any(|a| a == "push") && args.iter().any(|a| a.contains("refs/notes")) {
            if let Some(f) = self.before_push.lock().unwrap().take() {
                f();
            }
        }
        let mut c = Command::new("git")
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
    fn sleep(&self, secs: u64) {
        self.fake.sleep(secs);
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

const SPEC: &str = "Meter the sends. The meter counts every send once, retries included.";

struct Repo {
    dir: PathBuf,
    origin: PathBuf,
    local: PathBuf,
    wt: PathBuf,
    head: String,
}

fn repo(name: &str) -> Repo {
    let dir = tmpdir(name);
    let (origin, seed) = (dir.join("origin.git"), dir.join("seed"));
    g(&dir, &["init", "-q", "--bare", origin.to_str().unwrap()]);
    g(&dir, &["init", "-q", seed.to_str().unwrap()]);
    std::fs::write(seed.join("a.rs"), "one\ntwo\nthree\n").unwrap();
    g(&seed, &["add", "."]);
    g(&seed, &["commit", "-qm", "base"]);
    g(&seed, &["remote", "add", "origin", origin.to_str().unwrap()]);
    g(&seed, &["push", "-q", "origin", "staging"]);
    g(&seed, &["checkout", "-qb", "issue-12"]);
    std::fs::write(seed.join("a.rs"), "one\nTWO\nthree\n").unwrap();
    g(&seed, &["commit", "-qam", "meter: count sends"]);
    g(&seed, &["push", "-q", "origin", "issue-12"]);
    let head = g(&seed, &["rev-parse", "HEAD"]);
    let (local, wt) = (dir.join("local"), dir.join("wt"));
    g(
        &dir,
        &["clone", "-q", origin.to_str().unwrap(), local.to_str().unwrap()],
    );
    g(
        &dir,
        &[
            "clone",
            "-q",
            "-b",
            "issue-12",
            origin.to_str().unwrap(),
            wt.to_str().unwrap(),
        ],
    );
    Repo {
        dir,
        origin,
        local,
        wt,
        head,
    }
}

fn lane(repo: &Repo, update: bool) -> (drover::Ctx, FakeShed) {
    set_env("DROVER_LANE", "sendmeter:12:impl");
    let c = ctx(
        &repo.dir,
        &CONFIG.replace("/tmp/does-not-matter/sendmeter", repo.local.to_str().unwrap()),
    );
    let ev = |ts, kind: &str, d: Value| append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
    ev(
        10,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12", "worktree": repo.wt, "branch": "issue-12", "update": update}),
    );
    ev(
        11,
        "needs-decision",
        json!({"id": "q5", "question": "Count retries?", "kind": "owner", "agent": "sendmeter-impl-12"}),
    );
    ev(
        12,
        "decision",
        json!({"id": "q5", "choice": "yes, each once", "by": "owner", "via": "web"}),
    );
    let sh = FakeShed::new();
    sh.attach(&c);
    (c, sh)
}

fn body(head: &str) -> String {
    format!("Closes #12\n\n## For humans\n\nx\n\n## Evidence (head {head})")
}

fn fake(head: &str) -> Fake {
    fake_body(head, &body(head))
}

fn fake_body(head: &str, body: &str) -> Fake {
    let pr = json!({"number": 100, "state": "OPEN", "headRefOid": head, "body": body});
    Fake::default()
        .reply("gh pr view issue-12", &pr.to_string())
        .reply(
            "gh issue view 12",
            &json!({"title": "Meter sends", "body": SPEC}).to_string(),
        )
        .reply("gh pr edit", "")
}

const NOTE: &str = "Decided: count a retry as the same send\n\
Because: \"counts every send once, retries included\"\n\
Rejected: a counter per attempt — the spec counts sends\n\
Because: q5\n\
Check: a.rs:2 counts the retry once\n\
Owner: retries no longer double the meter\n";

fn origin_note(repo: &Repo, sha: &str) -> Option<String> {
    let o = Command::new("git")
        .arg("--git-dir")
        .arg(&repo.origin)
        .args(["notes", "--ref", "why", "show", sha])
        .output()
        .unwrap();
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

#[test]
fn why_note_written_on_origin_pr_body_and_log() {
    let repo = repo("why-written");
    let (c, _sh) = lane(&repo, false);
    std::fs::write(repo.wt.join("why.md"), NOTE).unwrap();
    let r = fake(&repo.head);
    let (code, out) = run(&c, &Git::new(&r), &["turn", "done"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(origin_note(&repo, &repo.head).as_deref(), Some(NOTE));
    let calls = r.calls.borrow();
    let edit = calls
        .iter()
        .find(|c| c.starts_with("gh pr edit 100 -R jirathip-dev/sendmeter --body"))
        .unwrap();
    assert!(edit.contains("Closes #12") && edit.contains("## For humans"), "{edit}");
    assert!(edit.contains("## Why (round 1)\n\nDecided: count a retry"), "{edit}");
    let w = &kinds(&c, "why")[0];
    assert_eq!(
        (w.issue, w.str("sha"), w.val("round").as_u64()),
        (Some(12), Some(repo.head.as_str()), Some(1))
    );
    assert_eq!(w.str("owner"), Some("retries no longer double the meter"));
    assert_eq!(*w.val("files"), json!(["a.rs"]));
    assert!(kinds(&c, "turn-end")[0].data.get("note").is_none());
    assert!(!repo.wt.join("why.md").exists());
}

#[test]
fn an_update_rounds_short_note_is_accepted() {
    let repo = repo("why-short");
    let (c, _sh) = lane(&repo, true);
    let short = "Decided: merged staging, no conflicts\nBecause: q5\nRejected: none\nOwner: brought up to date\n";
    std::fs::write(repo.wt.join("why.md"), short).unwrap();
    let (code, out) = run(&c, &Git::new(&fake(&repo.head)), &["turn", "done"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(origin_note(&repo, &repo.head).as_deref(), Some(short));
}

#[test]
fn an_invalid_note_is_refused_with_its_reason_and_nothing_is_pushed() {
    let cases = [
        (None, "why.md missing"),
        (
            Some(NOTE.replace("Check: a.rs:2 counts the retry once\n", "")),
            "why.md form: no Check: line",
        ),
        (
            Some(NOTE.replace("Because: q5", "Because: the owner said so")),
            "why.md:4 uncited",
        ),
        (
            Some(NOTE.replace("Because: q5", "Because: q99")),
            "why.md:4 cite: q99 is no answered ask",
        ),
        (
            Some(NOTE.replace("Because: q5", "Because: \"each send is metered twice\"")),
            "is not in the spec",
        ),
        (
            Some(NOTE.replace("Because: q5", "Because: a.rs:9")),
            "cite: a.rs:9 past its end (3)",
        ),
        (
            Some(NOTE.replace("Because: q5", "Because: token ghp_abcdefghij0123456789xyz")),
            "why.md:4 secret: Because: token •••",
        ),
    ];
    for (i, (note, why)) in cases.into_iter().enumerate() {
        let repo = repo(&format!("why-refused-{i}"));
        let (c, _sh) = lane(&repo, false);
        if let Some(n) = &note {
            std::fs::write(repo.wt.join("why.md"), n).unwrap();
        }
        let r = fake(&repo.head);
        let (code, out) = run(&c, &Git::new(&r), &["turn", "done"]);
        assert_eq!(code, 3, "{why}: {out}");
        assert!(
            out.contains("REFUSED WHY_INVALID: ") && out.contains(why),
            "{why}: {out}"
        );
        assert!(!out.contains("ghp_abcdefghij"), "{out}");
        let ls = g(&repo.dir, &["ls-remote", repo.origin.to_str().unwrap(), "refs/notes/*"]);
        assert_eq!(ls, "", "{why}: nothing pushed");
        assert!(!r.called("gh pr edit") && kinds(&c, "why").is_empty(), "{why}");
        let t = &kinds(&c, "turn-end")[0];
        assert!(
            t.str("note").unwrap().starts_with("WHY_INVALID: "),
            "{why}: the turn still ends"
        );
    }
}

#[test]
fn a_note_pushed_between_fetch_and_push_is_kept() {
    let repo = repo("why-race-between");
    let (c, _sh) = lane(&repo, false);
    std::fs::write(repo.wt.join("why.md"), NOTE).unwrap();
    let base = g(&repo.local, &["rev-parse", "origin/staging"]);
    let other = repo.dir.join("other");
    g(
        &repo.dir,
        &["clone", "-q", repo.origin.to_str().unwrap(), other.to_str().unwrap()],
    );
    let r = fake(&repo.head);
    let git = Git::new(&r);
    let o = other.clone();
    *git.before_push.lock().unwrap() = Some(Box::new(move || {
        g(
            &o,
            &["notes", "--ref", "why", "add", "-m", "Owner: the other lane", &base],
        );
        g(&o, &["push", "-q", "origin", "refs/notes/why"]);
    }));
    let (code, out) = run(&c, &git, &["turn", "done"]);
    assert_eq!(code, 0, "{out}");
    let base = g(&repo.local, &["rev-parse", "origin/staging"]);
    assert_eq!(origin_note(&repo, &base).as_deref(), Some("Owner: the other lane\n"));
    assert_eq!(origin_note(&repo, &repo.head).as_deref(), Some(NOTE));
    let pushes = r
        .calls
        .borrow()
        .iter()
        .filter(|c| c.contains("push -q origin refs/notes/why-"))
        .count();
    assert_eq!(pushes, 2, "rejected once, then pushed");
}

#[test]
fn two_lanes_finishing_at_once_both_keep_their_notes() {
    let repo = repo("why-race-two");
    let (c, _sh) = lane(&repo, false);
    let seed = repo.dir.join("seed");
    g(&seed, &["checkout", "-qb", "issue-13", "staging"]);
    std::fs::write(seed.join("b.rs"), "b\n").unwrap();
    g(&seed, &["add", "."]);
    g(&seed, &["commit", "-qm", "b"]);
    g(&seed, &["push", "-q", "origin", "issue-13"]);
    let other = g(&seed, &["rev-parse", "HEAD"]);
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let f = cfg.fleet("sendmeter").unwrap();
    let r = Fake::default();
    let git = Git::new(&r);
    std::thread::scope(|s| {
        let a = s.spawn(|| drover::why::attach(&c, &git, f, "sendmeter", ("issue-12", &repo.head), "Owner: twelve\n"));
        let b = s.spawn(|| drover::why::attach(&c, &git, f, "sendmeter", ("issue-13", &other), "Owner: thirteen\n"));
        a.join().unwrap().unwrap();
        b.join().unwrap().unwrap();
    });
    assert_eq!(origin_note(&repo, &repo.head).as_deref(), Some("Owner: twelve\n"));
    assert_eq!(origin_note(&repo, &other).as_deref(), Some("Owner: thirteen\n"));
}

#[test]
fn why_reads_a_merged_line_back() {
    let repo = repo("why-cmd");
    let (c, _sh) = lane(&repo, false);
    let seed = repo.dir.join("seed");
    std::fs::write(seed.join("c.rs"), "c\n").unwrap();
    g(&seed, &["add", "."]);
    g(&seed, &["commit", "-qm", "meter: test it"]);
    g(&seed, &["push", "-q", "origin", "issue-12"]);
    let head = g(&seed, &["rev-parse", "HEAD"]);
    g(&repo.wt, &["pull", "-q", "origin", "issue-12"]);
    std::fs::write(repo.wt.join("why.md"), NOTE).unwrap();
    assert_eq!(run(&c, &Git::new(&fake(&head)), &["turn", "done"]).0, 0);
    g(&seed, &["checkout", "-q", "staging"]);
    g(
        &seed,
        &["merge", "-q", "--no-ff", "-m", "Merge pull request #100", "issue-12"],
    );
    g(&seed, &["push", "-q", "origin", "staging"]);
    let merge = g(&seed, &["rev-parse", "HEAD"]);
    let d = json!({"pr": "100", "merge_sha": merge, "verdict_sha": head, "into": "staging"});
    append(&c.state, &Event::new(50, "sendmeter", Some(12), "merged", d)).unwrap();
    let look = repo.dir.join("look");
    g(
        &repo.dir,
        &["clone", "-q", repo.origin.to_str().unwrap(), look.to_str().unwrap()],
    );
    let at = format!("{}:2", look.join("a.rs").display());
    let r = fake(&head);
    let (code, out) = run(&c, &Git::new(&r), &["why", &at]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("TWO") && out.contains("meter: count sends"), "{out}");
    assert!(
        out.contains("issue   #12 Meter sends") && out.contains("PR #100 · merged"),
        "{out}"
    );
    assert!(out.contains(&format!("note    round 1 · on {head:.9}")), "{out}");
    assert!(out.contains("  Owner: retries no longer double the meter"), "{out}");
    assert!(
        out.contains("q5 owner · Count retries?") && out.contains("→ owner (web): yes, each once"),
        "{out}"
    );
    let (code, out) = run(&c, &Git::new(&r), &["why", &at, "--json"]);
    assert_eq!(code, 0, "{out}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        (v["issue"]["number"].as_u64(), v["issue"]["merge_sha"].as_str()),
        (Some(12), Some(merge.as_str()))
    );
    assert_eq!(v["note"]["sha"], json!(head));
    assert_eq!(v["note"]["owner"], json!("retries no longer double the meter"));
    assert_eq!(v["asks"][0]["answer"]["choice"], json!("yes, each once"));
}

#[test]
fn a_squash_merge_lands_one_commit_carrying_every_lane_note() {
    let repo = repo("why-squash");
    let (c, _sh) = lane(&repo, false);
    let pr = |head: &str| {
        let green =
            json!([{"__typename": "CheckRun", "name": "build", "status": "COMPLETED", "conclusion": "SUCCESS"}]);
        let pr = json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": head,
            "body": body(head), "statusCheckRollup": green});
        Fake::default()
            .reply("gh pr view issue-12", &pr.to_string())
            .reply(
                "gh issue view 12",
                &json!({"title": "Meter sends", "body": SPEC}).to_string(),
            )
            .reply("gh pr edit", "")
    };
    std::fs::write(repo.wt.join("why.md"), NOTE).unwrap();
    assert_eq!(run(&c, &Git::new(&pr(&repo.head)), &["turn", "done"]).0, 0);
    let seed = repo.dir.join("seed");
    std::fs::write(seed.join("c.rs"), "c\n").unwrap();
    g(&seed, &["add", "."]);
    g(&seed, &["commit", "-qm", "meter: test it"]);
    g(&seed, &["push", "-q", "origin", "issue-12"]);
    let head = g(&seed, &["rev-parse", "HEAD"]);
    g(&repo.wt, &["pull", "-q", "origin", "issue-12"]);
    let second = "Decided: test the meter\nBecause: q5\nRejected: no test — the meter would drift\nCheck: c.rs holds the test\nOwner: the meter has a test\n";
    std::fs::write(repo.wt.join("why.md"), second).unwrap();
    set_env("DROVER_LANE", "sendmeter:12:impl");
    let r = pr(&head);
    let (code, out) = run(&c, &Git::new(&r), &["turn", "done"]);
    assert_eq!(code, 0, "{out}");
    assert!(origin_note(&repo, &head).is_some(), "{out}");
    let v = json!({"verdict": "PASS", "sha": head});
    append(&c.state, &Event::new(40, "sendmeter", Some(12), "verdict", v)).unwrap();
    g(&seed, &["checkout", "-q", "staging"]);
    g(&seed, &["merge", "-q", "--squash", "issue-12"]);
    g(&seed, &["commit", "-qm", "meter: count sends (#12) (#100)"]);
    g(&seed, &["push", "-q", "origin", "staging"]);
    let squash = g(&seed, &["rev-parse", "HEAD"]);
    let r = pr(&head)
        .reply("gh pr merge 100", "")
        .reply("gh pr view 100", &json!({"mergeCommit": {"oid": squash}}).to_string())
        .reply("gh issue", "");
    let (code, out) = run(&c, &Git::new(&r), &["merge", "sendmeter", "12"]);
    assert_eq!(code, 0, "{out}");
    assert!(r.called(&format!(
        "gh pr merge 100 -R jirathip-dev/sendmeter --squash --delete-branch --match-head-commit {head}"
    )));
    let note = origin_note(&repo, &squash).expect("a note on the squash commit");
    assert_eq!(note.trim(), format!("{}\n\n{}", NOTE.trim(), second.trim()));
    let look = repo.dir.join("look");
    g(
        &repo.dir,
        &["clone", "-q", repo.origin.to_str().unwrap(), look.to_str().unwrap()],
    );
    assert_eq!(g(&look, &["rev-list", "--count", "origin/staging"]), "2");
    let at = format!("{}:2", look.join("a.rs").display());
    let (code, out) = run(&c, &Git::new(&r), &["why", &at]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(&format!("commit  {squash:.9}  meter: count sends (#12) (#100)")),
        "{out}"
    );
    assert!(
        out.contains("issue   #12 Meter sends") && out.contains("PR #100 · merged"),
        "{out}"
    );
    assert!(out.contains(&format!("on {squash:.9}")), "{out}");
    assert!(
        out.contains("  Owner: retries no longer double the meter") && out.contains("  Owner: the meter has a test"),
        "{out}"
    );
}

#[test]
fn a_refused_note_is_nudged_once_then_review_goes_ahead() {
    let repo = repo("why-nudge");
    let (c, sh) = lane(&repo, false);
    let wt = repo.wt.to_string_lossy().into_owned();
    sh.lane("sendmeter-impl-12", "idle", &wt);
    let ci = json!([{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}]);
    let pr = json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": repo.head, "statusCheckRollup": ci, "body": body(&repo.head)});
    let r = Fake::default().reply("gh pr view issue-12", &pr.to_string()).reply(
        "gh issue view 12",
        &json!({"title": "Meter sends", "body": SPEC}).to_string(),
    );
    assert_eq!(run(&c, &Git::new(&r), &["turn", "done"]).0, 3);
    let later = |s| drover::Ctx {
        now: c.now + s,
        ..c.clone()
    };
    let (code, out) = run(&later(200), &Git::new(&r), &["drive"]);
    assert_eq!(code, 0, "{out}");
    let nudge = "drover: your `turn done` was refused: WHY_INVALID: why.md missing";
    let nudges = || {
        sh.sent("sendmeter-impl-12")
            .iter()
            .filter(|t| t.starts_with(nudge))
            .count()
    };
    assert_eq!(nudges(), 1, "{:?}", sh.all("send"));
    assert!(kinds(&c, "pushed").is_empty(), "no review before the nudge is answered");
    assert_eq!(run(&later(300), &Git::new(&r), &["turn", "done"]).0, 3);
    run(&later(400), &Git::new(&r), &["drive"]);
    assert_eq!(nudges(), 1);
    assert_eq!(kinds(&c, "pushed").len(), 1, "{:?}", r.calls.borrow());
}

#[test]
fn a_pr_body_without_the_heads_evidence_is_refused_before_review() {
    let repo = repo("why-pr-body");
    let base = g(&repo.wt, &["rev-parse", "HEAD^1"]);
    let stale = format!("## For humans\n\nx\n\n## Evidence (head {base})\n\ncargo test: exit 0");
    let cases = [
        (stale, "names no sha of head"),
        (
            "## For humans\n\nx\n\n## Evidence (head SHA_PLACEHOLDER)\n\nWEEK_PLACEHOLDER".into(),
            "names no sha of head",
        ),
        (
            format!("## What changed\n\nx\n\n## Evidence (head {})", repo.head),
            "no `## For humans` section",
        ),
        (
            format!(
                "{}\n\n🤖 Generated with [Claude Code](https://claude.com/claude-code)",
                body(&repo.head)
            ),
            "carries `🤖 Generated with [Claude Code]",
        ),
    ];
    let (c, _sh) = lane(&repo, false);
    std::fs::write(repo.wt.join("why.md"), NOTE).unwrap();
    for (b, why) in cases {
        let r = fake_body(&repo.head, &b);
        let (code, out) = run(&c, &Git::new(&r), &["turn", "done"]);
        assert_eq!(code, 3, "{why}: {out}");
        assert!(out.contains("REFUSED PR_BODY: ") && out.contains(why), "{why}: {out}");
        assert!(!r.called("gh pr edit") && kinds(&c, "why").is_empty(), "{why}");
        let t = kinds(&c, "turn-end").pop().unwrap();
        assert!(
            t.str("note").unwrap().starts_with("PR_BODY: "),
            "{why}: the turn still ends"
        );
    }
    let merged = "Merge remote-tracking branch 'origin/staging' into issue-12";
    g(&repo.wt, &["commit", "-q", "--allow-empty", "-m", merged]);
    let top = g(&repo.wt, &["rev-parse", "HEAD"]);
    let (code, out) = run(&c, &Git::new(&fake_body(&top, &body(&repo.head))), &["turn", "done"]);
    assert_eq!(code, 0, "a staging merge over the named head passes: {out}");
}

struct Ui<'a> {
    git: Git<'a>,
    bin: PathBuf,
    hang: bool,
    limits: std::sync::Mutex<Vec<std::time::Duration>>,
}

impl Runner for Ui<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.git.run_in(prog, args, "")
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.git.run_in(prog, args, input)
    }
    fn run_for(&self, prog: &str, args: &[String], input: &str, limit: std::time::Duration) -> anyhow::Result<Out> {
        if prog != "sh" {
            return self.git.run_in(prog, args, input);
        }
        self.limits.lock().unwrap().push(limit);
        if self.hang {
            return Ok(Out {
                code: 124,
                stderr: format!("sh: no answer in {limit:?}"),
                ..Out::default()
            });
        }
        let path = format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap());
        let o = Command::new(prog).args(args).env("PATH", path).output()?;
        Ok(Out {
            code: o.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&o.stdout).into(),
            stderr: String::from_utf8_lossy(&o.stderr).into(),
        })
    }
}

fn ui_lane(name: &str, check: Option<(i32, Value)>) -> (Repo, drover::Ctx, FakeShed, PathBuf) {
    let repo = repo(name);
    let (c, sh) = lane(&repo, false);
    let cfg = std::fs::read_to_string(&c.config).unwrap();
    let cfg = cfg.replace(
        "integration = \"staging\"",
        "integration = \"staging\"\nui_paths = [\"web/**\"]",
    );
    std::fs::write(&c.config, cfg).unwrap();
    std::fs::write(repo.wt.join("why.md"), NOTE).unwrap();
    let bin = repo.dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    if let Some((code, outcome)) = check {
        std::fs::create_dir_all(repo.wt.join(".blocks")).unwrap();
        std::fs::write(
            repo.wt.join(".blocks/guard.json"),
            r#"{"screens": ["fixtures/screens"]}"#,
        )
        .unwrap();
        just(&repo, code, &outcome);
    }
    (repo, c, sh, bin)
}

fn just(repo: &Repo, code: i32, outcome: &Value) {
    let out = if outcome.is_null() {
        String::new()
    } else {
        outcome.to_string()
    };
    let log = repo.dir.join("just.log");
    let script = format!(
        "#!/bin/sh\necho \"$* $INTEGRATION $PWD\" >> {}\nprintf '%s' '{out}' > \"$GUARD_OUT\"\nexit {code}\n",
        log.display()
    );
    let path = repo.dir.join("bin/just");
    std::fs::write(&path, script).unwrap();
    let mut p = std::fs::metadata(&path).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut p, 0o755);
    std::fs::set_permissions(&path, p).unwrap();
}

fn ui_fake(head: &str, touched: &str) -> Fake {
    fake(head).reply("gh api repos/jirathip-dev/sendmeter/pulls/100/files", touched)
}

fn finding(rule: &str) -> Value {
    json!({"rule": rule, "severity": "error", "file": "fixtures/screens/queue.one.json", "at": "screen#queue/list#rows",
        "line": 4, "message": "the title is over 60 characters: 74 characters", "fix": "Shorten the title to 60 characters."})
}

fn outcome(new: &[Value], known: usize) -> Value {
    json!({"guard": 1, "pass": new.is_empty(), "new": new, "warned": [], "known": vec![finding("lint/old"); known],
        "ignored": [], "checked": ["fixtures/screens/queue.one.json"], "skipped": []})
}

fn ui<'a>(git: Git<'a>, bin: &Path, hang: bool) -> Ui<'a> {
    Ui {
        git,
        bin: bin.to_path_buf(),
        hang,
        limits: std::sync::Mutex::new(vec![]),
    }
}

#[test]
fn a_clean_ui_check_ends_the_turn_with_its_result() {
    for (name, known) in [("ui-clean", 0), ("ui-baseline", 3)] {
        let (repo, c, _sh, bin) = ui_lane(name, Some((0, outcome(&[], known))));
        let f = ui_fake(&repo.head, "web/board.html\n");
        let r = ui(Git::new(&f), &bin, false);
        let (code, out) = run(&c, &r, &["turn", "done"]);
        assert_eq!(code, 0, "{out}");
        let ran = std::fs::read_to_string(repo.dir.join("just.log")).unwrap();
        assert_eq!(
            ran.trim(),
            format!("guard staging {}", repo.wt.canonicalize().unwrap().display())
        );
        assert_eq!(*r.limits.lock().unwrap(), [std::time::Duration::from_secs(180)]);
        let t = kinds(&c, "turn-end").pop().unwrap();
        assert_eq!(t.val("ui_check")["result"], "clean", "{name}");
        assert_eq!(t.val("ui_check")["known"], known, "{name}");
        assert_eq!(kinds(&c, "ui-check").len(), 1);
        assert_eq!(kinds(&c, "why").len(), 1, "the why note still goes on");
    }
}

#[test]
fn a_new_finding_refuses_the_turn_once_then_review_takes_it() {
    let (repo, c, _sh, bin) = ui_lane("ui-new", Some((1, outcome(&[finding("lint/title-length")], 2))));
    let f = ui_fake(&repo.head, "fixtures/screens/queue.one.json\n");
    let (code, out) = run(&c, &ui(Git::new(&f), &bin, false), &["turn", "done"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED UI_CHECK: 1 new finding(s)"), "{out}");
    let line = "lint/title-length fixtures/screens/queue.one.json:4 screen#queue/list#rows: the title is over 60 characters: 74 characters\n  fix: Shorten the title to 60 characters.";
    assert!(out.contains(line), "{out}");
    assert!(kinds(&c, "turn-end").is_empty(), "a refused turn logs no turn-end");
    assert!(kinds(&c, "why").is_empty() && repo.wt.join("why.md").exists());
    let run1 = kinds(&c, "ui-check").pop().unwrap();
    assert_eq!((run1.val("new"), run1.val("refused")), (&json!(1), &json!(true)));
    assert!(run1.val("secs").is_u64());

    let (code, out) = run(&c, &ui(Git::new(&f), &bin, false), &["turn", "done"]);
    assert_eq!(code, 0, "the same findings again go to review: {out}");
    let t = kinds(&c, "turn-end").pop().unwrap();
    assert_eq!(t.val("ui_check")["result"], "new");
    let ev = read(&c.state).unwrap().to_vec();
    let brief = drover::guard::brief(&ev, ("sendmeter", 12)).unwrap();
    assert!(brief.contains(line), "rev's brief carries them: {brief}");
}

#[test]
fn a_different_finding_after_a_refusal_is_refused_again() {
    let (repo, c, _sh, bin) = ui_lane("ui-other", Some((1, outcome(&[finding("lint/title-length")], 0))));
    let f = ui_fake(&repo.head, "web/board.html\n");
    assert_eq!(run(&c, &ui(Git::new(&f), &bin, false), &["turn", "done"]).0, 3);
    just(&repo, 1, &outcome(&[finding("lint/raw-ui")], 0));
    let (code, out) = run(&c, &ui(Git::new(&f), &bin, false), &["turn", "done"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("lint/raw-ui"), "{out}");
    assert!(kinds(&c, "turn-end").is_empty());
}

#[test]
fn a_timed_out_or_broken_check_never_blocks_the_turn() {
    for (name, hang, code, result) in [("ui-timeout", true, 0, "timeout"), ("ui-broken", false, 2, "broken")] {
        let (repo, c, _sh, bin) = ui_lane(name, Some((code, Value::Null)));
        let f = ui_fake(&repo.head, "web/board.html\n");
        let (code, out) = run(&c, &ui(Git::new(&f), &bin, hang), &["turn", "done"]);
        assert_eq!(code, 0, "{name}: {out}");
        let t = kinds(&c, "turn-end").pop().unwrap();
        assert_eq!(t.val("ui_check")["result"], result, "{name}");
        assert_eq!(kinds(&c, "ui-check").pop().unwrap().val("result"), result);
    }
}

#[test]
fn no_check_runs_off_ui_paths_or_without_the_setup() {
    for (name, setup, touched) in [
        ("ui-off-path", true, "a.rs\n"),
        ("ui-no-setup", false, "web/board.html\n"),
    ] {
        let check = setup.then(|| (1, outcome(&[finding("lint/title-length")], 0)));
        let (repo, c, _sh, bin) = ui_lane(name, check);
        let f = ui_fake(&repo.head, touched);
        let r = ui(Git::new(&f), &bin, false);
        let (code, out) = run(&c, &r, &["turn", "done"]);
        assert_eq!(code, 0, "{name}: {out}");
        assert!(r.limits.lock().unwrap().is_empty(), "{name}: the check never ran");
        assert!(!repo.dir.join("just.log").exists(), "{name}");
        assert!(kinds(&c, "turn-end").pop().unwrap().data.get("ui_check").is_none());
    }
}
