//! #536: lanes' private scratch under `build_root`: drive deletes an ended lane's and trims the oldest over budget;
//! `clean` removes stray Cargo folders in the stray roots and a removed worktree's Claude temp dir.
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
    let d = json!({"role": role, "agent": agent, "worktree": w, "pane": format!("{agent}:p1"), "workspace": agent});
    ev(c, issue, "spawn", d);
}

/// #651: a lane spawned on the remote host `air`.
fn air_spawn(c: &drover::Ctx, issue: u64, role: &str, agent: &str) {
    let d = json!({"role": role, "agent": agent, "worktree": format!("/Users/me/.herdr/worktrees/sendmeter/{agent}"),
                   "pane": "w1:p1", "workspace": "w1", "host": "air", "machine": "air"});
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

/// `<dir>/<rel>` holding a 4096-byte blob.
fn blob(dir: &Path, rel: &str) -> PathBuf {
    let p = dir.join(rel);
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(p.join("blob"), vec![0u8; 4096]).unwrap();
    p
}

/// `p` and its entries last modified at `at`.
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
    let root = d.join("worktrees/.build"); // #630: the root's build folder
    let cfg = CONFIG.replace(
        "build_root = \"\"",
        &format!("build_budget_gb = {}", budget_bytes as f64 / 1_073_741_824.0),
    );
    (ctx(&d, &cfg), root.join(".scratch"))
}

fn budget(c: &drover::Ctx, r: &Fake) -> String {
    let mut out = vec![];
    drover::check::budget(c, &Config::load(&c.config).unwrap(), r, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

/// An ended lane's scratch goes at drive's pass (merged, superseded by a later round, or unknown); an open issue's
/// active lane, a live herdr agent and a building one keep theirs.
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
    held.lock().unwrap(); // a cargo build running there
    let r = Fake::default().reply("herdr agent list", &agents(&[("sendmeter-impl-14", "idle", "/x", "p")]));
    let out = budget(&c, &r);
    let gone = dirs.each_ref().map(|d| !d.exists());
    assert_eq!(
        gone,
        [true, true, false, false, false, true],
        "merged, superseded and unknown go; active, live and building stay: {out}"
    );
    assert!(out.contains("(lane ended)"), "{out}");
}

/// #798: a lane's `.build/<agent>` folder and its scratch go on the next pass once it is finished (a recorded verdict,
/// its own `turn done`, or replaced by a newer round), its session open or not; a working, building, active or unknown
/// one stays. Worktrees are never touched; each removal logs `build-freed`.
#[test]
fn a_finished_lanes_build_folder_goes_at_once_even_with_its_session_open() {
    let (c, s) = setup("build-finished", 1 << 40);
    let root = s.parent().unwrap().to_path_buf();
    let verdict = |n| ev(&c, n, "verdict", json!({"verdict": "FAIL", "sha": "a", "stage": "rev"}));
    spawn(&c, 13, "impl", "sendmeter-impl-13");
    spawn(&c, 13, "rev", "sendmeter-rev-13-r1");
    verdict(13);
    spawn(&c, 13, "impl", "sendmeter-impl-13-f1"); // the fix round's newer lane
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
    held.lock().unwrap(); // cargo building in #15's rev lane
    let wts = lanes.map(|a| blob(&c.worktrees.join("sendmeter"), &format!("{a}/src")));
    let open: Vec<_> = lanes[..6]
        .iter()
        .map(|a| (*a, if a.contains("-14-") { "working" } else { "idle" }, "/x", "p"))
        .collect();
    let r = Fake::default().reply("herdr agent list", &agents(&open));
    let out = budget(&c, &r);
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
    let all = drover::events::read(&c.state).unwrap();
    all.into_iter().filter(|e| e.kind == kind).collect()
}

/// Over `build_budget_gb`, scratch dirs count and the oldest unlocked one goes first.
#[test]
fn the_oldest_scratch_is_trimmed_over_budget() {
    let (c, s) = setup("scratch-trim", 6000);
    spawn(&c, 9, "rev", "sendmeter-rev-9-r1");
    spawn(&c, 13, "impl", "sendmeter-impl-13");
    let (old, new) = (blob(&s, "sendmeter-rev-9-r1"), blob(&s, "sendmeter-impl-13"));
    age(&old, 1_000);
    // #634: no live agent for either (spawned long ago); a live one's scratch is never trimmed (tests/shed.rs).
    let r = Fake::default().reply("herdr agent list", NO_AGENTS);
    let out = budget(&c, &r);
    assert!(!old.exists() && new.exists(), "{out}");
    assert!(out.contains("builds: trimmed"), "{out}");
}

/// `clean` lists stray Cargo folders older than a day in the stray roots; `--yes` removes them and the removed
/// worktree's Claude temp dir.
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

    let r = || {
        Fake::default()
            .reply("herdr agent list", NO_AGENTS)
            .reply(&format!("git -C {wt} rev-list"), "0\n")
            .reply("git -C", "")
            .reply("herdr", "{}")
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

/// #651: a remote host `air` whose build-folder commands run here, in a shell with `home` as `$HOME`, against real
/// folders: every ssh line naming its build root runs through `sh -c`, `df` answers `free_kb`, the rest (herdr, probes)
/// is `fake`'s.
struct Air {
    fake: Fake,
    free_kb: u64,
    home: PathBuf,
}

const AIR_ROOT: &str = "~/.cache/hs-651-air";

impl drover::runner::Runner for Air {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        let line = args.iter().position(|a| a == "--").map(|i| args[i + 1..].join(" "));
        let Some(line) = line.filter(|_| prog == "ssh" && args.iter().any(|a| a == "air")) else {
            return self.fake.run(prog, args);
        };
        self.fake.calls.borrow_mut().push(format!("ssh air -- {line}"));
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
            return self.fake.run(prog, args);
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
        match args.last() == Some(&batch) {
            true => self.fake.run_in(prog, args, input),
            false => self.run(prog, args),
        }
    }
    fn sleep(&self, _: u64) {}
}

/// A ctx whose fleet has the remote host `air` (workspace root `AIR_ROOT` there, absent here); air's `$HOME` and its
/// `.build/.scratch`.
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

fn tick(c: &drover::Ctx, home: &Path, air: &[(&str, &str, &str, &str)], free_kb: u64) -> (Air, String) {
    let fake = Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("herdr --machine air agent list", &agents(air))
        .reply(
            "herdr status",
            "client:\n  version: 0.9.5\n\nserver:\n  status: running\n  version: 0.9.5\n",
        )
        .reply("git -C", "");
    let r = Air {
        fake,
        free_kb,
        home: home.into(),
    };
    let (code, out) = run(c, &r, &["tick"]);
    assert_eq!(code, 0, "{out}");
    (r, out)
}

/// #651: one tick on a remote host with three scratch folders (a merged lane whose agent sits `done`, a working lane,
/// an orphan) removes the merged and orphan ones there, through its runner, and keeps the working one.
#[test]
fn a_remote_hosts_ended_and_orphan_scratch_goes_at_the_tick() {
    let (c, home, s) = air_setup("scratch-remote");
    air_spawn(&c, 12, "impl", "sendmeter-impl-12");
    merged(&c, 12);
    air_spawn(&c, 13, "impl", "sendmeter-impl-13");
    let [ended, working, orphan] =
        ["sendmeter-impl-12", "sendmeter-impl-13", "sendmeter-rev-9-r1"].map(|a| blob(&s, &format!("{a}/debug")));
    // #798: the lanes' build folders there too: the finished one goes, the working one and an unknown one stay.
    let b = s.parent().unwrap();
    let [built, building, unknown] =
        ["sendmeter-impl-12", "sendmeter-impl-13", "sendmeter-rev-9-r1"].map(|a| blob(b, &format!("{a}/debug")));
    let air = [
        ("sendmeter-impl-12", "done", "/x", "w1:p1"),
        ("sendmeter-impl-13", "working", "/x", "w2:p1"),
    ];
    let (r, out) = tick(&c, &home, &air, 200 * 1_048_576);
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
        r.fake
            .calls
            .borrow()
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

/// #651: under `min_free_gb` a host's scratch of every non-working lane goes, largest first, each logged with the host,
/// size and reason; a working lane's stays.
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
        ("sendmeter-impl-13", "idle", "/x", "w1:p1"),
        ("sendmeter-rev-13-r1", "done", "/x", "w2:p1"),
        ("sendmeter-impl-14", "working", "/x", "w3:p1"),
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

/// #678: `air` turned off for new lanes but answering: its merged lane #12 (worktree and scratch), #13's lane now live
/// on this host, #14's lane live on the Air.
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

/// #678: the disabled Air's runner: #12's worktree there clean and pushed, removable.
fn air_off_runner(home: &Path) -> Air {
    let wt = "/Users/me/.herdr/worktrees/sendmeter/sendmeter-impl-12";
    let ssh = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C -oControlPersist=10m air -- ";
    let fake = Fake::default()
        .reply(
            "herdr agent list",
            &agents(&[("sendmeter-impl-13", "working", "/x", "w5:p1")]),
        )
        .reply(
            "herdr --machine air agent list",
            &agents(&[("sendmeter-impl-14", "idle", "/y", "w2:p1")]),
        )
        .reply("gh issue list", "[]")
        .reply(&format!("{ssh}'sh' '-c' 'test -e"), "")
        .reply(&format!("{ssh}'git' '-C' '{wt}' 'status'"), "")
        .reply(&format!("{ssh}'git' '-C' '{wt}' 'rev-list'"), "0\n")
        .reply("herdr --machine air worktree remove", "{}")
        .reply("herdr --machine air pane close", "{}");
    Air {
        fake,
        free_kb: 200 * 1_048_576,
        home: home.into(),
    }
}

/// #678: an explicit `clean` sweeps a disabled host that answers: the dry run lists the merged lane's worktree and the
/// dead lane's scratch there, `--yes` removes both.
#[test]
fn clean_sweeps_a_disabled_but_reachable_host() {
    let (c, home, [dead, ..]) = air_off("scratch-off-clean");
    let wt = "/Users/me/.herdr/worktrees/sendmeter/sendmeter-impl-12";
    let (code, dry) = run(&c, &air_off_runner(&home), &["clean"]);
    assert_eq!(code, 0, "{dry}");
    assert!(!dry.contains("not cleaned"), "{dry}");
    assert!(
        dry.contains(&format!("would remove air:{wt} (pane w1:p1, sendmeter#12 merged)")),
        "{dry}"
    );
    assert!(
        dry.contains(&format!(
            "would remove scratch air:{AIR_ROOT}/.build/.scratch/sendmeter-impl-12"
        )),
        "{dry}"
    );
    assert!(dead.exists(), "a dry run removes nothing");
    let r = air_off_runner(&home);
    let (code, out) = run(&c, &r, &["clean", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains(&format!("removed air:{wt} (sendmeter#12 merged)")),
        "{out}"
    );
    assert!(
        r.fake.called("herdr --machine air worktree remove --workspace w1"),
        "{out}"
    );
    assert!(!dead.exists(), "{out}");
    assert!(
        out.contains(&format!(
            "scratch: air: removed {AIR_ROOT}/.build/.scratch/sendmeter-impl-12 "
        )),
        "{out}"
    );
}

/// #678: on a disabled host a lane's scratch stays only while an agent of that name is live there: one that moved to
/// another host loses its old copy (`reclaim --apply` too).
#[test]
fn a_disabled_hosts_scratch_stays_only_for_its_own_live_agents() {
    let (c, home, [dead, moved, live]) = air_off("scratch-off-reclaim");
    let r = air_off_runner(&home);
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
