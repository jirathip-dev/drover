//! #267: the Host tab is the TypeScript board's (web/src/host.ts, run under node), drawn from API v1's `host`: the page
//! carries none of it, an export ships the data as `api/v1/host` with its forms inert, a live board's forms post. #440:
//! it is System's Hosts & health (serve.js makes its section); ALERTS moved to Notifications & devices (system.ts).
mod common;
use common::*;
use herdsman::serve::Server;
use std::path::PathBuf;

const ADDR: &str = "127.0.0.1:7777";

/// The groups' names in page order.
fn groups(h: &str) -> Vec<&str> {
    h.split("<span class=gn>")
        .skip(1)
        .map(|g| g.split('<').next().unwrap())
        .collect()
}

#[test]
fn an_export_ships_the_host_data_and_its_forms_are_inert() {
    let demo = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo"));
    let d = tmpdir("host-export");
    let c = herdsman::Ctx {
        config: demo.join("fleet.toml"),
        state: demo.join("events.jsonl"),
        worktrees: d.join("wt"),
        hermes: d.join("hermes"),
        exe: "herdsman".into(),
        now: 1,
        home: d.join("home"),
        hints: vec![],
    };
    std::fs::create_dir_all(d.join("wt/oldproj")).unwrap(); // an orphan folder: [remove orphans]
    herdsman::serve::export(&c, &Fake::default(), &d.join("site"), true).unwrap();
    let page = std::fs::read_to_string(d.join("site/index.html")).unwrap();
    assert!(
        !page.contains("id=host") && !page.contains("class=hcd"),
        "the page has no Host HTML"
    );
    let h = host_html(&std::fs::read_to_string(d.join("site/api/v1/host")).unwrap());
    assert_eq!(groups(&h), ["HOSTS", "FLEET DISK", "SERVICES", "FLEETS"], "{h}");
    // The demo's chart series ride inline (no `frag?id=hist` to read); its forms never post.
    assert!(h.contains("data-k=load data-s=\"") && h.contains(" data-x=\""), "{h}");
    assert!(
        h.contains("<form inert><input type=hidden name=op value=orphans>") && !h.contains("method=post"),
        "{h}"
    );
    assert!(
        !h.contains("id=push") && !h.contains("sign in ▾"),
        "no per-device rows: {h}"
    );
}

#[test]
fn a_live_board_draws_cards_gauges_and_forms_that_post() {
    let c = ctx(&tmpdir("host-live"), CONFIG);
    let r = Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("df -k /", "F\n/dev/disk1 41943040 1 4194304\n");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let h = host_tab(&s, ADDR, 1_000_000);
    assert_eq!(groups(&h)[..3], ["HOSTS", "FLEET DISK", "SERVICES"], "{h}");
    // One card, the main host's, its disk (4 of 40 GB free) under the 10 GB floor: red, the used part, `▶`, how far
    // under and the words why.
    assert!(h.contains("<span class=\"tag m\">main</span>"), "{h}");
    let disk = &h[h.find("data-k=disk").unwrap()..];
    let disk = &disk[..disk.find("</details>").unwrap()];
    assert!(
        disk.contains(
            "<span class=\"bar r\">▕███████████<i>░</i>▏</span> <b class=r>▶ free 4 GB</b> <span class=mt>of 40</span> (250%)"
        ),
        "{disk}"
    );
    assert!(h.contains("disk under floor"), "{h}");
    assert!(h.contains("<p class=gr data-t=g><span class=k>theme</span><span class=w><b>farm</b></span></p>"));
    assert!(h.contains("<button id=push type=button hidden data-key=") && !h.contains("<form inert"));
    assert!(h.contains("no paired device yet"), "{h}");
}
