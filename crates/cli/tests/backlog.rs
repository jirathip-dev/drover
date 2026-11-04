//! #365 step 1: the Queue tab's backlog. The tick's pass (and [↻ read again]) reads a repo's open issues in one GraphQL
//! call, then only those updated since; the board renders it from that read with no GitHub call; [queue N] queues the
//! ticked issues in tick order, each row marked `needs-check`. serve.js's pure backlog part (search, quick filters,
//! tick order) runs under node; the 390 px view with 100 issues is gate 2's `queue/queue-backlog` (docs/ui.md).
mod common;
use common::*;
use herdsman::events::read;
use herdsman::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
/// 1970-04-26T17:46:40Z: an issue last touched on 1970-01-02 is 104 days stale.
const NOW: u64 = 10_000_000;
const READ: &str = "gh api graphql --paginate --jq .data.repository.issues";

fn issue(n: u64, state: &str, updated: &str, labels: &[&str], body: &str, prs: &[&str]) -> String {
    let i = json!({"number": n, "title": format!("issue {n} title"), "state": state, "body": body,
        "createdAt": updated, "updatedAt": updated, "labels": labels, "prs": prs});
    format!("{i}\n")
}

fn first_read() -> String {
    [
        issue(12, "OPEN", "1970-01-02T00:00:00Z", &["bug"], "", &["MERGED"]),
        issue(
            15,
            "OPEN",
            "1970-04-20T00:00:00Z",
            &["ui", "phone"],
            "## AC\n- works",
            &["OPEN"],
        ),
        issue(17, "OPEN", "1970-04-25T00:00:00Z", &[], "Acceptance criteria: x", &[]),
        issue(20, "OPEN", "1970-04-21T00:00:00Z", &[], "", &[]),
        issue(21, "OPEN", "1970-04-21T00:00:00Z", &[], "", &[]),
    ]
    .concat()
}

fn get(s: &Server, path: &str, cookie: &str) -> String {
    let req = format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}\r\n");
    String::from_utf8_lossy(&s.respond(&req, NOW)).into_owned()
}

fn post(s: &Server, cookie: &str, body: &str) -> String {
    let req = format!(
        "POST / HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&req, NOW)).into_owned()
}

fn mtime(page: &str) -> String {
    let i = page.find("name=mtime value=").unwrap() + 17;
    page[i..].chars().take_while(char::is_ascii_digit).collect()
}

fn cache(c: &herdsman::Ctx) -> Value {
    let p = c.state.with_file_name("cache").join("backlog-sendmeter-0.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// The tick's pass reads every open issue once; a fast step never does. The next read asks only for issues updated
/// since the newest one seen, open or closed: a closed one leaves, a changed one is replaced, a new one joins.
#[test]
fn the_tick_reads_all_open_issues_then_only_those_updated_since() {
    let c = ctx(&tmpdir("backlog-tick"), CONFIG);
    let r = Fake::default()
        .reply("herdr agent list", r#"{"result":{"agents":[]}}"#)
        .once(READ, 0, &first_read());
    run(&c, &r, &["drive", "--fast"]);
    assert_eq!(r.count(READ), 0, "a fast step reads no backlog");
    run(&c, &r, &["drive"]);
    let calls = r.calls.borrow().clone();
    let first: Vec<_> = calls.iter().filter(|l| l.starts_with(READ)).collect();
    assert_eq!(first.len(), 1, "{calls:?}");
    assert!(
        first[0].contains("issues(states:OPEN,") && !first[0].contains("since="),
        "{}",
        first[0]
    );
    let b = cache(&c);
    assert_eq!(b["since"], "1970-04-25T00:00:00Z");
    let ns: Vec<&String> = b["issues"].as_object().unwrap().keys().collect();
    assert_eq!(ns, ["12", "15", "17", "20", "21"]);
    assert_eq!(b["issues"]["12"]["pr"], true, "mentioned in a merged PR");
    assert_eq!(b["issues"]["15"]["pr"], false, "an open PR says nothing yet");
    assert_eq!(
        (b["issues"]["15"]["ac"].clone(), b["issues"]["17"]["ac"].clone()),
        (json!(true), json!(true))
    );
    assert_eq!(b["issues"]["20"]["ac"], false);
    assert_eq!(b["issues"]["12"]["updated"], 86_400, "epoch seconds");

    let r = Fake::default().once(
        READ,
        0,
        &[
            issue(12, "CLOSED", "1970-04-26T00:00:00Z", &["bug"], "", &[]),
            issue(15, "OPEN", "1970-04-26T01:00:00Z", &["ui"], "", &[]),
            issue(30, "OPEN", "1970-04-26T02:00:00Z", &[], "", &[]),
        ]
        .concat(),
    );
    assert_eq!(
        herdsman::gh::backlog(
            &c,
            &r,
            &herdsman::config::Config::load(&c.config).unwrap().fleet["sendmeter"],
            "sendmeter"
        )
        .unwrap(),
        5
    );
    let call = r.calls.borrow()[0].clone();
    assert!(
        call.contains("issues(states:[OPEN,CLOSED],") && call.ends_with("-f since=1970-04-25T00:00:00Z"),
        "{call}"
    );
    let b = cache(&c);
    let ns: Vec<&String> = b["issues"].as_object().unwrap().keys().collect();
    assert_eq!(ns, ["15", "17", "20", "21", "30"]);
    assert_eq!(b["issues"]["15"]["labels"], json!(["ui"]));
    assert_eq!(b["issues"]["15"]["ac"], false, "replaced by the newer read");
    assert_eq!(b["since"], "1970-04-26T02:00:00Z");

    // A failed read keeps the last one.
    assert!(herdsman::gh::backlog(
        &c,
        &Fake::default().once_err(READ, 1, "HTTP 502"),
        &herdsman::config::Config::load(&c.config).unwrap().fleet["sendmeter"],
        "sendmeter"
    )
    .is_err());
    assert_eq!(cache(&c)["since"], "1970-04-26T02:00:00Z");
}

/// [↻ read again] reads on demand; the page and every live fragment render from that read alone (no `gh` call), newest
/// first, leaving out queued issues and those with a lane, a `stale` tag on one untouched for 90 days.
#[test]
fn the_board_renders_the_backlog_from_the_last_read_with_no_github_call() {
    let c = ctx(&tmpdir("backlog-board"), CONFIG);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "20"]).0, 0);
    let ev = herdsman::events::Event::new(
        1,
        "sendmeter",
        Some(21),
        "spawn",
        json!({"agent": "sendmeter-impl-21", "role": "impl"}),
    );
    herdsman::events::append(&c.state, &ev).unwrap();
    let r = Fake::default().once(READ, 0, &first_read());
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "", NOW);
    let page = get(&s, "/", &cookie);
    assert!(!page.contains("class=bl>"), "never read: no backlog section");
    let res = post(
        &s,
        &cookie,
        &format!("op=backlog&fleet=sendmeter&mtime={}", mtime(&page)),
    );
    assert!(res.contains("msg=sendmeter%20backlog%3A%205%20open%20issues"), "{res}");
    assert_eq!(r.count("gh"), 1);

    for out in [
        get(&s, "/", &cookie),
        get(&s, "/?frag=all", &cookie),
        get(&s, "/frags?tab=queue", &cookie),
    ] {
        let out = out.replace("\\\"", "\"");
        assert!(out.contains("<span>backlog</span><span class=m>(3)</span>"), "{out}");
        let rows: Vec<&str> = out.split("<label class=bi data-n=").skip(1).collect();
        let ns: Vec<&str> = rows.iter().map(|r| &r[..2]).collect();
        assert_eq!(
            ns,
            ["17", "15", "12"],
            "newest first; #20 queued and #21 with a lane left out"
        );
        assert!(
            rows[2].contains(" data-pr>") && rows[2].contains("bug <span class=\"tag m\">stale</span>"),
            "{}",
            rows[2]
        );
        assert!(
            rows[1].contains(" data-ac>") && !rows[1].contains("stale"),
            "{}",
            rows[1]
        );
        assert!(
            rows[1].contains("data-l=\"[&quot;ui&quot;,&quot;phone&quot;]\""),
            "{}",
            rows[1]
        );
        assert!(out.contains("<option>bug</option><option>phone</option><option>ui</option>"));
        assert!(out.contains("class=bbar hidden><input type=hidden name=op value=bqueue>"));
    }
    assert_eq!(r.count("gh"), 1, "no GitHub call per view: {:?}", r.calls.borrow());
}

/// [queue N] queues the ticked issues in the order they were ticked (serve.js sends `issues` so), behind the queue;
/// each row says `needs-check`, which a later move keeps. Unpaired, it is refused.
#[test]
fn queue_n_follows_tick_order_and_marks_needs_check() {
    let c = ctx(&tmpdir("backlog-queue"), CONFIG);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "20"]).0, 0);
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let mt = mtime(&get(&s, "/", ""));
    let body = format!("op=bqueue&fleet=sendmeter&mtime={mt}&issues=17%2C12%2C15");
    assert!(post(&s, "", &body).starts_with("HTTP/1.1 401"), "unpaired cannot queue");
    let cookie = pair(&s, &c, "", NOW);
    let res = post(&s, &cookie, &body);
    assert!(
        res.contains("msg=queued%20%2317%20%2312%20%2315%20%28needs%2Dcheck%29"),
        "{res}"
    );
    let (_, out) = run(&c, &r, &["queue", "sendmeter", "ls", "--json"]);
    let ls: Value = serde_json::from_str(&out).unwrap();
    let order: Vec<u64> = ls
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["issue"].as_u64().unwrap())
        .collect();
    assert_eq!(order, [20, 17, 12, 15]);
    assert_eq!(run(&c, &r, &["queue", "sendmeter", "mv", "--top", "15"]).0, 0);
    let page = get(&s, "/", &cookie);
    let row = |n: u64| {
        let at = page.find(&format!("/issues/{n}\">#{n}</a>")).unwrap();
        let row = &page[page[..at].rfind("<details").unwrap()..];
        row[..row.find("</details>").unwrap()].to_string()
    };
    for n in [17, 12, 15] {
        let row = row(n);
        assert!(
            row.contains(" data-ck") && row.contains("needs-check: queued from the backlog"),
            "#{n}: {row}"
        );
    }
    assert!(!row(20).contains("needs-check"), "queued by hand: no mark");
    let qs: Vec<_> = read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "queue")
        .collect();
    assert_eq!(
        (qs[1].data["check"].clone(), qs[1].data["via"].clone()),
        (json!(true), json!("web"))
    );
}

/// serve.js's pure backlog part plus `js`, under node: its stdout as JSON. None where node is missing.
fn node(js: &str) -> Option<Value> {
    if std::process::Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping: no node here");
        return None;
    }
    let src = include_str!("../../../assets/serve.js");
    let part = &src[src.find("// #365 backlog: pure").unwrap()..src.find("// backlog:end").unwrap()];
    let d = tmpdir("backlog-js");
    std::fs::write(d.join("t.js"), format!("{part}\nconsole.log(JSON.stringify({js}))")).unwrap();
    let o = std::process::Command::new("node").arg(d.join("t.js")).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    Some(serde_json::from_slice(&o.stdout).unwrap())
}

#[test]
fn search_filters_and_ticks_in_serve_js() {
    let js = "(() => { const day = 86400, rows = [
        {n: '1', title: 'Fix the login page', labels: ['bug'], age: 2 * day, ac: true, pr: false},
        {n: '2', title: 'Phone layout', labels: ['ui', 'phone'], age: 40 * day, ac: false, pr: true},
        {n: '3', title: 'Old idea', labels: [], age: 120 * day, ac: true, pr: false}];
      const s = (o) => ({q: '', on: [], label: '', ...o}), pick = o => rows.filter(d => bpass(d, s(o))).map(d => d.n);
      let t = btick([], 3, true); t = btick(t, 1, true); t = btick(t, 2, true); t = btick(t, 1, false); t = btick(t, 1, true);
      return [pick({}), pick({q: 'LOGIN'}), pick({q: '#2'}), pick({q: 'phone layout'}), pick({q: 'ui'}), pick({label: 'bug'}),
        pick({on: ['new']}), pick({on: ['30']}), pick({on: ['90']}), pick({on: ['ac']}), pick({on: ['pr']}),
        pick({on: ['ac', '30']}), pick({q: 'zzz'}), t]; })()";
    let Some(got) = node(js) else { return };
    assert_eq!(
        got,
        json!([
            ["1", "2", "3"],
            ["1"],
            ["2"],
            ["2"],
            ["2"],
            ["1"],
            ["1"],
            ["2", "3"],
            ["3"],
            ["1", "3"],
            ["2"],
            ["3"],
            [],
            [3, 2, 1]
        ])
    );
}
