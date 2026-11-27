mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const T: u64 = 1_000_000;

fn setup(name: &str) -> (drover::Ctx, FakeShed) {
    let cfg =
        format!("{CONFIG}\n[fleet.drover]\nrepo = \"o/drover\"\nlocal = \"/p/drover\"\nintegration = \"staging\"\n");
    let c = ctx(&tmpdir(name), &cfg);
    let ev = |ts: u64, fleet: &str, issue: Option<u64>, kind: &str, d: Value| {
        append(&c.state, &Event::new(ts, fleet, issue, kind, d)).unwrap();
    };
    let spawn = json!({"role": "impl", "agent": "drover-impl-5", "harness": "claude", "model": "sonnet"});
    ev(900_500, "drover", Some(5), "spawn", spawn);
    for (id, fleet, title, ts) in [
        ("t6", "drover", "Orchestrator-run design lanes", 900_200),
        ("t7", "all", "Every repo", 900_100),
        ("t8", "drover", "Old chat", 900_000),
    ] {
        ev(ts, fleet, None, "thread-new", json!({"thread": id, "title": title}));
        let agent = format!("orchestrator-{id}");
        let d = json!({"thread": id, "agent": agent, "pane": agent, "harness": "claude", "model": "opus"});
        ev(ts, fleet, None, "orchestrator-start", d);
    }
    ev(
        900_300,
        "drover",
        None,
        "orchestrator-sleep",
        json!({"thread": "t8", "agent": "orchestrator-t8", "why": "idle"}),
    );
    ev(
        900_300,
        "drover",
        None,
        "thread-new",
        json!({"thread": "t9", "title": "Never started"}),
    );
    let b = FakeShed::new();
    b.attach(&c);
    for (name, status) in [
        ("drover-impl-5", "idle"),
        ("orchestrator-t6", "idle"),
        ("orchestrator-t7", "working"),
        ("orchestrator-t99", "idle"),
    ] {
        b.lane(name, status, "/x");
    }
    b.add("side", json!({"fleet": "side-project", "harness": "codex"}));
    b.set("side", |t| {
        t["cwd"] = json!("/Users/me/side");
        t["reported"] = json!({"state": "idle"});
    });
    (c, b)
}

#[test]
fn orchestrator_starts_and_archived_threads_make_no_lane() {
    let (c, b) = setup("orchestrator-lanes");
    let d = json!({"thread": "t8", "by": "migration"});
    append(&c.state, &Event::new(900_400, "drover", None, "thread-archive", d)).unwrap();
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let agents = drover::link::fresh(&c, &cfg, &b.on(&Fake::default()))
        .unwrap()
        .sessions();
    let events = read(&c.state).unwrap();
    let st = drover::events::fold(&events);
    let lanes = drover::lanes::join(&cfg, None, &events, &st, &agents);
    let mut got: Vec<_> = lanes
        .iter()
        .map(|l| (l.name.clone(), l.role.clone(), l.thread.clone()))
        .collect();
    got.sort();
    let r = |n: &str, role: &str| (n.to_string(), role.to_string(), None);
    assert_eq!(
        got,
        [
            r("drover-impl-5", "impl"),
            r("orchestrator-t6", "other"),
            r("orchestrator-t7", "other"),
            r("orchestrator-t99", "other"),
            r("side", "other"),
        ]
    );
}

#[test]
fn the_lanes_api_has_no_asleep_orchestrator_row() {
    let (c, b) = setup("orchestrator-lanes-page");
    let f = Fake::default();
    let r = b.on(&f);
    let s = Server::new(&c, &r, None);
    let v: serde_json::Value = serde_json::from_str(&api(&s, "", "", T, "lanes")).unwrap();
    let text = v.to_string();
    assert!(
        !text.contains("Old chat") && !text.contains("\"asleep\"") && !text.contains("\"orchestrator\""),
        "{v}"
    );
}
