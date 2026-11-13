mod common;
use common::*;
use herdsman::events::{append, read, Event};
use herdsman::serve::Server;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

fn fixture_json(name: &str) -> Value {
    let f = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/golden/cooee/{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap()
}

fn req(s: &Server, now: u64, method: &str, path: &str, body: &str, extra: &str) -> (u16, Value) {
    let r = format!(
        "{method} {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&r, now)).into_owned();
    let (head, body) = res.split_once("\r\n\r\n").unwrap();
    assert!(head.contains("Content-Type: application/json"), "{res}");
    (head[9..12].parse().unwrap(), serde_json::from_str(body).unwrap())
}

fn recorded(name: &str) -> (herdsman::Ctx, Fake) {
    let c = ctx(&tmpdir(name), &format!("models = [\"opus\", \"sonnet\"]\n{CONFIG}"));
    for args in [
        &["queue", "sendmeter", "add", "17", "20"][..],
        &["queue", "sendmeter", "block", "--on", "17", "20"],
    ] {
        assert_eq!(run(&c, &Fake::default(), args).0, 0);
    }
    let ev = |ts: u64, n: Option<u64>, kind: &str, d: Value| {
        append(&c.state, &Event::new(ts, "sendmeter", n, kind, d)).unwrap()
    };
    let spawn = |ts, n, role: &str, agent: &str| {
        ev(
            ts,
            Some(n),
            "spawn",
            json!({"role": role, "agent": agent, "harness": "claude"}),
        )
    };
    spawn(T - 9_000, 9, "impl", "sendmeter-impl-9");
    ev(T - 600, Some(9), "merged", json!({"pr": 41, "into": "staging"}));
    spawn(T - 5_000, 12, "impl", "sendmeter-impl-12");
    spawn(T - 4_000, 12, "rev", "sendmeter-rev-12-r1");
    ev(
        T - 3_500,
        Some(12),
        "verdict",
        json!({"verdict": "FAIL", "sha": "a".repeat(40)}),
    );
    spawn(T - 3_000, 12, "fix", "sendmeter-impl-12");
    spawn(T - 8_000, 5, "impl", "sendmeter-impl-5");
    ev(
        T - 2_000,
        Some(5),
        "verdict",
        json!({"verdict": "FAIL", "sha": "b".repeat(40)}),
    );
    ev(
        T - 1_900,
        Some(5),
        "needs-human",
        json!({"code": "ROUND_LIMIT", "reason": "review rounds ran out (4/3)"}),
    );
    let ask = json!({"id": "q7", "question": "Move the branch out?", "options": ["move", "refuse"], "recommend": 1, "kind": "routine"});
    ev(T - 300, None, "needs-decision", ask);
    let card = json!({"thread": "t1", "id": "r1", "cmd": "git push origin --delete wip/x", "why": "Delete #9's wip snapshot",
        "blast": "origin: 1 wip/ branch", "dir": "/tmp/sendmeter", "host": "main", "timeout": 60, "sha": "3b7e0c55a1d9".repeat(5)});
    ev(T - 120, None, "run-card", card);
    let list = agents(&[("sendmeter-impl-12", "working", "/x", "w1:p2")]);
    (c, Fake::default().reply("herdr agent list", &list))
}

fn skeleton(v: &Value, at: &str, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                match x {
                    Value::String(t) if k == "type" => drop(out.insert(format!("{at}.type={t}"))),
                    _ => skeleton(x, &format!("{at}.{k}"), out),
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| skeleton(x, &format!("{at}[]"), out)),
        _ => {}
    }
}

fn vocabulary() -> BTreeMap<String, BTreeSet<String>> {
    fn walk(v: &Value, parent: &str, out: &mut BTreeMap<String, BTreeSet<String>>) {
        match v {
            Value::Object(m) => {
                let kind = m
                    .get("type")
                    .and_then(Value::as_str)
                    .map_or(format!("under {parent}"), String::from);
                out.entry(kind).or_default().extend(m.keys().cloned());
                m.iter().for_each(|(k, x)| walk(x, k, out));
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, parent, out)),
            _ => {}
        }
    }
    let mut out = BTreeMap::new();
    for f in ["queue", "queue-stream", "events", "cards", "own-cards", "fleets"] {
        walk(&fixture_json(f), "", &mut out);
    }
    out
}

fn like(v: &Value, fixture: &str, exact: bool) {
    let vocab = vocabulary();
    fn keys(v: &Value, parent: &str, vocab: &BTreeMap<String, BTreeSet<String>>) {
        match v {
            Value::Object(m) => {
                let kind = m
                    .get("type")
                    .and_then(Value::as_str)
                    .map_or(format!("under {parent}"), String::from);
                let known = vocab
                    .get(&kind)
                    .unwrap_or_else(|| panic!("{kind} is in no fixture: {v}"));
                let spec = ["under swipe", "under options"].contains(&kind.as_str());
                let odd = m
                    .keys()
                    .find(|k| !known.contains(*k) && *k != "id" && !(spec && *k == "values"));
                assert!(
                    odd.is_none(),
                    "{kind}.{} is in no fixture: {v}",
                    odd.unwrap_or(&String::new())
                );
                m.iter()
                    .filter(|(k, _)| *k != "values")
                    .for_each(|(k, x)| keys(x, k, vocab));
            }
            Value::Array(a) => a.iter().for_each(|x| keys(x, parent, vocab)),
            _ => {}
        }
    }
    keys(v, "", &vocab);
    let (mut got, mut want) = (BTreeSet::new(), BTreeSet::new());
    skeleton(v, "", &mut got);
    skeleton(&fixture_json(fixture), "", &mut want);
    let extra: Vec<_> = got.difference(&want).collect();
    assert!(extra.is_empty(), "{fixture}: not in the fixture: {extra:?}");
    if exact {
        assert_eq!(got, want, "{fixture}");
    }
}

fn find<'a>(v: &'a Value, id: &str) -> &'a Value {
    fn go<'a>(v: &'a Value, id: &str) -> Option<&'a Value> {
        if v.get("id").and_then(Value::as_str) == Some(id) {
            return Some(v);
        }
        match v {
            Value::Object(m) => m.values().find_map(|x| go(x, id)),
            Value::Array(a) => a.iter().find_map(|x| go(x, id)),
            _ => None,
        }
    }
    go(v, id).unwrap_or_else(|| panic!("no block {id} in {v}"))
}

#[test]
fn queue_events_fleets_and_cards_are_cooees_fixtures_from_recorded_state() {
    let (c, r) = recorded("cooee-get");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let get = |p: &str| {
        let (status, v) = req(&s, T, "GET", p, "", "");
        assert_eq!(status, 200, "{p}: {v}");
        v
    };
    let q = get("/screens/queue");
    like(&q, "queue", true);
    let fx = fixture_json("queue");
    for k in ["type", "id", "title", "stream", "actions"] {
        assert_eq!(q[k], fx[k], "{k}");
    }
    let row = find(&q, "q-sendmeter-12");
    assert_eq!(row["title"], "#12");
    assert_eq!(row["badge"], json!({"text": "fix 1", "tone": "working"}));
    assert_eq!(row["figure"], json!({"state": "working", "tool": "edit"}));
    assert_eq!(row["subtitle"], "impl ✓ · review 1 ✗ · fix 1 ▸ sendmeter-impl-12");
    assert_eq!(row["tap"], json!({"thread": "sendmeter-impl-12"}));
    assert_eq!(row["swipe"][0]["action"], "stop");
    assert_eq!(find(&q, "q-sendmeter-5")["figure"], json!({"state": "needs"}));
    assert_eq!(
        find(&q, "q-sendmeter-17")["badge"],
        json!({"text": "#1", "tone": "mute"})
    );
    let held = find(&q, "q-sendmeter-20");
    assert_eq!(
        (&held["indent"], &held["subtitle"]),
        (&json!(1), &json!({"text": "held until #17", "tone": "bad"}))
    );
    assert_eq!(find(&q, "inprogress-sendmeter")["header"], "in progress (2)");
    assert_eq!(find(&q, "done-today-sendmeter")["value"], "1");
    assert_eq!(find(&q, "stats")["items"][0]["value"], "1");
    assert_eq!(find(&q, "stats")["items"][0]["spark"], json!([0, 0, 0, 0, 0, 0, 1]));
    let none = get("/screens/queue?repo=nope");
    assert_eq!(find(&none, "repo")["selected"], json!(["all"]));

    let e = get("/screens/events");
    like(&e, "events", true);
    let rows = find(&e, "ev")["rows"].as_array().unwrap();
    let titles: Vec<&str> = rows.iter().map(|r| r["title"].as_str().unwrap()).collect();
    assert!(titles[0].contains("q7"), "{titles:?}");
    assert!(
        rows.iter().any(|r| r["tap"] == json!({"thread": "sendmeter-impl-12"})),
        "{titles:?}"
    );
    let merged = rows
        .iter()
        .find(|r| r["tap"]["url"] == "https://github.com/jirathip-dev/sendmeter/pull/41");
    assert!(merged.is_some_and(|m| m["figure"]["state"] == "done"), "{rows:?}");
    let asks = get("/screens/events?kind=ask");
    assert_eq!(find(&asks, "ev")["rows"].as_array().unwrap().len(), 1);
    assert_eq!(asks["stream"], "/screens/events/stream?kind=ask");

    let f = get("/screens/fleets");
    like(&f, "fleets", false);
    let form = find(&f, "f-sendmeter");
    assert_eq!(
        form["fields"][0],
        json!({"type": "toggle", "name": "sendmeter.drive", "label": "Drive", "value": true})
    );
    assert_eq!(form["fields"][2]["options"], json!(["opus", "sonnet"]));
    assert_eq!(find(&f, "rm-sendmeter")["action"], "remove-fleet");

    let cards = get("/screens/cards");
    like(&cards, "cards", false);
    let ids: Vec<&str> = cards
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["run-r1", "ask-q7", "nhc-sendmeter-5"]);
    let ask = find(&cards, "q7");
    assert_eq!(
        (&ask["recommended"], &ask["default"]["value"]),
        (&json!("move"), &json!("move"))
    );
    assert_eq!(find(&cards, "r1")["sha"], "3b7e0c55a1d9");
    let nh = find(&cards, "nh-sendmeter-5")["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["value"].clone());
    assert_eq!(nh.collect::<Vec<_>>(), [json!("extend"), json!("park")]);
    let folds: Vec<&Value> = cards.as_array().unwrap()[1..].iter().map(|c| &c["fold"]).collect();
    assert_eq!(folds[0]["line"], "routine · Move the branch out? · 5m");
    assert_eq!(
        folds[0]["main"],
        json!({"block": "q7", "action": "answer", "value": "move", "label": "move"})
    );
    assert_eq!(folds[1]["line"], "#5 · needs-human · review rounds ran out (4/3) · 31m");
    assert_eq!(
        (&folds[1]["main"]["block"], &folds[1]["main"]["label"]),
        (&json!("nh-sendmeter-5"), &json!("one more round"))
    );
    assert_eq!(req(&s, T, "GET", "/screens/nope", "", "").0, 404);
    let foreign = req(&s, T, "GET", "/screens/queue", "", "Origin: https://evil.example\r\n");
    assert_eq!(foreign.0, 403);
}

#[test]
fn the_manifest_declares_cooees_places_and_permissions_and_each_screen_is_served() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../cooee-plugin.toml");
    let m: toml::Value = toml::from_str(&std::fs::read_to_string(root).unwrap()).unwrap();
    let m: Value = serde_json::to_value(m).unwrap();
    let mut places = fixture_json("places")["places"].clone();
    assert_eq!(m["places"], places);
    let want = fixture_json("permissions")["permissions"].clone();
    let p = &m["permissions"];
    for k in ["push", "actions", "destructive"] {
        assert_eq!(p[k], want[k], "{k}");
    }
    assert_eq!(
        p["network"],
        json!(["api.github.com", ADDR]),
        "GitHub and herdsman serve's own address"
    );
    let kinds: BTreeSet<&str> = places
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["type"].as_str().unwrap())
        .collect();
    let declared: BTreeSet<&str> = p["places"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_str().unwrap())
        .collect();
    assert_eq!(kinds, declared);
    let actions = p["actions"].as_array().unwrap();
    assert!(p["destructive"].as_array().unwrap().iter().all(|d| actions.contains(d)));
    for k in ["name", "version", "build", "run"] {
        assert!(m[k].is_string(), "{k}");
    }
    let (c, r) = recorded("cooee-manifest");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    for place in places
        .as_array_mut()
        .unwrap()
        .iter()
        .filter(|p| p["type"] != "chat-cards")
    {
        let screen = place["screen"].as_str().unwrap();
        assert!(screen.starts_with("/screens/"));
        assert_eq!(req(&s, T, "GET", screen, "", "").0, 200, "{screen}");
    }
}

#[test]
fn an_action_outside_the_manifest_is_refused_and_a_listed_one_runs() {
    let (c, r) = recorded("cooee-action");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let post = |v: Value, extra: &str| req(&s, T, "POST", "/action", &v.to_string(), extra);
    let drop17 = json!({"screen": "queue", "block": "q-sendmeter-17", "action": "drop", "values": {"fleet": "sendmeter", "issue": 17}});
    assert_eq!(post(drop17.clone(), "").0, 401);
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let before = read(&c.state).unwrap().len();
    let merge = json!({"screen": "queue", "block": "q-sendmeter-12", "action": "merge", "values": {"fleet": "sendmeter", "issue": 12}});
    let (status, v) = post(merge, &ck);
    assert_eq!(status, 403, "{v}");
    assert!(v["line"].as_str().unwrap().starts_with("REFUSED NOT_PERMITTED"), "{v}");
    assert_eq!(read(&c.state).unwrap().len(), before, "a refused action logs nothing");

    let (status, v) = post(drop17, &ck);
    assert_eq!(status, 200, "{v}");
    let last = read(&c.state).unwrap().to_vec().pop().unwrap();
    assert_eq!(
        (last.kind.as_str(), &last.data["op"], &last.data["issues"]),
        ("queue", &json!("rm"), &json!([17]))
    );

    let answer =
        json!({"screen": "cards", "block": "q7", "action": "answer", "values": {"choice": "refuse", "other": ""}});
    let cards = |ck: &str| req(&s, T, "GET", "/screens/cards", "", ck).1.to_string();
    assert!(cards(&ck).contains("\"ask-q7\""));
    let (status, v) = post(answer.clone(), &ck);
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["ops"], json!([{"op": "remove", "id": "ask-q7"}]), "{v}");
    assert!(!cards(&ck).contains("\"ask-q7\""));
    let state = req(&s, T, "GET", "/api/v1/state", "", &ck).1;
    assert!(!state["asks"].to_string().contains("\"q7\""), "{state}");
    let d = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .rfind(|e| e.kind == "decision")
        .unwrap();
    assert_eq!((d.str("id"), d.str("choice")), (Some("q7"), Some("refuse")));
    let (status, v) = post(answer, &ck);
    assert_eq!(status, 400, "an answered ask is no longer open: {v}");

    let filter = json!({"screen": "events", "block": "kind", "action": "filter", "values": {"kind": ["merge"]}});
    let (status, v) = post(filter, &ck);
    assert_eq!((status, &v["ops"][0]["op"]), (200, &json!("screen")), "{v}");
    let rows = find(&v["ops"][0]["screen"], "ev")["rows"].as_array().unwrap().clone();
    assert_eq!(rows.len(), 1, "{rows:?}");

    let deny = json!({"screen": "cards", "block": "r1", "action": "run-card", "values": {"choice": "deny", "sha": "3b7e0c55a1d9"}});
    let (status, v) = post(deny, &ck);
    assert_eq!(status, 200, "{v}");
    let settled = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .rfind(|e| e.kind == "run-settled")
        .unwrap();
    assert_eq!(
        (settled.str("via"), &settled.data["approved"]),
        (Some("cooee"), &json!(false))
    );
}

#[test]
fn a_folded_cards_main_button_answers_its_ask() {
    let (c, r) = recorded("cooee-fold");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let cards = req(&s, T, "GET", "/screens/cards", "", "").1;
    let main = &find(&cards, "ask-q7")["fold"]["main"];
    let tap = json!({"screen": "cards", "block": main["block"], "action": main["action"], "values": {"choice": main["value"]}});
    let (status, v) = req(&s, T, "POST", "/action", &tap.to_string(), &ck);
    assert_eq!(status, 200, "{v}");
    let d = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .rfind(|e| e.kind == "decision")
        .unwrap();
    assert_eq!((d.str("id"), d.str("choice")), (Some("q7"), Some("move")));
}

fn apply(screen: &mut Value, ops: &[Value]) {
    fn at<'a>(v: &'a mut Value, id: &str) -> Option<&'a mut Value> {
        if v.get("id").and_then(Value::as_str) == Some(id) {
            return Some(v);
        }
        match v {
            Value::Object(m) => m.values_mut().find_map(|x| at(x, id)),
            Value::Array(a) => a.iter_mut().find_map(|x| at(x, id)),
            _ => None,
        }
    }
    fn drop_id(v: &mut Value, id: &str) {
        match v {
            Value::Object(m) => m.values_mut().for_each(|x| drop_id(x, id)),
            Value::Array(a) => {
                a.retain(|x| x.get("id").and_then(Value::as_str) != Some(id));
                a.iter_mut().for_each(|x| drop_id(x, id));
            }
            _ => {}
        }
    }
    for op in ops {
        let id = op["id"].as_str().unwrap_or_default();
        match op["op"].as_str().unwrap() {
            "screen" => *screen = op["screen"].clone(),
            "badge" => screen["badge"] = op["count"].clone(),
            "merge" => {
                let b = at(screen, id).unwrap_or_else(|| panic!("merge: no {id}"));
                op["fields"]
                    .as_object()
                    .unwrap()
                    .iter()
                    .for_each(|(k, v)| b[k] = v.clone());
            }
            "replace" => *at(screen, id).unwrap_or_else(|| panic!("replace: no {id}")) = op["block"].clone(),
            "remove" => drop_id(screen, id),
            "insert" => {
                let into = at(screen, op["into"].as_str().unwrap()).unwrap();
                let list = if into.get("rows").is_some() {
                    &mut into["rows"]
                } else {
                    &mut into["blocks"]
                };
                let list = list.as_array_mut().unwrap();
                let before = op["before"]
                    .as_str()
                    .and_then(|b| list.iter().position(|x| x["id"] == b));
                list.insert(before.unwrap_or(list.len()), op["block"].clone());
            }
            o => panic!("unknown op {o}"),
        }
    }
}

#[test]
fn cooees_queue_stream_fixture_applies_in_place() {
    let mut q = fixture_json("queue");
    for e in fixture_json("queue-stream").as_array().unwrap() {
        apply(&mut q, e["ops"].as_array().unwrap());
    }
    assert_eq!(find(&q, "q-722")["badge"]["text"], "review 1");
    assert_eq!(find(&q, "q-456")["badge"]["text"], "impl");
    assert_eq!(find(&q, "q-686")["subtitle"]["text"], "held until #456 · in impl");
    assert_eq!(find(&q, "done-today")["value"], "7");
    assert!(!q.to_string().contains("q-708"));
    assert_eq!(q["badge"], 6);
}

fn next(r: &mut BufReader<std::net::TcpStream>) -> (u64, Vec<Value>) {
    let mut line = String::new();
    loop {
        line.clear();
        assert!(r.read_line(&mut line).unwrap() > 0, "stream closed");
        if let Some(d) = line.strip_prefix("data: ") {
            let v: Value = serde_json::from_str(d).unwrap();
            return (v["seq"].as_u64().unwrap(), v["ops"].as_array().unwrap().clone());
        }
    }
}

#[test]
fn the_queue_stream_updates_rows_in_place_and_the_strip_count() {
    let (c, _) = recorded("cooee-stream");
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let c2 = c.clone();
    std::thread::spawn(move || {
        let list = agents(&[
            ("sendmeter-impl-12", "working", "/x", "w1:p2"),
            ("sendmeter-rev-12-r2", "working", "/x", "w1:p3"),
            ("sendmeter-impl-17", "working", "/x", "w1:p4"),
        ]);
        herdsman::serve::serve_on(&c2, &Fake::default().reply("herdr agent list", &list), l, false)
    });
    let mut s = std::net::TcpStream::connect(&addr).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(20))).unwrap();
    write!(
        s,
        "GET /screens/queue/stream HTTP/1.1\r\nHost: {addr}\r\nLast-Event-ID: 40\r\n\r\n"
    )
    .unwrap();
    let mut r = BufReader::new(s);
    let (seq, ops) = next(&mut r);
    assert_eq!(
        (seq, &ops[0]["op"], &ops[1]),
        (41, &json!("screen"), &json!({"op": "badge", "count": 2}))
    );
    let mut screen = ops[0]["screen"].clone();
    let ev = |n, kind: &str, d: Value| {
        append(&c.state, &Event::new(herdsman::now(), "sendmeter", Some(n), kind, d)).unwrap()
    };

    ev(
        12,
        "spawn",
        json!({"role": "rev", "agent": "sendmeter-rev-12-r2", "harness": "claude"}),
    );
    let (seq, ops) = next(&mut r);
    assert_eq!(
        (seq, &ops[0]["op"], &ops[0]["id"]),
        (42, &json!("merge"), &json!("q-sendmeter-12"))
    );
    assert_eq!(ops[0]["fields"]["badge"]["text"], "review 2", "{ops:?}");
    apply(&mut screen, &ops);

    ev(5, "merged", json!({"pr": 42, "into": "staging"}));
    let (_, ops) = next(&mut r);
    assert_eq!(ops[0], json!({"op": "remove", "id": "q-sendmeter-5"}), "{ops:?}");
    assert!(
        ops.contains(&json!({"op": "merge", "id": "done-today-sendmeter", "fields": {"value": "1"}})),
        "{ops:?}"
    );
    assert!(ops.contains(&json!({"op": "badge", "count": 1})), "{ops:?}");
    apply(&mut screen, &ops);

    ev(
        17,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-17", "harness": "claude"}),
    );
    let (_, ops) = next(&mut r);
    let insert = ops
        .iter()
        .find(|o| o["op"] == "insert")
        .unwrap_or_else(|| panic!("{ops:?}"));
    assert_eq!(
        (&insert["into"], &insert["block"]["id"]),
        (&json!("inprogress-list-sendmeter"), &json!("q-sendmeter-17"))
    );
    apply(&mut screen, &ops);

    let list = agents(&[
        ("sendmeter-impl-12", "working", "/x", "w1:p2"),
        ("sendmeter-rev-12-r2", "working", "/x", "w1:p3"),
        ("sendmeter-impl-17", "working", "/x", "w1:p4"),
    ]);
    let r2 = Fake::default().reply("herdr agent list", &list);
    let fresh = req(
        &Server::new(&c, &r2, Some(ADDR.into())),
        herdsman::now(),
        "GET",
        "/screens/queue",
        "",
        "",
    )
    .1;
    screen.as_object_mut().unwrap().remove("badge");
    assert_eq!(screen, fresh, "the ops turned the first answer into today's");
}
