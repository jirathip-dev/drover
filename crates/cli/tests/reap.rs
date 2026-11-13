mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::json;

fn setup(name: &str) -> (std::path::PathBuf, drover::Ctx, String, String, FakeShed) {
    let d = tmpdir(name);
    let c = ctx(&d, CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
    let wt = |n: &str| {
        let p = c.worktrees.join("sendmeter").join(n);
        std::fs::create_dir_all(&p).unwrap();
        p.to_string_lossy().into_owned()
    };
    let (mine, sibling) = (wt("issue-12"), wt("issue-120"));
    let ev = |kind: &str, d| append(&c.state, &Event::new(10, "sendmeter", Some(12), kind, d)).unwrap();
    ev(
        "spawn",
        json!({"role": "impl", "agent": "impl-12", "worktree": mine, "pane": "impl-12", "runtime": "shed"}),
    );
    ev("merged", json!({"pr": "1", "merge_sha": "m", "into": "staging"}));
    (d, c, mine, sibling, sh)
}

fn lsof(mine: &str, sibling: &str) -> String {
    format!(
        "p101\ncnode\nfcwd\nn{mine}/web\np102\ncChrome Helper\nfcwd\nn{mine}\n\
         p103\ncvite\nfcwd\nn{sibling}\np104\ncnode\nfcwd\nn/Users/someone\np105\ncdrover\nfcwd\nn{mine}\n"
    )
}

fn fake(lsof: &str, wt: &str) -> Fake {
    Fake::default()
        .once("lsof", 0, lsof)
        .reply(&format!("git -C {wt} rev-list --count"), "0\n")
        .reply("git -C", "")
        .reply("kill", "")
}

fn at(r: &Fake, what: &str) -> Option<usize> {
    r.calls.borrow().iter().position(|c| c.contains(what))
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
    let (_d, c, mine, sibling, _sh) = setup("reap-kill");
    let rows = lsof(&mine, &sibling);
    let r = fake(&rows, &mine).reply("lsof", &rows);
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
        at(&r, "worktree remove").is_some() && r.index("kill") < at(&r, "worktree remove"),
        "reaped before the worktree goes: {:?}",
        r.calls.borrow()
    );
    let ev = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .find(|e| e.kind == "lane-reaped")
        .unwrap();
    assert_eq!(ev.issue, Some(12));
    assert_eq!(ev.data["killed"], 2);
    assert_eq!(ev.data["names"], json!(["node", "Chrome Helper"]));
}

#[test]
fn what_dies_on_term_gets_no_kill_and_an_empty_tree_is_not_logged() {
    let (_d, c, mine, sibling, _sh) = setup("reap-term");
    let r = fake(&lsof(&mine, &sibling), &mine).reply("lsof", &lsof("/gone", &sibling));
    let (code, out) = run(&c, &r, &["clean", "sendmeter", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(kills(&r), ["kill -TERM 101 102"]);

    let (_d, c, mine, sibling, _sh) = setup("reap-none");
    let r = fake(&lsof("/elsewhere", &sibling), &mine);
    let (code, out) = run(&c, &r, &["clean", "sendmeter", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(kills(&r).is_empty() && !r.called("sleep 5"), "{:?}", r.calls.borrow());
    assert!(!read(&c.state).unwrap().iter().any(|e| e.kind == "lane-reaped"));
}

#[test]
fn a_dry_run_and_a_live_lane_signal_nothing() {
    let (_d, c, mine, sibling, sh) = setup("reap-dry");
    let r = fake(&lsof(&mine, &sibling), &mine);
    let (code, out) = run(&c, &r, &["clean", "sendmeter"]);
    assert_eq!(code, 0, "{out}");
    assert!(kills(&r).is_empty(), "dry run");

    sh.lane("impl-12", "idle", &mine);
    let r = Fake::default()
        .reply("lsof", &lsof(&mine, &sibling))
        .reply("git -C", "")
        .reply("kill", "");
    let (code, out) = run(&c, &r, &["clean", "sendmeter", "--yes"]);
    assert_eq!(code, 0, "{out}");
    assert!(kills(&r).is_empty(), "a live agent's tree is its own: {out}");
}

#[test]
fn host_tab_shows_leftovers_and_reap_kills_them() {
    use drover::serve::Server;
    const ADDR: &str = "127.0.0.1:7777";
    let (_d, c, mine, sibling, sh) = setup("reap-host");
    let rows = lsof(&mine, &sibling);
    let r = Fake::default()
        .reply("lsof", &rows)
        .reply("ps -o rss= -p 101,102", "  204800\n  102400\n")
        .reply("kill", "");
    let on = sh.on(&r);
    let s = Server::new(&c, &on, Some(ADDR.into()));
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
    let sh = FakeShed::new();
    sh.attach(&c);
    let lane = |issue: u64, merged: bool| {
        let wt = c.worktrees.join("sendmeter").join(format!("issue-{issue}"));
        std::fs::create_dir_all(&wt).unwrap();
        let wt = wt.to_string_lossy().into_owned();
        let ev = |kind: &str, d| append(&c.state, &Event::new(10, "sendmeter", Some(issue), kind, d)).unwrap();
        ev(
            "spawn",
            json!({"role": "impl", "agent": format!("impl-{issue}"), "worktree": wt, "pane": format!("impl-{issue}"), "runtime": "shed"}),
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
    sh.lane("impl-16", "idle", &live);
    let r = Fake::default()
        .once("lsof", 0, &rows)
        .once("lsof", 0, &rows)
        .reply("lsof", "");
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        kills(&r),
        ["kill -TERM 101"],
        "gone after TERM: no KILL; active and live lanes untouched"
    );
    let reaped: Vec<_> = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == "lane-reaped")
        .collect();
    assert_eq!(reaped.len(), 1);
    assert_eq!(reaped[0].data["names"], json!(["Google Chrome Helper"]));
}

#[test]
fn check_trims_a_done_lanes_children_but_not_its_agent() {
    let d = tmpdir("trim-check");
    let c = ctx(&d, CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
    let wt = c.worktrees.join("sendmeter").join("issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let wt = wt.to_string_lossy().into_owned();
    let spawn = json!({"role": "impl", "agent": "impl-12", "worktree": wt, "pane": "impl-12", "runtime": "shed"});
    append(&c.state, &Event::new(10, "sendmeter", Some(12), "spawn", spawn)).unwrap();
    let rows = format!(
        "p200\nczsh\nfcwd\nn{wt}\np201\nc2.1.309\nfcwd\nn{wt}\np202\ncnode\nfcwd\nn{wt}/web\np203\ncnode\nfcwd\nn{wt}\n\
         p300\ncnode\nfcwd\nn/elsewhere\np400\nczsh\nfcwd\nn{wt}\np401\nczsh\nfcwd\nn{wt}\np402\ncnode\nfcwd\nn{wt}/web\n\
         p403\ncnode\nfcwd\nn{wt}\n"
    );
    let ps = "  199     1   9000\n  200   199   4000\n  201   200 300000\n  202   201 2097152\n  203   202 102400\n  \
              300   201   5000\n  400     1   1000\n  401   400   1000\n  402   401 2097152\n  403   402 102400\n";
    let fake = |sh: &FakeShed, status: &str| {
        sh.lane("impl-12", status, &wt);
        sh.set("impl-12", |t| t["pid"] = json!(200));
        Fake::default()
            .once("lsof", 0, &rows)
            .reply("lsof", "")
            .reply("ps -axo", ps)
            .reply("git -C", "")
            .reply("kill", "")
    };
    let trimmed = |c: &drover::Ctx| {
        read(&c.state)
            .unwrap()
            .to_vec()
            .into_iter()
            .filter(|e| e.kind == "lane-trimmed")
            .collect::<Vec<_>>()
    };

    let r = fake(&sh, "working");
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(kills(&r).is_empty(), "a working lane keeps its children");

    let r = fake(&sh, "done");
    let (code, out) = run(&c, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        kills(&r),
        ["kill -TERM 402 403"],
        "only the leftovers, never the agent's tree: {:?}",
        r.calls.borrow()
    );
    let ev = trimmed(&c);
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].data["agent"], "impl-12");
    assert_eq!(ev[0].data["killed"], json!(["node", "node"]));
    assert_eq!(ev[0].data["freed_mb"], 2148);

    let young = c.now - 60;
    let c2 = drover::Ctx {
        now: young + 120,
        ..ctx(&tmpdir("trim-young"), CONFIG)
    };
    let sh2 = FakeShed::new();
    sh2.attach(&c2);
    append(
        &c2.state,
        &Event::new(
            young,
            "sendmeter",
            Some(12),
            "spawn",
            json!({"role": "impl", "agent": "impl-12", "worktree": wt, "pane": "impl-12", "runtime": "shed"}),
        ),
    )
    .unwrap();
    let r = fake(&sh2, "done");
    let (code, out) = run(&c2, &r, &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(kills(&r).is_empty() && trimmed(&c2).is_empty(), "too soon to trim");
}

#[test]
fn host_tab_shows_top_memory_users_with_their_owner() {
    use drover::serve::Server;
    let (_d, c, mine, _, sh) = setup("top-host");
    sh.lane("impl-12", "done", &mine);
    let top = "Processes: 600 total\n\nPID    COMMAND          MEM \n18800  com.apple.Virtua 16G \n4242   node             2210M\n8138   python3.14       703M\n";
    let r = Fake::default()
        .reply("lsof", &format!("p4242\ncnode\nfcwd\nn{mine}\n"))
        .reply("top -l 1 -o mem", top);
    let on = sh.on(&r);
    let s = Server::new(&c, &on, Some("127.0.0.1:7777".into()));
    let page = host_tab(&s, "127.0.0.1:7777", 1_000_000);
    assert!(page.contains("<span class=k>top memory</span>"), "no top memory row");
    assert!(page.contains("16G VM (colima) · "), "the VM is classified");
    assert!(
        page.contains("2210M node (lane impl-12)"),
        "a lane's process names the lane"
    );
    assert!(page.contains("703M python3.14 (other)"));
}

#[test]
fn host_tab_sums_claude_lane_memory_by_state() {
    use drover::serve::Server;
    let (_d, c, mine, sibling, sh) = setup("claude-host");
    sh.lane("impl-12", "done", &mine);
    sh.lane("impl-120", "working", &sibling);
    let r = Fake::default()
        .reply(
            "lsof",
            &format!("p201\nc2.1.311\nfcwd\nn{mine}\np202\nc2.1.311\nfcwd\nn{sibling}\n"),
        )
        .reply("ps -axo pid=,rss=", "  201 2097152\n  202 1048576\n");
    let on = sh.on(&r);
    let s = Server::new(&c, &on, Some("127.0.0.1:7777".into()));
    let page = host_tab(&s, "127.0.0.1:7777", 1_000_000);
    assert!(page.contains("<span class=k>claude</span>"), "no claude row");
    assert!(
        page.contains("<b>3.0 GB</b> <span class=mt>(2 lanes: working 1 / done 1)</span>"),
        "the sum by state"
    );
}

#[test]
fn check_never_trims_an_acp_lanes_agent_or_its_working_turn() {
    let d = tmpdir("trim-acp");
    let c = ctx(&d, CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
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
    let rows = format!(
        "p200\ncsh\nfcwd\nn{wt}\np201\ncshed\nfcwd\nn{wt}\np202\ncnode\nfcwd\nn{wt}\np203\ncclaude\nfcwd\nn{wt}\n\
         p204\nczsh\nfcwd\nn{wt}\np205\nccargo\nfcwd\nn{wt}\np300\nczsh\nfcwd\nn{wt}\np301\nczsh\nfcwd\nn{wt}\n\
         p302\ncvite\nfcwd\nn{wt}/web\n"
    );
    let ps = "  199     1   9000\n  200   199   1000\n  201   200   2000\n  202   201 300000\n  203   202 400000\n  \
              204   203   3000\n  205   204 900000\n  300     1   1000\n  301   300   1000\n  302   301 102400\n";
    sh.lane("impl-12", "done", &wt);
    sh.set("impl-12", |t| t["pid"] = json!(200));
    let fake = || {
        Fake::default()
            .once("lsof", 0, &rows)
            .reply("lsof", "")
            .reply("ps -axo", ps)
            .reply("git -C", "")
            .reply("kill", "")
    };
    let trimmed = |c: &drover::Ctx| {
        let e = read(&c.state).unwrap().to_vec();
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

#[test]
fn check_alerts_no_stall_for_an_acp_lane_between_turns() {
    for (turn, stalls) in [("working", false), ("done", true)] {
        let c = ctx(&tmpdir(&format!("stall-acp-{turn}")), CONFIG);
        let sh = FakeShed::new();
        sh.attach(&c);
        let wt = c.worktrees.join("sendmeter").join("issue-12");
        std::fs::create_dir_all(&wt).unwrap();
        let wt = wt.to_string_lossy().into_owned();
        let ev = |kind: &str, d| append(&c.state, &Event::new(10, "sendmeter", Some(12), kind, d)).unwrap();
        ev(
            "spawn",
            json!({"role": "impl", "agent": "impl-12", "worktree": wt, "pane": "impl-12", "runtime": "shed"}),
        );
        ev("turn-end", json!({"state": turn, "role": "impl"}));
        sh.lane("impl-12", "done", &wt);
        sh.set("impl-12", |t| t["pid"] = json!(200));
        let r = Fake::default().reply("lsof", "").reply("git -C", "");
        let (code, out) = run(&c, &r, &["check"]);
        assert_eq!(code, 0, "{out}");
        let stalled = read(&c.state).unwrap().iter().any(|e| e.kind == "stall");
        assert_eq!(stalled, stalls, "{turn}: {out}");
    }
}
