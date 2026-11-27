use super::*;
use drover::runner::Real;
use std::path::Path;

struct Git(Fake);

impl Runner for Git {
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

fn commit(seed: &Path, file: &str, push: &str) -> String {
    std::fs::write(seed.join(file), file).unwrap();
    git(seed, &["add", "."]);
    git(seed, &["commit", "-qm", file]);
    git(seed, &["push", "-q", "origin", &format!("HEAD:{push}")]);
    git(seed, &["rev-parse", "HEAD"])
}

fn pass() -> (usize, u128) {
    let d = tmpdir("perf-fetches");
    let (origin, seed, local) = (d.join("origin.git"), d.join("seed"), d.join("local"));
    git(&d, &["init", "-q", "--bare", "-b", "staging", origin.to_str().unwrap()]);
    git(&d, &["clone", "-q", origin.to_str().unwrap(), seed.to_str().unwrap()]);
    for (k, v) in [("user.email", "t@example.com"), ("user.name", "t")] {
        git(&seed, &["config", k, v]);
    }
    git(&seed, &["checkout", "-qb", "staging"]);
    commit(&seed, "a.txt", "staging");
    git(&d, &["clone", "-q", origin.to_str().unwrap(), local.to_str().unwrap()]);
    let c = ctx(
        &d,
        &CONFIG.replace("/tmp/does-not-matter/sendmeter", local.to_str().unwrap()),
    );
    let _sh = FakeShed::attached(&c);
    let ok = json!([{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}]);
    let mut f = Fake::default()
        .reply("gh pr merge", "")
        .reply("gh pr view 1", &json!({"mergeCommit": {"oid": "m3rg3"}}).to_string())
        .reply("gh issue", "");
    let cache = c.state.with_file_name("cache");
    std::fs::create_dir_all(&cache).unwrap();
    for n in 1..=5u64 {
        let branch = format!("issue-{n}");
        git(&seed, &["checkout", "-q", "-B", &branch, "staging"]);
        let sha = commit(&seed, &format!("f{n}.txt"), &branch);
        let spawn = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "branch": branch, "pane": format!("sendmeter-impl-{n}")});
        append(&c.state, &Event::new(10, "sendmeter", Some(n), "spawn", spawn)).unwrap();
        let v = json!({"verdict": "PASS", "sha": sha});
        append(&c.state, &Event::new(20, "sendmeter", Some(n), "verdict", v)).unwrap();
        let pr = json!({"ts": 0, "branch": branch, "pr": {"number": 10 + n, "state": "OPEN"}});
        std::fs::write(cache.join(format!("pr-sendmeter-{n}.json")), pr.to_string()).unwrap();
        let view = json!({"number": 10 + n, "state": "OPEN", "baseRefName": "staging", "headRefOid": sha, "statusCheckRollup": ok});
        f = f.reply(&format!("gh pr view {branch} "), &view.to_string());
    }
    let r = Git(f);
    let t = Instant::now();
    let (code, out) = run(&c, &r, &["drive", "sendmeter"]);
    let ms = ms(t);
    assert_eq!(code, 0, "{out}");
    let fetches = r.0.calls.borrow().iter().filter(|l| l.contains(" fetch ")).count();
    (fetches, ms)
}

#[test]
fn a_pass_over_5_open_prs_fetches_its_clone_once() {
    let (fetches, ms) = pass();
    eprintln!("perf fetches: one pass over 5 open PRs: {fetches} fetch(es), {ms} ms");
    assert!(fetches <= 1, "{fetches} fetches of one clone in one pass");
}
