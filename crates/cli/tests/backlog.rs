//! #365 step 1: the Queue tab's backlog. The tick's pass (and [Drive now]) reads a repo's open issues in one GraphQL
//! call, then only those updated since; the board renders it from that read with no GitHub call; [queue N] queues the
//! ticked issues in tick order, each row marked `needs-check`. The read's `--jq` runs under jq on a raw GraphQL page;
//! serve.js's pure backlog part (rows, search, quick filters, tick order) under node; the 390 px view with 100 issues
//! is gate 2's `queue/queue-backlog` (docs/ui.md).
mod common;
use common::*;
use drover::events::read;
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const NOW: u64 = 10_000_000;
const READ: &str = "gh api graphql --paginate --jq .data.repository.issues";

/// One issue as the read's `--jq` prints it.
fn issue(n: u64, state: &str, updated: &str, labels: &[&str], ac: bool, pr: bool) -> String {
    let i = json!({"number": n, "title": format!("issue {n} title"), "state": state, "created": updated,
        "updated": updated, "labels": labels, "ac": ac, "pr": pr});
    format!("{i}\n")
}

fn first_read() -> String {
    [
        issue(12, "OPEN", "1970-01-02T00:00:00Z", &["bug"], false, true),
        issue(15, "OPEN", "1970-04-20T00:00:00Z", &["ui", "phone"], true, false),
        issue(17, "OPEN", "1970-04-25T00:00:00Z", &[], true, false),
        issue(20, "OPEN", "1970-04-21T00:00:00Z", &[], false, false),
        issue(21, "OPEN", "1970-04-21T00:00:00Z", &[], false, false),
    ]
    .concat()
}

/// #268: a page as the board shows it, its Queue tab drawn by web/src/queue.ts.
fn get(s: &Server, path: &str, cookie: &str) -> String {
    let req = format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}\r\n");
    with_board(&String::from_utf8_lossy(&s.respond(&req, NOW)), s, ADDR, cookie, NOW)
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

fn cache(c: &drover::Ctx) -> Value {
    let p = c.state.with_file_name("cache").join("backlog-sendmeter-0.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn keys(v: &Value) -> Vec<String> {
    v.as_object().unwrap().keys().cloned().collect()
}

fn agents() -> Fake {
    Fake::default().reply("herdr agent list", r#"{"result":{"agents":[]}}"#)
}

/// The tick's pass reads every open issue once; a fast step never does. The next read asks only for issues updated
/// since the newest one seen, open or closed: a closed one leaves, a changed one is replaced, a new one joins. A failed
/// read keeps the last one and the pass goes on.
#[test]
fn the_tick_reads_all_open_issues_then_only_those_updated_since() {
    let c = ctx(&tmpdir("backlog-tick"), CONFIG);
    let r = agents().once(READ, 0, &first_read());
    run(&c, &r, &["drive", "--fast"]);
    assert_eq!(r.count(READ), 0, "a fast step reads no backlog");
    assert_eq!(run(&c, &r, &["drive"]).0, 0);
    let first: Vec<_> = r
        .calls
        .borrow()
        .iter()
        .filter(|l| l.starts_with(READ))
        .cloned()
        .collect();
    assert_eq!(first.len(), 1, "{:?}", r.calls.borrow());
    assert!(
        first[0].contains("issues(states:OPEN,") && !first[0].contains("since="),
        "{}",
        first[0]
    );
    let b = cache(&c);
    assert_eq!(b["since"], "1970-04-25T00:00:00Z");
    assert_eq!(keys(&b["issues"]), ["12", "15", "17", "20", "21"]);
    assert_eq!(b["issues"]["15"]["labels"], json!(["ui", "phone"]));

    let next = [
        issue(12, "CLOSED", "1970-04-26T00:00:00Z", &["bug"], false, false),
        issue(15, "OPEN", "1970-04-26T01:00:00Z", &["ui"], false, false),
        issue(30, "OPEN", "1970-04-26T02:00:00Z", &[], false, false),
    ];
    let r = agents().once(READ, 0, &next.concat());
    assert_eq!(run(&c, &r, &["drive"]).0, 0);
    let call = r.calls.borrow().iter().find(|l| l.starts_with(READ)).cloned().unwrap();
    assert!(
        call.contains("issues(states:[OPEN,CLOSED],") && call.ends_with("-f since=1970-04-25T00:00:00Z"),
        "{call}"
    );
    let b = cache(&c);
    assert_eq!(keys(&b["issues"]), ["15", "17", "20", "21", "30"]);
    assert_eq!(
        (b["issues"]["15"]["labels"].clone(), b["issues"]["15"]["ac"].clone()),
        (json!(["ui"]), json!(false))
    );
    assert_eq!(b["since"], "1970-04-26T02:00:00Z");

    let r = agents().once_err(READ, 1, "HTTP 502");
    assert_eq!(run(&c, &r, &["drive"]).0, 0, "a failed read never fails the pass");
    assert_eq!(r.count(READ), 1);
    assert_eq!(cache(&c)["issues"], b["issues"], "the last read kept");
}

/// The read's `--jq` (taken from the call the pass made) on a raw GraphQL page, under jq: each issue flattened, `ac`
/// when the body has acceptance criteria (the words or an `AC` heading), `pr` when a merged or closed PR mentions it.
#[test]
fn the_reads_jq_flattens_each_issue() {
    if std::process::Command::new("jq").arg("--version").output().is_err() {
        return eprintln!("skipping: no jq here");
    }
    let c = ctx(&tmpdir("backlog-jq"), CONFIG);
    let r = agents();
    run(&c, &r, &["drive"]);
    let call = r.calls.borrow().iter().find(|l| l.starts_with(READ)).cloned().unwrap();
    let jq = &call[call.find("--jq ").unwrap() + 5..call.find(" -f owner=").unwrap()];
    let node = |n: u64, body: &str, labels: &[&str], refs: Value| {
        let labels: Vec<_> = labels.iter().map(|l| json!({"name": l})).collect();
        json!({"number": n, "title": format!("t{n}"), "state": "OPEN", "body": body, "createdAt": "c", "updatedAt": "u",
            "labels": {"nodes": labels}, "timelineItems": {"nodes": refs}})
    };
    let page = json!({"data": {"repository": {"issues": {"nodes": [
        node(1, "Intro\n## AC\n- works", &["bug", "ui"], json!([{"source": {"state": "OPEN"}}, {"source": {"state": "MERGED"}}])),
        node(2, "Has Acceptance Criteria below", &[], json!([{"source": {"state": "CLOSED"}}])),
        node(3, "an ACME thing, nothing more", &[], json!([{"source": {}}, {}, {"source": {"state": "OPEN"}}])),
    ]}}}});
    let d = tmpdir("backlog-jq-page");
    std::fs::write(d.join("page.json"), page.to_string()).unwrap();
    let o = std::process::Command::new("jq")
        .args(["-c", jq])
        .arg(d.join("page.json"))
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let got: Vec<Value> = String::from_utf8_lossy(&o.stdout)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        got,
        [
            json!({"number": 1, "title": "t1", "state": "OPEN", "created": "c", "updated": "u", "labels": ["bug", "ui"], "ac": true, "pr": true}),
            json!({"number": 2, "title": "t2", "state": "OPEN", "created": "c", "updated": "u", "labels": [], "ac": true, "pr": true}),
            json!({"number": 3, "title": "t3", "state": "OPEN", "created": "c", "updated": "u", "labels": [], "ac": false, "pr": false}),
        ]
    );
}

/// The page and every live fragment carry the backlog from the last read alone (no `gh` call per view), leaving out
/// queued issues and those with a lane; before any read there is no backlog section.
#[test]
fn the_board_renders_the_backlog_from_the_last_read_with_no_github_call() {
    let c = ctx(&tmpdir("backlog-board"), CONFIG);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "20"]).0, 0);
    let ev = drover::events::Event::new(
        1,
        "sendmeter",
        Some(21),
        "spawn",
        json!({"agent": "sendmeter-impl-21", "role": "impl"}),
    );
    drover::events::append(&c.state, &ev).unwrap();
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "", NOW);
    assert!(
        !get(&s, "/", &cookie).contains("class=bl"),
        "never read: no backlog section"
    );
    assert_eq!(run(&c, &agents().once(READ, 0, &first_read()), &["drive"]).0, 0);
    // #268: the Queue tab (the backlog in it) is the TypeScript board's, from API v1's `queue`: paired or not.
    for out in [get(&s, "/", &cookie), get(&s, "/", "")] {
        let out = out.replace("\\\"", "\"");
        let at = out
            .find("<details class=bl data-bl=\"")
            .unwrap_or_else(|| panic!("{out}"))
            + 27;
        let bl = &out[at..at + out[at..].find('"').unwrap()];
        let bl: Value = serde_json::from_str(&bl.replace("&quot;", "\"").replace("&amp;", "&")).unwrap();
        assert_eq!(keys(&bl), ["12", "15", "17"], "#20 queued and #21 with a lane left out");
        assert_eq!(bl["12"], cache(&c)["issues"]["12"]);
        assert!(
            out.contains("class=bbar hidden><input type=hidden name=op value=bqueue>"),
            "{out}"
        );
    }
    assert_eq!(r.count("gh"), 0, "no GitHub call per view: {:?}", r.calls.borrow());
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
        res.contains("%2320%20%2317%20%2312%20%2315%20%28needs%2Dcheck%29"),
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
            row.contains(" data-ck") && row.contains("needs-check: from the backlog"),
            "#{n}: {row}"
        );
    }
    assert!(!row(20).contains("needs-check"), "queued by hand: no mark");
    let qs: Vec<_> = read(&c.state)
        .unwrap()
        .to_vec()
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

/// The server's read as rows: newest first, ages from `created`, `stale` past 90 days untouched. Then search (number,
/// title, label; every word, any case), each quick filter, a label, and ticks kept in tick order.
#[test]
fn rows_search_filters_and_ticks_in_serve_js() {
    let js = "(() => { const now = Date.parse('2026-06-01T00:00:00Z') / 1000;
      const is = {'9': {title: 'Old idea', labels: [], created: '2026-01-01T00:00:00Z', updated: '2026-02-01T00:00:00Z', ac: true},
        '10': {title: 'Phone layout', labels: ['ui', 'phone'], created: '2026-04-20T00:00:00Z', updated: '2026-05-30T00:00:00Z', pr: true},
        '12': {title: 'Fix the login page', labels: ['bug'], created: '2026-05-30T00:00:00Z', updated: '2026-05-30T00:00:00Z', ac: true}};
      const rows = brows(is, now), s = o => ({q: '', on: [], label: '', ...o}), pick = o => rows.filter(d => bpass(d, s(o))).map(d => d.n);
      let t = btick([], 3, true); t = btick(t, 1, true); t = btick(t, 2, true); t = btick(t, 1, false); t = btick(t, 1, true);
      return [rows.map(d => [d.n, Math.round(d.age / 86400), d.stale, d.ac, d.pr]), pick({}), pick({q: 'LOGIN'}), pick({q: '#10'}),
        pick({q: 'phone layout'}), pick({q: 'ui'}), pick({label: 'bug'}), pick({on: ['new']}), pick({on: ['30']}),
        pick({on: ['90']}), pick({on: ['ac']}), pick({on: ['pr']}), pick({on: ['ac', '30']}), pick({q: 'zzz'}), t]; })()";
    let Some(got) = node(js) else { return };
    assert_eq!(
        got,
        json!([
            [
                ["12", 2, false, true, false],
                ["10", 42, false, false, true],
                ["9", 151, true, true, false]
            ],
            ["12", "10", "9"],
            ["12"],
            ["10"],
            ["10"],
            ["10"],
            ["12"],
            ["12"],
            ["10", "9"],
            ["9"],
            ["12", "9"],
            ["10"],
            ["9"],
            [],
            [3, 2, 1]
        ])
    );
}

/// #391 in headless Chrome (`CHROME`, the ui job's chrome-headless-shell; skipped without it): the real server
/// (`serve_on`), a backlog and a 3-step chain. The backlog draws its count, rows, search and quick filters on a fresh
/// load, after a `/live` change (#268: each a `queue`, drawn by the TypeScript board), and on a page that carried the
/// Lanes tab only, with no uncaught error. A `build` of
/// another build reloads the page once; its own build none. The stream's first event names the server's build.
#[test]
fn the_backlog_draws_on_a_fresh_load_and_after_a_live_patch() {
    let Some(chrome) = std::env::var_os("CHROME") else {
        eprintln!("skipping: no CHROME here");
        return;
    };
    let c = ctx(&tmpdir("backlog-live"), CONFIG);
    let r = agents().reply("gh issue view", r#"{"title": "a title", "body": ""}"#);
    assert_eq!(run(&c, &r, &["queue", "sendmeter", "add", "30", "31", "32", "33"]).0, 0);
    for (n, on) in [(31, 30), (32, 31)] {
        let (n, on) = (n.to_string(), on.to_string());
        assert_eq!(run(&c, &r, &["queue", "sendmeter", "block", "--on", &on, &n]).0, 0);
    }
    assert_eq!(run(&c, &agents().once(READ, 0, &first_read()), &["drive"]).0, 0);
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let c2 = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c2, &agents(), l, false));
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    std::io::Write::write_all(&mut s, format!("GET /live HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes()).unwrap();
    let mut head = [0; 160];
    let n = std::io::Read::read(&mut s, &mut head).unwrap();
    let head = String::from_utf8_lossy(&head[..n]).into_owned();
    let page = String::from_utf8_lossy(
        &Server::new(&c, &r, Some(addr.to_string())).respond(&format!("GET / HTTP/1.1\r\nHost: {addr}\r\n\r\n"), NOW),
    )
    .into_owned();
    let ver = &page[page.find("serve.js").unwrap() - 9..page.find("serve.js").unwrap() - 1];
    assert!(
        head.contains(&format!(": ping\n\nevent: build\ndata: {ver}\n\n")),
        "{head}"
    );
    drop(s);
    let line = |n: u64, t: u64| {
        json!({"ts": t, "fleet": "sendmeter", "type": "queue", "op": "add", "issues": [n]}).to_string()
    };
    let now = drover::now();
    let o = std::process::Command::new("node")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ui/live.mjs"))
        .args([
            chrome.as_os_str(),
            format!("http://{addr}/").as_ref(),
            c.state.as_os_str(),
        ])
        .args([line(40, now), line(41, now + 1)])
        .output()
        .unwrap();
    let out: Value =
        serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&o.stderr)));
    let drawn = json!([{"count": "(5)", "rows": 5, "search": 1, "chips": 5, "flex": "flex"}]);
    assert_eq!(out["failed"], Value::Null, "{out}");
    assert_eq!(out["fresh"], drawn, "fresh load: {out}");
    assert_eq!(
        (&out["frag"], &out["patched"]),
        (&json!(true), &json!(true)),
        "a first `queue`, then a second: {out}"
    );
    assert_eq!(out["after_patch"], drawn, "after a /live change: {out}");
    assert_eq!(out["reloads"], json!([0, 1, 0]), "{out}");
    assert_eq!(
        (&out["served_queue"], &out["tab"]),
        (&json!(false), &json!("queue")),
        "{out}"
    );
    assert_eq!(out["other_tab"], drawn, "a page that carried another tab: {out}");
    assert_eq!(out["errors"], json!([]), "{out}");
}
