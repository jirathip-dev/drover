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
