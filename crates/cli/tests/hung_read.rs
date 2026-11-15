mod common;
use common::*;
use std::time::{Duration, Instant};

#[test]
fn a_read_that_blocks_times_out_within_the_limit() {
    let d = tmpdir("hung-read");
    std::env::set_var("HOME", d.join("home"));
    let fifo = d.join("files/shot.png");
    std::fs::create_dir_all(fifo.parent().unwrap()).unwrap();
    let made = std::process::Command::new("mkfifo").arg(&fifo).status().unwrap();
    assert!(made.success());
    let t = Instant::now();
    let e = drover::runner::read_file(&fifo).unwrap_err();
    let took = t.elapsed();
    assert_eq!(e.kind(), std::io::ErrorKind::TimedOut, "{e}");
    let limit = drover::runner::probe_wait();
    assert!(took >= limit && took < limit + Duration::from_secs(1), "{took:?}");
    let t = Instant::now();
    let again = drover::runner::read_file(&fifo).unwrap_err();
    assert_eq!(again.kind(), std::io::ErrorKind::TimedOut);
    assert!(t.elapsed() < Duration::from_millis(200), "{:?}", t.elapsed());
    let _ = std::fs::OpenOptions::new().write(true).open(&fifo);
}
