mod common;
use common::*;
use drover::events::{read, Event};
use serde_json::json;

const NOW: u64 = 1_000_000;

fn config(dir: &std::path::Path) -> String {
    let (ins, mem) = (dir.join("orchestrator.md"), dir.join("memory"));
    std::fs::write(&ins, "Grill before filing.").unwrap();
    std::fs::create_dir_all(&mem).unwrap();
    std::fs::write(mem.join("owner.md"), "The owner reads on a phone.").unwrap();
    format!(
        "{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\n\n[orchestrator]\ninstructions = \"{}\"\nmemory = \"{}\"\n",
        ins.display(),
        mem.display()
    )
}

fn setup(name: &str) -> drover::Ctx {
    let d = tmpdir(name);
    let c = ctx(&d, &config(&d));
    assert_eq!(old_thread(&c, "sendmeter", "Grill #12"), "t1");
    c
}

fn acp_setup(name: &str, b: &FakeShed) -> drover::Ctx {
    let d = tmpdir(name);
    let cfg = format!(
        "{}\n[fleet.sendmeter.runtime]\nkind = \"shed-acp\"\nsocket = \"{}\"\n",
        config(&d),
        b.sock.display()
    );
    let mut c = ctx(&d, &cfg);
    c.exe = env!("CARGO_BIN_EXE_drover").into();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    assert_eq!(old_thread(&c, "sendmeter", "Grill #12"), "t1");
    c
}

fn acp(adapter: &[&str]) -> Fake {
    let agents = json!([{"id": "claude-acp", "version": "0.101.0", "distribution": "npx", "cmd": adapter}]);
    Fake::default()
        .reply("shed help run", "--meta <JSON>")
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
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
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
    assert!(
        !calls[0].contains("taskpolicy") && !calls[0].contains("renice"),
        "{calls:?}"
    );
    let st = &kinds(&c, "orchestrator-start")[0];
    assert_eq!(
        (st.str("runtime"), st.val("resumed").clone()),
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
    look(&at(&c, NOW + 60 + 31 * 60), &b.on(&b.cli(&f)));
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
    let history = format!("The thread so far is in {}, the last message new.", thread.display());
    assert!(calls[1].contains(&history), "{calls:?}");
    assert_eq!(*kinds(&c, "orchestrator-start")[1].val("resumed"), json!(false));
    assert_eq!(
        std::fs::read_to_string(&thread).unwrap(),
        "owner: what should #12 cover?\nOrchestrator: Scope and tests. Which first?\nowner: scope\n\
         system: no reply yet: the orchestrator is unknown\nowner: tests first\n"
    );

    let (_, show) = run(&c, &Fake::default(), &["thread", "show", "t1"]);
    assert_eq!(
        show,
        "t1 sendmeter: Grill #12\nowner: what should #12 cover?\nOrchestrator: Scope and tests. Which first?\n\
         owner: scope\nsystem: no reply yet: the orchestrator is unknown\nowner: tests first\n"
    );
}

#[test]
fn no_harness_starts_an_agent_with_a_control_character_in_its_argv() {
    let c = setup("orchestrator-argv-lanes");
    let _b = FakeShed::attached(&c);
    for h in drover::providers::lane_names() {
        for role in ["impl", "rev"] {
            let set = ["set", "sendmeter", role, "--harness", h];
            assert_eq!(run(&c, &Fake::default(), &set).0, 0);
            let dry = ["spawn", "sendmeter", "12", role, "--dry-run", "--json"];
            let (code, out) = run(&c, &Fake::default().reply("which shed", "/x/shed\n"), &dry);
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
    old_thread(&c, "all", "Cross-repo");
    let (code, out) = run(&c, &Fake::default(), &["thread", "say", "t1", "hi"]);
    assert!(code == 3 && out.contains("REFUSED NO_ORCHESTRATOR"), "{out}");
    let (code, out) = run(&c, &Fake::default(), &["spawn", "sendmeter", "12", "orchestrator"]);
    assert!(code == 3 && out.contains("REFUSED BAD_VALUE"), "{out}");
}

fn system_rows(c: &drover::Ctx) -> Vec<String> {
    let rows = kinds(c, "thread-msg")
        .into_iter()
        .filter(|e| e.str("from") == Some("system"));
    rows.map(|e| e.str("text").unwrap_or_default().to_string()).collect()
}

#[test]
fn a_rate_limited_orchestrator_tells_the_owner_and_gets_one_continue_at_its_reset() {
    let b = FakeShed::new();
    let c = acp_setup("orchestrator-limited", &b);
    assert_eq!(run(&c, &b.cli(&acp(&["env"])), &["thread", "say", "t1", "hello"]).0, 0);
    b.set("orchestrator-t1", |t| {
        t["reported"] = json!({"state": "rate-limited", "note": "You've hit your limit · resets 3pm (UTC)"});
    });
    let reset = 11 * 86400 + 15 * 3600;
    let f = acp(&["echo"]).reply(&format!("date -r {reset} +%a %H:%M"), "Mon 15:00");
    let (code, out) = run(&at(&c, NOW + 60), &b.cli(&f), &["thread", "say", "t1", "scope?"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(shed_calls(&f), ["shed send orchestrator-t1 [t1] owner: scope?"]);
    assert_eq!(
        system_rows(&c),
        ["orchestrator out of usage until Mon 15:00; your message is kept"]
    );

    let f = acp(&["echo"]);
    assert_eq!(run(&at(&c, NOW + 60 + 11 * 60), &b.cli(&f), &["check"]).0, 0);
    assert!(shed_calls(&f).is_empty(), "{:?}", shed_calls(&f));
    assert_eq!(
        system_rows(&c)[1..],
        ["no reply yet: the orchestrator is rate-limited: You've hit your limit · resets 3pm (UTC)"]
    );

    for now in [reset, reset + 60] {
        let f = acp(&["echo"]);
        assert_eq!(run(&at(&c, now), &b.cli(&f), &["check"]).0, 0);
        let want: &[&str] = if now == reset {
            &["shed send orchestrator-t1 continue"]
        } else {
            &[]
        };
        assert_eq!(shed_calls(&f), want);
    }
    assert!(kinds(&c, "orchestrator-sleep").is_empty());
    assert_eq!(system_rows(&c).len(), 2, "{:?}", system_rows(&c));
    let owner = kinds(&c, "thread-msg")
        .into_iter()
        .filter(|e| e.str("text") == Some("scope?"));
    assert_eq!(owner.count(), 1);
}

fn asleep(c: &drover::Ctx) {
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let d = json!({"thread": "t1", "agent": "orchestrator-t1", "why": "gone"});
    let e = Event::new(NOW + 60, "sendmeter", None, "orchestrator-sleep", d);
    drover::events::emit(&c.state, &cfg, e).unwrap();
}

#[test]
fn a_conductor_missing_from_the_list_is_gone_only_once_shed_has_no_session() {
    let b = FakeShed::new();
    let c = acp_setup("orchestrator-unlisted", &b);
    assert_eq!(run(&c, &b.cli(&acp(&["env"])), &["thread", "say", "t1", "hello"]).0, 0);
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let f = acp(&["echo"]);
    let cli = b.cli(&f);
    let (r, later) = (b.on(&cli), at(&c, NOW + 60));
    for state in ["running", "hibernated"] {
        b.set("orchestrator-t1", |t| t["reported"] = json!({"state": state}));
        let events = read(&c.state).unwrap().to_vec();
        drover::orchestrator::sleep_idle(&later, &cfg, &r, &events, &[]).unwrap();
        assert!(kinds(&c, "orchestrator-sleep").is_empty(), "{state}");
    }
    b.remove("orchestrator-t1");
    let events = read(&c.state).unwrap().to_vec();
    drover::orchestrator::sleep_idle(&later, &cfg, &r, &events, &[]).unwrap();
    assert_eq!(kinds(&c, "orchestrator-sleep")[0].str("why"), Some("gone"));
}

#[test]
fn a_message_to_an_asleep_thread_whose_session_is_hibernated_resumes_it_with_no_new_run() {
    let b = FakeShed::new();
    let c = acp_setup("orchestrator-hibernated", &b);
    assert_eq!(run(&c, &b.cli(&acp(&["env"])), &["thread", "say", "t1", "hello"]).0, 0);
    b.set("orchestrator-t1", |t| t["reported"] = json!({"state": "hibernated"}));
    asleep(&c);
    let f = acp(&["echo"]);
    let (code, out) = run(&at(&c, NOW + 120), &b.cli(&f), &["thread", "say", "t1", "still there?"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(shed_calls(&f), ["shed send orchestrator-t1 [t1] owner: still there?"]);
    let starts = kinds(&c, "orchestrator-start");
    assert_eq!(starts.len(), 2);
    assert_eq!(*starts[1].val("resumed"), json!(true));
}

#[test]
fn an_exited_same_name_session_is_replaced_and_the_replacement_is_logged() {
    let b = FakeShed::new();
    let c = acp_setup("orchestrator-exited", &b);
    assert_eq!(run(&c, &b.cli(&acp(&["env"])), &["thread", "say", "t1", "hello"]).0, 0);
    b.exit("orchestrator-t1");
    let f = acp(&["echo"]);
    let (code, out) = run(&at(&c, NOW + 120), &b.cli(&f), &["thread", "say", "t1", "again"]);
    assert_eq!(code, 0, "{out}");
    assert!(b.called("rm", "orchestrator-t1"), "{:?}", b.methods());
    let calls = shed_calls(&f);
    assert!(calls[0].starts_with("shed run --acp claude-acp"), "{calls:?}");
    let starts = kinds(&c, "orchestrator-start");
    assert_eq!(starts[1].str("replaced"), Some("exited"));
}
