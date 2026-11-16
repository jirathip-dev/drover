mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::serve::Server;
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

fn recorded(name: &str) -> (drover::Ctx, OnShed) {
    let c = ctx(&tmpdir(name), &format!("models = [\"opus\", \"sonnet\"]\n{CONFIG}"));
    for args in [
        &["queue", "sendmeter", "add", "17", "20"][..],
        &["queue", "sendmeter", "block", "--on", "17", "20"],
    ] {
        assert_eq!(run(&c, &Fake::default(), args).0, 0);
    }
    let ev = |ts: u64, n: Option<u64>, kind: &str, d: Value| {
        append(&c.state, &Event::new(ts, "sendmeter", n, kind, d)).unwrap();
    };
    let spawn = |ts, n, role: &str, agent: &str| {
        ev(
            ts,
            Some(n),
            "spawn",
            json!({"role": role, "agent": agent, "harness": "claude"}),
        );
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
    let r = OnShed::new(Fake::default());
    r.1.lane("sendmeter-impl-12", "working", "/x");
    (c, r)
}

fn working() -> OnShed {
    let r = OnShed::new(Fake::default());
    for name in ["sendmeter-impl-12", "sendmeter-rev-12-r2", "sendmeter-impl-17"] {
        r.1.lane(name, "working", "/x");
    }
    r
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
                let spec = ["under swipe", "under options", "under done"].contains(&kind.as_str());
                let coat = kind == "under figure";
                let odd = m.keys().find(|k| {
                    !known.contains(*k) && *k != "id" && (!spec || *k != "values") && (!coat || *k != "coat")
                });
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
    let vocab = vocabulary();
    keys(v, "", &vocab);
    let (mut got, mut want) = (BTreeSet::new(), BTreeSet::new());
    skeleton(v, "", &mut got);
    for f in fixture.split('+') {
        skeleton(&fixture_json(f), "", &mut want);
    }
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
    assert_eq!(row["subtitle"], "impl ✓ · review 1 ✗ · fix 1 ▸ working · sendmete…");
    assert_eq!(row["tap"], json!({"sheet": "/screens/lanes/sendmeter-impl-12"}));
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
fn a_queue_row_is_one_short_title_line_without_the_pr_title() {
    let (c, r) = recorded("cooee-short-row");
    let (issue, pr) = ("Long issue title ".repeat(7), "Long PR title ".repeat(8));
    assert!(issue.len() > 100 && pr.len() > 100);
    let cache = c.state.parent().unwrap().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(
        cache.join("issue-sendmeter-12.json"),
        json!({"title": issue}).to_string(),
    )
    .unwrap();
    let cached = json!({"ts": T, "pr": {"number": 281, "title": pr}});
    std::fs::write(cache.join("pr-sendmeter-12.json"), cached.to_string()).unwrap();
    let (status, q) = req(
        &Server::new(&c, &r, Some(ADDR.into())),
        T,
        "GET",
        "/screens/queue",
        "",
        "",
    );
    assert_eq!(status, 200, "{q}");
    let row = find(&q, "q-sendmeter-12");
    let (title, sub) = (row["title"].as_str().unwrap(), row["subtitle"].as_str().unwrap());
    assert!(
        title.starts_with("#12 Long issue title") && title.ends_with('…'),
        "{title}"
    );
    assert!(!title.contains("PR"), "{title}");
    assert!(sub.contains("PR #281") && !sub.contains("Long PR title"), "{sub}");
    for line in [title, sub] {
        assert!(line.chars().count() <= 49 && !line.contains('\n'), "{line}");
    }
    assert_eq!(row["tap"], json!({"sheet": "/screens/lanes/sendmeter-impl-12"}));
}

#[test]
fn parity_50_the_lanes_screen_and_a_lanes_sheet_are_cooees_blocks() {
    let (c, r) = recorded("cooee-lanes");
    r.1.lane("sendmeter-impl-20", "idle", "/x");
    r.1.lane("sendmeter-impl-5", "blocked", "/x");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let (status, v) = req(&s, T, "GET", "/screens/lanes", "", "");
    assert_eq!(status, 200, "{v}");
    like(&v, "lanes", true);
    assert_eq!(
        (&v["id"], &v["stream"]),
        (&json!("lanes"), &json!("/screens/lanes/stream"))
    );
    let row = find(&v, "l-sendmeter-impl-12");
    assert_eq!(row["title"], "sendmeter #12");
    assert_eq!(row["figure"]["state"], "working");
    assert!(row["figure"]["coat"].is_string(), "{row}");
    assert_eq!(row["tap"], json!({"sheet": "/screens/lanes/sendmeter-impl-12"}));
    assert_eq!(find(&v, "lanes-working")["header"], "working (1)");
    assert_eq!(find(&v, "lanes-blocked")["header"], "waiting (1)");
    assert_eq!(find(&v, "lanes-idle")["header"], "asleep (1)");

    let sheet = "/screens/lanes/sendmeter-impl-12";
    assert_eq!(
        req(&s, T, "GET", sheet, "", "").0,
        401,
        "a transcript needs a paired device"
    );
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let (status, v) = req(&s, T, "GET", sheet, "", &ck);
    assert_eq!(status, 200, "{v}");
    like(&v, "lane-sheet", true);
    assert_eq!(
        (&v["type"], &v["title"]),
        (&json!("sheet"), &json!("sendmeter-impl-12"))
    );
    assert_eq!(find(&v, "f-issue")["value"], "sendmeter #12");
    assert_eq!(find(&v, "f-role")["value"], "fix");
    let acts: Vec<&Value> = ["act-stop", "act-kill", "act-clean"]
        .iter()
        .map(|a| &find(&v, a)["action"])
        .collect();
    assert_eq!(acts, [&json!("stop"), &json!("kill"), &json!("clean")]);
    assert_eq!(
        v["done"],
        json!({"label": "Send", "action": "send", "values": {"agent": "sendmeter-impl-12"}})
    );
    assert_eq!(find(&v, "transcript")["blocks"][0]["type"], "text");
    assert_eq!(req(&s, T, "GET", "/screens/lanes/nobody", "", &ck).0, 404);
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
        "GitHub and drover serve's own address"
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
        (last.kind.as_str(), last.val("op"), last.val("issues")),
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
        (settled.str("via"), settled.val("approved")),
        (Some("cooee"), &json!(false))
    );
}

#[test]
fn parity_63_a_folded_cards_main_button_answers_its_ask() {
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
                for x in a.iter_mut() {
                    drop_id(x, id);
                }
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
    std::thread::spawn(move || drover::serve::serve_on(&c2, &working(), l, false));
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
    let ev =
        |n, kind: &str, d: Value| append(&c.state, &Event::new(drover::now(), "sendmeter", Some(n), kind, d)).unwrap();

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

    let r2 = working();
    let fresh = req(
        &Server::new(&c, &r2, Some(ADDR.into())),
        drover::now(),
        "GET",
        "/screens/queue",
        "",
        "",
    )
    .1;
    screen.as_object_mut().unwrap().remove("badge");
    assert_eq!(screen, fresh, "the ops turned the first answer into today's");
}

#[test]
fn the_workflow_screen_is_two_diagram_blocks_with_live_counts_and_limit_nodes() {
    let (c, r) = recorded("cooee-workflow");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let (status, v) = req(&s, T, "GET", "/screens/workflow", "", "");
    assert_eq!(status, 200, "{v}");
    assert_eq!((&v["type"], &v["id"]), (&json!("screen"), &json!("workflow")));
    let chart = find(&v, "states");
    assert_eq!(chart["type"], "diagram");
    assert!(
        chart["source"].as_str().unwrap().starts_with("stateDiagram-v2\n"),
        "{chart}"
    );
    assert!(
        chart["source"]
            .as_str()
            .unwrap()
            .contains("state \"fix(n) · 1\" as fix"),
        "{chart}"
    );
    for (state, n) in [("merged", 1), ("fix(n)", 1), ("needs-human", 1)] {
        assert_eq!(chart["counts"][state], n, "{state}: {chart}");
    }
    let pass = find(&v, "pass");
    assert_eq!(pass["type"], "diagram");
    let source = pass["source"].as_str().unwrap();
    let nodes = pass["nodes"].as_array().unwrap();
    for (id, limit) in [
        ("k_slow_secs", "slow_secs"),
        ("k_fast_secs", "fast_secs"),
        ("k_stall_minutes", "stall_minutes"),
    ] {
        assert!(nodes.contains(&json!({"id": id, "limit": limit})), "{id}: {nodes:?}");
    }
    for n in nodes {
        assert!(
            source.contains(&format!(
                "{}([\"{} = ",
                n["id"].as_str().unwrap(),
                n["limit"].as_str().unwrap()
            )),
            "{n}"
        );
    }
}

fn rows_of<'a>(v: &'a Value, list: &str) -> Vec<&'a str> {
    let rows = find(v, list)["rows"].as_array().unwrap();
    rows.iter().map(|r| r["id"].as_str().unwrap()).collect()
}

#[test]
fn parity_38_the_queue_draws_chains_as_a_folding_tree_more_rows_lock_tags_and_moves() {
    let (c, r) = recorded("cooee-parity");
    let ok = |args: &[&str]| assert_eq!(run(&c, &Fake::default(), args).0, 0, "{args:?}");
    ok(&["queue", "sendmeter", "add", "30", "31", "32", "33", "34"]);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let get = |p: &str| {
        let (status, v) = req(&s, T, "GET", p, "", "");
        assert_eq!(status, 200, "{p}: {v}");
        v
    };
    let q = get("/screens/queue");
    like(&q, "queue", false);
    assert_eq!(find(&q, "repo")["options"][1]["label"], "sendmeter ●", "a needs dot");
    assert_eq!(find(&q, "fleet-sendmeter")["tone"], "warn", "needs you");
    assert_eq!(
        rows_of(&q, "queued-list-sendmeter"),
        [
            "q-sendmeter-17",
            "chain-sendmeter-17",
            "q-sendmeter-20",
            "q-sendmeter-30",
            "q-sendmeter-31",
            "q-sendmeter-32",
            "more-queued-sendmeter"
        ]
    );
    assert_eq!(find(&q, "queued-sendmeter")["header"], "queued (7)");
    assert_eq!(find(&q, "q-sendmeter-20")["indent"], 1);
    let more = find(&q, "more-queued-sendmeter");
    assert_eq!(
        (&more["title"], &more["tap"]),
        (&json!("+2 more"), &json!({"open": "/screens/queue?more=sendmeter"}))
    );
    let all = get("/screens/queue?more=sendmeter");
    let ids = rows_of(&all, "queued-list-sendmeter");
    assert_eq!(&ids[6..], ["q-sendmeter-33", "q-sendmeter-34", "more-queued-sendmeter"]);
    assert_eq!(all["stream"], "/screens/queue/stream?more=sendmeter");
    let fold = find(&q, "chain-sendmeter-17")["tap"]["open"].as_str().unwrap();
    let folded = get(fold);
    assert!(!rows_of(&folded, "queued-list-sendmeter").contains(&"q-sendmeter-20"));
    assert_eq!(find(&folded, "chain-sendmeter-17")["title"], "▸ 1 held behind #17");
    assert_eq!(find(&folded, "chain-sendmeter-17")["tap"]["open"], "/screens/queue");

    let swipe = |v: &Value, id: &str| -> Vec<Value> {
        find(v, id)["swipe"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["label"].clone())
            .collect()
    };
    assert_eq!(swipe(&q, "q-sendmeter-17"), ["▼", "Top", "Drop"]);
    assert_eq!(swipe(&q, "q-sendmeter-30"), ["▲", "▼", "Top", "Drop"]);
    let up = find(&q, "q-sendmeter-30")["swipe"][0].clone();
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let tap = json!({"screen": "queue", "block": "q-sendmeter-30", "action": up["action"], "values": up["values"]});
    let (status, v) = req(&s, T, "POST", "/action", &tap.to_string(), &ck);
    assert_eq!(status, 200, "{v}");
    let again = get("/screens/queue");
    assert_eq!(
        find(&again, "q-sendmeter-30")["badge"]["text"],
        "#2",
        "moved above #20, kept on a reload"
    );

    ok(&["pause", "sendmeter"]);
    let paused = find(&get("/screens/queue"), "fleet-sendmeter").clone();
    assert!(
        paused["fold"]["line"]
            .as_str()
            .unwrap()
            .starts_with("sendmeter · paused"),
        "{paused}"
    );
    assert_eq!(
        paused["fold"]["main"],
        json!({"action": "pause", "label": "Resume", "values": {"fleet": "sendmeter", "on": false}})
    );
    let legend = get(q["actions"][0]["menu"][2]["tap"]["sheet"].as_str().unwrap());
    like(&legend, "fleets", false);
    let words: Vec<&str> = find(&legend, "legend-list")["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["title"].as_str().unwrap())
        .collect();
    for w in [
        "queued",
        "impl",
        "review n",
        "fix n",
        "ready",
        "merged",
        "needs-human",
        "parked",
        "dead",
        "🔒 hot ×N",
    ] {
        assert!(words.contains(&w), "{w}: {words:?}");
    }
}

#[test]
fn a_row_opens_its_issue_sheet_with_lit_stage_links_and_its_asking_thread() {
    let (c, r) = recorded("cooee-issue");
    let ask =
        json!({"id": "q8", "question": "Split the PR?", "options": ["split", "keep"], "recommend": 2, "thread": "t9"});
    append(
        &c.state,
        &Event::new(T - 60, "sendmeter", Some(17), "needs-decision", ask),
    )
    .unwrap();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let get = |p: &str| {
        let (status, v) = req(&s, T, "GET", p, "", "");
        assert_eq!(status, 200, "{p}: {v}");
        v
    };
    let q = get("/screens/queue");
    let waiting = find(&q, "q-sendmeter-17");
    assert_eq!(waiting["subtitle"], "asks: Split the PR? · split / keep (recommended)");
    assert_eq!(waiting["tap"], json!({"thread": "t9"}));
    let sheet = get(find(&q, "q-sendmeter-20")["tap"]["sheet"].as_str().unwrap());
    assert_eq!((&sheet["type"], &sheet["title"]), (&json!("sheet"), &json!("#20")));
    let issue = find(&sheet, "issue");
    assert_eq!(
        issue["tap"]["url"],
        "https://github.com/jirathip-dev/sendmeter/issues/20"
    );
    let fix = get("/screens/issue?repo=sendmeter&issue=12");
    let stage = find(&fix, "stage");
    let lit = stage["selected"].as_array().unwrap();
    assert_eq!(lit, &[json!("impl · fix")], "the stage #12 is on, lit: {stage}");
    assert_eq!(find(&fix, "lane")["tap"], json!({"thread": "sendmeter-impl-12"}));
    let asked = get("/screens/issue?repo=sendmeter&issue=17");
    assert_eq!(find(&asked, "asked")["tap"], json!({"thread": "t9"}));
    like(&asked, "fleets+queue", false);
    assert_eq!(find(&asked, "asks")["header"], "asks: Split the PR?");
    assert_eq!(find(&asked, "opt-1")["title"], "2. keep");
    assert_eq!(find(&asked, "▼")["action"], "move");
    assert_eq!(
        req(&s, T, "GET", "/screens/issue?repo=sendmeter&issue=99", "", "").0,
        404
    );
}

#[test]
fn parity_47_the_backlog_sheet_filters_searches_and_queues_the_ticked_issues() {
    let (c, r) = recorded("cooee-backlog");
    let issue = |t: &str, ac: bool, created: &str| json!({"title": t, "labels": ["ui"], "created": created, "updated": created, "ac": ac, "pr": false});
    let bl = json!({"ts": T, "issues": {"40": issue("Dark mode", true, "1970-01-01T00:00:00Z"), "41": issue("Fix push", false, "1970-01-12T00:00:00Z"), "42": issue("Old thing", false, "1970-01-01T00:00:00Z")}});
    let p = c.state.with_file_name("cache");
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(p.join("backlog-sendmeter-0.json"), bl.to_string()).unwrap();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let get = |p: &str| {
        let (status, v) = req(&s, T, "GET", p, "", "");
        assert_eq!(status, 200, "{p}: {v}");
        v
    };
    let row = find(&get("/screens/queue"), "backlog-sendmeter").clone();
    assert_eq!(row["title"], "backlog (3)");
    let sheet = get(row["tap"]["sheet"].as_str().unwrap());
    like(&sheet, "fleets+queue", false);
    let names = |v: &Value| -> Vec<String> {
        find(v, "bl-sendmeter")["fields"].as_array().unwrap()[1..]
            .iter()
            .map(|f| f["name"].as_str().unwrap().into())
            .collect()
    };
    assert_eq!(
        names(&sheet),
        ["sendmeter.pick.42", "sendmeter.pick.41", "sendmeter.pick.40"]
    );
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let post = |v: Value| req(&s, T, "POST", "/action", &v.to_string(), &ck);
    let (status, v) = post(
        json!({"screen": "backlog-sendmeter", "block": "on", "action": "filter", "values": {"sendmeter.on": ["ac"]}}),
    );
    assert_eq!(status, 200, "{v}");
    assert_eq!(names(&v["ops"][0]["screen"]), ["sendmeter.pick.40"]);
    let (_, v) = post(json!({"screen": "backlog-sendmeter", "action": "queue", "values": {"sendmeter.q": "push"}}));
    assert_eq!(names(&v["ops"][0]["screen"]), ["sendmeter.pick.41"], "{v}");
    let ticks =
        json!({"sendmeter.q": "", "sendmeter.pick.40": true, "sendmeter.pick.41": false, "sendmeter.pick.42": true});
    let (status, v) = post(json!({"screen": "backlog-sendmeter", "action": "queue", "values": ticks}));
    assert_eq!((status, &v["close"]), (200, &json!(true)), "{v}");
    let q = get("/screens/queue?more=sendmeter");
    let ids = rows_of(&q, "queued-list-sendmeter");
    assert!(
        ids.contains(&"q-sendmeter-40") && ids.contains(&"q-sendmeter-42") && !ids.contains(&"q-sendmeter-41"),
        "{ids:?}"
    );
    assert_eq!(find(&q, "backlog-sendmeter")["title"], "backlog (1)");
}
