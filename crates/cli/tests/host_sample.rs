mod common;
use common::*;
use drover::host;
use drover::hosts::On;
use drover::runner::{Out, Runner};

struct Count {
    fake: Fake,
    started: Shared<Vec<String>>,
}

impl Runner for Count {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.started.borrow_mut().push(format!("{prog} {}", args.join(" ")));
        self.fake.run(prog, args)
    }
    fn sleep(&self, secs: u64) {
        self.started.borrow_mut().push(format!("sleep {secs}"));
    }
}

fn probes() -> Fake {
    Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 5.00 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts: 100.\n")
}

#[test]
fn a_sample_never_sleeps_and_starts_one_process_per_host() {
    let r = Count {
        fake: probes(),
        started: Shared::default(),
    };
    let h = host::sample(&r);
    let started = r.started.borrow().clone();
    assert_eq!(started.len(), 1, "{started:?}");
    assert!(started[0].starts_with("sh -c "), "{started:?}");
    assert_eq!(h.pressure.as_deref(), Some("normal"));
    assert_eq!(h.load_per_cpu, Some(0.5));
    assert_eq!(h.swap_count, Some(100.0));
    r.started.borrow_mut().clear();
    host::sample(&On::new(&r, Some("mini")));
    let started = r.started.borrow().clone();
    assert_eq!(started.len(), 1, "one ssh round trip: {started:?}");
    assert!(
        started[0].starts_with("ssh ") && started[0].contains("mini"),
        "{started:?}"
    );
    assert!(r.fake.calls.borrow().iter().all(|c| !c.starts_with("sleep")));
}

#[test]
fn the_rate_comes_from_the_last_kept_count_with_none_on_the_first() {
    let d = tmpdir("host-pace");
    let state = d.join("events.jsonl");
    let r = probes()
        .once("vm_stat", 0, "Swapouts: 100.\n")
        .once("vm_stat", 0, "Swapouts: 700.\n");
    let mut first = host::sample(&r);
    host::pace(&state, None, &mut first, 1_000_000);
    assert_eq!(first.swapout, None, "no previous sample, no rate");
    assert!(host::admit(&first, &drover::config::Limits::default()).is_ok());
    let mut next = host::sample(&r);
    host::pace(&state, None, &mut next, 1_000_030);
    assert_eq!(next.swapout, Some(40.0), "600 in 30 s is 40 a 2 s window");
    let mut remote = host::sample(&r);
    host::pace(&state, Some("mini"), &mut remote, 1_000_060);
    assert_eq!(remote.swapout, None, "each host keeps its own count");
}
