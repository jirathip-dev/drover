//! #58 amendment B: one terminal-style control system. The `tests/ui/audit.js` console audit on every
//! screen (Queue with rows open, a fleet's [config] in System, Lanes, System's Hosts & health and Activity, the sheet,
//! Herd): zero outliers. 390 px screenshots go to `$HERDSMAN_SNAPSHOTS`.
mod common;
use common::*;
use herdsman::events::{append, Event};
use herdsman::serve::Server;
use serde_json::json;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;
const MENU: &str = "⏺ I need to pick a database.\n\n Which database should the meter use?\n\n❯ 1. SQLite\n  2. Postgres, with a long option label that has to ellipsise\n  3. Type something.\n\nEnter to select · ↑/↓ to navigate · Esc to cancel\n";

fn req(s: &Server, method: &str, path: &str, body: &str, extra: &str) -> String {
    String::from_utf8_lossy(&s.respond(
        &format!(
            "{method} {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
            body.len()
        ),
        T,
    ))
    .into_owned()
}

/// The audited page (two lanes, one blocked on a menu, a queue, the fleet's config, Events and Host spliced in) and the Herd's data.
fn fixture() -> (String, String) {
    let cfg = format!("models = [\"opus\", \"sonnet\", \"claude-haiku-4-5-20251001\"]\n\n{CONFIG}");
    let c = ctx(&tmpdir("ui-audit"), &cfg);
    for (issue, agent, harness) in [(12, "sendmeter-impl-12", "claude"), (13, "sendmeter-impl-13", "claude")] {
        append(
            &c.state,
            &Event::new(
                900_000,
                "sendmeter",
                Some(issue),
                "spawn",
                json!({"role": "impl", "agent": agent, "harness": harness}),
            ),
        )
        .unwrap();
    }
    assert_eq!(
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", "18", "19", "20"]).0,
        0
    );
    let sh = FakeShed::new(); // #771: the lanes live on shed
    sh.attach(&c);
    for (n, st) in [("sendmeter-impl-12", "working"), ("sendmeter-impl-13", "blocked")] {
        sh.lane(n, st, "/x");
        sh.screen(n, MENU);
    }
    let f = Fake::default();
    let r = sh.on(&f);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let page = req(&s, "GET", "/?all=1", "", &ck);
    // #60: the parts the live page loads on demand, spliced in as the page shows them once loaded. #268: the board's
    // Queue tab; #269: the needs cards, Lanes and [chat] as the board draws them for this paired phone; #266, #267,
    // #440: its Events, Host and System parts (a fleet's [config] in Fleets), without the board (it would read this
    // file's base server).
    let html = with_board(&page[page.find("<!doctype").unwrap()..], &s, ADDR, &ck, T);
    let parts = system_frags(&s, ADDR, T, &events_tab(&s, ADDR, T, "{}").unwrap_or_default());
    let html = html[..html.find("<script type=module").unwrap()].to_string() + &parts + "</body></html>\n";
    assert!(
        html.contains("▲ up") && html.contains("class=pick") && html.contains("<select name=model"),
        "fixture has arrows, a menu and pickers"
    );
    let herd = req(&s, "GET", "/herd.json?weather=calm", "", "");
    (html, herd[herd.find("\r\n\r\n").unwrap() + 4..].to_string())
}

#[test]
fn every_control_passes_the_audit_at_390px() {
    if std::process::Command::new("swift").arg("--version").output().is_err() || !cfg!(target_os = "macos") {
        eprintln!("skipping: no swift/WebKit here");
        return;
    }
    let (html, herd) = fixture();
    let d = tmpdir("ui-audit-png");
    let file = d.join("page.html");
    std::fs::write(&file, html).unwrap();
    let stub = "window.fetch = async () => new Response('no pane output', {status: 404})";
    let config = "config(document.querySelector('.cfg[data-f=\"sendmeter\"]'))";
    let scenes = [
        // The header (`[↻]` and the horse) is in every tab's frame and audit.
        ("ui-queue", "queue", "body", String::new()),
        ("ui-config", "queue", "#sys", config.to_string()),
        ("ui-lanes", "lanes", "body", String::new()),
        ("ui-host", "queue", "#sys", "sysopen('hosts')".to_string()),
        ("ui-events", "queue", "#sys", "sysopen('activity')".to_string()),
        ("ui-providers", "queue", "#sys", "sysopen('providers')".to_string()),
        (
            "ui-notifications",
            "queue",
            "#sys",
            "sysopen('notifications')".to_string(),
        ),
        (
            "ui-orchestrator",
            "queue",
            "#sys",
            "sysopen('orchestrator')".to_string(),
        ),
        ("ui-appearance", "queue", "#sys", "sysopen('appearance')".to_string()),
        (
            "ui-sheet",
            "lanes",
            "#sheet",
            "open(document.querySelector('.lane[data-agent=\"sendmeter-impl-12\"]'))".to_string(),
        ),
        ("ui-herd", "lanes", "#herd", format!("herd(true); herdData({herd})")),
        ("ui-chat-open", "lanes", "#sheet", chat_scene(true)),
        ("ui-chat-collapsed", "lanes", "#sheet", chat_scene(false)),
    ];
    let audit = include_str!("ui/audit.js");
    for (name, tab, root, then) in scenes {
        // The audit scrolls and opens rows, so the scene is set up again for the screenshot.
        // #138 (the chat scenes): no sideways scroll inside the chat or a panel, no chat text but 11 or 13 px; the blocks drawn.
        let fit = "const c = document.querySelector('.chat'); window.measure.push(c.scrollWidth - c.clientWidth \
            + [...c.querySelectorAll('pre, .blk')].filter(e => e.scrollWidth > e.clientWidth).length, \
            [...c.querySelectorAll('*')].filter(e => e.childNodes.length && [...e.childNodes].some(n => n.nodeType === 3 && n.textContent.trim()) && !['11px', '13px'].includes(getComputedStyle(e).fontSize)).length, c.querySelectorAll('.blk, .qs').length, c.querySelectorAll('.blk[open]').length)";
        let js = format!("{stub}; {audit}; {then}; window.measure = [audit('{root}')]; tab('{tab}'); {then}; {fit}");
        let png = std::env::var("HERDSMAN_SNAPSHOTS").map_or(d.join(format!("{name}.png")), |p| {
            std::path::Path::new(&p).join(format!("{name}.png"))
        });
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/webview.swift");
        let o = swift()
            .args([
                script,
                file.to_str().unwrap(),
                "390",
                tab,
                "horse",
                png.to_str().unwrap(),
                &js,
            ])
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&o.stdout);
        let v: Vec<f64> = out.split_whitespace().filter_map(|n| n.parse().ok()).collect();
        assert!(
            o.status.success() && v.len() == 7,
            "{name}: {out} {}",
            String::from_utf8_lossy(&o.stderr)
        );
        if name.starts_with("ui-chat") {
            let open = if name == "ui-chat-open" { 7.0 } else { 0.0 };
            assert_eq!(
                (v[3], v[4], v[5], v[6]),
                (0.0, 0.0, 10.0, open),
                "{name}: chat overflow, text not 11/13 px, blocks, open blocks"
            );
        }
        assert!(
            v[0] <= 390.0 && v[1] == 390.0,
            "{name}: scrollWidth {} at {}",
            v[0],
            v[1]
        );
        assert!(v[2] > 0.0, "{name}: no controls audited");
    }
}

/// #200: WebKit (the engine of iOS Safari) closing each sheet: the page above it is only ever the page as it was,
/// dimmed evenly by the fading backdrop, never another colour (iOS flashed it grey: `.out`, the chat blocks' box, on #sheet).
#[test]
fn closing_a_sheet_never_repaints_the_page() {
    if std::process::Command::new("swift").arg("--version").output().is_err() || !cfg!(target_os = "macos") {
        eprintln!("skipping: no swift/WebKit here");
        return;
    }
    let (html, _) = fixture();
    // #244: `judge` reads the backdrop as the page times k, a black scrim: github-dark's (farm's is tinted).
    let mut html = html.replace("\"farm\":\"farm\"", "\"farm\":\"github-dark\"");
    // #450: the page's clock is now, as a live page's is: the fixture's 1970 one shows the strip's `as of …d ago`
    // until the first paint, and its going would move the page mid-close.
    let i = html.find("<body data-ts=").unwrap() + 14;
    let j = i + html[i..].find(' ').unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    html.replace_range(i..j, &now.as_secs().to_string());
    let file = tmpdir("ui-close").join("page.html");
    std::fs::write(&file, html).unwrap();
    let stub = "window.fetch = async () => new Response('no pane output', {status: 404})";
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/webview.swift");
    let mut bad = vec![];
    for (name, tab, open, shut) in [
        (
            "chat",
            "lanes",
            "open(document.querySelector('.lane[data-agent=\"sendmeter-impl-12\"]'))",
            "close()",
        ),
        // #440: a fleet's [config] is System's Fleets row; System covers the page and its close uncovers it.
        (
            "config",
            "queue",
            "config(document.querySelector('.cfg[data-f=\"sendmeter\"]'))",
            "sysshut()",
        ),
        ("legend", "queue", "legend()", "close()"),
    ] {
        let png = std::env::var("HERDSMAN_SNAPSHOTS").map_or(file.with_file_name(format!("close-{name}.png")), |p| {
            std::path::Path::new(&p).join(format!("close-{name}.png"))
        });
        let o = swift()
            .args([
                script,
                file.to_str().unwrap(),
                "390",
                tab,
                "horse",
                png.to_str().unwrap(),
                &format!("{stub}; {open}"),
                shut,
            ])
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&o.stdout);
        let v: Vec<f64> = out.split_whitespace().filter_map(|n| n.parse().ok()).collect();
        assert!(
            o.status.success() && v.len() == 3 && v[0] >= 5.0,
            "{name}: {out} {}",
            String::from_utf8_lossy(&o.stderr)
        );
        if v[1] > 0.02 {
            bad.push(format!(
                "{name}: a frame has {:.1} % of the page off its colour",
                v[1] * 100.0
            ));
        }
        if v[2].abs() > 0.02 {
            bad.push(format!(
                "{name}: the page is {:.1} % dimmed after the close",
                v[2] * 100.0
            ));
        }
    }
    assert!(bad.is_empty(), "{bad:?}");
}

/// #206: a reload on Events painted the Queue tab (and `[?]`) with squished rows until serve.js ran `tab()`. WebKit with
/// `localStorage.tab` set: the first painted frame (the page with its scripts cut, as it paints before they run) shows
/// that tab's section alone, and the Queue rows (row height, `#n` column width) lay out as they settle.
#[test]
fn the_first_paint_is_the_stored_tab_and_the_settled_layout() {
    if std::process::Command::new("swift").arg("--version").output().is_err() || !cfg!(target_os = "macos") {
        eprintln!("skipping: no swift/WebKit here");
        return;
    }
    let (html, _) = fixture();
    let d = tmpdir("ui-first-paint");
    let (settled, first) = (d.join("settled.html"), d.join("first.html"));
    std::fs::write(&settled, &html).unwrap();
    std::fs::write(
        &first,
        format!("{}</body></html>", &html[..html.find("<script src=").unwrap()]),
    )
    .unwrap();
    // Shown or not: Queue, Lanes, Chat, [?]; then each Queue row's height and `#n` width.
    let measure = "window.measure = [...['#queue', '#lanes', '#chat', 'header .qhelp'].map(s => getComputedStyle(document.querySelector(s)).display === 'none' ? 0 : 1), \
        ...[...document.querySelectorAll('details.q')].flatMap(d => [d.querySelector('summary').getBoundingClientRect().height, d.querySelector('summary>span').getBoundingClientRect().width])]";
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/webview.swift");
    let shot = |tab: &str, frame: &str, file: &std::path::Path| {
        let png = std::env::var("HERDSMAN_SNAPSHOTS").map_or(d.join(format!("first-{tab}-{frame}.png")), |p| {
            std::path::Path::new(&p).join(format!("first-{tab}-{frame}.png"))
        });
        let o = swift()
            .env("HERDSMAN_TAB", tab)
            .args([
                script,
                file.to_str().unwrap(),
                "390",
                "-",
                "horse",
                png.to_str().unwrap(),
                measure,
            ])
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&o.stdout);
        let v: Vec<f64> = out.split_whitespace().filter_map(|n| n.parse().ok()).collect();
        assert!(
            o.status.success() && v.len() > 7,
            "{tab} {frame}: {out} {}",
            String::from_utf8_lossy(&o.stderr)
        );
        v[2..].to_vec()
    };
    // #440: a stored `events` (or `host`) tab, gone to System, paints Queue from the first frame on.
    for (tab, shown) in [
        ("lanes", [0.0, 1.0, 0.0, 0.0]),
        ("events", [1.0, 0.0, 0.0, 1.0]),
        ("host", [1.0, 0.0, 0.0, 1.0]),
        ("queue", [1.0, 0.0, 0.0, 1.0]),
    ] {
        let (a, b) = (shot(tab, "first", &first), shot(tab, "settled", &settled));
        assert_eq!(
            a[..4],
            shown,
            "{tab}: the first frame's sections (queue lanes chat [?])"
        );
        assert_eq!(b[..4], shown, "{tab}: the settled sections");
        assert_eq!(
            a, b,
            "{tab}: first paint vs settled (sections, then row height and #n width per Queue row)"
        );
    }
}
