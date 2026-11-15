mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::{json, Value};
use std::sync::atomic::Ordering::Relaxed;

#[test]
fn board_routes_read_the_working_set_and_a_merged_issues_shot_still_serves() {
    let d = tmpdir("serve-working-set");
    let c = ctx(&d, CONFIG);
    let lane = d.join("lane");
    std::fs::create_dir_all(lane.join("shots")).unwrap();
    std::fs::write(lane.join("shots/home-1440.png"), b"\x89PNG wide").unwrap();
    let ev = |n: Option<u64>, kind: &str, d| Event::new(1, "sendmeter", n, kind, d);
    let spawn = json!({"role": "verify", "stage": "verify", "agent": "verify-1", "worktree": lane});
    append(&c.state, &ev(Some(12), "spawn", spawn)).unwrap();
    let shot = json!([{"name": "home-1440.png", "path": lane.join("shots/home-1440.png")}]);
    let verdict = json!({"verdict": "PASS", "sha": "b".repeat(40), "stage": "verify", "files": shot});
    append(&c.state, &ev(Some(12), "verdict", verdict)).unwrap();
    for _ in 0..2_000 {
        append(&c.state, &ev(Some(12), "labelled", json!({}))).unwrap();
    }
    append(&c.state, &ev(Some(12), "merged", json!({}))).unwrap();
    for _ in 0..10_001 {
        append(&c.state, &ev(None, "drive-pass", json!({}))).unwrap();
    }
    let total = drover::db::last_seq(&c.state);
    let r = Fake::default();
    let s = Server::new(&c, &r, Some("127.0.0.1:7777".into()));
    let get = |p: &str| {
        let r0 = drover::db::ROWS.load(Relaxed);
        let r = s.respond(&format!("GET {p} HTTP/1.1\r\nHost: 127.0.0.1:7777\r\n\r\n"), 2);
        (
            String::from_utf8_lossy(&r).into_owned(),
            drover::db::ROWS.load(Relaxed) - r0,
        )
    };

    let (r, rows) = get("/thread/t1/a.png");
    assert!(r.starts_with("HTTP/1.1 404"), "{r}");
    assert!(
        rows + 2_000 <= usize::try_from(total).unwrap(),
        "the quiet merged issue's history stays unread: {rows} of {total} rows"
    );

    let (r, rows) = get("/api/v1/needs");
    assert!(r.starts_with("HTTP/1.1 200"), "{r}");
    assert!(
        rows + 2_000 <= usize::try_from(total).unwrap(),
        "the needs tab and its pending cards leave the full log unread: {rows} of {total} rows"
    );

    let (r, rows) = get("/shot/sendmeter/12/0");
    assert!(r.starts_with("HTTP/1.1 200") && r.ends_with("PNG wide"), "{r}");
    assert!(rows <= 2, "only spawns and verdicts: {rows} rows");

    let body = |r: &str| serde_json::from_str::<Value>(&r[r.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    let (r, _) = get("/api/v1/issue/sendmeter/12");
    let v = body(&r);
    assert_eq!(v["verdicts"].as_array().map(Vec::len), Some(1), "{v}");
    assert_eq!(v["verdicts"][0]["shots"], json!(["home-1440.png"]));

    let (r, rows) = get("/api/v1/events?since=1&until=3");
    let v = body(&r);
    let seqs: Vec<_> = v["list"].as_array().unwrap().iter().map(|e| e["seq"].clone()).collect();
    assert_eq!((seqs, &v["refresh"]), (vec![json!(2), json!(3)], &json!(false)));
    assert_eq!(v["seq"], json!(total));
    assert!(rows <= 2, "only the asked range: {rows} rows");
}
