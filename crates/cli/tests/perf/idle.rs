use super::*;
use drover::shed::{Shed, Wire};

const IDLE: u64 = 15;
const TICK: f64 = 32.0;
const STATUS: f64 = 6.0;

fn sixteen() -> (drover::Ctx, OnShed, Vec<String>) {
    let idle: String = (1..=IDLE)
        .map(|i| {
            format!(
                "\n[fleet.idle{i}]\nrepo = \"jirathip-dev/idle{i}\"\nlocal = \"/tmp/does-not-matter/idle{i}\"\nintegration = \"staging\"\n\
                 [fleet.idle{i}.role.impl]\nharness = \"claude\"\n[fleet.idle{i}.role.rev]\nharness = \"claude\"\n"
            )
        })
        .collect();
    let c = ctx(&tmpdir("perf-idle"), &format!("{CONFIG}{idle}"));
    let r = OnShed::new(cold_fake());
    r.1.attach(&c);
    let mut log = largest(&r.1, 30);
    for i in 1..=IDLE {
        let (f, agent) = (format!("idle{i}"), format!("idle{i}-impl-1"));
        let ev = |ts, kind, d| serde_json::to_string(&Event::new(ts, &f, Some(1), kind, d)).unwrap();
        let d = json!({"role": "impl", "agent": agent, "pane": agent, "runtime": "shed", "worktree": format!("/wt/{agent}"), "branch": "issue-1"});
        log.push(ev(NOW - 9_000, "spawn", d));
        log.push(ev(NOW - 8_000, "pushed", json!({"sha": "a".repeat(40)})));
        log.push(ev(NOW - 7_000, "merged", json!({"pr": 1})));
        log.push(ev(NOW - 6_000, "clean", json!({"worktree": format!("/wt/{agent}")})));
    }
    std::fs::create_dir_all(c.state.parent().unwrap().join("cache")).unwrap();
    std::fs::write(&c.state, log.join("\n") + "\n").unwrap();
    (c, r, log)
}

fn largest(sh: &FakeShed, lanes: u64) -> Vec<String> {
    let ev = |ts, n: Option<u64>, kind, d| serde_json::to_string(&Event::new(ts, "sendmeter", n, kind, d)).unwrap();
    let queued: Vec<u64> = (lanes + 1..=lanes + 150).collect();
    let mut log = vec![ev(NOW - 9_900, None, "queue", json!({"op": "add", "issues": queued}))];
    for n in 1..=lanes {
        let (agent, t) = (format!("sendmeter-impl-{n}"), NOW - 9_000 + n);
        let d = json!({"role": "impl", "agent": agent, "pane": agent, "runtime": "shed", "worktree": format!("/wt/{agent}"), "branch": format!("issue-{n}")});
        log.push(ev(t, Some(n), "spawn", d));
        log.push(ev(t + 1, Some(n), "pushed", json!({"sha": "a".repeat(40)})));
        sh.lane(&agent, "idle", &format!("/wt/{agent}"));
        sh.screen(&agent, "⏺ Working on it\n\n❯ \n");
    }
    for i in 0..10_000 - u64::try_from(log.len()).unwrap() {
        log.push(ev(
            NOW - 5_000 + i / 10,
            Some(1 + i % lanes),
            "driven",
            json!({"step": "review", "code": "EXIT", "result": "exit 1: boom"}),
        ));
    }
    log
}

fn cold_fake() -> Fake {
    Fake::default().reply("git -C", &format!("{} {}\n", "a".repeat(40), NOW - 600))
}

struct Cold(Fake, std::path::PathBuf);
impl Runner for Cold {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.0.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.0.run_in(prog, args, input)
    }
    fn sleep(&self, _: u64) {}
    fn shed(&self) -> Option<Shed> {
        Some(Shed {
            wire: Wire::Socket(self.1.clone()),
            acp: Some("claude-acp".into()),
            policy: "allow".into(),
        })
    }
}

fn cold(c: &drover::Ctx, sh: &FakeShed, args: &str) -> (f64, Vec<String>) {
    let spec = format!(
        "{}|{}|{}|{args}",
        c.state.parent().unwrap().parent().unwrap().display(),
        c.now,
        sh.sock.display()
    );
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["idle::cold_child", "--exact", "--nocapture"])
        .env("DROVER_PERF_1749", spec)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    let cpu = err.lines().find_map(|l| l.strip_prefix("#1749 cpu "));
    let cpu = cpu.and_then(|v| v.parse().ok()).unwrap_or_else(|| panic!("{err}"));
    let passes = err
        .lines()
        .filter_map(|l| l.strip_prefix("#1749 pass "))
        .map(String::from);
    (cpu, passes.collect())
}

fn tick(c: &drover::Ctx, sh: &FakeShed) -> f64 {
    let (t, passes) = cold(c, sh, "tick");
    t + passes.iter().map(|f| cold(c, sh, &format!("drive {f}")).0).sum::<f64>()
}

#[test]
fn cold_child() {
    let Ok(spec) = std::env::var("DROVER_PERF_1749") else {
        return;
    };
    let w: Vec<&str> = spec.split('|').collect();
    let dir = std::path::Path::new(w[0]);
    let c = drover::Ctx {
        config: dir.join("config/fleet.toml"),
        state: dir.join("state/events.jsonl"),
        worktrees: dir.join("worktrees"),
        exe: "/opt/drover/bin/drover".into(),
        now: w[1].parse().unwrap(),
        home: dir.join("home"),
        hints: vec![],
    };
    let r = Cold(cold_fake(), w[2].into());
    let args: Vec<&str> = w[3].split(' ').collect();
    let t0 = cpu_ms();
    let (code, out) = run_only(&c, &r, &args);
    let cpu = cpu_ms() - t0;
    assert_eq!(code, 0, "{out}");
    eprintln!("#1749 cpu {cpu:.0}");
    for l in r.0.calls.borrow().iter() {
        let f = l.strip_prefix("sh -c nohup '/opt/drover/bin/drover' drive ");
        if let Some(f) = f.and_then(|f| f.split_whitespace().next()) {
            eprintln!("#1749 pass {f}");
        }
    }
}

fn calibrate(log: &[String]) -> f64 {
    let t0 = cpu_ms();
    log.iter()
        .cycle()
        .take(3 * log.len())
        .for_each(|l| drop(std::hint::black_box(serde_json::from_str::<serde_json::Value>(l))));
    cpu_ms() - t0
}

fn median(mut t: Vec<f64>) -> f64 {
    t.sort_by(f64::total_cmp);
    t[t.len() / 2]
}

fn budget(what: &str, cpu: f64, t: &[f64], cal: f64, times: f64) {
    let budget = (times * cal).round();
    report(&format!(
        "#1749 perf: {what}: {cpu:.0} ms of CPU (median of {t:?}); budget {times} x this runner's 3 parses of the log ({cal:.0} ms) = {budget:.0} ms"
    ));
    if cfg!(debug_assertions) {
        return eprintln!("debug build: budgets apply in release (cargo test --release --test perf)");
    }
    assert!(
        cpu < budget,
        "{what} took {cpu:.0} ms of CPU (budget {budget:.0}): {t:?}"
    );
}

#[test]
fn one_tick_over_16_fleets_15_idle_stays_under_its_cpu_budget() {
    let (c, r, log) = sixteen();
    assert_eq!(run(&c, &r, &["tick"]).0, 0);
    let t: Vec<f64> = (1..=3)
        .map(|k| {
            let c = drover::Ctx {
                now: NOW + k * 60,
                ..c.clone()
            };
            tick(&c, &r.1)
        })
        .collect();
    let what =
        "one tick over 16 fleets (15 idle; the 16th 30 lanes, 150 queued, 10,000 events), the tick and each pass it starts in a fresh process";
    budget(what, median(t.clone()), &t, calibrate(&log), TICK);
}

#[test]
fn status_on_the_largest_fleet_stays_under_its_budget() {
    let c = ctx(&tmpdir("perf-status"), CONFIG);
    let sh = FakeShed::attached(&c);
    let log = largest(&sh, 30);
    std::fs::create_dir_all(c.state.parent().unwrap().join("cache")).unwrap();
    std::fs::write(&c.state, log.join("\n") + "\n").unwrap();
    assert_eq!(run(&c, &sh.on(&cold_fake()), &["status", "sendmeter"]).0, 0);
    let t: Vec<f64> = (0..3).map(|_| cold(&c, &sh, "status sendmeter").0).collect();
    let what = "status on the largest fleet (30 lanes, 150 queued, 10,000 events), in a fresh process";
    budget(what, median(t.clone()), &t, calibrate(&log), STATUS);
}
