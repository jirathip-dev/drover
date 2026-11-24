mod common;
use common::*;
use drover::events::read;

fn fake() -> Fake {
    Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.0 1.0 1.0 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "")
}

fn fleets(dir: &str, names: &[&str]) -> (drover::Ctx, FakeShed) {
    let (fleet, limits) = CONFIG.split_once("[limits]").unwrap();
    let all: String = names.iter().map(|n| fleet.replace("sendmeter", n)).collect();
    let limits = limits.replace("max_parallel_spawns = 0", "max_parallel_spawns = 6");
    let c = ctx(&tmpdir(dir), &format!("{all}[limits]{limits}"));
    for n in names {
        assert_eq!(run(&c, &Fake::default(), &["queue", n, "add", "1"]).0, 0);
    }
    (c.clone(), FakeShed::attached(&c))
}

fn mark(c: &drover::Ctx, n: u64, pid: u32, age: u64) {
    let e = serde_json::json!({"ts": 1, "fleet": "sendmeter", "issue": n, "type": "spawn", "role": "impl", "pid": pid});
    let key = format!("spawn-sendmeter-{n}.pending");
    let l = drover::db::take(&c.state, &key, c.now - age, 300, &e.to_string(), &|_| true).unwrap();
    assert!(drover::db::renew(&c.state, &l, &drover::db::Lease { pid, ..l.clone() }));
}

struct Hung(Fake, drover::Ctx);
impl drover::runner::Runner for Hung {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<drover::runner::Out> {
        let admitting = !drover::db::leases(&self.1.state, "admit-").is_empty();
        if std::thread::current().name() == Some("hung") && admitting {
            loop {
                std::thread::park();
            }
        }
        self.0.run(prog, args)
    }
    fn sleep(&self, _: u64) {}
}

type Beta = Result<(i32, String, Fake), std::sync::mpsc::RecvTimeoutError>;

fn beta_pass(c: &drover::Ctx, later: u64) -> Beta {
    let pid = std::process::id();
    let (tx, rx) = std::sync::mpsc::channel();
    let c = c.clone();
    std::thread::spawn(move || {
        let r = fake()
            .reply("kill -0 4242", "")
            .reply("ps -o etime= -p 4242", "  10:00\n")
            .reply(&format!("kill -0 {pid}"), "")
            .reply(&format!("ps -o etime= -p {pid}"), "  10:00\n");
        drover::sim::set(Some(drover::sim::Hooks {
            now: Box::new(move || drover::clock::wall() + later),
            pid,
            point: Box::new(|_| {}),
        }));
        let (code, out) = run_only(&c, &r, &["drive", "beta"]);
        tx.send((code, out, r)).ok();
    });
    rx.recv_timeout(std::time::Duration::from_secs(20))
}

#[test]
fn an_admission_its_hung_kick_left_holds_starts_at_most_the_wait_then_is_dropped() {
    let (c, _sh) = fleets("admission-hung", &["alpha", "beta"]);
    mark(&c, 9, 4242, 10);
    let hung = Hung(
        fake()
            .reply("kill -0 4242", "")
            .reply("ps -o etime= -p 4242", "  10:00\n"),
        c.clone(),
    );
    let hc = c.clone();
    std::thread::Builder::new()
        .name("hung".into())
        .spawn(move || run_only(&hc, &hung, &["drive", "alpha"]))
        .unwrap();
    let t = std::time::Instant::now();
    while lease_on(&c, "admit-local.lock").is_none() && t.elapsed().as_secs() < 20 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        lease_on(&c, "admit-local.lock").is_some(),
        "alpha's kick hung mid-admission"
    );

    let (code, out, r) =
        beta_pass(&c, 0).expect("beta's start waited at most admit_wait_secs, not on the hung admission");
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("beta #1 start: REFUSED SPAWN_PENDING: another pass is admitting a spawn on local"),
        "{out}"
    );
    assert_eq!(r.count("sh -c nohup '/opt/drover/bin/drover' kick beta 1"), 0, "{out}");

    let (code, out, r) = beta_pass(&c, 120).expect("the next pass never waits on the hung admission");
    assert_eq!(code, 0, "{out}");
    assert_eq!(r.count("sh -c nohup '/opt/drover/bin/drover' kick beta 1"), 1, "{out}");
    let dropped: Vec<_> = read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "admission-dropped")
        .cloned()
        .collect();
    assert_eq!(dropped.len(), 1, "{out}");
    assert_eq!(
        (dropped[0].str("host"), dropped[0].val("pid")),
        (Some("local"), &serde_json::json!(std::process::id())),
        "{:?}",
        dropped[0].data
    );
}

#[test]
fn no_spawn_for_an_hour_while_issues_are_startable_alerts_spawn_stuck_once() {
    let (c, _sh) = fleets("admission-stuck", &["sendmeter"]);
    let tried = |ts| {
        let a = serde_json::json!([{"issue": 1, "step": "start", "outcome": "pending", "words": "pending"}]);
        let e = drover::events::Event::new(ts, "sendmeter", None, "drive-pass", serde_json::json!({"actions": a}));
        drover::events::append(&c.state, &e).unwrap();
    };
    let stuck = || {
        read(&c.state)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "spawn-stuck")
            .count()
    };
    tried(c.now - 1800);
    let (code, out) = run(&c, &Fake::default(), &["check"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(stuck(), 0, "30 min without a spawn: {out}");
    let later = drover::Ctx {
        now: c.now + 1900,
        ..c.clone()
    };
    tried(c.now + 1800);
    let (code, out) = run(&later, &Fake::default(), &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("sendmeter: no drive spawn succeeded for 61m with #1 startable"),
        "{out}"
    );
    run(&later, &Fake::default(), &["check"]);
    assert_eq!(stuck(), 1, "once per episode");
}
