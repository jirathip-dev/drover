mod common;
use common::*;
use drover::events::read;
use drover::serve::Server;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

fn req(s: &Server, path: &str, body: &str, extra: &str) -> String {
    let r = format!(
        "POST {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&r, T)).into_owned()
}

fn mtime(s: &Server) -> String {
    let st: serde_json::Value = serde_json::from_str(&api(s, ADDR, "", T, "state")).unwrap();
    st["mtime"].as_str().unwrap().to_string()
}

fn setup(name: &str) -> drover::Ctx {
    let text = CONFIG.replace(
        "profile = \"fleet-rev\"\n",
        "profile = \"fleet-rev\"\nmodel = \"sonnet\"\n",
    );
    ctx(&tmpdir(name), &text)
}

fn acp_setup(name: &str, b: &FakeShed) -> drover::Ctx {
    let text = CONFIG
        .replace(
            "profile = \"fleet-rev\"\n",
            "profile = \"fleet-rev\"\nmodel = \"sonnet\"\n",
        )
        .replace("harness  =  \"claude\"    # toggled in tests", "harness = \"claude\"");
    let text = format!(
        "{text}\n[fleet.sendmeter.runtime]\nkind = \"shed-acp\"\nsocket = \"{}\"\n",
        b.sock.display()
    );
    let mut c = ctx(&tmpdir(name), &text);
    c.exe = env!("CARGO_BIN_EXE_drover").into();
    c
}

fn host() -> Fake {
    let agents =
        serde_json::json!([{"id": "claude-acp", "version": "0.101.0", "distribution": "npx", "cmd": ["echo"]}]);
    Fake::default()
        .reply("shed help run", "--meta <JSON>")
        .reply("shed agents --json", &agents.to_string())
        .reply("shed run", "")
        .reply("shed send", "")
        .reply("which claude", "/usr/local/bin/claude\n")
}

#[test]
fn a_fleet_without_a_orchestrator_role_attaches_its_chat_on_its_impl_role() {
    let b = FakeShed::new();
    let c = acp_setup("cdef-start", &b);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let profile = c.config.with_file_name("claude").join("lane-orchestrator");
    let lane = profile.join("settings.json");
    assert!(lane.exists());
    let (code, out) = run(&c, &Fake::default(), &["attach", "sendmeter", "c-1"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&format!("settings: {}", lane.display())), "{out}");
}

#[test]
fn a_orchestrator_choice_in_config_writes_its_table_after_a_backup() {
    let b = FakeShed::new();
    let c = acp_setup("cdef-set", &b);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let orig = std::fs::read_to_string(&c.config).unwrap();
    let f = host();
    let cli = b.cli(&f);
    let r = b.on(&cli);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let mt = mtime(&s);
    let tap = format!("op=cond&fleet=sendmeter&model=sonnet&mtime={mt}");
    let res = req(&s, "/", &tap, "");
    assert!(res.starts_with("HTTP/1.1 401"), "{res}");
    assert_eq!(std::fs::read_to_string(&c.config).unwrap(), orig);
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let res = req(&s, "/", &tap, &cookie);
    assert!(
        res.contains("Location: /?msg=sendmeter%20orchestrator%3A%20model%20sonnet"),
        "{res}"
    );
    let want = format!(
        "{orig}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"sonnet\"\nprofile = \"lane-orchestrator\"\n"
    );
    assert_eq!(
        std::fs::read_to_string(&c.config).unwrap(),
        want,
        "every other byte kept"
    );
    assert_eq!(
        std::fs::read_to_string(c.config.with_extension("toml.bak")).unwrap(),
        orig
    );
    let set = read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .rfind(|e| e.kind == "set")
        .unwrap();
    assert_eq!(
        (set.str("role"), set.str("key"), set.str("new")),
        (Some("orchestrator"), Some("model"), Some("sonnet"))
    );
}

#[test]
fn a_orchestrator_choice_of_a_model_its_harness_lacks_is_refused_and_leaves_fleet_toml_alone() {
    let c = setup("cdef-doctor");
    let orig = std::fs::read(&c.config).unwrap();
    let r = Fake::default();
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let mt = mtime(&s);
    let res = req(
        &s,
        "/",
        &format!("op=cond&fleet=sendmeter&model=haiku&mtime={mt}"),
        &cookie,
    );
    assert!(res.contains("REFUSED%20BAD%5FVALUE"), "{res}");
    assert_eq!(std::fs::read(&c.config).unwrap(), orig, "byte-identical");
    assert!(!c.config.with_extension("toml.bak").exists());
}
