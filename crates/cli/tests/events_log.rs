//! AC6: the event log survives kill -9 mid-write. #344: the log is `state.db`; a JSONL log from before is imported.
mod common;
use common::tmpdir;
use herdsman::events::{append, fold, read, Event};
use serde_json::json;

fn ev(i: u64) -> Event {
    Event::new(
        i,
        "sendmeter",
        Some(7),
        "spawn",
        json!({"role": "impl", "agent": format!("a{i}"), "pad": "x".repeat(4000)}),
    )
}

#[test]
fn torn_last_line_is_ignored_and_next_append_is_clean() {
    let d = tmpdir("torn");
    let p = d.join("events.jsonl");
    // An old log whose writer was killed half-way through record 3.
    let line = |i| serde_json::to_string(&ev(i)).unwrap();
    std::fs::write(&p, format!("{}\n{}\n{}", line(1), line(2), &line(3)[..100])).unwrap();
    assert_eq!(read(&p).unwrap().len(), 2);
    append(&p, &ev(4)).unwrap();
    let all = read(&p).unwrap();
    assert_eq!(all.iter().map(|e| e.ts).collect::<Vec<_>>(), vec![1, 2, 4]);
    assert_eq!(fold(&all)[&("sendmeter".to_string(), 7)].lane.as_deref(), Some("a4"));
}

/// Child mode: append forever until killed.
#[test]
fn kill9_child_writer() {
    let Ok(p) = std::env::var("FLEET_KILL_CHILD") else {
        return;
    };
    let mut i = 0;
    loop {
        append(std::path::Path::new(&p), &ev(i)).unwrap();
        i += 1;
    }
}

#[test]
fn kill9_mid_append_never_corrupts() {
    let d = tmpdir("kill9");
    let p = d.join("events.jsonl");
    for round in 0..3 {
        let start = read(&p).unwrap().len();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "kill9_child_writer", "--nocapture"])
            .env("FLEET_KILL_CHILD", &p)
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        // Wait (bounded) for the child's first record, then kill it mid-stream.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while read(&p).unwrap().len() <= start && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        child.kill().unwrap(); // SIGKILL
        child.wait().unwrap();
        let before = read(&p).unwrap();
        assert!(before.len() > start, "round {round}: child wrote nothing in 10 s");
        append(&p, &ev(999_000 + round)).unwrap();
        let after = read(&p).unwrap();
        assert_eq!(after.len(), before.len() + 1, "round {round}");
        assert_eq!(after.last().unwrap().ts, 999_000 + round);
    }
    // Every row reads back, numbered without a gap.
    let seqs: Vec<u64> = read(&p).unwrap().iter().map(|e| e.seq).collect();
    assert_eq!(seqs, (1..=seqs.len() as u64).collect::<Vec<_>>());
}

/// Child mode: read the log so the parent can inspect stderr.
#[test]
fn read_child() {
    let Ok(p) = std::env::var("FLEET_READ_CHILD") else {
        return;
    };
    println!("count={}", read(std::path::Path::new(&p)).unwrap().len());
}

fn read_stderr(p: &std::path::Path) -> (String, String) {
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "read_child", "--nocapture", "--test-threads=1"])
        .env("FLEET_READ_CHILD", p)
        .output()
        .unwrap();
    assert!(out.status.success());
    (
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

#[test]
fn corrupt_middle_line_warns_torn_tail_is_silent() {
    let d = tmpdir("corrupt");
    let p = d.join("events.jsonl");
    let line = |i| serde_json::to_string(&ev(i)).unwrap();
    // Torn final line: silent.
    std::fs::write(&p, format!("{}\n{}\n{}", line(1), line(2), &line(3)[..100])).unwrap();
    let (out, err) = read_stderr(&p);
    assert!(out.contains("count=2"), "stdout: {out}");
    assert!(!err.contains("corrupt"), "torn tail must be silent: {err}");
    // Garbage on line 2 of 3 (plus a blank line): exactly one warning naming it.
    let p = tmpdir("corrupt-mid").join("events.jsonl");
    std::fs::write(&p, format!("{}\nnot json\n\n{}\n", line(1), line(2))).unwrap();
    let (out, err) = read_stderr(&p);
    assert!(out.contains("count=2"), "stdout: {out}");
    let warns: Vec<&str> = err.lines().filter(|l| l.contains("corrupt")).collect();
    assert_eq!(warns.len(), 1, "stderr: {err}");
    assert!(warns[0].contains("line 2:"), "{}", warns[0]);
}

/// Child mode: append `FLEET_WRITER_N` events tagged with this writer's number.
#[test]
fn writer_child() {
    let (Ok(p), Ok(w)) = (std::env::var("FLEET_WRITER_LOG"), std::env::var("FLEET_WRITER_N")) else {
        return;
    };
    let w: u64 = w.parse().unwrap();
    (0..25).for_each(|i| append(std::path::Path::new(&p), &ev(w * 100 + i)).unwrap());
}

/// #259: every row gets a monotonic seq, across concurrent writers; old rows by line. #344: 16 writer processes at
/// once lose no event and none fails.
#[test]
fn seq_is_monotonic_across_concurrent_writers() {
    let d = tmpdir("seq");
    let p = d.join("events.jsonl");
    std::fs::write(&p, serde_json::to_string(&ev(0)).unwrap() + "\n").unwrap();
    let ws: Vec<_> = (0..16)
        .map(|w| {
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "writer_child", "--nocapture"])
                .env("FLEET_WRITER_LOG", &p)
                .env("FLEET_WRITER_N", w.to_string())
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    for mut w in ws {
        assert!(w.wait().unwrap().success(), "a writer failed");
    }
    let all = read(&p).unwrap();
    let seqs: Vec<u64> = all.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, (1..=401).collect::<Vec<_>>());
    let mut ts: Vec<u64> = all.iter().map(|e| e.ts).collect();
    ts.sort_unstable();
    let want: Vec<u64> = std::iter::once(0)
        .chain((0..16).flat_map(|w| (0..25).map(move |i| w * 100 + i)))
        .collect();
    let mut want = want;
    want.sort_unstable();
    assert_eq!(ts, want, "every writer's every event, once");
    let line = herdsman::events::jsonl(&p)
        .unwrap()
        .lines()
        .nth(400)
        .unwrap()
        .to_string();
    assert!(line.starts_with("{\"seq\":401,"), "{line}");
}
