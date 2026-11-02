//! #172: Herd themes: `[serve] theme`, `/theme/<file>` from `<config dir>/themes/<name>/`, then the built-in
//! theme, then farm's; path traversal refused; an invalid theme.json falls back to farm with a Host warning.
mod common;
use common::*;
use herdsman::serve::Server;

const ADDR: &str = "127.0.0.1:7777";
const FARM: &str = include_str!("../assets/themes/farm/theme.json");
const FARM_SPRITES: &str = include_str!("../assets/themes/farm/sprites.svg");
const KITCHEN: &str = include_str!("../assets/themes/kitchen/theme.json");
const KITCHEN_SCENE: &str = include_str!("../assets/themes/kitchen/scene.svg");

/// A server whose fleet.toml picks `theme` (none: the default), with user themes `(name, file, body)`.
fn server(theme: Option<&str>, own: &[(&str, &str, &str)]) -> (herdsman::Ctx, Fake) {
    let d = tmpdir("theme");
    let cfg = theme.map_or(CONFIG.to_string(), |t| format!("{CONFIG}\n[serve]\ntheme = \"{t}\"\n"));
    let c = ctx(&d, &cfg);
    for (name, file, body) in own {
        let dir = c.config.with_file_name("themes").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(file), body).unwrap();
    }
    (c, Fake::default())
}

fn get(c: &herdsman::Ctx, r: &Fake, path: &str) -> String {
    let s = Server::new(c, r, Some(ADDR.into()));
    String::from_utf8_lossy(&s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), 1_000_000)).into_owned()
}

fn body(res: &str) -> &str {
    assert!(res.starts_with("HTTP/1.1 200 OK"), "{res}");
    &res[res.find("\r\n\r\n").unwrap() + 4..]
}

#[test]
fn default_theme_is_farm() {
    let (c, r) = server(None, &[]);
    assert_eq!(body(&get(&c, &r, "/theme/theme.json")), FARM);
    assert_eq!(body(&get(&c, &r, "/theme/sprites.svg")), FARM_SPRITES);
    assert!(
        get(&c, &r, "/theme/scene.svg").starts_with("HTTP/1.1 404 "),
        "farm draws its scene in herd.js"
    );
    assert!(get(&c, &r, "/")
        .contains("<p class=gr data-t=g><span class=k>theme</span><span class=w><b>farm</b></span></p>"));
}

#[test]
fn lookup_is_user_then_built_in_then_farm() {
    // Built-in kitchen: its own three files.
    let (c, r) = server(Some("kitchen"), &[]);
    assert_eq!(body(&get(&c, &r, "/theme/theme.json")), KITCHEN);
    assert_eq!(body(&get(&c, &r, "/theme/scene.svg")), KITCHEN_SCENE);
    assert!(get(&c, &r, "/")
        .contains("<p class=gr data-t=g><span class=k>theme</span><span class=w><b>kitchen</b></span></p>"));
    // A user theme named like a built-in wins, file by file; what it lacks is the built-in's.
    let own = r#"{"poses":{"working":"w"}}"#;
    let (c, r) = server(Some("kitchen"), &[("kitchen", "theme.json", own)]);
    assert_eq!(body(&get(&c, &r, "/theme/theme.json")), own);
    assert_eq!(body(&get(&c, &r, "/theme/scene.svg")), KITCHEN_SCENE);
    // A user-only theme: its files, then farm's.
    let (c, r) = server(
        Some("space"),
        &[("space", "theme.json", own), ("space", "sprites.svg", "<g id=w/>")],
    );
    assert_eq!(body(&get(&c, &r, "/theme/sprites.svg")), "<g id=w/>");
    assert!(get(&c, &r, "/theme/sprites.svg").contains("Content-Type: image/svg+xml"));
    assert!(get(&c, &r, "/theme/theme.json").contains("Content-Type: application/json"));
    let (c, r) = server(Some("space"), &[("space", "theme.json", own)]);
    assert_eq!(body(&get(&c, &r, "/theme/sprites.svg")), FARM_SPRITES);
}

#[test]
fn invalid_theme_json_falls_back_to_farm_with_a_host_warning() {
    for (name, own) in [
        (
            "broken",
            vec![
                ("broken", "theme.json", "{ not json"),
                ("broken", "sprites.svg", "<g id=x/>"),
            ],
        ),
        ("nosuch", vec![]),
    ] {
        let (c, r) = server(Some(name), &own);
        assert_eq!(body(&get(&c, &r, "/theme/theme.json")), FARM, "{name}");
        assert_eq!(body(&get(&c, &r, "/theme/sprites.svg")), FARM_SPRITES, "{name}");
        let page = get(&c, &r, "/");
        let warn = format!(
            "<p class=gr data-t=a><span class=k>theme</span><span class=w><b class=a>farm</b> <span class=mt>`{name}` has no valid theme.json</span></span></p>"
        );
        assert_eq!(page.matches(&warn).count(), 1, "{name}: one warning row");
    }
}

#[test]
fn path_traversal_is_refused() {
    let (c, r) = server(Some(".."), &[]);
    std::fs::write(c.config.with_file_name("theme.json"), r#"{"leak":1}"#).unwrap();
    // A theme name that leaves the themes dir is never read: farm.
    assert_eq!(body(&get(&c, &r, "/theme/theme.json")), FARM);
    let (c, r) = server(None, &[]);
    for p in [
        "/theme/../fleet.toml",
        "/theme/%2e%2e/fleet.toml",
        "/theme/../../config/fleet.toml",
        "/theme/fleet.toml",
        "/theme/",
        "/theme/sub/theme.json",
    ] {
        let res = get(&c, &r, p);
        assert!(
            res.starts_with("HTTP/1.1 404 ") && !res.contains("[fleet."),
            "{p}: {res}"
        );
    }
}
