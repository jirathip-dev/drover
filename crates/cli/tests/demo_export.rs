mod common;
use common::*;
use drover::Ctx;
use serde_json::Value;
use std::path::PathBuf;

fn export() -> (PathBuf, Fake) {
    let demo = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo"));
    let d = tmpdir("demo-export");
    let c = Ctx {
        config: demo.join("fleet.toml"),
        state: demo.join("events.jsonl"),
        worktrees: d.join("wt"),
        homes: [("hermes", d.join("hermes"))].into(),
        exe: "drover".into(),
        now: 1,
        home: d.join("home"),
        hints: vec![],
    };
    let r = Fake::default();
    drover::serve::export(&c, &r, &d.join("site"), true).unwrap();
    (d.join("site"), r)
}

#[test]
fn demo_export_writes_the_file_set_without_running_anything() {
    let (site, r) = export();
    assert!(
        r.calls.borrow().is_empty(),
        "the passed runner was called: {:?}",
        r.calls.borrow()
    );
    for f in [
        "index.html",
        "herd.json",
        "manifest.webmanifest",
        "sw.js",
        "icon-180.png",
        "icon-192.png",
        "icon-512.png",
    ] {
        assert!(site.join(f).is_file(), "missing {f}");
    }
    let mut lanes: Vec<String> = std::fs::read_dir(site.join("lane"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    lanes.sort();
    assert_eq!(lanes.len(), 9, "{lanes:?}");
    for l in &lanes {
        let j: Value = serde_json::from_str(&std::fs::read_to_string(site.join("lane").join(l)).unwrap()).unwrap();
        assert!(!j["lines"].as_array().unwrap().is_empty(), "{l}: no lines");
    }
    let dsh: Value =
        serde_json::from_str(&std::fs::read_to_string(site.join("lane/acme-api-impl-17.json")).unwrap()).unwrap();
    assert_eq!(dsh["harness"], "dsh");
    let j: Value = serde_json::from_str(
        &std::fs::read_to_string(site.join("lane/acme-web-impl-48-contrast-audit-for-every-settings-pane-abxx.json"))
            .unwrap(),
    )
    .unwrap();
    assert!(j["status"].as_str().unwrap().contains("Combobulating"), "{j}");
    let sheet = j["lines"].to_string();
    for gone in [
        "────",
        "acme-web · impl #48",
        "Combobulating",
        "Tip:",
        "bypass permissions",
    ] {
        assert!(!sheet.contains(gone), "`{gone}` in the sheet: {sheet}");
    }
    assert!(sheet.contains("raising it one step."), "{sheet}");
    let h: Value = serde_json::from_str(&std::fs::read_to_string(site.join("herd.json")).unwrap()).unwrap();
    let fleets = h["fleets"].as_array().unwrap();
    assert_eq!(
        fleets.iter().map(|f| f["name"].as_str().unwrap()).collect::<Vec<_>>(),
        ["acme-api", "acme-ios", "acme-web", "docs"]
    );
    let all: Vec<&Value> = fleets
        .iter()
        .flat_map(|f| f["lanes"].as_array().unwrap())
        .chain(h["others"].as_array().unwrap())
        .collect();
    assert_eq!(all.len(), 11);
    for st in ["working", "idle", "done", "blocked", "parked", "dead"] {
        assert!(all.iter().any(|l| l["state"] == st), "no {st} lane");
    }
    assert!(h["others"].as_array().unwrap().iter().any(|l| l["role"] == "other"));
    let q: Vec<&Value> = fleets.iter().flat_map(|f| f["queue"].as_array().unwrap()).collect();
    assert_eq!(
        (q.len(), q.iter().filter(|i| !i["blocked_by"].is_null()).count()),
        (8, 3)
    );
}

#[test]
fn demo_page_is_inert_and_relative() {
    let (site, _) = export();
    let html = std::fs::read_to_string(site.join("index.html")).unwrap();
    let v = std::fs::read_dir(site.join("a"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let page = [
        "serve.css",
        "herd.css",
        "serve.js",
        "herd.js",
        "themes/farm/sprites.svg",
    ]
    .iter()
    .fold(html.clone(), |p, f| p + &std::fs::read_to_string(v.join(f)).unwrap());
    assert!(
        html.contains("<section id=queue></section>"),
        "the page has no Queue HTML"
    );
    let page = page + &queue_html(&std::fs::read_to_string(site.join("api/v1/queue")).unwrap(), "tab(d)");
    assert!(
        html.contains("<div id=needs></div>")
            && html.contains("<section id=lanes></section><section id=chat></section>"),
        "the page has no needs, Lanes or [chat] HTML"
    );
    let file = |n: &str| std::fs::read_to_string(site.join("api/v1").join(n)).unwrap();
    let (needs, lanes, chat) = (file("needs"), file("lanes"), file("chat"));
    let drawn = board_js(&[
        (&needs, "needs.tab(d)"),
        (&lanes, "lanes.tab(d)"),
        (&chat, "chat.tab(d)"),
    ]);
    let page = page + &drawn.concat();
    let cards = &drawn[0];
    assert!(html.contains(&format!(
        "<script src=a/{}/serve.js>",
        v.file_name().unwrap().to_string_lossy()
    )));
    assert!(
        page.contains("data-demo=1") && page.contains("id=demo>demo</span>"),
        "demo badge"
    );
    assert!(page.contains("Demo with fake data. Install drover to run your own."));
    assert_eq!(page.matches("<span class=g>merged</span>").count(), 4, "4 merges today");
    assert_eq!(cards.matches("<details class=ask>").count(), 3, "three needs-you cards");
    let card = &cards[cards.find("<details class=ask>").unwrap()..];
    assert!(
        card.starts_with("<details class=ask><summary><span>#21 · owner · Rate-limit")
            && card.contains("</summary><p><b>needs you</b> <span class=m>acme-api · #21 · 18m</span>"),
        "{card:.300}"
    );
    assert!(
        card.contains("<span class=m>· owner · waits for you</span>"),
        "{card:.300}"
    );
    let card = &card[card[1..].find("<details class=ask>").unwrap() + 1..];
    assert!(
        card.starts_with("<details class=ask><summary><span>#44 · routine · Split the settings")
            && card.contains("</summary><p><b>needs you</b> <span class=m>acme-web · #44 · acme-web-impl-44 · 6m</span> <span class=m>· routine · defaults to 2 in 24m</span>"),
        "{card:.300}"
    );
    assert!(
        !page.contains("<form method=post") && page.contains("<form inert"),
        "every form inert"
    );
    for abs in [
        "href=/",
        "href=\"/",
        "src=/",
        "src=\"/",
        "action=/",
        "fetch('/",
        "register('/",
        "EventSource('/",
    ] {
        assert!(!page.contains(abs), "absolute URL `{abs}`");
    }
    assert!(
        page.contains("rel=manifest href=manifest.webmanifest") && page.contains("serviceWorker.register(R + 'sw.js')")
    );
    assert!(!page.contains("Paired devices") && !page.contains("id=push"));
    for guard in [
        "function connect() {\n  if (DEMO) return;",
        "if (DEMO) return herdPull();",
        "o.method === 'POST' ? Promise.reject",
        "DEMO ? 'lane/' + encodeURIComponent(a) + '.json'",
        "!B.dataset.ro && !DEMO) cleanable()",
    ] {
        assert!(page.contains(guard), "missing demo guard `{guard}`");
    }
    let m = std::fs::read_to_string(site.join("manifest.webmanifest")).unwrap();
    assert!(!m.contains("\"/"), "manifest URLs relative: {m}");
    let sw = std::fs::read_to_string(site.join("sw.js")).unwrap();
    assert!(
        sw.contains("self.registration.scope") && !sw.contains("'/lane'"),
        "sw paths under its scope"
    );
    assert!(
        sw.contains(&format!("P + 'a/{}/'", v.file_name().unwrap().to_string_lossy())),
        "sw precaches this build's assets"
    );
}
