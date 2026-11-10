//! #779: a lane that dies at start is started at most `[limits] respawn_max` (3) times in `respawn_window_minutes`
//! (60); the next spawn is refused `RESPAWN_LOOP` with one owner ask ([try again] / [park]) and takes no wip snapshot.
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use serde_json::json;

/// A Fake on which `spawn sendmeter 12 impl` starts a lane on the fake shed whose prompt never lands (exit 4); after the
/// first start its worktree holds one unpushed commit, so each `--supersede` of it saves a `wip/` branch first.
fn dying(c: &herdsman::Ctx) -> (FakeShed, Fake) {
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

fn at(c: &herdsman::Ctx, mins: u64) -> herdsman::Ctx {
    herdsman::Ctx {
        now: c.now + mins * 60,
        ..c.clone()
    }
}

/// `spawn sendmeter 12 impl --supersede` at `mins` past the first start: (exit, output).
fn respawn(c: &herdsman::Ctx, r: &Fake, mins: u64) -> (i32, String) {
    run(&at(c, mins), r, &["spawn", "sendmeter", "12", "impl", "--supersede"])
}

fn wips(r: &Fake) -> usize {
    r.calls
        .borrow()
        .iter()
        .filter(|c| c.contains(" push origin HEAD:refs/heads/wip/"))
        .count()
}

fn asks(c: &herdsman::Ctx) -> Vec<Event> {
    let ev = read(&c.state).unwrap();
    ev.into_iter().filter(|e| e.kind == "needs-decision").collect()
}

/// Three failed starts in the hour: the fourth is refused with one owner ask, and no fourth wip branch is made.
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
    assert_eq!(a[0].data["options"], json!(["try again", "park"]));
    assert_eq!(a[0].str("kind"), Some("owner"));
    assert_eq!(a[0].issue, Some(12));
    // Drive's next pass is refused again: still the one ask, still no snapshot.
    assert_eq!(respawn(&c, &r, 20).0, 3);
    assert_eq!((asks(&c).len(), wips(&r)), (1, 2));
    let spawns = read(&c.state).unwrap().iter().filter(|e| e.kind == "spawn").count();
    assert_eq!(spawns, 3);
}

/// [try again] lets the next start through; a start that pushed resets the count.
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
