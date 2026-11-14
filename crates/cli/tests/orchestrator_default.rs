mod common;
use common::*;
use drover::events::{read, Event};
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

fn config_sheet(s: &Server) -> String {
    queue_js(s, ADDR, T, "sheet(d, 'sendmeter')")
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
        .replace(
            "harness  =  \"hermes\"    # toggled in tests",
            "harness = \"claude\"\naccount = \"C\"",
        );
    let text = format!(
        "{text}\n[fleet.sendmeter.runtime]\nkind = \"shed-acp\"\nsocket = \"{}\"\n\n[accounts]\nC = \"C\"\n",
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
        .reply("shed agents --json", &agents.to_string())
        .reply("shed run", "")
        .reply("shed send", "")
        .reply("which claude", "/usr/local/bin/claude\n")
}

fn starts(c: &drover::Ctx) -> Vec<Event> {
    let all = read(&c.state).unwrap().to_vec().into_iter();
    all.filter(|e| e.kind == "orchestrator-start").collect()
}

fn start_line(r: &Fake) -> String {
    let calls = r.calls.borrow();
    let start = calls.iter().find(|c| c.starts_with("shed run --acp claude-acp"));
    start.cloned().unwrap_or_default()
}

#[test]
fn a_fleet_without_a_orchestrator_role_starts_one_on_its_impl_role_from_the_board() {
    let b = FakeShed::new();
    let c = acp_setup("cdef-start", &b);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let profile = c.config.with_file_name("claude").join("lane-orchestrator");
    assert!(profile.join("settings.json").exists());
    let f = host();
    let cli = b.cli(&f);
    let r = b.on(&cli);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let res = req(&s, "/thread", "op=new&repo=sendmeter&text=Grill+%2312", &cookie);
    assert!(res.ends_with("\r\n\r\nt1 started\n"), "{res}");
    let st = &starts(&c)[0];
    assert_eq!((st.str("harness"), st.str("model")), (Some("claude"), Some("opus")));
    let lane = profile.join("settings.json");
    assert!(start_line(&f).contains(&*lane.to_string_lossy()), "{}", start_line(&f));
    let (code, out) = run(&c, &Fake::default(), &["thread", "reply", "t1", "Scope first."]);
    assert_eq!(code, 0, "{out}");
    let (_, show) = run(&c, &Fake::default(), &["thread", "show", "t1"]);
    assert_eq!(
        show,
        "t1 sendmeter: Grill #12\nowner: Grill #12\nOrchestrator: Scope first.\n"
    );
}

#[test]
fn a_orchestrator_choice_in_config_writes_its_table_after_a_backup_and_the_next_start_uses_it() {
    let b = FakeShed::new();
    let c = acp_setup("cdef-set", &b);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let orig = std::fs::read_to_string(&c.config).unwrap();
    let f = host();
    let cli = b.cli(&f);
    let r = b.on(&cli);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let sheet = config_sheet(&s);
    assert!(!sheet.contains("<h3>Conductor<span>"), "{sheet}");
    let i = sheet.find("name=mtime value=").unwrap() + 17;
    let mt: String = sheet[i..].chars().take_while(char::is_ascii_digit).collect();
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
        "{orig}\n[fleet.sendmeter.role.orchestrator]\naccount = \"C\"\nharness = \"claude\"\nmodel = \"sonnet\"\nprofile = \"lane-orchestrator\"\n"
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
    let res = req(&s, "/thread", "op=new&repo=sendmeter&text=Next", &cookie);
    assert!(res.ends_with("t1 started\n"), "{res}");
    assert_eq!(starts(&c)[0].str("model"), Some("sonnet"));
    let lane = c
        .config
        .with_file_name("claude")
        .join("lane-orchestrator/settings.json");
    let start = start_line(&f);
    assert!(start.contains(&*lane.to_string_lossy()), "{start}");
    assert!(start.contains("ANTHROPIC_MODEL='sonnet'"), "{start}");
}

#[test]
fn a_orchestrator_choice_that_fails_doctor_is_refused_and_leaves_fleet_toml_alone() {
    let c = setup("cdef-doctor");
    let orig = std::fs::read(&c.config).unwrap();
    let r = Fake::default().reply("which hermes", "/usr/local/bin/hermes\n");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let sheet = config_sheet(&s);
    let i = sheet.find("name=mtime value=").unwrap() + 17;
    let mt: String = sheet[i..].chars().take_while(char::is_ascii_digit).collect();
    let res = req(
        &s,
        "/",
        &format!("op=cond&fleet=sendmeter&harness=claude&mtime={mt}"),
        &cookie,
    );
    assert!(
        res.contains("REFUSED%20DOCTOR") && res.contains("claude%20on%20PATH"),
        "{res}"
    );
    assert_eq!(std::fs::read(&c.config).unwrap(), orig, "byte-identical");
    assert!(!c.config.with_extension("toml.bak").exists());
    let res = req(
        &s,
        "/",
        &format!("op=cond&fleet=sendmeter&model=haiku&mtime={mt}"),
        &cookie,
    );
    assert!(res.contains("REFUSED%20BAD%5FVALUE"), "{res}");
    assert_eq!(std::fs::read(&c.config).unwrap(), orig);
}
