mod common;
use common::*;
use drover::serve::Server;
use serde_json::{json, Value};
use std::collections::BTreeSet;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;
const ACTS: [&str; 7] = ["impl", "fix", "rev", "update", "idle", "done", "blocked"];

fn get(s: &Server, p: &str) -> Value {
    let r = format!("GET {p} HTTP/1.1\r\nHost: {ADDR}\r\n\r\n");
    let res = String::from_utf8_lossy(&s.respond(&r, T)).into_owned();
    let (head, body) = res.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("HTTP/1.1 200"), "{p}: {res}");
    serde_json::from_str(body).unwrap()
}

fn figures(v: &Value) -> Vec<(String, Value)> {
    fn go(v: &Value, out: &mut Vec<(String, Value)>) {
        match v {
            Value::Object(m) => {
                if let Some(f) = m.get("figure") {
                    out.push((m["id"].as_str().unwrap_or_default().to_string(), f.clone()));
                }
                m.values().for_each(|x| go(x, out));
            }
            Value::Array(a) => a.iter().for_each(|x| go(x, out)),
            _ => {}
        }
    }
    let mut out = vec![];
    go(v, &mut out);
    out
}

#[test]
fn parity_60_every_row_figure_carries_the_fields_its_art_needs_and_the_board_reads_no_old_route() {
    let (c, r) = recorded("cooee-art");
    r.1.lane("sendmeter-impl-5", "blocked", "/x");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let get = |p: &str| get(&s, p);
    let acts = |f: &Value| ACTS.contains(&f["act"].as_str().unwrap_or_default());
    let q = figures(&get("/screens/queue"));
    assert!(q.len() >= 3, "{q:?}");
    for (id, f) in &q {
        assert!(f["state"].is_string() && acts(f), "{id}: {f}");
        assert!(f["grow"].is_string() && f["issue"].is_u64(), "{id}: {f}");
    }
    let fix = &q.iter().find(|(id, _)| id == "q-sendmeter-12").unwrap().1;
    assert_eq!((&fix["act"], &fix["grow"]), (&json!("fix"), &json!("fix")));
    let e = figures(&get("/screens/events"));
    assert!(e.iter().all(|(_, f)| f["state"].is_string() && acts(f)), "{e:?}");
    let moments: BTreeSet<&str> = e.iter().filter_map(|(_, f)| f["moment"].as_str()).collect();
    assert_eq!(moments, BTreeSet::from(["fail", "merged", "needs-you"]), "{e:?}");
    let l = figures(&get("/screens/lanes"));
    let lane = |n: &str| {
        &l.iter()
            .find(|(id, _)| id == n)
            .unwrap_or_else(|| panic!("{n}: {l:?}"))
            .1
    };
    assert_eq!(
        (
            &lane("l-sendmeter-impl-12")["act"],
            &lane("l-sendmeter-impl-12")["state"]
        ),
        (&json!("fix"), &json!("working"))
    );
    assert_eq!(lane("l-sendmeter-impl-5")["act"], "blocked");
    assert_eq!(
        (
            &lane("l-sendmeter-impl-12")["grow"],
            &lane("l-sendmeter-impl-12")["issue"]
        ),
        (&json!("fix"), &json!(12))
    );
    assert!(l.iter().all(|(_, f)| f["coat"].is_string() && acts(f)), "{l:?}");
    let h = get("/screens/hosts");
    let main = &h["blocks"][0]["blocks"][0]["rows"][0];
    assert!(
        ["calm", "busy", "held"].contains(&main["figure"]["load"].as_str().unwrap_or_default()),
        "{main}"
    );
    let board = include_str!("../../../web/dist/board.js");
    for old in ["live?tab", "herd.json", "lane?agent", "lane?setup", "/live"] {
        assert!(!board.contains(old), "the board reads the old route {old}");
    }
    for want in [
        "ascii_rows",
        "ascii_load",
        "ascii_loading",
        "load_scene",
        "moments",
        "stages",
        "species",
    ] {
        assert!(board.contains(want), "the board draws no {want}");
    }
}
