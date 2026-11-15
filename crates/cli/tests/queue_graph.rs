mod common;
use common::*;
use drover::config::Config;
use drover::events::{append, Event};
use drover::serve::Server;
use serde_json::json;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

fn config(label: &str) -> String {
    format!(
        r#"[fleet.acme]
repo = "acme/app"
local = "/tmp/does-not-matter/app"
integration = "staging"
serial_paths = ["db/migrations/**", "db/seed.sql"]
{label}

[fleet.beta]
repo = "acme/beta"
local = "/tmp/does-not-matter/beta"
integration = "staging"
serial_paths = ["schema/**"]
"#
    )
}

fn load(label: &str) -> anyhow::Result<Config> {
    let d = tmpdir("qg-config");
    std::fs::write(d.join("fleet.toml"), config(label)).unwrap();
    Config::load(&d.join("fleet.toml"))
}

#[test]
fn serial_label_defaults_to_db_and_takes_an_override() {
    let c = load("").unwrap();
    assert_eq!(c.fleet["acme"].serial_label(), "db");
    let c = load("serial_label = \"schema\"").unwrap();
    assert_eq!(c.fleet["acme"].serial_label(), "schema");
    assert_eq!(c.fleet["beta"].serial_label(), "db");
    for bad in ["serial_label = \"\"", "serial_label = \"migrations\""] {
        let e = format!("{:#}", load(bad).unwrap_err());
        assert!(
            e.contains("fleet.acme.serial: a name (serial_label) must be 1 to 8 characters"),
            "{bad}: {e}"
        );
    }
}

fn fixture() -> (drover::Ctx, Fake) {
    let c = ctx(&tmpdir("queue-graph"), &config("serial_label = \"db\""));
    let ok = |args: &[&str]| assert_eq!(run(&c, &Fake::default(), args).0, 0, "{args:?}");
    ok(&["queue", "acme", "add", "1", "2", "3", "4", "5", "6", "7"]);
    ok(&["queue", "acme", "add", "--serial", "8"]);
    for (n, on) in [("2", "1"), ("3", "2"), ("4", "3"), ("5", "4"), ("6", "1"), ("7", "99")] {
        ok(&["queue", "acme", "block", "--on", on, n]);
    }
    ok(&["queue", "beta", "add", "--serial", "21", "22"]);
    ok(&["queue", "beta", "block", "--on", "21", "22"]);
    let cache = c.state.parent().unwrap().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    for (f, n) in (1..=8).map(|n| ("acme", n)).chain([("beta", 21), ("beta", 22)]) {
        let t = format!("Issue {n}: queue rows show their one-line title at every depth");
        std::fs::write(
            cache.join(format!("issue-{f}-{n}.json")),
            json!({ "title": t }).to_string(),
        )
        .unwrap();
    }
    let spawn = json!({"role": "impl", "agent": "acme-impl-1", "harness": "claude"});
    append(&c.state, &Event::new(900_000, "acme", Some(1), "spawn", spawn)).unwrap();
    (c, Fake::default())
}

#[test]
fn rows_carry_the_graph_data_and_the_fleet_its_serial_paths() {
    let (c, r) = fixture();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let q: serde_json::Value = serde_json::from_str(&api(&s, ADDR, "", T, "queue")).unwrap();
    let card = |f: &str| {
        q["fleets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == f)
            .unwrap_or_else(|| panic!("{q}"))
    };
    let (acme, beta) = (card("acme"), card("beta"));
    assert_eq!(
        (&acme["k"], &acme["serial"], &acme["paths"]),
        (&json!("l"), &json!(["db"]), &json!(["db/migrations/**, db/seed.sql"]))
    );
    assert_eq!((&beta["k"], &beta["paths"]), (&json!("a"), &json!(["schema/**"])));
    let row = |c: &serde_json::Value, n: u64| {
        c["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["issue"] == n)
            .unwrap_or_else(|| panic!("no #{n}: {c}"))
            .clone()
    };
    let r1 = row(acme, 1);
    assert_eq!(
        (&r1["act"], &r1["grow"], &r1["lane"]),
        (&json!("impl"), &json!("impl"), &json!("acme-impl-1"))
    );
    let r7 = row(acme, 7);
    assert_eq!(
        (&r7["blocked_by"], &r7["act"], &r7["grow"]),
        (&json!(99), &json!("idle"), &json!("queued"))
    );
    assert_eq!(row(acme, 8)["groups"], json!(["db"]));
    let r22 = row(beta, 22);
    assert_eq!((&r22["blocked_by"], &r22["groups"]), (&json!(21), &json!(["db"])));
}

const HOT: [&str; 5] = [
    "assets/serve.js",
    "crates/core/src/drive.rs",
    "crates/core/src/queue.rs",
    "crates/server/src/screens.rs",
    "crates/server/src/serve.rs",
];

fn hot_fixture() -> drover::Ctx {
    let c = ctx(&tmpdir("queue-hot"), &config("serial_label = \"db\""));
    let ok = |args: &[&str]| assert_eq!(run(&c, &Fake::default(), args).0, 0, "{args:?}");
    ok(&["queue", "acme", "add", "--serial", "9"]);
    ok(&["queue", "acme", "add", "10"]);
    let ev = |n, kind, d| append(&c.state, &Event::new(900_000, "acme", n, kind, d)).unwrap();
    ev(None, "hot-files", json!({ "files": HOT }));
    for p in HOT {
        ev(
            Some(9),
            "serial-joined",
            json!({"group": format!("hot {p}"), "by": "diff"}),
        );
    }
    c
}

#[test]
fn a_row_holding_five_hot_files_has_one_hot_chip_and_its_paths_folded() {
    let (c, r) = (hot_fixture(), Fake::default());
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let get = |p: &str| {
        let res =
            String::from_utf8_lossy(&s.respond(&format!("GET {p} HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), T)).into_owned();
        let (head, body) = res.split_once("\r\n\r\n").unwrap();
        assert!(head.starts_with("HTTP/1.1 200"), "{res}");
        serde_json::from_str::<serde_json::Value>(body).unwrap()
    };
    let q = get("/screens/queue");
    let text = q.to_string();
    assert_eq!(text.matches("🔒 hot ×5").count(), 1, "{text}");
    assert!(HOT.iter().all(|p| !text.contains(p)), "{text}");
    let list = q["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == "fleet-acme")
        .unwrap();
    let queued = list["blocks"][1]["blocks"][0]["rows"].as_array().unwrap();
    let ids: Vec<&str> = queued.iter().map(|r| r["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["q-acme-9", "hot-acme-9", "q-acme-10"]);
    assert_eq!(queued[1]["title"], "🔒 hot ×5");
    let sheet = get(queued[1]["tap"]["sheet"].as_str().unwrap());
    let paths: Vec<&str> = sheet["blocks"][0]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["title"].as_str().unwrap())
        .collect();
    assert_eq!(sheet["type"], "sheet");
    assert_eq!(paths, HOT);
}
