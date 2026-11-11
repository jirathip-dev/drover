//! #630: workspace roots (temp folders here): a new lane takes the first mounted root, `hosts eject` saves and
//! restarts a lane on the next one, a pulled root's lane restarts once from its pushed head, and `migrate` moves an
//! idle `~/.herdr/worktrees` worktree into `<root>/<repo>/<lane>`. git is real; gh and the host probes are fakes, the
//! lanes run on the fake shed.
mod common;
use common::*;
use herdsman::config::Config;
use herdsman::events::{append, read, Event};
use herdsman::runner::{Out, Real, Runner};
use serde_json::json;
use std::path::{Path, PathBuf};

const AGENT: &str = "sendmeter-impl-12";

/// git runs for real; the rest is the fake.
struct Git<'a>(&'a Fake);

impl Runner for Git<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        if prog == "git" || prog == "cp" {
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
    String::from_utf8(o.stdout).unwrap()
}

/// An origin with `staging` and a pushed `issue-12`, its clone (the fleet's `local`), the two roots, and the shed the
/// lanes run on.
struct Repo {
    c: herdsman::Ctx,
    sh: FakeShed,
    origin: PathBuf,
    local: PathBuf,
    nvme: PathBuf,
    internal: PathBuf,
}

fn repo(name: &str) -> Repo {
    let d = tmpdir(name);
    let (origin, local, nvme, internal) = (
        d.join("origin.git"),
        d.join("local"),
        d.join("nvme"),
        d.join("internal"),
    );
    for p in [&nvme, &internal] {
        std::fs::create_dir_all(p).unwrap();
    }
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q", "--bare"]);
    git(
        &d,
        &["clone", "-q", &origin.to_string_lossy(), &local.to_string_lossy()],
    );
    git(&local, &["checkout", "-q", "-b", "staging"]);
    std::fs::write(local.join("README"), "x\n").unwrap();
    git(&local, &["add", "README"]);
    let id = ["-c", "user.name=t", "-c", "user.email=t@t"];
    git(&local, &[&id[..], &["commit", "-q", "-m", "init"]].concat());
    git(&local, &["push", "-q", "origin", "staging", "staging:issue-12"]);
    git(&local, &["branch", "-q", "issue-12", "origin/issue-12"]);
    let roots = format!("roots = [\"{}\", \"{}\"]", nvme.display(), internal.display());
    let cfg = CONFIG
        .replace("harness  =  \"hermes\"", "harness = \"claude\"")
        .replace("/tmp/does-not-matter/sendmeter", &local.to_string_lossy())
        + &format!("\n[hosts.mini]\n{roots}\n");
    let c = ctx(&d, &cfg);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let sh = FakeShed::new();
    sh.attach(&c);
    Repo {
        c,
        sh,
        origin,
        local,
        nvme,
        internal,
    }
}

/// gh as a spawn on this host needs it; the lane, live on `r`'s shed, works in `cwd`.
fn fake(r: &Repo, cwd: &Path) -> Fake {
    let wt = cwd.join("sendmeter/issue-12");
    r.sh.lane(AGENT, "working", &wt.to_string_lossy());
    spawnable()
}

fn spawnable() -> Fake {
    Fake::default()
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("which shed", "/opt/shed/bin/shed\n")
}

/// Issue 12's impl lane working in the first root (its worktree made there, a file left uncommitted).
fn working_on_nvme(r: &Repo) -> PathBuf {
    let wt = r.nvme.join("sendmeter/issue-12");
    git(&r.local, &["worktree", "add", "-q", &wt.to_string_lossy(), "issue-12"]);
    std::fs::write(wt.join("wip.txt"), "half done\n").unwrap();
    let d = json!({"role": "impl", "agent": AGENT, "harness": "claude", "pane": "w1:p1", "workspace": "w1",
                   "worktree": wt, "branch": "issue-12", "host": "mini", "root": r.nvme});
    append(&r.c.state, &Event::new(10, "sendmeter", Some(12), "spawn", d)).unwrap();
    wt
}

fn spawns(c: &herdsman::Ctx) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "spawn")
        .collect()
}

#[test]
fn a_new_lane_lands_in_the_first_mounted_root_and_skips_a_missing_one() {
    let r = repo("roots-place");
    let f = fake(&r, &r.internal);
    let worktree = |c: &herdsman::Ctx| {
        let (code, out) = run(c, &f, &["spawn", "sendmeter", "12", "impl", "--dry-run", "--json"]);
        assert_eq!(code, 0, "{out}");
        serde_json::from_str::<serde_json::Value>(&out).unwrap()["worktree"].clone()
    };
    assert_eq!(worktree(&r.c), json!(r.nvme.join("sendmeter/issue-12")));
    std::fs::remove_dir(&r.nvme).unwrap(); // the drive is not plugged in
    assert_eq!(worktree(&r.c), json!(r.internal.join("sendmeter/issue-12")));
}

#[test]
fn eject_saves_and_restarts_a_live_lane_on_the_next_root_then_reports_safe() {
    let r = repo("roots-eject");
    let old = working_on_nvme(&r);
    let f = fake(&r, old.parent().unwrap().parent().unwrap());
    let (code, out) = run(&r.c, &Git(&f), &["hosts", "eject", "mini"]);
    assert_eq!(code, 0, "{out} {:?}", f.calls.borrow());
    assert!(out.contains(&format!("safe to unplug {}", r.nvme.display())), "{out}");
    // Saved: the uncommitted file is on `wip/<agent>-<stamp>` at origin, and the worktree is off the drive.
    let saved = out.lines().find_map(|l| l.strip_prefix("saved "));
    let wip = saved.and_then(|l| l.rsplit(" to ").next()).unwrap();
    assert!(wip.starts_with(&format!("wip/{AGENT}-1000000-")), "{out}");
    assert!(git(&r.origin, &["show", &format!("{wip}:wip.txt")]).contains("half done"));
    assert!(
        !old.exists()
            && r.nvme
                .read_dir()
                .unwrap()
                .all(|e| e.unwrap().file_name() == "sendmeter")
    );
    // Restarted from that save on the next root.
    let new = r.internal.join("sendmeter/issue-12");
    assert!(r.sh.called("kill", AGENT), "the live lane stopped first");
    assert_eq!(r.sh.last("run")["cwd"], json!(new), "and ran again there");
    let sp = spawns(&r.c);
    let last = sp.last().unwrap();
    assert_eq!((sp.len(), last.str("root")), (2, Some(&*r.internal.to_string_lossy())));
    assert_eq!(last.str("worktree"), Some(&*new.to_string_lossy()));
    assert_eq!(std::fs::read_to_string(new.join("wip.txt")).unwrap(), "half done\n");
    let list = git(&r.local, &["worktree", "list"]);
    assert!(
        list.contains(&*new.to_string_lossy()) && !list.contains(&*old.to_string_lossy()),
        "{list}"
    );
    // The ejected root takes no new lane while it stays mounted.
    let (_, dry) = run(&r.c, &f, &["spawn", "sendmeter", "12", "impl", "--dry-run", "--json"]);
    assert!(dry.contains(&*new.to_string_lossy()), "{dry}");
}

#[test]
fn a_pulled_root_is_noted_once_and_its_lane_restarts_on_the_next_root_from_its_pushed_head() {
    let r = repo("roots-pulled");
    working_on_nvme(&r);
    std::fs::remove_dir_all(&r.nvme).unwrap(); // pulled without eject
    let f = fake(&r, &r.nvme);
    let cfg = Config::load(&r.c.config).unwrap();
    let watch = || {
        let mut out = vec![];
        herdsman::roots::watch(&r.c, &cfg, &r.sh.on(&Git(&f)), &mut out).unwrap();
        String::from_utf8(out).unwrap()
    };
    let out = watch();
    let note = format!("! root {} gone", r.nvme.display());
    assert_eq!(out.matches(&note).count(), 1, "{out}");
    let sp = spawns(&r.c);
    assert_eq!((sp.len(), sp[1].str("root")), (2, Some(&*r.internal.to_string_lossy())));
    let new = r.internal.join("sendmeter/issue-12");
    let pushed = git(&r.origin, &["rev-parse", "issue-12"]);
    assert_eq!(git(&new, &["rev-parse", "HEAD"]), pushed, "from its pushed head");
    let ev = read(&r.c.state).unwrap();
    let moved = ev.iter().find(|e| e.kind == "root-moved").unwrap();
    assert!(
        moved
            .str("lost")
            .unwrap()
            .contains("uncommitted work, 0 commit(s) not pushed"),
        "{moved:?}"
    );
    // The next look: nothing new, no loop.
    assert_eq!(watch(), "");
    assert_eq!(spawns(&r.c).len(), 2);
    let gone = read(&r.c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "root-gone")
        .count();
    assert_eq!(gone, 1);
}

#[test]
fn migrate_moves_an_idle_worktree_into_the_new_layout() {
    let r = repo("roots-migrate");
    let old = r.c.home.join(".herdr/worktrees/sendmeter");
    let (idle, live) = (old.join("issue-12"), old.join("issue-13"));
    git(
        &r.local,
        &["worktree", "add", "-q", &idle.to_string_lossy(), "issue-12"],
    );
    git(
        &r.local,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "issue-13",
            &live.to_string_lossy(),
            "staging",
        ],
    );
    let cache = r.c.home.join(".cache/herdsman/target/sendmeter");
    std::fs::create_dir_all(&cache).unwrap();
    r.sh.lane("sendmeter-impl-13", "working", &live.to_string_lossy());
    let f = Fake::default();
    let (code, out) = run(&r.c, &Git(&f), &["migrate"]);
    assert_eq!(code, 0, "{out}");
    let to = r.c.worktrees.join("sendmeter/issue-12");
    assert!(to.join("README").is_file() && !idle.exists(), "{out}");
    let list = git(&r.local, &["worktree", "list"]);
    assert!(
        list.contains(&*to.to_string_lossy()) && !list.contains(&*idle.to_string_lossy()),
        "{list}"
    );
    assert!(live.exists() && out.contains("live, left for later"), "{out}");
    assert!(
        cache.exists(),
        "the old build caches stay while a worktree is left: {out}"
    );
    // Once that lane has ended, the next migrate moves it and drops the old caches.
    r.sh.exit("sendmeter-impl-13");
    let (code, out) = run(&r.c, &Git(&Fake::default()), &["migrate"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        r.c.worktrees.join("sendmeter/issue-13/README").is_file() && !cache.exists(),
        "{out}"
    );
}

/// Amendment A: a new lane's build folder `<root>/.build/<agent>` is a clone of its fleet's newest one on that root
/// (another fleet's never), the files `.herdsman/worktree-include` lists come from the main checkout, and the lane's
/// unsaved work shows in `herdsman status` and the Lanes tab's rows.
#[test]
fn a_new_lane_clones_its_fleets_warmest_build_on_its_root_and_gets_the_included_files() {
    let r = repo("roots-warm");
    let b = r.nvme.join(".build");
    for (d, f) in [
        ("sendmeter-impl-9", "old"),
        ("sendmeter-rev-11-r1", "warm"),
        ("other-impl-1", "theirs"),
    ] {
        std::fs::create_dir_all(b.join(d).join("release")).unwrap();
        std::fs::write(b.join(d).join("release").join(f), f).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100)); // mtimes in seconds: each newer
    }
    std::fs::write(r.local.join(".herdsman-ignored.env"), "PORT=1\n").unwrap();
    std::fs::write(r.local.join(".git/info/exclude"), ".herdsman-ignored.env\n").unwrap();
    std::fs::create_dir_all(r.local.join(".herdsman")).unwrap();
    std::fs::write(
        r.local.join(".herdsman/worktree-include"),
        "# setup\n.herdsman-ignored.env\n../escape\nmissing\n",
    )
    .unwrap();
    let f = spawnable();
    let (code, out) = run(&r.c, &Git(&f), &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "{out}");
    let mine = b.join(AGENT).join("release");
    let ev = read(&r.c.state).unwrap();
    let cl = ev.iter().find(|e| e.kind == "build-cloned").unwrap();
    assert_eq!(cl.str("from"), Some(&*b.join("sendmeter-rev-11-r1").to_string_lossy()));
    if reflinks(&b) {
        assert!(mine.join("warm").is_file(), "{out}");
        assert!(!mine.join("old").exists() && !mine.join("theirs").exists());
    } else {
        // A volume without copy-on-write (CI's ext4): the clone fails and nothing is fully copied instead.
        assert!(!mine.join("warm").exists(), "{out}");
        assert_eq!(cl.data["errors"].as_array().map(Vec::len), Some(1), "{out}");
    }
    let target = format!("{}'", b.join(AGENT).display());
    let sent = r.sh.sent(AGENT);
    assert!(
        sent.iter().any(|c| c.contains("TARGET_DIR=") && c.contains(&target)),
        "{target} {sent:?}"
    );
    let wt = r.nvme.join("sendmeter/issue-12");
    assert_eq!(
        std::fs::read_to_string(wt.join(".herdsman-ignored.env")).unwrap(),
        "PORT=1\n"
    );
    // Unsaved work: one new file, one local commit.
    std::fs::write(wt.join("new.txt"), "x").unwrap();
    std::fs::write(wt.join("README"), "y\n").unwrap();
    let id = ["-c", "user.name=t", "-c", "user.email=t@t"];
    git(&wt, &[&id[..], &["commit", "-qam", "local"]].concat());
    let (_, st) = run(&r.c, &Git(&f), &["status"]);
    assert!(st.contains(" · +1 ⇡1"), "{st}");
    let cfg = Config::load(&r.c.config).unwrap();
    // The lane the spawn started, as shed lists it.
    let a = herdsman::herdr::agents(&r.sh.on(&Git(&f))).unwrap();
    assert_eq!(a[0]["cwd"], json!(wt), "{a:?}");
    let st = herdsman::events::states(&r.c.state).unwrap();
    let ls = herdsman::lanes::join(&cfg, &Git(&f), &r.c.worktrees, &ev, &st, &a, &Default::default());
    let l = ls.iter().find(|l| l.name == AGENT).unwrap();
    let u = l.unsaved(&Git(&f));
    assert_eq!(
        (u, herdsman::spawn::unsaved_text(u).as_deref()),
        (Some((1, 1)), Some("+1 ⇡1"))
    );
}

/// Whether folder `d`'s volume clones copy-on-write, by the same `cp` flags the spawn uses.
fn reflinks(d: &std::path::Path) -> bool {
    let (src, dst) = (d.join(".probe"), d.join(".probe-clone"));
    std::fs::write(&src, "p").unwrap();
    let flag = if cfg!(target_os = "macos") {
        "-c"
    } else {
        "--reflink=always"
    };
    let ok = std::process::Command::new("cp")
        .arg(flag)
        .arg(&src)
        .arg(&dst)
        .status()
        .is_ok_and(|s| s.success());
    let _ = (std::fs::remove_file(&src), std::fs::remove_file(&dst));
    ok
}

/// #790: a root gone under a live lane: the next drive pass restarts the lane on the other root at its pushed head,
/// once, with one `root-moved` naming both roots; `waiting` no longer says "root gone".
#[test]
fn a_drive_pass_restarts_a_lane_whose_root_went_away_on_the_next_root_once() {
    let r = repo("roots-drive");
    working_on_nvme(&r);
    std::fs::remove_dir_all(&r.nvme).unwrap(); // force-unmounted
    let f = fake(&r, &r.internal);
    let (code, out) = run(&r.c, &Git(&f), &["drive"]);
    assert_eq!(code, 0, "{out}");
    let sp = spawns(&r.c);
    assert_eq!(
        (sp.len(), sp.last().unwrap().str("root")),
        (2, Some(&*r.internal.to_string_lossy())),
        "{out}"
    );
    let new = r.internal.join("sendmeter/issue-12");
    assert_eq!(
        git(&new, &["rev-parse", "HEAD"]),
        git(&r.origin, &["rev-parse", "issue-12"])
    );
    // The next pass: nothing new.
    let (code, out) = run(&r.c, &Git(&f), &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(spawns(&r.c).len(), 2, "{out}");
    let ev = read(&r.c.state).unwrap();
    let moved: Vec<&Event> = ev.iter().filter(|e| e.kind == "root-moved").collect();
    assert_eq!(moved.len(), 1, "{moved:?}");
    assert_eq!(moved[0].str("root"), Some(&*r.nvme.to_string_lossy()));
    assert_eq!(moved[0].str("to"), Some(&*r.internal.to_string_lossy()));
    let (code, out) = run(&r.c, &Git(&f), &["waiting"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("sendmeter #12") && !out.contains("root gone"), "{out}");
}

/// #790: a restart that fails (here gh answers nothing) logs no move and is tried again on the next pass.
#[test]
fn a_failed_restart_off_a_gone_root_is_retried_on_the_next_drive_pass() {
    let r = repo("roots-retry");
    working_on_nvme(&r);
    std::fs::remove_dir_all(&r.nvme).unwrap();
    let down = Fake::default().reply("which shed", "/opt/shed/bin/shed\n"); // gh answers nothing: the start fails
    let (code, out) = run(&r.c, &Git(&down), &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&format!("{AGENT}: restart failed")), "{out}");
    let moved = |c: &herdsman::Ctx| {
        read(&c.state)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "root-moved")
            .count()
    };
    assert_eq!((spawns(&r.c).len(), moved(&r.c)), (1, 0), "{out}");
    let (code, out) = run(&r.c, &Git(&fake(&r, &r.internal)), &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!((spawns(&r.c).len(), moved(&r.c)), (2, 1), "{out}");
}

#[test]
fn a_second_rev_round_reuses_the_first_rounds_build_folder_and_only_the_first_clones() {
    let r = repo("roots-reuse");
    let b = r.nvme.join(".build");
    std::fs::create_dir_all(b.join("sendmeter-impl-12/release")).unwrap();
    std::fs::write(b.join("sendmeter-impl-12/release/warm"), "w").unwrap();
    git(&r.local, &["branch", "-q", "rev-12-r1", "origin/issue-12"]);
    git(&r.local, &["branch", "-q", "rev-12-r2", "origin/issue-12"]);
    let f = spawnable(Fake::default().reply("herdr agent list", NO_AGENTS));
    let (code, out) = run(&r.c, &Git(&f), &["spawn", "sendmeter", "12", "rev"]);
    assert_eq!(code, 0, "{out}");
    let cp = |f: &Fake| {
        f.calls
            .borrow()
            .iter()
            .filter(|c| c.starts_with("git -cR") || c.starts_with("git -R"))
            .count()
    };
    assert_eq!(cp(&f), 1, "the first round clones: {:?}", f.calls.borrow());
    let ev = read(&r.c.state).unwrap();
    assert!(ev.iter().any(|e| e.kind == "build-cloned") && !ev.iter().any(|e| e.kind == "build-reused"));

    let r1 = b.join("sendmeter-rev-12-r1");
    std::fs::create_dir_all(r1.join("release")).unwrap();
    std::fs::write(r1.join("release/built"), "r1").unwrap();
    let verdict = json!({"verdict": "FAIL", "sha": "a".repeat(40), "body": "FAIL"});
    append(
        &r.c.state,
        &Event::new(r.c.now, "sendmeter", Some(12), "verdict", verdict),
    )
    .unwrap();
    let f = spawnable(Fake::default().reply("herdr agent list", NO_AGENTS));
    let (code, out) = run(&r.c, &Git(&f), &["spawn", "sendmeter", "12", "rev"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(cp(&f), 0, "the second round copies nothing: {:?}", f.calls.borrow());
    let r2 = b.join("sendmeter-rev-12-r2");
    assert_eq!(std::fs::read_to_string(r2.join("release/built")).unwrap(), "r1");
    assert!(!r1.exists());
    let ev = read(&r.c.state).unwrap();
    let e = ev
        .iter()
        .find(|e| e.kind == "build-reused")
        .expect("build-reused logged");
    assert_eq!(e.str("from"), Some("sendmeter-rev-12-r1"));
    assert_eq!(e.data["dirs"], json!([r2.display().to_string()]));
    assert_eq!(ev.iter().filter(|e| e.kind == "build-cloned").count(), 1);
}
