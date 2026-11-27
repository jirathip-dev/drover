use super::*;
use drover::runner::{Out, Runner};

struct RealClone<'a>(remote::Acp<'a, OverSsh<'a>>);

impl Runner for RealClone<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        match args.last().filter(|l| prog == "ssh" && l.contains("reflink")) {
            Some(l) => {
                self.0.r.f.calls.borrow_mut().push(format!("{prog} {}", args.join(" ")));
                drover::runner::Real.run("sh", &drover::runner::argv(&["-c", l]))
            }
            None => self.0.run(prog, args),
        }
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        if input.is_empty() {
            return self.run(prog, args);
        }
        self.0.run_in(prog, args, input)
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
}

#[cfg(target_os = "linux")]
fn reflinks(d: &std::path::Path) -> bool {
    let (src, dst) = (d.join(".probe"), d.join(".probe-clone"));
    std::fs::write(&src, "p").unwrap();
    let ok = std::process::Command::new("cp")
        .args(["--reflink=always", &*src.to_string_lossy(), &*dst.to_string_lossy()])
        .status()
        .is_ok_and(|s| s.success());
    std::fs::remove_file(&src).ok();
    std::fs::remove_file(&dst).ok();
    ok
}

fn remote_root(name: &str) -> (drover::Ctx, std::path::PathBuf) {
    let c = setup(name, "", true, 0);
    let root = c.state.parent().unwrap().parent().unwrap().join("bazzite-root");
    std::fs::create_dir_all(&root).unwrap();
    let cfg = std::fs::read_to_string(&c.config).unwrap() + &format!("root = \"{}\"\n", root.display());
    std::fs::write(&c.config, cfg).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    (c, root)
}

#[cfg(target_os = "linux")]
#[test]
fn a_remote_lane_clones_its_fleets_warmest_build_on_its_hosts_root() {
    let (c, root) = remote_root("hosts-warm");
    let b = root.join(".build");
    for (d, f) in [
        ("sendmeter-impl-9", "old"),
        ("sendmeter-rev-11-r1", "warm"),
        ("other-impl-1", "theirs"),
    ] {
        std::fs::create_dir_all(b.join(d).join("release")).unwrap();
        std::fs::write(b.join(d).join("release").join(f), f).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100));
    }
    let (_sh, bazzite, f) = (FakeShed::attached(&c), FakeShed::new(), remote_fake());
    let (code, out) = run(&c, &RealClone(acp(&f, &bazzite)), &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "{out} {:?}", f.calls.borrow());
    assert!(bazzite.has(IMPL), "{:?}", bazzite.methods());
    let calls = f.calls.borrow();
    let clone: Vec<_> = calls.iter().filter(|c| c.contains("reflink")).collect();
    assert_eq!(clone.len(), 1, "{calls:?}");
    assert!(clone[0].starts_with(SSH), "the clone runs on bazzite: {clone:?}");
    let ev = read(&c.state).unwrap();
    let cl = ev
        .iter()
        .find(|e| e.kind == "build-cloned")
        .expect("build-cloned logged");
    assert_eq!(cl.str("from"), Some(&*b.join("sendmeter-rev-11-r1").to_string_lossy()));
    let mine = b.join(IMPL).join("release");
    if reflinks(&b) {
        assert!(mine.join("warm").is_file(), "{out}");
        assert!(!mine.join("old").exists() && !mine.join("theirs").exists());
    } else {
        assert!(!b.join(IMPL).exists(), "{out}");
        let rel = cl.str("errors_ref").expect("the clone's error in cache/");
        assert!(drover::events::path_in(&c.state, rel).is_file(), "{out}");
    }
    assert!(ev.iter().any(|e| e.kind == "spawn"), "{out}");
}

#[test]
fn a_remote_lane_with_no_sibling_build_still_spawns_and_logs_no_clone() {
    let (c, root) = remote_root("hosts-warm-cold");
    std::fs::create_dir_all(root.join(".build/other-impl-1")).unwrap();
    let (_sh, bazzite, f) = (FakeShed::attached(&c), FakeShed::new(), remote_fake());
    let (code, out) = run(&c, &RealClone(acp(&f, &bazzite)), &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "{out}");
    let ev = read(&c.state).unwrap();
    assert!(ev.iter().all(|e| e.kind != "build-cloned"), "{out}");
    assert!(ev.iter().any(|e| e.kind == "spawn"), "{out}");
    assert!(!root.join(".build").join(IMPL).exists());
}
