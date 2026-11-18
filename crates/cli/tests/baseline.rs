mod common;
use common::*;
use drover::events::read;
use drover::runner::{argv, Out, Real, Runner};
use std::path::{Path, PathBuf};

struct RealGit(Fake);
impl Runner for RealGit {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        if prog == "git" {
            return Real.run(prog, args);
        }
        self.0.run(prog, args)
    }
    fn sleep(&self, _: u64) {}
}

fn sh(dir: &Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = sh(dir, args);
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn commit(dir: &Path, name: &str) -> String {
    std::fs::write(dir.join(name), name).unwrap();
    git(dir, &["add", name]);
    git(dir, &["commit", "-qm", name]);
    git(dir, &["rev-parse", "HEAD"])
}

fn lines(dir: &Path, args: &[&str]) -> usize {
    git(dir, args).lines().count()
}

fn since(wt: &Path, t: &str) -> usize {
    let not = format!("^{t}");
    let out = git(
        wt,
        &[
            "--no-replace-objects",
            "rev-list",
            "--boundary",
            "HEAD",
            "origin/staging",
            &not,
        ],
    );
    let mut seen: std::collections::BTreeSet<&str> = out.lines().map(|l| l.trim_start_matches('-')).collect();
    seen.insert(t);
    seen.len()
}

fn fake() -> RealGit {
    RealGit(
        Fake::default()
            .reply("which shed", "/opt/shed/bin/shed\n")
            .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
            .reply("cp ", ""),
    )
}

fn spawn(c: &drover::Ctx, n: u64) -> (String, PathBuf) {
    let (code, out) = run(c, &fake(), &["spawn", "sendmeter", &n.to_string(), "impl"]);
    assert_eq!(code, 0, "{out}");
    (out, c.worktrees.join(format!("sendmeter/issue-{n}")))
}

#[test]
fn lanes_see_history_since_the_baseline_and_drover_and_older_branches_see_all_of_it() {
    let d = tmpdir("baseline");
    let (origin, local) = (d.join("origin.git"), d.join("local"));
    git(&d, &["init", "-q", "--bare", "-b", "staging", origin.to_str().unwrap()]);
    git(&d, &["clone", "-q", origin.to_str().unwrap(), local.to_str().unwrap()]);
    git(&local, &["config", "user.email", "t@example.com"]);
    git(&local, &["config", "user.name", "t"]);
    let old = commit(&local, "a");
    commit(&local, "b");
    git(&local, &["checkout", "-qb", "oldbr"]);
    let side = commit(&local, "o");
    git(&local, &["checkout", "-q", "-"]);
    commit(&local, "c");
    let t = commit(&local, "d");
    git(&local, &["push", "-q", "origin", "HEAD:staging", "oldbr"]);
    git(&local, &["branch", "issue-13", "oldbr"]);
    let c = ctx(
        &d,
        &CONFIG.replace("/tmp/does-not-matter/sendmeter", local.to_str().unwrap()),
    );
    let _sh = FakeShed::attached(&c);

    let (code, out) = run(&c, &fake(), &["baseline", "sendmeter", &side]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED BASELINE_OFF_INTEGRATION"), "{out}");
    let (code, out) = run(&c, &fake(), &["baseline", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&format!("none -> {}", &t[..12])), "{out}");
    assert!(std::fs::read_to_string(&c.config)
        .unwrap()
        .contains(&format!("baseline = \"{t}\"")));
    assert_eq!(
        read(&c.state)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "baseline-set")
            .count(),
        1
    );

    git(&local, &["merge", "-q", "--no-ff", "-m", "merge oldbr", "oldbr"]);
    commit(&local, "e");
    git(&local, &["push", "-q", "origin", "HEAD:staging"]);
    let all = lines(&local, &["rev-list", "HEAD"]);

    let (out, wt) = spawn(&c, 12);
    assert!(out.contains(&format!("history: since {}", &t[..12])), "{out}");
    let brief = std::fs::read_to_string(wt.join(".brief.md")).unwrap();
    assert!(
        brief.contains("History: `git log` and `git blame` here start at"),
        "{brief}"
    );
    let seen = lines(&wt, &["log", "--oneline"]);
    assert_eq!(seen, since(&wt, &t), "only the commits since the baseline");
    assert!(seen < all, "{seen} of {all}");
    assert!(
        sh(&wt, &["show", "--stat", &old]).status.success(),
        "an old sha still opens"
    );
    assert_eq!(
        lines(&local, &["log", "--oneline"]),
        all,
        "the owner's checkout sees everything"
    );
    let through = Real
        .run("git", &argv(&["-C", wt.to_str().unwrap(), "rev-list", "HEAD"]))
        .unwrap();
    assert_eq!(through.stdout.lines().count(), all, "drover's own git is graft-blind");
    commit(&wt, "f");
    let base = |extra: &[&str]| git(&wt, &[extra, &["merge-base", "HEAD", "origin/staging"][..]].concat());
    assert_eq!(base(&[]), base(&["--no-replace-objects"]), "merge-base is unchanged");

    let (out, w13) = spawn(&c, 13);
    assert!(
        out.contains("history: full (issue-13: HEAD was cut before the baseline"),
        "{out}"
    );
    assert_eq!(
        lines(&w13, &["log", "--oneline"]),
        lines(&w13, &["--no-replace-objects", "rev-list", "HEAD"])
    );
    git(&w13, &["config", "user.email", "t@example.com"]);
    git(&w13, &["config", "user.name", "t"]);
    git(&w13, &["merge", "-q", "-m", "update", "origin/staging"]);

    let tip = git(&local, &["rev-parse", "origin/staging"]);
    let (code, out) = run(&c, &fake(), &["baseline", "sendmeter", &tip]);
    assert_eq!(code, 0, "{out}");
    let (out, w14) = spawn(&c, 14);
    assert!(out.contains(&format!("history: since {}", &tip[..12])), "{out}");
    assert_eq!(lines(&w14, &["log", "--oneline"]), since(&w14, &tip));
    assert_eq!(
        lines(&wt, &["log", "--oneline"]),
        all + 1,
        "a re-baseline turns the old views off"
    );

    let cfg = std::fs::read_to_string(&c.config).unwrap();
    let cfg: String = cfg
        .lines()
        .filter(|l| !l.starts_with("baseline = "))
        .map(|l| format!("{l}\n"))
        .collect();
    std::fs::write(&c.config, cfg).unwrap();
    let (out, _) = spawn(&c, 15);
    assert!(!out.contains("history:"), "{out}");
    assert_eq!(git(&local, &["replace", "-l"]), "", "the key removed drops every graft");
    assert_eq!(lines(&w14, &["log", "--oneline"]), all);
}
