mod common;
use common::*;
use drover::config::Config;
use drover::events::{append, read, Event};
use drover::serve::Server;
use serde_json::json;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

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

fn fleets(s: &Server) -> Vec<serde_json::Value> {
    let q: serde_json::Value = serde_json::from_str(&api(s, ADDR, "", T, "queue")).unwrap();
    q["fleets"].as_array().unwrap().clone()
}

fn sheet(s: &Server, fleet: &str) -> serde_json::Value {
    let f = fleets(s).into_iter().find(|f| f["name"] == fleet).unwrap();
    f["config"].clone()
}

fn fixture(name: &str) -> drover::Ctx {
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
        let want =
            drover::config::COLORS[usize::from_str_radix(&drover::devices::sha256(f.as_bytes())[..2], 16).unwrap() % 5];
        assert_eq!(c.fleet[f].color(f), want, "{f}");
    }
    assert_eq!(load("color = \"p\"").unwrap().fleet["alpha"].color("alpha"), "p");
    let e = format!("{:#}", load("color = \"r\"").unwrap_err());
    assert!(
        e.contains("fleet.alpha.color: \"r\" must be one of [\"l\", \"p\", \"c\", \"g\", \"a\"]"),
        "{e}"
    );
    let d = tmpdir("fc-colour-page");
    let c = ctx(&d, &config("color = \"c\""));
    let r = Fake::default();
    let f = fleets(&Server::new(&c, &r, Some(ADDR.into())));
    let alpha = f.iter().find(|f| f["name"] == "alpha").unwrap();
    assert_eq!(alpha["k"], "c", "{alpha}");
}

#[test]
fn fleets_that_need_you_go_first_then_by_name_and_a_paused_one_is_empty() {
    let c = fixture("fc-order");
    let r = Fake::default();
    let f = fleets(&Server::new(&c, &r, Some(ADDR.into())));
    let names: Vec<&str> = f.iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["beta", "zeta", "alpha", "gamma"]);
    let cfg = Config::load(&c.config).unwrap();
    assert_eq!(
        (&f[0]["k"], &f[0]["paused"], &f[0]["need"]),
        (
            &json!(cfg.fleet["beta"].color("beta")),
            &json!("paused · no lane"),
            &json!(1)
        ),
        "{}",
        f[0]
    );
    assert_eq!(
        (&f[3]["paused"], &f[3]["rows"]),
        (&json!("paused · no lane"), &json!([]))
    );
    let rows = |i: usize| -> Vec<(u64, String)> {
        f[i]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r["issue"].as_u64().unwrap(), r["state"].as_str().unwrap().to_string()))
            .collect()
    };
    assert_eq!(f[1]["need"], 1);
    assert_eq!(
        rows(1),
        [(5, "needs-human".to_string()), (4, "queued".to_string())],
        "needs-you row first"
    );
    assert_eq!((&f[2]["need"], &f[2]["paused"]), (&json!(0), &json!(null)));
    assert_eq!(
        (&f[2]["drive"]["tone"], &f[2]["drive"]["line"], &f[2]["drive"]["meta"]),
        (
            &json!("a"),
            &json!("drive · no pass yet"),
            &json!(["stall check: #1 is due now", "1 lane"])
        )
    );
}

#[test]
fn the_config_sheet_shows_every_group_and_its_pickers_post_through_set() {
    let c = fixture("fc-config");
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let res = sheet(&s, "alpha");
    assert_eq!(res["path"], c.config.display().to_string());
    assert_eq!((&res["repo"], &res["line_cap"]), (&json!("acme/alpha"), &json!(null)));
    assert_eq!(res["models"], json!(["opus", "sonnet"]));
    let imp = &res["roles"][0];
    assert_eq!(
        (&imp["name"], &imp["harness"], &imp["model"], &imp["efforts"]),
        (
            &json!("impl"),
            &json!("claude"),
            &json!("opus"),
            &json!(["none", "low", "medium", "high", "xhigh"])
        )
    );
    let q: serde_json::Value = serde_json::from_str(&api(&s, ADDR, "", T, "queue")).unwrap();
    let mt = q["mtime"].as_str().unwrap();
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
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == "set")
        .collect();
    assert_eq!(sets.len(), 2, "harness and effort changed, model kept");
    assert_eq!(sets[0].val("key"), "harness");
    assert_eq!(sets[0].val("via"), "web");
    let d = tmpdir("fc-config-nomodels");
    let c = ctx(&d, &config("").replace("models = [\"opus\", \"sonnet\"]\n", ""));
    let r = Fake::default();
    let res = sheet(&Server::new(&c, &r, Some(ADDR.into())), "alpha");
    assert_eq!(res["models"], json!([]), "no models = [...] in fleet.toml: {res}");
}
