//! #355: conductor chat: a thread's lifecycle (start, sleep, resume with history) and an ask round-trip.
mod common;
use common::*;
use drover::events::{read, Event};
use serde_json::json;

const NOW: u64 = 1_000_000;

fn config(harness: &str, dir: &std::path::Path) -> String {
    let (ins, mem) = (dir.join("orchestrator.md"), dir.join("memory"));
    std::fs::write(&ins, "Grill before filing.").unwrap();
    std::fs::create_dir_all(&mem).unwrap();
    std::fs::write(mem.join("owner.md"), "The owner reads on a phone.").unwrap();
    format!(
        "{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"{harness}\"\n\n[orchestrator]\ninstructions = \"{}\"\nmemory = \"{}\"\n",
        ins.display(),
        mem.display()
    )
}

fn setup(name: &str, harness: &str) -> drover::Ctx {
    let d = tmpdir(name);
    let c = ctx(&d, &config(harness, &d));
    let profile = c.config.with_file_name("claude").join("fleet-orchestrator");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Grill #12"]).1,
        "t1\n"
    );
    c
}

fn acp_setup(name: &str, b: &FakeShed) -> drover::Ctx {
    let d = tmpdir(name);
    let cfg = config("claude", &d).replace(
        "[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"",
        "[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\naccount = \"C\"",
    );
    let cfg = format!(
        "{cfg}\n[fleet.sendmeter.runtime]\nkind = \"shed-acp\"\nsocket = \"{}\"\n\n[accounts]\nC = \"C\"\n",
        b.sock.display()
    );
    let mut c = ctx(&d, &cfg);
    c.exe = env!("CARGO_BIN_EXE_drover").into();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Grill #12"]).1,
        "t1\n"
    );
    c
}

fn acp(adapter: &[&str]) -> Fake {
    let agents = json!([{"id": "claude-acp", "version": "0.101.0", "distribution": "npx", "cmd": adapter}]);
    Fake::default()
        .reply("shed agents --json", &agents.to_string())
        .reply("shed run", "")
        .reply("shed send", "")
}

fn shed_calls(f: &Fake) -> Vec<String> {
    let calls = f.calls.borrow();
    let ours = |c: &&String| c.starts_with("shed run") || c.starts_with("shed send");
    calls.iter().filter(ours).cloned().collect()
}

fn at(c: &drover::Ctx, now: u64) -> drover::Ctx {
    drover::Ctx { now, ..c.clone() }
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

/// herdr with `agents` live, ready to start a conductor in a new workspace or type into one.
fn herdr(agents: &str) -> Fake {
    Fake::default()
        .reply("herdr agent list", agents)
        .reply(
            "herdr workspace create",
            r#"{"result":{"root_pane":{"pane_id":"w9:p1"}}}"#,
        )
        .reply("herdr workspace close", "")
        .reply("herdr pane run", "")
        .reply("herdr pane send-text", "")
        .reply("herdr pane wait-output", "")
        .reply("herdr pane send-keys", "")
        .reply("herdr agent start", "")
        .reply("herdr agent wait", "")
        .reply("herdr agent prompt", "")
}

fn live(status: &str) -> String {
    agents(&[("orchestrator-t1", status, "/x", "w9:p1")])
}

fn start_line(r: &Fake) -> String {
    let calls = r.calls.borrow();
    calls
        .iter()
        .find(|c| c.starts_with("herdr agent start orchestrator-t1"))
        .cloned()
        .unwrap_or_default()
}

#[test]
fn a_thread_starts_sleeps_when_idle_and_restarts_with_its_history() {
    let b = FakeShed::new();
    let c = acp_setup("orchestrator-life", &b);
    let f = acp(&["env"]);
    let cli = b.cli(&f);
    let (code, out) = run(&c, &cli, &["thread", "say", "t1", "what should #12 cover?"]);
    assert_eq!(
        (code, out.as_str()),
        (0, "t1: -> orchestrator-t1\n"),
        "{:?}",
        f.calls.borrow()
    );
    let dir = c.state.with_file_name("orchestrator").join("t1");
    let calls = shed_calls(&f);
    let head = format!(
        "shed run --acp claude-acp --policy allow --name orchestrator-t1 --cwd {} ",
        dir.display()
    );
    assert!(calls[0].starts_with(&head), "{calls:?}");
    let st = &kinds(&c, "orchestrator-start")[0];
    assert_eq!(
        (st.str("runtime"), st.data["resumed"].clone()),
        (Some("shed-acp"), json!(false))
    );
    let thread = dir.join(".thread.md");
    let brief = std::fs::read_to_string(dir.join(".brief.md")).unwrap();
    let first = format!(
        "shed send orchestrator-t1 {}\n\nThe thread so far is in {}, the last message new.\n\n[t1] owner: what should #12 cover?",
        brief.trim_end(),
        thread.display()
    );
    assert_eq!(calls[1..], [first]);
    assert_eq!(
        std::fs::read_to_string(&thread).unwrap(),
        "owner: what should #12 cover?\n"
    );
    for want in [
        "# ROLE: ORCHESTRATOR",
        "Thread: t1, Grill #12",
        "Grill before filing.",
        "The owner reads on a phone.",
        "in a ``` fenced block, one block per command group",
    ] {
        assert!(brief.contains(want), "{want}: {brief}");
    }
    let env = cli.out.borrow().concat();
    assert!(
        env.lines().any(|l| l == "DROVER_LANE=sendmeter:-:orchestrator"),
        "{env}"
    );

    let (code, out) = run(
        &c,
        &Fake::default(),
        &["thread", "reply", "t1", "Scope and tests. Which first?"],
    );
    assert_eq!(code, 0, "{out}");

    let f = acp(&["echo"]);
    let c2 = at(&c, NOW + 60);
    assert_eq!(run(&c2, &b.cli(&f), &["thread", "say", "t1", "scope"]).0, 0);
    assert_eq!(shed_calls(&f), ["shed send orchestrator-t1 [t1] owner: scope"]);

    let (code, out) = run(&at(&c, NOW + 60 + 29 * 60), &b.cli(&f), &["check"]);
    assert_eq!(code, 0, "{out}");
    b.set("orchestrator-t1", |t| t["reported"] = json!({"state": "working"}));
    let (code, out) = run(&at(&c, NOW + 60 * 60), &b.cli(&f), &["check"]);
    assert_eq!(code, 0, "{out}");
    assert!(kinds(&c, "orchestrator-sleep").is_empty());
    assert!(!b.called("kill", "orchestrator-t1"), "{:?}", b.methods());
    b.set("orchestrator-t1", |t| t["reported"] = json!({"state": "idle"}));
    assert_eq!(run(&at(&c, NOW + 60 + 31 * 60), &b.cli(&f), &["check"]).0, 0);
    assert!(b.called("kill", "orchestrator-t1"), "{:?}", b.methods());
    assert_eq!(kinds(&c, "orchestrator-sleep")[0].str("why"), Some("idle"));

    let f = acp(&["echo"]);
    assert_eq!(
        run(&at(&c, NOW + 3600), &b.cli(&f), &["thread", "say", "t1", "tests first"]).0,
        0
    );
    let calls = shed_calls(&f);
    assert!(calls[0].starts_with(&head), "{calls:?}");
    assert!(calls[1].ends_with("[t1] owner: tests first"), "{calls:?}");
    assert_eq!(kinds(&c, "orchestrator-start")[1].data["resumed"], json!(false));
    assert_eq!(
        std::fs::read_to_string(&thread).unwrap(),
        "owner: what should #12 cover?\nOrchestrator: Scope and tests. Which first?\nowner: scope\nowner: tests first\n"
    );

    let (_, show) = run(&c, &Fake::default(), &["thread", "show", "t1"]);
    assert_eq!(
        show,
        "t1 sendmeter: Grill #12\nowner: what should #12 cover?\nOrchestrator: Scope and tests. Which first?\n\
         owner: scope\nowner: tests first\n"
    );
}

#[test]
fn a_herdr_orchestrator_resumes_its_session() {
    let c = setup("orchestrator-resume", "claude");
    let r = herdr(NO_AGENTS);
    assert_eq!(run(&c, &r, &["thread", "say", "t1", "what should #12 cover?"]).0, 0);
    let dir = c.state.with_file_name("orchestrator").join("t1");
    assert!(r.called(&format!(
        "herdr workspace create --cwd {} --label orchestrator-t1",
        dir.display()
    )));
    let st = &kinds(&c, "orchestrator-start")[0];
    let session = st.str("session").unwrap().to_string();
    assert_eq!((session.len(), st.data["resumed"].clone()), (36, json!(false)));
    let first = start_line(&r);
    assert!(first.contains(&format!("--session-id {session}")), "{first}");
    assert!(
        first.contains("--append-system-prompt-file") && !first.contains("--resume"),
        "{first}"
    );
    assert!(
        first.ends_with(&format!(
            "Read {} and act on it. The thread so far is in {}, the last message new.",
            dir.join(".brief.md").display(),
            dir.join(".thread.md").display()
        )),
        "{first}"
    );
    assert!(r.called("herdr pane run w9:p1 export DROVER_LANE='sendmeter:-:orchestrator'"));
    let r = herdr(&live("idle"));
    assert_eq!(run(&at(&c, NOW + 60), &r, &["thread", "say", "t1", "scope"]).0, 0);
    assert!(r.called("herdr agent prompt orchestrator-t1 [t1] owner: scope"));
    assert!(r.called("herdr pane send-keys w9:p1 Enter") && !r.called("herdr agent start"));
    let r = herdr(&live("idle"));
    assert_eq!(run(&at(&c, NOW + 60 + 31 * 60), &r, &["check"]).0, 0);
    assert!(r.called("herdr workspace close w9"), "{:?}", r.calls.borrow());
    let r = herdr(NO_AGENTS);
    assert_eq!(
        run(&at(&c, NOW + 3600), &r, &["thread", "say", "t1", "tests first"]).0,
        0
    );
    let resumed = start_line(&r);
    assert!(resumed.contains(&format!("--resume {session}")), "{resumed}");
    assert!(!resumed.contains("--session-id"), "{resumed}");
    assert!(resumed.ends_with(" [t1] owner: tests first"), "{resumed}");
    assert_eq!(kinds(&c, "orchestrator-start")[1].data["resumed"], json!(true));
}

#[test]
fn a_harness_without_resume_gets_the_history_in_its_kick() {
    let c = setup("orchestrator-generic", "hermes");
    assert_eq!(run(&c, &herdr(NO_AGENTS), &["thread", "say", "t1", "first"]).0, 0);
    run(&c, &Fake::default(), &["thread", "reply", "t1", "noted"]);
    run(&at(&c, NOW + 31 * 60), &herdr(&live("idle")), &["check"]);
    let r = herdr(NO_AGENTS);
    assert_eq!(run(&at(&c, NOW + 3600), &r, &["thread", "say", "t1", "second"]).0, 0);
    assert!(start_line(&r).contains("--kind hermes"));
    let calls = r.calls.borrow();
    let kick = calls
        .iter()
        .find(|c| c.starts_with("herdr agent prompt orchestrator-t1 Read"))
        .unwrap();
    let thread = c.state.with_file_name("orchestrator").join("t1/.thread.md");
    assert!(
        kick.contains(&format!("The thread so far is in {}", thread.display())),
        "{kick}"
    );
    assert_eq!(
        std::fs::read_to_string(thread).unwrap(),
        "owner: first\nOrchestrator: noted\nowner: second\n"
    );
}

/// start or resume, nor any lane spawn, sends one, however many lines the thread has. (dsh's launch carries no kick.)
#[test]
fn no_harness_starts_an_agent_with_a_control_character_in_its_argv() {
    let bad = |r: &Fake| {
        let calls = r.calls.borrow();
        let start = calls.iter().find(|c| c.starts_with("herdr agent start"));
        assert!(start.is_some(), "{calls:?}");
        start.filter(|c| c.contains(char::is_control)).cloned()
    };
    for h in ["claude", "hermes"] {
        let c = setup(&format!("orchestrator-argv-{h}"), h);
        let r = herdr(NO_AGENTS);
        let (code, out) = run(&c, &r, &["thread", "say", "t1", "line one\nline\ttwo"]);
        assert_eq!(code, 0, "{h}: {out}");
        assert_eq!(bad(&r), None, "{h}: first start");
        run(&at(&c, NOW + 31 * 60), &herdr(&live("idle")), &["check"]);
        let r = herdr(NO_AGENTS);
        let (code, out) = run(&at(&c, NOW + 3600), &r, &["thread", "say", "t1", "and\nagain"]);
        assert_eq!(code, 0, "{h}: {out}");
        assert_eq!(bad(&r), None, "{h}: resume");
    }
    let c = setup("orchestrator-argv-lanes", "claude");
    for h in drover::providers::lane_names() {
        for role in ["impl", "rev"] {
            let set = ["set", "sendmeter", role, "--harness", h];
            assert_eq!(run(&c, &Fake::default(), &set).0, 0);
            let dry = ["spawn", "sendmeter", "12", role, "--dry-run", "--json"];
            let (code, out) = run(&c, &Fake::default(), &dry);
            assert_eq!(code, 0, "{h} {role}: {out}");
            let v: serde_json::Value = serde_json::from_str(&out).unwrap();
            let args = v["commands"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|c| c.as_array().unwrap());
            assert!(
                args.clone().all(|a| !a.as_str().unwrap().contains(char::is_control)),
                "{h} {role}: {out}"
            );
        }
    }
}

#[test]
fn a_thread_ask_shows_its_options_recommended_first_and_its_answer_goes_back_into_the_thread() {
    let b = FakeShed::new();
    let c = acp_setup("orchestrator-ask", &b);
    assert_eq!(
        run(&c, &b.cli(&acp(&["echo"])), &["thread", "say", "t1", "grill me"]).0,
        0
    );
    let ask = [
        "ask",
        "sendmeter",
        "-",
        "Which scope?",
        "--option",
        "small",
        "--option",
        "full",
        "--recommend",
        "2",
        "--thread",
        "t1",
    ];
    let (code, out) = run(&c, &Fake::default(), &ask);
    assert_eq!((code, out.as_str()), (0, "q1\n"));
    let (_, ls) = run(&c, &Fake::default(), &["thread", "ls", "--json"]);
    let ls: serde_json::Value = serde_json::from_str(&ls).unwrap();
    assert_eq!(
        (ls[0]["unread"].clone(), ls[0]["last"].clone()),
        (json!(true), json!("Orchestrator asks q1: Which scope?"))
    );
    let (_, show) = run(&c, &Fake::default(), &["thread", "show", "t1", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&show).unwrap();
    let item = &v["items"][1];
    assert_eq!(
        item["options"],
        json!([{"n": 2, "text": "full"}, {"n": 1, "text": "small"}])
    );
    assert_eq!((item["ask"].clone(), item["open"].clone()), (json!("q1"), json!(true)));
    let (_, ls) = run(&c, &Fake::default(), &["thread", "ls"]);
    assert!(
        ls.starts_with("sendmeter\n  t1   Grill #12: Orchestrator asks q1"),
        "read now: {ls}"
    );

    // The tap (`answer`) types the DECISION into the thread's conductor and logs it in the thread.
    let f = acp(&["echo"]);
    let (code, out) = run(&c, &b.cli(&f), &["answer", "sendmeter", "q1", "1"]);
    assert_eq!((code, out.as_str()), (0, "sendmeter q1: small -> orchestrator-t1\n"));
    assert_eq!(shed_calls(&f), ["shed send orchestrator-t1 DECISION q1: small"]);
    let (_, show) = run(&c, &Fake::default(), &["thread", "show", "t1"]);
    assert!(
        show.ends_with(
            "Orchestrator asks q1: Which scope?\n  [2] full (recommended)\n  [1] small\nowner: DECISION q1: small\n"
        ),
        "{show}"
    );
    let (_, show) = run(&c, &Fake::default(), &["thread", "show", "t1", "--json"]);
    assert!(show.contains("\"open\": false"), "{show}");

    let bad = ["ask", "sendmeter", "-", "Q?", "--option", "a", "--thread", "t9"];
    let (code, out) = run(&c, &Fake::default(), &bad);
    assert!(code == 3 && out.contains("REFUSED NO_SUCH_THREAD"), "{out}");
}

#[test]
fn a_thread_needs_a_orchestrator_or_impl_role_and_spawn_never_starts_one() {
    let no_impl = CONFIG.replace("[fleet.sendmeter.role.impl]", "[fleet.sendmeter.role.build]");
    let c = ctx(&tmpdir("orchestrator-none"), &no_impl);
    let b = FakeShed::new();
    b.attach(&c);
    assert_eq!(run(&c, &Fake::default(), &["thread", "new", "all", "Cross-repo"]).0, 0);
    let (code, out) = run(&c, &Fake::default(), &["thread", "say", "t1", "hi"]);
    assert!(code == 3 && out.contains("REFUSED NO_ORCHESTRATOR"), "{out}");
    let (code, out) = run(&c, &Fake::default(), &["spawn", "sendmeter", "12", "orchestrator"]);
    assert!(code == 3 && out.contains("REFUSED BAD_VALUE"), "{out}");
    let (code, out) = run(&c, &Fake::default(), &["thread", "new", "nope", "x"]);
    assert_eq!(code, 1, "{out}");
}
