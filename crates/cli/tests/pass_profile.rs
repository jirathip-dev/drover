mod common;
use common::*;
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Instant;

fn cpu() -> f64 {
    let o = std::process::Command::new("ps")
        .args(["-o", "cputime=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    let t = String::from_utf8_lossy(&o.stdout).trim().to_string();
    t.split(':').fold(0.0, |a, p| a * 60.0 + p.parse::<f64>().unwrap())
}

#[test]
#[ignore]
fn pass_profile() {
    let src = PathBuf::from(std::env::var("HERDSMAN_PROFILE").expect("HERDSMAN_PROFILE"));
    let fast = std::env::var("HERDSMAN_PROFILE_MODE").as_deref() == Ok("fast");
    let runs: usize = std::env::var("HERDSMAN_PROFILE_RUNS").map_or(3, |n| n.parse().unwrap());
    let cfg: String = std::fs::read_to_string(src.join("fleet.toml"))
        .unwrap()
        .lines()
        .filter(|l| !l.trim_start().starts_with("roots"))
        .map(|l| format!("{l}\n"))
        .collect();
    let log = herdsman::events::read(&src.join("events.jsonl")).unwrap();
    let (len, now) = (log.len(), log.iter().map(|e| e.ts).max().unwrap() + 60);
    drop(log);
    for i in 0..runs {
        let d = tmpdir("profile");
        let mut c = ctx(&d, &cfg);
        c.now = now;
        std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
        std::fs::copy(src.join("state.db"), c.state.with_file_name("state.db")).unwrap();
        let f = OnShed::new(
            Fake::default()
                .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
                .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
                .reply("sysctl -n hw.ncpu", "10\n")
                .reply("vm_stat", "Pages free: 100000.\nSwapouts: 100.\n"),
        );
        let args: &[&str] = if fast { &["drive", "--fast"] } else { &["drive"] };
        let (r0, c0, t0) = (herdsman::db::ROWS.load(Relaxed), cpu(), Instant::now());
        let (code, _) = run(&c, &f, args);
        let (wall, cpu, rows) = (
            t0.elapsed().as_secs_f64(),
            cpu() - c0,
            herdsman::db::ROWS.load(Relaxed) - r0,
        );
        println!(
            "run {i} {:?}: exit {code} wall {wall:.2}s cpu {cpu:.2}s rows {rows} ({:.1} full-log reads of {len})",
            args,
            rows as f64 / len as f64
        );
    }
}
