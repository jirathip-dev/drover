mod common;
use common::*;
use drover::events::{append, history, Event};
use drover::reroot::Shas;
use serde_json::json;

#[test]
fn drover_compact_runs_the_compact_binary_for_the_fleet_and_logs_its_records_once() {
    let dir = tmpdir("compact-glue");
    let local = dir.join("local");
    let c = ctx(
        &dir,
        &CONFIG.replace("/tmp/does-not-matter/sendmeter", local.to_str().unwrap()),
    );
    let ev = |n: u64, kind: &str, d: serde_json::Value| {
        append(&c.state, &Event::new(10, "sendmeter", Some(n), kind, d)).unwrap();
    };
    ev(
        1,
        "merged",
        json!({"pr": "11", "merge_sha": "m1", "verdict_sha": "l1", "into": "staging"}),
    );
    ev(2, "verdict", json!({"verdict": "PASS", "sha": "o2"}));
    let d = dir.join("state/compact/jirathip-dev/sendmeter");
    let saved = d.join("saved.tsv");
    let fake = Fake::default().reply("compact ", "run card 7 posted: compact --push new1\n");
    let (code, out) = run(
        &c,
        &fake,
        &["compact", "sendmeter", "--cut", "2026-11-01", "--thread", "t"],
    );
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("run card 7 posted"), "{out}");
    let want = format!(
        "compact --repo jirathip-dev/sendmeter --line staging --clone {} --fleet sendmeter --saved {} --dir {} --cut 2026-11-01 --thread t",
        local.display(),
        saved.display(),
        d.display()
    );
    assert!(fake.called(&want), "{:?}", fake.calls.borrow());
    assert_eq!(
        std::fs::read_to_string(&saved).unwrap(),
        "sendmeter\t1\tm1\nsendmeter\t1\tl1\nsendmeter\t2\to2\n"
    );

    let plan = json!({"new_tip": "new1", "repo": "jirathip-dev/sendmeter", "line": "staging", "cut": "cut1",
        "root": "root1", "old_tip": "old1", "archive": "jirathip-dev/sendmeter-history", "commits": [9, 3], "refs": []});
    std::fs::write(d.join("plan.json"), plan.to_string()).unwrap();
    let moved =
        json!([{"fleet": "sendmeter", "issue": 2, "run": "new1", "moved": [{"from": "o2", "to": "n2", "held": []}]}]);
    std::fs::write(d.join("moved.json"), moved.to_string()).unwrap();
    for _ in 0..2 {
        let (code, out) = run(&c, &fake, &["compact", "sendmeter", "--reset"]);
        assert_eq!(code, 0, "{out}");
    }
    let events = history(&c.state).unwrap();
    let logged: Vec<Option<u64>> = (events.iter().filter(|e| e.kind == "compacted"))
        .map(|e| e.issue)
        .collect();
    assert_eq!(logged, [None, Some(2)], "logged once, however often it runs");
    let shas = Shas::of(&events);
    assert_eq!(shas.now("o2"), "n2");
    assert_eq!(
        shas.last("jirathip-dev/sendmeter").map(|r| r.root.as_str()),
        Some("root1")
    );

    std::fs::write(d.join("undone"), "new1\n").unwrap();
    let (code, out) = run(&c, &fake, &["compact", "sendmeter", "--reset"]);
    assert_eq!(code, 0, "{out}");
    let shas = Shas::of(&history(&c.state).unwrap());
    assert_eq!(shas.now("o2"), "o2");
    assert!(shas.last("jirathip-dev/sendmeter").is_none(), "the undo ends the run");
}

#[test]
fn a_compact_refusal_is_drovers_refusal() {
    let dir = tmpdir("compact-refused");
    let c = ctx(&dir, CONFIG);
    let fake = Fake::default().once_err(
        "compact ",
        3,
        "REFUSED COMPACT_UNAPPROVED: `compact --push new1` runs only inside its approved run card\n",
    );
    let (code, out) = run(&c, &fake, &["compact", "sendmeter", "--push", "new1"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED COMPACT_UNAPPROVED"), "{out}");
    assert!(dir.join("state/compact/jirathip-dev/sendmeter/saved.tsv").exists());
}

fn unlisted(src: &str) -> Vec<String> {
    let listed: Vec<&str> = (drover::reroot::SAVED.iter().map(|s| s.1))
        .chain(drover::reroot::KEPT.iter().map(|k| k.0))
        .collect();
    let mut out: Vec<String> = (src.match_indices("\": "))
        .filter_map(|(i, _)| Some(src[..i].rsplit_once('"')?.1))
        .filter(|key| *key == "sha" || key.ends_with("_sha"))
        .filter(|key| key.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))
        .map(String::from)
        .filter(|key| !listed.contains(&key.as_str()))
        .collect();
    out.sort();
    out.dedup();
    out
}

fn sources() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut todo: Vec<_> = ["core/src", "server/src", "cli/src"].map(|d| root.join(d)).into();
    let mut all = String::new();
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                todo.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                all += &std::fs::read_to_string(&p).unwrap();
            }
        }
    }
    all
}

#[test]
fn every_saved_sha_field_is_in_compacts_list() {
    assert_eq!(
        unlisted(&sources()),
        Vec::<String>::new(),
        "a saved commit sha compact does not remap: add it to reroot::SAVED, or to KEPT with why"
    );
}

#[test]
fn a_planted_saved_sha_field_fails_the_list_check() {
    let planted = sources() + "\nlet d = json!({\"pr\": num, \"base_sha\": base});\n";
    assert_eq!(unlisted(&planted), ["base_sha"]);
}
