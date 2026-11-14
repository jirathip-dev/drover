mod common;
use common::*;
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const BUILT: [&str; 10] = [
    "github-dark",
    "solarized-dark",
    "gruvbox",
    "nord",
    "dracula",
    "paper",
    "farm",
    "kitchen",
    "garden-light",
    "garden-dark",
];

fn built(n: &str) -> Value {
    let p = format!("{}/../../assets/schemes/{n}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn node(js: &str) -> Option<Value> {
    if std::process::Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping: no node here");
        return None;
    }
    let src = include_str!("../../../assets/serve.js");
    let part = &src[src.find("// #244 schemes").unwrap()..src.find("// schemes:end").unwrap()];
    let d = tmpdir("schemes-js");
    std::fs::write(d.join("t.js"), format!("{part}\nconsole.log(JSON.stringify({js}))")).unwrap();
    let o = std::process::Command::new("node").arg(d.join("t.js")).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    Some(serde_json::from_slice(&o.stdout).unwrap())
}

fn page(theme: Option<&str>, own: &[(&str, &str)]) -> String {
    let d = tmpdir("scheme");
    let cfg = theme.map_or(CONFIG.to_string(), |t| format!("{CONFIG}\n[serve]\ntheme = \"{t}\"\n"));
    let c = ctx(&d, &cfg);
    let dir = c.config.with_file_name("schemes");
    std::fs::create_dir_all(&dir).unwrap();
    for (n, body) in own {
        std::fs::write(dir.join(n), body).unwrap();
    }
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    String::from_utf8(s.respond(&format!("GET / HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), 1_000_000)).unwrap()
}

fn sd(page: &str) -> Value {
    let at = page.find("let SD=").unwrap() + 7;
    serde_json::Deserializer::from_str(&page[at..])
        .into_iter()
        .next()
        .unwrap()
        .unwrap()
}

#[test]
fn every_built_in_has_the_whole_token_set_and_passes_the_gate() {
    let Some(tokens) = node("TOKENS") else { return };
    let all: serde_json::Map<String, Value> = BUILT.iter().map(|n| (n.to_string(), built(n))).collect();
    for (n, s) in &all {
        let keys: Vec<&String> = s["tokens"].as_object().unwrap().keys().collect();
        assert_eq!(
            json!(keys.len()),
            json!(tokens.as_array().unwrap().len()),
            "{n}: {keys:?}"
        );
        assert!(
            tokens
                .as_array()
                .unwrap()
                .iter()
                .all(|t| s["tokens"].get(t.as_str().unwrap()).is_some()),
            "{n}"
        );
    }
    let got = node(&format!(
        "Object.fromEntries(Object.entries({}).map(([n, s]) => [n, fails(s)]))",
        json!(all)
    ))
    .unwrap();
    for n in BUILT {
        assert_eq!(got[n], json!([]), "{n} fails the gate");
    }
}

#[test]
fn a_bad_scheme_is_refused_with_the_reason() {
    let ok = built("github-dark");
    let with = |k: &str, v: &str| {
        let mut s = ok.clone();
        s["tokens"][k] = json!(v);
        s
    };
    let mut missing = ok.clone();
    missing["tokens"].as_object_mut().unwrap().remove("--fk-a");
    let mut light = ok.clone();
    light["color-scheme"] = json!("dim");
    let cases = [
        (missing, "missing or not #rrggbb[aa]: --fk-a"),
        (with("--r", "red"), "missing or not #rrggbb[aa]: --r"),
        (with("--mute", "#444c56"), "--mute on --bg 2.18 < 4.5"),
        (with("--fg", "#5b2a2a"), "--fg on --bg 1.63 < 4.5"),
        (with("--r", "#3a1d1d"), "--r on --bg 1.24 < 3"),
        (with("--a", "#f05a48"), "ΔE2000 --a ↔ --r 2.35 < 20"),
        (with("--l", "#d6a02a"), "ΔE2000 --l ↔ --a 2.06 < 20"),
        (light, "color-scheme is not \"dark\" or \"light\""),
    ];
    let js = format!(
        "{}.map(s => fails(s)[0])",
        json!(cases.iter().map(|c| &c.0).collect::<Vec<_>>())
    );
    let Some(got) = node(&js) else { return };
    for (i, (_, want)) in cases.iter().enumerate() {
        assert_eq!(got[i], json!(want), "case {i}");
    }
    let tint = node(&format!(
        "fails({}).filter(f => f.includes('needs-you card'))",
        with("--mute", "#7d8590")
    ))
    .unwrap();
    assert_eq!(tint, json!(["--mute on the needs-you card 4.33 < 4.5"]));
    for f in ["missing-token", "solarized-canonical"] {
        let p = format!(
            "{}/../../docs/design/schemes/refused/{f}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let n = node(&format!("fails({}).length", std::fs::read_to_string(p).unwrap())).unwrap();
        assert!(n.as_u64().unwrap() > 0, "{f} passes");
    }
}

#[test]
fn a_user_scheme_is_served_and_overrides_a_built_in() {
    let mut mine = built("nord");
    mine["tokens"]["--l"] = json!("#99ccff");
    let p = page(
        None,
        &[
            ("nord.json", &mine.to_string()),
            ("moss.json", &built("farm").to_string()),
            ("bad.json", "{not json"),
            ("a.b.json", "{}"),
            ("notes.txt", "{}"),
        ],
    );
    let s = &sd(&p)["s"];
    assert_eq!(s["nord"], mine, "the user file wins");
    assert_eq!(s["moss"], built("farm"));
    assert_eq!(s["bad"], Value::Null, "unreadable: served as null, the gate refuses it");
    assert_eq!(s["github-dark"], built("github-dark"));
    let names: Vec<&String> = s.as_object().unwrap().keys().collect();
    assert_eq!(
        names.len(),
        BUILT.len() + 2,
        "{names:?}: a.b and notes.txt are not schemes"
    );
    let Some(got) = node(&format!("[fails({}).length > 0, choose({s}, 'bad', 'farm')]", s["bad"])) else {
        return;
    };
    assert_eq!(got, json!([true, "farm"]));
}

#[test]
fn each_theme_names_its_default_and_the_first_paint_is_the_board_themes() {
    let p = page(None, &[]);
    assert_eq!(
        sd(&p)["t"],
        json!({"farm": "farm", "kitchen": "kitchen", "garden": {"light": "garden-light", "dark": "garden-dark"},
            "terminal": "github-dark"})
    );
    assert!(p.contains("<style id=sch>:root{--a:#f29b4b;"), "farm's block first");
    assert!(
        p.contains("<button id=sysb type=button") && !p.contains("id=look"),
        "#440: one [⚙], theme and colours in its Appearance"
    );
    assert!(p.contains("try{sb=localStorage.sb}catch(e){}if(sb)document.getElementById('sch').textContent=sb"));
    let p = page(Some("terminal"), &[]);
    assert!(
        p.contains("<style id=sch>:root{--a:#d29922;--bg:#0d1117;"),
        "terminal: github-dark"
    );
    let d = tmpdir("scheme-space");
    let c = ctx(&d, &format!("{CONFIG}\n[serve]\ntheme = \"space\"\n"));
    std::fs::create_dir_all(c.config.with_file_name("themes/space")).unwrap();
    std::fs::write(c.config.with_file_name("themes/space/theme.json"), "{}").unwrap();
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let p = String::from_utf8(s.respond(&format!("GET / HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), 1_000_000)).unwrap();
    assert_eq!(sd(&p)["t"]["space"], json!("github-dark"));
    let all = json!({"farm": built("farm"), "nord": built("nord"), "github-dark": built("github-dark")});
    let js = format!(
        "[choose({all}, null, 'farm'), choose({all}, 'nord', 'farm'), choose({all}, 'gone', 'nord'), choose({all}, null, 'gone')]"
    );
    let Some(got) = node(&js) else { return };
    assert_eq!(got, json!(["farm", "nord", "nord", "github-dark"]));
}

#[test]
fn github_dark_is_todays_board() {
    let t = &built("github-dark")["tokens"];
    let today = [
        ("--bg", "#0d1117"),
        ("--fg", "#c9d1d9"),
        ("--mute", "#8b949e"),
        ("--line", "#21262d"),
        ("--g", "#3fb950"),
        ("--a", "#d29922"),
        ("--r", "#f85149"),
        ("--l", "#58a6ff"),
        ("--p", "#bc8cff"),
        ("--c", "#39c5cf"),
        ("--card", "#161b22"),
        ("--edge", "#30363d"),
        ("--scrim", "#000000aa"),
        ("--link", "#58a6ff"),
        ("--fk-l", "#58a6ff"),
        ("--fk-p", "#bc8cff"),
        ("--fk-c", "#39c5cf"),
        ("--fk-g", "#3fb950"),
        ("--fk-a", "#d29922"),
    ];
    for (k, v) in today {
        assert_eq!(t[k], json!(v), "{k}");
    }
    assert_eq!(built("github-dark")["color-scheme"], json!("dark"));
}

#[test]
fn a_light_dark_scheme_follows_the_phone_and_a_pick_still_wins() {
    let p = page(Some("garden"), &[]);
    let (s, d) = (&sd(&p)["s"], &sd(&p)["t"]["garden"]);
    assert_eq!(*d, json!({"light": "garden-light", "dark": "garden-dark"}));
    let tok = |n: &str| built(n)["tokens"]["--bg"].as_str().unwrap().to_string();
    let first = &p[p.find("<style id=sch>").unwrap()..p.find("</style>").unwrap()];
    let (light, dark) = (
        first.find(&format!("--bg:{};", tok("garden-light"))),
        first.find("@media(prefers-color-scheme:dark){:root{"),
    );
    assert!(light.is_some() && dark.is_some() && light < dark, "{first}");
    assert!(
        first[dark.unwrap()..].contains(&format!("--bg:{};", tok("garden-dark"))),
        "{first}"
    );
    let Some(got) = node(&format!(
        "[choose({s}, null, {d}, false), choose({s}, null, {d}, true), choose({s}, 'nord', {d}, true), \
         choose({s}, null, 'kitchen', true), choose({s}, null, 'kitchen', false), choose({s}, null, undefined, true)]"
    )) else {
        return;
    };
    assert_eq!(
        got,
        json!([
            "garden-light",
            "garden-dark",
            "nord",
            "kitchen",
            "kitchen",
            "github-dark"
        ])
    );
}
