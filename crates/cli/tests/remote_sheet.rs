mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::json;

const ADDR: &str = "127.0.0.1:7777";
const NOW: u64 = 1_000_000;
const IMPL: &str = "sendmeter-impl-12";

#[test]
fn a_remote_lanes_sheet_shows_its_transcript() {
    let config = format!("{CONFIG}\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n");
    let c = ctx(&tmpdir("remote-sheet"), &config);
    let d = json!({"role": "impl", "agent": IMPL, "harness": "claude", "pane": IMPL, "worktree": "/wt",
                   "branch": "issue-12", "host": "bazzite", "machine": "bazzite"});
    append(&c.state, &Event::new(NOW - 60, "sendmeter", Some(12), "spawn", d)).unwrap();
    let (sh, bazzite, f) = (FakeShed::attached(&c), FakeShed::new(), Fake::default());
    bazzite.lane(IMPL, "working", "/wt");
    bazzite.screen(IMPL, "⏺ Bash(cargo test)\n  ⎿ ok on bazzite\n");
    let r = OverSsh {
        f: &f,
        hosts: vec![("bazzite", &bazzite)],
    };
    let w = sh.on(&r);
    look(&c, &w);
    let s = Server::new(&c, &w, Some(ADDR.into())).deferring();
    let cookie = pair(&s, &c, "", NOW);
    let get = |path: &str| {
        let req = format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}\r\n");
        String::from_utf8_lossy(&s.respond(&req, NOW)).into_owned()
    };
    let sheet = || {
        let res = get(&format!("/screens/lanes/{IMPL}"));
        let v: serde_json::Value = serde_json::from_str(res.split_once("\r\n\r\n").unwrap().1).unwrap();
        let t = v["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["id"] == "transcript")
            .cloned();
        t.unwrap_or_else(|| panic!("{v}")).to_string()
    };
    get("/screens/lanes");
    s.refresh(NOW);
    let t = sheet();
    assert!(t.contains("transcript loading…"), "never silently empty: {t}");
    s.refresh(NOW);
    let t = sheet();
    assert!(t.contains("ok on bazzite") && !t.contains("loading"), "{t}");
}
