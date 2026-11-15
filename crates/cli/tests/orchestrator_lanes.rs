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
        append(&c.state, &Event::new(ts, fleet, issue, kind, d)).unwrap()
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
fn orchestrators_are_lanes_of_their_repo_first_in_its_group_and_asleep_once_closed() {
    let (c, b) = setup("orchestrator-lanes");
    let agents = drover::herdr::agents(&b.on(&Fake::default())).unwrap();
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let events = read(&c.state).unwrap();
    let st = drover::events::fold(&events);
    let lanes = drover::lanes::join(
        &cfg,
        &Fake::default(),
        &c.worktrees,
        &events,
        &st,
        &agents,
        &Default::default(),
    );
    let row = |l: &drover::lanes::Lane| {
        let s = |v: &Option<String>| v.clone().unwrap_or_default();
        (
            s(&l.fleet),
            l.name.clone(),
            l.role.clone(),
            l.state.clone(),
            s(&l.model),
            s(&l.thread),
            l.issue,
        )
    };
    let got: Vec<_> = lanes.iter().map(row).collect();
    let r = |f: &str, n: &str, role: &str, st: &str, m: &str, t: &str, i| {
        (f.into(), n.into(), role.into(), st.into(), m.into(), t.into(), i)
    };
    assert_eq!(
        got,
        [
            r("drover", "drover-impl-5", "impl", "idle", "sonnet", "", Some(5)),
            r(
                "drover",
                "Orchestrator-run design lanes · t6",
                "orchestrator",
                "idle",
                "opus",
                "t6",
                None
            ),
            r("all", "Every repo · t7", "orchestrator", "working", "opus", "t7", None),
            r("", "orchestrator-t99", "other", "idle", "", "", None),
            r("", "side", "other", "idle", "", "", None),
            r("drover", "Old chat · t8", "orchestrator", "asleep", "opus", "t8", None),
        ]
    );
    let groups = drover::lanes::group(&lanes, |l| l.since);
    let names = |sec: &str, f: &str| -> Vec<String> {
        let subs = &groups.iter().find(|g| g.0 == sec).unwrap().1;
        subs.iter()
            .find(|s| s.0 == f)
            .unwrap()
            .1
            .iter()
            .map(|l| l.name.clone())
            .collect()
    };
    let idle: Vec<&str> = groups
        .iter()
        .find(|g| g.0 == "idle")
        .unwrap()
        .1
        .iter()
        .map(|s| s.0.as_str())
        .collect();
    assert_eq!(idle, ["drover", "Others"]);
    assert_eq!(
        names("idle", "drover"),
        ["Orchestrator-run design lanes · t6", "Old chat · t8", "drover-impl-5"]
    );
    assert_eq!(names("working", "all repos"), ["Every repo · t7"]);
    assert_eq!(names("idle", "Others"), ["orchestrator-t99", "side"]);
}

#[test]
fn the_lanes_api_carries_orchestrator_rows_with_their_thread() {
    let (c, b) = setup("orchestrator-lanes-page");
    let f = Fake::default();
    let r = b.on(&f);
    let s = Server::new(&c, &r, None);
    let v: serde_json::Value = serde_json::from_str(&api(&s, "", "", T, "lanes")).unwrap();
    let repo = |sec: &str, f: &str| {
        let secs = v["sections"].as_array().unwrap();
        let sec = secs.iter().find(|x| x["name"] == sec).unwrap_or_else(|| panic!("{v}"));
        let repos = sec["repos"].as_array().unwrap();
        repos.iter().find(|r| r["name"] == f).unwrap_or_else(|| panic!("{sec}"))["lanes"].clone()
    };
    let drover = repo("idle", "drover");
    let row = |name: &str| {
        let all = drover.as_array().unwrap();
        all.iter()
            .find(|l| l["name"] == name)
            .unwrap_or_else(|| panic!("{drover}"))
            .clone()
    };
    assert_eq!(drover[0]["name"], "Orchestrator-run design lanes · t6");
    assert_eq!(
        (&drover[0]["role"], &drover[0]["thread"]),
        (&json!("orchestrator"), &json!("t6"))
    );
    let old = row("Old chat · t8");
    assert_eq!(
        (&old["gone"], &old["thread"], &old["state"], &old["tone"]),
        (&json!(true), &json!("t8"), &json!("asleep"), &json!("m")),
        "asleep reads muted: {old}"
    );
    assert_eq!(repo("working", "all repos")[0]["thread"], "t7");
    let others = repo("idle", "Others");
    assert!(
        others.as_array().unwrap().iter().all(|l| l["thread"].is_null())
            && others.to_string().contains("orchestrator-t99"),
        "{others}"
    );
}
