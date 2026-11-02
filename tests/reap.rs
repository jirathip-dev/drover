//! #186: a closing lane's leftover processes (cwd inside its worktree) get TERM, 5 s, then KILL; nothing else does.
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use serde_json::json;

fn setup(name: &str) -> (std::path::PathBuf, herdsman::Ctx, String, String) {
    let d = tmpdir(name);
    let c = ctx(&d, CONFIG);
    let wt = |n: &str| {
        let p = c.worktrees.join("sendmeter").join(n);
        std::fs::create_dir_all(&p).unwrap();
        p.to_string_lossy().into_owned()
    };
    let (mine, sibling) = (wt("issue-12"), wt("issue-120"));
    let ev = |kind: &str, d| append(&c.state, &Event::new(10, "sendmeter", Some(12), kind, d)).unwrap();
    ev(
        "spawn",
        json!({"role": "impl", "agent": "impl-12", "worktree": mine, "pane": "wA:p1", "workspace": "wA"}),
    );
    ev("merged", json!({"pr": "1", "merge_sha": "m", "into": "staging"}));
    (d, c, mine, sibling)
}

/// `lsof -Fpcn` rows: two children in the lane's worktree, one in a sibling (prefix trap),
/// one outside, and the board server (`herdsman serve`) started from inside it.
fn lsof(mine: &str, sibling: &str) -> String {
    format!(
        "p101\ncnode\nfcwd\nn{mine}/web\np102\ncChrome Helper\nfcwd\nn{mine}\n\
         p103\ncvite\nfcwd\nn{sibling}\np104\ncnode\nfcwd\nn/Users/someone\np105\ncherdsman\nfcwd\nn{mine}\n"
    )
}

fn fake(lsof: &str) -> Fake {
    Fake::default()
        .once("lsof", 0, lsof)
        .reply("herdr agent list", &agents(&[]))
        .reply("git -C", "")
        .once("git -C", 0, "") // status: clean
        .once("git -C", 0, "0\n") // rev-list: pushed
        .reply("herdr", "{}")
        .reply("kill", "")
}

fn kills(r: &Fake) -> Vec<String> {
    r.calls
        .borrow()
        .iter()
        .filter(|c| c.starts_with("kill"))
        .cloned()
        .collect()
}

#[test]
fn closing_a_lane_terms_then_kills_only_its_worktree_pids() {
    let (_d, c, mine, sibling) = setup("reap-kill");
    let rows = lsof(&mine, &sibling);
    let r = fake(&rows).reply("lsof", &rows); // nothing dies on TERM
    let (code, out) = run(&c, &r, &["clean", "sendmeter", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(kills(&r), ["kill -TERM 101 102", "kill -KILL 101 102"]);
    let (term, wait, kill) = (r.index("kill -TERM"), r.index("sleep 5"), r.index("kill -KILL"));
    assert!(
        term < wait && wait < kill,
        "TERM, 5 s, then KILL: {:?}",
        r.calls.borrow()
    );
    assert!(
        r.index("kill") < r.index("herdr worktree remove"),
        "reaped before the worktree goes: {:?}",
        r.calls.borrow()
    );
    let ev = read(&c.state)
        .unwrap()
        .into_iter()
        .find(|e| e.kind == "lane-reaped")
        .unwrap();
    assert_eq!(ev.issue, Some(12));
    assert_eq!(ev.data["killed"], 2);
    assert_eq!(ev.data["names"], json!(["node", "Chrome Helper"]));
}

#[test]
fn what_dies_on_term_gets_no_kill_and_an_empty_tree_is_not_logged() {
    let (_d, c, mine, sibling) = setup("reap-term");
    let r = fake(&lsof(&mine, &sibling)).reply("lsof", &lsof("/gone", &sibling));
    let (code, out) = run(&c, &r, &["clean", "sendmeter", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(kills(&r), ["kill -TERM 101 102"]);

    let (_d, c, _, sibling) = setup("reap-none");
    let r = fake(&lsof("/elsewhere", &sibling));
    let (code, out) = run(&c, &r, &["clean", "sendmeter", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(kills(&r).is_empty() && !r.called("sleep 5"), "{:?}", r.calls.borrow());
    assert!(!read(&c.state).unwrap().iter().any(|e| e.kind == "lane-reaped"));
}

#[test]
fn a_dry_run_and_a_live_lane_signal_nothing() {
    let (_d, c, mine, sibling) = setup("reap-dry");
    let r = fake(&lsof(&mine, &sibling));
    let (code, out) = run(&c, &r, &["clean", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(kills(&r).is_empty(), "dry run");

    let r = Fake::default()
        .reply("lsof", &lsof(&mine, &sibling))
        .reply("herdr agent list", &agents(&[("impl-12", "idle", &mine, "wA:p1")]))
        .reply("git -C", "")
        .reply("kill", "");
    let (code, out) = run(&c, &r, &["clean", "sendmeter", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(kills(&r).is_empty(), "a live agent's tree is its own: {out}");
}

#[test]
fn host_tab_shows_leftovers_and_reap_kills_them() {
    use herdsman::serve::Server;
    const ADDR: &str = "127.0.0.1:7777";
    let (_d, c, mine, sibling) = setup("reap-host");
    let rows = lsof(&mine, &sibling);
    let r = Fake::default()
        .reply("lsof", &rows)
        .reply("herdr agent list", &agents(&[]))
        .reply("ps -o rss= -p 101,102", "  204800\n  102400\n")
        .reply("kill", "");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let page =
        String::from_utf8_lossy(&s.respond(&format!("GET / HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), 1_000_000)).into_owned();
    assert!(page.contains("<span class=k>leftovers</span>"), "no leftovers row");
    assert!(page.contains("2 running</b> <span class=mt>300 MB"), "count + memory");
    assert!(
        page.contains("name=op value=reap") && page.contains("data-confirm="),
        "a confirmed [reap]"
    );
    assert!(kills(&r).is_empty(), "rendering signals nothing");
    let i = page.find("name=mtime value=").unwrap() + 17;
    let mt: String = page[i..].chars().take_while(char::is_ascii_digit).collect();
    let body = format!("op=reap&fleet=&mtime={mt}");
    let req = format!(
        "POST / HTTP/1.1\r\nHost: {ADDR}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&req, 1_000_000)).into_owned();
    assert!(res.contains("reaped%202") || res.contains("reaped 2"), "{res}");
    assert_eq!(kills(&r), ["kill -TERM 101 102", "kill -KILL 101 102"]);
    assert!(read(&c.state).unwrap().iter().any(|e| e.kind == "lane-reaped"));
}
