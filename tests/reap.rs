//! #186: closing a lane reaps the processes whose cwd is inside its worktree (TERM, 5 s, KILL), nothing else.
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use serde_json::json;

fn ev(c: &herdsman::Ctx, issue: u64, kind: &str, d: serde_json::Value) {
    append(&c.state, &Event::new(10, "sendmeter", Some(issue), kind, d)).unwrap();
}

fn lane(c: &herdsman::Ctx, issue: u64, merged: bool) -> String {
    let wt = c.worktrees.join("sendmeter").join(format!("issue-{issue}"));
    std::fs::create_dir_all(&wt).unwrap();
    let wt = wt.to_string_lossy().into_owned();
    let d = json!({"role": "impl", "agent": format!("impl-{issue}"), "worktree": wt, "pane": format!("w{issue}:p1"), "workspace": format!("w{issue}")});
    ev(c, issue, "spawn", d);
    if merged {
        ev(
            c,
            issue,
            "merged",
            json!({"pr": "1", "merge_sha": "m", "into": "staging"}),
        );
    }
    wt
}

/// `lsof -d cwd -F pcn` rows for (pid, command, cwd).
fn lsof(rows: &[(&str, &str, &str)]) -> String {
    rows.iter()
        .map(|(p, c, n)| format!("p{p}\nc{c}\nfcwd\nn{n}\n"))
        .collect()
}

fn kills(r: &Fake) -> Vec<String> {
    r.calls
        .borrow()
        .iter()
        .filter(|c| c.starts_with("kill "))
        .cloned()
        .collect()
}

#[test]
fn closing_a_lane_terms_then_kills_only_its_worktree_and_logs_the_reap() {
    let d = tmpdir("reap-clean");
    let c = ctx(&d, CONFIG);
    let wt = lane(&c, 12, true);
    let rows = lsof(&[
        ("101", "node", &format!("{wt}/web")),
        ("202", "chrome", "/Users/me/elsewhere"),
        ("303", "herdsman", &wt),           // the board server / tick: denylisted
        ("404", "vite", &format!("{wt}0")), // a sibling worktree `issue-120`
    ]);
    let r = Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("lsof -d cwd -F pcn", &rows)
        .reply("git -C", "")
        .reply("herdr", "{}");
    let (code, out) = run(&c, &r, &["clean", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(kills(&r), ["kill -TERM 101", "kill -KILL 101"], "{out}");
    let (term, kill) = (r.index("kill -TERM").unwrap(), r.index("kill -KILL").unwrap());
    assert!(term < r.index("sleep 5").unwrap() && r.index("sleep 5").unwrap() < kill);
    let e = read(&c.state)
        .unwrap()
        .into_iter()
        .find(|e| e.kind == "lane-reaped")
        .expect("lane-reaped logged");
    assert_eq!(
        (e.issue, e.data["killed"].clone(), e.data["names"].clone()),
        (Some(12), json!(1), json!(["node"]))
    );
}

#[test]
fn check_reaps_closed_lanes_only_and_skips_kill_once_they_exit() {
    let d = tmpdir("reap-check");
    let c = ctx(&d, CONFIG);
    let (closed, active, live) = (lane(&c, 12, true), lane(&c, 15, false), lane(&c, 16, true));
    let rows = lsof(&[
        ("101", "Google Chrome Helper", &closed),
        ("505", "node", &active),
        ("606", "vite", &live),
    ]);
    let r = Fake::default()
        .reply("herdr agent list", &agents(&[("impl-16", "idle", &live, "w16:p1")]))
        .once("lsof -d cwd -F pcn", 0, &rows)
        .once("lsof -d cwd -F pcn", 0, &rows)
        .reply("lsof -d cwd -F pcn", "")
        .reply("herdr", "{}");
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        kills(&r),
        ["kill -TERM 101"],
        "gone after TERM: no KILL; active and live lanes untouched"
    );
    let reaped: Vec<_> = read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "lane-reaped")
        .collect();
    assert_eq!(reaped.len(), 1);
    assert_eq!(reaped[0].data["names"], json!(["Google Chrome Helper"]));
}
