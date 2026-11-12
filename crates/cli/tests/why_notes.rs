//! #677: why notes. `turn done` checks the lane's why.md and puts it on the pushed head under `refs/notes/why` on a
//! real (bare) origin, the PR body and the log; refusals push nothing; concurrent writers lose no note; `drover why`
//! reads a merged line back; every brief kind carries its notes. Every test runs as #12's impl lane (process-wide env).
mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::runner::{Out, Runner};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Real `git` (each call logged in the Fake too); everything else is the Fake's. `before_push` runs once, right
/// before the first push of a notes ref: another writer getting in between this one's fetch and push.
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
        use std::io::Write;
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

/// `git -C dir args` in a fixture, its stdout; it must succeed.
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

/// A bare origin with `a.rs` (3 lines) on staging and #12's branch one commit on; the fleet's local clone; the lane's
/// worktree on the branch.
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

/// #12's impl lane in `repo`'s worktree (spawned at ts 10, `update` for an update round), and q5 answered by the owner.
fn lane(repo: &Repo, update: bool) -> drover::Ctx {
    std::env::set_var("DROVER_LANE", "sendmeter:12:impl");
    let c = ctx(
        &repo.dir,
        &CONFIG.replace("/tmp/does-not-matter/sendmeter", repo.local.to_str().unwrap()),
    );
    let ev = |ts, kind: &str, d: Value| append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
    ev(
        10,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "w9:p1", "worktree": repo.wt, "branch": "issue-12", "update": update}),
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
    c
}

/// The Fake around git: #12's open PR at `head`, its spec, and the writes it allows.
fn fake(head: &str) -> Fake {
    let pr = json!({"number": 100, "state": "OPEN", "headRefOid": head, "body": "Closes #12\n\n## For humans\n\nx"});
    Fake::default()
        .reply("gh pr view issue-12", &pr.to_string())
        .reply(
            "gh issue view 12",
            &json!({"title": "Meter sends", "body": SPEC}).to_string(),
        )
        .reply("gh pr edit", "")
        .reply("herdr", "{}")
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
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

#[test]
fn why_note_written_on_origin_pr_body_and_log() {
    let repo = repo("why-written");
    let c = lane(&repo, false);
    std::fs::write(repo.wt.join("why.md"), NOTE).unwrap();
    let r = fake(&repo.head);
    let (code, out) = run(&c, &Git::new(&r), &["turn", "done"]);
    assert_eq!(code, 0, "{out}");
    // 1. the note on the head, on origin.
    assert_eq!(origin_note(&repo, &repo.head).as_deref(), Some(NOTE));
    // 2. the PR body keeps its own text and gains the round's section.
    let calls = r.calls.borrow();
    let edit = calls
        .iter()
        .find(|c| c.starts_with("gh pr edit 100 -R jirathip-dev/sendmeter --body"))
        .unwrap();
    assert!(edit.contains("Closes #12") && edit.contains("## For humans"), "{edit}");
    assert!(edit.contains("## Why (round 1)\n\nDecided: count a retry"), "{edit}");
    // 3. the `why` event: sha, round, the Owner line, the files since the base.
    let w = &kinds(&c, "why")[0];
    assert_eq!(
        (w.issue, w.str("sha"), w.data["round"].as_u64()),
        (Some(12), Some(repo.head.as_str()), Some(1))
    );
    assert_eq!(w.str("owner"), Some("retries no longer double the meter"));
    assert_eq!(w.data["files"], json!(["a.rs"]));
    // 4. the turn is logged without a refusal and why.md is consumed.
    assert!(kinds(&c, "turn-end")[0].data.get("note").is_none());
    assert!(!repo.wt.join("why.md").exists());
}

#[test]
fn an_update_rounds_short_note_is_accepted() {
    let repo = repo("why-short");
    let c = lane(&repo, true);
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
        let c = lane(&repo, false);
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

/// Another writer's note lands on origin between this write's fetch and push: the rejected push starts over and
/// both notes stay.
#[test]
fn a_note_pushed_between_fetch_and_push_is_kept() {
    let repo = repo("why-race-between");
    let c = lane(&repo, false);
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

/// Two lanes' notes written at once from one local clone onto one origin: both end up there.
#[test]
fn two_lanes_finishing_at_once_both_keep_their_notes() {
    let repo = repo("why-race-two");
    let c = lane(&repo, false);
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

/// A line from a merged PR: blame lands on a branch commit without a note; its round head's note, the issue and its
/// asks with answers come back, as text and `--json`.
#[test]
fn why_reads_a_merged_line_back() {
    let repo = repo("why-cmd");
    let c = lane(&repo, false);
    let seed = repo.dir.join("seed");
    // The round head: one more commit on the branch, noted by `turn done`.
    std::fs::write(seed.join("c.rs"), "c\n").unwrap();
    g(&seed, &["add", "."]);
    g(&seed, &["commit", "-qm", "meter: test it"]);
    g(&seed, &["push", "-q", "origin", "issue-12"]);
    let head = g(&seed, &["rev-parse", "HEAD"]);
    g(&repo.wt, &["pull", "-q", "origin", "issue-12"]);
    std::fs::write(repo.wt.join("why.md"), NOTE).unwrap();
    assert_eq!(run(&c, &Git::new(&fake(&head)), &["turn", "done"]).0, 0);
    // Merged with `--merge`.
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
    assert!(out.contains(&format!("note    round 1 · on {:.9}", head)), "{out}");
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

/// drive tells a refused note's lane once before its review; its next `done` goes to review without a note.
#[test]
fn a_refused_note_is_nudged_once_then_review_goes_ahead() {
    let repo = repo("why-nudge");
    let c = lane(&repo, false);
    let wt = repo.wt.to_string_lossy().into_owned();
    let list = agents(&[("sendmeter-impl-12", "idle", &wt, "w9:p1")]);
    let ci = json!([{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}]);
    let pr = json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": repo.head, "statusCheckRollup": ci});
    let r = Fake::default()
        .reply("herdr agent list", &list)
        .reply("gh pr view issue-12", &pr.to_string())
        .reply(
            "gh issue view 12",
            &json!({"title": "Meter sends", "body": SPEC}).to_string(),
        )
        .reply("herdr", "{}");
    assert_eq!(run(&c, &Git::new(&r), &["turn", "done"]).0, 3);
    let later = |s| drover::Ctx {
        now: c.now + s,
        ..c.clone()
    };
    let (code, out) = run(&later(200), &Git::new(&r), &["drive"]);
    assert_eq!(code, 0, "{out}");
    let nudge = "herdr agent prompt sendmeter-impl-12 drover: your why note was refused: WHY_INVALID: why.md missing";
    assert_eq!(r.count(nudge), 1, "{:?}", r.calls.borrow());
    assert!(kinds(&c, "pushed").is_empty(), "no review before the nudge is answered");
    // The lane answers with another `done`, still without a note: the review goes ahead, the nudge is not repeated.
    assert_eq!(run(&later(300), &Git::new(&r), &["turn", "done"]).0, 3);
    run(&later(400), &Git::new(&r), &["drive"]);
    assert_eq!(r.count(nudge), 1);
    assert_eq!(kinds(&c, "pushed").len(), 1, "{:?}", r.calls.borrow());
}
