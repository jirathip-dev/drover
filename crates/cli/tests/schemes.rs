mod common;
use common::*;
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const SWAG: [&str; 12] = [
    "dracula",
    "farm",
    "farm-light",
    "garden-dark",
    "garden-light",
    "github-dark",
    "gruvbox",
    "kitchen",
    "kitchen-light",
    "nord",
    "paper",
    "solarized-dark",
];

fn served(theme: Option<&str>, own: &[(&str, &str)]) -> (String, Value) {
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
    let p = String::from_utf8(s.respond(&format!("GET / HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), 1_000_000)).unwrap();
    (
        p,
        serde_json::from_str(&api(&s, ADDR, "", 1_000_000, "health")).unwrap(),
    )
}

fn page(theme: Option<&str>, own: &[(&str, &str)]) -> String {
    served(theme, own).0
}

fn node(all: &Value, js: &str) -> Option<Value> {
    if std::process::Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping: no node here");
        return None;
    }
    let src = include_str!("../../../web/src/theme.ts");
    let part = &src[src.find("type Scheme").unwrap()..src.find("const NS").unwrap()];
    let d = tmpdir("schemes-js");
    let body = format!("const SD = {{ s: {all}, t: {{}} }};\n{part}\nconsole.log(JSON.stringify({js}))");
    std::fs::write(d.join("t.ts"), body).unwrap();
    let o = std::process::Command::new("node").arg(d.join("t.ts")).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    Some(serde_json::from_slice(&o.stdout).unwrap())
}

fn built(n: &str) -> Value {
    sd(&page(None, &[]))["s"][n].clone()
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
fn every_swag_scheme_is_served_and_none_is_refused() {
    let (p, h) = served(None, &[]);
    let names: Vec<String> = sd(&p)["s"].as_object().unwrap().keys().cloned().collect();
    assert_eq!(names, SWAG);
    assert_eq!(h["schemes"], json!([]));
}

#[test]
fn an_owner_scheme_that_fails_the_gate_is_refused_on_load_with_the_reason_in_health() {
    let ok = built("github-dark");
    let with = |k: &str, v: &str| {
        let mut s = ok.clone();
        s["tokens"][k] = json!(v);
        s.to_string()
    };
    let mut missing = ok.clone();
    missing["tokens"].as_object_mut().unwrap().remove("--fk-a");
    let mut dim = ok.clone();
    dim["color-scheme"] = json!("dim");
    let refused = std::fs::read_to_string(format!(
        "{}/../../docs/design/schemes/refused/solarized-canonical.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let (mute, fg, delta) = (
        with("--mute", "#444c56"),
        with("--fg", "#5b2a2a"),
        with("--a", "#f05a48"),
    );
    let (missing, dim) = (missing.to_string(), dim.to_string());
    let own = [
        ("mute.json", mute.as_str()),
        ("nord.json", fg.as_str()),
        ("delta.json", delta.as_str()),
        ("missing.json", missing.as_str()),
        ("dim.json", dim.as_str()),
        ("bad.json", "{not json"),
        ("canonical.json", refused.as_str()),
    ];
    let (p, h) = served(None, &own);
    let names: Vec<String> = sd(&p)["s"].as_object().unwrap().keys().cloned().collect();
    assert_eq!(names, SWAG, "no refused scheme is served");
    assert_ne!(
        sd(&p)["s"]["nord"]["tokens"]["--fg"],
        json!("#5b2a2a"),
        "a refused own nord keeps swag's"
    );
    let why: Vec<&str> = h["schemes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let has = |n: &str, part: &str| {
        assert!(
            why.iter()
                .any(|w| w.starts_with(&format!("scheme {n} refused: ")) && w.contains(part)),
            "{n}: {part} in {why:?}"
        );
    };
    has("mute", "--mute on --bg 2.18 < 4.5");
    has("nord", "--fg on --bg 1.63 < 4.5");
    has("delta", "ΔE2000 --a ↔ --r 2.35 < 20");
    has("missing", "missing field `--fk-a`");
    has("dim", "unknown variant `dim`");
    has("bad", "key must be a string");
    has("canonical", " < ");
    assert_eq!(why.len(), own.len(), "{why:?}");
}

#[test]
fn parity_11_a_user_scheme_is_served_and_overrides_a_built_in() {
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
    assert_eq!(s["bad"], Value::Null, "unreadable: refused, not served");
    assert_eq!(s["github-dark"], built("github-dark"));
    let names: Vec<&String> = s.as_object().unwrap().keys().collect();
    assert_eq!(
        names.len(),
        SWAG.len() + 1,
        "{names:?}: a.b and notes.txt are not schemes"
    );
    let first = json!(&p[p.find("<style id=sch>").unwrap() + 14..p.find("</style>").unwrap()]);
    let Some(got) = node(s, &format!("[choose('bad', 'farm', true), css(SD.s.farm) === {first}]")) else {
        return;
    };
    assert_eq!(
        got,
        json!(["farm", true]),
        "the page writes farm's block as serve's first paint does"
    );
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
    let js = "[choose(null, 'farm'), choose('nord', 'farm'), choose('gone', 'nord'), choose(null, 'gone'), choose('toString', null)]";
    let Some(got) = node(&all, js) else { return };
    assert_eq!(got, json!(["farm", "nord", "nord", "github-dark", "github-dark"]));
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
    let Some(got) = node(
        s,
        &format!(
            "[choose(null, {d}, false), choose(null, {d}, true), choose('nord', {d}, true), \
             choose(null, 'kitchen', true), choose(null, 'kitchen', false), choose(null, undefined, true)]"
        ),
    ) else {
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
