//! #404: `[chat]` sets new threads' conductor defaults (else the fleet's `role.conductor`); a thread keeps the harness
//! it started on (`thread new --harness`, the board's harness chip); its model/effort chip applies from the next turn.
mod common;
use common::*;
use herdsman::config::Config;
use herdsman::events::{append, read, Event};
use herdsman::serve::Server;
use serde_json::json;

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

/// `CONFIG` with a claude conductor on opus (rev on claude sonnet: the claude models are opus and sonnet).
fn config(extra: &str) -> String {
    let c = CONFIG.replace(
        "[fleet.sendmeter.role.rev]\nharness = \"hermes\"",
        "[fleet.sendmeter.role.rev]\nharness = \"claude\"\nmodel = \"sonnet\"",
    );
    format!(
        "{c}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\neffort = \"high\"\n{extra}"
    )
}

fn req(s: &Server, path: &str, body: &str, extra: &str) -> String {
    let r = format!(
        "POST {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    String::from_utf8_lossy(&s.respond(&r, T)).into_owned()
}

/// The [chat] tab as the board draws it (#269: web/src/chat.ts from API v1's `chat`).
fn chat(s: &Server, cookie: &str) -> String {
    let [_, _, chat] = parts(s, ADDR, cookie, T);
    chat
}

/// herdr ready to start conductors (`agents`: its agent list), dsh-tui at the pinned profile.
fn host(agents: &str) -> Fake {
    let dsh = format!("profile: {}  /x\n", herdsman::harness::DSH_TUI_VERSION);
    Fake::default()
        .reply("herdr agent list", agents)
        .reply(
            "herdr workspace create",
            r#"{"result":{"root_pane":{"pane_id":"w9:p1"}}}"#,
        )
        .reply(
            "herdr pane get",
            r#"{"result":{"pane":{"agent":"dsh-tui","agent_status":"idle"}}}"#,
        )
        .reply("herdr", "")
        .reply("dsh-tui version", &dsh)
}

fn kind(c: &herdsman::Ctx, k: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == k).collect()
}

#[test]
fn chat_defaults_come_from_the_chat_table_else_the_conductor_role() {
    let plain = Config::parse(&config("")).unwrap();
    let (_, r) = herdsman::orchestrator::defaults(&plain, "sendmeter").unwrap();
    assert_eq!(
        (r.harness.as_str(), r.model.as_deref(), r.effort.as_deref()),
        ("claude", Some("opus"), Some("high"))
    );
    let on = Config::parse(&config("[chat]\nmodel = \"sonnet\"\neffort = \"low\"\n")).unwrap();
    let (_, r) = herdsman::orchestrator::defaults(&on, "sendmeter").unwrap();
    assert_eq!(
        (r.harness.as_str(), r.model.as_deref(), r.effort.as_deref()),
        ("claude", Some("sonnet"), Some("low"))
    );
    // `[chat]`'s model is a pick on the harness its fleets' conductors default to.
    let m = Config::parse(&config("[chat]\nmodel = \"haiku\"\n")).unwrap();
    assert_eq!(
        herdsman::orchestrator::harness_models(&m, "claude"),
        ["haiku", "opus", "sonnet"]
    );
    // Another harness drops the role's model; its profile stays the conductor's.
    let h = Config::parse(&config("[chat]\nharness = \"hermes\"\n")).unwrap();
    let (_, r) = herdsman::orchestrator::defaults(&h, "sendmeter").unwrap();
    assert_eq!(
        (r.harness.as_str(), r.model.as_deref(), r.profile.as_deref()),
        ("hermes", None, None)
    );
    let bad = Config::parse(&config("[chat]\nharness = \"codex\"\n")).unwrap_err();
    assert!(format!("{bad:#}").contains("chat.harness"), "{bad:#}");
    let bad = Config::parse(&config("[chat]\naccount = \"nobody\"\n")).unwrap_err();
    assert!(format!("{bad:#}").contains("not in [accounts]"), "{bad:#}");
}

#[test]
fn thread_new_harness_is_recorded_and_its_conductor_starts_on_it() {
    let c = ctx(&tmpdir("pick-new"), &config("[chat]\nharness = \"claude\"\n"));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let (code, out) = run(
        &c,
        &Fake::default(),
        &["thread", "new", "sendmeter", "Grill", "--harness", "hermes"],
    );
    assert_eq!((code, out.as_str()), (0, "t1\n"));
    let (code, out) = run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Plain"]);
    assert_eq!((code, out.as_str()), (0, "t2\n"));
    let news = kind(&c, "thread-new");
    assert_eq!(
        (news[0].str("harness"), news[1].str("harness")),
        (Some("hermes"), Some("claude"))
    );
    let (code, out) = run(
        &c,
        &Fake::default(),
        &["thread", "new", "sendmeter", "X", "--harness", "codex"],
    );
    assert!(code == 3 && out.contains("REFUSED BAD_VALUE"), "{out}");
    // A later [chat] change leaves the recorded harness alone.
    std::fs::write(&c.config, config("[chat]\nharness = \"dsh\"\n")).unwrap();
    let r = host(NO_AGENTS);
    assert_eq!(run(&c, &r, &["thread", "say", "t1", "hi"]).0, 0);
    let st = &kind(&c, "orchestrator-start")[0];
    assert_eq!((st.str("harness"), st.str("model")), (Some("hermes"), None));
    // The board's first message records the harness chip's pick; the header names it.
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let page = chat(&s, &cookie);
    assert!(
        page.contains(
            "<div class=cbar><select class=cp name=harness aria-label=harness><option value=\"claude\">claude</option>\
             <option value=\"hermes\">hermes</option><option value=\"dsh\" selected>dsh</option></select></div></form>"
        ),
        "the empty thread's chip, preset to the default: {page}"
    );
    let res = req(
        &s,
        "/thread",
        "op=new&repo=sendmeter&harness=claude&text=Board",
        &cookie,
    );
    assert!(res.ends_with("t3 started\n"), "{res}");
    assert_eq!(kind(&c, "thread-new")[2].str("harness"), Some("claude"));
    // #780 (q1173): the frozen board matches the old `conductor-*` types, so its awake dot is gone.
    assert!(chat(&s, &cookie)
        .contains("<b>Board</b><small class=tid>· t3</small></div><span class=ds>sendmeter · claude · opus"));
}

#[test]
fn the_conductors_instructions_reach_a_claude_a_hermes_and_a_dsh_thread() {
    let notes = tmpdir("pick-notes").join("AGENTS.md");
    std::fs::write(&notes, "Always grill scope first.\n").unwrap();
    let extra = format!("[conductor]\ninstructions = \"{}\"\n", notes.display());
    let c = ctx(&tmpdir("pick-reach"), &config(&extra));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    for (i, h) in ["claude", "hermes", "dsh"].iter().enumerate() {
        let id = format!("t{}", i + 1);
        assert_eq!(
            run(&c, &Fake::default(), &["thread", "new", "sendmeter", h, "--harness", h]).0,
            0
        );
        let r = host(NO_AGENTS);
        let (code, out) = run(&c, &r, &["thread", "say", &id, "hi"]);
        assert_eq!(code, 0, "{out}");
        assert_eq!(kind(&c, "orchestrator-start")[i].str("harness"), Some(*h));
        let dir = herdsman::orchestrator::dir(&c, &Config::load(&c.config).unwrap(), &id);
        let brief = std::fs::read_to_string(dir.join(".brief.md")).unwrap();
        assert!(
            brief.contains("# Orchestrator brief: thread") && brief.contains("Always grill scope first."),
            "{brief}"
        );
        // Each harness's own channel carries the kick that reads the brief.
        let calls = r.calls.borrow().join("\n");
        let kick = format!("Read {} and act on it.", dir.join(".brief.md").display());
        let channel = match *h {
            "claude" => "herdr agent start orchestrator-t1 --kind claude",
            "hermes" => "herdr agent prompt orchestrator-t2 Read ",
            _ => "herdr pane send-text w9:p1 Read ",
        };
        assert!(
            calls.lines().any(|l| l.starts_with(channel) && l.contains(&kick)),
            "{h}: {calls}"
        );
        if *h == "claude" {
            let role = c.config.with_file_name("claude").join("fleet-orchestrator/role.md");
            let line = calls.lines().find(|l| l.starts_with(channel)).unwrap();
            assert!(
                line.contains(&format!("--append-system-prompt-file {}", role.display())),
                "{line}"
            );
            assert!(std::fs::read_to_string(role).unwrap().contains("orchestrator"));
        }
    }
}

#[test]
fn a_model_and_effort_pick_is_logged_shown_and_the_next_turn_resumes_on_it() {
    let c = ctx(&tmpdir("pick-model"), &config(""));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let r = host(NO_AGENTS);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    let res = req(&s, "/thread", "op=new&repo=sendmeter&text=Grill", &cookie);
    assert!(res.ends_with("t1 started\n"), "{res}");
    let page = chat(&s, &cookie);
    assert!(
        page.contains(
            "<div class=cbar><select class=cp name=model aria-label=model><option value=\"opus\" selected>opus</option>\
             <option value=\"sonnet\">sonnet</option></select><select class=cp name=effort aria-label=effort>"
        ),
        "{page}"
    );
    // Only the thread harness's models and efforts.
    let res = req(&s, "/thread", "op=model&t=t1&model=gpt-9", &cookie);
    assert!(res.contains("REFUSED BAD_VALUE"), "{res}");
    let res = req(&s, "/thread", "op=model&t=t1&model=sonnet&effort=low", &cookie);
    assert!(res.ends_with("picked\n"), "{res}");
    let pick = &kind(&c, "thread-model")[0];
    assert_eq!((pick.str("model"), pick.str("effort")), (Some("sonnet"), Some("low")));
    let page = chat(&s, &cookie);
    assert!(
        page.contains(" class=\"qs note\">model sonnet · effort low from the next turn</p>"),
        "{page}"
    );
    assert!(
        page.contains("<option value=\"sonnet\" selected>sonnet</option>"),
        "{page}"
    );
    let (_, show) = run(&c, &Fake::default(), &["thread", "show", "t1"]);
    assert!(
        show.ends_with("system: model sonnet · effort low from the next turn\n"),
        "{show}"
    );
    // The conductor is awake: the next message closes it and resumes the same session on the pick.
    let first = kind(&c, "orchestrator-start")[0].clone();
    let awake = r#"{"result":{"agents":[{"name":"orchestrator-t1","agent_status":"idle","pane_id":"w9:p1"}]}}"#;
    let r2 = host(awake);
    assert_eq!(run(&c, &r2, &["thread", "say", "t1", "next"]).0, 0);
    let calls = r2.calls.borrow().join("\n");
    assert!(calls.contains("herdr workspace close w9"), "{calls}");
    let sleep = kind(&c, "orchestrator-sleep");
    assert_eq!(sleep[0].str("why"), Some("model"));
    let next = &kind(&c, "orchestrator-start")[1];
    assert_eq!((next.str("model"), next.str("effort")), (Some("sonnet"), Some("low")));
    assert_eq!(
        (next.str("session"), next.data["resumed"].as_bool()),
        (first.str("session"), Some(true))
    );
    let start = calls
        .lines()
        .find(|l| l.starts_with("herdr agent start orchestrator-t1"))
        .unwrap();
    assert!(start.contains("--model sonnet --effort low"), "{start}");
    assert!(
        start.contains(&format!("--resume {}", first.str("session").unwrap())),
        "{start}"
    );
    // Awake on the pick already: the message after is typed in, no restart.
    let r3 = host(awake);
    assert_eq!(run(&c, &r3, &["thread", "say", "t1", "again"]).0, 0);
    assert_eq!(kind(&c, "orchestrator-start").len(), 2);
}

#[test]
fn an_old_thread_without_a_harness_renders_and_starts_on_the_default() {
    let c = ctx(&tmpdir("pick-old"), &config(""));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let d = json!({"thread": "t1", "title": "Before #404"});
    append(&c.state, &Event::new(T - 60, "sendmeter", None, "thread-new", d)).unwrap();
    let r = host(NO_AGENTS);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    assert!(chat(&s, &cookie).contains(
        "<b>Before #404</b><small class=tid>· t1</small></div><span class=ds>sendmeter · claude · opus</span>"
    ));
    assert_eq!(run(&c, &r, &["thread", "say", "t1", "hi"]).0, 0);
    assert_eq!(kind(&c, "orchestrator-start")[0].str("harness"), Some("claude"));
}
