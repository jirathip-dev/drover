mod common;
use common::*;
use drover::runner::{argv, hung, Real, Runner};
use std::time::Duration;

fn overloaded() {
    let d = tmpdir("overloaded");
    let c = ctx(&d, &CONFIG.replace("[limits]\n", "[limits]\nload_per_cpu = -1.0\n"));
    drover::config::Config::load(&c.config).unwrap();
}

fn slow_probe(dir: &str, secs: u64) -> std::thread::JoinHandle<bool> {
    let (began, rx) = std::sync::mpsc::channel();
    let owned = dir.to_string();
    let t = std::thread::spawn(move || {
        hung(&owned, Duration::from_secs(2), move |d| {
            began.send(()).unwrap();
            std::thread::sleep(Duration::from_secs(secs));
            drop(std::fs::read_dir(d));
            None
        })
    });
    rx.recv().unwrap();
    t
}

#[test]
fn on_an_overloaded_host_a_slow_listing_waits_and_the_git_call_runs() {
    overloaded();
    let dir = tmpdir("overloaded-slow").to_string_lossy().into_owned();
    let first = slow_probe(&dir, 3);
    let o = Real
        .run("git", &argv(&["-C", &dir, "rev-parse", "--is-inside-work-tree"]))
        .unwrap();
    assert_ne!(o.code, 124, "{o:?}");
    assert!(!o.stderr.contains("not answering"), "{}", o.stderr);
    assert!(!first.join().unwrap(), "the slow listing is not a hung disk");
}
