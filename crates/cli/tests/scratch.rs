//! #536: lanes' private scratch under `build_root`: drive deletes an ended lane's and trims the oldest over budget;
//! `clean` removes stray Cargo folders in the stray roots and a removed worktree's Claude temp dir.
mod common;
use common::*;
use herdsman::config::Config;
use herdsman::events::{append, Event};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

fn ev(c: &herdsman::Ctx, issue: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(10, "sendmeter", Some(issue), kind, d)).unwrap();
}

fn spawn(c: &herdsman::Ctx, issue: u64, role: &str, agent: &str) {
    let w = c.worktrees.join("sendmeter").join(agent);
    std::fs::create_dir_all(&w).unwrap();
    let d = json!({"role": role, "agent": agent, "worktree": w, "pane": agent, "runtime": "shed"});
    ev(c, issue, "spawn", d);
}

/// #651: a lane spawned on the remote host `air`.
fn air_spawn(c: &herdsman::Ctx, issue: u64, role: &str, agent: &str) {
    let d = json!({"role": role, "agent": agent, "worktree": format!("/Users/me/worktrees/sendmeter/{agent}"),
                   "pane": agent, "runtime": "shed", "host": "air", "machine": "air"});
    ev(c, issue, "spawn", d);
}

fn merged(c: &herdsman::Ctx, issue: u64) {
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

fn setup(name: &str, budget_bytes: u64) -> (herdsman::Ctx, PathBuf) {
    let d = tmpdir(name);
    let root = d.join("worktrees/.build"); // #630: the root's build folder
    let cfg = CONFIG.replace(
        "build_root = \"\"",
        &format!("build_budget_gb = {}", budget_bytes as f64 / 1_073_741_824.0),
    );
    (ctx(&d, &cfg), root.join(".scratch"))
}

/// `check::budget`'s output, its lanes on `sh` (attached to `c`).
fn budget(c: &herdsman::Ctx, sh: &FakeShed) -> String {
    let (mut out, f) = (vec![], Fake::default());
    herdsman::check::budget(c, &Config::load(&c.config).unwrap(), &sh.on(&f), &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

/// An ended lane's scratch goes at drive's pass (merged, superseded by a later round, or unknown); an open issue's
/// active lane, a live agent and a building one keep theirs.
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

/// Over `build_budget_gb`, scratch dirs count and the oldest unlocked one goes first.
#[test]
fn the_oldest_scratch_is_trimmed_over_budget() {
    let (c, s) = setup("scratch-trim", 6000);
    spawn(&c, 9, "rev", "sendmeter-rev-9-r1");
    spawn(&c, 13, "impl", "sendmeter-impl-13");
    let (old, new) = (blob(&s, "sendmeter-rev-9-r1"), blob(&s, "sendmeter-impl-13"));
    age(&old, 1_000);
    // #634: no live agent for either (spawned long ago); a live one's scratch is never trimmed (tests/shed.rs).
    let sh = FakeShed::new();
    sh.attach(&c);
    let out = budget(&c, &sh);
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

/// #651: a remote host `air` whose build-folder commands run here, in a shell with `home` as `$HOME`, against real
/// folders: every ssh line naming its build root runs through `sh -c`, `df` answers `free_kb`, its `shed connect` is
/// `over`'s fake shed for air, the rest (probes) is `over`'s fake.
struct Air<'a> {
    over: OverSsh<'a>,
    free_kb: u64,
    home: PathBuf,
}

const AIR_ROOT: &str = "~/.cache/hs-651-air";

impl herdsman::runner::Runner for Air<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<herdsman::runner::Out> {
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
            return Ok(herdsman::runner::Out {
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
        Ok(herdsman::runner::Out {
            code: o.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&o.stdout).into(),
            stderr: String::from_utf8_lossy(&o.stderr).into(),
        })
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<herdsman::runner::Out> {
        match args.iter().any(|a| a.contains("'shed' 'connect'")) {
            true => self.over.run_in(prog, args, input),
            false => self.run(prog, args),
        }
    }
    fn sleep(&self, _: u64) {}
}

/// A ctx whose fleet has the remote host `air` (workspace root `AIR_ROOT` there, absent here); air's `$HOME` and its
/// `.build/.scratch`.
fn air_setup(name: &str) -> (herdsman::Ctx, PathBuf, PathBuf) {
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

/// One tick, air's shed running the `(name, status)` lanes; the calls the fake saw, and the tick's output.
fn tick(c: &herdsman::Ctx, home: &Path, air: &[(&str, &str)], free_kb: u64) -> (Vec<String>, String) {
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
    let air = [("sendmeter-impl-12", "done"), ("sendmeter-impl-13", "working")];
    let (calls, out) = tick(&c, &home, &air, 200 * 1_048_576);
    assert!(!ended.exists() && !orphan.exists(), "{out}");
    assert!(working.exists(), "a working lane's scratch stays: {out}");
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
