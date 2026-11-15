mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::runner::{Out, Real, Runner};
use serde_json::json;
use std::path::{Path, PathBuf};

struct Git<'a>(&'a Fake, Vec<String>);

impl Runner for Git<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        if prog == "git" && args.len() > 1 && args[0] == "-C" && self.1.contains(&args[1]) {
            self.0.calls.borrow_mut().push(format!("git {}", args.join(" ")));
            return Real.run(prog, args);
        }
        self.0.run(prog, args)
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs)
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
    origin: PathBuf,
    local: PathBuf,
    wt: PathBuf,
    alerts: PathBuf,
    head: String,
    _sh: FakeShed,
}

fn repo(name: &str) -> Repo {
    let d = tmpdir(name);
    let (origin, local, alerts) = (d.join("origin.git"), d.join("local"), d.join("alerts.txt"));
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q", "--bare"]);
    git(
        &d,
        &["clone", "-q", &origin.to_string_lossy(), &local.to_string_lossy()],
    );
    let id = ["-c", "user.name=t", "-c", "user.email=t@t"];
    git(&local, &["checkout", "-q", "-b", "staging"]);
    std::fs::write(local.join("README"), "x\n").unwrap();
    git(&local, &["add", "README"]);
    git(&local, &[&id[..], &["commit", "-q", "-m", "init"]].concat());
    git(&local, &["push", "-q", "origin", "staging", "staging:issue-12"]);
    std::fs::write(local.join("NEWS"), "y\n").unwrap();
    git(&local, &["add", "NEWS"]);
    git(&local, &[&id[..], &["commit", "-q", "-m", "news"]].concat());
    git(&local, &["push", "-q", "origin", "staging"]);
    let head = git(&local, &["rev-parse", "origin/issue-12"]);
    let cfg = CONFIG.replace("/tmp/does-not-matter/sendmeter", &local.to_string_lossy())
        + &format!(
            "\n[[notify]]\ncmd = \"cat >> '{}'\"\non = [\"auto-update-failed\"]\n",
            alerts.display()
        );
    let c = ctx(&d, &cfg);
    let sh = FakeShed::new();
    sh.attach(&c);
    let wt = c.state.with_file_name("update-sendmeter");
    let cache = c.state.with_file_name("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let strict = json!({"ts": c.now, "strict": true}).to_string();
    std::fs::write(cache.join("strict-sendmeter-0.json"), strict).unwrap();
    let lane = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&lane).unwrap();
    let ev = |ts, kind, d| append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
    ev(
        10,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12",
        "workspace": "", "runtime": "shed", "worktree": lane, "branch": "issue-12"}),
    );
    ev(30, "verdict", json!({"verdict": "PASS", "sha": head, "body": "PASS"}));
    Repo {
        c,
        origin,
        local,
        wt,
        alerts,
        head,
        _sh: sh,
    }
}

fn fake(r: &Repo) -> Fake {
    let pr = json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": r.head,
        "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "IN_PROGRESS"}]});
    Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh pr view issue-12", &pr.to_string())
        .reply("git -C", "")
}

fn drive(r: &Repo, f: &Fake) -> String {
    let dirs = [&r.local, &r.wt].map(|p| p.to_string_lossy().into_owned()).to_vec();
    let (code, out) = run(&r.c, &Git(f, dirs), &["drive"]);
    assert_eq!(code, 0, "{out} {:?}", f.calls.borrow());
    out
}

#[test]
fn a_missing_but_locked_update_worktree_no_longer_stops_the_auto_update() {
    let r = repo("update-locked");
    git(
        &r.local,
        &["worktree", "add", "-q", "--detach", &r.wt.to_string_lossy(), "HEAD"],
    );
    git(
        &r.local,
        &["worktree", "lock", "--reason", "initializing", &r.wt.to_string_lossy()],
    );
    std::fs::remove_dir_all(&r.wt).unwrap();
    let f = fake(&r);
    let out = drive(&r, &f);
    assert!(
        out.contains(&format!("sendmeter #12 auto-updated: {:.7} → ", r.head)),
        "{out}"
    );
    let pushed = git(&r.origin, &["rev-parse", "issue-12"]);
    assert_ne!(pushed, r.head, "{out}");
    assert_eq!(pushed, git(&r.origin, &["rev-parse", "staging"]), "{out}");
}

#[test]
fn an_auto_update_failing_every_pass_raises_one_alert() {
    let r = repo("update-alert");
    std::fs::remove_dir_all(&r.origin).unwrap();
    let f = fake(&r);
    for _ in 0..3 {
        let out = drive(&r, &f);
        assert!(out.contains("sendmeter auto-update: "), "{out}");
    }
    let log = read(&r.c.state).unwrap();
    let alerts: Vec<&Event> = log.iter().filter(|e| e.kind == "auto-update-failed").collect();
    assert_eq!(alerts.len(), 1, "{alerts:?}");
    assert!(alerts[0].str("reason").unwrap().contains("fetch"), "{alerts:?}");
    let sent = std::fs::read_to_string(&r.alerts).unwrap();
    assert_eq!(sent.lines().count(), 1, "{sent}");
    assert!(sent.starts_with("sendmeter auto-update-failed: "), "{sent}");
}
