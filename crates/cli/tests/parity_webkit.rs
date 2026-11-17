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
fn parity_24_opening_a_thread_pushes_history_and_back_returns_to_the_list() {
    if no_webkit() {
        return;
    }
    let (_, url) = page("parity-24", |c| thread(c, 4, &json!([])));
    let js = "location.hash = '#threads'; \
        await until(() => $('.row[data-id^=\"t\"]')); \
        const h0 = history.length; $('.row[data-id^=\"t\"]').click(); \
        await until(() => $('.cmp')); \
        const pushed = +(history.length > h0); \
        if (pushed) history.back(); \
        const back = pushed && await until(() => $('.row[data-id^=\"t\"]') && !$('.cmp')); \
        return [pushed, +back];";
    assert_eq!(
        late(&url, "parity-24-png", js),
        [1.0, 1.0],
        "a history entry, and back shows the list"
    );
}

#[test]
fn parity_24_an_edge_swipe_past_40_percent_goes_back_to_the_list_where_it_was_and_a_short_one_stays() {
    if no_webkit() {
        return;
    }
    let (_, url) = page("parity-24-swipe", |c| {
        thread(c, 4, &json!([]));
        for n in 2..=40 {
            let d = json!({"thread": format!("t{n}"), "title": format!("Thread {n}"), "harness": "claude"});
            ev(c, now() - 5_000 + n, None, "thread-new", d);
        }
    });
    let js = "location.hash = '#threads'; \
        await until(() => $('.card[data-id^=\"older-\"] > .fold')); $('.card[data-id^=\"older-\"] > .fold').click(); \
        await until(() => $$('.row[data-id^=\"t\"]').length >= 30); \
        scrollTo(0, 400); await w(100); const y0 = scrollY; \
        $('.row[data-id=\"t1\"]').click(); \
        await until(() => $('.cmp')); await w(300); \
        const touch = (type, x) => { const at = document.elementFromPoint(5, 400) ?? document.body, p = { identifier: 1, target: at, clientX: x, clientY: 400 }; \
          const list = a => Object.assign(a, { item: i => a[i] ?? null }), e = new Event(type, { bubbles: true, cancelable: true }); \
          Object.defineProperties(e, { touches: { value: list(type === 'touchend' ? [] : [p]) }, changedTouches: { value: list([p]) } }); at.dispatchEvent(e); }; \
        const moved = () => $$('main > section:not([hidden]) > *').some(k => k.style.transform.includes('translateX')); \
        const drag = to => { touch('touchstart', 8); for (let x = 8; x <= to; x += 20) touch('touchmove', x); const f = +moved(); touch('touchend', to); return f; }; \
        const follows = drag(innerWidth * 0.3); await w(400); \
        const stayed = +(!!$('.cmp') && !moved()); \
        drag(innerWidth * 0.6); \
        const back = await until(() => $('.row[data-id^=\"t\"]') && !$('.cmp')); await w(200); \
        return [scrollY > 0 ? 1 : 0, follows, stayed, +back, +!moved(), Math.round(scrollY - y0)];";
    assert_eq!(
        late(&url, "parity-24-swipe-png", js),
        [1.0, 1.0, 1.0, 1.0, 1.0, 0.0],
        "the list scrolled, the view follows the finger, a 30% drag stays, a 60% drag goes back, no slide left, the list where it was"
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
        await until(() => $('.cmp')); await w(300); \
        const se = document.scrollingElement, page = +(se.scrollHeight > se.clientHeight + 1); \
        const msg = $('[data-id^=\"m-\"]'); let box = msg && msg.parentElement; \
        while (box && !(box.scrollHeight > box.clientHeight + 1 && /auto|scroll/.test(getComputedStyle(box).overflowY))) box = box.parentElement; \
        const say = $('.cmp').getBoundingClientRect(); \
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
        await until(() => $('.cmp')); await w(300); \
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
        await until(() => $('.cmp')); \
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
    await until(() => $('.cmp')); await w(300); \
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
        return [...labels, +(copied[0] === {want}), +$('.cmp textarea').value.startsWith('> See t2'), +$('#sheet').hidden];"
    );
    assert_eq!(
        late(&coded("parity-28"), "parity-28-png", &js),
        [1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        "the menu's three items; copy as markdown copies the raw text; quote fills the composer"
    );
}

fn handoffs(c: &drover::Ctx) {
    let t = now();
    let new = |ts, id: &str, title: &str| {
        ev(
            c,
            ts,
            None,
            "thread-new",
            json!({"thread": id, "title": title, "harness": "claude"}),
        );
    };
    new(t - 4_000, "t1", "Search worker split");
    new(t - 3_900, "t2", "Rate limits");
    let d = json!({"thread": "t1", "from": "t1", "to": "t2", "title": "Burst size", "text": "burst 100, not 50",
        "target": "Rate limits · t2"});
    ev(c, t - 3_000, None, "thread-handoff", d);
    let d = json!({"thread": "t2", "from": "t1", "text": "handoff to Rate limits · t2: burst 100, not 50"});
    ev(c, t - 2_990, None, "thread-msg", d);
    let d = json!({"thread": "t1", "from": "system", "via": "t2", "text": "filed #52: cap bursts at 100"});
    ev(c, t - 2_000, None, "thread-msg", d);
    let card = |id: &str, title: &str| {
        let opts = [
            "Rate limits · t2 — burst 100, not 50 (33m ago)".to_string(),
            format!("new thread: {title}"),
        ];
        json!({"id": id, "question": format!("Hand off \"{title}\" to which sendmeter thread?"), "options": opts,
            "kind": "owner", "thread": "t1", "handoff": {"repo": "sendmeter", "title": title, "text": "do it", "targets": ["t2", "new"]}})
    };
    ev(c, t - 1_500, None, "needs-decision", card("q2", "Old index"));
    let d = json!({"id": "q2", "to": "t2", "reason": "t2's orchestrator is not reachable"});
    ev(c, t - 1_400, None, "ask-failed", d);
    ev(c, t - 1_000, None, "needs-decision", card("q1", "Index rebuild"));
}

#[test]
fn parity_35_handoff_chips_flow_back_lines_and_a_pick_a_thread_tap_that_delivers_once() {
    if no_webkit() {
        return;
    }
    let (c, url) = page("parity-35", handoffs);
    let f = Fake::default();
    let ck = pair(
        &drover::serve::Server::new(c, &f, Some("127.0.0.1:7777".into())),
        c,
        "Mozilla/5.0 (iPhone)",
        now(),
    );
    let js = "location.hash = '#threads'; \
        await until(() => $('.row[data-id=\"t1\"]')); $('.row[data-id=\"t1\"]').click(); \
        await until(() => $('.cmp')); await w(300); \
        const card = t => $$('main .card').find(k => k.textContent.includes(t)); \
        const chip = (k, l) => !!k && [...k.querySelectorAll('button')].some(b => b.textContent === l); \
        const sent = chip(card('Burst size'), 'continued in t2'), back = chip(card('filed #52'), 'from t2'); \
        const bad = card('Old index'); \
        const failed = !!bad && bad.classList.contains('bad') && bad.textContent.includes(\"not delivered: t2's orchestrator is not reachable\"); \
        const picks = $$('main button[name=choice]').map(b => b.textContent); \
        const rows = +(picks.some(p => p.startsWith('Rate limits · t2')) && picks.includes('new thread: Index rebuild')); \
        const tap = $$('main button[name=choice]').find(b => b.textContent === 'new thread: Index rebuild'); tap.click(); \
        const landed = await until(() => chip(card('Index rebuild'), 'continued in t3') && !$('main button[name=choice]')); \
        tap.click(); await w(800); \
        [...card('Burst size').querySelectorAll('button')].find(b => b.textContent === 'continued in t2').click(); \
        const from = await until(() => { const k = $$('main .card.k-orch').find(k => k.textContent.includes('burst 100')); \
            return !!k && k.querySelector('.ch b')?.textContent === 'from t1' && chip(k, 'from t1'); }); \
        return [+sent, +back, +failed, rows, +landed, +from];";
    assert_eq!(
        late_as(&url, "parity-35-png", js, ck.trim().trim_start_matches("Cookie: ")),
        [1.0, 1.0, 1.0, 1.0, 1.0, 1.0],
        "the sent row's chip, the flow-back row's chip, the failed card's error, the pick rows, the tap lands in a new t3, \
         the target shows the message from t1 with its chip"
    );
    let log = drover::events::read(&c.state).unwrap();
    let sent = log
        .iter()
        .filter(|e| e.kind == "thread-handoff" && e.str("ask") == Some("q1"));
    let made = log.iter().filter(|e| e.kind == "thread-new");
    let picked = log.iter().filter(|e| e.kind == "decision" && e.str("id") == Some("q1"));
    assert_eq!(
        (sent.count(), made.count(), picked.count()),
        (1, 3, 1),
        "two taps deliver once: one handoff, one new thread, one decision"
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
