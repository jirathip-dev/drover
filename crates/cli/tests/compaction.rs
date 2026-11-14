mod common;
use common::{ctx, run, tmpdir, Fake};
use drover::events::{append, closed_row, fold, history, read, states, Event, States};
use serde_json::{json, Value};
use std::path::Path;

struct Rng(u64);
impl Rng {
    fn n(&mut self, k: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % k
    }
}

fn log(seed: u64, n: u64, t0: u64) -> Vec<Event> {
    let mut r = Rng(seed);
    (0..n)
        .map(|i| {
            let (fleet, issue) = (["a", "b"][r.n(2) as usize], 1 + r.n(6));
            let role = ["impl", "rev"][r.n(2) as usize];
            let agent = format!("{fleet}-{role}-{issue}-{}", r.n(3));
            let sha = format!("{:040}", r.n(4));
            let (kind, d) = match r.n(13) {
                0..=2 => (
                    "spawn",
                    json!({"role": role, "agent": agent, "branch": format!("issue-{issue}")}),
                ),
                3..=4 => (
                    "verdict",
                    json!({"sha": sha, "verdict": (["PASS", "FAIL"][r.n(2) as usize])}),
                ),
                5 => (["merged", "closed", "needs-human"][r.n(3) as usize], json!({})),
                11 => ("needs-decision", json!({"id": format!("q{}", r.n(5)), "agent": agent})),
                6 => ("clean", json!({"why": (["merged", "closed"][r.n(2) as usize])})),
                7 => ("lane-killed", json!({"role": role, "agent": agent})),
                8 => ("round-extended", json!({})),
                9 => ("stall", json!({"reason": "quiet"})),
                10 => ("queue", json!({"op": "add", "issues": [issue]})),
                _ => (
                    ["decision", "ask-withdrawn"][r.n(2) as usize],
                    json!({"id": format!("q{}", r.n(5))}),
                ),
            };
            let issue = (kind != "queue").then_some(issue);
            Event::new(t0 + i * 600, fleet, issue, kind, d)
        })
        .collect()
}

fn same(a: &States, b: &States) {
    assert_eq!(
        serde_json::to_value(a.values().collect::<Vec<_>>()).unwrap(),
        serde_json::to_value(b.values().collect::<Vec<_>>()).unwrap()
    );
}

fn write(p: &Path, ev: &[Event]) {
    let n = read(p).unwrap().last().map_or(0, |e| e.seq);
    let rows: String = (ev.iter().zip(n + 1..))
        .map(|(e, seq)| serde_json::to_string(&Event { seq, ..e.clone() }).unwrap() + "\n")
        .collect();
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p).unwrap();
    std::io::Write::write_all(&mut f, rows.as_bytes()).unwrap();
}

const DAY: u64 = 86_400;

fn tables(p: &Path) -> Vec<String> {
    let c = rusqlite::Connection::open(drover::db::file(p)).unwrap();
    let mut out = vec![];
    for t in ["states", "lanes", "asks"] {
        let mut q = c.prepare(&format!("SELECT * FROM {t} ORDER BY 1, 2")).unwrap();
        let n = q.column_count();
        let rows = q.query_map([], |r| {
            (0..n)
                .map(|i| r.get::<_, rusqlite::types::Value>(i))
                .collect::<Result<Vec<_>, _>>()
        });
        out.extend(rows.unwrap().map(|r| format!("{t} {:?}", r.unwrap())));
    }
    out
}

#[test]
fn the_tables_match_the_events_and_a_rebuild_gives_identical_rows() {
    for seed in 1..=20u64 {
        let d = tmpdir("tables");
        let mut c = ctx(&d, "");
        let p = c.state.clone();
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let ev = log(seed, 300, 1_000);
        let cut = (seed * 13 % 300) as usize;
        write(&p, &ev[..cut]);
        ev[cut..].iter().for_each(|e| append(&p, e).unwrap());
        same(&states(&p).unwrap(), &fold(&history(&p).unwrap()));
        let live = read(&p).unwrap();
        let copy = live.to_vec();
        let asks = |v: &[Event]| v.iter().filter(|e| e.kind == "needs-decision").count();
        let ends = |v: &[Event]| {
            (0..asks(v))
                .map(|k| {
                    let a = v.iter().filter(|e| e.kind == "needs-decision").nth(k).unwrap();
                    closed_row(v, a)
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(ends(&live), ends(&copy), "seed {seed}");
        assert!(ends(&copy).iter().any(Option::is_some), "seed {seed}: some ask closed");
        let rows = tables(&p);
        assert!(rows.iter().any(|r| r.starts_with("asks ")) && rows.iter().any(|r| r.starts_with("lanes ")));
        let (code, out) = run(&c, &Fake::default(), &["compact", "--rebuild"]);
        assert_eq!(code, 0, "{out}");
        assert_eq!(tables(&p), rows, "seed {seed}: rebuild");
        c.now = 1_000 + 300 * 600 + 30 * DAY;
        let (code, out) = run(&c, &Fake::default(), &["compact"]);
        assert_eq!(code, 0, "{out}");
        assert!(
            read(&p).unwrap().len() < live.len(),
            "seed {seed}: some events archived: {out}"
        );
        let (code, out) = run(&c, &Fake::default(), &["compact", "--rebuild"]);
        assert_eq!(code, 0, "{out}");
        assert_eq!(tables(&p), rows, "seed {seed}: rebuild from the archive");
    }
}

#[test]
fn compaction_archives_old_done_issues_and_deletes_old_heartbeats() {
    let d = tmpdir("compact");
    let mut c = ctx(&d, "");
    c.now = 100 * DAY;
    let p = c.state.clone();
    let spawn = |n: u64| json!({"role": "impl", "agent": format!("a-impl-{n}"), "branch": format!("issue-{n}")});
    let ev = [
        Event::new(5 * DAY, "a", None, "queue", json!({"op": "add", "issues": [1, 2, 3]})),
        Event::new(10 * DAY, "a", None, "drive-pass", json!({})),
        Event::new(10 * DAY, "a", Some(3), "spawn", spawn(3)),
        Event::new(40 * DAY, "a", Some(1), "spawn", spawn(1)),
        Event::new(40 * DAY, "a", Some(2), "spawn", spawn(2)),
        Event::new(
            45 * DAY,
            "a",
            Some(1),
            "needs-decision",
            json!({"id": "q1", "agent": "a-impl-1"}),
        ),
        Event::new(46 * DAY, "a", Some(1), "decision", json!({"id": "q1", "choice": "1"})),
        Event::new(50 * DAY, "a", Some(1), "merged", json!({})),
        Event::new(95 * DAY, "a", Some(2), "merged", json!({})),
        Event::new(97 * DAY, "a", Some(1), "kick", json!({})),
        Event::new(99 * DAY, "a", Some(3), "driven", json!({})),
    ];
    ev.iter().for_each(|e| append(&p, e).unwrap());
    let (before, st) = (history(&p).unwrap(), states(&p).unwrap());
    let (code, out) = run(&c, &Fake::default(), &["compact"]);
    assert_eq!((code, out.trim()), (0, "2 heartbeats deleted, 5 events archived"));
    let kinds = |v: &[Event]| v.iter().map(|e| format!("{}{:?}", e.kind, e.issue)).collect::<Vec<_>>();
    let hot = kinds(&read(&p).unwrap());
    assert_eq!(hot, ["queueNone", "spawnSome(3)", "mergedSome(2)", "drivenSome(3)"]);
    let gone = |e: &&Event| !matches!(e.kind.as_str(), "drive-pass" | "kick");
    let rows = |v: &[Event]| {
        v.iter()
            .map(|e| serde_json::to_value(e).unwrap())
            .collect::<Vec<Value>>()
    };
    let kept: Vec<Event> = before.iter().filter(gone).cloned().collect();
    assert_eq!(
        rows(&history(&p).unwrap()),
        rows(&kept),
        "history: the archive and the hot events, once each"
    );
    assert!(d.join("state/archive/events-1970-02.jsonl.zst").is_file());
    same(&states(&p).unwrap(), &st);
    let (_, asks) = run(&c, &Fake::default(), &["asks", "--all", "--json"]);
    let asks: Value = serde_json::from_str(&asks).unwrap();
    assert_eq!(
        (asks[0]["id"].as_str(), asks[0]["closed"].as_str()),
        (Some("q1"), Some("answered"))
    );
    let wal = std::fs::metadata(d.join("state/state.db-wal")).map_or(0, |m| m.len());
    assert_eq!(wal, 0, "the WAL truncated");
    let (_, out) = run(&c, &Fake::default(), &["compact"]);
    assert_eq!(out.trim(), "0 heartbeats deleted, 0 events archived");
    assert_eq!(rows(&history(&p).unwrap()), rows(&kept));
}

#[test]
fn the_shared_read_follows_appends_and_compaction() {
    let d = tmpdir("shared");
    let p = d.join("events.jsonl");
    let now = 90 * DAY;
    let seqs = |v: &[Event]| v.iter().map(|e| e.seq).collect::<Vec<_>>();
    let same = || assert_eq!(seqs(&drover::events::shared(&p).unwrap()), seqs(&read(&p).unwrap()));
    same();
    write(&p, &log(3, 400, 0));
    same();
    let first = drover::events::shared(&p).unwrap();
    append(&p, &Event::new(now - 10, "a", Some(1), "stall", json!({}))).unwrap();
    same();
    assert_eq!(drover::events::shared(&p).unwrap().len(), first.len() + 1);
    drover::db::compact(&p, now).unwrap();
    assert!(read(&p).unwrap().len() < first.len(), "the compaction archived some");
    same();
    assert_eq!(
        fold(&drover::events::shared(&p).unwrap()).len(),
        states(&p).unwrap().len()
    );
    append(&p, &Event::new(now, "a", Some(1), "stall", json!({}))).unwrap();
    same();
}

#[test]
fn build_cloned_rows_from_before_978_move_their_errors_to_cache() {
    let d = tmpdir("offload");
    let p = d.join("events.jsonl");
    let err = format!("cp: {}", "z".repeat(4000));
    let spawn = json!({"role": "impl", "agent": "a-impl-1", "branch": "issue-1"});
    append(&p, &Event::new(1_000, "a", Some(1), "spawn", spawn)).unwrap();
    let cl = json!({"dirs": [], "secs": 3, "errors": [err.clone()]});
    append(&p, &Event::new(1_001, "a", Some(1), "build-cloned", cl)).unwrap();
    let st = states(&p).unwrap();
    std::thread::spawn({
        let db = d.join("state.db");
        move || {
            let c = rusqlite::Connection::open(db).unwrap();
            c.execute_batch("DELETE FROM states; DELETE FROM lanes; DELETE FROM asks; PRAGMA user_version = 0;")
                .unwrap();
        }
    })
    .join()
    .unwrap();
    let back = std::thread::spawn(move || (read(&p).unwrap().to_vec(), states(&p).unwrap()));
    let (back, st2) = back.join().unwrap();
    same(&st2, &st);
    let rel = back[1].str("errors_ref").expect("a reference").to_string();
    assert!(
        back[1].data.get("errors").is_none() && rel.starts_with("cache/build-cloned/"),
        "{:?}",
        back[1]
    );
    assert_eq!(std::fs::read_to_string(d.join(rel)).unwrap(), err);
    assert!(serde_json::to_string(&back[1]).unwrap().len() < 300);
}

#[test]
fn a_5mb_build_cloned_is_stored_in_16kb() {
    let d = tmpdir("cap");
    let p = d.join("events.jsonl");
    let spawn = json!({"role": "impl", "agent": "a-impl-1", "branch": "issue-1"});
    let big = format!("cp: {}", "x".repeat(5 << 20));
    let ev = [
        Event::new(1_000, "a", Some(1), "spawn", spawn),
        Event::new(
            1_001,
            "a",
            Some(1),
            "build-cloned",
            json!({"dirs": ["target"], "secs": 7, "errors": [big]}),
        ),
    ];
    ev.iter().for_each(|e| append(&p, e).unwrap());
    let back = read(&p).unwrap();
    let stored = serde_json::to_string(&Event {
        seq: 0,
        ..back[1].clone()
    })
    .unwrap();
    assert!(stored.len() <= 16 << 10, "{} bytes", stored.len());
    let err = back[1].data["errors"][0].as_str().unwrap();
    assert!(
        err.starts_with("cp: xxx") && err.contains("bytes cut"),
        "{}",
        &err[..80]
    );
    assert_eq!(
        (back[1].data["dirs"].clone(), back[1].data["secs"].clone()),
        (json!(["target"]), json!(7))
    );
    assert_eq!(serde_json::to_value(&back[0]).unwrap()["agent"], "a-impl-1");
    same(&fold(&back), &fold(&ev));
}
