mod common;
use common::*;
use drover::serve::Server;
use drover::Ctx;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const SEED: &str = "demo";
const SHEETS: [&str; 8] = [
    "/screens/workflow",
    "/screens/issue?repo=acme-api&issue=21",
    "/screens/hot?repo=acme-api&issue=21",
    "/screens/verdict?repo=acme-web&issue=36&seq=4",
    "/screens/ask?seq=33",
    "/screens/backlog?repo=acme-web",
    "/screens/queue/done?repo=acme-web",
    "/screens/drive-health",
];

fn demo_ctx(d: &Path) -> Ctx {
    let demo = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo"));
    let mut dirs = vec![demo.clone()];
    while let Some(dir) = dirs.pop() {
        std::fs::create_dir_all(d.join(dir.strip_prefix(&demo).unwrap())).unwrap();
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                dirs.push(p);
            } else {
                std::fs::copy(&p, d.join(p.strip_prefix(&demo).unwrap())).unwrap();
            }
        }
    }
    Ctx {
        config: d.join("fleet.toml"),
        state: d.join("events.jsonl"),
        worktrees: d.join("wt"),
        exe: "drover".into(),
        now: 1,
        home: d.join("home"),
        hints: vec![],
    }
}

fn name(path: &str) -> String {
    let p = path.trim_start_matches("/screens/").replace(['/', '?', '&', '='], "-");
    format!("{p}.{SEED}.json")
}

fn built() -> BTreeMap<String, String> {
    let d = tmpdir("screen-fixtures");
    let c = demo_ctx(&d);
    let r = Fake::default();
    drover::serve::export(&c, &r, &d.join("site"), true).unwrap();
    let mut out = BTreeMap::new();
    let mut dirs = vec![d.join("site/screens")];
    while let Some(dir) = dirs.pop() {
        for e in std::fs::read_dir(&dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.file_name().is_some_and(|f| f == "index.html") {
                let rel = p.parent().unwrap().strip_prefix(d.join("site")).unwrap();
                let v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
                out.insert(format!("/{}", rel.to_string_lossy()), v);
            }
        }
    }
    let now = std::fs::read_to_string(&c.state)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap()["ts"].as_u64().unwrap())
        .max()
        .unwrap()
        + 60;
    let c = Ctx { now, ..c };
    let issue = r#"{"number": 51, "title": "Dark mode for the settings page", "state": "OPEN", "created": "2026-10-01T00:00:00Z", "updated": "2026-10-02T00:00:00Z", "labels": ["ui"], "ac": true, "pr": false}"#;
    let gh = Fake::default().once("gh api graphql", 0, &format!("{issue}\n"));
    let cfg = drover::config::Config::load(&c.config).unwrap();
    drover::gh::backlog(&c, &gh, cfg.fleet("acme-web").unwrap(), "acme-web").unwrap();
    let s = Server::new(&c, &r, Some("demo".into()));
    for path in SHEETS {
        let res = String::from_utf8_lossy(&s.respond(&format!("GET {path} HTTP/1.1\r\nHost: demo\r\n\r\n"), now))
            .into_owned();
        let (head, body) = res.split_once("\r\n\r\n").unwrap();
        assert!(head.starts_with("HTTP/1.1 200"), "{path}: {res}");
        out.insert(path.to_string(), serde_json::from_str(body).unwrap());
    }
    out.into_iter()
        .filter(|(_, v)| matches!(v["type"].as_str(), Some("screen" | "sheet")))
        .map(|(p, v)| (name(&p), serde_json::to_string_pretty(&v).unwrap() + "\n"))
        .collect()
}

#[test]
fn every_served_screen_has_a_fixture_its_builder_writes_today() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/screens");
    let want = built();
    if std::env::var_os("BLESS").is_some() {
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        for (f, body) in &want {
            std::fs::write(dir.join(f), body).unwrap();
        }
    }
    let mut have = BTreeMap::new();
    for e in std::fs::read_dir(&dir).unwrap() {
        let p = e.unwrap().path();
        let f = p.file_name().unwrap().to_string_lossy().into_owned();
        have.insert(f, std::fs::read_to_string(&p).unwrap());
    }
    let names = |m: &BTreeMap<String, String>| m.keys().cloned().collect::<Vec<_>>();
    assert_eq!(names(&have), names(&want), "fixtures/screens: rerun with BLESS=1");
    for (f, body) in &want {
        assert_eq!(&have[f], body, "fixtures/screens/{f} is stale: rerun with BLESS=1");
    }
    let served = [
        "queue",
        "lanes",
        "lanes-",
        "events",
        "fleets",
        "notify",
        "workflow",
        "drive-health",
        "scene",
        "hot-",
        "issue-",
        "verdict-",
        "backlog-",
        "queue-done-",
        "ask-",
        "hosts",
        "threads",
    ];
    for s in served {
        assert!(
            want.keys().any(|f| f.starts_with(s)),
            "no fixture for /screens/{s}: {:?}",
            names(&want)
        );
    }
}
