mod common;
use common::*;
use drover::events::{read, Event};
use drover::runner::{Out, Runner};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

const SLOW: &str = "\n[fleet.slowfleet]\nrepo = \"jirathip-dev/slowfleet\"\nlocal = \"/tmp/does-not-matter/slowfleet\"\nintegration = \"staging\"\n\n[fleet.slowfleet.role.impl]\nharness = \"claude\"\nprofile = \"fleet-impl\"\n\n[fleet.slowfleet.role.rev]\nharness = \"claude\"\nprofile = \"fleet-rev\"\n";

struct Gate {
    f: Fake,
    held: Mutex<(bool, bool)>,
    moved: Condvar,
}

impl Gate {
    fn hold(&self, args: &[String]) {
        if std::thread::current().name() != Some("hung") || !args.iter().any(|a| a.contains("slowfleet")) {
            return;
        }
        let mut g = self.held.lock().unwrap();
        g.0 = true;
        self.moved.notify_all();
        while !g.1 {
            g = self.moved.wait(g).unwrap();
        }
    }
    fn until_held(&self) {
        let g = self.held.lock().unwrap();
        let (g, t) = self
            .moved
            .wait_timeout_while(g, Duration::from_secs(30), |g| !g.0)
            .unwrap();
        assert!(!t.timed_out() && g.0, "slowfleet's pass never reached a call");
    }
    fn open(&self) {
        self.held.lock().unwrap().1 = true;
        self.moved.notify_all();
    }
}

impl Runner for Gate {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.hold(args);
        self.f.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.hold(args);
        self.f.run_in(prog, args, input)
    }
    fn sleep(&self, _: u64) {}
}

fn at(c: &drover::Ctx, now: u64) -> drover::Ctx {
    drover::Ctx { now, ..c.clone() }
}

#[test]
fn with_one_fleets_pass_hung_the_others_last_pass_advances_each_fast_step() {
    let cfg = format!("{CONFIG}{SLOW}").replace("[limits]\n", "[limits]\nfast_secs = 30\n");
    let c = ctx(&tmpdir("loops-hung"), &cfg);
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let _sh = FakeShed::attached(&c);
    let pid = std::process::id().to_string();
    let r = Gate {
        f: Fake::default()
            .reply(&format!("kill -0 {pid}"), "")
            .reply(&format!("ps -o etime= -p {pid}"), "00:01\n")
            .reply("sh -c nohup", "4321\n")
            .reply("git -C", ""),
        held: Mutex::default(),
        moved: Condvar::new(),
    };
    std::thread::scope(|s| {
        let hung = std::thread::Builder::new()
            .name("hung".into())
            .spawn_scoped(s, || run_only(&c, &r, &["drive", "slowfleet"]))
            .unwrap();
        r.until_held();
        let step = std::fs::read_to_string(c.state.with_file_name("drive-slowfleet.lock.step")).unwrap();
        let started = |f: &str| {
            let a = format!("'/opt/drover/bin/drover' drive {f} >>");
            r.f.calls
                .borrow()
                .iter()
                .filter(|l| l.starts_with("sh -c nohup") && l.contains(&a))
                .count()
        };
        for (i, k) in (0..4).enumerate() {
            let now = at(&c, c.now + k * 30);
            let (code, out) = run_only(&now, &r, &["drive"]);
            assert_eq!(code, 0, "{out}");
            assert_eq!((started("sendmeter"), started("slowfleet")), (1, 0), "{out}");
            if i > 0 {
                assert_eq!(
                    drover::drive::last_pass(&now, "sendmeter"),
                    Some(now.now),
                    "quiet: no process, its last pass now"
                );
            }
            let (code, out) = run_only(&now, &r, &["drive", "sendmeter"]);
            assert_eq!(code, 0, "{out}");
            assert_eq!(drover::drive::last_pass(&now, "sendmeter"), Some(now.now), "{out}");
            assert_eq!(drover::drive::last_pass(&now, "slowfleet"), None, "{step}");
        }
        r.open();
        assert_eq!(hung.join().unwrap().0, 0);
    });
    assert_eq!(drover::drive::last_pass(&c, "slowfleet"), Some(c.now));
}

#[test]
fn a_fleet_with_no_pass_for_slo_pass_minutes_has_its_own_pass_stopped_and_no_other() {
    let c = ctx(&tmpdir("loops-heal"), &format!("{CONFIG}{SLOW}"));
    let _sh = FakeShed::attached(&c);
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    for f in ["sendmeter", "slowfleet"] {
        std::fs::write(st.join(format!("drive-{f}.last")), (c.now - 5 * 60).to_string()).unwrap();
    }
    lease_as(&c, "drive-slowfleet.lock", 4242, c.now - 5 * 60, 120);
    let r = Fake::default()
        .reply("kill -TERM 4242", "")
        .reply("sh -c nohup", "4321\n")
        .reply("git -C", "");
    let (code, out) = run_only(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    let heal: Vec<Event> = (read(&c.state).unwrap().iter())
        .filter(|e| e.kind == "self-heal")
        .cloned()
        .collect();
    assert_eq!(heal.len(), 1, "{out}");
    assert_eq!(heal[0].fleet, "slowfleet");
    assert_eq!(
        heal[0].str("did"),
        Some("stopped slowfleet's pass pid 4242 (SIGTERM)"),
        "{out}"
    );
    assert_eq!(r.count("kill -TERM"), 1, "sendmeter's pass is never stopped: {out}");
}

#[test]
fn a_hung_pass_ends_itself_at_pass_max_secs_and_names_its_step() {
    let d = tmpdir("loops-deadline");
    let c = ctx(&d, &CONFIG.replace("[limits]\n", "[limits]\npass_max_secs = 1\n"));
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let _sh = FakeShed::attached(&c);
    let d12 = serde_json::json!({"role": "impl", "agent": "sendmeter-impl-12", "pane": "sendmeter-impl-12", "branch": "issue-12"});
    drover::events::append(&c.state, &Event::new(c.now - 60, "sendmeter", Some(12), "spawn", d12)).unwrap();
    let bin = d.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let gh = bin.join("gh");
    std::fs::write(&gh, "#!/bin/sh\nexec sleep 5\n").unwrap();
    std::fs::set_permissions(&gh, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let home = c.state.parent().unwrap().parent().unwrap();
    let t = Instant::now();
    let st = std::process::Command::new(env!("CARGO_BIN_EXE_drover"))
        .args(["drive", "sendmeter"])
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("HOME", home)
        .env("DROVER_HOME", home)
        .env("FLEET_CONFIG", &c.config)
        .env("FLEET_STATE", &c.state)
        .env("FLEET_WORKTREES", &c.worktrees)
        .output()
        .unwrap();
    let secs = t.elapsed().as_secs_f64();
    let said = String::from_utf8_lossy(&st.stdout).into_owned() + &String::from_utf8_lossy(&st.stderr);
    assert_eq!(
        st.status.code(),
        Some(124),
        "ended by its deadline after {secs:.1} s: {said}"
    );
    assert!(secs < 4.0, "the pass ran {secs:.1} s past a 1 s limit");
    let ev = read(&c.state).unwrap();
    let slow = ev.iter().find(|e| e.kind == "pass-slow").expect("pass-slow logged");
    assert_eq!(slow.fleet, "sendmeter");
    assert_eq!(slow.str("step"), Some("closed issues"), "{:?}", slow.data);
    assert!(ev.iter().all(|e| e.kind != "drive-pass"), "it never finished");
}

#[test]
fn a_wake_starts_its_fleets_pass_within_a_second() {
    let c = ctx(&tmpdir("loops-wake"), &format!("{CONFIG}{SLOW}"));
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let _sh = FakeShed::attached(&c);
    let r: &'static Fake = Box::leak(Box::new(Fake::default()));
    let api = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let sc = c.clone();
    std::thread::spawn(move || drover_service::run(sc, r, || 1_000_000, api));
    let pass = |f: &str| r.count(&format!("/opt/drover/bin/drover drive {f}"));
    let t = Instant::now();
    while (pass("sendmeter") == 0 || pass("slowfleet") == 0) && t.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        pass("sendmeter"),
        1,
        "the service's first fast job woke each fleet's loop"
    );
    let before = (pass("sendmeter"), pass("slowfleet"));
    let t = Instant::now();
    assert!(drover::service::wake(&c, "sendmeter"));
    while pass("sendmeter") == before.0 && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let ms = t.elapsed().as_millis();
    assert_eq!(pass("sendmeter"), before.0 + 1, "the wake started sendmeter's pass");
    assert!(ms < 1000, "sendmeter's pass started {ms} ms after its wake");
    assert_eq!(pass("slowfleet"), before.1, "a wake starts its own fleet's pass only");
}

#[test]
fn a_service_that_answers_a_wake_but_runs_no_pass_never_leaves_a_fleets_last_pass_stale() {
    let c = ctx(&tmpdir("loops-1625-deaf"), CONFIG);
    let _sh = deaf(&c);
    assert!(drover::service::wake(&c, "sendmeter"), "the service answers a wake");
    let last = c.state.with_file_name("drive-sendmeter.last");
    let detached = "sh -c nohup '/opt/drover/bin/drover' drive sendmeter";
    let r = Fake::default().reply("sh -c nohup", "4321\n");
    std::fs::write(&last, c.now.to_string()).unwrap();
    let (code, out) = run_only(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(r.count(detached), 0, "a fresh pass: the wake is trusted: {out}");
    std::fs::write(&last, (c.now - 900).to_string()).unwrap();
    let (code, out) = run_only(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(r.count(detached), 1, "a 15 min old pass: drive starts it itself: {out}");
}

fn deaf(c: &drover::Ctx) -> FakeShed {
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let sh = FakeShed::attached(c);
    fake_service(c, 1);
    sh
}

#[test]
fn a_tick_skips_a_fleet_the_live_service_drove_within_the_tick_interval() {
    let c = ctx(&tmpdir("loops-1802-tick"), CONFIG);
    let _sh = deaf(&c);
    let last = c.state.with_file_name("drive-sendmeter.last");
    let detached = "sh -c nohup '/opt/drover/bin/drover' drive sendmeter";
    let r = Fake::default().reply("sh -c nohup", "4321\n").reply("git -C", "");
    std::fs::write(&last, (c.now - 240).to_string()).unwrap();
    let (code, out) = run_only(&c, &r, &["tick"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(r.count(detached), 0, "driven by the service 4 min ago: {out}");
    std::fs::write(&last, (c.now - 400).to_string()).unwrap();
    let (code, out) = run_only(&c, &r, &["tick"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        r.count(detached),
        1,
        "older than the tick interval: the tick drives it: {out}"
    );
}
