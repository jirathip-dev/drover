//! #172/#241: themes. `/themes/<name>/<file>` from `<config dir>/themes/<name>/`, then the built-in theme, then farm's
//! (an icon: the board's); theme.json checked against the contract (docs/themes.md) and served resolved; path
//! traversal refused; an invalid theme.json falls back to farm with a Host warning naming the problem; the `[◐]`
//! picker's default is `[serve] theme`; terminal draws no sprites.
mod common;
use common::*;
use herdsman::serve::{act, resolve, validate, Server};
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const FARM: &str = include_str!("../../../assets/themes/farm/theme.json");
const FARM_SPRITES: &str = include_str!("../../../assets/themes/farm/sprites.svg");
const KITCHEN: &str = include_str!("../../../assets/themes/kitchen/theme.json");
const KITCHEN_SCENE: &str = include_str!("../../../assets/themes/kitchen/scene.svg");
const KITCHEN_SPRITES: &str = include_str!("../../../assets/themes/kitchen/sprites.svg");
const KITCHEN_ICON: &[u8] = include_bytes!("../../../assets/themes/kitchen/icon-180.png");
const ICON: &[u8] = include_bytes!("../../../assets/icon-180.png");
const TERMINAL_ICON: &[u8] = include_bytes!("../../../assets/themes/terminal/icon-180.png");
const ACTS: [&str; 7] = ["impl", "fix", "rev", "update", "idle", "done", "blocked"];

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

fn raw(c: &herdsman::Ctx, r: &Fake, path: &str) -> Vec<u8> {
    let s = Server::new(c, r, Some(ADDR.into()));
    s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), 1_000_000)
}

fn get(c: &herdsman::Ctx, r: &Fake, path: &str) -> String {
    String::from_utf8_lossy(&raw(c, r, path)).into_owned()
}

fn body(res: &str) -> &str {
    assert!(res.starts_with("HTTP/1.1 200 OK"), "{res}");
    &res[res.find("\r\n\r\n").unwrap() + 4..]
}

/// The bytes after the head of a 200.
fn bytes(c: &herdsman::Ctx, r: &Fake, path: &str) -> Vec<u8> {
    let res = raw(c, r, path);
    assert!(res.starts_with(b"HTTP/1.1 200 OK"), "{path}");
    let at = res.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    res[at + 4..].to_vec()
}

fn json(c: &herdsman::Ctx, r: &Fake, path: &str) -> Value {
    serde_json::from_str(body(&get(c, r, path))).unwrap()
}

fn farm() -> Value {
    serde_json::from_str(FARM).unwrap()
}

#[test]
fn farm_fills_the_whole_contract() {
    let f = validate(FARM.as_bytes()).unwrap();
    for a in ACTS {
        assert!(f["actions"][a].is_string(), "farm action {a}");
    }
    for k in [
        "looks",
        "load_scene",
        "rail",
        "moments",
        "row_sprites",
        "icons",
        "ascii",
        "modes",
    ] {
        assert!(f.get(k).is_some(), "farm has {k}");
    }
    // #242: farm fills every key itself, no fallback: an action, row sprite and ASCII art per action, both rail
    // sprites, the three moments.
    for a in ACTS {
        assert!(f["row_sprites"][a].is_string() && f["ascii"][a].is_string(), "farm {a}");
        assert_eq!(
            f["ascii_rows"][a].as_str().map(|r| r.chars().count()),
            Some(3),
            "#335 farm {a}"
        );
        assert_ne!(f["row_sprites"][a], f["actions"][a], "farm row sprite {a}");
    }
    assert!(f["rail"]["item"].is_string() && f["rail"]["needs"].is_string());
    assert_ne!(f["rail"]["item"], f["actions"]["idle"]);
    assert_ne!(f["rail"]["needs"], f["rail"]["item"]);
    for m in ["merged", "fail", "needs-you"] {
        assert!(f["moments"][m]["sprite"].is_string(), "farm moment {m}");
    }
    // Every sprite farm names is in its sprites.svg.
    let r = resolve(&f, &f);
    for k in ["actions", "row_sprites", "rail", "moments", "ascii"] {
        assert_eq!(r[k], f[k], "farm's own {k}");
    }
    let ids = ACTS
        .map(|a| r["actions"][a].clone())
        .into_iter()
        .chain(ACTS.map(|a| r["row_sprites"][a].clone()))
        .chain([r["rail"]["item"].clone(), r["rail"]["needs"].clone()]);
    for id in ids.chain(["merged", "fail", "needs-you"].map(|m| r["moments"][m]["sprite"].clone())) {
        let id = id.as_str().unwrap().to_string();
        assert!(FARM_SPRITES.contains(&format!("id=\"{id}\"")), "{id}");
    }
    assert_eq!(r["modes"], json!(["scene", "ascii"]));
    assert_eq!(r["load_scene"]["kind"], "weather");
    for t in [KITCHEN, include_str!("../../../assets/themes/terminal/theme.json")] {
        validate(t.as_bytes()).unwrap();
    }
}

/// #239: the kitchen fills the contract with its own sprites: an action per role (cook, chop, taste, carry), each
/// state's, the rail, the moments, the 16 px rows and the held-heat fan, all in its sprites.svg; nothing falls to farm.
#[test]
fn kitchen_draws_every_action_itself() {
    let k = resolve(&validate(KITCHEN.as_bytes()).unwrap(), &farm());
    let acts = ACTS.map(|a| k["actions"][a].as_str().unwrap().to_string());
    assert_eq!(acts[..4], ["chef-cook", "chef-chop", "chef-taste", "chef-carry"]);
    let mut ids: Vec<Value> = ACTS
        .iter()
        .flat_map(|a| [k["actions"][a].clone(), k["row_sprites"][a].clone()])
        .collect();
    ids.extend([&k["rail"]["item"], &k["rail"]["needs"], &k["load_scene"]["fan"]].map(Value::clone));
    ids.extend(["merged", "fail", "needs-you"].map(|m| k["moments"][m]["sprite"].clone()));
    for id in ids {
        let id = id.as_str().unwrap().to_string();
        assert!(KITCHEN_SPRITES.contains(&format!("<g id=\"{id}\"")), "{id}");
    }
    assert_eq!(
        (&k["load_scene"]["kind"], k["looks"].as_array().unwrap().len()),
        (&json!("heat"), 8)
    );
    assert_eq!(k["ascii"]["fix"], "♨\nchop");
    // #335: the ascii style's own figures (3 characters per row), heat words and loading frames, none of them farm's.
    for a in ACTS {
        assert_eq!(k["ascii_rows"][a].as_str().unwrap().chars().count(), 3, "{a}");
        assert_ne!(k["ascii_rows"][a], farm()["ascii_rows"][a], "{a}");
    }
    assert_eq!(
        (&k["ascii_load"]["held"], &k["ascii_loading"]),
        (&json!("^^^^^"), &json!("c~@\nc~o"))
    );
    // The scene's three heat layers.
    for l in ["h-calm", "h-busy", "h-held", "ksmoke", "kglow"] {
        assert!(KITCHEN_SCENE.contains(l), "{l}");
    }
}

#[test]
fn default_theme_is_farm() {
    let (c, r) = server(None, &[]);
    assert_eq!(json(&c, &r, "/themes/farm/theme.json"), resolve(&farm(), &farm()));
    assert_eq!(body(&get(&c, &r, "/themes/farm/sprites.svg")), FARM_SPRITES);
    assert!(
        get(&c, &r, "/themes/farm/scene.svg").starts_with("HTTP/1.1 404 "),
        "farm draws its scene in herd.js"
    );
    let page = get(&c, &r, "/?all=1");
    assert!(page.contains("<p class=gr data-t=g><span class=k>theme</span><span class=w><b>farm</b></span></p>"));
    // The picker: this phone's choice, else the board's default, set before the first paint.
    assert!(page.contains("m=m||localStorage.theme}catch(e){}") && page.contains("h.dataset.theme=m||\"farm\";"));
    // #290: one `[◐]` opens the look sheet (serve.js); no native select for theme or colours.
    assert!(page.contains(
        "<button id=look type=button aria-haspopup=dialog aria-label=\"look: theme and colours\">◐</button>"
    ));
    assert!(!page.contains("id=theme") && !page.contains("id=scheme"));
}

#[test]
fn the_picker_default_is_serve_theme_and_lists_your_own() {
    let (c, r) = server(Some("kitchen"), &[]);
    assert!(get(&c, &r, "/?all=1").contains("h.dataset.theme=m||\"kitchen\";"));
    let (c, r) = server(Some("space"), &[("space", "theme.json", "{}")]);
    let page = get(&c, &r, "/?all=1");
    assert!(page.contains("h.dataset.theme=m||\"space\";"));
    // The look sheet lists SD.t's keys: the built-ins and your own.
    assert!(page.contains("\"kitchen\":\"kitchen\",\"space\":") && page.contains(",\"terminal\":\"github-dark\"}"));
    // A name that would close the script stays a string.
    let (c, r) = server(Some("</script>"), &[]);
    assert!(get(&c, &r, "/?all=1").contains("h.dataset.theme=m||\"\\u003c/script>\";"));
}

#[test]
fn lookup_is_user_then_built_in_then_farm() {
    // Built-in kitchen: its own files, its theme.json resolved.
    let (c, r) = server(Some("kitchen"), &[]);
    let k: Value = serde_json::from_str(KITCHEN).unwrap();
    assert_eq!(json(&c, &r, "/themes/kitchen/theme.json"), resolve(&k, &farm()));
    assert_eq!(body(&get(&c, &r, "/themes/kitchen/scene.svg")), KITCHEN_SCENE);
    assert!(get(&c, &r, "/?all=1")
        .contains("<p class=gr data-t=g><span class=k>theme</span><span class=w><b>kitchen</b></span></p>"));
    // Any phone may pick another theme than the board's.
    assert_eq!(json(&c, &r, "/themes/farm/theme.json"), resolve(&farm(), &farm()));
    // A user theme named like a built-in wins, file by file; what it lacks is the built-in's.
    let own = r#"{"poses":{"working":"w"}}"#;
    let (c, r) = server(Some("kitchen"), &[("kitchen", "theme.json", own)]);
    assert_eq!(json(&c, &r, "/themes/kitchen/theme.json")["actions"]["impl"], "w");
    assert_eq!(body(&get(&c, &r, "/themes/kitchen/scene.svg")), KITCHEN_SCENE);
    // A user-only theme: its files, then farm's.
    let (c, r) = server(
        Some("space"),
        &[("space", "theme.json", own), ("space", "sprites.svg", "<g id=w/>")],
    );
    assert_eq!(body(&get(&c, &r, "/themes/space/sprites.svg")), "<g id=w/>");
    assert!(get(&c, &r, "/themes/space/sprites.svg").contains("Content-Type: image/svg+xml"));
    assert!(get(&c, &r, "/themes/space/theme.json").contains("Content-Type: application/json"));
    let (c, r) = server(Some("space"), &[("space", "theme.json", own)]);
    assert_eq!(body(&get(&c, &r, "/themes/space/sprites.svg")), FARM_SPRITES);
}

/// #241: each fallback step: an action is the theme's own, else its pose's sprite, else farm's; a row sprite its
/// action's; the rail item its idle action, the needs variant the item; other maps key by key; `false` stays.
#[test]
fn every_fallback_step() {
    let own = json!({
        "poses": {"working": "p-work", "done": "p-done"},
        "actions": {"fix": "a-fix"},
        "row_sprites": {"rev": "r-rev"},
        "glyphs": {"working": "♨"},
        "ascii": {"impl": "chop"},
    });
    let t = resolve(&validate(own.to_string().as_bytes()).unwrap(), &farm());
    let f = farm();
    assert_eq!(t["actions"]["fix"], "a-fix", "own action");
    assert_eq!(t["actions"]["impl"], "p-work", "own pose");
    assert_eq!(t["actions"]["update"], "p-work");
    assert_eq!(t["actions"]["done"], "p-done");
    assert_eq!(t["actions"]["idle"], f["actions"]["idle"], "farm's action");
    assert_eq!(t["row_sprites"]["rev"], "r-rev", "own row sprite");
    assert_eq!(t["row_sprites"]["fix"], "a-fix", "the action");
    assert_eq!(t["row_sprites"]["blocked"], f["actions"]["blocked"]);
    assert_eq!(t["rail"]["item"], f["actions"]["idle"], "rail: the idle action");
    assert_eq!(t["rail"]["needs"], t["rail"]["item"], "needs: the item");
    assert_eq!(t["glyphs"]["working"], "♨");
    assert_eq!(t["glyphs"]["done"], f["glyphs"]["done"], "key by key");
    assert_eq!(t["ascii"]["impl"], "chop");
    assert_eq!(t["ascii"]["fix"], f["ascii"]["fix"], "farm's ASCII horse");
    assert_eq!(
        (&t["modes"], &t["load_scene"], &t["icons"]),
        (&f["modes"], &f["load_scene"], &f["icons"])
    );
    let r = resolve(&json!({"rail": {"item": "bale"}}), &f);
    assert_eq!(
        (&r["rail"]["item"], &r["rail"]["needs"]),
        (&json!("bale"), &json!("bale"))
    );
    let r = resolve(
        &json!({"rail": {"item": "bale", "needs": "red-bale"}, "row_sprites": false}),
        &f,
    );
    assert_eq!(
        (&r["rail"]["needs"], &r["row_sprites"]),
        (&json!("red-bale"), &json!(false))
    );
}

/// #241: the contract's rules, each with the error the Host tab shows.
#[test]
fn schema_validation_names_the_problem() {
    for (bad, err) in [
        ("{ not json", "not JSON"),
        ("[1]", "not a JSON object"),
        (
            r#"{"actions": {"cook": "x"}}"#,
            "`actions.cook` must be a sprite id (keys: impl, fix, rev, update, idle, done, blocked)",
        ),
        (r#"{"actions": {"impl": 3}}"#, "`actions.impl` must be a sprite id"),
        (r#"{"poses": {"running": "x"}}"#, "`poses.running`"),
        (r#"{"actions": []}"#, "`actions` must be an object"),
        (r#"{"glyphs": {"x": 1}}"#, "`glyphs.x` must be a string"),
        (r#"{"rail": {"bale": "x"}}"#, "`rail.bale`"),
        (r#"{"row_sprites": true}"#, "`row_sprites` must be an object"),
        (
            r#"{"moments": {"merged": {"anim": "spin"}}}"#,
            "`moments.merged` must be {\"sprite\": id, \"anim\": hop|shake|pulse}",
        ),
        (r#"{"moments": {"party": {}}}"#, "`moments.party`"),
        (
            r#"{"icons": {"180": "../fleet.toml"}}"#,
            "`icons.180` must be icon-180.png, icon-192.png or icon-512.png",
        ),
        (r#"{"icons": {"64": "icon-180.png"}}"#, "`icons.64`"),
        (
            r#"{"ascii": {"impl": "ninechars"}}"#,
            "`ascii.impl` must be at most 3 lines of 8 narrow characters",
        ),
        (r#"{"ascii": {"impl": "a\nb\nc\nd"}}"#, "`ascii.impl`"),
        (r#"{"ascii": {"impl": "鍋"}}"#, "`ascii.impl`"),
        (r#"{"ascii": {"impl": "🔥"}}"#, "`ascii.impl`"),
        (
            r#"{"ascii_rows": {"impl": "~h~~"}}"#,
            "`ascii_rows.impl` must be one line of at most 3 narrow characters",
        ),
        (r#"{"ascii_rows": {"impl": "h\nh"}}"#, "`ascii_rows.impl`"),
        (r#"{"ascii_load": {"hot": "~"}}"#, "`ascii_load.hot`"),
        (r#"{"ascii_load": {"calm": "\\O/ \\O/"}}"#, "`ascii_load.calm`"),
        (
            r#"{"ascii_loading": "a\nb\nc\nd"}"#,
            "`ascii_loading` must be at most 3 lines",
        ),
        (r#"{"scene": 1}"#, "`scene` must be an object"),
        (
            r#"{"looks": [{"c": "red"}]}"#,
            "`looks` must be a list of {\"--var\": \"value\"} objects",
        ),
        (
            r#"{"load_scene": {"kind": "rain"}}"#,
            "`load_scene` must be {\"kind\": weather|heat|none",
        ),
        (
            r#"{"load_scene": {"kind": "heat", "levels": {"hot": "x"}}}"#,
            "`load_scene`",
        ),
        (
            r#"{"modes": []}"#,
            "`modes` must be a list of scene, ascii, glyph, each at most once",
        ),
        (r#"{"modes": ["scene", "scene"]}"#, "`modes`"),
        (r#"{"modes": ["horse"]}"#, "`modes`"),
    ] {
        let e = validate(bad.as_bytes()).unwrap_err();
        assert!(e.contains(err), "{bad}: {e}");
    }
    let ok = r##"{"ascii": {"impl": "» chop", "done": "_/>\n(__)"}, "looks": [{"--c": "#fff"}], "moments": false,
        "load_scene": {"kind": "none"}, "icons": {"favicon": "icon-192.png"}, "modes": ["glyph"], "rail": {"needs": "x"}}"##;
    validate(ok.as_bytes()).unwrap();
}

#[test]
fn invalid_theme_json_falls_back_to_farm_with_a_host_warning() {
    for (name, own, why) in [
        (
            "broken",
            vec![
                ("broken", "theme.json", "{ not json"),
                ("broken", "sprites.svg", "<g id=x/>"),
            ],
            "not JSON (key must be a string at line 1 column 3)",
        ),
        (
            "modes",
            vec![("modes", "theme.json", r#"{"modes": ["horse"]}"#)],
            "`modes` must be a list of scene, ascii, glyph, each at most once",
        ),
        ("nosuch", vec![], "no theme.json"),
    ] {
        let (c, r) = server(Some(name), &own);
        assert_eq!(
            json(&c, &r, &format!("/themes/{name}/theme.json")),
            resolve(&farm(), &farm()),
            "{name}"
        );
        assert_eq!(
            body(&get(&c, &r, &format!("/themes/{name}/sprites.svg"))),
            FARM_SPRITES,
            "{name}"
        );
        let page = get(&c, &r, "/?all=1");
        let warn = format!(
            "<p class=gr data-t=a><span class=k>theme</span><span class=w><b class=a>farm</b> <span class=mt>`{name}`: {}</span></span></p>",
            why.replace('"', "&quot;")
        );
        assert_eq!(page.matches(&warn).count(), 1, "{name}: one warning row\n{page}");
    }
}

/// #241: terminal is today's plain board: glyph mode only, no row sprites, no moments, the load as text.
#[test]
fn terminal_has_no_sprites() {
    let (c, r) = server(Some("terminal"), &[]);
    let t = json(&c, &r, "/themes/terminal/theme.json");
    assert_eq!(
        (&t["modes"], &t["row_sprites"], &t["moments"], &t["load_scene"]["kind"]),
        (&json!(["glyph"]), &json!(false), &json!(false), &json!("none"))
    );
    // The page is the same for every theme: the sprites are the phone's (serve.js, only when `row_sprites`).
    let page = get(&c, &r, "/?all=1");
    assert!(
        !page.contains("rsp") && !page.contains("#hr-") && !page.contains("class=lsc"),
        "{page}"
    );
    assert!(page.contains("h.dataset.theme=m||\"terminal\";"));
}

/// #305: the busy `[↻]` is a theme's own `loading` sprite: farm's trotting horse and the kitchen's cooking chef, each in
/// its sprites.svg; terminal and a theme without the key keep the spinning ↻ (never farm's horse).
#[test]
fn loading_is_the_themes_own_or_the_spinning_glyph() {
    let (c, r) = server(None, &[("plain", "theme.json", "{}")]);
    for (t, id, sheet) in [
        ("farm", "horse-working", FARM_SPRITES),
        ("kitchen", "chef-cook", KITCHEN_SPRITES),
    ] {
        assert_eq!(json(&c, &r, &format!("/themes/{t}/theme.json"))["loading"], id, "{t}");
        assert!(sheet.contains(&format!("id=\"{id}\"")), "{t}: {id}");
    }
    for t in ["terminal", "plain"] {
        assert_eq!(
            json(&c, &r, &format!("/themes/{t}/theme.json"))["loading"],
            Value::Null,
            "{t}"
        );
    }
}

#[test]
fn the_theme_icon_route_and_its_fallback() {
    let (c, r) = server(None, &[("space", "theme.json", "{}"), ("space", "icon-192.png", "own")]);
    assert_eq!(bytes(&c, &r, "/themes/terminal/icon-180.png"), TERMINAL_ICON);
    assert!(get(&c, &r, "/themes/terminal/icon-512.png").contains("Content-Type: image/png"));
    // #239: the kitchen's own icon; no icon of its own (or no such theme): the board's.
    assert_eq!(bytes(&c, &r, "/themes/kitchen/icon-180.png"), KITCHEN_ICON);
    assert_eq!(bytes(&c, &r, "/themes/space/icon-180.png"), ICON);
    assert_eq!(bytes(&c, &r, "/themes/nosuch/icon-180.png"), ICON);
    assert_eq!(bytes(&c, &r, "/themes/space/icon-192.png"), b"own");
    assert_eq!(
        json(&c, &r, "/themes/terminal/theme.json")["icons"],
        json!({"180": "icon-180.png", "192": "icon-192.png", "512": "icon-512.png", "favicon": "icon-192.png"})
    );
}

/// #241: a lane's sprite is its role's action while it works, else its state's; an issue's from the log's state.
#[test]
fn per_role_action_resolution() {
    for (role, state, want) in [
        ("impl", "working", "impl"),
        ("rev", "working", "rev"),
        ("fix", "working", "fix"),
        ("update", "working", "update"),
        ("design", "working", "impl"),
        ("rev", "idle", "idle"),
        ("rev", "unknown", "idle"),
        ("impl", "parked", "idle"),
        ("impl", "done", "done"),
        ("rev", "blocked", "blocked"),
        ("impl", "dead", "blocked"),
        // An issue's state from the log (`update`: the fix round updates the branch).
        ("", "impl", "impl"),
        ("", "review(2)", "rev"),
        ("", "fix(1)", "fix"),
        ("update", "fix(1)", "update"),
        ("", "ready", "done"),
        ("", "merged", "done"),
        ("", "queued", "idle"),
        ("", "needs-human", "blocked"),
        ("", "⏸ waiting: decision", "blocked"),
    ] {
        assert_eq!(act(role, state), want, "{role} {state}");
    }
    let a = herdsman::serve::avatar("x-rev-1", "rev", "working");
    assert!(a.contains("data-act=rev") && a.contains("<use href=#ha-rev />"), "{a}");
}

#[test]
fn path_traversal_is_refused() {
    let (c, r) = server(Some(".."), &[]);
    std::fs::write(c.config.with_file_name("theme.json"), r#"{"leak":1}"#).unwrap();
    // A theme name that leaves the themes dir is never read: farm.
    assert_eq!(json(&c, &r, "/themes/../theme.json"), resolve(&farm(), &farm()));
    assert_eq!(json(&c, &r, "/themes/%2e%2e/theme.json"), resolve(&farm(), &farm()));
    let (c, r) = server(None, &[]);
    for p in [
        "/themes/farm/../fleet.toml",
        "/themes/../../config/fleet.toml",
        "/themes/farm/fleet.toml",
        "/themes/",
        "/themes//theme.json",
        "/themes/farm/sub/theme.json",
        "/theme/theme.json",
    ] {
        let res = get(&c, &r, p);
        assert!(
            res.starts_with("HTTP/1.1 404 ") && !res.contains("[fleet."),
            "{p}: {res}"
        );
    }
}
