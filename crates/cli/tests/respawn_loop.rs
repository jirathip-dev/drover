mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::json;

fn dying(c: &drover::Ctx) -> (FakeShed, Fake) {
    let sh = FakeShed::attached(c);
    sh.wait_ends("sendmeter-impl-12", "timeout");
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let f = Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .once(&format!("git -C {} rev-list", wt.display()), 0, "0\n")
        .reply(&format!("git -C {} rev-list", wt.display()), "1\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "");
    (sh, f)
}

fn at(c: &drover::Ctx, mins: u64) -> drover::Ctx {
    drover::Ctx {
        now: c.now + mins * 60,
        ..c.clone()
    }
}

fn respawn(c: &drover::Ctx, r: &Fake, mins: u64) -> (i32, String) {
    run(
        &at(c, mins),
        r,
        &["spawn", "sendmeter", "12", "impl", "--supersede", "--why", "retry"],
    )
}

fn wips(r: &Fake) -> usize {
    r.calls
        .borrow()
        .iter()
        .filter(|c| c.contains(" push origin HEAD:refs/heads/wip/"))
        .count()
}

fn asks(c: &drover::Ctx) -> Vec<Event> {
    let ev = read(&c.state).unwrap().to_vec();
    ev.into_iter().filter(|e| e.kind == "needs-decision").collect()
}

#[test]
fn fourth_start_after_three_failed_is_refused_with_one_owner_ask() {
    let c = ctx(&tmpdir("respawn-loop"), CONFIG);
    let (_sh, r) = dying(&c);
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 4, "{out}");
    for m in [5, 10] {
        assert_eq!(respawn(&c, &r, m).0, 4, "{:?}", r.calls.borrow());
    }
    assert_eq!(wips(&r), 2, "each supersede saved the lane before it");
    let (code, out) = respawn(&c, &r, 15);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED RESPAWN_LOOP"), "{out}");
    assert_eq!(
        out.matches("prompt never landed").count(),
        3,
        "the 3 starts and why: {out}"
    );
    assert_eq!(wips(&r), 2, "no wip snapshot while refused");
    let a = asks(&c);
    assert_eq!(a.len(), 1, "{a:?}");
    assert_eq!(*a[0].val("options"), json!(["try again", "park"]));
    assert_eq!(a[0].str("kind"), Some("owner"));
    assert_eq!(a[0].issue, Some(12));
    assert_eq!(respawn(&c, &r, 20).0, 3);
    assert_eq!((asks(&c).len(), wips(&r)), (1, 2));
    let spawns = read(&c.state).unwrap().iter().filter(|e| e.kind == "spawn").count();
    assert_eq!(spawns, 3);
}

#[test]
fn a_supersede_that_killed_the_live_lane_it_replaced_is_no_failed_start() {
    for (live, refused) in [(true, false), (false, true)] {
        let c = ctx(&tmpdir("respawn-live"), CONFIG);
        let (_sh, r) = dying(&c);
        let agent = "sendmeter-impl-12";
        for m in [0, 5, 10] {
            let t = c.now + m * 60;
            let d = json!({"role": "impl", "agent": agent, "confirmed": true});
            append(&c.state, &Event::new(t, "sendmeter", Some(12), "spawn", d)).unwrap();
            let killed = if live { vec![agent] } else { vec![] };
            let d = json!({"role": "impl", "agent": agent, "killed": killed, "reason": "", "wip": null});
            append(&c.state, &Event::new(t + 60, "sendmeter", Some(12), "supersede", d)).unwrap();
        }
        let (_, out) = respawn(&c, &r, 15);
        assert_eq!(out.contains("REFUSED RESPAWN_LOOP"), refused, "live {live}: {out}");
    }
}

#[test]
fn try_again_lets_a_start_through_and_a_push_resets_the_count() {
    let c = ctx(&tmpdir("respawn-again"), CONFIG);
    let (_sh, r) = dying(&c);
    assert_eq!(run(&c, &r, &["spawn", "sendmeter", "12", "impl"]).0, 4);
    respawn(&c, &r, 5);
    respawn(&c, &r, 10);
    assert_eq!(respawn(&c, &r, 15).0, 3);
    let id = asks(&c)[0].str("id").unwrap().to_string();
    let (code, out) = run(&at(&c, 16), &r, &["answer", "sendmeter", &id, "1"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(respawn(&c, &r, 17).0, 4, "try again: one more start");

    let c = ctx(&tmpdir("respawn-reset"), CONFIG);
    let (_sh, r) = dying(&c);
    assert_eq!(run(&c, &r, &["spawn", "sendmeter", "12", "impl"]).0, 4);
    respawn(&c, &r, 5);
    respawn(&c, &r, 10);
    let pushed = Event::new(c.now + 11 * 60, "sendmeter", Some(12), "pushed", json!({"sha": "abc"}));
    append(&c.state, &pushed).unwrap();
    respawn(&c, &r, 15);
    respawn(&c, &r, 20);
    let (code, out) = respawn(&c, &r, 25);
    assert_eq!(code, 4, "2 failed starts since the push: {out}");
    assert_eq!(respawn(&c, &r, 30).0, 3, "the third since the push trips it");
}

#[test]
fn a_supersede_that_errors_logs_why_and_three_alike_raise_one_owner_ask() {
    let c = ctx(&tmpdir("respawn-supersede-error"), CONFIG);
    let _sh = FakeShed::attached(&c);
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let r = Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply(&format!("git -C {} rev-list", wt.display()), "0\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("git -C", "");
    let (code, out) = run(&c, &r, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "{out}");
    let d = json!({"by": "owner"});
    append(
        &c.state,
        &Event::new(c.now + 60, "sendmeter", Some(12), "round-extended", d),
    )
    .unwrap();
    let r = Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply(&format!("git -C {} rev-list", wt.display()), "1\n")
        .reply("git -C", "");
    for m in [5, 10, 15] {
        let (code, out) = respawn(&c, &r, m);
        assert_eq!(code, 1, "{out}");
    }
    let ev = read(&c.state).unwrap().to_vec();
    let failed: Vec<&Event> = ev.iter().filter(|e| e.kind == "failed").collect();
    assert_eq!(failed.len(), 3, "{failed:?}");
    for f in &failed {
        assert_eq!(f.str("step"), Some("supersede"));
        assert!(f.str("error").is_some_and(|e| e.contains("gh issue view")), "{f:?}");
    }
    let (code, out) = respawn(&c, &r, 20);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED RESPAWN_LOOP"), "{out}");
    assert!(out.contains("gh issue view"), "the card names the error: {out}");
    assert_eq!(respawn(&c, &r, 25).0, 3);
    let a = asks(&c);
    assert_eq!(a.len(), 1, "{a:?}");
    assert_eq!(a[0].str("kind"), Some("owner"));
}
