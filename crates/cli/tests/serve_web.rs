mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::serve::Server;
use serde_json::json;

const ADDR: &str = "127.0.0.1:7777";

fn config() -> String {
    format!("models = [\"opus\", \"sonnet\"]\n\n[serve]\nallowed_hosts = [\"mac.tail1.ts.net\"]\n\n{CONFIG}")
}

fn get(s: &Server, path: &str) -> String {
    String::from_utf8_lossy(&s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), 1_000_000)).into_owned()
}

fn screen(s: &Server, path: &str) -> serde_json::Value {
    let res = get(s, path);
    serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap_or_else(|_| panic!("{res}"))
}

fn find<'a>(v: &'a serde_json::Value, id: &str) -> Option<&'a serde_json::Value> {
    if v.get("id").and_then(serde_json::Value::as_str) == Some(id) {
        return Some(v);
    }
    match v {
        serde_json::Value::Object(m) => m.values().find_map(|x| find(x, id)),
        serde_json::Value::Array(a) => a.iter().find_map(|x| find(x, id)),
        _ => None,
    }
}

fn mtime(s: &Server) -> String {
    screen(s, "/api/v1/state")["mtime"].as_str().unwrap().to_string()
}

fn post(s: &Server, body: &str, extra: &str) -> String {
    let req = format!(
        "POST / HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&req, 1_000_000)).into_owned()
}

fn location(res: &str) -> String {
    let l = res
        .lines()
        .find_map(|l| l.strip_prefix("Location: "))
        .unwrap_or_default()
        .as_bytes();
    let (mut out, mut i) = (vec![], 0);
    while i < l.len() {
        if l[i] == b'%' {
            out.push(u8::from_str_radix(std::str::from_utf8(&l[i + 1..i + 3]).unwrap(), 16).unwrap());
            i += 3;
        } else {
            out.push(l[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

fn last(c: &drover::Ctx) -> (serde_json::Value, Option<String>) {
    let mut e = read(&c.state).unwrap().to_vec().pop().unwrap();
    let via = e.data.remove("via").and_then(|v| v.as_str().map(String::from));
    (json!({"type": e.kind, "fleet": e.fleet, "data": e.data}), via)
}

fn pair(name: &str) -> (drover::Ctx, drover::Ctx) {
    let (a, b) = (tmpdir(&format!("{name}-cli")), tmpdir(&format!("{name}-web")));
    let (a, b) = (ctx(&a, &config()), ctx(&b, &config()));
    for c in [&a, &b] {
        assert_eq!(
            run(c, &Fake::default(), &["queue", "sendmeter", "add", "17", "18", "12"]).0,
            0
        );
    }
    (a, b)
}

fn order(c: &drover::Ctx) -> Vec<u64> {
    drover::queue::replay(&read(&c.state).unwrap(), "sendmeter")
        .iter()
        .map(|i| i.issue)
        .collect()
}

#[test]
fn queue_buttons_move_like_queue_mv() {
    let (cli, web) = pair("mv");
    let r = Fake::default();
    let s = Server::new(&web, &r, Some(ADDR.into()));
    for (args, body, msg) in [
        (
            vec!["mv", "12", "--before", "18"],
            "op=mv&fleet=sendmeter&issue=12&before=18",
            "moved #12 before #18",
        ),
        (
            vec!["mv", "17", "--after", "18"],
            "op=mv&fleet=sendmeter&issue=17&after=18",
            "moved #17 after #18",
        ),
    ] {
        assert_eq!(
            run(&cli, &Fake::default(), &[&["queue", "sendmeter"][..], &args].concat()).0,
            0
        );
        let res = post(&s, &format!("{body}&mtime={}", mtime(&s)), "");
        assert!(res.starts_with("HTTP/1.1 303 See Other\r\n"), "{res}");
        assert_eq!(location(&res), format!("/?msg={msg}"), "{res}");
        assert!(msg.len() <= 60);
        assert_eq!(order(&cli), order(&web));
        assert_eq!(last(&cli), (last(&web).0, None));
        assert_eq!(last(&web).1.as_deref(), Some("web"));
    }
    assert_eq!(order(&web), [12, 18, 17]);
    let res = post(
        &s,
        &format!("op=mv&fleet=sendmeter&issue=99&before=12&mtime={}", mtime(&s)),
        "",
    );
    assert!(
        location(&res).starts_with("/?msg=error: #99 is not queued"),
        "an error is kept: {res}"
    );
    assert!(get(&s, "/").contains("</nav>"), "page still renders");
}

#[test]
fn pause_button_matches_cli_and_blocks_next_and_spawn() {
    let (cli, web) = pair("pause");
    let r = Fake::default();
    let s = Server::new(&web, &r, Some(ADDR.into()));
    assert_eq!(
        run(&cli, &Fake::default(), &["pause", "sendmeter"]),
        (0, "sendmeter: paused\n".into())
    );
    let res = post(&s, &format!("op=pause&fleet=sendmeter&mtime={}", mtime(&s)), "");
    assert_eq!(location(&res), "/?msg=sendmeter paused", "{res}");
    let text = |c: &drover::Ctx| std::fs::read_to_string(&c.config).unwrap();
    assert_eq!(text(&cli), text(&web));
    assert!(
        text(&web).contains("integration = \"staging\"   # never main\npaused = true\n"),
        "{}",
        text(&web)
    );
    assert_eq!(last(&cli), (last(&web).0, None));
    assert_eq!(
        last(&web),
        (
            json!({"type": "pause", "fleet": "sendmeter", "data": {}}),
            Some("web".into())
        )
    );
    assert!(get(&s, "/screens/queue").contains("\"label\":\"Resume\""));
    for c in [&cli, &web] {
        assert_eq!(
            run(c, &Fake::default(), &["next", "sendmeter"]),
            (1, "sendmeter: paused\n".into())
        );
        for role in ["impl", "rev"] {
            let (code, out) = run(c, &Fake::default(), &["spawn", "sendmeter", "17", role]);
            assert_eq!(code, 3, "{out}");
            assert!(out.starts_with("REFUSED PAUSED: "), "{out}");
        }
    }
    assert_eq!(run(&cli, &Fake::default(), &["resume", "sendmeter"]).0, 0);
    let res = post(&s, &format!("op=resume&fleet=sendmeter&mtime={}", mtime(&s)), "");
    assert_eq!(location(&res), "/?msg=sendmeter resumed");
    assert_eq!(text(&cli), text(&web));
    assert!(text(&web).contains("paused = false\n"));
    assert_eq!(
        last(&web),
        (
            json!({"type": "resume", "fleet": "sendmeter", "data": {}}),
            Some("web".into())
        )
    );
    assert_eq!(run(&web, &Fake::default(), &["next", "sendmeter"]), (0, "17\n".into()));
}

#[test]
fn role_dropdowns_set_like_drover_set() {
    let (cli, web) = pair("set");
    let r = Fake::default();
    let s = Server::new(&web, &r, Some(ADDR.into()));
    let fleets = screen(&s, "/screens/fleets").to_string();
    assert!(
        fleets.contains("\"name\":\"sendmeter.impl\",\"options\":[\"opus\",\"sonnet\"]"),
        "only `models`: {fleets}"
    );
    assert_eq!(
        run(
            &cli,
            &Fake::default(),
            &["set", "sendmeter", "impl", "--model", "sonnet", "--effort", "high"]
        )
        .0,
        0
    );
    let res = post(
        &s,
        &format!(
            "op=set&fleet=sendmeter&role=impl&model=sonnet&effort=high&mtime={}",
            mtime(&s)
        ),
        "",
    );
    assert_eq!(
        location(&res),
        "/?msg=sendmeter impl: model sonnet, effort high",
        "{res}"
    );
    let text = |c: &drover::Ctx| std::fs::read_to_string(&c.config).unwrap();
    assert_eq!(text(&cli), text(&web));
    assert!(text(&web).contains("model = \"sonnet\"\neffort = \"high\"\n"));
    let sets = |c: &drover::Ctx| {
        read(&c.state)
            .unwrap()
            .to_vec()
            .into_iter()
            .filter(|e| e.kind == "set")
            .collect::<Vec<_>>()
    };
    assert_eq!(sets(&web).len(), 2);
    for (a, b) in sets(&cli).iter().zip(sets(&web)) {
        let mut b = b.clone();
        assert_eq!(b.data.remove("via"), Some(json!("web")));
        assert_eq!(a.data, b.data);
    }
    let before = text(&web);
    let res = post(
        &s,
        &format!("op=set&fleet=sendmeter&role=impl&model=gpt&mtime={}", mtime(&s)),
        "",
    );
    assert!(
        location(&res).starts_with("/?msg=REFUSED BAD_VALUE: model \"gpt\" is not in `models`"),
        "{res}"
    );
    let res = post(
        &s,
        &format!("op=set&fleet=sendmeter&role=impl&effort=max&mtime={}", mtime(&s)),
        "",
    );
    assert!(location(&res).contains("effort \"max\" must be one of"), "{res}");
    assert_eq!(text(&web), before, "no write");
    assert_eq!(
        run(&cli, &Fake::default(), &["set", "sendmeter", "impl", "--model", "gpt"]).0,
        3
    );
}

#[test]
fn set_refuses_toml_injection_on_web_and_cli() {
    let (cli, web) = pair("inject");
    let r = Fake::default();
    let s = Server::new(&web, &r, Some(ADDR.into()));
    let text = |c: &drover::Ctx| std::fs::read_to_string(&c.config).unwrap();
    let (before, log) = (text(&web), drover::events::jsonl(&web.state).unwrap());
    let evil = "opus%22%0A%5B%5Bnotify%5D%5D%0Acmd+%3D+%22touch+%2Ftmp%2Fpwned%22%0A%23";
    for field in ["model", "effort"] {
        let res = post(
            &s,
            &format!("op=set&fleet=sendmeter&role=impl&{field}={evil}&mtime={}", mtime(&s)),
            "",
        );
        assert!(
            location(&res).starts_with(&format!("/?msg=REFUSED BAD_VALUE: {field} ")),
            "{res}"
        );
    }
    assert_eq!(text(&web), before, "fleet.toml byte-identical");
    assert_eq!(drover::events::jsonl(&web.state).unwrap(), log, "no event");
    let before = text(&cli);
    for v in [
        "opus\"\n[[notify]]\ncmd = \"touch /tmp/pwned\"\n#",
        "x\"\nprofile = \"evil",
        "a\\b",
        "",
    ] {
        for flag in ["--model", "--effort", "--profile"] {
            let (code, out) = run(&cli, &Fake::default(), &["set", "sendmeter", "impl", flag, v]);
            assert_eq!(code, 3, "{flag} {v:?}: {out}");
            assert!(out.contains("REFUSED BAD_VALUE"), "{out}");
        }
    }
    assert_eq!(text(&cli), before, "fleet.toml byte-identical");
}

#[test]
fn stale_mtime_refuses_without_writing() {
    let (_, web) = pair("stale");
    let r = Fake::default();
    let s = Server::new(&web, &r, Some(ADDR.into()));
    let (cfg, log) = (
        std::fs::read_to_string(&web.config).unwrap(),
        drover::events::jsonl(&web.state).unwrap(),
    );
    for body in [
        "op=pause&fleet=sendmeter&mtime=1",
        "op=mv&fleet=sendmeter&issue=12&before=17&mtime=1",
        "op=set&fleet=sendmeter&role=impl&model=sonnet",
    ] {
        let res = post(&s, body, "");
        assert!(res.starts_with("HTTP/1.1 303 "), "{res}");
        assert!(location(&res).starts_with("/?msg=REFUSED STALE: "), "{res}");
    }
    assert_eq!(std::fs::read_to_string(&web.config).unwrap(), cfg);
    assert_eq!(drover::events::jsonl(&web.state).unwrap(), log);
    assert!(get(&s, "/?msg=REFUSED%20STALE%3A%20x").contains("<p class=\"msg r\">REFUSED STALE: x</p>"));
}

#[test]
fn foreign_host_or_origin_is_403_and_read_only_is_405() {
    let (_, web) = pair("origin");
    let r = Fake::default();
    let s = Server::new(&web, &r, Some(ADDR.into()));
    let body = format!("op=pause&fleet=sendmeter&mtime={}", mtime(&s));
    let log = drover::events::jsonl(&web.state).unwrap();
    for extra in [
        "Origin: https://evil.example\r\n",
        "Origin: http://127.0.0.1:7777.evil\r\n",
        "Origin: null\r\n",
    ] {
        assert!(post(&s, &body, extra).starts_with("HTTP/1.1 403 "), "{extra}");
    }
    let wrong = format!(
        "POST / HTTP/1.1\r\nHost: evil.example\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    assert!(String::from_utf8_lossy(&s.respond(&wrong, 1_000_000)).starts_with("HTTP/1.1 403 "));
    assert_eq!(drover::events::jsonl(&web.state).unwrap(), log, "nothing written");
    let ts = format!(
        "POST / HTTP/1.1\r\nHost: mac.tail1.ts.net:7443\r\nOrigin: https://mac.tail1.ts.net:7443\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    assert!(String::from_utf8_lossy(&s.respond(&ts, 1_000_000)).starts_with("HTTP/1.1 303 "));
    assert!(post(
        &s,
        &format!("op=resume&fleet=sendmeter&mtime={}", mtime(&s)),
        &format!("Origin: http://{ADDR}\r\n")
    )
    .starts_with("HTTP/1.1 303 "));
    let ro = Server::new(&web, &r, None);
    assert!(post(&ro, &body, "").starts_with("HTTP/1.1 405 "));
    let page = get(&ro, "/");
    assert!(
        !page.contains("<form method=post") && page.contains("data-ro=1"),
        "no buttons"
    );
}

#[test]
fn page_reads_only_cached_pr_data_and_serves_pwa_assets() {
    let d = tmpdir("pwa");
    let c = ctx(&d, &config());
    run(&c, &Fake::default(), &["queue", "sendmeter", "add", "12"]);
    append(
        &c.state,
        &Event::new(
            999_000,
            "sendmeter",
            Some(12),
            "spawn",
            json!({"role": "impl", "agent": "sendmeter-impl-12"}),
        ),
    )
    .unwrap();
    let cache = c.state.parent().unwrap().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let pr = json!({"number": 34, "state": "OPEN", "statusCheckRollup": [{"__typename": "CheckRun", "status": "COMPLETED", "conclusion": "SUCCESS"}]});
    std::fs::write(
        cache.join("pr-sendmeter-12.json"),
        json!({"ts": 1, "branch": "issue-12", "pr": pr}).to_string(),
    )
    .unwrap();
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let page = get(&s, "/");
    let q = screen(&s, "/screens/queue");
    let row = find(&q, "q-sendmeter-12").unwrap_or_else(|| panic!("{q}"));
    assert_eq!(row["subtitle"], json!("impl · PR #34 · CI ✓"), "{row}");
    let sheet = screen(&s, row["tap"]["sheet"].as_str().unwrap_or_else(|| panic!("{row}")));
    let pr = find(&sheet, "pr").unwrap_or_else(|| panic!("{sheet}"));
    assert_eq!(
        pr["tap"]["url"],
        json!("https://github.com/jirathip-dev/sendmeter/pull/34"),
        "{pr}"
    );
    assert!(!r.called("gh"), "no gh call per page load: {:?}", r.calls.borrow());
    for (path, ctype) in [
        ("/manifest.webmanifest", "application/manifest+json"),
        ("/sw.js", "text/javascript"),
        ("/icon-180.png", "image/png"),
        ("/icon-192.png", "image/png"),
        ("/icon-512.png", "image/png"),
    ] {
        let res = s.respond(&format!("GET {path} HTTP/1.1\r\n\r\n"), 1);
        let head = String::from_utf8_lossy(&res[..res.len().min(200)]).into_owned();
        assert!(
            head.starts_with("HTTP/1.1 200 OK") && head.contains(ctype),
            "{path}: {head}"
        );
    }
    for n in [180u32, 192, 512] {
        let res = s.respond(&format!("GET /icon-{n}.png HTTP/1.1\r\n\r\n"), 1);
        let png = &res[res.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4..];
        let dim = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().unwrap());
        assert!(
            png.starts_with(b"\x89PNG\r\n\x1a\n") && (dim(16), dim(20)) == (n, n),
            "icon-{n}.png dimensions"
        );
    }
    let body = |p: &str| {
        let res = s.respond(&format!("GET {p} HTTP/1.1\r\n\r\n"), 1);
        let at = res.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        (String::from_utf8_lossy(&res[..at]).into_owned(), res[at + 4..].to_vec())
    };
    for (path, icon) in [
        ("/apple-touch-icon.png", "/icon-180.png"),
        ("/apple-touch-icon-precomposed.png", "/icon-180.png"),
        ("/favicon.ico", "/icon-192.png"),
    ] {
        let (head, png) = body(path);
        assert!(
            head.starts_with("HTTP/1.1 200 OK") && head.contains("image/png"),
            "{path}: {head}"
        );
        assert!(png == body(icon).1, "{path}: not the {icon} bytes");
    }
    let v = page.split("/a/").nth(1).unwrap().split('/').next().unwrap();
    assert!(
        page.contains(&format!("rel=icon href=/icon-192.png?v={v}"))
            && page.contains(&format!("rel=apple-touch-icon href=/icon-180.png?v={v}")),
        "{page}"
    );
    assert!(get(&s, "/manifest.webmanifest").contains("\"display\": \"standalone\""));
    assert!(
        page.contains("rel=manifest href=/manifest.webmanifest")
            && std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../web/src/main.ts"))
                .unwrap()
                .contains("serviceWorker.register(url('sw.js'))")
    );
    assert!(page.contains("data-ts=1000000"), "render time for the stale badge");
}

#[test]
fn lanes_group_by_fleet_with_others() {
    let d = tmpdir("lanes");
    let two = format!("{CONFIG}\n[fleet.synergy]\nrepo = \"o/synergy-apps\"\nlocal = \"/p/synergy-apps\"\nintegration = \"staging\"\n");
    let c = ctx(&d, &two);
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let ev = |fleet: &str, issue: Option<u64>, d: serde_json::Value| {
        append(&c.state, &Event::new(900_000, fleet, issue, "spawn", d)).unwrap()
    };
    ev(
        "sendmeter",
        Some(12),
        json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude", "model": "opus", "effort": "high"}),
    );
    ev(
        "sendmeter",
        Some(9),
        json!({"role": "rev", "agent": "sendmeter-rev-9-r1", "harness": "hermes"}),
    );
    ev(
        "synergy",
        Some(5),
        json!({"role": "design", "agent": "synergy-design-5", "harness": "claude"}),
    );
    ev(
        "synergy",
        Some(7),
        json!({"role": "impl", "agent": "synergy-impl-7", "harness": "claude", "pane": "w5:p1"}),
    );
    let events = read(&c.state).unwrap();
    let st = drover::events::fold(&events);
    let wt = c.worktrees.join("synergy-apps/designer").to_string_lossy().into_owned();
    let lab = |f: &str, h: &str| json!({"fleet": f, "harness": h});
    let agents = sessions(json!([
        {"name": "sendmeter-impl-12", "status": "working", "cwd": "/x", "labels": lab("sendmeter", "claude")},
        {"name": "synergy-design-5", "status": "done", "cwd": "/x", "labels": lab("synergy", "claude")},
        {"name": "w3", "status": "working", "cwd": wt, "labels": lab("synergy", "claude")},
        {"name": "side-project", "status": "idle", "cwd": "/Users/me/Projects/side", "labels": lab("", "codex")},
        {"name": "synergy-impl-7", "status": "waiting-approval", "cwd": "/p/synergy-apps", "labels": lab("synergy", "dsh")},
    ]));
    let lanes = drover::lanes::join(&cfg, &Fake::default(), &events, &st, &agents);
    let got: Vec<(Option<&str>, &str, &str, String, bool)> = lanes
        .iter()
        .map(|l| {
            (
                l.fleet.as_deref(),
                l.name.as_str(),
                l.role.as_str(),
                l.state.word(),
                l.parked.is_some(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            (Some("sendmeter"), "sendmeter-impl-12", "impl", "working".into(), false),
            (Some("synergy"), "synergy-design-5", "design", "done".into(), false),
            (Some("synergy"), "w3", "other", "working".into(), false),
            (None, "side-project", "other", "idle".into(), false),
            (Some("synergy"), "synergy-impl-7", "impl", "blocked".into(), false),
            (Some("sendmeter"), "sendmeter-rev-9-r1", "rev", "dead".into(), false),
        ]
    );
    assert_eq!(
        (lanes[0].model.as_deref(), lanes[0].effort.as_deref(), lanes[0].issue),
        (Some("opus"), Some("high"), Some(12))
    );
    let sh = FakeShed::attached(&c);
    sh.lane("sendmeter-impl-12", "working", "/x");
    sh.set("sendmeter-impl-12", |t| {
        t["reported"]["note"] = json!("running: cargo test (4m)")
    });
    sh.lane("synergy-design-5", "done", "/x");
    for (name, harness, cwd, state) in [
        ("w3", "claude", wt.as_str(), "working"),
        ("side-project", "codex", "/Users/me/Projects/side", "idle"),
    ] {
        sh.add(name, json!({"fleet": "", "harness": harness}));
        sh.set(name, |t| {
            (t["cwd"], t["reported"]) = (json!(cwd), json!({"state": state}))
        });
    }
    sh.lane("synergy-impl-7", "blocked", "/p/synergy-apps");
    let f = Fake::default();
    let r = sh.on(&f);
    let lanes = screen(&Server::new(&c, &r, Some(ADDR.into())), "/screens/lanes");
    assert_eq!(find(&lanes, "lanes-working").unwrap()["header"], "working (2)");
    assert_eq!(
        find(&lanes, "l-sendmeter-impl-12").unwrap()["subtitle"],
        "impl · local · running: cargo test (4m)"
    );
    assert_eq!(
        find(&lanes, "lanes-dead").unwrap()["blocks"][0]["rows"][0]["id"],
        "l-sendmeter-rev-9-r1",
        "a log/pane mismatch is dead"
    );
}

#[test]
fn one_lane_per_agent_name() {
    let c = ctx(&tmpdir("lanes-dup"), CONFIG);
    let cfg = drover::config::Config::load(&c.config).unwrap();
    for ts in [900_000, 950_000] {
        let d = json!({"role": "impl", "agent": "sendmeter-impl-3061", "harness": "claude", "pane": "w1:p1"});
        append(&c.state, &Event::new(ts, "sendmeter", Some(3061), "spawn", d)).unwrap();
    }
    let events = read(&c.state).unwrap();
    let lab = json!({"fleet": "sendmeter"});
    let agents = sessions(json!([
        {"name": "sendmeter-impl-3061", "status": "working", "cwd": "/x", "labels": lab},
        {"name": "sendmeter-impl-3061", "status": "idle", "cwd": "/x", "host": "mini", "labels": lab},
    ]));
    let st = drover::events::fold(&events);
    let fake = Fake::default();
    let lanes = drover::lanes::join(&cfg, &fake, &events, &st, &agents);
    let got: Vec<_> = lanes
        .iter()
        .map(|l| (l.name.as_str(), l.state.word(), l.host.as_deref()))
        .collect();
    assert_eq!(got, [("sendmeter-impl-3061", "working".to_string(), None)]);
}

#[test]
fn secrets_in_shown_text_are_masked() {
    use drover::lanes::{mask, tail};
    let secrets = [
        (
            concat!("export ANTHROPIC_API_KEY=sk", "-ant-api03-abcdefghijklmnop"),
            "export ANTHROPIC_API_KEY=•••",
        ),
        (
            concat!("curl -H token sk", "-ant-api03-abcdefghijklmnop"),
            "curl -H token sk-•••",
        ),
        (
            concat!("gh auth gh", "p_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"),
            "gh auth ghp_•••",
        ),
        (
            concat!("gh", "o_16C7e42F292c6912E7710c838347Ae178B4a done"),
            "gho_••• done",
        ),
        (concat!("slack xo", "xb-1234567890-abcdefghij"), "slack xox•••"),
        (
            concat!("jwt ey", "JhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.abc123def"),
            "jwt •••",
        ),
        ("sha 0123456789abcdef0123456789abcdef01234567 ok", "sha ••• ok"),
        ("b64 QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVowMTIzNDU2Nzg5", "b64 •••"),
        ("GITHUB_TOKEN=\"abc def\" next", "GITHUB_TOKEN=\"••• def\" next"),
        ("DB_PASSWORD=hunter2 cargo test", "DB_PASSWORD=••• cargo test"),
        (
            "cargo test --release -p drover serve_web",
            "cargo test --release -p drover serve_web",
        ),
        (
            "/Users/jirathip/.herdr/worktrees/drover/issue-16/src",
            "/Users/jirathip/.herdr/worktrees/drover/issue-16/src",
        ),
    ];
    for (raw, want) in secrets {
        assert_eq!(mask(raw), want, "{raw}");
    }
    let lines: String = (1..=14).map(|i| format!("line {i}\n\n")).collect::<String>()
        + concat!("gh", "p_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789\n");
    let t = tail(&lines, 10);
    assert_eq!((t.len(), t[0].as_str(), t[9].as_str()), (10, "line 6", "ghp_•••"));
}

#[test]
fn queue_shows_open_work_only_and_folds_paused_retired_and_noise() {
    let d = tmpdir("short");
    let two = format!("{}\n[fleet.synergy]\nrepo = \"o/synergy-apps\"\nlocal = \"/p/synergy-apps\"\nintegration = \"staging\"\npaused = true\n", config());
    let c = ctx(&d, &two);
    let ev = |fleet: &str, issue: u64, ts: u64, kind: &str, d: serde_json::Value| {
        append(&c.state, &Event::new(ts, fleet, Some(issue), kind, d)).unwrap()
    };
    append(
        &c.state,
        &Event::new(1, "fleet", None, "queue", json!({"op": "add", "issues": [3]})),
    )
    .unwrap();
    ev("fleet", 3, 2, "spawn", json!({"role": "impl", "agent": "fleet-impl-3"}));
    run(&c, &Fake::default(), &["queue", "sendmeter", "add", "17", "18"]);
    for n in 100..130 {
        run(&c, &Fake::default(), &["queue", "sendmeter", "add", &n.to_string()]);
        ev(
            "sendmeter",
            n,
            990_000 + n,
            "merged",
            json!({"pr": (n + 1000).to_string(), "merge_sha": "abc", "into": "staging"}),
        );
    }
    ev(
        "sendmeter",
        17,
        999_000,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-17"}),
    );
    run(&c, &Fake::default(), &["queue", "synergy", "add", "1039", "1040"]);
    ev(
        "synergy",
        1039,
        999_000,
        "spawn",
        json!({"role": "impl", "agent": "synergy-impl-1039"}),
    );
    for k in 0..4 {
        ev(
            "sendmeter",
            17,
            999_500 + k,
            if k % 2 == 0 { "clean" } else { "notify-failed" },
            json!({"why": "merged"}),
        );
    }
    let sh = FakeShed::new();
    sh.attach(&c);
    sh.lane("sendmeter-impl-17", "working", "/x");
    let f = Fake::default();
    let r = sh.on(&f);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let q = screen(&s, "/screens/queue");
    let text = q.to_string();
    assert!(
        !text.contains("fleet-impl-3") && find(&q, "fleet-fleet").is_none(),
        "no retired fleet"
    );
    assert_eq!(
        text.matches("\"id\":\"q-").count(),
        2 + 2,
        "open items only: sendmeter #17 #18, synergy #1039 #1040"
    );
    assert_eq!(find(&q, "done-today-sendmeter").unwrap()["value"], "30");
    let syn = find(&q, "fleet-synergy").unwrap();
    assert_eq!(syn["subtitle"], "paused · 1 lane (dead)", "{syn}");
    assert_eq!(find(syn, "pause-synergy").unwrap()["label"], "Resume");
    let dead = find(&q, "q-synergy-1039").unwrap();
    assert_eq!(dead["badge"]["text"], "dead", "{dead}");
    assert!(dead.get("swipe").is_none(), "no action on a dead lane: {dead}");
    assert_eq!(
        find(&q, "q-sendmeter-17").unwrap()["badge"]["text"],
        "impl",
        "live lane stays impl"
    );
}

#[test]
fn page_never_samples_the_host_and_links_immutable_assets() {
    let c = ctx(&tmpdir("assets"), &config());
    let r = Fake::default()
        .reply("vm_stat", "Swapouts: 100.\n")
        .reply("sysctl", "1\n");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let page = get(&s, "/");
    assert_eq!(r.count("vm_stat"), 0, "{:?}", r.calls.borrow());
    assert!(
        !r.called("sysctl")
            && page.contains("Cache-Control: no-store")
            && !page.contains("<style>")
            && page.matches("<script>").count() == 1
            && page.contains("<script>let h=document.documentElement,m=new URLSearchParams")
    );
    let i = page.find("<script type=module src=/a/").unwrap() + 27;
    let v = &page[i..i + 8];
    assert!(v.chars().all(|c| c.is_ascii_hexdigit()), "{v}");
    let sw = get(&s, "/sw.js");
    for (f, ctype) in [
        ("board.css", "text/css"),
        ("board.js", "text/javascript"),
        ("themes/farm/sprites.svg", "image/svg+xml"),
        ("themes/farm/theme.json", "application/json"),
    ] {
        let res = get(&s, &format!("/a/{v}/{f}"));
        assert!(
            res.starts_with("HTTP/1.1 200 OK")
                && res.contains(ctype)
                && res.contains("Cache-Control: public, max-age=31536000, immutable\r\n"),
            "{f}"
        );
        assert!(
            page.contains(&format!("/a/{v}/{f}")) || f.starts_with("themes/"),
            "{f} linked"
        );
        assert!(
            sw.contains(&format!("'{f}'")) && sw.contains(&format!("P + 'a/{v}/'")),
            "{f} precached"
        );
    }
    assert!(
        get(&s, "/a/00000000/board.js").starts_with("HTTP/1.1 404 "),
        "another build's hash"
    );
    assert!(
        get(&s, "/icon-192.png").contains("Cache-Control: public, max-age=86400\r\n")
            && get(&s, "/sw.js").contains("Cache-Control: no-store\r\n")
    );
    assert_eq!(r.count("vm_stat"), 0);
}

#[test]
fn fetch_only_reads_redirect_navigations_to_the_board() {
    let (c, r) = (ctx(&tmpdir("frags-nav"), &config()), Fake::default());
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let send = |path: &str, extra: &str| {
        String::from_utf8_lossy(&s.respond(
            &format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}\r\n"),
            1_000_000,
        ))
        .into_owned()
    };
    for path in ["/api/v1/needs", "/api/v1/lanes"] {
        let res = send(path, "");
        assert!(
            res.starts_with("HTTP/1.1 200 OK") && res.contains("Content-Type: application/json; charset=utf-8\r\n"),
            "{path}: {res}"
        );
        let j: serde_json::Value = serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap();
        assert!(j.is_object(), "{path}");
    }
    assert!(send("/frags", "").starts_with("HTTP/1.1 404"), "no HTML parts");
    for path in ["/lane?agent=sendmeter-impl-12"] {
        for nav in [
            "Sec-Fetch-Mode: navigate\r\n",
            "Sec-Fetch-Dest: document\r\n",
            "Accept: text/html,application/xhtml+xml\r\n",
        ] {
            let res = send(path, nav);
            assert!(
                res.starts_with("HTTP/1.1 303 See Other\r\nLocation: /\r\n"),
                "{path} {nav}: {res}"
            );
        }
        assert!(
            !send(path, "Sec-Fetch-Mode: cors\r\nAccept: */*\r\n").starts_with("HTTP/1.1 303"),
            "{path}: fetch() is not redirected"
        );
    }
    assert!(
        send("/", "Sec-Fetch-Mode: navigate\r\n").starts_with("HTTP/1.1 200 OK"),
        "the board itself"
    );
    let sw = send("/sw.js", "");
    assert!(sw.contains("e.request.mode === 'navigate' && u.pathname !== P && !u.pathname.startsWith(P + 'thread/')) return e.respondWith(caches.match(P).then(m => m || fetch(P)))"), "{sw}");
    assert!(
        sw.contains("['api/', 'screens/', 'design/', 'shot/'].some(d => u.pathname.startsWith(P + d))")
            && !sw.contains("frags")
    );
    let v = send("/", "");
    let v = &v[v.find("/a/").unwrap()..][..11];
    assert!(!send(&format!("{v}/board.js"), "").contains("frags"));
}
