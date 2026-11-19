mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use drover::runner::{Out, Real, Runner};
use serde_json::json;
use std::path::{Path, PathBuf};

struct Mixed(Fake, PathBuf, Shared<Vec<bool>>, bool);

impl Runner for Mixed {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        if prog == "git" {
            if args.iter().any(|a| a == "fetch") {
                let lock = self.1.join(".git/drover-fetch.lock");
                let held = std::fs::File::open(lock).is_ok_and(|f| f.try_lock().is_err());
                self.2.borrow_mut().push(held);
            }
            self.0.calls.borrow_mut().push(format!("git {}", args.join(" ")));
            if self.3 && args.iter().any(|a| a == "ls-remote" || a == "--refetch") {
                let stderr = "fatal: unable to access 'https://github.com/jirathip-dev/sendmeter.git/': Could not resolve host: github.com".into();
                return Ok(Out {
                    code: 128,
                    stderr,
                    ..Out::default()
                });
            }
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

fn loose(local: &Path, sha: &str) {
    std::fs::remove_file(local.join(".git/objects").join(&sha[..2]).join(&sha[2..])).unwrap();
}

struct Repos {
    d: PathBuf,
    local: PathBuf,
    c: drover::Ctx,
    head: String,
}

fn repos(name: &str) -> Repos {
    let d = tmpdir(name);
    let (origin, seed, local) = (d.join("origin.git"), d.join("seed"), d.join("local"));
    git(&d, &["init", "-q", "--bare", "-b", "staging", origin.to_str().unwrap()]);
    git(&d, &["clone", "-q", origin.to_str().unwrap(), seed.to_str().unwrap()]);
    for (k, v) in [("user.email", "t@example.com"), ("user.name", "t")] {
        git(&seed, &["config", k, v]);
    }
    git(&seed, &["checkout", "-qb", "staging"]);
    commit(&seed, "a.txt", "staging");
    git(&d, &["clone", "-q", origin.to_str().unwrap(), local.to_str().unwrap()]);
    for (k, v) in [("user.email", "t@example.com"), ("user.name", "t")] {
        git(&local, &["config", k, v]);
    }
    git(&seed, &["checkout", "-qb", "issue-3"]);
    let head = commit(&seed, "b.txt", "issue-3");
    git(&seed, &["checkout", "-q", "staging"]);
    let config = CONFIG.replace("/tmp/does-not-matter/sendmeter", local.to_str().unwrap());
    let c = ctx(&d, &config);
    Repos { d, local, c, head }
}

fn break_clone(r: &Repos) -> (String, String) {
    let seed = r.d.join("seed");
    commit(&seed, "c.txt", "staging");
    git(&seed, &["checkout", "-qb", "issue-9"]);
    let gone = commit(&seed, "d.txt", "issue-9");
    git(&seed, &["checkout", "-q", "staging"]);
    git(&r.local, &["fetch", "-q", "origin"]);
    git(&seed, &["push", "-q", "origin", ":issue-9"]);
    let tree = git(&r.local, &["rev-parse", "origin/staging^{tree}"]);
    loose(&r.local, &tree);
    loose(&r.local, &gone);
    (tree, gone)
}

fn ready(c: &drover::Ctx, head: &str) {
    let spawn = json!({"role": "impl", "agent": "sendmeter-impl-3", "branch": "issue-3", "pane": "sendmeter-impl-3"});
    append(&c.state, &Event::new(10, "sendmeter", Some(3), "spawn", spawn)).unwrap();
    let v = json!({"verdict": "PASS", "sha": head, "round": 1});
    append(&c.state, &Event::new(20, "sendmeter", Some(3), "verdict", v)).unwrap();
    let cache = c.state.with_file_name("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let pr = json!({"ts": 0, "branch": "issue-3", "pr": {"number": 13, "state": "OPEN"}});
    std::fs::write(cache.join("pr-sendmeter-3.json"), pr.to_string()).unwrap();
}

fn merger(local: &Path, head: &str) -> Mixed {
    let ok = json!([{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}]);
    let pr =
        json!({"number": 13, "state": "OPEN", "baseRefName": "staging", "headRefOid": head, "statusCheckRollup": ok});
    let f = Fake::default()
        .reply("gh pr view issue-3 ", &pr.to_string())
        .reply("gh api repos/jirathip-dev/sendmeter/branches/issue-3", head)
        .reply("sh -c", "src lines (formatted): 10 / cap 14700\n")
        .reply("gh pr merge", "")
        .reply("gh pr view 13", &json!({"mergeCommit": {"oid": "m3rg3"}}).to_string())
        .reply("gh issue", "");
    Mixed(f, local.to_path_buf(), Shared::default(), false)
}

fn events(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

fn premerge(local: &Path) {
    std::fs::create_dir_all(local.join("ci")).unwrap();
    std::fs::write(local.join("ci/cap.toml"), "[fleet.sendmeter]\nline_cap = 14700\n").unwrap();
}

#[test]
fn a_pass_heals_a_clone_missing_objects_and_merges() {
    let r = repos("heal-merge");
    let _sh = FakeShed::attached(&r.c);
    premerge(&r.local);
    ready(&r.c, &r.head);
    let (tree, _) = break_clone(&r);
    let m = merger(&r.local, &r.head);
    let (code, out) = run(&r.c, &m, &["drive", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    let st = &fold(&read(&r.c.state).unwrap())[&("sendmeter".to_string(), 3)];
    assert_eq!(st.state, "merged", "{out} {:#?}", m.0.calls.borrow());
    let healed = events(&r.c, "clone-healed");
    assert_eq!(healed.len(), 1, "{out}");
    let h = &healed[0];
    assert_eq!(h.fleet, "sendmeter");
    assert_eq!(h.str("clone"), r.local.to_str());
    assert_eq!(h.val("dropped"), &json!(["refs/remotes/origin/issue-9"]));
    assert!(h.val("secs").is_u64(), "{:?}", h.data);
    assert!(m.0.called(&format!("git -C {} fetch --refetch", r.local.display())));
    git(&r.local, &["cat-file", "-e", &tree]);
    git(&r.local, &["fsck", "--connectivity-only"]);
}

#[test]
fn an_unhealable_clone_alerts_once_and_the_other_steps_still_run() {
    let r = repos("heal-unreachable");
    let _sh = FakeShed::attached(&r.c);
    let note = r.d.join("alerts.txt");
    let toml = std::fs::read_to_string(&r.c.config).unwrap();
    std::fs::write(
        &r.c.config,
        toml + &format!("\n[[notify]]\ncmd = \"cat >> '{}'\"\n", note.display()),
    )
    .unwrap();
    break_clone(&r);
    append(&r.c.state, &Event::new(5, "sendmeter", Some(4), "merged", json!({}))).unwrap();
    append(
        &r.c.state,
        &Event::new(6, "sendmeter", Some(5), "held", json!({"on": [4]})),
    )
    .unwrap();
    for _ in 0..2 {
        let mut m = merger(&r.local, &r.head);
        m.3 = true;
        let (code, out) = run(&r.c, &m, &["drive", "sendmeter"]);
        assert_eq!(code, 0, "{out}");
    }
    let broken = events(&r.c, "clone-broken");
    assert_eq!(broken.len(), 1, "one alert, not one per pass: {broken:?}");
    assert_eq!(broken[0].str("clone"), r.local.to_str());
    assert!(
        broken[0].str("error").is_some_and(|e| !e.is_empty()),
        "{:?}",
        broken[0].data
    );
    let sent = std::fs::read_to_string(&note).unwrap_or_default();
    assert_eq!(sent.matches("clone-broken").count(), 1, "{sent}");
    assert!(sent.contains(r.local.to_str().unwrap()), "{sent}");
    assert!(events(&r.c, "clone-healed").is_empty());
    assert_eq!(events(&r.c, "released").len(), 1, "a step needing no fetch ran");
    git(
        &r.local,
        &["rev-parse", "--verify", "-q", "refs/remotes/origin/issue-9"],
    );
    let (code, out) = run(&r.c, &merger(&r.local, &r.head), &["drive", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        events(&r.c, "clone-healed").len(),
        1,
        "origin back: the due heal runs {out}"
    );
    git(&r.local, &["fsck", "--connectivity-only"]);
}

#[test]
fn every_fetch_of_a_clone_runs_under_its_lock() {
    let r = repos("heal-lock");
    let _sh = FakeShed::attached(&r.c);
    premerge(&r.local);
    ready(&r.c, &r.head);
    let m = merger(&r.local, &r.head);
    let (code, out) = run(&r.c, &m, &["drive", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    let held = m.2.borrow().clone();
    assert!(held.len() >= 2, "{held:?} {:#?}", m.0.calls.borrow());
    assert!(held.iter().all(|h| *h), "a fetch ran with the lock free: {held:?}");
    let lock = std::fs::File::open(r.local.join(".git/drover-fetch.lock")).unwrap();
    assert!(lock.try_lock().is_ok(), "released after the fetch");
}

#[test]
fn doctor_names_the_missing_object_and_the_heal_command() {
    let r = repos("heal-doctor");
    let _sh = FakeShed::attached(&r.c);
    let doctor = || {
        let m = Mixed(Fake::default(), r.local.clone(), Shared::default(), false);
        run(&r.c, &m, &["doctor"]).1
    };
    let out = doctor();
    let ok = format!("✓ clone sendmeter — {} connectivity ok", r.local.display());
    assert!(out.contains(&ok), "{out}");
    let (tree, _) = break_clone(&r);
    let out = doctor();
    let line = out.lines().find(|l| l.contains("clone sendmeter")).unwrap_or_default();
    assert!(line.starts_with('✗'), "{out}");
    assert!(line.contains(&format!("missing tree {tree}")), "{line}");
    let dir = r.local.display();
    assert!(
        line.contains(&format!("git -C {dir} update-ref -d refs/remotes/origin/issue-9")),
        "{line}"
    );
    assert!(line.contains(&format!("git -C {dir} fetch --refetch origin")), "{line}");
}

#[test]
fn clean_removes_temp_packs_older_than_a_day() {
    let r = repos("heal-tmp-pack");
    let _sh = FakeShed::attached(&r.c);
    let pack = r.local.join(".git/objects/pack");
    let (old, new) = (pack.join("tmp_pack_old1"), pack.join("tmp_pack_new1"));
    for p in [&old, &new] {
        std::fs::write(p, vec![0u8; 1000]).unwrap();
    }
    let two_days = std::time::SystemTime::now() - std::time::Duration::from_hours(48);
    std::fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(two_days)
        .unwrap();
    let (_, dry) = run(&r.c, &Fake::default(), &["clean"]);
    assert!(old.exists() && dry.contains("would remove 1 temp pack"), "{dry}");
    let (code, out) = run(&r.c, &Fake::default(), &["clean", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(!old.exists(), "{out}");
    assert!(new.exists(), "a day-old pack may be a running fetch's");
    assert!(out.contains("removed 1 temp pack"), "{out}");
}

const SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C -oControlPersist=10m bazzite -- ";

#[test]
fn a_remote_hosts_clone_gets_the_same_heal() {
    let base = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
    let c = ctx(
        &tmpdir("heal-remote"),
        &format!("{base}\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n"),
    );
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": 1_000_000, "seen": 1_000_000, "up": true},
        "bazzite": {"tags": ["os:linux"], "home": "/home/me", "probed": 1_000_000, "seen": 1_000_000, "up": true,
                    "load": 0.1, "mem": 12.0, "disk": 200.0},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let _sh = FakeShed::attached(&c);
    let git = format!("{SSH}'git' '--no-replace-objects' '-C' ");
    let gone = "4f50d959cad0891493b08636928fbfd2db083ea0";
    let bad = "fatal: bad object refs/remotes/origin/issue-9\nerror: did not send all necessary objects";
    let r = Fake::default()
        .reply("sysctl -n vm.loadavg", "{ 0.4 1 1 }")
        .reply("sysctl -n hw.ncpu", "4")
        .reply(&format!("{SSH}'cat' '/proc/loadavg'"), "0.4 1 1 1/9 42")
        .reply(&format!("{SSH}'nproc'"), "4")
        .reply(
            &format!("{SSH}'/opt/drover/bin/drover' 'profiles' 'sync' '--profile'"),
            "",
        )
        .reply("gh issue view 12", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .once_err(&format!("{git}'/tmp/does-not-matter/sendmeter' 'fetch'"), 1, bad)
        .reply(&format!("{SSH}'sh' '-c' 'git clone"), "")
        .reply(&format!("{SSH}'sh' '-c' 'mkdir"), "")
        .reply(&format!("{SSH}'sh' '-c' 'umask 077"), "")
        .reply(&format!("{SSH}'rm' '-f'"), "")
        .reply("which shed", "/opt/shed/bin/shed\n");
    let r = Fake {
        replies: [
            vec![
                (
                    format!("{git}'/tmp/does-not-matter/sendmeter' 'for-each-ref'"),
                    Out {
                        stdout: format!("{gone}  refs/remotes/origin/issue-9\n"),
                        ..Out::default()
                    },
                ),
                (
                    format!("{git}'/tmp/does-not-matter/sendmeter' 'cat-file' '-e' '{gone}'"),
                    Out {
                        code: 1,
                        ..Out::default()
                    },
                ),
                (git.clone(), Out::default()),
            ],
            r.replies,
        ]
        .concat(),
        ..r
    };
    let bazzite = FakeShed::new();
    let o = OverSsh {
        f: &r,
        hosts: vec![("bazzite", &bazzite)],
    };
    let (code, out) = run(&c, &o, &["spawn", "sendmeter", "12", "impl", "--host", "bazzite"]);
    assert_eq!(code, 0, "{out} {:#?}", r.calls.borrow());
    let heal = [
        "'update-ref' '-d' 'refs/remotes/origin/issue-9'",
        "'fetch' '--refetch' '-q' 'origin'",
        "'fsck' '--connectivity-only'",
    ];
    let at: Vec<Option<usize>> = heal
        .iter()
        .map(|h| {
            r.calls
                .borrow()
                .iter()
                .position(|c| c.starts_with(SSH) && c.ends_with(h))
        })
        .collect();
    assert!(
        at.iter().all(Option::is_some) && at.is_sorted(),
        "{at:?} {:#?}",
        r.calls.borrow()
    );
    let healed = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .find(|e| e.kind == "clone-healed");
    assert_eq!(
        healed.map(|e| e.val("dropped").clone()),
        Some(json!(["refs/remotes/origin/issue-9"]))
    );
}
