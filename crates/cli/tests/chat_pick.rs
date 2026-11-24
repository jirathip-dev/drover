mod common;
use common::*;
use drover::config::Config;
use drover::events::{append, read, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

fn config(extra: &str) -> String {
    let c = CONFIG.replace(
        "[fleet.sendmeter.role.rev]\nharness = \"claude\"",
        "[fleet.sendmeter.role.rev]\nharness = \"claude\"\nmodel = \"sonnet\"",
    );
    format!(
        "{c}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\neffort = \"high\"\n{extra}"
    )
}

fn acp_config(extra: &str, b: &FakeShed) -> String {
    let c = config(extra).replace("effort = \"high\"\n", "effort = \"high\"\naccount = \"C\"\n");
    format!(
        "{c}\n[fleet.sendmeter.runtime]\nkind = \"shed-acp\"\nsocket = \"{}\"\n\n[accounts]\nC = \"C\"\n",
        b.sock.display()
    )
}

fn acp_ctx(name: &str, extra: &str, b: &FakeShed) -> drover::Ctx {
    let mut c = ctx(&tmpdir(name), &acp_config(extra, b));
    c.exe = env!("CARGO_BIN_EXE_drover").into();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    c
}

fn acp() -> Fake {
    let agents = json!([{"id": "claude-acp", "version": "0.101.0", "distribution": "npx", "cmd": ["echo"]}]);
    Fake::default()
        .reply("shed help run", "--meta <JSON>")
        .reply("shed agents --json", &agents.to_string())
        .reply("shed run", "")
        .reply("shed send", "")
}

fn req(s: &Server, path: &str, body: &str, extra: &str) -> String {
    let r = format!(
        "POST {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&r, T)).into_owned()
}

fn chat(s: &Server, cookie: &str) -> Value {
    serde_json::from_str(&api(s, ADDR, cookie, T, "chat")).unwrap()
}

fn thread<'a>(chat: &'a Value, id: &str) -> &'a Value {
    let all = chat["threads"].as_array().unwrap();
    all.iter()
        .find(|t| t["id"] == id)
        .unwrap_or_else(|| panic!("no {id}: {chat}"))
}

fn kind(c: &drover::Ctx, k: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == k)
        .collect()
}

#[test]
fn chat_defaults_come_from_the_chat_table_else_the_conductor_role() {
    let plain = Config::parse(&config("")).unwrap();
    let (_, r) = drover::orchestrator::defaults(&plain, "sendmeter").unwrap();
    assert_eq!(
        (r.harness.as_str(), r.model.as_deref(), r.effort.as_deref()),
        ("claude", Some("opus"), Some("high"))
    );
    let on = Config::parse(&config("[chat]\nmodel = \"sonnet\"\neffort = \"low\"\n")).unwrap();
    let (_, r) = drover::orchestrator::defaults(&on, "sendmeter").unwrap();
    assert_eq!(
        (r.harness.as_str(), r.model.as_deref(), r.effort.as_deref()),
        ("claude", Some("sonnet"), Some("low"))
    );
    let m = Config::parse(&config("[chat]\nmodel = \"haiku\"\n")).unwrap();
    assert_eq!(
        drover::orchestrator::harness_models(&m, "claude"),
        ["haiku", "opus", "sonnet"]
    );
    let bad = Config::parse(&config("[chat]\nharness = \"codex\"\n")).unwrap_err();
    assert!(format!("{bad:#}").contains("chat.harness"), "{bad:#}");
    let old = Config::parse(&config("[chat]\naccount = \"nobody\"\n")).unwrap();
    assert!(
        old.warnings().iter().any(|w| w.1.contains("chat.account")),
        "#923: a warning, never an error"
    );
}

#[test]
fn thread_new_harness_is_recorded_and_its_conductor_starts_on_it() {
    let b = FakeShed::new();
    let c = acp_ctx("pick-new", "[chat]\nharness = \"claude\"\n", &b);
    let (code, out) = run(
        &c,
        &Fake::default(),
        &["thread", "new", "sendmeter", "Grill", "--harness", "claude"],
    );
    assert_eq!((code, out.as_str()), (0, "t1\n"));
    let (code, out) = run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Plain"]);
    assert_eq!((code, out.as_str()), (0, "t2\n"));
    let news = kind(&c, "thread-new");
    assert_eq!(
        (news[0].str("harness"), news[1].str("harness")),
        (Some("claude"), Some("claude"))
    );
    let (code, out) = run(
        &c,
        &Fake::default(),
        &["thread", "new", "sendmeter", "X", "--harness", "codex"],
    );
    assert!(code == 3 && out.contains("REFUSED BAD_VALUE"), "{out}");
    std::fs::write(&c.config, acp_config("[chat]\nharness = \"claude\"\n", &b)).unwrap();
    let f = acp();
    let cli = b.cli(&f);
    assert_eq!(run(&c, &cli, &["thread", "say", "t1", "hi"]).0, 0);
    let st = &kind(&c, "orchestrator-start")[0];
    assert_eq!(
        (st.str("harness"), st.str("model"), st.str("runtime")),
        (Some("claude"), Some("opus"), Some("shed-acp"))
    );
    assert_eq!(f.count("shed run --acp claude-acp"), 1, "{:?}", f.calls.borrow());
    let r = b.on(&cli);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let page = chat(&s, &cookie);
    let chip = &page["repos"][0]["harness"];
    assert_eq!(
        (&chip["name"], &chip["all"]),
        (&json!("harness"), &json!(["claude"])),
        "the empty thread's chip: {page}"
    );
    assert!(
        chip["cur"].is_null() || chip["cur"] == "claude",
        "preset to the default: {page}"
    );
    let res = req(
        &s,
        "/thread",
        "op=new&repo=sendmeter&harness=claude&text=Board",
        &cookie,
    );
    assert!(res.ends_with("t3 started\n"), "{res}");
    assert_eq!(kind(&c, "thread-new")[2].str("harness"), Some("claude"));
    assert_eq!(
        f.count("shed run --acp claude-acp --policy allow --name orchestrator-t3"),
        1
    );
    let page = chat(&s, &cookie);
    let t3 = thread(&page, "t3");
    assert_eq!(t3["title"], "Board");
    assert!(
        t3["ds"].as_str().unwrap().starts_with("sendmeter · claude · opus"),
        "{t3}"
    );
}

#[test]
fn the_conductors_instructions_reach_the_thread_brief() {
    let notes = tmpdir("pick-notes").join("AGENTS.md");
    std::fs::write(&notes, "Always grill scope first.\n").unwrap();
    let extra = format!("[conductor]\ninstructions = \"{}\"\n", notes.display());
    let b = FakeShed::new();
    let c = acp_ctx("pick-reach", &extra, &b);
    assert_eq!(run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Grill"]).0, 0);
    let f = acp();
    let (code, out) = run(&c, &b.cli(&f), &["thread", "say", "t1", "hi"]);
    assert_eq!(code, 0, "{out}");
    let dir = drover::orchestrator::dir(&c, &Config::load(&c.config).unwrap(), "t1");
    let brief = std::fs::read_to_string(dir.join(".brief.md")).unwrap();
    assert!(
        brief.contains("# Orchestrator brief: thread") && brief.contains("Always grill scope first."),
        "{brief}"
    );
    assert!(f.called("shed send orchestrator-t1 # Orchestrator brief: thread"), "{:?}", f.calls.borrow());
}

#[test]
fn a_model_and_effort_pick_is_logged_shown_and_the_next_turn_restarts_on_it() {
    let b = FakeShed::new();
    let c = acp_ctx("pick-model-acp", "", &b);
    let f = acp();
    let cli = b.cli(&f);
    let r = b.on(&cli);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let res = req(&s, "/thread", "op=new&repo=sendmeter&text=Grill", &cookie);
    assert!(res.ends_with("t1 started\n"), "{res}");
    let page = chat(&s, &cookie);
    let chips = &thread(&page, "t1")["chips"];
    assert_eq!(
        (&chips[0]["name"], &chips[0]["all"], &chips[1]["name"]),
        (&json!("model"), &json!(["opus", "sonnet"]), &json!("effort")),
        "{page}"
    );
    assert!(chips[0]["cur"].is_null() || chips[0]["cur"] == "opus", "{page}");
    let res = req(&s, "/thread", "op=model&t=t1&model=gpt-9", &cookie);
    assert!(res.contains("REFUSED BAD_VALUE"), "{res}");
    let res = req(&s, "/thread", "op=model&t=t1&model=sonnet&effort=low", &cookie);
    assert!(res.ends_with("picked\n"), "{res}");
    let pick = &kind(&c, "thread-model")[0];
    assert_eq!((pick.str("model"), pick.str("effort")), (Some("sonnet"), Some("low")));
    let page = chat(&s, &cookie);
    let t1 = thread(&page, "t1");
    assert!(
        t1["page"]["msgs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["text"] == "model sonnet · effort low from the next turn"),
        "{t1}"
    );
    assert_eq!(t1["chips"][0]["cur"], "sonnet", "{t1}");
    let (_, show) = run(&c, &Fake::default(), &["thread", "show", "t1"]);
    assert!(
        show.ends_with("system: model sonnet · effort low from the next turn\n"),
        "{show}"
    );
    let first = f.count("shed run --acp");
    let f2 = acp();
    assert_eq!(run(&c, &b.cli(&f2), &["thread", "say", "t1", "next"]).0, 0);
    assert!(b.called("kill", "orchestrator-t1"), "{:?}", b.methods());
    let sleep = kind(&c, "orchestrator-sleep");
    assert_eq!(sleep[0].str("why"), Some("model"));
    let next = &kind(&c, "orchestrator-start")[1];
    assert_eq!((next.str("model"), next.str("effort")), (Some("sonnet"), Some("low")));
    assert_eq!(next.val("resumed").as_bool(), Some(false));
    let calls = f2.calls.borrow().join("\n");
    let start = calls.lines().find(|l| l.starts_with("shed run --acp")).unwrap();
    assert!(start.contains("ANTHROPIC_MODEL='sonnet'"), "{start}");
    assert_eq!(first, 1);
    let f3 = acp();
    assert_eq!(run(&c, &b.cli(&f3), &["thread", "say", "t1", "again"]).0, 0);
    assert_eq!(kind(&c, "orchestrator-start").len(), 2);
    assert!(f3.called("shed send orchestrator-t1 [t1] owner: again") && !f3.called("shed run"));
}

#[test]
fn an_old_thread_without_a_harness_renders_and_starts_on_the_default() {
    let b = FakeShed::new();
    let c = acp_ctx("pick-old", "", &b);
    let d = json!({"thread": "t1", "title": "Before #404"});
    append(&c.state, &Event::new(T - 60, "sendmeter", None, "thread-new", d)).unwrap();
    let f = acp();
    let cli = b.cli(&f);
    let r = b.on(&cli);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let page = chat(&s, &cookie);
    let t1 = thread(&page, "t1");
    assert_eq!(
        (&t1["title"], &t1["ds"]),
        (&json!("Before #404"), &json!("sendmeter · claude · opus")),
        "{t1}"
    );
    assert_eq!(run(&c, &r, &["thread", "say", "t1", "hi"]).0, 0);
    assert_eq!(kind(&c, "orchestrator-start")[0].str("harness"), Some("claude"));
}

fn listed(models: &str) -> String {
    with_models(&config(""), models)
}

fn with_models(c: &str, models: &str) -> String {
    let c = c.replace("model = \"opus\"", "model = \"claude-opus-5-5\"");
    format!(
        "models = [{models}]\n{}",
        c.replace("model = \"sonnet\"", "model = \"claude-opus-5-5\"")
    )
}

fn system(c: &drover::Ctx) -> Vec<String> {
    let sys = kind(c, "thread-msg")
        .into_iter()
        .filter(|e| e.str("from") == Some("system"));
    sys.map(|e| e.str("text").unwrap_or_default().to_string()).collect()
}

fn pick_fable(c: &drover::Ctx) {
    let r = Fake::default();
    let s = Server::new(c, &r, Some(ADDR.into()));
    let cookie = pair(&s, c, "Mozilla/5.0 (iPhone)", T);
    let res = req(&s, "/thread", "op=model&t=t1&model=fable", &cookie);
    assert!(res.ends_with("picked\n"), "{res}");
}

#[test]
fn the_model_chip_offers_the_models_list_and_a_pick_from_it_is_taken() {
    let m = Config::parse(&listed("\"opus\", \"fable\"")).unwrap();
    assert_eq!(
        drover::orchestrator::harness_models(&m, "claude"),
        ["claude-opus-5-5", "fable", "opus"]
    );
    assert_eq!(drover::orchestrator::harness_models(&m, "codex"), [] as [&str; 0]);
    let c = ctx(&tmpdir("pick-listed"), &listed("\"opus\", \"fable\""));
    assert_eq!(run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Grill"]).0, 0);
    pick_fable(&c);
    assert_eq!(kind(&c, "thread-model")[0].str("model"), Some("fable"));
}

#[test]
fn a_pick_dropped_from_the_models_list_is_refused_on_send_and_the_message_stays() {
    let b = FakeShed::new();
    let listed = |models: &str| with_models(&acp_config("", &b), models);
    let mut c = ctx(&tmpdir("pick-dropped"), &listed("\"opus\", \"fable\""));
    c.exe = env!("CARGO_BIN_EXE_drover").into();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    assert_eq!(run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Grill"]).0, 0);
    pick_fable(&c);
    std::fs::write(&c.config, listed("\"opus\"")).unwrap();
    let f = acp();
    let (code, out) = run(&c, &b.cli(&f), &["thread", "say", "t1", "keep me"]);
    assert!(code == 3 && out.contains("REFUSED BAD_VALUE"), "{out}");
    assert!(!f.called("shed run"), "{:?}", f.calls.borrow());
    let sys = system(&c);
    assert_eq!(sys.len(), 1, "{sys:?}");
    assert!(
        sys[0].contains("model fable") && sys[0].contains("[\"claude-opus-5-5\", \"opus\"]"),
        "{sys:?}"
    );
    let (_, show) = run(&c, &Fake::default(), &["thread", "show", "t1"]);
    assert!(show.contains("keep me"), "{show}");
}
