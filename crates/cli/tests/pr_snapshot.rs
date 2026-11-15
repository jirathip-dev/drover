mod common;
use common::*;
use drover::events::{append, read, Event};
use serde_json::json;

const HEAD: &str = "1111111111111111111111111111111111111111";
const ALPHA: &str = "[fleet.alpha]\nrepo = \"jirathip-dev/alpha\"\nlocal = \"/tmp/does-not-matter/alpha\"\n\
    integration = \"staging\"\n\n[fleet.alpha.role.impl]\nharness = \"hermes\"\nprofile = \"fleet-impl\"\n\n[limits]";

fn lane(c: &drover::Ctx, fleet: &str, n: u64) {
    let d = json!({"role": "impl", "agent": format!("{fleet}-impl-{n}"), "branch": format!("issue-{n}")});
    append(&c.state, &Event::new(10, fleet, Some(n), "spawn", d)).unwrap();
}

fn open_prs(ns: &[u64]) -> String {
    let check = json!({"__typename": "CheckRun", "name": "build", "status": "COMPLETED", "conclusion": "SUCCESS"});
    let line = |n: &u64| {
        let pr = json!({"number": 100 + n, "state": "OPEN", "baseRefName": "staging", "headRefName": format!("issue-{n}"),
            "headRefOid": HEAD, "mergeable": "MERGEABLE", "labels": [], "statusCheckRollup": [check]});
        format!("{pr}\n")
    };
    ns.iter().map(line).collect()
}

fn calm() -> Fake {
    Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
}

#[test]
fn a_fast_step_reads_every_open_pr_in_one_graphql_call_per_fleet() {
    let c = ctx(&tmpdir("snap-one-call"), &CONFIG.replace("[limits]", ALPHA));
    let sh = FakeShed::new();
    sh.attach(&c);
    for n in [12, 13, 14, 15] {
        lane(&c, "sendmeter", n);
    }
    lane(&c, "alpha", 5);
    let r = calm().reply("gh api graphql", &open_prs(&[12, 13, 14, 15, 5]));
    let (code, out) = run(&c, &r, &["drive", "--fast"]);
    assert_eq!(code, 0, "{out}");
    let calls = r.calls.borrow();
    let graphql: Vec<_> = calls.iter().filter(|l| l.starts_with("gh api graphql")).collect();
    assert_eq!(graphql.len(), 2, "one per fleet: {graphql:?}");
    assert!(graphql.iter().any(|l| l.contains("name=sendmeter")) && graphql.iter().any(|l| l.contains("name=alpha")));
    assert!(graphql
        .iter()
        .all(|l| l.contains("--paginate") && l.contains("first:100")));
    assert!(
        !calls.iter().any(|l| l.starts_with("gh pr view")),
        "no per-PR read: {calls:?}"
    );
    for n in [12, 13, 14, 15] {
        assert_eq!(drover::gh::cached_pr(&c, "sendmeter", n)["number"], 100 + n);
    }
    let (at, secs) = drover::drive::fast_step(&c).unwrap();
    assert_eq!((at, secs.is_some()), (c.now, true));
}

#[test]
fn the_merge_gate_reads_checks_from_the_snapshot() {
    let c = ctx(&tmpdir("snap-merge"), CONFIG);
    lane(&c, "sendmeter", 12);
    let v = json!({"verdict": "PASS", "sha": HEAD});
    append(&c.state, &Event::new(20, "sendmeter", Some(12), "verdict", v)).unwrap();
    let r = Fake::default()
        .reply("gh api graphql", &open_prs(&[12]))
        .reply("gh pr merge 112", "")
        .reply("gh pr view 112", r#"{"mergeCommit":{"oid":"m3rg3"}}"#)
        .reply("gh issue", "");
    let (code, out) = run(&c, &r, &["merge", "sendmeter", "12"]);
    assert_eq!(code, 0, "{out}");
    assert!(r.called(&format!(
        "gh pr merge 112 -R jirathip-dev/sendmeter --merge --match-head-commit {HEAD}"
    )));
    assert!(!r.called("gh pr view issue-12"), "{:?}", r.calls.borrow());
    assert!(read(&c.state).unwrap().iter().any(|e| e.kind == "merged"));
}

#[test]
fn status_and_serve_make_no_gh_call() {
    let c = ctx(&tmpdir("snap-read-only"), CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
    lane(&c, "sendmeter", 12);
    let r = calm().reply("gh api graphql", &open_prs(&[12]));
    assert_eq!(run(&c, &r, &["drive", "--fast"]).0, 0);
    let seen = r.calls.borrow().len();
    let (code, out) = run(&c, &r, &["status"]);
    assert_eq!(code, 0, "{out}");
    drover::serve::Server::new(&c, &r, None).respond("GET / HTTP/1.1\r\n\r\n", c.now);
    let after = &r.calls.borrow()[seen..];
    assert!(!after.iter().any(|l| l.starts_with("gh ")), "{after:?}");
}

fn probe(status: &str, etag: &str) -> String {
    format!("HTTP/2.0 {status}\r\nContent-Type: application/json\r\nEtag: {etag}\r\n\r\n[]")
}

#[test]
fn an_unchanged_fast_step_spends_only_304s_and_30_prs_need_one_query() {
    let c = ctx(&tmpdir("snap-etag"), CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
    let ns: Vec<u64> = (1..=30).collect();
    for n in &ns {
        lane(&c, "sendmeter", *n);
    }
    let pulls = "gh api -i repos/jirathip-dev/sendmeter/pulls?";
    let r = calm()
        .reply("gh api graphql", &open_prs(&ns))
        .reply(pulls, &probe("200 OK", "W/\"p1\""))
        .reply(
            "gh api -i repos/jirathip-dev/sendmeter/actions/runs",
            &probe("200 OK", "\"a1\""),
        )
        .reply(
            "gh api -i repos/jirathip-dev/sendmeter/branches/staging",
            &probe("200 OK", "\"b1\""),
        )
        .reply("gh api -i -H If-None-Match: ", &probe("304 Not Modified", "\"x\""));
    let mut c = c;
    assert_eq!(run(&c, &r, &["drive", "--fast"]).0, 0);
    assert_eq!(
        r.count("gh api graphql"),
        1,
        "30 PRs, one query: {:?}",
        r.calls.borrow()
    );
    assert_eq!(drover::gh::cached_pr(&c, "sendmeter", 30)["number"], 130);
    c.now += 15;
    let seen = r.calls.borrow().len();
    assert_eq!(run(&c, &r, &["drive", "--fast"]).0, 0);
    let gh: Vec<_> = r.calls.borrow()[seen..]
        .iter()
        .filter(|l| l.starts_with("gh "))
        .cloned()
        .collect();
    assert_eq!(
        gh,
        [
            "gh api -i -H If-None-Match: W/\"p1\" repos/jirathip-dev/sendmeter/pulls?state=open&sort=updated&direction=desc&per_page=100",
            "gh api -i -H If-None-Match: \"a1\" repos/jirathip-dev/sendmeter/actions/runs?per_page=50",
            "gh api -i -H If-None-Match: \"b1\" repos/jirathip-dev/sendmeter/branches/staging",
        ]
    );
    assert_eq!(
        drover::gh::cached_at(&c, "sendmeter", 30),
        Some(c.now),
        "the cards stay fresh"
    );
    let r2 = calm()
        .reply("gh api graphql", &open_prs(&ns))
        .reply("gh api -i -H If-None-Match: W/\"p1\"", &probe("200 OK", "W/\"p2\""))
        .reply("gh api -i -H If-None-Match: ", &probe("304 Not Modified", "\"x\""));
    c.now += 15;
    assert_eq!(run(&c, &r2, &["drive", "--fast"]).0, 0);
    assert_eq!(r2.count("gh api graphql"), 1, "{:?}", r2.calls.borrow());
}
