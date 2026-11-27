mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::json;
use std::rc::Rc;

thread_local! {
    static SHED: Rc<FakeShed> = Rc::new(FakeShed::new());
}

fn fleet(f: &str) -> String {
    format!(
        "[fleet.{f}]\nrepo = \"jirathip-dev/{f}\"\nlocal = \"/tmp/does-not-matter/{f}\"\nintegration = \"staging\"\n\n\
         [fleet.{f}.role.impl]\nharness = \"claude\"\nprofile = \"fleet-impl\"\n\n\
         [fleet.{f}.role.rev]\nharness = \"claude\"\nprofile = \"fleet-rev\"\n\n"
    )
}

fn setup(name: &str, limits: &str) -> drover::Ctx {
    let fleets: String = ["a", "b", "c"].map(fleet).concat();
    let cfg = format!("{fleets}[limits]\nreview_rounds = 3\nmax_parallel_spawns = 0\nstray_roots = []\n{limits}\n");
    let c = common::ctx(&tmpdir(name), &cfg);
    SHED.with(|s| s.attach(&c));
    c
}

fn fake() -> Fake {
    Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("git -C", "")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
}

fn queue(c: &drover::Ctx, f: &str, n: &str) {
    assert_eq!(run(c, &Fake::default(), &["queue", f, "add", n]).0, 0);
}

fn ev(c: &drover::Ctx, f: &str, n: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(c.now, f, Some(n), kind, d)).unwrap();
}

fn building(c: &drover::Ctx, f: &str, ns: &[u64]) {
    for &n in ns {
        let d = json!({"role": "impl", "agent": format!("{f}-impl-{n}"), "branch": format!("issue-{n}")});
        ev(c, f, n, "spawn", d);
    }
}

fn state_of(c: &drover::Ctx, f: &str, n: u64) -> String {
    let st = fold(&read(&c.state).unwrap());
    st.get(&(f.to_string(), n)).map_or("queued".into(), |s| s.state.clone())
}

#[test]
fn a_full_total_refuses_a_first_start_in_another_fleet_naming_both_but_never_a_fix_round() {
    let c = setup("total-full", "max_in_flight_total = 2");
    building(&c, "a", &[1, 2]);
    queue(&c, "b", "5");
    let (code, out) = run(&c, &fake(), &["spawn", "b", "5", "impl"]);
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("REFUSED IN_FLIGHT_CAP: 2/2 PRs in flight across fleets (a: #1, #2)"),
        "{out}"
    );
    assert_eq!(state_of(&c, "b", 5), "queued");
    let status = run(&c, &fake(), &["status"]).1;
    assert_eq!(status.matches("in flight 2/2\n").count(), 1, "{status}");

    ev(
        &c,
        "a",
        1,
        "spawn",
        json!({"role": "rev", "agent": "a-rev-1", "branch": "issue-1"}),
    );
    let fail = json!({"verdict": "FAIL", "sha": "a".repeat(40), "body": "FAIL\n1. blocking: src/x.rs:3"});
    ev(&c, "a", 1, "verdict", fail);
    let (code, out) = run(&c, &fake(), &["spawn", "a", "1", "impl", "--supersede"]);
    assert_eq!(code, 0, "{out}");
    assert!(state_of(&c, "a", 1).starts_with("fix"), "{out}");

    let (code, out) = run(&c, &fake(), &["spawn", "b", "5", "impl", "--force"]);
    assert_eq!((code, state_of(&c, "b", 5)), (0, "impl".into()), "{out}");
    let all = read(&c.state).unwrap();
    let o: Vec<_> = all.iter().filter(|e| e.kind == "override").collect();
    assert_eq!(
        (o.len(), o[0].str("code"), o[0].str("by")),
        (1, Some("IN_FLIGHT_CAP"), Some("owner"))
    );
}

#[test]
fn a_merge_frees_the_slot_and_the_next_pass_starts_the_other_fleets_queued_issue() {
    let c = setup("total-merge", "max_in_flight_total = 2");
    building(&c, "a", &[1, 2]);
    queue(&c, "b", "5");
    let out = run(&c, &fake(), &["drive"]).1;
    assert!(out.contains("b #5 start: REFUSED IN_FLIGHT_CAP: 2/2"), "{out}");
    assert_eq!(state_of(&c, "b", 5), "queued");
    ev(&c, "a", 1, "merged", json!({"pr": 7}));
    let out = run(&c, &fake(), &["drive"]).1;
    assert_eq!(state_of(&c, "b", 5), "impl", "{out}");
}

#[test]
fn fleet_priority_gives_the_one_free_slot_to_the_listed_fleet_before_an_older_entry() {
    let c = setup(
        "total-priority",
        "max_in_flight_total = 2\nfleet_priority = [\"b\", \"a\"]",
    );
    building(&c, "c", &[1]);
    queue(&c, "a", "7");
    let mut later = c.clone();
    later.now += 60;
    queue(&later, "b", "8");
    let out = run(&later, &fake(), &["drive"]).1;
    assert_eq!(
        (state_of(&c, "a", 7), state_of(&c, "b", 8)),
        ("queued".into(), "impl".into()),
        "{out}"
    );
    assert!(out.contains("a #7 start: REFUSED IN_FLIGHT_CAP: 1/2 PRs in flight across fleets (c: #1); 1 start(s) in other fleets go first"), "{out}");
}

#[test]
fn unlisted_fleets_take_a_free_slot_oldest_queue_entry_first() {
    let c = setup("total-oldest", "max_in_flight_total = 1");
    queue(&c, "b", "3");
    let mut later = c.clone();
    later.now += 60;
    queue(&later, "a", "4");
    let out = run(&later, &fake(), &["drive"]).1;
    assert_eq!(
        (state_of(&c, "a", 4), state_of(&c, "b", 3)),
        ("queued".into(), "impl".into()),
        "{out}"
    );
}
