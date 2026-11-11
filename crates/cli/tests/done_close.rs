//! #248: `herdsman check` closes a done lane's session once its handoff is logged; dirty or unpushed lanes stay.
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use serde_json::json;

const T: u64 = 1_000_000;
const LANE: &str = "sendmeter-impl-12";

fn lane(name: &str, pushed: u64) -> (herdsman::Ctx, String, FakeShed) {
    let c = ctx(&tmpdir(name), CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let wt = wt.to_string_lossy().into_owned();
    let ev = |ts, kind: &str, d| append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
    ev(
        T - 3600,
        "spawn",
        json!({"role": "impl", "agent": LANE, "pane": LANE, "worktree": wt}),
    );
    ev(T - pushed, "pushed", json!({"sha": "abc", "via": "pr"}));
    (c, wt, sh)
}

fn fake(sh: &FakeShed, wt: &str, status: &str, ahead: &str) -> Fake {
    sh.lane(LANE, "done", wt);
    with(wt, status, ahead)
}

fn with(wt: &str, status: &str, ahead: &str) -> Fake {
    Fake::default()
        .reply(&format!("git -C {wt} status --porcelain"), status)
        .reply(&format!("git -C {wt} rev-list --count"), ahead)
        .reply("git -C", "")
        .reply("lsof", &format!("p201\nc2.1.311\nfcwd\nn{wt}\n"))
        .reply("ps -axo pid=,rss=", "  201 276480\n")
}

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

#[test]
fn a_done_pushed_lane_closes_after_the_delay() {
    let (c, wt, sh) = lane("close-young", 120);
    let r = fake(&sh, &wt, "?? .mcp.lane.json\n?? ui-390/\n", "0\n");
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        !sh.called("kill", LANE) && sh.has(LANE),
        "pushed 2 min ago: under done_close_minutes (5)"
    );

    let (c, wt, sh) = lane("close-due", 400);
    let r = fake(&sh, &wt, "?? .mcp.lane.json\n?? ui-390/\n", "0\n");
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(sh.called("kill", LANE) && !sh.has(LANE), "{:?}", sh.methods());
    let closed = kinds(&c, "lane-closed");
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].data["agent"], LANE);
    assert_eq!(closed[0].data["freed_mb"], 270);
    assert!(std::path::Path::new(&wt).is_dir(), "the worktree stays");
    let r = with(&wt, "", "0\n");
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(kinds(&c, "lane-dead").is_empty(), "{out}");
}

#[test]
fn a_dirty_or_unpushed_lane_stays_open_with_an_event() {
    for (status, ahead, why) in [
        (" M src/a.rs\n?? .mcp.lane.json\n", "0\n", "1 uncommitted change(s)"),
        ("", "2\n", "2 unpushed commit(s)"),
    ] {
        let (c, wt, sh) = lane("close-dirty", 400);
        for _ in 0..2 {
            let r = fake(&sh, &wt, status, ahead);
            let (code, out) = run(&c, &r, &["check"]);
            assert_eq!(code, 0, "{out}");
            assert!(!sh.called("kill", LANE) && sh.has(LANE), "{why}");
        }
        let kept = kinds(&c, "lane-kept-dirty");
        assert_eq!(kept.len(), 1, "once per handoff: {why}");
        assert_eq!(kept[0].data["why"], why);
        assert!(kinds(&c, "lane-closed").is_empty());
        // The card says why it stays open.
        let f = fake(&sh, &wt, status, ahead);
        let r = sh.on(&f);
        let s = herdsman::serve::Server::new(&c, &r, None);
        let page = with_queue(
            &String::from_utf8_lossy(&s.respond("GET /?all=1 HTTP/1.1\r\n\r\n", T)),
            &s,
            "",
            T,
        );
        assert!(page.contains(&format!("kept open: {why}")), "{why}");
    }
}

#[test]
fn a_lane_without_a_logged_handoff_stays_open() {
    let (c, wt, sh) = lane("close-nohandoff", 400);
    // A newer spawn of the lane: the push was the previous round's.
    let sp = json!({"role": "impl", "agent": LANE, "pane": LANE, "worktree": wt});
    append(&c.state, &Event::new(T - 300 + 1, "sendmeter", Some(12), "spawn", sp)).unwrap();
    let r = fake(&sh, &wt, "", "0\n");
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(!sh.called("kill", LANE) && kinds(&c, "lane-closed").is_empty());
}

#[test]
fn a_spawn_whose_prompt_never_landed_is_not_closed_as_done() {
    // #444 (#363): the spawn exited 4, logged in the same second as the handoff it then seemed to follow.
    let c = ctx(&tmpdir("close-unconfirmed"), CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let wt = wt.to_string_lossy().into_owned();
    let ev = |kind: &str, d| append(&c.state, &Event::new(T - 3600, "sendmeter", Some(12), kind, d)).unwrap();
    ev("pushed", json!({"sha": "abc", "via": "pr"}));
    let sp = json!({"role": "impl", "agent": LANE, "pane": LANE, "worktree": wt, "confirmed": false});
    ev("spawn", sp);
    let r = fake(&sh, &wt, "", "0\n");
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(!sh.called("kill", LANE) && kinds(&c, "lane-closed").is_empty(), "{out}");
}
