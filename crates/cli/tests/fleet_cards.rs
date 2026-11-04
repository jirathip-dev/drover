//! #162: the Queue tab as one card per fleet (colour by a stable name hash or `color`, fleets needing you first, a
//! paused fleet collapsed to one line) and the [config] sheet (every group; its pickers write through `set`).
mod common;
use common::*;
use herdsman::config::Config;
use herdsman::events::{append, read, Event};
use herdsman::serve::Server;
use serde_json::json;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

/// `alpha` works, `zeta` needs you (#5 needs-human), `beta` is paused and needs you (an open ask on #9).
fn config(extra: &str) -> String {
    let fleet = |n: &str, more: &str| {
        format!(
            "[fleet.{n}]\nrepo = \"acme/{n}\"\nlocal = \"/tmp/does-not-matter/{n}\"\nintegration = \"staging\"\n{more}\n\
             [fleet.{n}.role.impl]\nharness = \"claude\"\nmodel = \"opus\"\n\n"
        )
    };
    format!(
        "models = [\"opus\", \"sonnet\"]\n\n{}{}{}{}",
        fleet("alpha", extra),
        fleet("beta", "paused = true\n"),
        fleet("zeta", ""),
        fleet("gamma", "paused = true\n"),
    )
}

/// #268: a page as the board shows it, its Queue tab drawn by web/src/queue.ts.
fn get(s: &Server, path: &str) -> String {
    let res =
        String::from_utf8_lossy(&s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), T)).into_owned();
    with_queue(&res, s, ADDR, T)
}

/// #268: a fleet's [config] sheet as the board draws it from API v1's `queue`.
fn sheet(s: &Server, fleet: &str) -> String {
    queue_js(s, ADDR, T, &format!("sheet(d, {fleet:?})"))
}

fn fixture(name: &str) -> herdsman::Ctx {
    let c = ctx(&tmpdir(name), &config(""));
    let ok = |args: &[&str]| assert_eq!(run(&c, &Fake::default(), args).0, 0, "{args:?}");
    ok(&["queue", "alpha", "add", "1", "2"]);
    ok(&["queue", "zeta", "add", "4", "5"]);
    ok(&["queue", "beta", "add", "9"]);
    let ev = |f: &str, n: u64, kind: &str, data: serde_json::Value| {
        append(&c.state, &Event::new(900_000, f, Some(n), kind, data)).unwrap();
    };
    ev(
        "alpha",
        1,
        "spawn",
        json!({"role": "impl", "agent": "alpha-impl-1", "harness": "claude"}),
    );
    ev(
        "zeta",
        5,
        "needs-human",
        json!({"code": "ROUND_LIMIT", "reason": "rounds ran out"}),
    );
    ev(
        "beta",
        9,
        "needs-decision",
        json!({"id": "q1", "question": "which?", "options": ["a", "b"], "kind": "owner", "recommend": 1}),
    );
    c
}

#[test]
fn colour_is_a_stable_name_hash_and_color_overrides_it() {
    let load = |extra: &str| {
        let d = tmpdir("fc-colour");
        std::fs::write(d.join("fleet.toml"), config(extra)).unwrap();
        Config::load(&d.join("fleet.toml"))
    };
    let c = load("").unwrap();
    // sha256(name)[0] % 5 over [l, p, c, g, a] (docs/design/fleet-cards): the same on every load.
    let auto: Vec<&str> = ["alpha", "beta", "zeta", "gamma"]
        .iter()
        .map(|f| c.fleet[*f].color(f))
        .collect();
    let again = load("").unwrap();
    assert_eq!(
        auto,
        ["alpha", "beta", "zeta", "gamma"].map(|f| again.fleet[f].color(f))
    );
    for f in ["alpha", "beta", "zeta", "gamma"] {
        let want = herdsman::config::COLORS
            [usize::from_str_radix(&herdsman::devices::sha256(f.as_bytes())[..2], 16).unwrap() % 5];
        assert_eq!(c.fleet[f].color(f), want, "{f}");
    }
    assert_eq!(load("color = \"p\"").unwrap().fleet["alpha"].color("alpha"), "p");
    let e = format!("{:#}", load("color = \"r\"").unwrap_err());
    assert!(
        e.contains("fleet.alpha.color: \"r\" must be one of [\"l\", \"p\", \"c\", \"g\", \"a\"]"),
        "{e}"
    );
    // The card carries it.
    let d = tmpdir("fc-colour-page");
    let c = ctx(&d, &config("color = \"c\""));
    let page = get(&Server::new(&c, &Fake::default(), Some(ADDR.into())), "/");
    assert!(
        page.contains("<div class=\"fc k-c\"><div class=fh><span class=fn><b>alpha</b>"),
        "{page}"
    );
}

#[test]
fn fleets_that_need_you_go_first_then_by_name_and_a_paused_one_collapses() {
    let c = fixture("fc-order");
    let page = get(&Server::new(&c, &Fake::default(), Some(ADDR.into())), "/");
    let at = |n: &str| {
        page.find(&format!("<b>{n}</b>"))
            .unwrap_or_else(|| panic!("no {n}: {page}"))
    };
    // Need you: beta (paused, an ask), zeta (needs-human); then alpha, gamma by name.
    assert!(at("beta") < at("zeta") && at("zeta") < at("alpha") && at("alpha") < at("gamma"));
    // A paused fleet is one collapsed line with its need-you count and [resume]; no header, no [config].
    let beta = &page[page[..at("beta")].rfind("<details class=\"fc off").unwrap()..];
    assert!(
        beta.starts_with(&format!(
            "<details class=\"fc off k-{}\"><summary><i class=cv></i><span class=fn><b>beta</b> <i class=m>●</i> paused · no lane</span>\
             <span class=\"tag r\">! 1 need you</span><form method=post>",
            Config::load(&c.config).unwrap().fleet["beta"].color("beta")
        )),
        "{beta}"
    );
    assert!(beta[..beta.find("</summary>").unwrap()].contains("<button>Resume</button>"));
    let gamma = &page[at("gamma")..];
    assert!(!gamma.contains("class=fh") && !gamma.contains("class=cfg"), "{gamma}");
    // A working card: the header, [pause] [config], `need you` only where rows need you; needs-you rows first.
    let zeta = &page[at("zeta")..at("alpha")];
    assert!(zeta.contains("<span class=\"tag r\">! 1 need you</span>"), "{zeta}");
    assert!(
        zeta.find(">#5</a>") < zeta.find(">#4</a>"),
        "needs-you row first: {zeta}"
    );
    assert!(zeta.contains("<span class=r>needs-human</span>"));
    let alpha = &page[at("alpha")..at("gamma")];
    assert!(
        !alpha.contains("need you") && alpha.contains("<button>Pause</button>"),
        "{alpha}"
    );
    assert!(alpha.contains("<button type=button class=cfg data-f=\"alpha\">Config</button>"));
    assert!(alpha.contains("<details class=\"dp a\"><summary><i>●</i><span>drive · no pass yet</span><i class=cv></i></summary><ul><li>stall check: #1 is due now</li><li>1 lane</li>"));
}

#[test]
fn the_config_sheet_shows_every_group_and_its_pickers_post_through_set() {
    let c = fixture("fc-config");
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let res = sheet(&s, "alpha");
    for g in [
        "<h3>General<span>edit fleet.toml</span></h3>",
        "<h3>Limits<span>edit fleet.toml</span></h3>",
        "<h3>Roles<span>set: harness · model · effort</span></h3>",
        "<h3>Alerts<span>edit fleet.toml</span></h3>",
        "<h3>Hosts / needs<span>after #157</span></h3>",
    ] {
        assert!(res.contains(g), "{g}: {res}");
    }
    let path = c.config.display().to_string();
    assert!(res.contains("<p class=cp><span class=m>config</span> ") && res.contains(&path[path.len() - 20..]));
    assert!(res.contains("<dt>repo</dt><dd>acme/alpha</dd>") && res.contains("<dt>line cap</dt>"));
    assert!(res.contains("<select name=harness aria-label=harness><option selected data-e=\"none low medium high xhigh\">claude</option>"));
    assert!(res.contains(
        "<select name=model aria-label=model><option selected>opus</option><option>sonnet</option></select>"
    ));
    assert!(res.contains("<select name=effort aria-label=effort>"));
    // The pickers write through `set`: the same validation (dsh takes only effort none) and the same events.
    let i = res.find("name=mtime value=").unwrap() + 17;
    let mt: String = res[i..].chars().take_while(char::is_ascii_digit).collect();
    let post = |body: &str| {
        let req = format!(
            "POST / HTTP/1.1\r\nHost: {ADDR}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        String::from_utf8_lossy(&s.respond(&req, T)).into_owned()
    };
    let bad = post(&format!(
        "op=set&fleet=alpha&role=impl&harness=dsh&model=opus&effort=high&mtime={mt}"
    ));
    assert!(bad.contains("does%20not%20support%20effort"), "{bad}");
    let ok = post(&format!(
        "op=set&fleet=alpha&role=impl&harness=dsh&model=opus&effort=none&mtime={mt}"
    ));
    assert!(ok.contains("Location: /?msg=alpha%20impl%3A%20harness%20dsh"), "{ok}");
    let text = std::fs::read_to_string(&c.config).unwrap();
    assert!(
        text.contains("[fleet.alpha.role.impl]\nharness = \"dsh\"\nmodel = \"opus\"\neffort = \"none\"\n"),
        "{text}"
    );
    let sets: Vec<_> = read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "set")
        .collect();
    assert_eq!(sets.len(), 2, "harness and effort changed, model kept");
    assert_eq!(sets[0].data["key"], "harness");
    assert_eq!(sets[0].data["via"], "web");
    // No `models`: the model picker is disabled with the fix, so a set never sends it.
    let d = tmpdir("fc-config-nomodels");
    let c = ctx(&d, &config("").replace("models = [\"opus\", \"sonnet\"]\n", ""));
    let res = sheet(&Server::new(&c, &Fake::default(), Some(ADDR.into())), "alpha");
    assert!(
        res.contains("<select name=model aria-label=model disabled data-off>"),
        "{res}"
    );
    assert!(res.contains("add models = [...] to fleet.toml"));
}
