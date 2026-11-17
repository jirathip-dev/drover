mod common;
use common::*;

#[test]
fn a_tests_dirs_go_when_it_passes_or_panics() {
    for fail in [false, true] {
        let (tx, rx) = std::sync::mpsc::channel();
        let t = std::thread::spawn(move || {
            let d = tmpdir("tmpdir-gone");
            std::fs::write(d.join("f"), "x").unwrap();
            tx.send(d).unwrap();
            assert!(!fail, "this test thread fails");
        });
        assert_eq!(t.join().is_err(), fail);
        let d = rx.recv().unwrap();
        assert!(!d.exists(), "{} left behind (fail: {fail})", d.display());
    }
}

#[test]
fn writer_outliving_its_test_child() {
    if std::env::var_os("FLEET_TMPDIR_WRITER").is_none() {
        return;
    }
    let d = tmpdir("tmpdir-late-writer");
    println!("dir={}", d.display());
    std::thread::spawn(move || loop {
        let _ = std::fs::create_dir_all(d.join("state"));
        let _ = std::fs::write(d.join("state/state.db"), "x");
        std::thread::sleep(std::time::Duration::from_micros(50));
    });
}

#[test]
fn a_dir_a_background_thread_still_writes_after_its_test_is_gone_once_the_process_ends() {
    for _ in 0..20 {
        let o = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "writer_outliving_its_test_child", "--nocapture"])
            .env("FLEET_TMPDIR_WRITER", "1")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let out = String::from_utf8(o.stdout).unwrap();
        let d = std::path::PathBuf::from(out.lines().find_map(|l| l.strip_prefix("dir=")).unwrap());
        assert!(!d.exists(), "{} left behind", d.display());
    }
}
