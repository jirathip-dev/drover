mod common;
use common::*;
use drover::serve::Server;

const ADDR: &str = "127.0.0.1:7777";

#[test]
fn the_host_api_carries_the_cards_gauges_and_this_phones_key() {
    let c = ctx(&tmpdir("host-live"), CONFIG);
    let sh = FakeShed::new();
    sh.attach(&c);
    let r = Fake::default().reply("df -k /", "F\n/dev/disk1 41943040 1 4194304\n");
    let w = sh.on(&r);
    let s = Server::new(&c, &w, Some(ADDR.into()));
    let h: serde_json::Value = serde_json::from_str(&api(&s, ADDR, "", 1_000_000, "host")).unwrap();
    let host = &h["hosts"][0];
    assert_eq!(
        (&host["main"], &host["tone"]),
        (&serde_json::json!(true), &serde_json::json!("r")),
        "{h}"
    );
    assert!(host["words"].as_str().unwrap().contains("disk under floor"), "{h}");
    let disk = host["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["k"] == "disk")
        .unwrap();
    assert_eq!(
        (&disk["t"], &disk["bar"]["pct"], &disk["b"], &disk["notes"]),
        (
            &serde_json::json!("r"),
            &serde_json::json!("250"),
            &serde_json::json!("free 4 GB"),
            &serde_json::json!(["of 40"])
        ),
        "{disk}"
    );
    assert_eq!(h["theme"]["name"], "farm", "{h}");
    assert!(
        h["live"] == true && h["key"].as_str().is_some_and(|k| !k.is_empty()),
        "{h}"
    );
    assert_eq!(h["devices"], serde_json::json!([]), "no paired device yet: {h}");
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
fn the_host_api_gives_each_provider_login_its_vendor_and_no_account() {
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
            "accounts": [],
            "setups": [{"name": "claude", "vendor": "anthropic"}, {"name": "codex", "vendor": "openai"}],
        }),
        "gh is no provider's login: {v}"
    );
}
