mod common;
use common::*;
use drover::config::Config;
use drover::events::{append, Event};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

fn ev(c: &drover::Ctx, issue: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(10, "sendmeter", Some(issue), kind, d)).unwrap();
}

fn spawn(c: &drover::Ctx, issue: u64, role: &str, agent: &str) {
    let w = c.worktrees.join("sendmeter").join(agent);
    std::fs::create_dir_all(&w).unwrap();
    let d = json!({"role": role, "agent": agent, "worktree": w, "pane": agent, "runtime": "shed"});
    ev(c, issue, "spawn", d);
}

fn air_spawn(c: &drover::Ctx, issue: u64, role: &str, agent: &str) {
    let d = json!({"role": role, "agent": agent, "worktree": format!("/Users/me/worktrees/sendmeter/{agent}"),
                   "pane": agent, "runtime": "shed", "host": "air", "machine": "air"});
    ev(c, issue, "spawn", d);
}

fn merged(c: &drover::Ctx, issue: u64) {
    ev(
        c,
        issue,
        "merged",
        json!({"pr": "1", "merge_sha": "m", "into": "staging"}),
    );
}

fn blob(dir: &Path, rel: &str) -> PathBuf {
    let p = dir.join(rel);
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(p.join("blob"), vec![0u8; 4096]).unwrap();
    p
}

fn age(p: &Path, at: u64) {
    for e in std::fs::read_dir(p).unwrap().flatten() {
        let f = std::fs::File::open(e.path()).unwrap();
        f.set_modified(UNIX_EPOCH + Duration::from_secs(at)).unwrap();
    }
    let f = std::fs::File::open(p).unwrap();
    f.set_modified(UNIX_EPOCH + Duration::from_secs(at)).unwrap();
}

fn setup(name: &str, budget_bytes: u64) -> (drover::Ctx, PathBuf) {
    let d = tmpdir(name);
    let root = d.join("worktrees/.build");
    let cfg = CONFIG.replace(
        "build_root = \"\"",
        &format!("build_budget_gb = {}", budget_bytes as f64 / 1_073_741_824.0),
    );
    (ctx(&d, &cfg), root.join(".scratch"))
}

fn budget(c: &drover::Ctx, sh: &FakeShed) -> String {
    let (mut out, f) = (vec![], Fake::default());
    drover::check::budget(c, &Config::load(&c.config).unwrap(), &sh.on(&f), &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

#[test]
fn drive_deletes_an_ended_lanes_scratch() {
    let (c, s) = setup("scratch-sweep", 1 << 40);
    spawn(&c, 12, "impl", "sendmeter-impl-12");
    merged(&c, 12);
    spawn(&c, 13, "impl", "sendmeter-impl-13");
    spawn(&c, 13, "rev", "sendmeter-rev-13-r1");
    spawn(&c, 13, "rev", "sendmeter-rev-13-r2");
    let dirs = [
        "sendmeter-impl-12",
        "sendmeter-rev-13-r1",
        "sendmeter-rev-13-r2",
        "sendmeter-impl-14",
        "sendmeter-rev-15-r1",
        "stray",
    ]
    .map(|a| {
        blob(&s, &format!("{a}/base/target/debug"))
            .ancestors()
            .nth(3)
            .unwrap()
            .to_path_buf()
    });
    let lock = s.join("sendmeter-rev-15-r1/base/target/debug/.cargo-lock");
    std::fs::write(&lock, "").unwrap();
    let held = std::fs::File::open(&lock).unwrap();
    held.lock().unwrap();
    let sh = FakeShed::new();
    sh.attach(&c);
    sh.lane("sendmeter-impl-14", "idle", "/x");
    let out = budget(&c, &sh);
    let gone = dirs.each_ref().map(|d| !d.exists());
    assert_eq!(
        gone,
        [true, true, false, false, false, true],
        "merged, superseded and unknown go; active, live and building stay: {out}"
    );
    assert!(out.contains("(lane ended)"), "{out}");
}

#[test]
fn a_finished_lanes_build_folder_goes_at_once_even_with_its_session_open() {
    let (c, s) = setup("build-finished", 1 << 40);
    let root = s.parent().unwrap().to_path_buf();
    let verdict = |n| ev(&c, n, "verdict", json!({"verdict": "FAIL", "sha": "a", "stage": "rev"}));
    spawn(&c, 13, "impl", "sendmeter-impl-13");
    spawn(&c, 13, "rev", "sendmeter-rev-13-r1");
    verdict(13);
    spawn(&c, 13, "impl", "sendmeter-impl-13-f1");
    for n in [14, 15] {
        spawn(&c, n, "rev", &format!("sendmeter-rev-{n}-r1"));
        verdict(n);
    }
    spawn(&c, 16, "impl", "sendmeter-impl-16");
    ev(&c, 16, "turn-end", json!({"state": "done", "role": "impl"}));
    let lanes = [
        "sendmeter-impl-13",
        "sendmeter-rev-13-r1",
        "sendmeter-impl-13-f1",
        "sendmeter-rev-14-r1",
        "sendmeter-rev-15-r1",
        "sendmeter-impl-16",
        "stray",
    ];
    let builds = lanes.map(|a| blob(&root, &format!("{a}/debug")).parent().unwrap().to_path_buf());
    let scratch = blob(&s, "sendmeter-rev-13-r1");
    for b in &builds {
        std::fs::write(b.join("debug/.cargo-lock"), "").unwrap();
    }
    let held = std::fs::File::open(builds[4].join("debug/.cargo-lock")).unwrap();
    held.lock().unwrap();
    let wts = lanes.map(|a| blob(&c.worktrees.join("sendmeter"), &format!("{a}/src")));
    let sh = FakeShed::new();
    sh.attach(&c);
    for a in &lanes[..6] {
        sh.lane(a, if a.contains("-14-") { "working" } else { "idle" }, "/x");
    }
    let out = budget(&c, &sh);
    let gone = builds.each_ref().map(|d| !d.exists());
    assert_eq!(
        gone,
        [true, true, false, false, false, true, false],
        "superseded, verdict and turn done go; active, working, building and unknown stay: {out}"
    );
    assert!(!scratch.exists(), "the finished lane's scratch goes too: {out}");
    assert!(
        wts.iter().all(|w| w.join("blob").is_file()),
        "worktrees untouched: {out}"
    );
    let freed = events(&c, "build-freed");
    let mut got: Vec<_> = (freed.iter())
        .map(|e| {
            (
                e.str("agent").unwrap(),
                e.str("why").unwrap(),
                e.data["bytes"].as_u64().unwrap() > 0,
            )
        })
        .collect();
    got.sort();
    assert_eq!(
        got,
        [
            ("sendmeter-impl-13", "superseded", true),
            ("sendmeter-impl-16", "lane finished", true),
            ("sendmeter-rev-13-r1", "lane finished", true),
            ("sendmeter-rev-13-r1", "lane finished", true),
        ],
        "{out}"
    );
    drop(held);
}

fn events(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    let all = drover::events::read(&c.state).unwrap().to_vec();
    all.into_iter().filter(|e| e.kind == kind).collect()
}

#[test]
fn the_oldest_scratch_is_trimmed_over_budget() {
    let (c, s) = setup("scratch-trim", 6000);
    spawn(&c, 9, "rev", "sendmeter-rev-9-r1");
    spawn(&c, 13, "impl", "sendmeter-impl-13");
    let (old, new) = (blob(&s, "sendmeter-rev-9-r1"), blob(&s, "sendmeter-impl-13"));
    age(&old, 1_000);
    let sh = FakeShed::new();
    sh.attach(&c);
    let out = budget(&c, &sh);
    assert!(!old.exists() && new.exists(), "{out}");
    assert!(out.contains("builds: trimmed"), "{out}");
}

#[test]
fn clean_removes_stray_cargo_folders_and_claude_temp_dirs() {
    let d = tmpdir("scratch-clean");
    let tmp = d.join("tmp");
    let cfg = CONFIG.replace("stray_roots = []", &format!("stray_roots = [\"{}\"]", tmp.display()));
    let mut c = ctx(&d, &cfg);
    c.now = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let old = c.now - 2 * 86_400;
    let cargo = |name: &str, marker: &str, at: u64| {
        let p = blob(&tmp, &format!("{name}/debug")).parent().unwrap().to_path_buf();
        std::fs::write(p.join(marker), "").unwrap();
        age(&p, at);
        p
    };
    let stray = cargo("t344-target", "CACHEDIR.TAG", old);
    let info = cargo("rev447t", ".rustc_info.json", old);
    let fresh = cargo("h370-base-target", "CACHEDIR.TAG", c.now);
    let building = cargo("rev502-target", "CACHEDIR.TAG", old);
    let lock = building.join("debug/.cargo-lock");
    std::fs::write(&lock, "").unwrap();
    let held = std::fs::File::open(&lock).unwrap();
    held.lock().unwrap();
    age(&building, old);
    let plain = blob(&tmp, "notes-target");
    age(&plain, old);

    spawn(&c, 12, "impl", "sendmeter-impl-12");
    merged(&c, 12);
    let wt = c
        .worktrees
        .join("sendmeter")
        .join("sendmeter-impl-12")
        .display()
        .to_string();
    let key: String = wt
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect();
    let claude = blob(&tmp, &format!("claude-501/{key}"));
    let other = blob(&tmp, "claude-501/-Users-me-elsewhere");

    let sh = FakeShed::new();
    sh.attach(&c);
    let r = || {
        Fake::default()
            .reply(&format!("git -C {wt} rev-list"), "0\n")
            .reply("git -C", "")
    };
    let (code, dry) = run(&c, &r(), &["clean"]);
    assert_eq!(code, 0, "{dry}");
    for p in [&stray, &info] {
        assert!(
            dry.contains(&format!(
                "would remove stray build folder {}",
                p.canonicalize().unwrap().display()
            )),
            "{dry}"
        );
    }
    for p in [&fresh, &building, &plain] {
        assert!(
            !dry.contains(&p.file_name().unwrap().to_string_lossy().into_owned()),
            "{dry}"
        );
    }
    assert!(
        stray.exists() && info.exists() && claude.exists(),
        "a dry run removes nothing"
    );

    let (code, done) = run(&c, &r(), &["clean", "--yes"]);
    assert_eq!(code, 0, "{done}");
    assert!(!stray.exists() && !info.exists(), "{done}");
    assert!(fresh.exists() && building.exists() && plain.exists(), "{done}");
    assert!(done.contains(&format!("removed {wt}")), "{done}");
    assert!(
        !claude.exists() && other.exists(),
        "only the removed worktree's Claude temp dir goes"
    );
}

struct Air<'a> {
    over: OverSsh<'a>,
    free_kb: u64,
    home: PathBuf,
}

const AIR_ROOT: &str = "~/.cache/hs-651-air";

impl drover::runner::Runner for Air<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        let line = args.iter().position(|a| a == "--").map(|i| args[i + 1..].join(" "));
        let Some(line) = line.filter(|_| prog == "ssh" && args.iter().any(|a| a == "air")) else {
            return self.over.run(prog, args);
        };
        self.over.f.calls.borrow_mut().push(format!("ssh air -- {line}"));
        if line.starts_with("'df'") {
            let df = format!(
                "Filesystem 1024-blocks Used Available\n/dev/disk3 488000000 1 {}\n",
                self.free_kb
            );
            return Ok(drover::runner::Out {
                stdout: df,
                ..Default::default()
            });
        }
        if !line.contains(AIR_ROOT) {
            return self.over.run(prog, args);
        }
        let o = std::process::Command::new("sh")
            .args(["-c", &line])
            .env("HOME", &self.home)
            .output()?;
        Ok(drover::runner::Out {
            code: o.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&o.stdout).into(),
            stderr: String::from_utf8_lossy(&o.stderr).into(),
        })
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<drover::runner::Out> {
        let batch = drover::hosts::quote("sh", &["-c".into(), drover::hosts::EXISTS.into()]);
        match args.last() == Some(&batch) || args.iter().any(|a| a.contains("'shed' 'connect'")) {
            true => self.over.run_in(prog, args, input),
            false => self.run(prog, args),
        }
    }
    fn sleep(&self, _: u64) {}
}

fn air_setup(name: &str) -> (drover::Ctx, PathBuf, PathBuf) {
    let d = tmpdir(name);
    let cfg = CONFIG.replace("build_root = \"\"", "build_budget_gb = 1000");
    let c = ctx(
        &d,
        &format!("{cfg}\n[hosts.air]\nmachine = \"air\"\nroot = \"{AIR_ROOT}\"\n"),
    );
    let home = d.join("air-home");
    let s = home.join(AIR_ROOT.trim_start_matches("~/")).join(".build/.scratch");
    (c, home, s)
}

fn tick(c: &drover::Ctx, home: &Path, air: &[(&str, &str)], free_kb: u64) -> (Vec<String>, String) {
    let sh = FakeShed::new();
    sh.attach(c);
    let air_sh = FakeShed::new();
    air.iter().for_each(|(n, st)| air_sh.lane(n, st, "/x"));
    let f = Fake::default()
        .reply(
            "herdr status",
            "client:\n  version: 0.9.5\n\nserver:\n  status: running\n  version: 0.9.5\n",
        )
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("git -C", "");
    let r = Air {
        over: OverSsh {
            f: &f,
            hosts: vec![("air", &air_sh)],
        },
        free_kb,
        home: home.into(),
    };
    let (code, out) = run(c, &r, &["tick"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        air_sh.methods().contains(&"ls".to_string()),
        "air's lanes listed on its shed"
    );
    let calls = f.calls.borrow().clone();
    (calls, out)
}

#[test]
fn a_remote_hosts_ended_and_orphan_scratch_goes_at_the_tick() {
    let (c, home, s) = air_setup("scratch-remote");
    air_spawn(&c, 12, "impl", "sendmeter-impl-12");
    merged(&c, 12);
    air_spawn(&c, 13, "impl", "sendmeter-impl-13");
    let [ended, working, orphan] =
        ["sendmeter-impl-12", "sendmeter-impl-13", "sendmeter-rev-9-r1"].map(|a| blob(&s, &format!("{a}/debug")));
    let b = s.parent().unwrap();
    let [built, building, unknown] =
        ["sendmeter-impl-12", "sendmeter-impl-13", "sendmeter-rev-9-r1"].map(|a| blob(b, &format!("{a}/debug")));
    let air = [("sendmeter-impl-12", "done"), ("sendmeter-impl-13", "working")];
    let (calls, out) = tick(&c, &home, &air, 200 * 1_048_576);
    assert!(!ended.exists() && !orphan.exists(), "{out}");
    assert!(working.exists(), "a working lane's scratch stays: {out}");
    assert!(!built.parent().unwrap().exists(), "{out}");
    assert!(building.exists() && unknown.exists(), "{out}");
    let freed = events(&c, "build-freed");
    assert!(
        freed
            .iter()
            .any(|e| e.str("agent") == Some("sendmeter-impl-12") && e.str("host") == Some("air")),
        "{out}"
    );
    assert!(
        calls
            .iter()
            .any(|l| l.starts_with("ssh air -- 'sh' '-c'") && l.contains(".scratch")),
        "swept through air's runner"
    );
    for a in ["sendmeter-impl-12", "sendmeter-rev-9-r1"] {
        assert!(
            out.contains(&format!("scratch: air: removed {AIR_ROOT}/.build/.scratch/{a} ")),
            "{out}"
        );
    }
    assert!(out.contains("(lane ended)"), "{out}");
}

#[test]
fn a_host_short_of_disk_drops_non_working_scratch_largest_first() {
    let (c, home, s) = air_setup("scratch-low");
    air_spawn(&c, 13, "impl", "sendmeter-impl-13");
    air_spawn(&c, 13, "rev", "sendmeter-rev-13-r1");
    air_spawn(&c, 14, "impl", "sendmeter-impl-14");
    let sized = |a: &str, kb: usize| {
        let p = s.join(a);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("blob"), vec![1u8; kb * 1024]).unwrap();
        p
    };
    let (small, big, busy) = (
        sized("sendmeter-impl-13", 64),
        sized("sendmeter-rev-13-r1", 512),
        sized("sendmeter-impl-14", 256),
    );
    let air = [
        ("sendmeter-impl-13", "idle"),
        ("sendmeter-rev-13-r1", "done"),
        ("sendmeter-impl-14", "working"),
    ];
    let (_, out) = tick(&c, &home, &air, 1_048_576);
    assert!(!small.exists() && !big.exists(), "{out}");
    assert!(busy.exists(), "a working lane's scratch is never touched: {out}");
    let at = |a: &str| {
        out.find(&format!("scratch: air: removed {AIR_ROOT}/.build/.scratch/{a} "))
            .expect(&out)
    };
    assert!(
        at("sendmeter-rev-13-r1") < at("sendmeter-impl-13"),
        "largest first: {out}"
    );
    assert!(out.contains("(disk low)"), "{out}");
}

fn air_off(name: &str) -> (drover::Ctx, PathBuf, [PathBuf; 3]) {
    let (c, home, s) = air_setup(name);
    let text = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(&c.config, format!("{text}enabled = false\n")).unwrap();
    for (n, a) in [
        (12, "sendmeter-impl-12"),
        (13, "sendmeter-impl-13"),
        (14, "sendmeter-impl-14"),
    ] {
        air_spawn(&c, n, "impl", a);
    }
    merged(&c, 12);
    let dirs = ["sendmeter-impl-12", "sendmeter-impl-13", "sendmeter-impl-14"].map(|a| blob(&s, &format!("{a}/debug")));
    (c, home, dirs)
}

fn air_off_runner<'a>(home: &Path, f: &'a Fake, air: &'a FakeShed) -> Air<'a> {
    Air {
        over: OverSsh {
            f,
            hosts: vec![("air", air)],
        },
        free_kb: 200 * 1_048_576,
        home: home.into(),
    }
}

fn air_off_hosts(c: &drover::Ctx) -> (Fake, FakeShed, FakeShed) {
    let wt = AIR_WT;
    let ssh = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C -oControlPersist=10m air -- ";
    let f = Fake::default()
        .reply("gh issue list", "[]")
        .reply(&format!("{ssh}'sh' '-c' 'test -e"), "")
        .reply(&format!("{ssh}'git' '-C' '{wt}' 'status'"), "")
        .reply(&format!("{ssh}'git' '-C' '{wt}' 'rev-list'"), "0\n")
        .reply(ssh, "");
    let here = FakeShed::attached(c);
    here.lane("sendmeter-impl-13", "working", "/x");
    let air = FakeShed::new();
    air.lane("sendmeter-impl-14", "idle", "/y");
    (f, here, air)
}

const AIR_WT: &str = "/Users/me/worktrees/sendmeter/sendmeter-impl-12";

#[test]
fn clean_sweeps_a_disabled_but_reachable_host() {
    let (c, home, [dead, ..]) = air_off("scratch-off-clean");
    let wt = AIR_WT;
    let (f, _here, air) = air_off_hosts(&c);
    let (code, dry) = run(&c, &air_off_runner(&home, &f, &air), &["clean"]);
    assert_eq!(code, 0, "{dry}");
    assert!(!dry.contains("not cleaned"), "{dry}");
    assert!(
        dry.contains(&format!(
            "would remove air:{wt} (pane sendmeter-impl-12, sendmeter#12 merged)"
        )),
        "{dry}"
    );
    assert!(
        dry.contains(&format!(
            "would remove scratch air:{AIR_ROOT}/.build/.scratch/sendmeter-impl-12"
        )),
        "{dry}"
    );
    assert!(dead.exists(), "a dry run removes nothing");
    let (f, _here, air) = air_off_hosts(&c);
    let r = air_off_runner(&home, &f, &air);
    let (code, out) = run(&c, &r, &["clean", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(&format!("removed air:{wt} (sendmeter#12 merged)")),
        "{out}"
    );
    let calls = f.calls.borrow();
    assert!(
        calls
            .iter()
            .any(|l| l.contains(" air ") && l.contains("worktree") && l.contains("remove") && l.contains(wt)),
        "{calls:?}"
    );
    drop(calls);
    assert!(!dead.exists(), "{out}");
    assert!(
        out.contains(&format!(
            "scratch: air: removed {AIR_ROOT}/.build/.scratch/sendmeter-impl-12 "
        )),
        "{out}"
    );
}

#[test]
fn a_disabled_hosts_scratch_stays_only_for_its_own_live_agents() {
    let (c, home, [dead, moved, live]) = air_off("scratch-off-reclaim");
    let (f, _here, air) = air_off_hosts(&c);
    let r = air_off_runner(&home, &f, &air);
    let (code, out) = run(&c, &r, &["reclaim", "--apply"]);
    assert_eq!(code, 0, "{out}");
    assert!(!dead.exists() && !moved.exists(), "{out}");
    assert!(live.exists(), "the Air's own live agent keeps its scratch: {out}");
    assert!(
        out.contains(&format!(
            "scratch: air: removed {AIR_ROOT}/.build/.scratch/sendmeter-impl-13 "
        )),
        "{out}"
    );
}
