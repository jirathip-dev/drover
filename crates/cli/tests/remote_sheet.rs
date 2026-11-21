mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const NOW: u64 = 1_000_000;
const IMPL: &str = "sendmeter-impl-12";
const HERE: &str = "sendmeter-impl-14";

fn numbered(n: usize, at: &str) -> String {
    (1..=n).map(|i| format!("line {i} {at}\n")).collect()
}

#[test]
fn a_lane_sheet_opens_without_a_shed_call() {
    let config = format!("{CONFIG}\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n");
    let c = ctx(&tmpdir("remote-sheet"), &config);
    let d = json!({"role": "impl", "agent": IMPL, "harness": "claude", "pane": IMPL, "worktree": "/wt",
                   "branch": "issue-12", "host": "bazzite", "machine": "bazzite"});
    append(&c.state, &Event::new(NOW - 60, "sendmeter", Some(12), "spawn", d)).unwrap();
    let d = json!({"role": "impl", "agent": HERE, "harness": "claude", "pane": HERE, "worktree": "/wt14",
                   "branch": "issue-14"});
    append(&c.state, &Event::new(NOW - 50, "sendmeter", Some(14), "spawn", d)).unwrap();
    let (sh, bazzite, f) = (FakeShed::attached(&c), FakeShed::new(), Fake::default());
    sh.lane(HERE, "working", "/wt14");
    sh.screen(HERE, &numbered(300, "here"));
    bazzite.lane(IMPL, "working", "/wt");
    bazzite.screen(IMPL, &numbered(300, "on bazzite"));
    let r = OverSsh {
        f: &f,
        hosts: vec![("bazzite", &bazzite)],
    };
    let w = sh.on(&r);
    look(&c, &w);
    let s = Server::new(&c, &w, Some(ADDR.into())).deferring();
    let cookie = pair(&s, &c, "", NOW);
    let get = |path: &str, now: u64| {
        let req = format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}\r\n");
        String::from_utf8_lossy(&s.respond(&req, now)).into_owned()
    };
    let calls = || (sh.methods().len(), bazzite.methods().len(), f.calls.borrow().len());
    let transcript = |agent: &str, now: u64| {
        let before = calls();
        let res = get(&format!("/screens/lanes/{agent}"), now);
        assert_eq!(calls(), before, "{agent}: no shed, ssh or other call on open");
        let v: Value = serde_json::from_str(res.split_once("\r\n\r\n").unwrap().1).unwrap();
        let t = v["blocks"].as_array().unwrap().iter().find(|b| b["id"] == "transcript");
        t.unwrap_or_else(|| panic!("{v}"))["blocks"].as_array().unwrap().clone()
    };
    let ids = |t: &[Value]| {
        t.iter()
            .map(|b| b["id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    get("/screens/lanes", NOW);
    s.refresh(NOW);
    for (agent, at) in [(HERE, "here"), (IMPL, "on bazzite")] {
        let t = transcript(agent, NOW).iter().map(Value::to_string).collect::<String>();
        assert!(
            t.contains(&format!("line 300 {at}")) && t.contains(&format!("line 101 {at}")),
            "{t}"
        );
        assert!(!t.contains(&format!("line 100 {at}")), "the last 200 lines: {t}");
    }

    s.refresh(NOW + 1);
    s.refresh(NOW + 11);
    let (here, there) = (ids(&transcript(HERE, NOW + 11)), ids(&transcript(IMPL, NOW + 11)));
    let gen = here[0].split('-').next().unwrap().to_string();
    assert_eq!(
        here,
        ["101", "151", "201", "251"].map(|n| format!("{gen}-{n}")),
        "a block a 50-line run, its id its first line"
    );
    sh.screen(HERE, &numbered(301, "here"));
    bazzite.screen(IMPL, &numbered(301, "on bazzite"));
    s.refresh(NOW + 22);
    let (here2, there2) = (transcript(HERE, NOW + 22), transcript(IMPL, NOW + 22));
    assert_eq!(ids(&here2), [&here[..], &[format!("{gen}-301")]].concat(), "ids keep");
    assert_eq!(ids(&there2)[..there.len()], there[..], "ids keep on a remote lane");
    assert!(here2.last().unwrap().to_string().contains("line 301 here"));
    assert!(there2.last().unwrap().to_string().contains("line 301 on bazzite"));
}
