mod common;
use common::*;
use drover::serve::Server;
use std::path::PathBuf;

const ADDR: &str = "127.0.0.1:7777";

fn groups(h: &str) -> Vec<&str> {
    h.split("<span class=gn>")
        .skip(1)
        .map(|g| g.split('<').next().unwrap())
        .collect()
}

#[test]
fn an_export_ships_the_host_data_and_its_forms_are_inert() {
    let demo = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo"));
    let d = tmpdir("host-export");
    let c = drover::Ctx {
        config: demo.join("fleet.toml"),
        state: demo.join("events.jsonl"),
        worktrees: d.join("wt"),
        homes: [("hermes", d.join("hermes"))].into(),
        exe: "drover".into(),
        now: 1,
        home: d.join("home"),
        hints: vec![],
    };
    std::fs::create_dir_all(d.join("wt/oldproj")).unwrap();
    drover::serve::export(&c, &Fake::default(), &d.join("site"), true).unwrap();
    let page = std::fs::read_to_string(d.join("site/index.html")).unwrap();
    assert!(
        !page.contains("id=host") && !page.contains("class=hcd"),
        "the page has no Host HTML"
    );
    let h = host_html(&std::fs::read_to_string(d.join("site/api/v1/host")).unwrap());
    assert_eq!(groups(&h), ["HOSTS", "FLEET DISK", "SERVICES", "FLEETS"], "{h}");
    assert!(h.contains("data-k=load data-s=\"") && h.contains(" data-x=\""), "{h}");
    assert!(
        h.contains("<form inert><input type=hidden name=op value=orphans>") && !h.contains("method=post"),
        "{h}"
    );
    assert!(
        !h.contains("id=push") && !h.contains("sign in ▾"),
        "no per-device rows: {h}"
    );
}

#[test]
fn a_live_board_draws_cards_gauges_and_forms_that_post() {
    let c = ctx(&tmpdir("host-live"), CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
    let r = Fake::default().reply("df -k /", "F\n/dev/disk1 41943040 1 4194304\n");
    let w = sh.on(&r);
    let s = Server::new(&c, &w, Some(ADDR.into()));
    let h = host_tab(&s, ADDR, 1_000_000);
    assert_eq!(groups(&h)[..3], ["HOSTS", "FLEET DISK", "SERVICES"], "{h}");
    assert!(h.contains("<span class=\"tag m\">main</span>"), "{h}");
    let disk = &h[h.find("data-k=disk").unwrap()..];
    let disk = &disk[..disk.find("</details>").unwrap()];
    assert!(
        disk.contains(
            "<span class=\"bar r\">▕███████████<i>░</i>▏</span> <b class=r>▶ free 4 GB</b> <span class=mt>of 40</span> (250%)"
        ),
        "{disk}"
    );
    assert!(h.contains("disk under floor"), "{h}");
    assert!(h.contains("<p class=gr data-t=g><span class=k>theme</span><span class=w><b>farm</b></span></p>"));
    assert!(h.contains("<button id=push type=button hidden data-key=") && !h.contains("<form inert"));
    assert!(h.contains("no paired device yet"), "{h}");
}

#[test]
fn the_host_api_shows_each_root_with_its_state_and_free_space() {
    let c = ctx(&tmpdir("host-roots"), CONFIG);
    let roots = r#"[{"path":"/Volumes/NVMe/hs","state":"gone"},{"path":"~/w","state":"active","free_gb":41.6}]"#;
    let seen = format!(r#"{{"this host":{{"roots":{roots}}}}}"#);
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    std::fs::write(c.state.with_file_name("hosts.json"), seen).unwrap();
    let sh = FakeShed::new();
    sh.attach(&c);
    let r = Fake::default();
    let w = sh.on(&r);
    let s = Server::new(&c, &w, Some(ADDR.into()));
    let res =
        String::from_utf8(s.respond(&format!("GET /api/v1/host HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), 1_000_000)).unwrap();
    let v: serde_json::Value = serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    let rows: Vec<_> = (v["hosts"][0]["rows"].as_array().unwrap().iter())
        .filter(|c| c["k"] == "root")
        .map(|c| {
            (
                c["t"].as_str().unwrap(),
                c["text"].as_str().unwrap(),
                c["notes"].clone(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("m", "/Volumes/NVMe/hs", serde_json::json!(["gone"])),
            ("g", "~/w", serde_json::json!(["active", "42 GB free"])),
        ],
        "{v}"
    );
}

#[test]
fn the_host_api_gives_each_account_and_provider_login_its_vendor() {
    let c = ctx(
        &tmpdir("host-vendors"),
        &(CONFIG.to_string() + "[accounts]\nA = \"A\"\n"),
    );
    let sh = FakeShed::attached(&c);
    let r = Fake::default();
    let w = sh.on(&r);
    let s = Server::new(&c, &w, Some(ADDR.into()));
    let res =
        String::from_utf8(s.respond(&format!("GET /api/v1/host HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), 1_000_000)).unwrap();
    let v: serde_json::Value = serde_json::from_str(&res[res.find("\r\n\r\n").unwrap() + 4..]).unwrap();
    let setups = v["hosts"][0]["setups"].as_array().unwrap();
    assert!(
        setups.contains(&"claude".into()) && setups.contains(&"gh".into()),
        "{v}"
    );
    assert_eq!(
        v["vendors"],
        serde_json::json!({
            "accounts": [{"name": "A", "vendor": "anthropic"}],
            "setups": [{"name": "claude", "vendor": "anthropic"}, {"name": "codex", "vendor": "openai"}],
        }),
        "gh is no provider's login: {v}"
    );
}
