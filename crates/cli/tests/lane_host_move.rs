mod common;
use common::*;
use drover::events::{append, Event};
use serde_json::json;

const LANE: &str = "sendmeter-impl-12";

fn moved(name: &str, spawn_on_air: bool) -> (drover::Ctx, FakeShed, FakeShed) {
    let (mini, air) = (FakeShed::new(), FakeShed::new());
    let hosts = "[hosts.mini]\n\n[hosts.air]\nmachine = \"air\"\n";
    let c = ctx(&tmpdir(name), &mini.config(&format!("{CONFIG}\n{hosts}")));
    let mut d = json!({"role": "impl", "agent": LANE, "harness": "claude", "pane": LANE, "runtime": "shed"});
    if spawn_on_air {
        d["host"] = json!("air");
        d["machine"] = json!("air");
    }
    append(&c.state, &Event::new(c.now - 500, "sendmeter", Some(12), "spawn", d)).unwrap();
    (c, mini, air)
}

fn crashed(sh: &FakeShed) {
    sh.lane(LANE, "working", "/w/issue-12");
    sh.set(LANE, |t| {
        (t["state"], t["exit"]) = (json!("exited"), json!(1));
        t["reported"] = json!({"state": "working", "note": "acp: the agent closed its stdout"});
    });
}

fn sweep(c: &drover::Ctx, mini: &FakeShed, air: &FakeShed, times: u64) {
    let f = Fake::default();
    let over = OverSsh {
        f: &f,
        hosts: vec![("air", air)],
    };
    let r = drover::shed::With(&over, mini.shed());
    for i in 0..times {
        let c = drover::Ctx {
            now: c.now + i * 60,
            ..c.clone()
        };
        let (code, out) = run(&c, &r, &["check"]);
        assert_eq!(code, 0, "{out}");
    }
}

fn states(c: &drover::Ctx) -> Vec<Event> {
    let all = drover::events::read(&c.state).unwrap().to_vec();
    all.into_iter().filter(|e| e.kind == "lane-state").collect()
}

#[test]
fn an_exited_terminal_on_the_old_host_is_ignored_and_removed() {
    let (c, mini, air) = moved("host-move-exited", true);
    crashed(&mini);
    air.lane(LANE, "hibernated", "/w/issue-12");
    sweep(&c, &mini, &air, 2);
    let rows = states(&c);
    assert!(rows.iter().all(|e| e.str("state") != Some("gone")), "{rows:?}");
    assert!(rows.len() <= 1, "{rows:?}");
    assert!(!mini.has(LANE), "the old host's exited terminal is removed");
    assert!(air.has(LANE), "the spawn host's running one stays");
}

#[test]
fn a_running_terminal_on_the_old_host_is_never_removed() {
    let (c, mini, air) = moved("host-move-running", true);
    mini.lane(LANE, "working", "/w/issue-12");
    air.lane(LANE, "hibernated", "/w/issue-12");
    sweep(&c, &mini, &air, 2);
    assert!(mini.has(LANE) && air.has(LANE));
    assert!(!mini.methods().contains(&"rm".to_string()), "{:?}", mini.methods());
    let rows = states(&c);
    assert_eq!(
        rows.iter().map(|e| e.str("state")).collect::<Vec<_>>(),
        [Some("hibernated")],
        "{rows:?}"
    );
}

#[test]
fn the_old_hosts_exit_events_stamp_nothing() {
    let (c, mini, _) = moved("host-move-stream", true);
    mini.lock().events.push(json!({"seq": 1, "name": LANE,
        "labels": {"fleet": "sendmeter", "issue": "12", "role": "impl", "harness": "claude"},
        "event": "exited", "code": 1}));
    let cfg = drover::config::Config::load(&c.config).unwrap();
    drover::session::stamp::intake(&c, &cfg, &cfg.shed().unwrap().wire).unwrap();
    assert!(states(&c).is_empty(), "{:?}", states(&c));
}

#[test]
fn the_spawn_hosts_exit_still_records_gone_with_its_cause() {
    let (c, mini, air) = moved("host-move-same", false);
    crashed(&mini);
    sweep(&c, &mini, &air, 2);
    let rows = states(&c);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (rows[0].str("state"), rows[0].str("cause"), rows[0].str("note")),
        (
            Some("gone"),
            Some("adapter crash"),
            Some("acp: the agent closed its stdout")
        )
    );
    assert!(mini.has(LANE), "the spawn host's own exited terminal stays");
}
