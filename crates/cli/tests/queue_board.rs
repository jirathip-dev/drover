//! #268: the Queue tab is the TypeScript board's (web/src/queue.ts, run under node), drawn from API v1's `queue`: the
//! page carries none of it, and every form the board draws posts through API v1 with exactly its fields: [▼], [Drive
//! now], [Pause], [Resume], a defaulted decision's [change], the backlog's [queue N], a role's [Set] and a Conductor
//! choice from the [config] sheet.
mod common;
use common::*;
use herdsman::events::{append, read, Event};
use herdsman::serve::Server;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

/// A drawn form: its hidden fields, its buttons' `name=value`.
type Form = (BTreeMap<String, String>, Vec<(String, String)>);

/// Each live form in `html` (`<form method=post …>`): its hidden fields, then each button's `name=value` (if any).
fn forms(html: &str) -> Vec<Form> {
    let attr = |tag: &str, k: &str| {
        let at = tag.find(&format!(" {k}="))? + k.len() + 2;
        let v = &tag[at..];
        Some(match v.strip_prefix('"') {
            Some(q) => q[..q.find('"')?].replace("&quot;", "\"").replace("&amp;", "&"),
            None => v[..v.find([' ', '>']).unwrap_or(v.len())].to_string(),
        })
    };
    html.split("<form method=post")
        .skip(1)
        .map(|f| {
            let f = &f[..f.find("</form>").unwrap()];
            let tags = |t: &str| {
                f.split(&format!("<{t} "))
                    .skip(1)
                    .map(|x| format!(" {}", &x[..x.find('>').unwrap() + 1]))
                    .collect::<Vec<_>>()
            };
            let hidden = tags("input")
                .iter()
                .filter(|i| i.contains("type=hidden"))
                .map(|i| (attr(i, "name").unwrap(), attr(i, "value").unwrap_or_default()))
                .collect();
            let buttons = tags("button")
                .iter()
                .filter_map(|b| Some((attr(b, "name")?, attr(b, "value")?)))
                .collect();
            (hidden, buttons)
        })
        .collect()
}

/// POST `fields` to `/api/v1/<op>` as a paired device: the reply's JSON.
fn post(s: &Server, cookie: &str, fields: &BTreeMap<String, String>) -> Value {
    let body = fields
        .iter()
        .filter(|(k, _)| *k != "op")
        .map(|(k, v)| format!("{k}={}", v.replace(' ', "+")))
        .collect::<Vec<_>>()
        .join("&");
    let req = format!(
        "POST /api/v1/{} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        fields["op"],
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&req, T)).into_owned();
    serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap_or_else(|_| panic!("{res}"))
}

/// The drawn form whose hidden `op` is `op` (and, given, whose fields include `k=v`).
fn form(html: &str, op: &str, with: Option<(&str, &str)>) -> Form {
    forms(html)
        .into_iter()
        .find(|(h, b)| {
            let mut all = h.clone();
            all.extend(b.iter().cloned());
            h.get("op").map(String::as_str) == Some(op) && with.is_none_or(|(k, v)| all.get(k).is_some_and(|x| x == v))
        })
        .unwrap_or_else(|| panic!("no {op} form {with:?}: {html}"))
}

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

#[test]
fn the_page_has_no_queue_html_and_every_drawn_form_posts_through_api_v1() {
    let c = ctx(
        &tmpdir("queue-board"),
        &format!("models = [\"opus\", \"sonnet\"]\n\n{CONFIG}").replace(
            "profile = \"fleet-rev\"\n",
            "profile = \"fleet-rev\"\nmodel = \"sonnet\"\n",
        ),
    );
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12", "17", "18"]).0,
        0
    );
    // #12: a routine ask decided by default (its [change]); a backlog read with #30 and #31 in it.
    let ev =
        |n: u64, kind: &str, d: Value| append(&c.state, &Event::new(T - 60, "sendmeter", Some(n), kind, d)).unwrap();
    ev(
        12,
        "needs-decision",
        json!({"id": "q1", "question": "split?", "options": ["yes", "no"], "kind": "routine", "recommend": 2}),
    );
    ev(12, "decision", json!({"id": "q1", "choice": "2", "by": "default"}));
    let issue = |n: u64| {
        json!({"number": n, "title": format!("t{n}"), "state": "OPEN", "created": "2026-01-01T00:00:00Z",
            "updated": "2026-01-02T00:00:00Z", "labels": [], "ac": true, "pr": false})
    };
    let cache = herdsman::gh::cache_path(&c, "backlog", "sendmeter", 0);
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(
        &cache,
        json!({"ts": T - 60, "issues": {"30": issue(30), "31": issue(31)}}).to_string(),
    )
    .unwrap();
    let r = Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("herdr agent prompt", "")
        .reply("which hermes", "/usr/local/bin/hermes\n");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let page = String::from_utf8_lossy(&s.respond(&format!("GET / HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), T)).into_owned();
    assert!(
        page.contains("<section id=queue></section>") && !page.contains("class=\"fc "),
        "the page has no Queue HTML"
    );
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let tab = || queue_tab(&s, ADDR, T);
    let ok = |f: &BTreeMap<String, String>| {
        let v = post(&s, &cookie, f);
        assert_eq!(v["ok"], true, "{f:?}: {v}");
        v["message"].as_str().unwrap().to_string()
    };

    // [▼] on #12: the queue order moves as `queue mv` would.
    let (mv, _) = form(&tab(), "mv", Some(("issue", "12")));
    assert_eq!(mv.get("after").map(String::as_str), Some("17"), "{mv:?}");
    ok(&mv);
    let order: Vec<u64> = herdsman::queue::view(
        &read(&c.state).unwrap(),
        &herdsman::events::fold(&read(&c.state).unwrap()),
        "sendmeter",
    )
    .iter()
    .map(|i| i.issue)
    .collect();
    assert_eq!(order, [17, 12, 18]);

    // [change] on the defaulted decision: a new decision, the owner's.
    let (mut change, buttons) = form(&tab(), "answer", Some(("id", "q1")));
    assert_eq!(
        buttons[0],
        ("choice".into(), "2".into()),
        "recommended first: {buttons:?}"
    );
    change.insert("choice".into(), buttons[1].1.clone());
    ok(&change);
    let d = kinds(&c, "decision");
    assert_eq!(
        (d.len(), d[1].str("choice"), d[1].str("by")),
        (2, Some("yes"), Some("owner")),
        "{d:?}"
    );

    // The backlog's [queue N] (serve.js fills `issues` with the ticks, in tick order).
    let (mut bq, _) = form(&tab(), "bqueue", None);
    bq.insert("issues".into(), "31,30".into());
    assert!(ok(&bq).ends_with("(needs-check)"));
    let q = kinds(&c, "queue");
    assert_eq!(
        (
            q.last().unwrap().data["issues"].clone(),
            q.last().unwrap().data["check"].clone()
        ),
        (json!([31, 30]), json!(true))
    );

    // [Drive now]: a pass runs.
    let before = kinds(&c, "drive-pass").len();
    ok(&form(&tab(), "drive", None).0);
    assert_eq!(kinds(&c, "drive-pass").len(), before + 1);

    // [Pause], then the paused card's [Resume].
    ok(&form(&tab(), "pause", None).0);
    let paused = tab();
    assert!(paused.contains("<details class=\"fc off "), "{paused}");
    ok(&form(&paused, "resume", None).0);
    assert!(!tab().contains("<details class=\"fc off "));

    let sheet = queue_js(&s, ADDR, T, "sheet(d, 'sendmeter')");
    let (mut set, _) = form(&sheet, "set", Some(("role", "impl")));
    set.extend([("harness", "hermes"), ("model", "sonnet"), ("effort", "")].map(|(k, v)| (k.into(), v.into())));
    assert!(ok(&set).contains("model sonnet"));
    let sheet = queue_js(&s, ADDR, T, "sheet(d, 'sendmeter')");
    assert!(
        forms(&sheet)
            .iter()
            .all(|(h, _)| h.get("op").map(String::as_str) != Some("cond")),
        "{sheet}"
    );
    // Unpaired, a paired op through the API is refused; the board's mtime is checked as the form's.
    let v = post(&s, "", &form(&tab(), "drive", None).0);
    assert_eq!(v["code"], "UNPAIRED", "{v}");
    let mut stale = form(&tab(), "pause", None).0;
    stale.insert("mtime".into(), "1".into());
    assert_eq!(post(&s, &cookie, &stale)["code"], "STALE");
}

/// #517: a conductor's thread ask on an issue: its question and options on that issue's queue card (and the needs
/// card), `asked in <title> · <id>`; answered there, the thread's copy shows it answered, no longer open.
#[test]
fn a_thread_ask_on_an_issue_is_answerable_from_its_queue_card() {
    let c = ctx(&tmpdir("queue-thread-ask"), CONFIG);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "2"]).0, 0);
    let ev = |n: Option<u64>, kind: &str, d: Value| {
        append(&c.state, &Event::new(T - 60, "sendmeter", n, kind, d)).unwrap();
    };
    ev(None, "thread-new", json!({"thread": "t6", "title": "the bell"}));
    let ask = json!({"id": "q398", "question": "ship the bell now?", "options": ["now", "after #10"],
        "recommend": 1, "kind": "owner", "thread": "t6"});
    ev(Some(2), "needs-decision", ask);
    // herdr ready to start the thread's conductor: the answer goes to it, as a tap in the thread would.
    let r = Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply(
            "herdr workspace create",
            r#"{"result":{"root_pane":{"pane_id":"w9:p1"}}}"#,
        )
        .reply("herdr pane", "")
        .reply("herdr agent start", "")
        .reply("herdr agent wait", "")
        .reply("herdr agent prompt", "");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let html = queue_tab(&s, ADDR, T);
    assert!(
        html.contains("ship the bell now?")
            && html.contains("asked in <a href=#chat class=ath data-go=\"t6\">the bell · t6</a>"),
        "{html}"
    );
    let (mut f, buttons) = form(&html, "answer", Some(("id", "q398")));
    assert_eq!(buttons[0], ("choice".into(), "1".into()), "{buttons:?}");
    // The needs card shows it too, with the same thread line.
    let [needs, _, _] = parts(&s, ADDR, &cookie, T);
    assert!(
        needs.contains("ship the bell now?") && needs.contains("the bell · t6"),
        "{needs}"
    );
    f.insert("choice".into(), buttons[1].1.clone());
    let v = post(&s, &cookie, &f);
    assert_eq!(v["ok"], true, "{v}");
    let d = kinds(&c, "decision");
    assert_eq!(
        (d.len(), d[0].str("choice"), d[0].str("by")),
        (1, Some("after #10"), Some("owner"))
    );
    assert!(r.called("herdr agent start orchestrator-t6"), "{:?}", r.calls.borrow());
    // The thread's copy is answered, not open; the card has no form left.
    let chat: Value = serde_json::from_str(&api(&s, ADDR, &cookie, T, "chat")).unwrap();
    let t = (chat["threads"].as_array().unwrap().iter())
        .find(|t| t["id"] == "t6")
        .unwrap_or_else(|| panic!("{chat}"));
    let m = (t["page"]["msgs"].as_array().unwrap().iter())
        .find(|m| m["ask"]["id"] == "q398")
        .unwrap_or_else(|| panic!("{t}"));
    assert_eq!(
        (&m["open"], &m["answer"]["choice"]),
        (&json!(false), &json!("after #10")),
        "{m}"
    );
    assert!(!queue_tab(&s, ADDR, T).contains("ship the bell now?"));
}
