//! #343: the event log's folded checkpoint and its rotation out of the live log (#344: archived rows in `state.db`).
mod common;
use common::tmpdir;
use drover::events::{append, checkpoint, fold, history, read, rotate, states, Event, States};
use serde_json::{json, Value};
use std::path::Path;

/// A deterministic xorshift, so a failing case replays.
struct Rng(u64);
impl Rng {
    fn n(&mut self, k: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % k
    }
}

/// `n` events over 12 issues of two fleets: spawns, verdicts, merges, closes, kills, extensions, passive and queue rows.
fn log(seed: u64, n: u64, t0: u64) -> Vec<Event> {
    let mut r = Rng(seed);
    (0..n)
        .map(|i| {
            let (fleet, issue) = (["a", "b"][r.n(2) as usize], 1 + r.n(6));
            let role = ["impl", "rev"][r.n(2) as usize];
            let agent = format!("{fleet}-{role}-{issue}-{}", r.n(3));
            let sha = format!("{:040}", r.n(4));
            let (kind, d) = match r.n(11) {
                0..=2 => (
                    "spawn",
                    json!({"role": role, "agent": agent, "branch": format!("issue-{issue}")}),
                ),
                3..=4 => (
                    "verdict",
                    json!({"sha": sha, "verdict": (["PASS", "FAIL"][r.n(2) as usize])}),
                ),
                5 => (["merged", "closed", "needs-human"][r.n(3) as usize], json!({})),
                6 => ("clean", json!({"why": (["merged", "closed"][r.n(2) as usize])})),
                7 => ("lane-killed", json!({"role": role, "agent": agent})),
                8 => ("round-extended", json!({})),
                9 => ("stall", json!({"reason": "quiet"})),
                _ => ("queue", json!({"op": "add", "issues": [issue]})),
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

/// Bulk rows numbered on from the log's last, as an old JSONL log the next read imports (#344).
fn write(p: &Path, ev: &[Event]) {
    let n = read(p).unwrap().last().map_or(0, |e| e.seq);
    let rows: String = (ev.iter().zip(n + 1..))
        .map(|(e, seq)| serde_json::to_string(&Event { seq, ..e.clone() }).unwrap() + "\n")
        .collect();
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p).unwrap();
    std::io::Write::write_all(&mut f, rows.as_bytes()).unwrap();
}

#[test]
fn a_checkpoint_plus_its_tail_folds_like_the_whole_log() {
    for seed in 1..=40u64 {
        let d = tmpdir("cp");
        let p = d.join("events.jsonl");
        let ev = log(seed, 120, 1_000);
        let cut = (seed * 7 % 120) as usize;
        write(&p, &ev[..cut]);
        checkpoint(&p).unwrap();
        write(&p, &ev[cut..]);
        same(&states(&p).unwrap(), &fold(&read(&p).unwrap()));
        // A second checkpoint mid-way, then more rows: still the whole fold.
        checkpoint(&p).unwrap();
        write(&p, &log(seed + 99, 30, 200_000));
        same(&states(&p).unwrap(), &fold(&read(&p).unwrap()));
    }
}

#[test]
fn rotation_keeps_every_event_and_deleting_the_checkpoint_is_harmless() {
    for seed in 1..=10u64 {
        let d = tmpdir("rot");
        let p = d.join("events.jsonl");
        let now = 90 * 86_400;
        write(&p, &log(seed, 400, 0)); // 400 × 10 min: all older than 30 days
        let recent = (0..40).map(|i| Event::new(now - 86_400 + i, "a", Some(1), "stall", json!({})));
        write(&p, &recent.collect::<Vec<_>>());
        let before = read(&p).unwrap();
        let full = fold(&before);
        let size = drover::db::live_bytes(&p).unwrap();
        rotate(&p, size + 1, now).unwrap();
        assert_eq!(read(&p).unwrap().len(), before.len(), "under the limit nothing moves");
        rotate(&p, 1, now).unwrap();
        assert!(drover::db::live_bytes(&p).unwrap() < size, "the live log shrank");
        assert!(read(&p).unwrap().len() < before.len());
        let back = history(&p).unwrap();
        let rows = |v: &[Event]| {
            v.iter()
                .map(|e| serde_json::to_value(e).unwrap())
                .collect::<Vec<Value>>()
        };
        assert_eq!(
            rows(&back),
            rows(&before),
            "the archives and the live log hold every event once"
        );
        same(&states(&p).unwrap(), &full);
        // The live log alone still says what is open, queued and done.
        let live = fold(&read(&p).unwrap());
        for (k, s) in &full {
            let l = live.get(k).map(|s| (s.state.as_str(), s.done()));
            assert!(
                l.is_some_and(|l| l == (s.state.as_str(), s.done())) || !s.done() && l.is_none(),
                "{k:?}: {l:?} vs {}",
                s.state
            );
        }
        let queue = |v: &[Event]| v.iter().filter(|e| e.kind == "queue").count();
        assert_eq!(queue(&read(&p).unwrap()), queue(&before));
        // A new row continues the seq; deleting the checkpoint rebuilds the same states from the logs.
        append(&p, &Event::new(now, "a", Some(1), "stall", json!({}))).unwrap();
        assert_eq!(read(&p).unwrap().last().unwrap().seq, before.len() as u64 + 1);
        let with = states(&p).unwrap();
        drover::db::put(&p, "states", 0, &json!({})).unwrap(); // an unreadable checkpoint
        same(&states(&p).unwrap(), &with);
        checkpoint(&p).unwrap();
        same(&states(&p).unwrap(), &with);
    }
}

/// #560: `shared` (serve's read) is `read` at every point: rows appended, a rotation's archive, a new log.
#[test]
fn the_shared_read_follows_appends_and_rotations() {
    let d = tmpdir("shared");
    let p = d.join("events.jsonl");
    let now = 90 * 86_400;
    let seqs = |v: &[Event]| v.iter().map(|e| e.seq).collect::<Vec<_>>();
    let same = || assert_eq!(seqs(&drover::events::shared(&p).unwrap()), seqs(&read(&p).unwrap()));
    same(); // no database yet
    write(&p, &log(3, 400, 0));
    same();
    let first = drover::events::shared(&p).unwrap();
    append(&p, &Event::new(now - 10, "a", Some(1), "stall", json!({}))).unwrap();
    same();
    assert_eq!(drover::events::shared(&p).unwrap().len(), first.len() + 1);
    rotate(&p, 1, now).unwrap();
    assert!(read(&p).unwrap().len() < first.len(), "the rotation archived some");
    same();
    append(&p, &Event::new(now, "a", Some(1), "stall", json!({}))).unwrap();
    same();
}

/// #343 measure: `status` / `queue ls` fold a 50 MB log in under 20 ms from its checkpoint (release:
/// `cargo test --release --test compaction -- --ignored --nocapture`).
#[test]
#[ignore = "builds a 50 MB log; run in release"]
fn the_fold_of_a_50mb_log_takes_under_20ms() {
    let d = tmpdir("big");
    let p = d.join("events.jsonl");
    let pad = "x".repeat(400);
    let mut text = String::new();
    let mut seq = 0;
    while text.len() < 50 << 20 {
        for e in log(seq + 1, 1000, seq * 600) {
            seq += 1;
            let mut v = serde_json::to_value(Event { seq, ..e }).unwrap();
            v["pad"] = json!(pad);
            text += &(v.to_string() + "\n");
        }
    }
    std::fs::write(&p, text).unwrap();
    checkpoint(&p).unwrap();
    // #344: the rows after the checkpoint as live writers add them.
    log(7, 50, seq * 600).iter().for_each(|e| append(&p, e).unwrap());
    let t = std::time::Instant::now();
    let st = states(&p).unwrap();
    let ms = t.elapsed().as_secs_f64() * 1e3;
    eprintln!(
        "#343: fold of a {} MB log from its checkpoint: {ms:.2} ms",
        drover::db::live_bytes(&p).unwrap() >> 20
    );
    assert!(ms < 20.0, "{ms} ms");
    same(&st, &fold(&read(&p).unwrap()));
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
    assert!(drover::db::live_bytes(&p).unwrap() <= 32 << 10);
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

#[test]
fn rotation_of_a_young_log_archives_old_noise_and_keeps_the_folds() {
    let d = tmpdir("young");
    let p = d.join("events.jsonl");
    let (now, day) = (100 * 86_400, 86_400);
    let mut ev = log(5, 600, now - 15 * day);
    let noise = [
        "driven",
        "kick",
        "drive-pass",
        "lane-state",
        "conflict-predicted",
        "intent",
    ];
    for i in 0..2000u64 {
        let (fleet, issue, kind) = (["a", "b"][(i % 2) as usize], 1 + i % 6, noise[(i % 6) as usize]);
        let issue = (kind != "drive-pass").then_some(issue);
        let d = json!({"agent": format!("{fleet}-impl-{}", i % 3), "step": "start", "state": "gone", "files": ["x"]});
        ev.push(Event::new(now - 15 * day + i * 640, fleet, issue, kind, d));
    }
    for (i, n) in (1..=6u64).enumerate() {
        ev.push(Event::new(
            now - 14 * day,
            "a",
            Some(n),
            "needs-decision",
            json!({"id": format!("q{i}")}),
        ));
    }
    ev.push(Event::new(
        now - 13 * day,
        "a",
        None,
        "decision",
        json!({"id": "q0", "choice": "1"}),
    ));
    let huge = "y".repeat(1 << 20);
    ev.push(Event::new(
        now - 12 * day,
        "a",
        Some(2),
        "build-cloned",
        json!({"errors": [huge.clone()]}),
    ));
    ev.push(Event::new(
        now - 12 * day,
        "a",
        Some(3),
        "verdict",
        json!({"sha": "f".repeat(40), "verdict": "FAIL", "body": huge}),
    ));
    write(&p, &ev);
    let before = read(&p).unwrap();
    rotate(&p, 1, now).unwrap();
    let after = read(&p).unwrap();
    same(&fold(&after), &fold(&before));
    same(&states(&p).unwrap(), &fold(&before));
    for f in ["a", "b"] {
        let q = |v: &[Event]| drover::queue::replay(v, f);
        assert_eq!(q(&after), q(&before), "queue {f}");
    }
    let asks = |v: &[Event]| drover::events::open_asks(v).iter().map(|a| a.seq).collect::<Vec<_>>();
    assert_eq!(asks(&after), asks(&before));
    assert_eq!(asks(&before).len(), 5);
    let old_noise = |v: &[Event]| {
        v.iter()
            .filter(|e| noise.contains(&e.kind.as_str()) && e.ts + 2 * day < now)
            .count()
    };
    assert!(
        old_noise(&after) * 10 < old_noise(&before),
        "{} of {}",
        old_noise(&after),
        old_noise(&before)
    );
    let newest = |v: &[Event], k: &str, f: &str, n: Option<u64>| {
        v.iter()
            .rfind(|e| e.kind == k && e.fleet == f && e.issue == n)
            .map(|e| e.seq)
    };
    for (k, f, n) in noise
        .iter()
        .flat_map(|k| ["a", "b"].map(|f| (k, f)))
        .flat_map(|(k, f)| (0..=6).map(move |n| (k, f, (n > 0).then_some(n))))
    {
        assert_eq!(
            newest(&after, k, f, n),
            newest(&before, k, f, n),
            "newest {k} {f} {n:?}"
        );
    }
    assert!(
        after
            .iter()
            .all(|e| serde_json::to_string(e).unwrap().len() <= 16 << 10),
        "no oversize live row"
    );
    assert!(
        !after.iter().any(|e| e.kind == "build-cloned"),
        "the oversize passive row is archived"
    );
    let all = history(&p).unwrap();
    assert_eq!(all.len(), before.len(), "nothing deleted");
    let cloned = all.iter().find(|e| e.kind == "build-cloned").unwrap();
    assert_eq!(
        cloned.data["errors"][0].as_str().unwrap().len(),
        1 << 20,
        "history keeps it whole"
    );
    let v = after
        .iter()
        .find(|e| e.kind == "verdict" && e.ts == now - 12 * day)
        .unwrap();
    assert!(
        v.str("body").unwrap().contains("bytes cut"),
        "a row a fold reads is rewritten capped"
    );
    rotate(&p, 1, now).unwrap();
    assert_eq!(
        read(&p).unwrap().len(),
        after.len(),
        "a second rotation does what the first did"
    );
}
