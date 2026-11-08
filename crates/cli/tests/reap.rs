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
    let page = host_tab(&s, ADDR, 1_000_000);
    assert!(page.contains("<span class=k>leftovers</span>"), "no leftovers row");
    assert!(
        page.contains("<b class=a>2</b> processes left in closed lanes <span class=mt>· 300 MB"),
        "count + memory"
    );
    assert!(
        page.contains("name=op value=reap") && page.contains("data-confirm="),
        "a confirmed [reap]"
    );
    assert!(kills(&r).is_empty(), "rendering signals nothing");
    let i = page.find("name=mtime value=").unwrap() + 17;
    let mt: String = page[i..].chars().take_while(char::is_ascii_digit).collect();
    let body = format!("op=reap&fleet=&mtime={mt}");
    let unpaired = format!(
        "POST / HTTP/1.1\r\nHost: {ADDR}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&unpaired, 1_000_000)).into_owned();
    assert!(res.starts_with("HTTP/1.1 401"), "only a paired device reaps: {res}");
    assert!(kills(&r).is_empty());
    let cookie = pair(&s, &c, "", 1_000_000);
    let req = format!(
        "POST / HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&req, 1_000_000)).into_owned();
    assert!(res.contains("reaped%202") || res.contains("reaped 2"), "{res}");
    assert_eq!(kills(&r), ["kill -TERM 101 102", "kill -KILL 101 102"]);
    assert!(read(&c.state).unwrap().iter().any(|e| e.kind == "lane-reaped"));
}

#[test]
fn check_reaps_closed_lanes_but_spares_active_and_live_ones() {
    let d = tmpdir("reap-check");
    let c = ctx(&d, CONFIG);
    let lane = |issue: u64, merged: bool| {
        let wt = c.worktrees.join("sendmeter").join(format!("issue-{issue}"));
        std::fs::create_dir_all(&wt).unwrap();
        let wt = wt.to_string_lossy().into_owned();
        let ev = |kind: &str, d| append(&c.state, &Event::new(10, "sendmeter", Some(issue), kind, d)).unwrap();
        ev(
            "spawn",
            json!({"role": "impl", "agent": format!("impl-{issue}"), "worktree": wt}),
        );
        if merged {
            ev("merged", json!({"pr": "1", "merge_sha": "m", "into": "staging"}));
        }
        wt
    };
    let (closed, active, live) = (lane(12, true), lane(15, false), lane(16, true));
    let rows = format!(
        "p101\ncGoogle Chrome Helper\nfcwd\nn{closed}\np505\ncnode\nfcwd\nn{active}\np606\ncvite\nfcwd\nn{live}\n"
    );
    let r = Fake::default()
        .reply("herdr agent list", &agents(&[("impl-16", "idle", &live, "w16:p1")]))
        .once("lsof", 0, &rows)
        .once("lsof", 0, &rows)
        .reply("lsof", "")
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

/// #223: a `done` lane past `idle_reap_minutes` loses the processes its agent started, never its shell or agent.
#[test]
fn check_trims_a_done_lanes_children_but_not_its_agent() {
    let d = tmpdir("trim-check");
    let c = ctx(&d, CONFIG);
    let wt = c.worktrees.join("sendmeter").join("issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let wt = wt.to_string_lossy().into_owned();
    let spawn = json!({"role": "impl", "agent": "impl-12", "worktree": wt, "pane": "wA:p1"});
    append(&c.state, &Event::new(10, "sendmeter", Some(12), "spawn", spawn)).unwrap();
    // pane shell 200 (parent: herdr) → agent 201 → tsserver 202 → its helper 203; 300 sits outside the worktree.
    let rows = format!("p200\nczsh\nfcwd\nn{wt}\np201\nc2.1.309\nfcwd\nn{wt}\np202\ncnode\nfcwd\nn{wt}/web\np203\ncnode\nfcwd\nn{wt}\np300\ncnode\nfcwd\nn/elsewhere\n");
    let ps = "  199     1   9000\n  200   199   4000\n  201   200 300000\n  202   201 2097152\n  203   202 102400\n  300   201   5000\n";
    let fake = |status: &str| {
        Fake::default()
            .reply("herdr agent list", &agents(&[("impl-12", status, &wt, "wA:p1")]))
            .once("lsof", 0, &rows)
            .reply("lsof", "")
            .reply("ps -axo", ps)
            .reply("herdr", "{}")
            .reply("git -C", "")
            .reply("kill", "")
    };
    let trimmed = |c: &herdsman::Ctx| {
        read(&c.state)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == "lane-trimmed")
            .collect::<Vec<_>>()
    };

    let r = fake("working");
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(kills(&r).is_empty(), "a working lane keeps its children");

    let r = fake("done");
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        kills(&r),
        ["kill -TERM 202 203"],
        "only the agent's children: {:?}",
        r.calls.borrow()
    );
    let ev = trimmed(&c);
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].data["agent"], "impl-12");
    assert_eq!(ev[0].data["killed"], json!(["node", "node"]));
    assert_eq!(ev[0].data["freed_mb"], 2148);

    let young = c.now - 60; // done, but its last event is under `idle_reap_minutes` old
    let c2 = herdsman::Ctx {
        now: young + 120,
        ..ctx(&tmpdir("trim-young"), CONFIG)
    };
    append(
        &c2.state,
        &Event::new(
            young,
            "sendmeter",
            Some(12),
            "spawn",
            json!({"role": "impl", "agent": "impl-12", "worktree": wt}),
        ),
    )
    .unwrap();
    let r = fake("done");
    let (code, out) = run(&c2, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(kills(&r).is_empty() && trimmed(&c2).is_empty(), "too soon to trim");
}

/// #223: the Host tab names the top memory users and their owner: a lane, the VM, or other.
#[test]
fn host_tab_shows_top_memory_users_with_their_owner() {
    use herdsman::serve::Server;
    let (_d, c, mine, _) = setup("top-host");
    let top = "Processes: 600 total\n\nPID    COMMAND          MEM \n18800  com.apple.Virtua 16G \n4242   node             2210M\n8138   python3.14       703M\n";
    let r = Fake::default()
        .reply("lsof", &format!("p4242\ncnode\nfcwd\nn{mine}\n"))
        .reply("herdr agent list", &agents(&[("impl-12", "done", &mine, "wA:p1")]))
        .reply("top -l 1 -o mem", top);
    let s = Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    let page = host_tab(&s, "127.0.0.1:7777", 1_000_000);
    assert!(page.contains("<span class=k>top memory</span>"), "no top memory row");
    assert!(page.contains("16G VM (colima) · "), "the VM is classified");
    assert!(
        page.contains("2210M node (lane impl-12)"),
        "a lane's process names the lane"
    );
    assert!(page.contains("703M python3.14 (other)"));
}

/// #248: the Host tab sums what the Claude lanes hold, working and done apart.
#[test]
fn host_tab_sums_claude_lane_memory_by_state() {
    use herdsman::serve::Server;
    let (_d, c, mine, sibling) = setup("claude-host");
    let mut list: serde_json::Value = serde_json::from_str(&agents(&[
        ("impl-12", "done", &mine, "wA:p1"),
        ("impl-120", "working", &sibling, "wA:p2"),
    ]))
    .unwrap();
    for a in list["result"]["agents"].as_array_mut().unwrap() {
        a["agent"] = json!("claude");
    }
    let r = Fake::default()
        .reply(
            "lsof",
            &format!("p201\nc2.1.311\nfcwd\nn{mine}\np202\nc2.1.311\nfcwd\nn{sibling}\n"),
        )
        .reply("ps -axo pid=,rss=", "  201 2097152\n  202 1048576\n")
        .reply("herdr agent list", &list.to_string());
    let s = Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    let page = host_tab(&s, "127.0.0.1:7777", 1_000_000);
    assert!(page.contains("<span class=k>claude</span>"), "no claude row");
    assert!(
        page.contains("<b>3.0 GB</b> <span class=mt>(2 lanes: working 1 / done 1)</span>"),
        "the sum by state"
    );
}

/// #679: a shed-acp lane `done` between turns keeps its agent's tree (adapter, `node`, `claude`, their background job);
/// while its last turn is `working` nothing is trimmed; after a `turn done` only what is not the agent's goes.
#[test]
fn check_never_trims_an_acp_lanes_agent_or_its_working_turn() {
    let d = tmpdir("trim-acp");
    let c = ctx(&d, CONFIG);
    let wt = c.worktrees.join("sendmeter").join("issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let wt = wt.to_string_lossy().into_owned();
    let ev = |kind: &str, d| append(&c.state, &Event::new(10, "sendmeter", Some(12), kind, d)).unwrap();
    ev(
        "spawn",
        json!({"role": "impl", "agent": "impl-12", "worktree": wt, "pane": "impl-12", "runtime": "shed", "harness": "claude"}),
    );
    let why = "full cargo test running in background";
    ev("turn-end", json!({"state": "working", "why": why, "role": "impl"}));
    // shed server 199 (outside) → sh 200 (the terminal's pid) → shed acp-bridge 201 → node 202 → claude 203 → zsh 204
    // → cargo 205; a stray shell 300 (parent: launchd) → zsh 301 → vite 302, not the agent's.
    let rows = format!(
        "p200\ncsh\nfcwd\nn{wt}\np201\ncshed\nfcwd\nn{wt}\np202\ncnode\nfcwd\nn{wt}\np203\ncclaude\nfcwd\nn{wt}\n\
         p204\nczsh\nfcwd\nn{wt}\np205\nccargo\nfcwd\nn{wt}\np300\nczsh\nfcwd\nn{wt}\np301\nczsh\nfcwd\nn{wt}\n\
         p302\ncvite\nfcwd\nn{wt}/web\n"
    );
    let ps = "  199     1   9000\n  200   199   1000\n  201   200   2000\n  202   201 300000\n  203   202 400000\n  \
              204   203   3000\n  205   204 900000\n  300     1   1000\n  301   300   1000\n  302   301 102400\n";
    let list = json!({"result": {"agents": [{"name": "impl-12", "agent_status": "done", "cwd": wt, "pane_id": "impl-12",
        "agent": "claude", "runtime": "shed", "pid": 200}]}});
    let fake = || {
        Fake::default()
            .reply("herdr agent list", &list.to_string())
            .once("lsof", 0, &rows)
            .reply("lsof", "")
            .reply("ps -axo", ps)
            .reply("herdr", "{}")
            .reply("git -C", "")
            .reply("kill", "")
    };
    let trimmed = |c: &herdsman::Ctx| {
        let e = read(&c.state).unwrap();
        e.into_iter().filter(|e| e.kind == "lane-trimmed").collect::<Vec<_>>()
    };

    let r = fake();
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        kills(&r).is_empty(),
        "a working turn keeps everything: {:?}",
        r.calls.borrow()
    );
    assert!(trimmed(&c).is_empty());

    ev("turn-end", json!({"state": "done", "role": "impl"}));
    let r = fake();
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        kills(&r),
        ["kill -TERM 302"],
        "only the stray tree: {:?}",
        r.calls.borrow()
    );
    let ev = trimmed(&c);
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].data["killed"], json!(["vite"]), "no lane-trimmed names the agent");
}

/// #679: a live shed lane whose last turn is `working` is idle between turns: `check` alerts no stall; a herdr lane
/// still does.
#[test]
fn check_alerts_no_stall_for_an_acp_lane_between_turns() {
    for (runtime, stalls) in [("shed", false), ("herdr", true)] {
        let c = ctx(&tmpdir(&format!("stall-acp-{runtime}")), CONFIG);
        let wt = c.worktrees.join("sendmeter").join("issue-12");
        std::fs::create_dir_all(&wt).unwrap();
        let wt = wt.to_string_lossy().into_owned();
        let ev = |kind: &str, d| append(&c.state, &Event::new(10, "sendmeter", Some(12), kind, d)).unwrap();
        ev(
            "spawn",
            json!({"role": "impl", "agent": "impl-12", "worktree": wt, "pane": "impl-12", "runtime": runtime}),
        );
        ev("turn-end", json!({"state": "working", "role": "impl"}));
        let list = json!({"result": {"agents": [{"name": "impl-12", "agent_status": "done", "cwd": wt,
            "pane_id": "impl-12", "runtime": runtime, "pid": 200}]}});
        let r = Fake::default()
            .reply("herdr agent list", &list.to_string())
            .reply("lsof", "")
            .reply("herdr", "{}")
            .reply("git -C", "");
        let (code, out) = run(&c, &r, &["check"]);
        assert_eq!(code, 0, "{out}");
        let stalled = read(&c.state).unwrap().iter().any(|e| e.kind == "stall");
        assert_eq!(stalled, stalls, "{runtime}: {out}");
    }
}
