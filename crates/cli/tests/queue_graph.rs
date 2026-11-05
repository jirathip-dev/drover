//! #115: the Queue tab as a git-log style graph. `serial_label` config, the rows' graph data, and (WebKit, 390 px) the
//! drawn graph on a fixture with chains, a fork, a clamped chain and a blocker out of view, the [?] legend sheet and
//! the control audit on both. Screenshots go to `$HERDSMAN_SNAPSHOTS`.
mod common;
use common::*;
use herdsman::config::Config;
use herdsman::events::{append, Event};
use herdsman::serve::Server;
use serde_json::json;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

/// `acme` names its serial mark, `beta` keeps the default (`db`, #294).
fn config(label: &str) -> String {
    format!(
        r#"[fleet.acme]
repo = "acme/app"
local = "/tmp/does-not-matter/app"
integration = "staging"
serial_paths = ["db/migrations/**", "db/seed.sql"]
{label}

[fleet.beta]
repo = "acme/beta"
local = "/tmp/does-not-matter/beta"
integration = "staging"
serial_paths = ["schema/**"]
"#
    )
}

fn load(label: &str) -> anyhow::Result<Config> {
    let d = tmpdir("qg-config");
    std::fs::write(d.join("fleet.toml"), config(label)).unwrap();
    Config::load(&d.join("fleet.toml"))
}

#[test]
fn serial_label_defaults_to_db_and_takes_an_override() {
    // #294: the old form is one group named `serial_label`, `db` when unset.
    let c = load("").unwrap();
    assert_eq!(c.fleet["acme"].serial_label(), "db");
    let c = load("serial_label = \"schema\"").unwrap();
    assert_eq!(c.fleet["acme"].serial_label(), "schema");
    assert_eq!(c.fleet["beta"].serial_label(), "db");
    for bad in ["serial_label = \"\"", "serial_label = \"migrations\""] {
        let e = format!("{:#}", load(bad).unwrap_err());
        assert!(
            e.contains("fleet.acme.serial: a name (serial_label) must be 1 to 8 characters"),
            "{bad}: {e}"
        );
    }
}

/// #268: a page as the board shows it, its Queue tab drawn by web/src/queue.ts.
fn req(s: &Server, path: &str, extra: &str) -> String {
    let res = String::from_utf8_lossy(&s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}\r\n"), T))
        .into_owned();
    with_queue(&res, s, ADDR, T)
}

/// acme: 1 → 2 → 3 → 4 → 5 (5 clamps at depth 4), 6 also on 1 (a fork), 7 on #99 (not queued), 8 serial.
/// beta: 22 on 21, both serial. #1 has an impl lane.
fn fixture() -> (herdsman::Ctx, Fake) {
    let c = ctx(&tmpdir("queue-graph"), &config("serial_label = \"db\""));
    let ok = |args: &[&str]| assert_eq!(run(&c, &Fake::default(), args).0, 0, "{args:?}");
    ok(&["queue", "acme", "add", "1", "2", "3", "4", "5", "6", "7"]);
    ok(&["queue", "acme", "add", "--serial", "8"]);
    for (n, on) in [("2", "1"), ("3", "2"), ("4", "3"), ("5", "4"), ("6", "1"), ("7", "99")] {
        ok(&["queue", "acme", "block", "--on", on, n]);
    }
    ok(&["queue", "beta", "add", "--serial", "21", "22"]);
    ok(&["queue", "beta", "block", "--on", "21", "22"]);
    // #331: every row's title (the deep ones too) is long enough to be cut at the card edge.
    let cache = c.state.parent().unwrap().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    for (f, n) in (1..=8).map(|n| ("acme", n)).chain([("beta", 21), ("beta", 22)]) {
        let t = format!("Issue {n}: queue rows show their one-line title at every depth");
        std::fs::write(
            cache.join(format!("issue-{f}-{n}.json")),
            json!({ "title": t }).to_string(),
        )
        .unwrap();
    }
    let spawn = json!({"role": "impl", "agent": "acme-impl-1", "harness": "claude"});
    append(&c.state, &Event::new(900_000, "acme", Some(1), "spawn", spawn)).unwrap();
    (c, Fake::default())
}

#[test]
fn rows_carry_the_graph_data_and_the_fleet_its_serial_paths() {
    let (c, r) = fixture();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let page = req(&s, "/", "");
    assert!(page.contains("<button type=button class=qhelp aria-label=\"what the marks mean\">?</button><button id=rf"));
    assert!(page.contains("<div class=\"fc k-l\" data-s=\"db\" data-sp=\"db/migrations/**, db/seed.sql\"><div class=fh><span class=fn><b>acme</b>"));
    assert!(page.contains(
        "<div class=\"fc k-a\" data-s=\"db\" data-sp=\"schema/**\"><div class=fh><span class=fn><b>beta</b>"
    ));
    // #242: a row with a lane carries its coat for the row sprite.
    let at = page
        .find("<details class=\"q\" data-q data-act=impl style=\"--rc:#")
        .unwrap();
    assert!(page[at..]
        .split_once('>')
        .unwrap()
        .1
        .starts_with("<summary><span><a href=\"https://github.com/acme/app/issues/1\">#1</a></span>"));
    assert!(page.contains("<details class=\"q\" data-q data-b=99 data-act=idle><summary><span><a href=\"https://github.com/acme/app/issues/7\">#7</a></span>"));
    assert!(page.contains("<details class=\"q\" data-q data-s=\"db\" data-act=idle><summary><span><a href=\"https://github.com/acme/app/issues/8\">#8</a></span>"));
    assert!(page.contains("<details class=\"q\" data-q data-b=21 data-s=\"db\" data-act=idle><summary><span><a href=\"https://github.com/acme/beta/issues/22\">#22</a></span>"));
    // Change 1 of the approval: the card no longer repeats `#n state · age`; the lane stays.
    assert!(!page.contains("<p>#1 impl"), "{page}");
    assert!(page.contains("<p>acme-impl-1</p>"));
}

#[test]
fn the_graph_and_its_legend_pass_the_audit_at_390px() {
    if std::process::Command::new("swift").arg("--version").output().is_err() || !cfg!(target_os = "macos") {
        eprintln!("skipping: no swift/WebKit here");
        return;
    }
    let (c, r) = fixture();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let page = req(&s, "/", &pair(&s, &c, "Mozilla/5.0 (iPhone)", T));
    let d = tmpdir("queue-graph-png");
    let file = d.join("page.html");
    // #268: the Queue tab drawn in (`req`), without the board (it would read this file's base server).
    let html = &page[page.find("<!doctype").unwrap()..page.find("<script type=module").unwrap()];
    std::fs::write(&file, format!("{html}</body></html>\n")).unwrap();
    // Per row in page order: #n, its gutter (#241: a row sprite's glyph), its state cell with the tags, its moves.
    // #367: each group draws its own trees, so the running #1 stands alone and its queued followers carry `! #1`.
    let want = [
        "#1 ● impl",
        "#2 ○ queued ! #1 ▼after6",
        "#3 └─○ queued",
        "#4   └─○ queued",
        "#5   └─○ queued after #4",
        "#6 ○ queued ! #1 ▲before2 ▼after7",
        "#7 ○ queued ! #99 ▲before6 ▼after8",
        "#8 ○ queued 🔒db ▲before7",
        "#21 ○ queued 🔒db",
        "#22 └─○ queued 🔒db",
    ]
    .join("|");
    // #367: every repo's card shown and the chain opened, so every row is measured.
    let graph = format!(
        "document.querySelectorAll('#queue .fc').forEach(c => {{ c.hidden = false; }}); \
         document.querySelectorAll('#queue details.qc').forEach(d => {{ d.open = true; }}); \
         const got = [...document.querySelectorAll('#queue details.q')].map(d => [d.querySelector('summary>span').textContent, \
         [...d.querySelector('.gt').children].map(n => n.dataset.g ?? n.textContent).join('') + ' ' + d.querySelector('summary>span:nth-of-type(2)').textContent, \
         ...[...d.querySelectorAll('[name=before],[name=after]')].map(i => (i.name === 'before' ? '▲' : '▼') + i.name + i.value)].join(' ')).join('|'); \
         if (got !== '{want}') throw new Error('graph: ' + got); \
         const h0 = document.querySelector('#queue details.q.d0>summary').getBoundingClientRect().height; \
         for (const d of document.querySelectorAll('#queue details.q')) {{ const t = d.querySelector('summary>.tl'), r = t?.getBoundingClientRect(); \
           if (!r || !r.height || r.height > parseFloat(getComputedStyle(t).lineHeight) + 1 || r.right > 390 || getComputedStyle(t).textOverflow !== 'ellipsis' \
             || t.title !== t.textContent || d.querySelector('summary').getBoundingClientRect().height > h0 + 1) \
             throw new Error('title of ' + d.querySelector('summary>span').textContent + ': ' + JSON.stringify(r) + ' ' + t?.title); }} \
         if (document.querySelector('details.q[data-s] .tag').getAttribute('aria-label') !== 'serial: one lane at a time') throw new Error('lock label'); \
         if (document.querySelector('details.q summary>span:nth-of-type(2)>span').title !== 'impl = being built') throw new Error('no state hint'); \
         document.querySelectorAll('#queue details').forEach((d, i) => {{ d.open = i === 0; }})"
    );
    let legend = "legend(); const l = [...document.querySelectorAll('.top p')].map(p => p.textContent).join('|'); \
         for (const w of ['queued = waiting its turn', 'review(n) = being checked, round n', 'needs you = waiting for your answer', \
         '! needs you', '🔒db one at a time: touches db/migrations/**, db/seed.sql', '🔒db one at a time: touches schema/**']) \
         if (!l.includes(w)) throw new Error('legend: ' + l); \
         sh.style.willChange = 'auto'; sh.style.transition = 'none'";
    let stub = "window.fetch = async () => new Response('no pane output', {status: 404})";
    let audit = include_str!("ui/audit.js");
    for (name, root, then) in [
        ("ui-queue-graph", "#queue", graph),
        ("ui-queue-legend", "#sheet", legend.to_string()),
    ] {
        let js =
            format!("{stub}; {audit}; {{ {then} }}; window.measure = [audit('{root}')]; tab('queue'); {{ {then} }}");
        let png = std::env::var("HERDSMAN_SNAPSHOTS").map_or(d.join(format!("{name}.png")), |p| {
            std::path::Path::new(&p).join(format!("{name}.png"))
        });
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/webview.swift");
        let o = swift()
            .args([
                script,
                file.to_str().unwrap(),
                "390",
                "queue",
                "horse",
                png.to_str().unwrap(),
                &js,
            ])
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&o.stdout);
        let v: Vec<f64> = out.split_whitespace().filter_map(|n| n.parse().ok()).collect();
        assert!(
            o.status.success() && v.len() == 3,
            "{name}: {out} {}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(
            v[0] <= 390.0 && v[1] == 390.0,
            "{name}: scrollWidth {} at {}",
            v[0],
            v[1]
        );
        // The legend sheet has no controls of its own (the lane parts are hidden); [?] is audited on the queue.
        assert!(v[2] > 0.0 || name == "ui-queue-legend", "{name}: no controls audited");
    }
}
