mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::json;

const GH: &str = "https://github.com/jirathip-dev/sendmeter";

fn row(s: &Server, n: u64) -> (serde_json::Value, serde_json::Value) {
    let q: serde_json::Value = serde_json::from_str(&api(s, "127.0.0.1:7777", "", 1_000_000, "queue")).unwrap();
    let r = q["fleets"][0]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["issue"] == n)
        .cloned()
        .unwrap();
    let res =
        String::from_utf8_lossy(&s.respond("GET /screens/queue HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n\r\n", 1_000_000))
            .into_owned();
    let screen: serde_json::Value = serde_json::from_str(res.split_once("\r\n\r\n").unwrap().1).unwrap();
    fn find(v: &serde_json::Value, id: &str) -> Option<serde_json::Value> {
        if v["id"] == id {
            return Some(v.clone());
        }
        match v {
            serde_json::Value::Object(m) => m.values().find_map(|x| find(x, id)),
            serde_json::Value::Array(a) => a.iter().find_map(|x| find(x, id)),
            _ => None,
        }
    }
    let block = find(&screen, &format!("q-sendmeter-{n}")).unwrap_or_else(|| panic!("no row {n}: {screen}"));
    (r, block)
}

fn spawned(name: &str) -> drover::Ctx {
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
    let sh = FakeShed::new();
    sh.attach(&c);
    sh.lane("sendmeter-impl-12", "idle", "/x");
    let f = Fake::default();
    let r = sh.on(&f);
    let s = Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    let (api_row, block) = row(&s, 12);
    assert_eq!(api_row["pr"]["pr"], 33, "{api_row}");
    assert_eq!(block["tap"], json!({"url": format!("{GH}/pull/33")}), "{block}");
    let lanes = api(&s, "127.0.0.1:7777", "", 1_000_000, "lanes");
    assert!(
        lanes.contains("\"pr\":{\"pr\":33,"),
        "Lanes rows carry the same PR: {lanes}"
    );
    assert_eq!(f.count("gh"), 0, "no gh call on render: {:?}", f.calls.borrow());
}

#[test]
fn an_issue_with_no_pr_links_the_ticks_latest_run_or_nothing() {
    let c = spawned("links-nopr");
    let f = Fake::default();
    let (api_row, block) = row(&Server::new(&c, &f, Some("127.0.0.1:7777".into())), 17);
    assert_eq!(
        (&api_row["pr"]["pr"], &api_row["pr"]["run"]),
        (&json!(null), &json!(null)),
        "no PR read yet"
    );
    assert!(block.get("tap").is_none(), "{block}");
    let url = format!("{GH}/actions/runs/99");
    let _sh = FakeShed::new();
    _sh.attach(&c);
    let r = Fake::default().reply("gh pr view", "").reply(
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
    let (api_row, block) = row(&Server::new(&c, &f, Some("127.0.0.1:7777".into())), 17);
    assert_eq!(api_row["pr"]["run"], url, "{api_row}");
    assert!(!block.to_string().contains("actions?query"), "{block}");
}
