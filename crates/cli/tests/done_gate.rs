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
}

impl<'a> Git<'a> {
    fn new(fake: &'a Fake) -> Self {
        Git { fake }
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

fn lane(repo: &Repo) -> (drover::Ctx, FakeShed) {
    set_env("DROVER_LANE", "sendmeter:12:impl");
    let c = ctx(
        &repo.dir,
        &CONFIG.replace("/tmp/does-not-matter/sendmeter", repo.local.to_str().unwrap()),
    );
    let ev = |ts, kind: &str, d: Value| append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
    ev(
        10,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12", "worktree": repo.wt, "branch": "issue-12"}),
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
    Fake::default().reply("gh pr view issue-12", &pr.to_string()).reply(
        "gh issue view 12",
        &json!({"title": "Meter sends", "body": SPEC}).to_string(),
    )
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
fn why_reads_a_merged_line_back_with_its_issue_and_asks_and_no_note() {
    let repo = repo("why-cmd");
    let (c, _sh) = lane(&repo);
    let seed = repo.dir.join("seed");
    let head = repo.head.clone();
    g(&seed, &["checkout", "-q", "staging"]);
    g(
        &seed,
        &["merge", "-q", "--no-ff", "-m", "Merge pull request #100", "issue-12"],
    );
    g(&seed, &["push", "-q", "origin", "staging"]);
    let merge = g(&seed, &["rev-parse", "HEAD"]);
    let d = json!({"pr": "100", "merge_sha": merge, "verdict_sha": head, "into": "staging"});
    append(&c.state, &Event::new(50, "sendmeter", Some(12), "merged", d)).unwrap();
    let old = json!({"sha": head, "round": 1, "owner": "an old note"});
    append(&c.state, &Event::new(30, "sendmeter", Some(12), "why", old)).unwrap();
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
    assert!(!out.contains("note") && !out.contains("an old note"), "{out}");
    assert!(
        out.contains("q5 owner · Count retries?") && out.contains("→ owner (web): yes, each once"),
        "{out}"
    );
    assert!(
        !r.calls.borrow().iter().any(|c| c.contains("notes")),
        "{:?}",
        r.calls.borrow()
    );
    let (code, out) = run(&c, &Git::new(&r), &["why", &at, "--json"]);
    assert_eq!(code, 0, "{out}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        (v["issue"]["number"].as_u64(), v["issue"]["merge_sha"].as_str()),
        (Some(12), Some(merge.as_str()))
    );
    assert!(v.get("note").is_none(), "{v}");
    assert_eq!(v["asks"][0]["answer"]["choice"], json!("yes, each once"));
}

fn refused(c: &drover::Ctx, r: &dyn Runner, code: &str) -> (String, Event) {
    let (exit, out) = run(c, r, &["turn", "done"]);
    assert_eq!(exit, 3, "{out}");
    assert!(out.contains(&format!("REFUSED {code}: ")), "{out}");
    assert!(
        kinds(c, "turn-end").is_empty(),
        "a refused done logs no turn-end: {out}"
    );
    (out, kinds(c, "done-refused").pop().unwrap())
}

#[test]
fn a_pr_body_without_the_heads_evidence_is_refused_and_ends_no_turn() {
    let repo = repo("done-pr-body");
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
    let (c, _sh) = lane(&repo);
    for (b, why) in cases {
        let r = fake_body(&repo.head, &b);
        let (out, e) = refused(&c, &Git::new(&r), "PR_BODY");
        assert!(out.contains(why), "{why}: {out}");
        assert_eq!(
            (e.str("code"), e.str("sha")),
            (Some("PR_BODY"), Some(repo.head.as_str()))
        );
        assert!(e.str("why").unwrap().contains(why), "{why}");
    }
    let merged = "Merge remote-tracking branch 'origin/staging' into issue-12";
    g(&repo.wt, &["commit", "-q", "--allow-empty", "-m", merged]);
    let top = g(&repo.wt, &["rev-parse", "HEAD"]);
    let (code, out) = run(&c, &Git::new(&fake_body(&top, &body(&repo.head))), &["turn", "done"]);
    assert_eq!(code, 0, "a staging merge over the named head passes: {out}");
    assert_eq!(kinds(&c, "turn-end").pop().unwrap().str("sha"), Some(top.as_str()));
}

#[test]
fn pr_body_applies_the_done_rule_to_a_file_with_no_fleet_config() {
    let repo = repo("pr-body-verb");
    let file = repo.dir.join("body.md");
    let verb = |b: &str, head: &str| {
        std::fs::write(&file, b).unwrap();
        let o = Command::new(env!("CARGO_BIN_EXE_drover"))
            .args(["pr-body", "--file", file.to_str().unwrap(), "--head", head])
            .env("HOME", repo.dir.join("home"))
            .env("FLEET_CONFIG", repo.dir.join("none.toml"))
            .current_dir(&repo.wt)
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
        (o.status.code().unwrap(), out)
    };
    assert_eq!(verb(&body(&repo.head), &repo.head), (0, String::new()));
    let base = g(&repo.wt, &["rev-parse", "HEAD^1"]);
    for (b, why) in [
        (body(&base), "names no sha of head"),
        (
            format!("## Evidence (head {})", repo.head),
            "no `## For humans` section",
        ),
        (
            format!("{}\n\nGenerated with x", body(&repo.head)),
            "carries `Generated with x`",
        ),
    ] {
        let (code, out) = verb(&b, &repo.head);
        assert_eq!(code, 1, "{why}: {out}");
        assert!(out.contains("PR_BODY: ") && out.contains(why), "{why}: {out}");
    }
    let merged = "Merge remote-tracking branch 'origin/staging' into issue-12";
    g(&repo.wt, &["commit", "-q", "--allow-empty", "-m", merged]);
    let top = g(&repo.wt, &["rev-parse", "HEAD"]);
    assert_eq!(verb(&body(&repo.head), &top).0, 0);
    assert_eq!(verb(&body(&repo.head), "nothex").0, 1);
}

#[test]
fn an_unpushed_commit_is_refused_push_first() {
    let repo = repo("done-unpushed");
    let (c, _sh) = lane(&repo);
    g(&repo.wt, &["commit", "-q", "--allow-empty", "-m", "local only"]);
    let r = fake(&repo.head);
    let (out, e) = refused(&c, &Git::new(&r), "DONE_CHECK");
    assert!(out.contains("push first"), "{out}");
    assert_eq!(e.str("sha"), Some(repo.head.as_str()));
}

fn tool(bin: &Path, name: &str, code: i32, say: &str) {
    let path = bin.join(name);
    let log = bin.join("ran.log");
    let script = format!(
        "#!/bin/sh\necho \"{name} $* $(pwd -P)\" >> {}\necho '{say}'\nexit {code}\n",
        log.display()
    );
    std::fs::write(&path, script).unwrap();
    let mut p = std::fs::metadata(&path).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut p, 0o755);
    std::fs::set_permissions(&path, p).unwrap();
}

fn checked(name: &str, (fmt, arch): (i32, i32)) -> (Repo, drover::Ctx, FakeShed, PathBuf) {
    let (repo, c, sh, bin) = ui_lane(name, None);
    std::fs::write(repo.wt.join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::create_dir_all(repo.wt.join(".drover")).unwrap();
    std::fs::write(repo.wt.join(".drover/arch.toml"), "").unwrap();
    tool(&bin, "cargo", fmt, "Diff in a.rs at line 2");
    tool(&bin, "drover", arch, "a.rs:2: a comment");
    (repo, c, sh, bin)
}

#[test]
fn a_failing_format_or_arch_check_is_refused_with_its_output_tail() {
    for (name, codes, failed) in [
        (
            "done-fmt",
            (1, 0),
            "`cargo fmt --check` failed (exit 1):\nDiff in a.rs at line 2",
        ),
        (
            "done-arch",
            (0, 1),
            "`drover arch-check` failed (exit 1):\na.rs:2: a comment",
        ),
    ] {
        let (repo, c, _sh, bin) = checked(name, codes);
        let f = fake(&repo.head);
        let (out, e) = refused(&c, &ui(Git::new(&f), &bin, false), "DONE_CHECK");
        assert!(out.contains(failed), "{name}: {out}");
        assert_eq!(e.str("code"), Some("DONE_CHECK"));
    }
}

#[test]
fn a_clean_done_runs_both_checks_in_the_worktree_and_logs_its_head() {
    let (repo, c, _sh, bin) = checked("done-clean", (0, 0));
    let f = fake(&repo.head);
    let r = ui(Git::new(&f), &bin, false);
    let (code, out) = run(&c, &r, &["turn", "done"]);
    assert_eq!(code, 0, "{out}");
    let wt = repo.wt.canonicalize().unwrap();
    let ran = std::fs::read_to_string(bin.join("ran.log")).unwrap();
    assert_eq!(
        ran,
        format!(
            "drover arch-check {}\ncargo fmt --check {}\n",
            wt.display(),
            wt.display()
        )
    );
    assert_eq!(*r.limits.lock().unwrap(), [std::time::Duration::from_mins(30); 2]);
    let t = kinds(&c, "turn-end").pop().unwrap();
    assert_eq!((t.str("state"), t.str("sha")), (Some("done"), Some(repo.head.as_str())));
    assert!(kinds(&c, "done-refused").is_empty());
}

#[test]
fn a_check_that_cannot_start_or_times_out_refuses_the_done() {
    let (repo, c, _sh, bin) = checked("done-missing", (0, 0));
    let cfg = std::fs::read_to_string(&c.config).unwrap();
    let cfg = cfg.replace(
        "integration = \"staging\"",
        "integration = \"staging\"\ndone_checks = [\"no-such-tool-1362 --check\"]",
    );
    std::fs::write(&c.config, cfg).unwrap();
    let f = fake(&repo.head);
    let (out, _) = refused(&c, &ui(Git::new(&f), &bin, false), "DONE_CHECK");
    assert!(out.contains("`no-such-tool-1362 --check` cannot start"), "{out}");
    assert!(!bin.join("ran.log").exists(), "the set list replaces the defaults");
    let (out, _) = refused(&c, &ui(Git::new(&f), &bin, true), "DONE_CHECK");
    assert!(
        out.contains("`no-such-tool-1362 --check` timed out after 30 min"),
        "{out}"
    );
    assert_eq!(kinds(&c, "done-refused").len(), 2);
}

fn gatekept(name: &str, code: i32) -> (Repo, drover::Ctx, FakeShed, PathBuf) {
    let (repo, c, sh, bin) = checked(name, (0, 0));
    let gk = repo.wt.join(".gatekeeper");
    std::fs::create_dir_all(&gk).unwrap();
    let ok = (1..=12).map(|i| format!("ok gate-{i}\n")).collect::<String>();
    let say = format!("{ok}FAIL fmt: a.rs:2 unformatted\nFAIL arch: a.rs:2 comment");
    tool(&gk, "run", code, &say);
    (repo, c, sh, bin)
}

#[test]
fn a_gatekeeper_wrapper_replaces_the_default_checks() {
    let (repo, c, _sh, bin) = gatekept("done-gk-clean", 0);
    let f = fake(&repo.head);
    let (code, out) = run(&c, &ui(Git::new(&f), &bin, false), &["turn", "done"]);
    assert_eq!(code, 0, "{out}");
    let wt = repo.wt.canonicalize().unwrap();
    let ran = std::fs::read_to_string(repo.wt.join(".gatekeeper/ran.log")).unwrap();
    let want = format!("run check --pr-body target/gatekeeper/pr-body.md {}\n", wt.display());
    assert_eq!(ran, want);
    assert!(!bin.join("ran.log").exists(), "arch-check and fmt do not run");
}

struct Far<'a> {
    fake: &'a Fake,
    home: PathBuf,
    sent: std::sync::Mutex<Vec<String>>,
}

impl Runner for Far<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.run_in(prog, args, "")
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        if prog != "ssh" {
            return self.fake.run_in(prog, args, input);
        }
        self.sent.lock().unwrap().push(args.join(" "));
        let mut c = Command::new("sh")
            .args(["-c", args.last().unwrap()])
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap())
            .env("HOME", &self.home)
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
    fn run_for(&self, prog: &str, args: &[String], input: &str, _: std::time::Duration) -> anyhow::Result<Out> {
        self.run_in(prog, args, input)
    }
}

fn files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .unwrap()
        .flat_map(|e| {
            let p = e.unwrap().path();
            if p.is_dir() {
                files(&p)
            } else {
                vec![p]
            }
        })
        .collect()
}

#[test]
fn a_remote_lane_with_no_fleet_config_passes_done_with_the_names_on_stdin_only() {
    let repo = repo("done-gk-far");
    let (c, _sh) = lane(&repo);
    let cfg = std::fs::read_to_string(&c.config).unwrap().replace(
        "integration = \"staging\"",
        "integration = \"staging\"\nprivate_names = [\"quillmoor\", \"north-pier\"]",
    );
    std::fs::write(&c.config, cfg).unwrap();
    let sp = json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12",
                    "worktree": repo.wt, "branch": "issue-12", "host": "far", "machine": "far"});
    append(&c.state, &Event::new(13, "sendmeter", Some(12), "spawn", sp)).unwrap();
    let gk = repo.wt.join(".gatekeeper");
    std::fs::create_dir_all(&gk).unwrap();
    let sum = |s: &str| {
        let o = Command::new("sh")
            .args(["-c", "printf %s \"$0\" | cksum", s])
            .output()
            .unwrap();
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    };
    let want = sum("quillmoor\nnorth-pier");
    let script = format!(
        "#!/bin/sh\n[ \"$(printf %s \"$DROVER_PRIVATE_NAMES\" | cksum)\" = '{want}' ] || {{ echo 'FAIL core/hygiene: list-missing'; exit 1; }}\n\
         grep -q '## For humans' \"$3\" || exit 1\necho \"$* $GATEKEEPER_HEAD\" > {}\n",
        gk.join("ran.log").display()
    );
    std::fs::write(gk.join("run"), script).unwrap();
    let mut p = std::fs::metadata(gk.join("run")).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut p, 0o755);
    std::fs::set_permissions(gk.join("run"), p).unwrap();
    let home = repo.dir.join("far-home");
    std::fs::create_dir_all(&home).unwrap();
    let f = fake(&repo.head);
    let r = Far {
        fake: &f,
        home,
        sent: std::sync::Mutex::default(),
    };
    let (code, out) = run(&c, &r, &["turn", "done"]);
    assert_eq!(code, 0, "{out}");
    let ran = std::fs::read_to_string(gk.join("ran.log")).unwrap();
    assert_eq!(
        ran,
        format!("check --pr-body target/gatekeeper/pr-body.md {}\n", repo.head)
    );
    let t = kinds(&c, "turn-end").pop().unwrap();
    assert_eq!(t.str("sha"), Some(repo.head.as_str()));
    let sent = r.sent.lock().unwrap();
    assert!(sent.iter().any(|a| a.contains(".gatekeeper/run check")), "{sent:#?}");
    for name in ["quillmoor", "north-pier"] {
        assert!(!out.contains(name), "{out}");
        assert!(sent.iter().all(|a| !a.contains(name)), "{name} in an argument");
        assert!(f.calls.borrow().iter().all(|a| !a.contains(name)), "{name} in a call");
        for file in files(&repo.dir).into_iter().filter(|p| *p != c.config) {
            let text = String::from_utf8_lossy(&std::fs::read(&file).unwrap()).into_owned();
            assert!(!text.contains(name), "{name} in {}", file.display());
        }
    }
}

#[test]
fn a_failing_gatekeeper_run_is_refused_with_its_failing_gates() {
    let (repo, c, _sh, bin) = gatekept("done-gk-fail", 1);
    let f = fake(&repo.head);
    let (out, _) = refused(&c, &ui(Git::new(&f), &bin, false), "DONE_CHECK");
    assert!(out.contains("`./.gatekeeper/run check` failed (exit 1):"), "{out}");
    assert!(
        out.contains("FAIL fmt: a.rs:2 unformatted\nFAIL arch: a.rs:2 comment"),
        "{out}"
    );
}

#[test]
fn a_fleets_done_checks_win_over_the_gatekeeper_wrapper() {
    let (repo, c, _sh, bin) = gatekept("done-gk-set", 1);
    let cfg = std::fs::read_to_string(&c.config).unwrap();
    let cfg = cfg.replace(
        "integration = \"staging\"",
        "integration = \"staging\"\ndone_checks = [\"cargo fmt --check\"]",
    );
    std::fs::write(&c.config, cfg).unwrap();
    let f = fake(&repo.head);
    let (code, out) = run(&c, &ui(Git::new(&f), &bin, false), &["turn", "done"]);
    assert_eq!(code, 0, "{out}");
    assert!(!repo.wt.join(".gatekeeper/ran.log").exists());
    let ran = std::fs::read_to_string(bin.join("ran.log")).unwrap();
    assert!(ran.starts_with("cargo fmt --check "), "{ran}");
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
    let (c, sh) = lane(&repo);
    let cfg = std::fs::read_to_string(&c.config).unwrap();
    let cfg = cfg.replace(
        "integration = \"staging\"",
        "integration = \"staging\"\nui_paths = [\"web/**\"]",
    );
    std::fs::write(&c.config, cfg).unwrap();
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
        "#!/bin/sh\necho \"$* $INTEGRATION $(pwd -P)\" >> {}\nprintf '%s' '{out}' > \"$GUARD_OUT\"\nexit {code}\n",
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

fn routed_done(name: &str, runners: i32, checks: &str) -> (Repo, drover::Ctx, Fake, PathBuf) {
    let (repo, mut c, _sh, bin) = gatekept(name, 1);
    c.exe = env!("CARGO_BIN_EXE_drover").into();
    let cfg = std::fs::read_to_string(&c.config).unwrap().replace(
        "integration = \"staging\"",
        &format!("integration = \"staging\"\n{checks}"),
    );
    let route =
        "\n[fleet.sendmeter.heavy]\ncargo = [\"cargo test*\"]\n\n[fleet.sendmeter.remote.cargo]\ntag = \"linux\"\n";
    std::fs::write(&c.config, format!("{cfg}{route}")).unwrap();
    let script = format!(
        "#!/bin/sh\necho \"runners $* | $GATEKEEPER_HEAD\" >> {}\necho 'remote: FAIL gate-9'\nexit {runners}\n",
        bin.join("ran.log").display()
    );
    std::fs::write(bin.join("runners"), script).unwrap();
    let mut p = std::fs::metadata(bin.join("runners")).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut p, 0o755);
    std::fs::set_permissions(bin.join("runners"), p).unwrap();
    let f = fake(&repo.head);
    (repo, c, f, bin)
}

#[test]
fn a_routed_fleets_gatekeeper_done_check_runs_on_the_build_machine_with_its_exit_code() {
    let (repo, c, f, bin) = routed_done("done-gk-routed", 0, "");
    let (code, out) = run(&c, &ui(Git::new(&f), &bin, false), &["turn", "done"]);
    assert_eq!(code, 0, "{out} RAN {:?}", std::fs::read_to_string(bin.join("ran.log")));
    assert!(
        !repo.wt.join(".gatekeeper/ran.log").exists(),
        "the failing local wrapper never ran"
    );
    let ran = std::fs::read_to_string(bin.join("ran.log")).unwrap();
    let want = format!(
        "runners run --tag linux --dir {} --env DROVER_PRIVATE_NAMES --env GATEKEEPER_HEAD --env GATEKEEPER_PR_BODY -- bash -c mkdir -p target/gatekeeper && printf %s \"$GATEKEEPER_PR_BODY\" > target/gatekeeper/pr-body.md && ./.gatekeeper/run check --pr-body target/gatekeeper/pr-body.md | {}\n",
        repo.wt.display(),
        repo.head
    );
    assert_eq!(ran, want);
    let t = kinds(&c, "turn-end").pop().unwrap();
    assert_eq!(t.str("sha"), Some(repo.head.as_str()));

    let (repo, c, f, bin) = routed_done("done-gk-routed-fail", 3, "");
    let (out, _) = refused(&c, &ui(Git::new(&f), &bin, false), "DONE_CHECK");
    assert!(
        out.contains("`./.gatekeeper/run check` failed (exit 3):\nremote: FAIL gate-9"),
        "{out}"
    );
    assert!(!repo.wt.join(".gatekeeper/ran.log").exists());
}

#[test]
fn a_routed_fleets_done_checks_route_the_same_way_but_never_routed_ones() {
    let (repo, c, f, bin) = routed_done(
        "done-set-routed",
        4,
        "done_checks = [\"drover arch-check\", \"cargo test\"]",
    );
    let (out, _) = refused(&c, &ui(Git::new(&f), &bin, false), "DONE_CHECK");
    assert!(
        out.contains("`cargo test` failed (exit 4):\nremote: FAIL gate-9"),
        "{out}"
    );
    let wt = repo.wt.canonicalize().unwrap();
    let ran = std::fs::read_to_string(bin.join("ran.log")).unwrap();
    let want = format!(
        "drover arch-check {}\nrunners run --tag linux --dir {} -- bash -c cargo test | \n",
        wt.display(),
        repo.wt.display()
    );
    assert_eq!(ran, want);
}
