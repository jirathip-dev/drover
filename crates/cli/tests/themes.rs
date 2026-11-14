//! #172/#241: themes. `/themes/<name>/<file>` from `<config dir>/themes/<name>/`, then the built-in theme, then farm's
//! (an icon: the board's); theme.json checked against the contract (docs/themes.md) and served resolved; path
//! traversal refused; an invalid theme.json falls back to farm with a Host warning naming the problem; the `[◐]`
//! picker's default is `[serve] theme`; terminal draws no sprites.
mod common;
use common::*;
use drover::serve::{act, resolve, validate, Server};
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const FARM: &str = include_str!("../../../assets/swag/farm/theme.json");
const FARM_SPRITES: &str = include_str!("../../../assets/swag/farm/sprites.svg");
const KITCHEN: &str = include_str!("../../../assets/swag/kitchen/theme.json");
const KITCHEN_SCENE: &str = include_str!("../../../assets/swag/kitchen/scene.svg");
const KITCHEN_SPRITES: &str = include_str!("../../../assets/swag/kitchen/sprites.svg");
const KITCHEN_ICON: &[u8] = include_bytes!("../../../assets/swag/kitchen/icon-180.png");
const ICON: &[u8] = include_bytes!("../../../assets/icon-180.png");
const TERMINAL_ICON: &[u8] = include_bytes!("../../../assets/terminal/icon-180.png");
const ACTS: [&str; 7] = ["impl", "fix", "rev", "update", "idle", "done", "blocked"];

/// A server whose fleet.toml picks `theme` (none: the default), with user themes `(name, file, body)`.
fn server(theme: Option<&str>, own: &[(&str, &str, &str)]) -> (drover::Ctx, Fake) {
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

fn raw(c: &drover::Ctx, r: &Fake, path: &str) -> Vec<u8> {
    let s = Server::new(c, r, Some(ADDR.into()));
    s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), 1_000_000)
}

/// #267: the Host tab as the board draws it.
fn host(c: &drover::Ctx, r: &Fake) -> String {
    host_tab(&Server::new(c, r, Some(ADDR.into())), ADDR, 1_000_000)
}

fn get(c: &drover::Ctx, r: &Fake, path: &str) -> String {
    String::from_utf8_lossy(&raw(c, r, path)).into_owned()
}

fn body(res: &str) -> &str {
    assert!(res.starts_with("HTTP/1.1 200 OK"), "{res}");
    &res[res.find("\r\n\r\n").unwrap() + 4..]
}

/// The bytes after the head of a 200.
fn bytes(c: &drover::Ctx, r: &Fake, path: &str) -> Vec<u8> {
    let res = raw(c, r, path);
    assert!(res.starts_with(b"HTTP/1.1 200 OK"), "{path}");
    let at = res.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    res[at + 4..].to_vec()
}

fn json(c: &drover::Ctx, r: &Fake, path: &str) -> Value {
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
    for t in [KITCHEN, include_str!("../../../assets/terminal/theme.json")] {
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
    assert!(
        host(&c, &r).contains("<p class=gr data-t=g><span class=k>theme</span><span class=w><b>farm</b></span></p>")
    );
    let page = get(&c, &r, "/?all=1");
    // The picker: this phone's choice, else the board's default, set before the first paint.
    assert!(page.contains("m=m||localStorage.theme}catch(e){}") && page.contains("h.dataset.theme=m||\"farm\";"));
    // #290: the look sheet's rows, no native select for theme or colours; #440: in System's Appearance, behind [⚙].
    assert!(page.contains(
        "<button id=sysb type=button aria-haspopup=dialog aria-label=system>⚙<i class=sd hidden></i></button>"
    ));
    assert!(!page.contains("id=look"));
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
    assert!(
        host(&c, &r).contains("<p class=gr data-t=g><span class=k>theme</span><span class=w><b>kitchen</b></span></p>")
    );
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
            "`load_scene` must be {\"kind\": weather|heat|light|none",
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
        let page = host(&c, &r);
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
    let a = &avatars(&[("x-rev-1", "rev", "working")])[0];
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

/// #370: the board's labels pair a theme's own words with the plain ones; a word the theme lacks is the plain word
/// alone, never farm's (terminal: none, today's board). The Queue and Events rows by kind and the Host load minis
/// are in each sheet, and `row_sprites` takes those kinds and no other.
#[test]
fn theme_words_are_the_themes_own_and_fall_back_to_plain() {
    let (c, r) = server(None, &[("plain", "theme.json", r#"{"words": {"merged": "shipped"}}"#)]);
    let w = |t: &str| json(&c, &r, &format!("/themes/{t}/theme.json"))["words"].clone();
    let (f, k) = (w("farm"), w("kitchen"));
    for (key, farm, kitchen) in [
        ("needs", "at the gate", "the bell"),
        ("working", "on the trail", "on the line"),
        ("idle", "grazing", "standing by"),
        ("done", "in the stable", "at the pass"),
        ("queued", "in the paddock", "tickets"),
        ("impl", "plough", "cook"),
        ("fix", "farrier", "chop"),
        ("rev", "vet", "taste"),
        ("update", "haul", "carry"),
        ("merged", "over the fence", "order up"),
        ("tab-events", "logbook", "the rail"),
        ("tab-host", "barn", "the kitchen"),
        ("empty-queue", "the paddock is empty", "the kitchen's quiet: no tickets"),
    ] {
        assert_eq!((f[key].as_str(), k[key].as_str()), (Some(farm), Some(kitchen)), "{key}");
    }
    assert_eq!(
        w("plain"),
        json!({"merged": "shipped"}),
        "farm's words never fill a theme's gaps"
    );
    assert_eq!(w("terminal"), json!({}));
    for (t, sheet) in [("farm", FARM_SPRITES), ("kitchen", KITCHEN_SPRITES)] {
        let j = json(&c, &r, &format!("/themes/{t}/theme.json"));
        let ids = ["queued", "needs", "merged", "fail"]
            .map(|x| j["row_sprites"][x].clone())
            .into_iter()
            .chain(["calm", "busy", "held"].map(|l| j["load_scene"]["minis"][l].clone()));
        for id in ids {
            let id = id.as_str().unwrap().to_string();
            assert!(sheet.contains(&format!("<g id=\"{id}\"")), "{t} {id}");
        }
    }
    assert_eq!(
        validate(br#"{"row_sprites": {"merged": "a", "bogus": "b"}}"#).unwrap_err(),
        "`row_sprites.bogus` must be a sprite id (keys: impl, fix, rev, update, idle, done, blocked, queued, needs, merged, fail)"
    );
}

const GARDEN: &str = include_str!("../../../assets/swag/garden/theme.json");
const GARDEN_SPRITES: &str = include_str!("../../../assets/swag/garden/sprites.svg");
const GARDEN_SCENE: &str = include_str!("../../../assets/swag/garden/scene.svg");

/// serve.js's pure plants part (#423) plus `js`, run under node: its stdout as JSON. None where node is missing.
fn plants(js: &str) -> Option<Value> {
    if std::process::Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping: no node here");
        return None;
    }
    let src = include_str!("../../../assets/serve.js");
    let part = &src[src.find("// #423 plants").unwrap()..src.find("// plants:end").unwrap()];
    let d = tmpdir("plants-js");
    std::fs::write(d.join("t.js"), format!("{part}\nconsole.log(JSON.stringify({js}))")).unwrap();
    let o = std::process::Command::new("node").arg(d.join("t.js")).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    Some(serde_json::from_slice(&o.stdout).unwrap())
}

/// #423: the garden fills the contract with its own sprites: the keeper's action per role (water, prune, inspect,
/// repot), a plant per growth stage in each of the six species (lane and Queue row size), the window light, the counter's
/// cuttings and the moments, all in its sprites.svg; it is served as a built-in, scene and icon too.
#[test]
fn garden_draws_every_stage_and_species_itself() {
    let g = resolve(&validate(GARDEN.as_bytes()).unwrap(), &farm());
    let has = |id: &str| GARDEN_SPRITES.contains(&format!("<g id=\"{id}\""));
    let acts = ACTS.map(|a| g["actions"][a].as_str().unwrap().to_string());
    assert_eq!(
        acts[..4],
        ["keeper-water", "keeper-prune", "keeper-inspect", "keeper-repot"]
    );
    let mut ids: Vec<Value> = ACTS
        .iter()
        .flat_map(|a| [g["actions"][a].clone(), g["row_sprites"][a].clone()])
        .collect();
    ids.extend(["queued", "needs", "merged", "fail"].map(|k| g["row_sprites"][k].clone()));
    ids.extend(["calm", "busy", "held"].map(|l| g["load_scene"]["minis"][l].clone()));
    ids.extend([&g["rail"]["item"], &g["rail"]["needs"], &g["loading"]].map(Value::clone));
    ids.extend(["merged", "fail", "needs-you"].map(|m| g["moments"][m]["sprite"].clone()));
    for id in ids {
        assert!(has(id.as_str().unwrap()), "{id}");
    }
    let species: Vec<&str> = g["species"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        species,
        [
            "monstera",
            "philodendron",
            "peperomia",
            "alocasia",
            "pothos",
            "calathea"
        ]
    );
    for st in ["queued", "impl", "review", "fix", "ready", "merged", "blocked"] {
        let id = g["stages"][st].as_str().unwrap();
        for s in &species {
            assert!(has(&format!("{id}-{s}")) && has(&format!("{id}-{s}-row")), "{id}-{s}");
        }
    }
    assert_eq!(g["load_scene"]["kind"], "light");
    assert_eq!(g["scheme"], json!({"light": "garden-light", "dark": "garden-dark"}));
    for l in ["l-sun", "l-overcast", "l-rain"] {
        assert!(GARDEN_SCENE.contains(&format!("class=\"{l}")), "{l}");
    }
    let (c, r) = server(Some("garden"), &[]);
    assert_eq!(json(&c, &r, "/themes/garden/theme.json"), g);
    assert_eq!(body(&get(&c, &r, "/themes/garden/scene.svg")), GARDEN_SCENE);
    assert_eq!(
        bytes(&c, &r, "/themes/garden/icon-180.png"),
        include_bytes!("../../../assets/swag/garden/icon-180.png")
    );
    assert!(get(&c, &r, "/").contains("h.dataset.theme=m||\"garden\";"));
}

/// #423 AC: a lane's sprite is its role's action over its issue's plant at its stage; a theme without `stages` keeps
/// the action alone (the role pose), so farm and kitchen draw as before. The species is the issue number's, every time.
#[test]
fn a_lane_draws_its_plant_only_with_stages_and_the_species_is_stable() {
    let farm = resolve(&farm(), &farm());
    let kitchen = resolve(&validate(KITCHEN.as_bytes()).unwrap(), &farm);
    let garden = resolve(&validate(GARDEN.as_bytes()).unwrap(), &farm);
    let js = format!(
        "(() => {{ const F = {farm}, K = {kitchen}, G = {garden}, all = () => true, none = () => false; return [\
         duo(F, all, 'ha-impl', 'impl', 48), duo(K, all, 'ha-fix', 'fix', 7), plant(F, 'ready', 3),\
         duo(G, all, 'ha-impl', 'review', 48), duo(G, i => i === 'hs-plant-fix', 'ha-fix', 'fix', 49),\
         duo(G, none, 'ha-rev', 'review', 50), plant(G, 'queued', 51, 1).ids,\
         [48, 49, 50, 51, 52, 53, 54, 48].map(n => species(G, n).id), species(G, -1), species(F, 4)] }})()"
    );
    let Some(got) = plants(&js) else { return };
    assert_eq!(got[0], "<use href=\"#ha-impl\"></use>", "farm: the role pose alone");
    assert_eq!(got[1], "<use href=\"#ha-fix\"></use>", "kitchen: the role pose alone");
    assert_eq!(got[2], Value::Null);
    assert_eq!(
        got[3],
        "<use href=\"#hs-plant-review-monstera\" style=\"--ld:#2a6a34;--lf:#3f8f45;--lv:#7cc46a\"></use><use href=\"#ha-impl\"></use>"
    );
    assert_eq!(
        got[4],
        "<use href=\"#hs-plant-fix\" style=\"--ld:#356f2a;--lf:#4f9a3a;--lv:#b4dc5a\"></use><use href=\"#ha-fix\"></use>",
        "a sheet without the species' sprite: the stage's own"
    );
    assert_eq!(
        got[5], "<use href=\"#ha-rev\"></use>",
        "no such sprite: the action alone"
    );
    assert_eq!(got[6], json!(["plant-queued-alocasia-row", "plant-queued-row"]));
    assert_eq!(
        got[7],
        json!([
            "monstera",
            "philodendron",
            "peperomia",
            "alocasia",
            "pothos",
            "calathea",
            "monstera",
            "monstera"
        ])
    );
    assert_eq!((&got[8], &got[9]), (&Value::Null, &Value::Null));
}

/// #423: an issue's growth stage from its state, and from its lane's action where the state says nothing.
#[test]
fn the_growth_stage_follows_the_issue() {
    use drover::serve::stage;
    for (state, act, want) in [
        ("queued", "idle", "queued"),
        ("impl", "impl", "impl"),
        ("review(1)", "rev", "review"),
        ("verify(1)", "idle", "review"),
        ("fix(2)", "update", "fix"),
        ("ready", "done", "ready"),
        ("merged", "done", "merged"),
        ("needs-human", "blocked", "blocked"),
        ("review(2)", "blocked", "blocked"),
        ("", "rev", "review"),
        ("", "done", "ready"),
        ("", "idle", "impl"),
    ] {
        assert_eq!(stage(state, act), want, "{state} {act}");
    }
}

/// #423: the new keys' shapes; a wrong one names itself and the theme is farm.
#[test]
fn stages_species_and_a_light_dark_scheme_are_checked() {
    for (bad, err) in [
        (
            r#"{"stages": {"sprout": "x"}}"#,
            "`stages.sprout` must be a sprite id (keys: queued, impl, review",
        ),
        (r#"{"stages": {"impl": 1}}"#, "`stages.impl` must be a sprite id"),
        (
            r##"{"species": [{"--lf": "#fff"}]}"##,
            "`species` must be a list of {\"id\": suffix",
        ),
        (r#"{"species": []}"#, "`species`"),
        (r##"{"species": [{"id": "a", "lf": "#fff"}]}"##, "`species`"),
        (
            r#"{"scheme": {"light": "paper"}}"#,
            "`scheme` must be a colour scheme's name, or {\"light\"",
        ),
        (r#"{"scheme": {"light": "paper", "dark": 2}}"#, "`scheme`"),
        (r#"{"scheme": 3}"#, "`scheme`"),
    ] {
        let e = validate(bad.as_bytes()).unwrap_err();
        assert!(e.contains(err), "{bad}: {e}");
    }
    let ok = r##"{"stages": {"ready": "p"}, "species": [{"id": "fern", "--lf": "#0f0"}], "scheme": "paper",
        "load_scene": {"kind": "light", "levels": {"calm": "sun"}}}"##;
    validate(ok.as_bytes()).unwrap();
    validate(br#"{"scheme": {"light": "paper", "dark": "nord"}}"#).unwrap();
}

#[test]
fn pinned_packs_load_and_a_custom_theme_overrides_them() {
    use sha2::Digest;
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/swag");
    let source = std::fs::read_to_string(dir.join("SOURCE")).unwrap();
    let pin = source
        .trim()
        .strip_prefix("jirathip-dev/swag legacy-board at ")
        .unwrap();
    assert!(
        pin.len() == 40 && pin.bytes().all(|b| b.is_ascii_hexdigit()),
        "{source}"
    );
    let hex = |b: &[u8]| -> String { sha2::Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect() };
    let sums = std::fs::read_to_string(dir.join("SHA256SUMS")).unwrap();
    let pinned: Vec<(&str, &str)> = sums.lines().map(|l| l.split_once("  ").unwrap()).collect();
    let mut on_disk = vec![];
    for t in ["farm", "kitchen", "garden"] {
        for e in std::fs::read_dir(dir.join(t)).unwrap() {
            on_disk.push(format!("{t}/{}", e.unwrap().file_name().to_string_lossy()));
        }
    }
    on_disk.sort();
    assert_eq!(
        on_disk,
        pinned.iter().map(|p| p.1).collect::<Vec<_>>(),
        "every pack file is pinned"
    );
    let (c, r) = server(None, &[]);
    for (sum, path) in &pinned {
        let file = std::fs::read(dir.join(path)).unwrap();
        assert_eq!(&hex(&file), sum, "{path} is the pinned commit's");
        let served = bytes(&c, &r, &format!("/themes/{path}"));
        if path.ends_with("theme.json") {
            let own = validate(&file).unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&served).unwrap(),
                resolve(&own, &farm()),
                "{path}"
            );
        } else {
            assert_eq!(&hex(&served), sum, "{path} served as pinned");
        }
    }
    let (c, r) = server(Some("garden"), &[("garden", "sprites.svg", "<g id=\"mine\"/>")]);
    assert_eq!(body(&get(&c, &r, "/themes/garden/sprites.svg")), "<g id=\"mine\"/>");
    assert_eq!(body(&get(&c, &r, "/themes/garden/scene.svg")), GARDEN_SCENE);
}
