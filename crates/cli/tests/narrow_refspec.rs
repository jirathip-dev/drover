mod common;
use common::*;
use drover::events::{append, Event};
use drover::runner::{Out, Real, Runner};
use serde_json::json;
use std::path::{Path, PathBuf};

const NOW: u64 = 1_000_000;

struct Git<'a, R: Runner>(&'a R, &'a Fake, Vec<String>);

impl<R: Runner> Runner for Git<'_, R> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        let ours = |a: &String| self.2.iter().any(|d| a.starts_with(d.as_str()));
        if prog == "git" && args.len() > 1 && args[0] == "-C" && ours(&args[1]) {
            self.1.calls.borrow_mut().push(format!("git {}", args.join(" ")));
            return Real.run(prog, args);
        }
        self.0.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.0.run_in(prog, args, input)
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
}

fn git(dir: &Path, a: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(a)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {a:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

struct Repo {
    c: drover::Ctx,
    local: PathBuf,
    head: String,
}

fn repo(name: &str, hosts: &str) -> Repo {
    let d = tmpdir(name);
    let (origin, seed, local) = (d.join("origin.git"), d.join("seed"), d.join("local"));
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q", "--bare", "-b", "staging"]);
    git(&d, &["clone", "-q", &origin.to_string_lossy(), &seed.to_string_lossy()]);
    let id = ["-c", "user.name=t", "-c", "user.email=t@t"];
    git(&seed, &["checkout", "-q", "-b", "staging"]);
    std::fs::write(seed.join("README"), "x\n").unwrap();
    git(&seed, &["add", "README"]);
    git(&seed, &[&id[..], &["commit", "-q", "-m", "init"]].concat());
    git(&seed, &["push", "-q", "origin", "staging"]);
    git(
        &d,
        &[
            "clone",
            "-q",
            "--single-branch",
            "--branch",
            "staging",
            &origin.to_string_lossy(),
            &local.to_string_lossy(),
        ],
    );
    assert_eq!(
        git(&local, &["config", "--get-all", "remote.origin.fetch"]),
        "+refs/heads/staging:refs/remotes/origin/staging"
    );
    git(&seed, &["checkout", "-q", "-b", "issue-12"]);
    std::fs::write(seed.join("work"), "y\n").unwrap();
    git(&seed, &["add", "work"]);
    git(&seed, &[&id[..], &["commit", "-q", "-m", "work"]].concat());
    git(&seed, &["push", "-q", "origin", "issue-12"]);
    let head = git(&seed, &["rev-parse", "HEAD"]);
    let base = CONFIG
        .replace("harness  =  \"hermes\"", "harness = \"claude\"")
        .replace("/tmp/does-not-matter/sendmeter", &local.to_string_lossy());
    let c = ctx(&d, &format!("{base}{hosts}"));
    Repo { c, local, head }
}

fn fake() -> Fake {
    Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 0.4 1 1 }\n")
        .reply("sysctl -n hw.ncpu", "4\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("git ", "")
}

fn dirs(r: &Repo) -> Vec<String> {
    let wts = r.c.worktrees.join("sendmeter");
    [&r.local, &wts].map(|p| p.to_string_lossy().into_owned()).to_vec()
}

#[test]
fn a_review_spawn_in_a_narrow_clone_starts_from_the_pushed_issue_branch() {
    let r = repo("narrow-rev", "");
    let sh = FakeShed::new();
    sh.attach(&r.c);
    let f = fake();
    let (code, out) = run(&r.c, &Git(&f, &f, dirs(&r)), &["spawn", "sendmeter", "12", "rev"]);
    assert_eq!(code, 0, "{out} {:#?}", f.calls.borrow());
    let wt = r.c.worktrees.join("sendmeter/rev-12-r1");
    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), r.head);
}

#[test]
fn a_lane_moved_off_a_host_respawns_from_its_pushed_head_in_a_narrow_clone() {
    let r = repo(
        "narrow-move",
        "\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n",
    );
    let st = r.c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "bazzite": {"tags": ["os:linux"], "home": "/home/me", "probed": NOW, "seen": NOW, "up": true},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    assert_eq!(run(&r.c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let sh = FakeShed::new();
    sh.attach(&r.c);
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude", "pane": "sendmeter-impl-12",
                   "workspace": "", "worktree": "/home/me/.herdr/worktrees/sendmeter/issue-12", "branch": "issue-12",
                   "host": "bazzite", "machine": "bazzite"});
    append(&r.c.state, &Event::new(NOW - 900, "sendmeter", Some(12), "spawn", d)).unwrap();
    let (f, bazzite) = (fake(), FakeShed::new());
    let o = OverSsh {
        f: &f,
        hosts: vec![("bazzite", &bazzite)],
    };
    let args = ["spawn", "sendmeter", "12", "impl", "--host", "mini", "--supersede"];
    let (code, out) = run(&r.c, &Git(&o, &f, dirs(&r)), &args);
    assert_eq!(code, 0, "{out} {:#?}", f.calls.borrow());
    let wt = r.c.worktrees.join("sendmeter/issue-12");
    assert_eq!(git(&wt, &["rev-parse", "HEAD"]), r.head, "{:#?}", f.calls.borrow());
}
