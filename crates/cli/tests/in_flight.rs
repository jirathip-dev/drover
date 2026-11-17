mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use serde_json::json;
use std::rc::Rc;

thread_local! {
    static SHED: Rc<FakeShed> = Rc::new(FakeShed::new());
}

const NONE: &[(&str, &str, &str)] = &[];

fn setup(name: &str, extra: &str, ns: &[&str]) -> drover::Ctx {
    let cfg = CONFIG.replace(
        "integration = \"staging\"",
        &format!("integration = \"staging\"\n{extra}"),
    );
    let c = common::ctx(&tmpdir(name), &cfg);
    SHED.with(|s| s.attach(&c));
    let add = [&["queue", "sendmeter", "add"][..], ns].concat();
    assert_eq!(run(&c, &Fake::default(), &add).0, 0);
    c
}

fn fake<A: AsRef<str>, B: AsRef<str>, C: AsRef<str>>(live: &[(A, B, C)]) -> Fake {
    SHED.with(|sh| {
        for (name, status, cwd) in live {
            sh.lane(name.as_ref(), status.as_ref(), cwd.as_ref());
        }
    });
    Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("git -C", "")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
}

fn working(c: &drover::Ctx, ns: &[u64]) -> Vec<(String, &'static str, String)> {
    let wt = |n: u64| {
        c.worktrees
            .join(format!("sendmeter/issue-{n}"))
            .to_string_lossy()
            .to_string()
    };
    ns.iter()
        .map(|n| (format!("sendmeter-impl-{n}"), "working", wt(*n)))
        .collect()
}

fn drive(c: &drover::Ctx, r: &Fake) -> String {
    let (code, out) = run(c, r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    out
}

fn ev(c: &drover::Ctx, n: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(c.now, "sendmeter", Some(n), kind, d)).unwrap();
}

fn building(c: &drover::Ctx, ns: std::ops::RangeInclusive<u64>) {
    for n in ns {
        let agent = format!("sendmeter-impl-{n}");
        ev(
            c,
            n,
            "spawn",
            json!({"role": "impl", "agent": agent, "branch": format!("issue-{n}")}),
        );
    }
}

fn state_of(c: &drover::Ctx, n: u64) -> String {
    let st = fold(&read(&c.state).unwrap());
    st.get(&("sendmeter".to_string(), n))
        .map_or("queued".into(), |s| s.state.clone())
}

fn logged(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == kind)
        .cloned()
        .collect()
}

#[test]
fn at_the_cap_drive_starts_no_new_issue_and_every_view_says_so() {
    let c = setup("inflight-hold", "max_in_flight = 2", &["1", "2", "3"]);
    let out = drive(&c, &fake(NONE));
    assert!(
        out.contains("sendmeter #3 start: REFUSED IN_FLIGHT_CAP: 2/2 PRs in flight in sendmeter (#1, #2)"),
        "{out}"
    );
    assert_eq!(
        [state_of(&c, 1), state_of(&c, 2), state_of(&c, 3)],
        ["impl", "impl", "queued"]
    );
    let r = fake(&working(&c, &[1, 2]));
    let status = run(&c, &r, &["status", "sendmeter"]).1;
    assert!(status.contains("sendmeter: in flight 2/2"), "{status}");
    assert!(status.contains("waits: in-flight cap 2/2"), "{status}");
    let board = run(&c, &r, &["board", "sendmeter"]).1;
    assert!(
        board.contains("sendmeter: 3 queued, 2 active, in flight 2/2"),
        "{board}"
    );
    assert!(board.contains(" #3     queued      in-flight cap 2/2 "), "{board}");
    let waiting = run(&c, &r, &["waiting", "sendmeter"]).1;
    assert!(waiting.contains("sendmeter: in flight 2/2\n"), "{waiting}");
    assert!(
        waiting.contains("sendmeter #3 drive: next in the queue, held back: waits: in-flight cap 2/2"),
        "{waiting}"
    );
}

#[test]
fn a_review_spawn_is_not_held_by_the_cap_and_a_merge_frees_a_slot_for_the_next_in_queue_order() {
    let c = setup("inflight-free", "max_in_flight = 2", &["1", "2", "3", "4"]);
    drive(&c, &fake(NONE));
    let r = fake(&working(&c, &[1, 2]));
    let (_, out) = run(&c, &r, &["spawn", "sendmeter", "2", "rev"]);
    assert!(!out.contains("IN_FLIGHT_CAP"), "{out}");
    assert!(state_of(&c, 2).starts_with("review"), "{out}");
    ev(&c, 1, "merged", json!({"pr": 7}));
    let out = drive(&c, &fake(&working(&c, &[2])));
    assert_eq!(
        (state_of(&c, 3), state_of(&c, 4)),
        ("impl".into(), "queued".into()),
        "{out}"
    );
    assert!(out.contains("sendmeter #4 start: REFUSED IN_FLIGHT_CAP: 2/2"), "{out}");
}

#[test]
fn the_cap_defaults_to_8_and_0_turns_it_off() {
    let c = setup("inflight-default", "", &["9"]);
    building(&c, 1..=8);
    let (code, out) = run(&c, &fake(NONE), &["spawn", "sendmeter", "9", "impl"]);
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("REFUSED IN_FLIGHT_CAP: 8/8 PRs in flight in sendmeter (#1, #2, #3, #4, #5, #6, #7, #8)"),
        "{out}"
    );
    assert_eq!(state_of(&c, 9), "queued");

    let c = setup("inflight-seven", "", &["9"]);
    building(&c, 1..=7);
    let (_, out) = run(&c, &fake(NONE), &["spawn", "sendmeter", "9", "impl"]);
    assert!(!out.contains("IN_FLIGHT_CAP"), "{out}");
    assert_eq!(state_of(&c, 9), "impl", "{out}");

    let c = setup("inflight-off", "max_in_flight = 0", &["10"]);
    building(&c, 1..=8);
    let (_, out) = run(&c, &fake(NONE), &["spawn", "sendmeter", "10", "impl"]);
    assert!(!out.contains("IN_FLIGHT_CAP"), "{out}");
    assert_eq!(state_of(&c, 10), "impl", "{out}");
    let status = run(&c, &fake(NONE), &["status", "sendmeter"]).1;
    assert!(!status.contains("in flight"), "{status}");
}

#[test]
fn a_held_issue_with_an_open_pr_counts_and_force_overrides_the_refusal_logged() {
    let c = setup("inflight-force", "max_in_flight = 2", &["5"]);
    building(&c, 1..=1);
    ev(&c, 2, "needs-human", json!({"code": "ROUND_LIMIT"}));
    let (_, out) = run(&c, &fake(NONE), &["spawn", "sendmeter", "5", "impl"]);
    assert!(
        !out.contains("IN_FLIGHT_CAP"),
        "a held issue without a PR is not in flight: {out}"
    );
    let c = setup("inflight-force2", "max_in_flight = 2", &["6"]);
    building(&c, 1..=1);
    ev(&c, 2, "needs-human", json!({"code": "ROUND_LIMIT"}));
    let cache = c.state.with_file_name("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let pr = json!({"ts": c.now, "pr": {"number": 22, "state": "OPEN"}});
    std::fs::write(cache.join("pr-sendmeter-2.json"), pr.to_string()).unwrap();
    let (code, out) = run(&c, &fake(NONE), &["spawn", "sendmeter", "6", "impl"]);
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("REFUSED IN_FLIGHT_CAP: 2/2 PRs in flight in sendmeter (#1, #2)"),
        "{out}"
    );
    assert!(logged(&c, "override").is_empty());
    let (_, out) = run(&c, &fake(NONE), &["spawn", "sendmeter", "6", "impl", "--force"]);
    assert_eq!(state_of(&c, 6), "impl", "{out}");
    let o = logged(&c, "override");
    assert_eq!(o.len(), 1, "{out}");
    assert_eq!(
        (o[0].str("code"), o[0].str("by")),
        (Some("IN_FLIGHT_CAP"), Some("owner"))
    );
}
