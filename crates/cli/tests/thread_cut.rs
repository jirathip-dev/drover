mod common;
use common::*;
use drover::events::{read, Event};
use drover::serve::Server;
use drover::thread::{RowKind, Target};
use serde_json::{json, Value};

const T: u64 = 1_000_000;
const TOKEN: &str = "tok-thread-cut";

const LOG: &[&str] = &[
    r#"{"seq":1,"ts":999000,"fleet":"sendmeter","type":"thread-new","thread":"t1","title":"gate flakes"}"#,
    r#"{"seq":2,"ts":999001,"fleet":"sendmeter","type":"orchestrator-start","thread":"t1","agent":"orchestrator-t1"}"#,
    r#"{"seq":3,"ts":999002,"fleet":"sendmeter","issue":12,"type":"issue-filed","filed_in":"t1"}"#,
    r#"{"seq":4,"ts":999003,"fleet":"sendmeter","type":"thread-msg","thread":"t1","from":"owner","text":"fix the flake"}"#,
    r#"{"seq":5,"ts":999004,"fleet":"sendmeter","type":"thread-new","thread":"t2","title":"facets"}"#,
    r#"{"seq":6,"ts":999005,"fleet":"sendmeter","type":"thread-archive","thread":"t2"}"#,
];

fn board(name: &str) -> drover::Ctx {
    let c = ctx(&tmpdir(name), CONFIG);
    rewrite_log(&c.state, &(LOG.join("\n") + "\n"));
    std::fs::write(c.config.with_file_name("cooee-token"), TOKEN).unwrap();
    c
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.kind == kind)
        .cloned()
        .collect()
}

fn get(s: &Server, path: &str) -> Value {
    let r = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:4810\r\nAuthorization: Bearer {TOKEN}\r\n\r\n");
    let res = String::from_utf8_lossy(&s.respond(&r, T)).into_owned();
    serde_json::from_str(res.split_once("\r\n\r\n").unwrap().1).unwrap()
}

#[test]
fn check_gives_each_open_thread_its_closing_row_ends_its_orchestrator_and_archives_it_once() {
    let c = board("thread-cut-retire");
    let b = FakeShed::new();
    b.attach(&c);
    b.lane("orchestrator-t1", "idle", "/x");
    let f = Fake::default();
    let r = b.on(&f);
    assert_eq!(run(&c, &r, &["check"]).0, 0);
    assert_eq!(run(&c, &r, &["check"]).0, 0);
    let closing: Vec<_> = kinds(&c, "thread-msg")
        .into_iter()
        .filter(|e| e.val("closing") == true)
        .collect();
    assert_eq!(closing.len(), 1, "one closing row, for the open thread only");
    let text = closing[0].str("text").unwrap_or_default();
    assert!(
        closing[0].str("thread") == Some("t1") && text.contains("attach t1"),
        "{text}"
    );
    let archived: Vec<_> = kinds(&c, "thread-archive")
        .into_iter()
        .filter(|e| e.str("by") == Some("migration"))
        .collect();
    assert_eq!(archived.len(), 1);
    assert_eq!(archived[0].str("thread"), Some("t1"));
    let slept = kinds(&c, "orchestrator-sleep");
    assert_eq!((slept.len(), slept[0].str("why")), (1, Some("archived")), "{slept:?}");
    let events = read(&c.state).unwrap();
    let rows = drover::thread::rows(&events);
    let row = rows.iter().find(|(_, r)| r.seq == closing[0].seq).unwrap();
    assert_eq!(
        (&row.1.to, row.1.kind),
        (&Target::Thread { id: "t1".into() }, RowKind::System)
    );
}

#[test]
fn attach_adopts_an_archived_thread_and_its_filed_issues_route_to_that_chat() {
    let c = board("thread-cut-adopt");
    let f = Fake::default();
    let (code, out) = run(&c, &f, &["attach", "sendmeter", "t1"]);
    assert_ne!(code, 0, "an open drover thread is not attached: {out}");
    assert!(out.contains("open drover thread"), "{out}");
    let b = FakeShed::new();
    b.attach(&c);
    assert_eq!(run(&c, &b.on(&f), &["check"]).0, 0);
    let (code, out) = run(&c, &f, &["attach", "sendmeter", "t1"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("DROVER_THREAD=t1"), "{out}");
    let events = read(&c.state).unwrap();
    assert_eq!(
        drover::thread::route::home(&events, "sendmeter", Some(12)),
        Target::Thread { id: "t1".into() }
    );
    let merged = Event::new(T, "sendmeter", Some(12), "merged", json!({"pr": 40}));
    drover::events::append(&c.state, &merged).unwrap();
    let events = read(&c.state).unwrap();
    let rows = drover::thread::rows(&events);
    let row = rows.iter().rfind(|(_, r)| r.kind == RowKind::Merged).unwrap();
    assert_eq!(row.1.to, Target::Thread { id: "t1".into() });
}

#[test]
fn an_archived_threads_history_is_still_served_by_its_rows_route() {
    let c = board("thread-cut-rows");
    let b = FakeShed::new();
    b.attach(&c);
    let f = Fake::default();
    assert_eq!(run(&c, &b.on(&f), &["check"]).0, 0);
    let s = Server::new(&c, &f, Some("127.0.0.1:7777".into()));
    let v = get(&s, "/api/v1/thread/t1/rows?n=50");
    let texts: Vec<&str> = v["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["text"].as_str())
        .collect();
    assert!(
        texts.contains(&"fix the flake") && texts.iter().any(|t| t.contains("attach t1")),
        "{v}"
    );
}

#[test]
fn a_handoff_to_a_repo_is_an_open_desk_row_with_no_thread_and_move_sends_it_to_a_chat() {
    let c = board("thread-cut-handoff");
    let f = Fake::default();
    for id in ["chat-1", "chat-2"] {
        assert_eq!(run(&c, &f, &["attach", "sendmeter", id]).0, 0);
    }
    let (code, out) = run(
        &c,
        &f,
        &["thread", "handoff", "chat-1", "sendmeter", "Facets", "add facets"],
    );
    assert_eq!((code, out.trim()), (0, "sendmeter"));
    assert!(kinds(&c, "thread-new").len() == 2, "no thread is made");
    let events = read(&c.state).unwrap();
    let open = drover::thread::open(&events);
    let h = open.iter().find(|(_, r)| r.kind == RowKind::Handoff).unwrap();
    assert_eq!(
        h.1.to,
        Target::Desk {
            repo: "sendmeter".into()
        }
    );
    let seq = h.1.seq;
    let cfg = drover::config::Config::load(&c.config).unwrap();
    assert_eq!(
        drover::orchestrator::move_handoff(&c, &cfg, &f, seq, "chat-2").unwrap(),
        "chat-2"
    );
    let events = read(&c.state).unwrap();
    assert!(!drover::thread::open(&events)
        .iter()
        .any(|(_, r)| r.kind == RowKind::Handoff && r.seq == seq));
    let rows = drover::thread::rows(&events);
    let moved = rows.iter().rfind(|(_, r)| r.kind == RowKind::Handoff).unwrap();
    assert_eq!(moved.1.to, Target::Thread { id: "chat-2".into() });
    assert_eq!(moved.1.prompt.as_deref(), Some("[chat-2] chat-1: add facets"));
    let (code, out) = run(&c, &f, &["thread", "new", "sendmeter", "x"]);
    assert_ne!(code, 0, "{out}");
}
