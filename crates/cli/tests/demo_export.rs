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

fn screen(site: &std::path::Path, path: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(site.join(path).join("index.html")).unwrap()).unwrap()
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
        "manifest.webmanifest",
        "sw.js",
        "icon-180.png",
        "icon-192.png",
        "icon-512.png",
        "themes/farm/theme.json",
        "themes/farm/sprites.svg",
    ] {
        assert!(site.join(f).is_file(), "missing {f}");
    }
    for gone in ["herd.json", "lane", "api", "thread"] {
        assert!(!site.join(gone).exists(), "{gone} is the old board's");
    }
    let v = std::fs::read_dir(site.join("a"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    for f in ["board.js", "board.css"] {
        assert!(v.join(f).is_file(), "missing a/<ver>/{f}");
    }
    for p in ["queue", "lanes", "events", "fleets", "cards"] {
        let j = screen(&site, &format!("screens/{p}"));
        assert!(j.is_object() || j.is_array(), "screens/{p}: {j}");
    }
    assert_eq!(screen(&site, "screens/queue")["id"], "queue");
    assert_eq!(screen(&site, "screens/fleets")["type"], "sheet");
    let cards = screen(&site, "screens/cards");
    let ids: Vec<&str> = cards
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["ask-q2", "ask-q3", "ask-q33", "nhc-acme-api-22"]);
    let lanes = screen(&site, "screens/lanes").to_string();
    let mut sheets: Vec<String> = std::fs::read_dir(site.join("screens/lanes"))
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    sheets.sort();
    assert_eq!(sheets.len(), 11, "{sheets:?}");
    for l in &sheets {
        let j = screen(&site, &format!("screens/lanes/{l}"));
        assert_eq!(j["type"], "sheet", "{l}: {j}");
        assert!(
            lanes.contains(&format!("\"/screens/lanes/{l}\"")),
            "{l}: no row opens it"
        );
    }
    let j = screen(
        &site,
        "screens/lanes/acme-web-impl-48-contrast-audit-for-every-settings-pane-abxx",
    );
    let said = j["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == "transcript")
        .unwrap()
        .to_string();
    for gone in [
        "────",
        "acme-web · impl #48",
        "Combobulating",
        "Tip:",
        "bypass permissions",
    ] {
        assert!(!said.contains(gone), "`{gone}` in the sheet: {said}");
    }
    assert!(said.contains("raising it one step."), "{said}");
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
    let ver = v.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        html.contains(&format!("<script type=module src=a/{ver}/board.js>")),
        "{html}"
    );
    assert!(
        html.contains("data-demo=1") && html.contains("id=demo>demo</span>"),
        "demo badge"
    );
    assert!(html.contains("Demo with fake data. Install drover to run your own."));
    assert!(
        !html.contains("data-pair") && !html.contains("data-key"),
        "no pairing or alerts in the demo"
    );
    let page = html.clone() + &std::fs::read_to_string(v.join("board.js")).unwrap();
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
    assert!(html.contains("rel=manifest href=manifest.webmanifest") && html.contains("<main></main><nav></nav>"));
    for gone in ["<section id=queue>", "serve.js", "herd.js", "<form"] {
        assert!(!html.contains(gone), "`{gone}` is the old board's");
    }
    let m = std::fs::read_to_string(site.join("manifest.webmanifest")).unwrap();
    assert!(!m.contains("\"/"), "manifest URLs relative: {m}");
    let sw = std::fs::read_to_string(site.join("sw.js")).unwrap();
    assert!(sw.contains("self.registration.scope"), "sw paths under its scope");
    assert!(
        sw.contains(&format!("P + 'a/{ver}/'")),
        "sw precaches this build's assets"
    );
}
