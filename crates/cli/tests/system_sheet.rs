//! #440: the tab bar is queue · lanes · chat; [⚙] on every tab opens the System sheet, its dot red while a host card is
//! red; every former Host, Events, [config] and [◐] control renders inside its section; the old links (`#host`,
//! `#events`, `#config/<fleet>`, a stored `host` tab, a Queue card's [config]) land on `#system/<section>`. The real
//! page in WebKit (tests/support/webview.swift), the board's parts painted as main.ts paints them (`system_frags`).
mod common;
use common::*;
use drover::serve::Server;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

/// The live, paired page with its Queue tab and System's parts; `red`: the main host's disk under its floor.
fn page(name: &str, red: bool) -> std::path::PathBuf {
    let cfg = format!("{CONFIG}\n[accounts]\nA = \"CLAUDE_CODE_OAUTH_TOKEN_A\"\n");
    let c = ctx(&tmpdir(name), &cfg);
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "18"]).0, 0);
    let df = if red {
        "F\n/dev/disk1 41943040 1 4194304\n"
    } else {
        "F\n/dev/disk1 41943040 1 31457280\n"
    };
    let r = Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply("df -k /", df);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let res = String::from_utf8_lossy(&s.respond(
        &format!(
            "GET /?all=1 HTTP/1.1\r\nHost: {ADDR}\r\n{}\r\n",
            pair(&s, &c, "Mozilla/5.0 (iPhone)", T)
        ),
        T,
    ))
    .into_owned();
    let html = with_queue(&res[res.find("<!doctype").unwrap()..], &s, ADDR, T);
    let parts = system_frags(&s, ADDR, T, &events_tab(&s, ADDR, T, "{}").unwrap_or_default());
    let html = html[..html.find("<script type=module").unwrap()].to_string() + &parts + "</body></html>\n";
    let file = tmpdir(&format!("{name}-page")).join("page.html");
    std::fs::write(&file, html).unwrap();
    file
}

/// `js`'s `window.measure` (or `window.late`, read 1 s on) on `file` at 390 px; `pre` runs before the page's own head
/// script, `stored` is the stored tab.
fn measure(file: &std::path::Path, pre: &str, stored: &str, js: &str) -> Vec<f64> {
    let (png, run) = (file.with_extension("png"), file.with_extension("run.html"));
    let html = std::fs::read_to_string(file).unwrap();
    std::fs::write(
        &run,
        html.replacen(
            "<meta charset=utf-8>",
            &format!("<meta charset=utf-8><script>{pre}</script>"),
            1,
        ),
    )
    .unwrap();
    let o = webview()
        .env("DROVER_TAB", stored)
        .env("DROVER_WAIT", "1")
        .args([run.to_str().unwrap(), "390", "-", "horse", png.to_str().unwrap(), js])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{out} {}", String::from_utf8_lossy(&o.stderr));
    out.split_whitespace().skip(2).filter_map(|n| n.parse().ok()).collect()
}

#[test]
fn three_tabs_and_the_gear_on_each_with_the_dot_only_for_a_red_host() {
    if no_webkit() {
        return;
    }
    // The nav's links, then per tab: [⚙] shown, its dot shown; then no [◐], no host or events tab, and the dot's label.
    let js = "const nav = [...document.querySelectorAll('nav a')].map(a => a.hash).join(); \
        window.measure = [+(nav === '#queue,#lanes,#chat'), ...['queue', 'lanes', 'chat'].flatMap(t => { tab(t); \
          const b = document.querySelector('#sysb'); return [+b.checkVisibility(), +b.querySelector('.sd').checkVisibility()]; }), \
          +!document.querySelector('#look, nav a[href=\"#host\"], nav a[href=\"#events\"]'), \
          +document.querySelector('#sysb').getAttribute('aria-label').startsWith('system (host alert: this host'), \
          +document.querySelector('.sxi a[data-s=hosts] .sd').hidden]";
    let red = measure(&page("sys-red", true), "", "queue", js);
    assert_eq!(
        red,
        [1., 1., 1., 1., 1., 1., 1., 1., 1., 0.],
        "red host: the dot on every tab"
    );
    let calm = measure(&page("sys-calm", false), "", "queue", js);
    assert_eq!(calm, [1., 1., 0., 1., 0., 1., 0., 1., 0., 1.], "no red host: no dot");
}

#[test]
fn every_former_control_is_in_its_section_and_the_gear_opens_at_agent_providers() {
    if no_webkit() {
        return;
    }
    let js = "document.querySelector('#sysb').click(); \
        const n = (s, q) => document.querySelectorAll(`#sy-${s} ${q}`).length, got = [+sysOn(), +(location.hash === '#system/providers'), \
          +(ytop.scrollTop === 0), +!!document.querySelector('.sxi a.on[data-s=providers]')]; \
        got.push(n('providers', '.pvc[data-p=anthropic] .pvr'), \
          n('fleets', '.sfl[data-f=sendmeter] .cg'), n('fleets', '.sfl select[name=model]'), n('fleets', '.sfl .pp .pg'), \
          n('hosts', '#host .hcd'), n('hosts', '#host>.grp'), n('hosts', '#host .su'), n('hosts', '#host [data-su=claude]'), \
          +(n('activity', '#events .er') > 0), +(n('activity', '#events .chips button') > 1), \
          n('notifications', '#push'), n('notifications', 'form.role2 [name=hash]'), n('notifications', '.cg'), \
          n('conductor', '.kv2 a.go[href=\"#system/fleets/sendmeter\"]'), \
          n('appearance', '[role=listbox]'), n('appearance', '.lko[data-t]'), \
          +!document.querySelector('main #host, main #events, #look'), \
          +![...document.querySelectorAll('#host .gn')].some(g => g.textContent === 'ALERTS'), \
          +![...document.querySelectorAll('#host .k')].some(k => ['accounts', 'deepseek'].includes(k.textContent))); \
        sysopen('appearance'); document.querySelector('#sy-appearance .lko[data-t=kitchen]').click(); \
        got.push(+(document.documentElement.dataset.theme === 'kitchen'), +sysOn()); \
        document.querySelector('.sxc').click(); got.push(+!sysOn(), +(location.hash === '#queue')); window.measure = got";
    let m = measure(&page("sys-all", true), "", "queue", &format!("{{ {js} }}"));
    assert_eq!(
        m,
        [1., 1., 1., 1., 1., 6., 3., 1., 1., 4., 1., 0., 1., 1., 1., 1., 4., 1., 3., 4., 1., 1., 1., 1., 1., 1., 1.],
        "{m:?}"
    );
}

#[test]
fn the_old_links_land_on_their_system_section() {
    if no_webkit() {
        return;
    }
    let file = page("sys-routes", false);
    let js = "const o = document.querySelector('.sfl[data-f=sendmeter]'); window.measure = [+(document.documentElement.dataset.tab === 'queue'), \
        +sysOn(), ['hosts', 'activity', 'fleets'].indexOf(document.querySelector('.sxi a.on')?.dataset.s), +!!o?.open, \
        ['#system/hosts', '#system/activity', '#system/fleets/sendmeter', ''].indexOf(location.hash)]";
    let go = |h: &str| format!("history.replaceState(null, '', '{h}')");
    assert_eq!(measure(&file, &go("#host"), "lanes", js), [1., 1., 0., 0., 0.], "#host");
    assert_eq!(
        measure(&file, &go("#events"), "chat", js),
        [1., 1., 1., 0., 1.],
        "#events"
    );
    assert_eq!(
        measure(&file, &go("#config/sendmeter"), "queue", js),
        [1., 1., 2., 1., 2.],
        "#config/<fleet>"
    );
    // A stored `host` tab: Queue, the sheet closed.
    assert_eq!(measure(&file, "", "host", js), [1., 0., -1., 0., 3.], "stored host tab");
    // A Queue card's [config], and a later `#events` link (hashchange) on an open page.
    let tap = format!("document.querySelector('#queue .cfg').click(); {js}");
    assert_eq!(measure(&file, "", "queue", &tap), [1., 1., 2., 1., 2.], "[config]");
    let later = js.replace(
        "window.measure = ",
        "location.hash = '#events'; setTimeout(() => { window.late = ",
    ) + " }, 300)";
    assert_eq!(
        measure(&file, "", "lanes", &later),
        [1., 1., 1., 0., 1.],
        "#events, later"
    );
}
