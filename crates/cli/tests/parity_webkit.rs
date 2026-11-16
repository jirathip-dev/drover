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
    let profile = c.config.with_file_name("claude").join("fleet-orchestrator");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
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

fn thread(c: &drover::Ctx, messages: u64, files: &Value) {
    let t = now();
    ev(
        c,
        t - 4_000,
        None,
        "thread-new",
        json!({"thread": "t1", "title": "Search worker split", "harness": "claude"}),
    );
    for i in 0..messages {
        let from = if i % 2 == 0 { "owner" } else { "orchestrator" };
        let text = format!("message {i}: the search worker splits into a reader and a writer, one line each way");
        ev(
            c,
            t - 3_000 + i,
            None,
            "thread-msg",
            json!({"thread": "t1", "from": from, "text": text}),
        );
    }
    if files != &json!([]) {
        let d = json!({"thread": "t1", "from": "orchestrator", "text": "the screenshot", "files": files});
        ev(c, t - 1_000, None, "thread-msg", d);
    }
}

fn late(url: &str, name: &str, js: &str) -> Vec<f64> {
    let png = tmpdir(name).join("page.png");
    let o = webview()
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
fn parity_24_opening_a_thread_pushes_history_and_back_returns_to_the_list() {
    if no_webkit() {
        return;
    }
    let (_, url) = page("parity-24", |c| thread(c, 4, &json!([])));
    let js = "location.hash = '#threads'; \
        await until(() => $('.row[data-id^=\"t\"]')); \
        const h0 = history.length; $('.row[data-id^=\"t\"]').click(); \
        await until(() => $('.form.say')); \
        const pushed = +(history.length > h0); \
        if (pushed) history.back(); \
        const back = pushed && await until(() => $('.row[data-id^=\"t\"]') && !$('.form.say')); \
        return [pushed, +back];";
    assert_eq!(
        late(&url, "parity-24-png", js),
        [1.0, 1.0],
        "a history entry, and back shows the list"
    );
}

#[test]
fn parity_r4_in_a_thread_only_the_messages_scroll_and_the_composer_sits_at_the_bottom() {
    if no_webkit() {
        return;
    }
    let (_, url) = page("parity-r4", |c| thread(c, 40, &json!([])));
    let js = "location.hash = '#threads'; \
        await until(() => $('.row[data-id^=\"t\"]')); $('.row[data-id^=\"t\"]').click(); \
        await until(() => $('.form.say')); await w(300); \
        const se = document.scrollingElement, page = +(se.scrollHeight > se.clientHeight + 1); \
        const msg = $('[data-id^=\"m-\"]'); let box = msg && msg.parentElement; \
        while (box && !(box.scrollHeight > box.clientHeight + 1 && /auto|scroll/.test(getComputedStyle(box).overflowY))) box = box.parentElement; \
        const say = $('.form.say').getBoundingClientRect(); \
        return [page, +!!box, +(say.bottom <= innerHeight + 1 && say.bottom >= innerHeight - 120)];";
    assert_eq!(
        late(&url, "parity-r4-png", js),
        [0.0, 1.0, 1.0],
        "the page does not scroll, the messages do, the composer is at the bottom"
    );
}

#[test]
fn parity_29_a_long_thread_shows_its_last_30_and_older_ones_load_on_scroll() {
    if no_webkit() {
        return;
    }
    let (_, url) = page("parity-29", |c| thread(c, 40, &json!([])));
    let js = "location.hash = '#threads'; \
        await until(() => $('.row[data-id^=\"t\"]')); $('.row[data-id^=\"t\"]').click(); \
        await until(() => $('.form.say')); await w(300); \
        const msgs = () => $$('[data-id^=\"m-\"]:not([data-id$=\"-t\"])').length; \
        const n0 = msgs(); \
        let box = $('[data-id^=\"m-\"]').parentElement; \
        while (box && !(box.scrollHeight > box.clientHeight + 1)) box = box.parentElement; \
        (box ?? document.scrollingElement).scrollTop = 0; scrollTo(0, 0); \
        await until(() => msgs() > n0); \
        return [n0, msgs()];";
    assert_eq!(
        late(&url, "parity-29-png", js),
        [30.0, 40.0],
        "the last 30 messages, then all 40 once scrolled to the top"
    );
}

#[test]
fn parity_20_a_tapped_image_opens_fitted_in_the_viewer() {
    if no_webkit() {
        return;
    }
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89\0\0\0\rIDATx\x9cc\0\x01\0\0\x05\0\x01\r\n-\xb4\0\0\0\0IEND\xaeB`\x82";
    let (_, url) = page("parity-20", |c| {
        let dir = c.state.with_file_name("orchestrator").join("t1/files");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("shot.png"), png).unwrap();
        let path = dir.join("shot.png").to_string_lossy().into_owned();
        thread(c, 2, &json!([{"name": "shot.png", "path": path, "size": png.len()}]));
    });
    let js = "location.hash = '#threads'; \
        await until(() => $('.row[data-id^=\"t\"]')); $('.row[data-id^=\"t\"]').click(); \
        await until(() => $('.form.say')); \
        const img = await until(() => $('main img')) && $('main img'); \
        if (img) img.click(); \
        const big = () => $$('img').some(i => i !== img && i.getBoundingClientRect().width >= innerWidth * 0.9 && i.naturalWidth > 0); \
        return [+!!img, +(await until(big))];";
    assert_eq!(
        late(&url, "parity-20-png", js),
        [1.0, 1.0],
        "the message's image is drawn, and a tap opens it fitted to the screen"
    );
}

const CODE: &str = "See t2, https://example.com/a and **this**:\n```sh\nif [ 1 < 2 ]; then\n  echo \"b & c\"\nfi\n```";

fn coded(name: &str) -> String {
    page(name, |c| {
        thread(c, 2, &json!([]));
        let d = json!({"thread": "t1", "from": "orchestrator", "text": CODE});
        ev(c, now() - 500, None, "thread-msg", d);
    })
    .1
}

const OPEN: &str = "location.hash = '#threads'; \
    await until(() => $('.row[data-id^=\"t\"]')); $('.row[data-id^=\"t\"]').click(); \
    await until(() => $('.form.say')); await w(300); \
    const card = $$('.card.k-orch').pop(), copied = []; \
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: t => (copied.push(t), Promise.resolve()) } });";

#[test]
fn parity_18_a_message_links_its_urls_and_thread_refs_and_keeps_its_markdown() {
    if no_webkit() {
        return;
    }
    let js = format!(
        "{OPEN} return [+!!card.querySelector('a[href=\"https://example.com/a\"]'), \
        +!!card.querySelector('a[data-thread=\"t2\"]'), +(card.querySelector('.txt b')?.textContent === 'this')];"
    );
    assert_eq!(
        late(&coded("parity-18"), "parity-18-png", &js),
        [1.0, 1.0, 1.0],
        "a plain URL and a t<n> ref are links, bold stays bold"
    );
}

#[test]
fn parity_19_a_fenced_block_copies_its_exact_text() {
    if no_webkit() {
        return;
    }
    let want = serde_json::to_string("if [ 1 < 2 ]; then\n  echo \"b & c\"\nfi").unwrap();
    let js = format!(
        "{OPEN} const b = card.querySelector('.cb .cpy'); b.click(); await w(100); \
        return [+!!b, +(copied[0] === {want}), +(b.textContent === 'copied')];"
    );
    assert_eq!(
        late(&coded("parity-19"), "parity-19-png", &js),
        [1.0, 1.0, 1.0],
        "the [copy] puts the block's exact text on the clipboard and says so"
    );
}

#[test]
fn parity_28_a_long_press_offers_copy_copy_as_markdown_and_quote() {
    if no_webkit() {
        return;
    }
    let want = serde_json::to_string(CODE).unwrap();
    let js = format!(
        "{OPEN} const press = () => card.querySelector('.txt').dispatchEvent(new MouseEvent('contextmenu', {{ bubbles: true, cancelable: true }})); \
        const pick = l => [...$$('#sheet button')].find(b => b.textContent === l); \
        press(); const labels = ['copy', 'copy as markdown', 'quote'].map(l => +!!pick(l)); \
        pick('copy as markdown').click(); await w(100); press(); pick('quote').click(); \
        return [...labels, +(copied[0] === {want}), +$('.form.say input').value.startsWith('> See t2'), +$('#sheet').hidden];"
    );
    assert_eq!(
        late(&coded("parity-28"), "parity-28-png", &js),
        [1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        "the menu's three items; copy as markdown copies the raw text; quote fills the composer"
    );
}
