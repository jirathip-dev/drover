//! #367: the Queue tab one repo at a time. serve.js's pure queue part, run under node: the group order (needs you, in
//! progress, queued) for every state word a row shows, the time `in impl 1h 47m`, and a collapsed chain's third line; the
//! server gives each row its age in seconds for it. (Chips, chain and `+ N more` in a browser: gate 2's `queue-repos`
//! and `queue-chain` views, docs/ui.md.)
mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

/// serve.js's pure queue part plus `js`, run under node: its stdout as JSON. None where node is missing.
fn node(js: &str) -> Option<Value> {
    if std::process::Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping: no node here");
        return None;
    }
    let src = include_str!("../../../assets/serve.js");
    let part = &src[src.find("// #367 queue: pure").unwrap()..src.find("// queue:end").unwrap()];
    let d = tmpdir("queue-js");
    std::fs::write(d.join("t.js"), format!("{part}\nconsole.log(JSON.stringify({js}))")).unwrap();
    let o = std::process::Command::new("node").arg(d.join("t.js")).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    Some(serde_json::from_slice(&o.stdout).unwrap())
}

#[test]
fn rows_group_needs_you_then_in_progress_then_queued() {
    // The state words serve.rs writes in a row's state cell, in a mixed order; grouped in QG's order, each group keeps
    // the rows' own order.
    let js = "(() => { const rows = ['queued', 'impl', 'needs-human', 'review(2)', 'blocked', '⏸ waiting: decision', 'fix(1)', \
              'ready', 'dead', 'parked', 'update', 'queued']; \
              return Object.keys(QG).map(g => [QG[g], rows.filter(s => qgroup(s) === g)]); })()";
    let Some(got) = node(js) else { return };
    assert_eq!(
        got,
        json!([
            ["needs you", ["needs-human", "⏸ waiting: decision"]],
            [
                "in progress",
                ["impl", "review(2)", "fix(1)", "ready", "dead", "parked", "update"]
            ],
            ["queued", ["queued", "blocked", "queued"]]
        ])
    );
}

#[test]
fn time_is_one_unit_style_naming_its_phase() {
    let js = "[[0, 59, 60, 6420, 3600, 86399, 90061].map(dur), inph('impl', 6420), inph('review(2)', 300), \
              inph('⏸ waiting: decision', 61), inph('needs-human', 7200)]";
    let Some(got) = node(js) else { return };
    assert_eq!(
        got,
        json!([
            ["0s", "59s", "1m", "1h 47m", "1h 0m", "23h 59m", "1d 1h"],
            "in impl 1h 47m",
            "in review 5m",
            "in waiting 1m",
            "in needs-human 2h 0m"
        ])
    );
}

/// #391: the chain row's third line names its first and last step and the one it waits on next, with that one's state.
#[test]
fn a_chain_names_first_to_last_and_the_next_steps_state() {
    let steps = "[{n: '267', b: '266'}, {n: '268', b: '267'}, {n: '344', b: '268'}, {n: '270', b: '267'}, \
                 {n: '345', b: '344'}, {n: '356', b: '345'}]";
    let js = format!(
        "[qsum({steps}, 'impl'), qsum({steps}, 'review(2)'), qsum({steps}, '⏸ waiting: decision'), qsum({steps}, ''), \
         qsum([{{n: '7'}}, {{n: '8', b: '7'}}], 'queued')]"
    );
    let Some(got) = node(&js) else { return };
    assert_eq!(
        got,
        json!([
            "#267 → #356 · next: #266 (in impl)",
            "#267 → #356 · next: #266 (in review)",
            "#267 → #356 · next: #266 (waiting)",
            "#267 → #356 · next: #266",
            "#7 → #8 · next: #7 (queued)"
        ])
    );
}

#[test]
fn a_row_carries_the_time_its_age_counts_from() {
    let c = ctx(
        &tmpdir("queue-tab"),
        "[fleet.acme]\nrepo = \"acme/app\"\nlocal = \"/tmp/does-not-matter/app\"\nintegration = \"staging\"\n",
    );
    assert_eq!(run(&c, &Fake::default(), &["queue", "acme", "add", "1", "2"]).0, 0);
    let spawn = json!({"role": "impl", "agent": "acme-impl-1", "harness": "claude"});
    append(&c.state, &Event::new(T - 6420, "acme", Some(1), "spawn", spawn)).unwrap();
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let page = queue_tab(&s, ADDR, T); // #268: the board draws the rows from API v1's `queue`
    let row = |n: u64| {
        let at = page.find(&format!("/issues/{n}\">#{n}</a>")).unwrap();
        page[at..].split("</summary>").next().unwrap().to_string()
    };
    assert!(
        row(1).contains(&format!("<span class=m data-t={}>", T - 6420)),
        "{}",
        row(1)
    );
    // #2 has no state of its own yet: it counts from now.
    assert!(row(2).contains(&format!("<span class=m data-t={T}>")), "{}", row(2));
}
