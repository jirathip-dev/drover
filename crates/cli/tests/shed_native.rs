mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const NOW: u64 = 1_000_000;
const LANE: &str = "sendmeter-impl-12";
const STALL: u64 = 30 * 60;

fn setup(name: &str, config: &str) -> (drover::Ctx, FakeShed) {
    let c = ctx(&tmpdir(name), config);
    let sp = json!({"role": "impl", "agent": LANE, "harness": "claude", "pane": LANE, "runtime": "shed-acp"});
    append(&c.state, &Event::new(NOW - 60, "sendmeter", Some(12), "spawn", sp)).unwrap();
    let sh = FakeShed::attached(&c);
    (c, sh)
}

fn at(c: &drover::Ctx, now: u64) -> drover::Ctx {
    drover::Ctx { now, ..c.clone() }
}

fn check(c: &drover::Ctx) -> Vec<Event> {
    let before = read(&c.state).unwrap().len();
    let (code, out) = run(c, &Fake::default(), &["check"]);
    assert_eq!(code, 0, "{out}");
    let alerts = [
        "stall",
        "lane-dead",
        "orphan-lane",
        "lane-failed-start",
        "usage-limit",
        "signed-out",
    ];
    let all = read(&c.state).unwrap();
    all[before..]
        .iter()
        .filter(|e| alerts.contains(&e.kind.as_str()))
        .cloned()
        .collect()
}

fn kinds(evs: &[Event]) -> Vec<&str> {
    evs.iter().map(|e| e.kind.as_str()).collect()
}

fn v1(c: &drover::Ctx, sh: &FakeShed, path: &str) -> Value {
    let f = Fake::default();
    let r = sh.on(&f);
    let s = Server::new(c, &r, Some(ADDR.into()));
    let ck = pair(&s, c, "", c.now);
    serde_json::from_str(&api(&s, ADDR, &ck, c.now, path)).unwrap()
}

fn row(c: &drover::Ctx, sh: &FakeShed, name: &str) -> Value {
    let lanes = v1(c, sh, "lanes");
    let rows = lanes["sections"].as_array().unwrap().iter().flat_map(|s| {
        let repos = s["repos"].as_array().unwrap();
        repos
            .iter()
            .flat_map(|r| r["lanes"].as_array().unwrap().clone())
            .collect::<Vec<_>>()
    });
    rows.into_iter()
        .find(|l| l["name"] == name)
        .unwrap_or_else(|| panic!("{lanes}"))
}

fn herd(c: &drover::Ctx, sh: &FakeShed, name: &str) -> Value {
    let h = v1(c, sh, "herd");
    let lanes = h["fleets"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|f| f["lanes"].as_array().unwrap().clone());
    lanes
        .into_iter()
        .find(|l| l["name"] == name)
        .unwrap_or_else(|| panic!("{h}"))
}

#[test]
fn a_shed_session_goes_running_idle_exited_on_every_surface() {
    let (c, sh) = setup("shed-native", CONFIG);
    sh.lane(LANE, "running", "/x");
    let said = json!([
        {"seq": 1, "event": "turn", "prompt": "Read .brief.md"},
        {"seq": 2, "event": "reply", "text": "Pushed.", "stop_reason": "end_turn"}
    ]);
    sh.set(LANE, |t| {
        t["reported"]["note"] = json!("running: cargo test (8m)");
        (t["acp"], t["events"]) = (json!("claude-acp"), said);
    });
    let c0 = at(&c, NOW);
    let r = row(&c0, &sh, LANE);
    assert_eq!(
        (&r["state"], &r["now"]["n"]),
        (&json!("running"), &json!("running: cargo test (8m)")),
        "{r}"
    );
    assert_eq!(herd(&c0, &sh, LANE)["state"], "running");
    assert_eq!(kinds(&check(&c0)), Vec::<&str>::new());

    sh.set(LANE, |t| t["reported"] = json!({"state": "idle", "note": "end_turn"}));
    assert_eq!(kinds(&check(&c0)), Vec::<&str>::new(), "quiet, under the threshold");
    let c1 = at(&c, NOW + STALL + 1);
    let r = row(&c1, &sh, LANE);
    assert_eq!(
        (&r["state"], &r["activity"]),
        (&json!("idle"), &json!("end_turn")),
        "{r}"
    );
    assert_eq!(herd(&c1, &sh, LANE)["state"], "idle");
    let got = check(&c1);
    assert_eq!(kinds(&got), ["stall"]);
    assert!(
        got[0].str("reason").unwrap().starts_with("quiet for 30m, no turn-end"),
        "{got:?}"
    );
    assert_eq!(kinds(&check(&c1)), Vec::<&str>::new(), "once");

    sh.set(LANE, |t| t["reported"] = json!({"state": "working"}));
    assert_eq!(kinds(&check(&at(&c, NOW + STALL + 2))), Vec::<&str>::new());
    sh.set(LANE, |t| t["reported"] = json!({"state": "done"}));
    check(&at(&c, NOW + STALL + 3));
    let d = json!({"state": "done", "role": "impl", "agent": LANE});
    append(
        &c.state,
        &Event::new(NOW + STALL + 4, "sendmeter", Some(12), "turn-end", d),
    )
    .unwrap();
    let c2 = at(&c, NOW + 2 * STALL + 10);
    assert_eq!(row(&c2, &sh, LANE)["state"], "done");
    assert_eq!(kinds(&check(&c2)), Vec::<&str>::new(), "a turn-end is never a stall");

    sh.set(LANE, |t| (t["state"], t["exit"]) = (json!("exited"), json!(1)));
    let got = check(&c2);
    assert_eq!(kinds(&got), ["lane-dead"]);
    assert_eq!(got[0].str("cause"), Some("adapter crash"), "{got:?}");
    let r = row(&c2, &sh, LANE);
    assert_eq!(
        (&r["state"], &r["dead"], &r["gone"]),
        (&json!("dead"), &json!(true), &json!(true)),
        "{r}"
    );
    assert_eq!(herd(&c2, &sh, LANE)["state"], "dead");
    let gone = read(&c.state)
        .unwrap()
        .iter()
        .rev()
        .find(|e| e.kind == "lane-state")
        .cloned()
        .unwrap();
    assert_eq!(
        (gone.str("state"), gone.str("cause")),
        (Some("gone"), Some("adapter crash")),
        "the poll stamped it"
    );
    let t = v1(&c2, &sh, &format!("lane/{LANE}"));
    let blocks = t["blocks"].as_array().unwrap();
    assert!(
        blocks.iter().any(|b| b.to_string().contains("Pushed.")),
        "shed's own log: {t}"
    );
}

#[test]
fn a_session_no_spawn_names_is_an_orphan_and_an_unreachable_host_is_never_dead() {
    let cfg = format!("{CONFIG}\n[hosts.mini]\n\n[hosts.box]\nmachine = \"box\"\n");
    let (c, sh) = setup("shed-orphan", &cfg);
    let sp =
        json!({"role": "impl", "agent": "sendmeter-impl-14", "harness": "claude", "host": "box", "machine": "box"});
    append(&c.state, &Event::new(NOW - 60, "sendmeter", Some(14), "spawn", sp)).unwrap();
    sh.lane(LANE, "working", "/x");
    sh.lane("sendmeter-impl-99", "working", "/y");
    let got = check(&at(&c, NOW));
    assert_eq!(kinds(&got), ["orphan-lane"], "{got:?}");
    assert_eq!(got[0].issue, Some(99));
    assert_eq!(
        got[0].str("reason"),
        Some("session sendmeter-impl-99 has no spawn event")
    );
    assert_eq!(
        kinds(&check(&at(&c, NOW + 2 * STALL))),
        Vec::<&str>::new(),
        "box's lane: no judgement"
    );
}

#[test]
fn a_lane_that_died_on_econnrefused_names_the_proxy_tunnel_not_an_adapter_crash() {
    let (c, sh) = setup("shed-econnrefused", CONFIG);
    sh.lane(LANE, "running", "/x");
    let c0 = at(&c, NOW);
    check(&c0);
    sh.set(LANE, |t| {
        t["reported"] = json!({"state": "error", "note": "API Error: Connection refused (ECONNREFUSED)"});
        (t["state"], t["exit"]) = (json!("exited"), json!(1));
    });
    let got = check(&c0);
    assert_eq!(kinds(&got), ["lane-dead"]);
    assert_eq!(got[0].str("cause"), Some("ECONNREFUSED: proxy tunnel down"), "{got:?}");
    let f = Fake::default();
    let (_, w) = run(&at(&c, NOW + 120), &sh.on(&f), &["waiting", "sendmeter"]);
    assert!(w.contains("lane gone (ECONNREFUSED: proxy tunnel down)"), "{w}");
}
