mod common;
use common::*;
use drover::events::{append, Event};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

const PRELUDE: &str = "const $ = s => document.querySelector(s), $$ = s => [...document.querySelectorAll(s)]; \
    const w = ms => new Promise(r => setTimeout(r, ms)); \
    const until = async f => { for (let i = 0; i < 100 && !f(); i++) await w(100); return !!f(); };";

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

fn page(name: &str, seed: impl FnOnce(&drover::Ctx)) -> (&'static drover::Ctx, String) {
    let c = ctx(
        &tmpdir(name),
        &format!("{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\n"),
    );
    seed(&c);
    let (c, r): (&'static drover::Ctx, &'static Fake) = (Box::leak(Box::new(c)), Box::leak(Box::default()));
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", l.local_addr().unwrap());
    std::thread::spawn(move || drover::serve::serve_on(c, r, l, false));
    (c, url)
}

fn ev(c: &drover::Ctx, ts: u64, issue: Option<u64>, kind: &str, d: Value) {
    append(&c.state, &Event::new(ts, "sendmeter", issue, kind, d)).unwrap();
}

fn late(url: &str, name: &str, js: &str) -> Vec<f64> {
    late_as(url, name, js, "")
}

fn late_as(url: &str, name: &str, js: &str, cookie: &str) -> Vec<f64> {
    let png = tmpdir(name).join("page.png");
    let o = webview()
        .env("DROVER_COOKIE", cookie)
        .args([url, "390", "-", "-", png.to_str().unwrap()])
        .arg(format!("{PRELUDE} window.late = (async () => {{ {js} }})()"))
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(o.status.success(), "{out} {}", String::from_utf8_lossy(&o.stderr));
    out.split_whitespace().skip(2).filter_map(|n| n.parse().ok()).collect()
}

#[test]
fn parity_r25_a_sheet_closes_without_a_flash() {
    if no_webkit() {
        return;
    }
    let (_, url) = page("parity-r25", |_| {});
    let png = tmpdir("parity-r25-png").join("close.png");
    let o = webview()
        .args([&url, "390", "-", "-", png.to_str().unwrap()])
        .arg("document.querySelector('#sysb').click()")
        .arg("document.querySelector('#sheet .sh button').click()")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    let v: Vec<f64> = out.split_whitespace().filter_map(|n| n.parse().ok()).collect();
    assert!(
        o.status.success() && v.len() == 3,
        "{out} {}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(
        v[0] >= 3.0 && v[1] < 0.02 && v[2].abs() < 0.02,
        "frames, worst frame's off share, dim left: {v:?} (worst frame: {})",
        png.display()
    );
}

#[test]
fn parity_r5_a_slow_load_shows_the_strip_after_300ms_and_the_page_stays_tappable() {
    if no_webkit() {
        return;
    }
    let (_, url) = page("parity-r5", |_| {});
    let js = "await until(() => $('main .scr')); \
        const f = window.fetch; window.fetch = (u, o) => w(1500).then(() => f(u, o)); \
        $('#rf').click(); \
        await w(150); const early = +!$('#ld').hidden; \
        await w(350); const shown = +!$('#ld').hidden; \
        const m = $('main .scr').getBoundingClientRect(), hit = document.elementFromPoint(m.left + m.width / 2, m.top + 8); \
        const tap = +!!(hit && $('main').contains(hit)); \
        await w(1800); return [early, shown, tap, +$('#ld').hidden];";
    assert_eq!(
        late(&url, "parity-r5-png", js),
        [0.0, 1.0, 1.0, 1.0],
        "strip at 150 ms, at 500 ms, the page under it tappable, strip gone once loaded"
    );
}

#[test]
#[ignore = "red until #1084: a live update repaints every node"]
fn parity_2_a_live_update_never_moves_the_scroll_or_replaces_a_kept_node() {
    if no_webkit() {
        return;
    }
    let t = now();
    let (c, url) = page("parity-2", |c| {
        for n in 1..=60 {
            ev(c, t - 7_200 + n * 60, Some(n), "merged", json!({"pr": 100 + n}));
        }
    });
    std::thread::spawn(move || {
        for n in 61..=90 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            ev(c, now(), Some(n), "merged", json!({"pr": 100 + n}));
        }
    });
    let js = "location.hash = '#events'; \
        const rows = () => $$('main section:not([hidden]) .row'); \
        await until(() => rows().length >= 40); \
        scrollTo(0, 600); await w(300); \
        const n0 = rows().length, kept = rows()[30], y0 = kept.getBoundingClientRect().top; \
        await until(() => rows().length > n0); \
        return [+(rows().length > n0), +kept.isConnected, Math.round(kept.getBoundingClientRect().top - y0)];";
    assert_eq!(
        late(&url, "parity-2-png", js),
        [1.0, 1.0, 0.0],
        "a row arrived, the kept row is the same node, and it did not move"
    );
}

#[test]
fn done_today_opens_the_list_a_row_opens_its_issue_card_and_back_returns() {
    if no_webkit() {
        return;
    }
    let t = now();
    let (_, url) = page("done-today", |c| {
        ev(
            c,
            t - 600,
            Some(9),
            "merged",
            json!({"pr": "41", "merge_sha": "e".repeat(40)}),
        );
    });
    let js = "const done = '.row[data-id=\"done-today-sendmeter\"]', row = '.row[data-id^=\"done-9-\"]'; \
        await until(() => $(done)); $(done).click(); \
        const list = await until(() => $(row)); \
        if (list) $(row).click(); \
        const card = await until(() => !$('#sheet').hidden && $('#sheet .sh b')?.textContent.startsWith('#9')); \
        if (card) $('#sheet .sh button').click(); \
        const shut = await until(() => $('#sheet').hidden && $(row)); \
        const bk = $$('main button').find(b => b.textContent.includes('back')); \
        if (bk) bk.click(); \
        const back = await until(() => location.hash === '#queue' && $(done) && !$(row)); \
        return [+list, +card, +shut, +back];";
    assert_eq!(
        late(&url, "done-today-png", js),
        [1.0, 1.0, 1.0, 1.0],
        "the list, #9's card, the card closed over the list, back on the queue"
    );
}
