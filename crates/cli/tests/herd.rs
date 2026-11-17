mod common;
use common::*;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;
const SCREEN: &str = "⏺ Bash(cargo test)\n  ⎿ PANE-SECRET-TEXT ok\n";

fn fixture(name: &str) -> (drover::Ctx, OnShed) {
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
            .reply("sysctl -n kern.memorystatus_vm_pressure_level", "2\n")
            .reply("sysctl -n vm.loadavg", "{ 3.90 1.20 1.00 }\n")
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
        (&json!(2.5), &json!("warn"), &json!(0.0), &json!("memory pressure warn"))
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
    for t in ["kitchen", "garden"] {
        let (head, svg) = raw(&format!("/themes/{t}/scene.svg"));
        assert!(head.starts_with("HTTP/1.1 200 OK"), "{t}: {head}");
        let sum = format!("{}  {t}/scene.svg", drover::devices::sha256(&svg));
        assert!(
            sums.lines().any(|l| l == sum),
            "{t}'s scene is not the pinned pack's: {sum}"
        );
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
fn the_herd_in_webkit_draws_a_figure_per_lane_switches_night_and_repo() {
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
        const q = s => document.querySelector(s), figs = () => document.querySelectorAll('#sheet .herd .av').length; \
        q('#sysb').click(); \
        (await w(() => [...document.querySelectorAll('#sheet .row')].find(r => r.textContent.toLowerCase().startsWith('herd')))).click(); \
        await w(() => q('#sheet .herd .hsc svg')); \
        const one = [figs(), !!q('#sheet .hsc #kglow'), q('#sheet .herd .hn').textContent === 'sendmeter', !!q('#sheet .herd.night')]; \
        q('#sheet .herd button[aria-label=night]').click(); \
        const night = !!q('#sheet .herd.night'); \
        q('#sheet .herd button[aria-label=\"next repo\"]').click(); \
        return [...one, night, q('#sheet .herd .hn').textContent === 'synergy', figs(), !!q('#sheet .herd.night')]; })()";
    let o = webview()
        .env("DROVER_PRE", "localStorage['herd-night'] = '0'")
        .args([&format!("http://{addr}/?theme=kitchen"), "390", "-", "-"])
        .arg(d.join("herd.png"))
        .arg(js)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(o.status.success(), "{out} {}", String::from_utf8_lossy(&o.stderr));
    let v: Vec<&str> = out.split_whitespace().collect();
    assert_eq!(
        v.get(2..),
        Some(&["1", "1", "1", "0", "1", "1", "2", "1"][..]),
        "figures in sendmeter, kitchen's scene, sendmeter first, day; [☾] night; [›] synergy, its 2 figures, still night: {out}"
    );
}
