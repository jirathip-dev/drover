mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::json;

const T: u64 = 1_000_000;

#[test]
fn lanes_newest_first_and_a_gone_lane_has_no_link() {
    let c = ctx(&tmpdir("queue-lanes"), CONFIG);
    let names = [
        "impl-3",
        "rev-3-r1",
        "impl-3-f1",
        "rev-3-r2",
        "impl-3-f2",
        "rev-3-r3",
        "impl-3-u1",
    ];
    for (i, a) in names.iter().enumerate() {
        let d = json!({"role": if a.starts_with("rev") { "rev" } else { "impl" }, "agent": a, "harness": "claude"});
        append(
            &c.state,
            &Event::new(T - 3600 + i as u64 * 60, "sendmeter", Some(3), "spawn", d),
        )
        .unwrap();
    }
    let sh = FakeShed::new();
    sh.attach(&c);
    for (n, status) in [("impl-3-u1", "working"), ("rev-3-r3", "done")] {
        sh.lane(n, status, "/x");
        sh.screen(n, "");
    }
    let f = Fake::default();
    let r = sh.on(&f);
    let lanes = |r: &dyn drover::runner::Runner| {
        let q: serde_json::Value = serde_json::from_str(&api(&Server::new(&c, r, None), "", "", T, "queue")).unwrap();
        let row = q["fleets"][0]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["issue"] == 3)
            .cloned();
        row.unwrap_or_else(|| panic!("{q}"))["lanes"]
            .as_array()
            .unwrap()
            .clone()
    };
    let line = |l: &serde_json::Value| {
        format!(
            "{} {} {} {}",
            l["name"].as_str().unwrap(),
            l["state"],
            l["ago"],
            l["link"]
        )
    };
    let got = lanes(&r);
    let mut order: Vec<&str> = names.to_vec();
    order.reverse();
    assert_eq!(
        got.iter().map(|l| l["name"].as_str().unwrap()).collect::<Vec<_>>(),
        order,
        "newest first"
    );
    assert_eq!(line(&got[0]), "impl-3-u1 \"working\" \"54m\" \"live\"");
    assert_eq!(line(&got[1]), "rev-3-r3 \"done\" \"55m\" \"live\"");
    assert_eq!(
        line(&got[3]),
        "rev-3-r2 \"gone\" \"57m\" \"\"",
        "a gone lane has no tap target"
    );
    sh.lane("rev-3-r2", "done", "/x");
    drover::term::close(&c, &r, "rev-3-r2").unwrap();
    assert_eq!(lanes(&r)[3]["link"], "gone", "the log drover archived at close");
    drover::session::transcript::forget(&c, "rev-3-r2");
    drop(sh);
    assert_eq!(lanes(&r)[0]["link"], "", "no pane: no link");
}
