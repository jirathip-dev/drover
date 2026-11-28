mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const HEAD: &str = "1111111111111111111111111111111111111111";
const TOKEN: &str = "tok-epics";

fn green() -> Value {
    json!([{"__typename": "CheckRun", "name": "build", "status": "COMPLETED", "conclusion": "SUCCESS"}])
}

fn spec(title: &str, n: u64) -> String {
    let url = format!("https://github.com/jirathip-dev/sendmeter/issues/{n}");
    json!({"title": title, "body": "", "updatedAt": "t", "comments": [], "url": url}).to_string()
}

fn fake() -> Fake {
    let mut f = Fake::default();
    for (n, pr) in [(11, 31), (12, 32), (13, 33)] {
        let v = json!({"number": pr, "state": "OPEN", "baseRefName": "staging", "headRefOid": HEAD, "statusCheckRollup": green()});
        f = f.reply(&format!("gh pr view issue-{n}"), &v.to_string());
    }
    let closed = json!({"data": {"repository": {"i13": {"__typename": "Issue", "state": "CLOSED"}}}});
    f.reply("gh pr merge", "")
        .reply("gh pr view 3", &json!({"mergeCommit": {"oid": "m3rg3"}}).to_string())
        .reply("gh issue view 24 ", &spec("Payments API v2", 24))
        .reply("gh issue view 11 ", &spec("Refunds endpoint", 11))
        .reply("gh issue view 12 ", &spec("Currency field", 12))
        .reply("gh issue view 13 ", &spec("Cursor pagination", 13))
        .reply("gh issue view 30 ", &spec("Search", 30))
        .reply(
            "gh issue view 40 ",
            &spec("A pull request", 40).replace("/issues/", "/pull/"),
        )
        .reply("gh api graphql", &closed.to_string())
        .reply("gh issue comment", "")
        .reply("gh issue close", "")
}

fn board(name: &str) -> drover::Ctx {
    let c = ctx(&tmpdir(name), CONFIG);
    std::fs::write(c.config.with_file_name("cooee-token"), TOKEN).unwrap();
    c
}

fn ready(c: &drover::Ctx, n: u64) {
    let spawn = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "branch": format!("issue-{n}")});
    append(&c.state, &Event::new(10, "sendmeter", Some(n), "spawn", spawn)).unwrap();
    let v = json!({"verdict": "PASS", "sha": HEAD});
    append(&c.state, &Event::new(20, "sendmeter", Some(n), "verdict", v)).unwrap();
}

fn merge(c: &drover::Ctx, r: &Fake, n: u64) -> String {
    ready(c, n);
    let (exit, out) = run(c, r, &["merge", "sendmeter", &n.to_string()]);
    assert_eq!(exit, 0, "{out}");
    out
}

fn get(c: &drover::Ctx, r: &Fake, path: &str) -> Value {
    let s = Server::new(c, r, Some("127.0.0.1:7777".into()));
    let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:7777\r\nAuthorization: Bearer {TOKEN}\r\n\r\n");
    let res = String::from_utf8_lossy(&s.respond(&req, c.now)).into_owned();
    serde_json::from_str(res.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn find<'a>(v: &'a Value, id: &str) -> Option<&'a Value> {
    if v.get("id").and_then(Value::as_str) == Some(id) {
        return Some(v);
    }
    match v {
        Value::Object(m) => m.values().find_map(|x| find(x, id)),
        Value::Array(a) => a.iter().find_map(|x| find(x, id)),
        _ => None,
    }
}

fn landed(c: &drover::Ctx) -> usize {
    read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == "epic-landed")
        .count()
}

fn epic_rows(c: &drover::Ctx, r: &Fake) -> Vec<Value> {
    let rows = get(c, r, "/api/v1/rows?since=0")["rows"].as_array().unwrap().clone();
    rows.into_iter().filter(|r| r["key"] == "epic-sendmeter-24").collect()
}

#[test]
fn an_epic_of_three_shows_two_of_three_and_the_third_merge_closes_the_parent_once() {
    let c = board("epic-three");
    let r = fake();
    let (exit, out) = run(&c, &r, &["epic", "sendmeter", "24", "add", "11", "12"]);
    assert_eq!(exit, 0, "{out}");
    let (exit, out) = run(&c, &r, &["queue", "sendmeter", "add", "13", "--epic", "24"]);
    assert_eq!(exit, 0, "{out}");
    assert!(
        out.contains("sendmeter #24 Payments API v2: 3 children (#11 #12 #13)"),
        "{out}"
    );
    merge(&c, &r, 11);
    merge(&c, &r, 12);
    let (_, w) = run(&c, &r, &["waiting", "sendmeter"]);
    assert!(
        w.contains("epic sendmeter #24 Payments API v2 · 2/3 merged · 1 queued · open #13"),
        "{w}"
    );
    let q = get(&c, &r, "/screens/queue");
    let head = find(&q, "epic-sendmeter-24").unwrap_or_else(|| panic!("no epic head in {q}"));
    assert_eq!(head["badge"]["text"], "2/3 merged");
    assert_eq!(find(&q, "ep-sendmeter-11").unwrap()["subtitle"], "merged · PR #31");
    assert_eq!(find(&q, "epics-sendmeter").unwrap()["header"], "epics (1)");
    let cards = get(&c, &r, "/api/v1/queue");
    assert_eq!(cards["fleets"][0]["epics"][0]["children"][0]["is"], "merged", "{cards}");
    assert_eq!(landed(&c), 0);
    assert!(!r.called("gh issue comment 24"));

    let out = merge(&c, &r, 13);
    assert!(out.contains("sendmeter #24 landed: 3 issues"), "{out}");
    assert_eq!(r.count("gh issue comment 24"), 1);
    assert_eq!(r.count("gh issue close 24"), 1);
    let calls = r.calls.borrow().clone();
    let body = calls.iter().find(|l| l.starts_with("gh issue comment 24")).unwrap();
    assert!(
        body.ends_with(
            "Landed: 3 of 3 issues merged.\n- #11 Refunds endpoint: PR #31\n- #12 Currency field: PR #32\n- #13 Cursor pagination: PR #33"
        ),
        "{body}"
    );
    assert_eq!(landed(&c), 1);
    let rows = epic_rows(&c, &r);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (&rows[0]["kind"], &rows[0]["issue"], &rows[0]["epic"]["merged"][2]["pr"]),
        (&json!("merged"), &json!(24), &json!(33))
    );
    assert_eq!(rows[0]["text"], "sendmeter #24 Payments API v2 landed: 3 issues");
    let st = fold(&read(&c.state).unwrap());
    assert_eq!(st[&("sendmeter".to_string(), 24)].state, "merged");
    assert!(find(&get(&c, &r, "/screens/queue"), "epic-sendmeter-24").is_none());

    run(&c, &r, &["drive", "sendmeter"]);
    assert_eq!(
        (landed(&c), r.count("gh issue comment 24"), r.count("gh issue close 24")),
        (1, 1, 1)
    );
    assert_eq!(epic_rows(&c, &r).len(), 1);
}

#[test]
fn a_child_closed_unmerged_is_dropped_and_does_not_block_the_close() {
    let c = board("epic-dropped");
    let r = fake();
    let (exit, out) = run(&c, &r, &["epic", "sendmeter", "24", "add", "11", "12", "13"]);
    assert_eq!(exit, 0, "{out}");
    merge(&c, &r, 11);
    let out = merge(&c, &r, 12);
    assert!(!out.contains("landed"), "#13 is still open: {out}");
    let (_, o) = run(&c, &r, &["drive", "sendmeter"]);
    let st = fold(&read(&c.state).unwrap());
    assert_eq!(st[&("sendmeter".to_string(), 13)].state, "closed", "{o}");
    assert_eq!(landed(&c), 1, "{o}");
    let calls = r.calls.borrow().clone();
    let body = calls.iter().find(|l| l.starts_with("gh issue comment 24")).unwrap();
    assert!(body.contains("Landed: 2 of 2 issues merged."), "{body}");
    assert!(body.ends_with("Dropped (closed unmerged): #13"), "{body}");
    assert!(!body.contains("#13 Cursor"), "{body}");
    assert_eq!(r.count("gh issue close 24"), 1);
    let rows = epic_rows(&c, &r);
    assert_eq!(rows[0]["epic"]["dropped"], json!([13]));
    assert_eq!(
        rows[0]["text"],
        "sendmeter #24 Payments API v2 landed: 2 issues · 1 dropped"
    );
}

#[test]
fn a_dropped_child_shows_as_dropped_and_leaves_the_count() {
    let c = board("epic-dropped-view");
    let r = fake();
    run(&c, &r, &["epic", "sendmeter", "24", "add", "11", "12", "13"]);
    append(
        &c.state,
        &Event::new(30, "sendmeter", Some(13), "closed-other", json!({})),
    )
    .unwrap();
    merge(&c, &r, 11);
    let events = read(&c.state).unwrap();
    let e = &drover::epic::open(&c, &events, &fold(&events), "sendmeter")[0];
    assert_eq!((e.merged(), e.total(), e.dropped(), e.lands()), (1, 2, 1, false));
    assert_eq!(
        e.line(),
        "epic sendmeter #24 Payments API v2 · 1/2 merged · 1 not queued · 1 dropped · open #12 (#13 dropped)"
    );
    let q = get(&c, &r, "/screens/queue");
    let x = find(&q, "ep-sendmeter-13").unwrap();
    assert_eq!(
        (&x["badge"]["text"], &x["subtitle"]),
        (&json!("dropped"), &json!("closed unmerged · not counted"))
    );
}

#[test]
fn epic_add_refuses_a_bad_parent_a_taken_child_and_an_epic_with_nothing_open() {
    let c = board("epic-refusals");
    let r = fake();
    assert_eq!(run(&c, &r, &["epic", "sendmeter", "24", "add", "11"]).0, 0);
    let before = read(&c.state).unwrap().len();
    let (exit, out) = run(&c, &r, &["epic", "sendmeter", "24", "add", "11"]);
    assert_eq!(exit, 0, "{out}");
    assert_eq!(read(&c.state).unwrap().len(), before, "a repeated add logs nothing");
    let (exit, out) = run(&c, &r, &["epic", "sendmeter", "30", "add", "11"]);
    assert!(
        exit == 3 && out.contains("REFUSED EPIC_CHILD") && out.contains("drover epic sendmeter 24 rm 11"),
        "{out}"
    );
    let (exit, out) = run(&c, &r, &["epic", "sendmeter", "30", "add", "24"]);
    assert!(exit == 3 && out.contains("REFUSED EPIC_PARENT"), "{out}");
    let (exit, out) = run(&c, &r, &["queue", "sendmeter", "add", "24"]);
    assert!(exit == 3 && out.contains("REFUSED EPIC_PARENT"), "{out}");
    let (exit, out) = run(&c, &r, &["spawn", "sendmeter", "24", "impl"]);
    assert!(exit == 3 && out.contains("REFUSED EPIC_PARENT"), "{out}");
    let (exit, out) = run(&c, &r, &["epic", "sendmeter", "30", "add", "40"]);
    assert!(exit == 3 && out.contains("REFUSED IS_PR"), "{out}");
    merge(&c, &r, 12);
    let (exit, out) = run(&c, &r, &["epic", "sendmeter", "30", "add", "12"]);
    assert!(exit == 3 && out.contains("REFUSED EPIC_DONE"), "{out}");
    let epics = read(&c.state).unwrap().iter().filter(|e| e.kind == "epic").count();
    assert_eq!(epics, 1, "only the first add is logged");
}
