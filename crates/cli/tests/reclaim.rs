mod common;
use common::*;
use drover::events::{append, Event};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

const NOW: u64 = 1_800_000_000;
const DAY: u64 = 86_400;

fn lane(c: &drover::Ctx, issue: u64) -> PathBuf {
    let w = c.worktrees.join("sendmeter").join(format!("issue-{issue}"));
    for d in ["target", "node_modules", ".cache", "build", "src"] {
        std::fs::create_dir_all(w.join(d)).unwrap();
        std::fs::write(w.join(d).join("blob"), vec![0u8; 4096]).unwrap();
    }
    let agent = format!("sendmeter-impl-{issue}");
    let d = json!({"role": "impl", "agent": agent, "worktree": w, "pane": "p1"});
    append(&c.state, &Event::new(10, "sendmeter", Some(issue), "spawn", d)).unwrap();
    w
}

fn touch(p: &Path, at: u64) {
    let f = std::fs::File::open(p).unwrap();
    f.set_modified(UNIX_EPOCH + Duration::from_secs(at)).unwrap();
}

fn git(r: Fake, w: &Path, head_at: u64, status: &str) -> Fake {
    let w = w.display();
    r.reply(&format!("git -C {w} log -g"), &format!("HEAD@{{{head_at}}}\n"))
        .reply(&format!("git -C {w} status"), status)
}

fn setup(name: &str) -> drover::Ctx {
    let cfg = CONFIG.replace(
        "[fleet.sendmeter.role.impl]",
        "build_dirs = [\"build\"]\n\n[fleet.sendmeter.role.impl]",
    );
    let mut c = ctx(&tmpdir(name), &cfg);
    c.now = NOW;
    c
}

fn gone(w: &Path) -> Vec<bool> {
    ["target", "node_modules", ".cache", "build", "src"]
        .map(|d| !w.join(d).exists())
        .to_vec()
}

#[test]
fn preview_changes_nothing_and_apply_removes_only_build_folders() {
    let c = setup("reclaim-apply");
    let w = lane(&c, 12);
    let old = NOW - 8 * DAY;
    std::fs::write(w.join("notes.md"), "x").unwrap();
    touch(&w.join("notes.md"), old);
    touch(&w, NOW);
    let sh = FakeShed::new();
    sh.attach(&c);
    let r = || {
        git(Fake::default(), &w, old, "?? notes.md\0")
            .reply("lsof", "")
            .reply("git -C", "")
    };
    let (code, out) = run(&c, &r(), &["reclaim"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("would remove") && out.contains("nothing changed"), "{out}");
    assert!(out.contains("APFS clones may free less"), "{out}");
    assert_eq!(gone(&w), [false; 5], "preview removes nothing");

    let (code, out) = run(&c, &r(), &["reclaim", "--apply"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("freed"), "{out}");
    assert_eq!(gone(&w), [true, true, true, true, false], "src stays");
    assert!(w.join("notes.md").exists(), "untracked work stays");
}

#[test]
fn a_y_answer_applies() {
    let c = setup("reclaim-y");
    let w = lane(&c, 12);
    touch(&w, NOW);
    let sh = FakeShed::new();
    sh.attach(&c);
    let r = git(Fake::default(), &w, NOW - 8 * DAY, "")
        .reply("lsof", "")
        .reply("git -C", "");
    let mut out = Vec::new();
    let argv = ["drover", "reclaim"].map(String::from).to_vec();
    let code = drover::main_with(argv, &c, &r, &mut out, &mut &b"y\n"[..]);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&out));
    assert_eq!(gone(&w), [true, true, true, true, false]);
}

#[test]
fn busy_or_recent_worktrees_are_skipped() {
    let c = setup("reclaim-skip");
    let (proc_wt, live, edited, moved) = (lane(&c, 1), lane(&c, 2), lane(&c, 3), lane(&c, 4));
    let deleted = lane(&c, 5);
    let old = NOW - 8 * DAY;
    std::fs::write(edited.join("src/lib.rs"), "x").unwrap();
    touch(&edited.join("src/lib.rs"), NOW - DAY);
    touch(&deleted.join("src"), NOW - DAY);
    let lsof = format!("p42\ncnode\nn{}/src\n", proc_wt.display());
    let r = git(Fake::default(), &proc_wt, old, "");
    let r = git(r, &live, old, "");
    let r = git(r, &edited, old, " M src/lib.rs\0");
    let r = git(r, &deleted, old, " D src/gone.rs\0");
    let sh = FakeShed::new();
    sh.attach(&c);
    sh.lane("sendmeter-impl-2", "idle", "/elsewhere");
    let r = git(r, &moved, NOW - 2 * DAY, "")
        .reply("lsof", &lsof)
        .reply("git -C", "");
    let (code, out) = run(&c, &r, &["reclaim", "--apply"]);
    assert_eq!(code, 0, "{out}");
    for (w, why) in [
        (&proc_wt, "a process runs there"),
        (&live, "a live lane"),
        (&edited, "touched"),
        (&deleted, "touched"),
        (&moved, "touched"),
    ] {
        assert!(out.contains(&format!("skip {}: {why}", w.display())), "{out}");
        assert_eq!(gone(w), [false; 5], "{}", w.display());
    }
    assert!(out.contains("0.00 GB freed"), "{out}");
}

struct Held(std::process::Child);

impl Drop for Held {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

fn sqlite_holding(db: &Path) -> Held {
    use std::io::BufRead;
    let py = "import sqlite3,sys\nc=sqlite3.connect(sys.argv[1],isolation_level=None)\n\
              c.execute('PRAGMA locking_mode=EXCLUSIVE');c.execute('BEGIN EXCLUSIVE')\n\
              c.execute('CREATE TABLE t(x)');print('held',flush=True);sys.stdin.read()";
    let mut c = std::process::Command::new("python3")
        .args(["-I", "-c", py])
        .arg(db)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    std::io::BufReader::new(c.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let held = Held(c);
    assert_eq!(line.trim(), "held");
    held
}

#[test]
fn swift_build_folders_are_trimmed_past_the_budget_except_one_xcodebuild_or_swiftpm_holds() {
    let d = tmpdir("reclaim-swift");
    let root = d.join("worktrees/.build");
    let cfg = CONFIG.replace("build_root = \"\"", "build_budget_gb = 0");
    let c = ctx(&d, &cfg);
    let folder = |p: &str| {
        let dir = root.join(p);
        std::fs::create_dir_all(dir.join("Build")).unwrap();
        std::fs::write(dir.join("Build/blob"), vec![0u8; 4096]).unwrap();
        dir
    };
    let xcode = folder("sendmeter-impl-1/xcode");
    let db = xcode.join("Build/Intermediates.noindex/XCBuildData/build.db");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let spm = folder("sendmeter-impl-2/swiftpm");
    std::fs::write(spm.join(".lock"), "").unwrap();
    let flock = std::fs::File::open(spm.join(".lock")).unwrap();
    flock.lock().unwrap();
    let idle = [folder("other-impl-3/xcode"), folder("other-impl-3/swiftpm")];
    let sqlite = sqlite_holding(&db);
    let sh = FakeShed::new();
    sh.attach(&c);
    let (code, out) = run(&c, &Fake::default(), &["reclaim", "--apply"]);
    assert_eq!(code, 0, "{out}");
    assert!(idle.iter().all(|d| !d.exists()), "{out}");
    for d in [&xcode, &spm] {
        assert!(d.join("Build/blob").exists(), "{}: {out}", d.display());
        assert!(
            out.contains(&format!("keep {}: swift is building", d.display())),
            "{out}"
        );
    }
    drop((sqlite, flock));
    let (code, out) = run(&c, &Fake::default(), &["reclaim", "--apply"]);
    assert_eq!(code, 0, "{out}");
    assert!(!xcode.exists() && !spm.exists(), "released, they go: {out}");
}

#[test]
fn the_shared_build_folder_is_trimmed_past_its_budget_except_a_building_profile() {
    let d = tmpdir("reclaim-shared");
    let root = d.join("worktrees/.build");
    let cfg = CONFIG.replace("build_root = \"\"", "build_budget_gb = 0");
    let c = ctx(&d, &cfg);
    let profile = |p: &str| {
        let dir = root.join(p);
        std::fs::create_dir_all(dir.join("deps")).unwrap();
        std::fs::write(dir.join("deps/blob"), vec![0u8; 4096]).unwrap();
        std::fs::write(dir.join(".cargo-lock"), "").unwrap();
        dir
    };
    let (building, idle) = (profile("sendmeter/debug"), profile("other/debug"));
    let held = std::fs::File::open(building.join(".cargo-lock")).unwrap();
    held.lock().unwrap();
    let sh = FakeShed::new();
    sh.attach(&c);
    let r = || Fake::default().reply("lsof", "");
    let (code, out) = run(&c, &r(), &["reclaim"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("would trim") && idle.exists(),
        "preview removes nothing: {out}"
    );
    let (code, out) = run(&c, &r(), &["reclaim", "--apply"]);
    assert_eq!(code, 0, "{out}");
    assert!(!idle.exists(), "{out}");
    assert!(
        building.join("deps/blob").exists() && out.contains("cargo is building"),
        "{out}"
    );
}

#[test]
fn over_build_budget_an_idle_live_lanes_folder_is_trimmed_but_a_working_or_building_one_stays() {
    let d = tmpdir("reclaim-idle-live");
    let root = d.join("worktrees/.build");
    let cfg = CONFIG.replace("build_root = \"\"", "build_budget_gb = 0");
    let c = ctx(&d, &cfg);
    let ev = |issue: u64, kind: &str, d: serde_json::Value| {
        append(&c.state, &Event::new(10, "sendmeter", Some(issue), kind, d)).unwrap();
    };
    let spawn = |issue: u64, role: &str| {
        let agent = format!("sendmeter-{role}-{issue}");
        ev(issue, "spawn", json!({"role": role, "agent": agent, "pane": "p1"}));
    };
    let profile = |agent: &str| {
        let dir = root.join(agent).join("debug");
        std::fs::create_dir_all(dir.join("deps")).unwrap();
        std::fs::write(dir.join("deps/blob"), vec![0u8; 4096]).unwrap();
        std::fs::write(dir.join(".cargo-lock"), "").unwrap();
        dir
    };
    spawn(1, "impl");
    ev(1, "needs-human", json!({"why": "ROUND_LIMIT"}));
    spawn(2, "impl");
    ev(2, "parked", json!({"why": "later"}));
    spawn(3, "impl");
    spawn(3, "rev");
    spawn(6, "impl");
    spawn(6, "rev");
    ev(6, "verdict", json!({"sha": "a".repeat(40), "verdict": "PASS"}));
    spawn(4, "impl");
    spawn(5, "impl");
    ev(5, "needs-human", json!({"why": "ROUND_LIMIT"}));
    let sh = FakeShed::attached(&c);
    let agents = ["impl-1", "impl-2", "impl-3", "impl-4", "impl-5", "rev-3", "rev-6"].map(|a| format!("sendmeter-{a}"));
    for a in &agents {
        sh.lane(a, "idle", "/x");
    }
    let idle = [
        (profile(&agents[0]), "needs-human"),
        (profile(&agents[1]), "parked"),
        (profile(&agents[2]), "stage moved on"),
    ];
    let (working, reviewing, reviewed) = (profile(&agents[3]), profile(&agents[5]), profile(&agents[6]));
    let building = profile(&agents[4]);
    let held = std::fs::File::open(building.join(".cargo-lock")).unwrap();
    held.lock().unwrap();
    let r = Fake::default().reply("git -C", "").reply("lsof", "");
    let mut out = vec![];
    let config = drover::config::Config::load(&c.config).unwrap();
    drover::check::budget(&c, &config, &sh.on(&r), &mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    for (dir, why) in &idle {
        assert!(!dir.exists(), "{}: {out}", dir.display());
        assert!(out.contains(&format!("builds: trimmed {}", dir.display())), "{out}");
        let line = out.lines().find(|l| l.contains(&*dir.to_string_lossy())).unwrap();
        assert!(line.ends_with(&format!("idle: {why})")), "{line}");
    }
    assert!(!reviewed.exists(), "a rev lane with a verdict builds nothing: {out}");
    for w in [&working, &reviewing] {
        assert!(w.join("deps/blob").exists(), "{out}");
        assert!(out.contains(&format!("keep {}: a live lane", w.display())), "{out}");
    }
    assert!(building.join("deps/blob").exists(), "{out}");
    assert!(
        out.contains(&format!("keep {}: cargo is building", building.display())),
        "{out}"
    );

    let again = [profile(&agents[0]), profile(&agents[2])];
    let (code, out) = run(&c, &r, &["reclaim", "--apply"]);
    assert_eq!(code, 0, "{out}");
    assert!(again.iter().all(|d| !d.exists()), "reclaim uses the same rule: {out}");
    assert!(
        working.join("deps/blob").exists() && building.join("deps/blob").exists(),
        "{out}"
    );
}
