//! #169: one link builder wherever an issue shows: `[issue] [PR #M] [checks]`, `checks` the PR's checks page
//! (a reviewer's event too), else the issue branch's latest run read by the tick; never a query-string Actions URL.
mod common;
use common::*;
use herdsman::events::{append, Event};
use herdsman::serve::Server;
use serde_json::json;

const GH: &str = "https://github.com/jirathip-dev/sendmeter";

fn get(s: &Server, path: &str) -> String {
    String::from_utf8_lossy(&s.respond(
        &format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n\r\n"),
        1_000_000,
    ))
    .into_owned()
}

fn section(page: &str, id: &str) -> String {
    let i = page.find(&format!("<section id={id}>")).unwrap();
    page[i..i + page[i..].find("</section>").unwrap()].to_string()
}

fn spawned(name: &str) -> herdsman::Ctx {
    let c = ctx(&tmpdir(name), CONFIG);
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "17"]).0,
        0
    );
    for (n, role, agent, branch) in [
        (12, "impl", "sendmeter-impl-12", "issue-12"),
        (12, "rev", "sendmeter-rev-12-r1", "rev-12-r1"),
        (17, "impl", "sendmeter-impl-17", "issue-17"),
    ] {
        let d = json!({"role": role, "agent": agent, "branch": branch});
        append(&c.state, &Event::new(500, "sendmeter", Some(n), "spawn", d)).unwrap();
    }
    let cache = c.state.parent().unwrap().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let pr = json!({"ts": 1, "branch": "issue-12", "pr": {"number": 33, "state": "OPEN"}});
    std::fs::write(cache.join("pr-sendmeter-12.json"), pr.to_string()).unwrap();
    c
}

#[test]
fn queue_row_links_the_issue_and_its_card_shows_three_links() {
    let c = spawned("links-queue");
    let r = Fake::default().reply(
        "herdr agent list",
        &agents(&[("sendmeter-impl-12", "idle", "/x", "w1:p1")]),
    );
    let page = get(&Server::new(&c, &r, Some("127.0.0.1:7777".into())), "/");
    let q = section(&page, "queue");
    assert!(
        q.contains(&format!("<summary><span><a href=\"{GH}/issues/12\">#12</a></span>")),
        "{q}"
    );
    let three = format!(
        "[<a href=\"{GH}/issues/12\">issue</a>] [<a href=\"{GH}/pull/33\">PR #33</a>] [<a href=\"{GH}/pull/33/checks\">checks</a>]"
    );
    assert!(q.contains(&three), "{q}");
    assert!(
        section(&page, "lanes").contains(&three),
        "Lanes cards share the builder: {page}"
    );
    assert!(!page.contains("actions?query"), "{page}");
    assert_eq!(r.count("gh"), 0, "no gh call on render: {:?}", r.calls.borrow());
}

#[test]
fn a_reviewer_event_links_the_prs_checks_not_its_own_branch() {
    let c = spawned("links-rev");
    let page = get(
        &Server::new(&c, &Fake::default(), Some("127.0.0.1:7777".into())),
        "/?all=1",
    );
    let ev = section(&page, "events");
    // Newest first: the reviewer's spawn (branch rev-12-r1) is the first `#12 spawn` row.
    let rev = ev.split("<p class=ev>").find(|l| l.contains("#12 spawn"));
    let rev = rev.unwrap_or_else(|| panic!("no reviewer event: {ev}"));
    assert!(
        rev.contains(&format!("[<a href=\"{GH}/pull/33/checks\">checks</a>]")),
        "{ev}"
    );
    assert!(
        !page.contains("rev-12-r1\">") && !page.contains("actions?query") && !page.contains(">runs<"),
        "{page}"
    );
}

#[test]
fn an_issue_with_no_pr_links_the_ticks_latest_run_or_nothing() {
    let c = spawned("links-nopr");
    let page = get(&Server::new(&c, &Fake::default(), Some("127.0.0.1:7777".into())), "/");
    let q = section(&page, "queue");
    assert!(
        q.contains(&format!("[<a href=\"{GH}/issues/17\">issue</a>] · no PR read yet")),
        "{q}"
    );
    // The tick (its `drive` pass, #155) reads the issue branch's latest run; a render only reads that cache.
    let url = format!("{GH}/actions/runs/99");
    let r = Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("gh pr view", "")
        .reply(
            "gh run list -R jirathip-dev/sendmeter --branch issue-17",
            &format!("[{{\"url\":\"{url}\"}}]"),
        );
    run(&c, &r, &["drive"]);
    assert!(
        r.called("gh run list -R jirathip-dev/sendmeter --branch issue-17 --limit 1 --json url"),
        "{:?}",
        r.calls.borrow()
    );
    assert!(
        !r.called("gh run list -R jirathip-dev/sendmeter --branch rev-"),
        "{:?}",
        r.calls.borrow()
    );
    let page = get(&Server::new(&c, &Fake::default(), Some("127.0.0.1:7777".into())), "/");
    let q = section(&page, "queue");
    assert!(
        q.contains(&format!(
            "[<a href=\"{GH}/issues/17\">issue</a>] [<a href=\"{url}\">checks</a>]"
        )),
        "{q}"
    );
    assert!(!page.contains("actions?query"), "{page}");
}
