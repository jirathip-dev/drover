//! #677: each brief kind carries its why notes, read with real `git notes` from the fleet's local clone: a fix, update
//! or respawned lane every note on its branch; a new impl the latest notes on the files its spec names; rev the round
//! head's note, or why there is none.
mod common;
use common::*;
use herdsman::events::{append, Event};
use herdsman::runner::{Out, Runner};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Command;

/// Real `git` for `git notes` (the clone's real notes); the Fake for every other call.
struct Notes<'a>(&'a Fake);

impl Runner for Notes<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        if prog != "git" || !args.iter().any(|a| a == "notes") {
            return self.0.run(prog, args);
        }
        let o = Command::new("git").args(args).output()?;
        Ok(Out {
            code: o.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&o.stdout).into(),
            stderr: String::from_utf8_lossy(&o.stderr).into(),
        })
    }
    fn run_in(&self, prog: &str, args: &[String], _: &str) -> anyhow::Result<Out> {
        self.run(prog, args)
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
}

fn g(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-C"])
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

/// A local clone with two noted commits: #12's round 1 head and #7's (which changed src/meter.rs).
fn setup(name: &str) -> (herdsman::Ctx, String, String) {
    let d = tmpdir(name);
    let local = d.join("local");
    g(&d, &["init", "-q", local.to_str().unwrap()]);
    let mut shas = vec![];
    for (msg, note) in [
        ("seven", "Owner: #7 made the meter monotonic"),
        ("twelve", "Owner: #12 counts retries once"),
    ] {
        g(&local, &["commit", "-q", "--allow-empty", "-m", msg]);
        let sha = g(&local, &["rev-parse", "HEAD"]);
        g(&local, &["notes", "--ref", "why", "add", "-m", note, &sha]);
        shas.push(sha);
    }
    let c = ctx(
        &d,
        &CONFIG.replace("/tmp/does-not-matter/sendmeter", local.to_str().unwrap()),
    );
    let why = |n, sha: &str, files: Value| {
        let d = json!({"sha": sha, "round": 1, "owner": "x", "files": files, "branch": format!("issue-{n}")});
        append(&c.state, &Event::new(20, "sendmeter", Some(n), "why", d)).unwrap();
    };
    why(7, &shas[0], json!(["src/meter.rs", "README.md"]));
    (c, shas.remove(0), shas.remove(0))
}

fn fake(rev_head: &str, wt: &Path) -> Fake {
    Fake::default()
        .reply(
            "gh issue view 12",
            r#"{"title":"Meter retries","body":"Retries double the count in `src/meter.rs`."}"#,
        )
        .reply(
            &format!("git -C {} rev-parse HEAD", wt.display()),
            &format!("{rev_head}\n"),
        )
        .reply("git -C", "0\n")
        .reply("which shed", "/opt/shed/bin/shed\n")
}

/// Spawn `args` (after `spawn sendmeter 12`) on a fake shed (#771: no lane live before it) and read the lane's brief.
fn brief(c: &herdsman::Ctx, head: &str, args: &[&str]) -> String {
    let sh = FakeShed::new();
    sh.attach(c);
    let dir = if args[0] == "rev" { "rev-12-r1" } else { "issue-12" };
    let wt = c.worktrees.join("sendmeter").join(dir);
    let f = fake(head, &wt);
    let (code, out) = run(c, &Notes(&f), &[&["spawn", "sendmeter", "12"][..], args].concat());
    assert!(code == 0 || code == 4, "{args:?}: {code} {out}");
    std::fs::read_to_string(wt.join(".brief.md")).unwrap()
}

fn noted_round(c: &herdsman::Ctx, sha: &str) {
    let d = json!({"sha": sha, "round": 1, "owner": "#12 counts retries once", "files": ["src/meter.rs"]});
    append(&c.state, &Event::new(30, "sendmeter", Some(12), "why", d)).unwrap();
}

#[test]
fn a_new_impl_brief_carries_the_notes_on_the_files_its_spec_names() {
    let (c, seven, _) = setup("why-brief-impl");
    let b = brief(&c, "", &["impl"]);
    assert!(
        b.contains("## Why notes for the files this issue names\n\n### src/meter.rs"),
        "{b}"
    );
    assert!(
        b.contains(&format!("#7 · {seven:.7}\nOwner: #7 made the meter monotonic")),
        "{b}"
    );
    assert!(!b.contains("README.md"), "only the files the spec names: {b}");
}

#[test]
fn a_fix_brief_carries_its_branchs_notes() {
    let (c, _, twelve) = setup("why-brief-fix");
    noted_round(&c, &twelve);
    let d = json!({"verdict": "FAIL", "sha": twelve, "body": "FAIL\n\n- blocking [x] a.rs:1: off"});
    append(&c.state, &Event::new(40, "sendmeter", Some(12), "verdict", d)).unwrap();
    let b = brief(&c, "", &["impl"]);
    assert!(b.contains("## Review findings to fix"), "{b}");
    assert!(
        b.contains(&format!(
            "## Why notes on this branch\n\nround 1 · {twelve:.7}\nOwner: #12 counts retries once"
        )),
        "{b}"
    );
    assert!(!b.contains("#7 made"), "{b}");
}

#[test]
fn an_update_brief_carries_its_branchs_notes() {
    let (c, _, twelve) = setup("why-brief-update");
    noted_round(&c, &twelve);
    let b = brief(&c, "", &["impl", "--update"]);
    assert!(b.contains("## Update onto staging"), "{b}");
    assert!(
        b.contains(&format!("round 1 · {twelve:.7}\nOwner: #12 counts retries once")),
        "{b}"
    );
}

#[test]
fn a_respawned_lanes_brief_carries_its_branchs_notes() {
    let (c, _, twelve) = setup("why-brief-respawn");
    let wt = c.worktrees.join("sendmeter/issue-12");
    let d =
        json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "w8:p1", "worktree": wt, "branch": "issue-12"});
    append(&c.state, &Event::new(10, "sendmeter", Some(12), "spawn", d)).unwrap();
    noted_round(&c, &twelve);
    let b = brief(&c, "", &["impl", "--supersede"]);
    assert!(
        b.contains(&format!(
            "## Why notes on this branch\n\nround 1 · {twelve:.7}\nOwner: #12 counts retries once"
        )),
        "{b}"
    );
}

#[test]
fn a_rev_brief_carries_the_rounds_note_or_why_there_is_none() {
    let (c, _, twelve) = setup("why-brief-rev");
    noted_round(&c, &twelve);
    let b = brief(&c, &twelve, &["rev"]);
    assert!(
        b.contains(&format!(
            "## Why note for this round ({twelve:.7})\n\nOwner: #12 counts retries once"
        )),
        "{b}"
    );
    assert!(
        b.contains("a why note that contradicts the diff is a blocking finding"),
        "{b}"
    );
    // Another head, its `done` refused: the brief says so.
    let (c, seven, _) = setup("why-brief-rev-none");
    let d = json!({"state": "done", "role": "impl", "note": "WHY_INVALID: why.md missing"});
    append(&c.state, &Event::new(30, "sendmeter", Some(12), "turn-end", d)).unwrap();
    let b = brief(&c, &seven, &["rev"]);
    assert!(
        b.contains("## Why note for this round\n\nno why note: WHY_INVALID: why.md missing"),
        "{b}"
    );
}
