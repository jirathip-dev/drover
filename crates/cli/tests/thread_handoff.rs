mod common;
use common::*;
use drover::events::{read, Event};
use drover::serve::Server;
use serde_json::json;
use std::io::Write;
use std::process::{Command, Stdio};

const ADDR: &str = "127.0.0.1:7777";

fn acp_setup(name: &str, b: &FakeShed) -> drover::Ctx {
    let cfg = format!(
        "{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\nmodel = \"opus\"\naccount = \"C\"\n\n\
         [fleet.sendmeter.runtime]\nkind = \"shed-acp\"\nsocket = \"{}\"\n\n[accounts]\nC = \"C\"\n",
        b.sock.display()
    );
    let mut c = ctx(&tmpdir(name), &cfg);
    c.exe = env!("CARGO_BIN_EXE_drover").into();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Grill #12"]).1,
        "t1\n"
    );
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

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

fn to_new(c: &drover::Ctx, r: &dyn drover::runner::Runner, repo: &str, title: &str, text: &str) -> String {
    let (code, out) = run(c, r, &["thread", "handoff", "t1", repo, title, text]);
    assert_eq!(code, 0, "{out}");
    out.trim().to_string()
}

fn show(c: &drover::Ctx, id: &str) -> String {
    run(c, &Fake::default(), &["thread", "show", id]).1
}

fn bin(c: &drover::Ctx, thread: Option<&str>, args: &[&str], stdin: &str) -> (i32, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_drover"));
    for k in [
        "DROVER_LANE",
        "FLEET_LANE",
        "DROVER_THREAD",
        "DROVER_OUTBOX",
        "SHED",
        "BERTH",
    ] {
        cmd.env_remove(k);
    }
    if let Some(t) = thread {
        cmd.env("DROVER_LANE", "sendmeter:-:orchestrator")
            .env("DROVER_THREAD", t);
    }
    let mut child = cmd
        .args(args)
        .env("FLEET_CONFIG", &c.config)
        .env("FLEET_STATE", &c.state)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&o.stdout).into_owned() + &String::from_utf8_lossy(&o.stderr);
    (o.status.code().unwrap_or(-1), text)
}

fn ran(c: &drover::Ctx, thread: &str, cmd: &str, stdout: &str) {
    let input = json!({"hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": {"command": cmd},
        "tool_response": {"stdout": stdout, "stderr": "", "interrupted": false}});
    let (code, out) = bin(c, Some(thread), &["hook", "--role", "orchestrator"], &input.to_string());
    assert_eq!(code, 0, "{out}");
}

#[test]
fn a_handoff_to_a_repo_starts_its_desk_thread_and_wakes_its_orchestrator_with_the_text() {
    let b = FakeShed::new();
    let c = acp_setup("handoff-new", &b);
    let r = acp();
    let cli = b.cli(&r);
    assert_eq!(to_new(&c, &cli, "sendmeter", "Rate limits", "429s from #19"), "t2");
    let new = kinds(&c, "thread-new");
    assert_eq!(
        (new.len(), new[1].str("title"), new[1].fleet.as_str()),
        (2, Some("Desk"), "sendmeter")
    );
    let h = kinds(&c, "thread-handoff");
    assert_eq!(h.len(), 1);
    assert_eq!(
        (h[0].str("from"), h[0].str("to"), h[0].str("title"), h[0].str("text")),
        (Some("t1"), Some("t2"), Some("Rate limits"), Some("429s from #19"))
    );
    assert!(
        r.called("shed run --acp claude-acp --policy allow --name orchestrator-t2"),
        "{:?}",
        r.calls.borrow()
    );
    let sent = r.calls.borrow().join("\n");
    assert!(
        sent.contains("\n\n[t2] t1: handoff to Desk · t2: 429s from #19"),
        "{sent}"
    );
    let dir = c.state.with_file_name("orchestrator").join("t2");
    assert_eq!(
        std::fs::read_to_string(dir.join(".thread.md")).unwrap(),
        "from t1: handoff to Desk · t2: 429s from #19\n"
    );
    let msg = kinds(&c, "thread-msg");
    assert_eq!((msg[0].str("thread"), msg[0].str("from")), (Some("t2"), Some("t1")));
    assert!(
        show(&c, "t1").ends_with("continued in t2: Rate limits → Desk · t2\n"),
        "{}",
        show(&c, "t1")
    );
    assert!(
        show(&c, "t2").ends_with("from t1: handoff to Desk · t2: 429s from #19\n"),
        "{}",
        show(&c, "t2")
    );

    let (code, out) = run(&c, &cli, &["thread", "handoff", "t1", "t1", "x", "y"]);
    assert!(code == 3 && out.contains("REFUSED BAD_VALUE"), "{out}");
    let (code, out) = run(&c, &cli, &["thread", "handoff", "t1", "t9", "x", "y"]);
    assert!(code == 3 && out.contains("REFUSED NO_SUCH_THREAD"), "{out}");
    assert_eq!(kinds(&c, "thread-new").len(), 2, "refusals start no thread");
}

#[test]
fn a_handoff_to_a_repo_without_a_orchestrator_logs_nothing() {
    let b = FakeShed::new();
    let c = acp_setup("handoff-noorchestrator", &b);
    let cfg = std::fs::read_to_string(&c.config).unwrap();
    let other = "\n[fleet.other]\nrepo = \"jirathip-dev/other\"\nlocal = \"/tmp/does-not-matter/other\"\nintegration = \"staging\"\n";
    std::fs::write(&c.config, cfg + other).unwrap();
    let (code, out) = run(&c, &b.cli(&acp()), &["thread", "handoff", "t1", "other", "x", "y"]);
    assert!(code == 3 && out.contains("REFUSED NO_ORCHESTRATOR"), "{out}");
    assert_eq!(
        (
            kinds(&c, "thread-new").len(),
            kinds(&c, "thread-handoff").len(),
            kinds(&c, "needs-decision").len()
        ),
        (1, 0, 0)
    );
}

#[test]
fn a_handoff_to_an_existing_thread_appends_to_it_and_types_into_its_awake_orchestrator() {
    let b = FakeShed::new();
    let c = acp_setup("handoff-reuse", &b);
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Search"]).1,
        "t2\n"
    );
    b.add(
        "orchestrator-t2",
        json!({"fleet": "sendmeter", "role": "orchestrator", "thread": "t2"}),
    );
    let st = json!({"thread": "t2", "agent": "orchestrator-t2", "pane": "orchestrator-t2", "runtime": "shed-acp", "harness": "claude"});
    drover::events::append(
        &c.state,
        &Event::new(c.now, "sendmeter", None, "orchestrator-start", st),
    )
    .unwrap();
    let r = acp();
    let (code, out) = run(
        &c,
        &b.cli(&r),
        &["thread", "handoff", "t1", "t2", "Burst size", "burst 100, not 50"],
    );
    assert_eq!((code, out.as_str()), (0, "t2\n"));
    assert_eq!(kinds(&c, "thread-new").len(), 2, "no new thread");
    assert!(
        kinds(&c, "needs-decision").is_empty(),
        "a thread id is delivered at once, no card"
    );
    assert!(
        r.called("shed send orchestrator-t2 [t2] t1: handoff to Search · t2: burst 100, not 50"),
        "{:?}",
        r.calls.borrow()
    );
    assert!(!r.called("shed run"));
    assert!(
        show(&c, "t2").ends_with("from t1: handoff to Search · t2: burst 100, not 50\n"),
        "{}",
        show(&c, "t2")
    );
    assert!(
        show(&c, "t1").ends_with("continued in t2: Burst size → Search · t2\n"),
        "{}",
        show(&c, "t1")
    );
}

#[test]
fn a_forward_marks_the_received_row_and_a_handoff_after_the_owner_wrote_does_not() {
    let b = FakeShed::new();
    let c = acp_setup("handoff-forward", &b);
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Test"]).1,
        "t2\n"
    );
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Search"]).1,
        "t3\n"
    );
    let f = acp();
    let r = b.cli(&f);
    assert_eq!(
        run(&c, &r, &["thread", "handoff", "t1", "t2", "Facets", "add facets"]).1,
        "t2\n"
    );
    assert_eq!(
        run(
            &c,
            &r,
            &["thread", "handoff", "t2", "sendmeter", "Facets", "add facets"]
        )
        .0,
        0
    );
    assert!(
        show(&c, "t2").contains("from t1: handoff to Test · t2: add facets (forwarded)\n"),
        "{}",
        show(&c, "t2")
    );
    assert_eq!(
        run(&c, &r, &["thread", "handoff", "t1", "t2", "Docs", "doc it"]).1,
        "t2\n"
    );
    assert_eq!(run(&c, &r, &["thread", "say", "t2", "yes, do it here"]).0, 0);
    assert_eq!(
        run(&c, &r, &["thread", "handoff", "t2", "t3", "Index", "reindex"]).1,
        "t3\n"
    );
    let t2 = show(&c, "t2");
    assert!(t2.contains("handoff to Test · t2: doc it\n"), "{t2}");
    assert_eq!(t2.matches("(forwarded)").count(), 1, "{t2}");
    let f = Fake::default();
    let r = b.on(&f);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", c.now);
    let chat = chat_all(&s, ADDR, &cookie, c.now).to_string();
    assert_eq!(
        chat.matches("\"text\":\"handoff to Test · t2: add facets (forwarded)\"")
            .count(),
        1,
        "{chat}"
    );
}

#[test]
fn an_issue_filed_and_an_ask_answered_in_the_target_each_add_one_row_to_the_source() {
    let b = FakeShed::new();
    let c = acp_setup("handoff-flow", &b);
    assert_eq!(to_new(&c, &b.cli(&acp()), "sendmeter", "Rate limits", "go"), "t2");
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Unlinked"]).1,
        "t3\n"
    );
    let before = show(&c, "t1");

    let cmd = "gh issue create -R acme/x -t \"Rate-limit /v1/search\" -F -";
    ran(&c, "t2", cmd, "https://github.com/acme/x/issues/52\n");
    assert_eq!(
        show(&c, "t1"),
        format!("{before}from t2: filed #52: Rate-limit /v1/search\n")
    );
    ran(&c, "t2", "gh issue create --title='Burst docs' -F -", "");
    assert!(
        show(&c, "t1").ends_with("from t2: filed: Burst docs\n"),
        "{}",
        show(&c, "t1")
    );
    let rows = show(&c, "t1").lines().count();
    ran(&c, "t2", "gh issue list", "#52 Rate-limit");
    ran(&c, "t3", cmd, "https://github.com/acme/x/issues/53\n");
    assert_eq!(show(&c, "t1").lines().count(), rows);
    assert!(!show(&c, "t3").contains("filed"));

    let ask = [
        "ask",
        "sendmeter",
        "-",
        "Which scope?",
        "--option",
        "small",
        "--option",
        "full",
        "--thread",
        "t2",
    ];
    assert_eq!(run(&c, &Fake::default(), &ask).1, "q1\n");
    let r = acp();
    assert_eq!(run(&c, &b.cli(&r), &["answer", "sendmeter", "q1", "1"]).0, 0);
    assert!(
        r.called("shed send orchestrator-t2 DECISION q1: small") && !r.called("shed run"),
        "{:?}",
        r.calls.borrow()
    );
    let t1 = show(&c, "t1");
    assert_eq!(t1.lines().count(), rows + 1, "{t1}");
    assert!(t1.ends_with("from t2: answered q1: Which scope? → small\n"), "{t1}");
}

#[test]
fn a_orchestrators_thread_new_is_refused_naming_handoff_and_the_owners_still_works() {
    let b = FakeShed::new();
    let c = acp_setup("handoff-perm", &b);
    let (code, out) = bin(&c, Some("t1"), &["thread", "new", "sendmeter", "Side quest"], "");
    assert!(
        code == 3 && out.contains("REFUSED USE_HANDOFF") && out.contains("thread handoff"),
        "{code} {out}"
    );
    assert_eq!(kinds(&c, "thread-new").len(), 1);
    assert_eq!(
        bin(&c, None, &["thread", "new", "sendmeter", "Side quest"], ""),
        (0, "t2\n".into())
    );
    assert_eq!(to_new(&c, &b.cli(&acp()), "all", "Cross-repo", "both fleets"), "t3");
    assert_eq!(kinds(&c, "thread-new")[2].fleet, "all");
    let usage = run(&c, &Fake::default(), &["help", "--agent"]).1;
    assert!(
        usage.contains("`drover thread handoff <FROM> <TO> <TITLE> <TEXT>`"),
        "{usage}"
    );
    let brief = std::fs::read_to_string(c.state.with_file_name("orchestrator").join("t3/.brief.md")).unwrap();
    assert!(brief.contains("drover thread handoff <thread> <repo|t<n>>"), "{brief}");
}

fn linked(name: &str, b: &FakeShed) -> drover::Ctx {
    let c = acp_setup(name, b);
    assert_eq!(to_new(&c, &b.cli(&acp()), "sendmeter", "Rate limits", "go"), "t2");
    ran(
        &c,
        "t2",
        "gh issue create -t \"Rate-limit search\"",
        "https://github.com/acme/x/issues/52",
    );
    c
}

#[test]
fn the_chat_api_links_each_linked_row_and_thread_row() {
    let b = FakeShed::new();
    let c = linked("handoff-tab", &b);
    let f = Fake::default();
    let r = b.on(&f);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", c.now);
    let chat: serde_json::Value = chat_all(&s, ADDR, &cookie, c.now);
    let thread = |t: &str| {
        chat["threads"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == t)
            .unwrap()["page"]["msgs"]
            .clone()
    };
    let link = |t: &str, w: &str| json!({"t": t, "w": w});
    let t1 = thread("t1");
    let msgs = t1.as_array().unwrap();
    assert!(
        msgs.iter()
            .any(|m| m["link"] == link("t2", "continued in") && m["text"] == "Rate limits → Desk · t2"),
        "{t1}"
    );
    assert!(
        msgs.iter()
            .any(|m| m["link"] == link("t2", "from") && m["text"] == "filed #52: Rate-limit search"),
        "{t1}"
    );
    let t2 = thread("t2");
    assert!(
        t2.as_array().unwrap().iter().any(|m| m["link"] == link("t1", "from")),
        "{t2}"
    );
    let rows = chat["repos"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|r| r["rows"].as_array().unwrap().iter());
    let links = |t: &str| rows.clone().find(|r| r["id"] == t).unwrap()["links"].clone();
    assert_eq!(links("t1"), json!([link("t2", "continued in")]));
    assert_eq!(links("t2"), json!([link("t1", "from")]));
}

fn long_text() -> String {
    let text: String = (0..300).map(|i| format!("line {i:03}. ")).collect();
    text[..2400].to_string()
}

#[test]
fn a_long_handoff_lands_in_full_in_a_new_thread_and_in_an_asleep_one() {
    let b = FakeShed::new();
    let c = acp_setup("handoff-long-acp", &b);
    let f = acp();
    let r = b.cli(&f);
    let text = long_text();
    assert_eq!(to_new(&c, &r, "sendmeter", "Big plan", &text), "t2");
    let first = kinds(&c, "thread-msg")
        .into_iter()
        .find(|e| e.str("thread") == Some("t2"))
        .unwrap();
    let line = format!("handoff to Desk · t2: {text}");
    assert_eq!(
        (first.str("from"), first.str("text")),
        (Some("t1"), Some(line.as_str()))
    );
    let md = c.state.with_file_name("orchestrator").join("t2/.thread.md");
    assert_eq!(std::fs::read_to_string(&md).unwrap(), format!("from t1: {line}\n"));

    b.remove("orchestrator-t2");
    let (code, out) = run(&c, &r, &["thread", "handoff", "t1", "t2", "More", &text]);
    assert_eq!((code, out.as_str()), (0, "t2\n"));
    assert!(
        std::fs::read_to_string(&md)
            .unwrap()
            .ends_with(&format!("from t1: {line}\n")),
        "{}",
        std::fs::read_to_string(&md).unwrap()
    );
    assert_eq!(f.count("shed run --acp"), 2, "{:?}", f.calls.borrow());
    let prompts: Vec<String> = f
        .calls
        .borrow()
        .iter()
        .filter(|l| l.starts_with("shed send orchestrator-t2 "))
        .cloned()
        .collect();
    assert_eq!(prompts.len(), 2, "{prompts:?}");
    for p in &prompts {
        assert!(p.ends_with(&format!("\n\n[t2] t1: {line}")), "{p}");
    }
}

fn v1_post(s: &Server, cookie: &str, now: u64, op: &str, body: &serde_json::Value) -> serde_json::Value {
    let body = body.to_string();
    let req = format!(
        "POST /api/v1/{op} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let res = String::from_utf8_lossy(&s.respond(&req, now)).into_owned();
    serde_json::from_str(res.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn items(s: &Server, cookie: &str, now: u64, t: &str) -> Vec<serde_json::Value> {
    let page: serde_json::Value = serde_json::from_str(&api(s, ADDR, cookie, now, &format!("thread/{t}"))).unwrap();
    let msgs = page["msgs"].as_array().unwrap().iter();
    msgs.filter(|m| !m["handoff"].is_null())
        .map(|m| m["handoff"].clone())
        .collect()
}

#[test]
fn a_handoff_to_a_repo_goes_at_once_to_its_desk_thread_and_files_no_ask() {
    let b = FakeShed::new();
    let c = acp_setup("handoff-desk", &b);
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Search"]).1,
        "t2\n"
    );
    let r = acp();
    let cli = b.cli(&r);
    assert_eq!(to_new(&c, &cli, "sendmeter", "Facets", "add facets"), "t3");
    assert_eq!(to_new(&c, &cli, "sendmeter", "Docs", "doc it"), "t3");
    assert!(kinds(&c, "needs-decision").is_empty());
    let facts = kinds(&c, "thread-facts");
    assert_eq!(facts.len(), 1, "{facts:?}");
    assert_eq!(
        (facts[0].str("thread"), facts[0].str("profile"), facts[0].val("desk")),
        (Some("t3"), Some("sendmeter"), &json!(true))
    );
    let h = kinds(&c, "thread-handoff");
    assert_eq!(
        h.iter().map(|e| (e.str("from"), e.str("to"))).collect::<Vec<_>>(),
        [(Some("t1"), Some("t3")), (Some("t1"), Some("t3"))]
    );
    assert_eq!(h[0].str("target"), Some("Desk · t3"));
    assert!(
        show(&c, "t3").ends_with("from t1: handoff to Desk · t3: doc it\n"),
        "{}",
        show(&c, "t3")
    );
    assert!(!show(&c, "t2").contains("handoff"), "{}", show(&c, "t2"));
}

#[test]
fn a_move_redelivers_a_desk_handoff_marks_the_first_copy_forwarded_and_flows_back_to_the_sender() {
    let b = FakeShed::new();
    let c = acp_setup("handoff-move", &b);
    let f = acp();
    let cli = b.cli(&f);
    assert_eq!(to_new(&c, &cli, "sendmeter", "Facets", "add facets"), "t2");
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Search"]).1,
        "t3\n"
    );
    let first = kinds(&c, "thread-handoff").pop().unwrap();
    let r = b.on(&cli);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", c.now);
    let body = json!({"id": first.seq.to_string(), "to": "t3"});
    let got = v1_post(&s, "", c.now, "handoff-move", &body);
    assert_eq!(got["code"], json!("UNPAIRED"), "{got}");
    let got = v1_post(&s, &ck, c.now, "handoff-move", &body);
    assert_eq!(
        (&got["ok"], &got["message"]),
        (&json!(true), &json!("moved to t3")),
        "{got}"
    );
    let again = v1_post(&s, &ck, c.now, "handoff-move", &body);
    assert_eq!(again["ok"], json!(true), "{again}");
    let other = v1_post(
        &s,
        &ck,
        c.now,
        "handoff-move",
        &json!({"id": first.seq.to_string(), "to": "t1"}),
    );
    assert_eq!(other["code"], json!("BAD_VALUE"), "{other}");

    let h = kinds(&c, "thread-handoff");
    assert_eq!(h.len(), 2, "{h:?}");
    assert_eq!(
        (h[1].str("from"), h[1].str("to"), h[1].val("moves")),
        (Some("t1"), Some("t3"), &json!(first.seq))
    );
    assert!(
        show(&c, "t2").contains("handoff to Desk · t2: add facets (forwarded)\n"),
        "{}",
        show(&c, "t2")
    );
    assert!(
        show(&c, "t3").contains("from t1: handoff to Search · t3: add facets\n"),
        "{}",
        show(&c, "t3")
    );
    assert!(
        f.calls
            .borrow()
            .iter()
            .any(|l| l.starts_with("shed send orchestrator-t2 ") && l.contains("moved to t3: don't act on it here")),
        "{:?}",
        f.calls.borrow()
    );
    let desk = items(&s, &ck, c.now, "t2");
    assert_eq!(desk[0]["state"], json!("forwarded"), "{desk:?}");
    let rows: serde_json::Value = serde_json::from_str(&api(&s, ADDR, &ck, c.now, "rows")).unwrap();
    let states: Vec<_> = rows["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["kind"] == "handoff")
        .map(|r| (r["to"]["id"].clone(), r["handoff"]["state"].clone()))
        .collect();
    assert_eq!(
        states,
        [(json!("t2"), json!("forwarded")), (json!("t3"), json!("delivered"))]
    );

    let before = show(&c, "t1");
    ran(
        &c,
        "t2",
        "gh issue create -t \"Desk one\"",
        "https://github.com/acme/x/issues/51\n",
    );
    assert_eq!(show(&c, "t1"), before, "the first copy's thread no longer flows back");
    ran(
        &c,
        "t3",
        "gh issue create -t \"Facets\"",
        "https://github.com/acme/x/issues/52\n",
    );
    assert_eq!(show(&c, "t1"), format!("{before}from t3: filed #52: Facets\n"));
}

#[test]
fn a_repos_threads_list_newest_active_first_with_newest_on_the_first() {
    let b = FakeShed::new();
    let c = acp_setup("repo-threads", &b);
    for (n, t) in [(2, "Search"), (3, "Billing")] {
        let at = drover::Ctx {
            now: c.now + n * 60,
            ..c.clone()
        };
        assert_eq!(
            run(&at, &Fake::default(), &["thread", "new", "sendmeter", t]).1,
            format!("t{n}\n")
        );
        assert_eq!(
            run(&at, &Fake::default(), &["thread", "reply", &format!("t{n}"), "on it"]).0,
            0
        );
    }
    let late = drover::Ctx {
        now: c.now + 600,
        ..c.clone()
    };
    assert_eq!(run(&late, &Fake::default(), &["thread", "reply", "t1", "grilled"]).0, 0);
    let r = Fake::default();
    let s = Server::new(&late, &r, Some(ADDR.into()));
    let v: serde_json::Value = serde_json::from_str(&api(&s, ADDR, "", late.now, "repo/sendmeter/threads")).unwrap();
    assert_eq!(v["repo"], "sendmeter");
    let rows: Vec<_> = v["threads"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| (t["id"].clone(), t["newest"].clone(), t["last"]["line"].clone()))
        .collect();
    assert_eq!(
        rows,
        [
            (json!("t1"), json!(true), json!("conductor: grilled")),
            (json!("t3"), json!(false), json!("conductor: on it")),
            (json!("t2"), json!(false), json!("conductor: on it"))
        ]
    );
    assert_eq!(v["threads"][0]["last"]["ts"], json!(late.now));
    assert_eq!(
        (&v["threads"][0]["title"], &v["threads"][0]["archived"]),
        (&json!("Grill #12"), &json!(false))
    );
    let req = format!("GET /api/v1/repo/nowhere/threads HTTP/1.1\r\nHost: {ADDR}\r\n\r\n");
    let res = String::from_utf8_lossy(&s.respond(&req, late.now)).into_owned();
    assert!(res.starts_with("HTTP/1.1 404") && res.contains("NOT_FOUND"), "{res}");
}
