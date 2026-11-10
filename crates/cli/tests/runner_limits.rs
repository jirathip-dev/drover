//! #713: every external call is time-limited (`Real` kills it at its limit, exit 124), and nothing in `src/` starts a
//! process but `crates/core/src/runner.rs`.
mod common;
use common::*;
use herdsman::runner::{argv, limit, Real, Runner};
use std::path::Path;
use std::time::{Duration, Instant};

/// An executable `dir/name` that sleeps 30 s.
fn sleeper(dir: &Path, name: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    std::fs::write(&p, "#!/bin/sh\nexec sleep 30\n").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p.to_string_lossy().into_owned()
}

#[test]
fn a_call_past_its_limit_is_cut_at_the_limit_with_124() {
    let prog = sleeper(&tmpdir("limit-for"), "slow");
    let t = Instant::now();
    let o = Real.run_for(&prog, &[], "", Duration::from_secs(1)).unwrap();
    let took = t.elapsed();
    assert_eq!(o.code, 124, "{o:?}");
    assert!(o.stderr.contains("no answer in 1s"), "{}", o.stderr);
    assert!(
        took >= Duration::from_secs(1) && took < Duration::from_secs(3),
        "{took:?}"
    );
}

#[test]
fn a_call_with_no_explicit_limit_is_bounded_by_the_table() {
    // codesign waits on the Keychain: 10 s, though the program would sleep 30.
    let prog = sleeper(&tmpdir("limit-default"), "codesign");
    let t = Instant::now();
    let o = Real.run(&prog, &argv(&["-v", "x"])).unwrap();
    let took = t.elapsed();
    assert_eq!(o.code, 124, "{o:?}");
    assert!(
        took >= Duration::from_secs(10) && took < Duration::from_secs(13),
        "{took:?}"
    );
}

#[test]
fn the_limits_table() {
    let secs = |prog: &str, args: &[&str]| limit(prog, &argv(args)).as_secs();
    assert_eq!(secs("ps", &["-o", "pid"]), 60, "the default");
    assert_eq!(secs("gh", &["pr", "view", "issue-9"]), 60);
    assert_eq!(secs("security", &["find-generic-password", "-s", "x"]), 10);
    assert_eq!(secs("/usr/bin/codesign", &["-v", "x"]), 10);
    assert_eq!(secs("herdr", &["--machine", "air", "agent", "list"]), 20);
    assert_eq!(secs("herdr", &["agent", "start", "a", "--timeout", "90000", "--"]), 120);
    assert_eq!(secs("git", &["-C", "/r", "fetch", "origin"]), 600);
    assert_eq!(secs("git", &["-C", "/r", "status"]), 60);
    assert_eq!(secs("cp", &["-cR", "a", "b"]), 600);
    assert_eq!(secs("cargo", &["build", "--release"]), 1800);
}

/// `file:line` of each `Command::new` under `root/crates/*/src/` but `crates/core/src/runner.rs`.
fn stray(root: &Path) -> Vec<String> {
    fn walk(d: &Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let mut files = vec![];
    for c in std::fs::read_dir(root.join("crates")).unwrap().flatten() {
        walk(&c.path().join("src"), &mut files);
    }
    files.sort();
    let mut hits = vec![];
    for f in files {
        let rel = f.strip_prefix(root).unwrap().to_string_lossy().into_owned();
        if rel == "crates/core/src/runner.rs" {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap();
        for (i, l) in text.lines().enumerate() {
            if l.contains("Command::new") && !l.trim_start().starts_with("//") {
                hits.push(format!("{rel}:{}", i + 1));
            }
        }
    }
    hits
}

#[test]
fn a_stray_command_new_in_src_is_named_by_file_and_line() {
    let d = tmpdir("stray-command");
    let put = |rel: &str, text: &str| {
        let p = d.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    put("crates/core/src/runner.rs", "let c = Command::new(prog);\n");
    put(
        "crates/core/src/x/mail.rs",
        "fn f() {\n    // Command::new in a comment\n    let c = std::process::Command::new(\"sendmail\");\n}\n",
    );
    put("crates/cli/tests/t.rs", "let c = Command::new(\"sh\");\n");
    put("crates/cli/build.rs", "let c = Command::new(\"git\");\n");
    assert_eq!(stray(&d), ["crates/core/src/x/mail.rs:3"]);
}

#[test]
fn nothing_in_src_starts_a_process_but_the_runner() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let hits = stray(&root);
    assert!(
        hits.is_empty(),
        "Command::new outside crates/core/src/runner.rs (use the Runner): {hits:?}"
    );
}

/// Each call's program and the limit it was given (0: none, the runner's own table).
#[derive(Default)]
struct Limits(std::sync::Mutex<Vec<(String, u64)>>);

impl Runner for Limits {
    fn run(&self, prog: &str, _: &[String]) -> anyhow::Result<herdsman::runner::Out> {
        self.0.lock().unwrap().push((prog.into(), 0));
        Ok(herdsman::runner::Out::default())
    }
    fn run_for(&self, prog: &str, _: &[String], _: &str, l: Duration) -> anyhow::Result<herdsman::runner::Out> {
        self.0.lock().unwrap().push((prog.into(), l.as_secs()));
        Ok(herdsman::runner::Out::default())
    }
}

#[test]
fn a_remote_call_keeps_the_limit_of_the_command_it_carries() {
    let r = Limits::default();
    let on = herdsman::hosts::On::new(&r, Some("air"));
    on.run("git", &argv(&["-C", "/r", "fetch", "origin"])).unwrap();
    on.run("cargo", &argv(&["install", "x"])).unwrap();
    on.run("sh", &argv(&["-c", "true"])).unwrap();
    on.run_for("sh", &argv(&["-c", "cargo install x"]), "", Duration::from_secs(1800))
        .unwrap();
    herdsman::hosts::On::new(&r, None)
        .run("git", &argv(&["status"]))
        .unwrap();
    let seen = r.0.lock().unwrap().clone();
    let want = [("ssh", 600), ("ssh", 1800), ("ssh", 60), ("ssh", 1800), ("git", 0)];
    assert_eq!(seen, want.map(|(p, s)| (p.to_string(), s)));
}

/// #776: a folder whose listing hangs (its disk stopped answering) counts as hung after the wait, and is never probed
/// again while that probe is still stuck.
#[test]
fn a_folder_whose_listing_hangs_is_hung_and_probed_once() {
    use herdsman::runner::hung;
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    use std::sync::Arc;
    let dir = tmpdir("hung-probe").to_string_lossy().into_owned();
    let probes = Arc::new(AtomicUsize::new(0));
    let slow = |n: Arc<AtomicUsize>| {
        move |_: &str| {
            (
                n.fetch_add(1, Relaxed),
                std::thread::sleep(Duration::from_secs(5)),
                None,
            )
                .2
        }
    };
    let t = Instant::now();
    assert!(hung(&dir, Duration::from_millis(300), slow(probes.clone())));
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    let t = Instant::now();
    assert!(hung(&dir, Duration::from_millis(300), slow(probes.clone())));
    assert!(
        t.elapsed() < Duration::from_millis(100),
        "the second call waits on nothing: {:?}",
        t.elapsed()
    );
    assert_eq!(probes.load(Relaxed), 1, "one probe thread, not one per call");
}

#[test]
fn a_folder_that_answers_is_not_hung() {
    let dir = tmpdir("hung-ok").to_string_lossy().into_owned();
    assert!(!herdsman::runner::hung(&dir, Duration::from_secs(2), |d| {
        drop(std::fs::read_dir(d));
        None
    }));
}

/// #776: `git -C <dir>` on a hung folder answers 124 at once instead of starting a child that never exits.
#[test]
fn a_git_call_on_a_hung_folder_fails_at_once() {
    let dir = tmpdir("hung-git").to_string_lossy().into_owned();
    assert!(herdsman::runner::hung(&dir, Duration::from_millis(100), |_| {
        std::thread::sleep(Duration::from_secs(5));
        None
    }));
    let t = Instant::now();
    let o = Real.run("git", &argv(&["-C", &dir, "status", "--porcelain"])).unwrap();
    assert_eq!(o.code, 124, "{o:?}");
    assert!(o.stderr.contains("not answering"), "{}", o.stderr);
    assert!(t.elapsed() < Duration::from_millis(200), "{:?}", t.elapsed());
}

/// #776: a healthy folder still runs git normally (the probe passes within its wait).
#[test]
fn a_git_call_on_a_healthy_folder_runs() {
    let dir = tmpdir("hung-healthy-git").to_string_lossy().into_owned();
    let o = Real
        .run("git", &argv(&["-C", &dir, "rev-parse", "--is-inside-work-tree"]))
        .unwrap();
    assert_ne!(o.code, 124, "{o:?}");
}

/// #776: once a call on a disk runs out of time, every folder on that disk is hung, even one whose listing still
/// answers (macOS serves it from memory while the files hang).
#[test]
fn a_disk_marked_hung_fails_every_folder_on_it() {
    use herdsman::runner::{hung, sicken};
    // A disk id no real disk has, fresh per run: the mark is shared on disk (~/.cache/herdsman/sick-disks).
    let dev = 0xDEAD_0776_0000_0000 | u64::from(std::process::id());
    let (a, b) = (
        tmpdir("sick-a").to_string_lossy().into_owned(),
        tmpdir("sick-b").to_string_lossy().into_owned(),
    );
    assert!(!hung(&a, Duration::from_secs(2), move |_| Some(dev)));
    assert!(!hung(&b, Duration::from_secs(2), move |_| Some(dev)));
    sicken(&a);
    let t = Instant::now();
    assert!(
        hung(&b, Duration::from_secs(2), move |_| Some(dev)),
        "the other folder on the same disk"
    );
    assert!(t.elapsed() < Duration::from_millis(100), "{:?}", t.elapsed());
}

/// #775: serve's file reads go through `read_file`: a readable file comes back, one in a hung folder is None at once.
#[test]
fn read_file_returns_bytes_and_gives_up_on_a_hung_folder() {
    let ok = tmpdir("read-ok");
    std::fs::write(ok.join("shot.png"), b"png").unwrap();
    assert_eq!(
        herdsman::runner::read_file(ok.join("shot.png")).as_deref(),
        Some(&b"png"[..])
    );
    let bad = tmpdir("read-hung");
    let dir = bad.to_string_lossy().into_owned();
    assert!(herdsman::runner::hung(&dir, Duration::from_millis(100), |_| {
        std::thread::sleep(Duration::from_secs(5));
        None
    }));
    let t = Instant::now();
    assert_eq!(herdsman::runner::read_file(bad.join("shot.png")), None);
    assert!(t.elapsed() < Duration::from_millis(200), "{:?}", t.elapsed());
}

/// #776: the mark is shared on disk, so a fresh process (each drive or tick run) sees a disk another one marked.
#[test]
fn a_disk_mark_is_seen_by_a_process_that_never_saw_the_timeout() {
    let dev = 0xDEAD_0777_0000_0000 | u64::from(std::process::id());
    let home = std::env::var("HOME").unwrap();
    let mark = std::path::Path::new(&home)
        .join(".cache/herdsman/sick-disks")
        .join(dev.to_string());
    std::fs::create_dir_all(mark.parent().unwrap()).unwrap();
    std::fs::write(&mark, b"").unwrap();
    let dir = tmpdir("sick-shared").to_string_lossy().into_owned();
    let hung = herdsman::runner::hung(&dir, Duration::from_secs(2), move |_| Some(dev));
    std::fs::remove_file(&mark).unwrap();
    assert!(hung, "a folder on a disk marked by another process");
}
