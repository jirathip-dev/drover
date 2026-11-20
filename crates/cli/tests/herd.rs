mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;
const SCREEN: &str = "⏺ Bash(cargo test)\n  ⎿ PANE-SECRET-TEXT ok\n";

fn fixture(name: &str) -> (drover::Ctx, OnShed) {
    fixture_at(name, "2", "3.90")
}

fn fixture_at(name: &str, pressure: &str, load: &str) -> (drover::Ctx, OnShed) {
    let cfg = format!("[serve]\nallowed_hosts = [\"mac.example\"]\n\n{CONFIG}\n[fleet.synergy]\nrepo = \"o/synergy-apps\"\nlocal = \"/p/synergy-apps\"\nintegration = \"staging\"\n");
    let c = ctx(&tmpdir(name), &cfg);
    let spawn = |fleet: &str, issue: u64, ts: u64, agent: &str| {
        append(
            &c.state,
            &Event::new(
                ts,
                fleet,
                Some(issue),
                "spawn",
                json!({"role": "impl", "agent": agent, "harness": "claude"}),
            ),
        )
        .unwrap();
    };
    spawn("sendmeter", 12, 900_100, "sendmeter-impl-12");
    spawn("synergy", 7, 900_400, "synergy-impl-7");
    spawn("synergy", 8, 900_500, "synergy-impl-8");
    for args in [
        &["queue", "sendmeter", "add", "18", "19", "20"][..],
        &["queue", "sendmeter", "block", "--on", "18", "19"],
    ] {
        assert_eq!(run(&c, &Fake::default(), args).0, 0);
    }
    let cache = c.state.parent().unwrap().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(
        cache.join("issue-sendmeter-12.json"),
        json!({"title": "Send meter readings nightly"}).to_string(),
    )
    .unwrap();
    std::fs::write(
        cache.join("issue-sendmeter-18.json"),
        json!({"title": "Herd mode"}).to_string(),
    )
    .unwrap();
    let r = OnShed::new(
        Fake::default()
            .reply(
                "sysctl -n kern.memorystatus_vm_pressure_level",
                &format!("{pressure}\n"),
            )
            .reply("sysctl -n vm.loadavg", &format!("{{ {load} 1.20 1.00 }}\n"))
            .reply("sysctl -n hw.ncpu", "10\n")
            .reply("vm_stat", "Swapouts: 100.\n"),
    );
    for (n, st) in [
        ("sendmeter-impl-12", "working"),
        ("synergy-impl-7", "blocked"),
        ("synergy-impl-8", "done"),
    ] {
        r.1.lane(n, st, "/x");
        r.1.screen(n, SCREEN);
    }
    r.1.add("side-project", json!({"fleet": "-", "harness": "codex"}));
    r.1.set("side-project", |t| {
        t["cwd"] = json!("/home/me/side");
        t["reported"] = json!({"state": "idle"});
    });
    (c, r)
}

fn get(s: &Server, path: &str, host: &str, extra: &str) -> String {
    String::from_utf8_lossy(&s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n{extra}\r\n"), T)).into_owned()
}

fn body(res: &str) -> Value {
    assert!(
        res.starts_with("HTTP/1.1 200 OK") && res.contains("application/json"),
        "{res}"
    );
    serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap()
}

#[test]
fn the_herd_read_shape_on_a_fixture() {
    let (c, r) = fixture("shape");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    *s.host.lock().unwrap() = drover::host::sample(&r);
    let v = body(&get(&s, "/api/v1/herd", ADDR, ""));
    let names = |l: &Value| {
        l.as_array()
            .unwrap()
            .iter()
            .map(|x| x["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let fleets = v["fleets"].as_array().unwrap();
    assert_eq!(
        fleets.iter().map(|f| f["name"].as_str().unwrap()).collect::<Vec<_>>(),
        ["sendmeter", "synergy"]
    );
    assert_eq!(names(&fleets[0]["lanes"]), ["sendmeter-impl-12"]);
    assert_eq!(names(&fleets[1]["lanes"]), ["synergy-impl-7", "synergy-impl-8"]);
    assert_eq!(names(&v["others"]), ["side-project"]);
    let l = &fleets[0]["lanes"][0];
    assert_eq!(
        (
            &l["issue"],
            &l["title"],
            &l["state"],
            &l["shown"],
            &l["tone"],
            &l["role"]
        ),
        (
            &json!(12),
            &json!("Send meter readings nightly"),
            &json!("working"),
            &json!("working"),
            &json!("g"),
            &json!("impl")
        )
    );
    assert_eq!(l["age"], "27h");
    assert!(l["coat"].as_str().unwrap().starts_with("--c:#"));
    let q = &fleets[0]["queue"];
    assert_eq!(
        q.as_array()
            .unwrap()
            .iter()
            .map(|i| (i["issue"].as_u64().unwrap(), i["blocked_by"].as_u64()))
            .collect::<Vec<_>>(),
        [(18, None), (19, Some(18)), (20, None)]
    );
    assert_eq!(q[0]["title"], "Herd mode");
    assert_eq!(fleets[1]["queue"], json!([]));
    let h = &v["host"];
    assert_eq!(
        (&h["limit"], &h["pressure"], &h["swapouts"], &h["gate"]),
        (
            &json!(2.5),
            &json!("warn"),
            &json!(null),
            &json!("memory pressure warn")
        )
    );
    assert!((h["load"].as_f64().unwrap() - 0.39).abs() < 1e-9, "{h}");
}

#[test]
fn the_herd_read_carries_the_action_and_fleet_colour() {
    let (c, r) = fixture("act");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let v = body(&get(&s, "/api/v1/herd", ADDR, ""));
    let f = &v["fleets"];
    assert!(
        ["l", "p", "c", "g", "a"].contains(&f[0]["color"].as_str().unwrap()),
        "{f}"
    );
    assert_eq!(f[0]["lanes"][0]["act"], "impl");
    assert_eq!(
        (&f[1]["lanes"][0]["act"], &f[1]["lanes"][1]["act"]),
        (&json!("blocked"), &json!("done"))
    );
    let fail = json!({"verdict": "FAIL", "sha": "a".repeat(40)});
    append(&c.state, &Event::new(900_200, "sendmeter", Some(12), "verdict", fail)).unwrap();
    let v = body(&get(&Server::new(&c, &r, Some(ADDR.into())), "/api/v1/herd", ADDR, ""));
    assert_eq!(v["fleets"][0]["lanes"][0]["act"], "fix");
}

#[test]
fn the_herd_read_has_no_pane_text() {
    let (c, r) = fixture("nopane");
    let res = get(&Server::new(&c, &r, Some(ADDR.into())), "/api/v1/herd", ADDR, "");
    assert!(
        !res.contains("PANE-SECRET-TEXT") && !res.contains("cargo test") && !res.contains("turns"),
        "{res}"
    );
    assert_eq!(body(&res)["fleets"][1]["lanes"][0]["state"], "blocked");
}

#[test]
fn the_herd_read_refuses_a_foreign_host_or_origin() {
    let (c, r) = fixture("origin");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    assert!(get(&s, "/api/v1/herd", "evil.example", "").starts_with("HTTP/1.1 403 "));
    assert!(get(&s, "/api/v1/herd", ADDR, "Origin: https://evil.example\r\n").starts_with("HTTP/1.1 403 "));
    assert!(
        !r.called("herdr agent list") && r.1.methods().is_empty(),
        "refused before any read"
    );
    body(&get(
        &s,
        "/api/v1/herd",
        "mac.example",
        "Origin: https://mac.example\r\n",
    ));
}

#[test]
fn a_shed_lane_reported_running_shows_running_with_its_note() {
    let (c, r) = fixture("running");
    let note = "running: cargo test (8m)";
    r.1.set("sendmeter-impl-12", |t| {
        t["reported"] = json!({"state": "running", "note": note});
        t["text"] = json!("");
    });
    r.1.set("synergy-impl-8", |t| t["reported"] = json!({"state": "idle"}));
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let herd = body(&get(&s, "/api/v1/herd", ADDR, ""));
    let l = &herd["fleets"][0]["lanes"][0];
    assert_eq!(
        (&l["name"], &l["state"], &l["act"]),
        (&json!("sendmeter-impl-12"), &json!("running"), &json!("impl")),
        "{l}"
    );
    assert_eq!(herd["fleets"][1]["lanes"][1]["act"], "idle", "{herd}");
    let lanes = body(&get(&s, "/api/v1/lanes", ADDR, ""));
    let section = |name: &str| -> Vec<Value> {
        let s = lanes["sections"].as_array().unwrap().iter().find(|s| s["name"] == name);
        s.map_or(vec![], |s| {
            s["repos"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|r| r["lanes"].as_array().unwrap().clone())
                .collect()
        })
    };
    let running = section("running");
    let row = running
        .iter()
        .find(|l| l["name"] == "sendmeter-impl-12")
        .unwrap_or_else(|| panic!("{lanes}"));
    assert_eq!((&row["act"], &row["now"]["n"]), (&json!("impl"), &json!(note)), "{row}");
    assert!(section("idle").iter().any(|l| l["name"] == "synergy-impl-8"), "{lanes}");
}

#[test]
fn the_scene_is_the_pinned_swag_packs_and_a_custom_theme_overrides_it() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/swag");
    let sums = std::fs::read_to_string(root.join("SHA256SUMS")).unwrap();
    let pin = std::fs::read_to_string(root.join("SOURCE")).unwrap();
    assert!(
        pin.starts_with("jirathip-dev/swag ") && pin.trim().rsplit(' ').next().unwrap().len() == 40,
        "{pin}"
    );
    let (c, r) = fixture("swag");
    let raw = |p: &str| s_raw(&Server::new(&c, &r, Some(ADDR.into())), p);
    for f in ["kitchen/scene.svg", "garden/scene.svg", "farm/sprites.svg"] {
        let (head, body) = raw(&format!("/themes/{f}"));
        assert!(head.starts_with("HTTP/1.1 200 OK"), "{f}: {head}");
        let sum = format!("{}  {f}", drover::devices::sha256(&body));
        assert!(sums.lines().any(|l| l == sum), "{f} is not the pinned pack's: {sum}");
    }
    assert!(
        raw("/themes/farm/scene.svg").0.starts_with("HTTP/1.1 404 "),
        "farm draws its own ranch"
    );
    let own = c.config.with_file_name("themes").join("kitchen");
    std::fs::create_dir_all(&own).unwrap();
    std::fs::write(own.join("theme.json"), "{}").unwrap();
    std::fs::write(own.join("scene.svg"), "<svg id=mine/>").unwrap();
    assert_eq!(raw("/themes/kitchen/scene.svg").1, b"<svg id=mine/>");
}

fn s_raw(s: &Server, path: &str) -> (String, Vec<u8>) {
    let res = s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), T);
    let at = res.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    (String::from_utf8_lossy(&res[..at]).into_owned(), res[at + 4..].to_vec())
}

#[test]
fn the_scene_in_webkit_is_a_full_page_header_toggle_drawn_from_the_scene_screen() {
    if no_webkit() {
        return;
    }
    let (c, r) = fixture("webkit");
    let d = c.state.parent().unwrap().to_path_buf();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || drover::serve::serve_on(&c, &r, l, false));
    let js = "window.late = (async () => { \
        const w = f => new Promise(ok => { const t = Date.now(), g = () => { const v = f(); if (v) ok(v); else if (Date.now() - t > 8000) ok(null); else setTimeout(g, 50); }; g(); }); \
        const q = s => document.querySelector(s), b = q('#scb'), sc = q('#scene'), lb = q('#hlt'), n = x => x ? 1 : 0; \
        const figs = () => document.querySelectorAll('#scene .hh').length, title = () => q('#scene .hs b').textContent; \
        const off = [n(b.getAttribute('aria-pressed') === 'false'), n(sc.hidden), n(lb.hidden)]; \
        b.click(); \
        await w(() => figs()); \
        const on = [n(b.getAttribute('aria-pressed') === 'true'), n(!sc.hidden), n(localStorage.scene === '1'), n(q('#sheet').hidden), n(getComputedStyle(q('main')).display === 'none'), \
          n(!lb.hidden), figs(), n(title() === 'sendmeter'), n(!!q('#scene .rsn .cn')), n(!!q('#scene .hh.working use[href=\"#sa-impl\"]')), n(!!q('#sa-impl .leg')), n(sc.classList.contains('night')), n(lb.textContent === '☀')]; \
        lb.click(); \
        const night = [n(sc.classList.contains('night')), n(localStorage['scene-light'] === 'night'), n(lb.textContent === '☾')]; \
        q('#scene .hnx').click(); \
        const next = [n(title() === 'synergy'), figs(), n(sc.classList.contains('night')), n(localStorage['scene-repo'] === 'synergy')]; \
        b.click(); \
        const back = [n(b.getAttribute('aria-pressed') === 'false'), n(sc.hidden), n(localStorage.scene == null), n(getComputedStyle(q('main')).display !== 'none'), n(lb.hidden)]; \
        const got = performance.getEntriesByType('resource').map(e => new URL(e.name).pathname); \
        const ours = /^\\/(screens\\/|themes\\/|a\\/|pair\\/|api\\/v1\\/timings$|sw\\.js$|manifest\\.webmanifest$|icon-|favicon|apple-touch)/; \
        return [...off, ...on, ...night, ...next, ...back, n(got.includes('/screens/scene')), got.filter(p => !ours.test(p) || /herd/.test(p)).length]; })()";
    let o = webview()
        .env("DROVER_PRE", "localStorage['scene-light'] = 'day'")
        .args([&format!("http://{addr}/"), "390", "-", "-"])
        .arg(d.join("scene.png"))
        .arg(js)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(o.status.success(), "{out} {}", String::from_utf8_lossy(&o.stderr));
    let v: Vec<&str> = out.split_whitespace().collect();
    let want = [
        "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "1", "0", "1", "1", "1", "1", "1", "2", "1",
        "1", "1", "1", "1", "1", "1", "1", "0",
    ];
    assert_eq!(
        v.get(2..),
        Some(&want[..]),
        "off: unpressed, hidden, no light; [scene]: pressed, the page under the header (no sheet, the tabs gone), kept, \
         the light shown, sendmeter's 1 horse ploughing in its gait, the ranch drawn, day [☀]; [☀] → night [☾], kept; \
         [›] synergy, its 2 horses, still night, kept; [scene] again: unpressed, hidden, forgotten, the tabs back; \
         /screens/scene fetched and nothing outside the page's routes (no herd.json, no old board route): {out}"
    );
}

fn scene_probe(theme: &str, level: &str, reduce: bool, check: &str) -> Vec<String> {
    scene_at(&format!("theme={theme}"), theme, "", level, reduce, check)
}

fn scene_at(query: &str, name: &str, pick: &str, level: &str, reduce: bool, check: &str) -> Vec<String> {
    let (pressure, load) = match level {
        "calm" => ("1", "1.00"),
        "busy" => ("1", "15.00"),
        _ => ("2", "3.90"),
    };
    let (c, r) = fixture_at(&format!("load-{name}-{level}"), pressure, load);
    append(
        &c.state,
        &Event::new(
            900_200,
            "sendmeter",
            Some(13),
            "spawn",
            json!({"role": "impl", "agent": "sendmeter-impl-13", "harness": "claude"}),
        ),
    )
    .unwrap();
    r.1.lane("sendmeter-impl-13", "idle", "/x");
    let d = c.state.parent().unwrap().to_path_buf();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || drover::serve::serve_on(&c, &r, l, false));
    let js = format!(
        "window.late = (async () => {{ \
        const w = f => new Promise(ok => {{ const t = Date.now(), g = () => {{ const v = f(); if (v) ok(v); else if (Date.now() - t > 8000) ok(null); else setTimeout(g, 50); }}; g(); }}); \
        const q = s => document.querySelector(s), on = e => getComputedStyle(e).display !== 'none'; \
        const shown = s => [...document.querySelectorAll('#scene .rsn ' + s)].filter(on).length; \
        {pick} \
        await w(() => q('#scene .rsn svg') && document.querySelector('#scene .hh') && document.querySelector('symbol[id^=sa-]') \
            && (document.documentElement.dataset.theme === 'farm' || q('#scene .rsn :is(.h-calm,.l-sun,.s-moderate)'))); \
        const h = q('#scene'), anim = e => e && getComputedStyle(e).animationName !== 'none' ? 1 : 0; \
        {check} }})()"
    );
    let mut cmd = webview();
    cmd.env(
        "DROVER_PRE",
        "localStorage.scene = '1'; (f => { window.fetch = (u, o) => String(u).endsWith('/scene.svg') ? new Promise(ok => setTimeout(ok, 2000)).then(() => f(u, o)) : f(u, o); })(window.fetch.bind(window));",
    );
    if reduce {
        cmd.env("DROVER_REDUCE", "1");
    }
    let o = cmd
        .args([&format!("http://{addr}/?{query}"), "390", "-", "-"])
        .arg(d.join(format!("scene-{name}-{level}.png")))
        .arg(js)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(o.status.success(), "{out} {}", String::from_utf8_lossy(&o.stderr));
    out.split_whitespace().skip(2).map(String::from).collect()
}

#[test]
fn the_scene_in_webkit_draws_kitchens_heat_held_while_the_spawn_gate_is_held() {
    if no_webkit() {
        return;
    }
    let check = "const lit = h.classList.contains('heat-held') ? 1 : 0; \
        const flames = shown('.h-held'), others = shown(':is(.h-calm,.h-busy):not(.h-held)'), glow = anim(q('#scene .rsn .kglow')), loop = anim(h); \
        return [lit, flames > 0 ? 1 : 0, others, glow, loop, document.querySelectorAll('#scene use[href=\"#sfan\"]').length];";
    assert_eq!(
        scene_probe("kitchen", "held", false, check),
        ["1", "1", "0", "1", "1", "1"],
        "kitchen held: heat-held on the Scene, its flames shown, no calm or busy layer, the glow and the frame loop run, the idle lane fans"
    );
}

#[test]
fn the_scene_in_webkit_draws_each_load_level_of_kitchen_and_garden_and_farms_sky() {
    if no_webkit() {
        return;
    }
    for (level, sun) in [("calm", "l-sun"), ("busy", "l-overcast"), ("held", "l-rain")] {
        let heat = format!(
            "return [h.classList.contains('heat-{level}') ? 1 : 0, shown('.h-{level}') > 0 ? 1 : 0, shown(':is(.h-calm,.h-busy,.h-held):not(.h-{level})')];"
        );
        assert_eq!(
            scene_probe("kitchen", level, false, &heat),
            ["1", "1", "0"],
            "kitchen {level}: heat-{level}, its flames and no other level's"
        );
        let light = format!(
            "return [h.classList.contains('light-{level}') ? 1 : 0, shown('.{sun}') > 0 ? 1 : 0, shown(':is(.l-sun,.l-overcast,.l-rain):not(.{sun})')];"
        );
        assert_eq!(
            scene_probe("garden", level, false, &light),
            ["1", "1", "0"],
            "garden {level}: light-{level}, its .{sun} window and no other"
        );
    }
    for (level, want) in [
        ("calm", ["0", "0", "0", "0"]),
        ("busy", ["0", "1", "0", "0"]),
        ("held", ["1", "0", "0", "1"]),
    ] {
        let farm = "const c = h.classList, n = x => x ? 1 : 0; \
            return [n(c.contains('storm')), n(c.contains('gust')), n(/heat|light/.test(h.className)), n(getComputedStyle(q('#scene .hrain')).display === 'block')];";
        assert_eq!(
            scene_probe("farm", level, false, farm),
            want,
            "farm {level}: storm, gust, no heat or light, rain only in the storm"
        );
    }
}

#[test]
fn the_scene_in_webkit_stops_the_flames_and_glow_under_reduced_motion() {
    if no_webkit() {
        return;
    }
    let check = "return [h.classList.contains('heat-held') ? 1 : 0, anim(h), anim(q('#scene .rsn .kglow'))];";
    assert_eq!(
        scene_probe("kitchen", "held", true, check),
        ["1", "0", "0"],
        "kitchen held under reduced motion: heat-held, no frame loop, no glow animation"
    );
}

const PICK_BUSH: &str = "q('#sysb').click(); \
    (await w(() => [...document.querySelectorAll('#sheet .row')].find(r => r.textContent.replace('✓', '').trim() === 'bush'))).click(); \
    await w(() => document.documentElement.dataset.theme === 'bush' && [...document.querySelectorAll('.hdefs symbol')].some(s => s.id === 'ha-fix' && s.innerHTML.includes('--wo'))); \
    [...document.querySelectorAll('#sheet .sh button')].find(b => b.textContent === 'close').click();";

#[test]
fn the_scene_in_webkit_draws_bush_picked_from_the_theme_list_its_sign_at_each_level_light_and_dark() {
    if no_webkit() {
        return;
    }
    for (query, scheme) in [
        ("theme=farm", "bush-light"),
        ("theme=farm&scheme=bush-dark", "bush-dark"),
    ] {
        for (level, word) in [("calm", "moderate"), ("busy", "high"), ("held", "catastrophic")] {
            let check = format!(
                "await w(() => q('#scene.sign-{level}') && document.querySelector('#scene .fdr')); \
                return [document.documentElement.dataset.scheme === '{scheme}' ? 1 : 0, h.classList.contains('sign-{level}') ? 1 : 0, \
                shown('.fdr') > 0 ? 1 : 0, shown('.s-{word}') > 0 ? 1 : 0, shown(':is(.s-moderate,.s-high,.s-catastrophic):not(.s-{word})'), \
                h.querySelectorAll('use[href=\"#sa-idle\"]').length > 0 ? 1 : 0, anim(h)];"
            );
            assert_eq!(
                scene_at(query, scheme, PICK_BUSH, level, false, &check),
                ["1", "1", "1", "1", "0", "1", "1"],
                "bush {scheme} {level}: picked from the list, sign-{level} on the Scene, the sign reads {word} and no other level, the lane drawn, the loop runs"
            );
        }
    }
}

#[test]
fn the_scene_in_webkit_holds_bushs_sign_still_under_reduced_motion() {
    if no_webkit() {
        return;
    }
    let check = "await w(() => q('#scene.sign-held') && document.querySelector('#scene .fdr')); \
        return [h.classList.contains('sign-held') ? 1 : 0, shown('.s-catastrophic') > 0 ? 1 : 0, shown(':is(.s-moderate,.s-high):not(.s-catastrophic)'), anim(h)];";
    assert_eq!(
        scene_at("theme=bush", "bush-still", "", "held", true, check),
        ["1", "1", "0", "0"],
        "bush held under reduced motion: catastrophic alone shown, no frame loop"
    );
}
