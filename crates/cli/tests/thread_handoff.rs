//! #413: `thread handoff`: a new or an existing thread linked both ways, the text typed into its conductor as from the
//! source, flow-back rows (an issue filed, an ask answered), the board's chips, and a conductor's `thread new` refused.
mod common;
use common::*;
use herdsman::events::{read, Event};
use herdsman::serve::Server;
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

const ADDR: &str = "127.0.0.1:7777";

fn setup(name: &str) -> herdsman::Ctx {
    let d = tmpdir(name);
    let c = ctx(
        &d,
        &format!("{CONFIG}\n[fleet.sendmeter.role.conductor]\nharness = \"claude\"\nmodel = \"opus\"\n"),
    );
    let profile = c.config.with_file_name("claude").join("fleet-conductor");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join("settings.json"), "{}").unwrap();
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Grill #12"]).1,
        "t1\n"
    );
    c
}

fn kinds(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
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
        .reply("herdr pane send-keys", "")
        .reply("herdr agent start", "")
        .reply("herdr agent wait", "")
        .reply("herdr agent prompt", "")
}

fn show(c: &herdsman::Ctx, id: &str) -> String {
    run(c, &Fake::default(), &["thread", "show", id]).1
}

/// The real `herdsman` binary on `c`'s files, in a conductor lane of thread `thread` (`None`: the owner's shell).
fn bin(c: &herdsman::Ctx, thread: Option<&str>, args: &[&str], stdin: &str) -> (i32, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_herdsman"));
    for k in [
        "HERDSMAN_LANE",
        "FLEET_LANE",
        "HERDSMAN_THREAD",
        "HERDSMAN_MAIN",
        "BERTH",
    ] {
        cmd.env_remove(k);
    }
    if let Some(t) = thread {
        cmd.env("HERDSMAN_LANE", "sendmeter:-:conductor")
            .env("HERDSMAN_THREAD", t);
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

/// The conductor of `thread` ran `cmd` in its shell, which printed `stdout` (a `PostToolUse` hook call).
fn ran(c: &herdsman::Ctx, thread: &str, cmd: &str, stdout: &str) {
    let input = json!({"hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": {"command": cmd},
        "tool_response": {"stdout": stdout, "stderr": "", "interrupted": false}});
    let (code, out) = bin(c, Some(thread), &["hook", "--role", "conductor"], &input.to_string());
    assert_eq!(code, 0, "{out}");
}

#[test]
fn a_handoff_to_a_repo_starts_a_linked_thread_and_wakes_its_conductor_with_the_text() {
    let c = setup("handoff-new");
    let r = herdr(NO_AGENTS);
    let (code, out) = run(
        &c,
        &r,
        &["thread", "handoff", "t1", "sendmeter", "Rate limits", "429s from #19"],
    );
    assert_eq!((code, out.as_str()), (0, "t2\n"));
    let new = kinds(&c, "thread-new");
    assert_eq!(
        (new.len(), new[1].str("title"), new[1].fleet.as_str()),
        (2, Some("Rate limits"), "sendmeter")
    );
    let h = kinds(&c, "thread-handoff");
    assert_eq!(h.len(), 1);
    assert_eq!(
        (h[0].str("from"), h[0].str("to"), h[0].str("title"), h[0].str("text")),
        (Some("t1"), Some("t2"), Some("Rate limits"), Some("429s from #19"))
    );
    // t2's conductor starts, the handoff its thread's last message, marked as from t1 (not the owner).
    assert!(r.called("herdr agent start conductor-t2"), "{:?}", r.calls.borrow());
    let dir = c.state.with_file_name("conductor").join("t2");
    assert_eq!(
        std::fs::read_to_string(dir.join(".thread.md")).unwrap(),
        "from t1: 429s from #19\n"
    );
    let msg = kinds(&c, "thread-msg");
    assert_eq!((msg[0].str("thread"), msg[0].str("from")), (Some("t2"), Some("t1")));
    assert!(
        show(&c, "t1").ends_with("continued in t2: Rate limits\n"),
        "{}",
        show(&c, "t1")
    );
    assert!(
        show(&c, "t2").ends_with("from t1: 429s from #19\n"),
        "{}",
        show(&c, "t2")
    );

    let (code, out) = run(&c, &r, &["thread", "handoff", "t1", "t1", "x", "y"]);
    assert!(code == 3 && out.contains("REFUSED BAD_VALUE"), "{out}");
    let (code, out) = run(&c, &r, &["thread", "handoff", "t1", "t9", "x", "y"]);
    assert!(code == 3 && out.contains("REFUSED NO_SUCH_THREAD"), "{out}");
    assert_eq!(kinds(&c, "thread-new").len(), 2, "refusals start no thread");
}

#[test]
fn a_handoff_to_an_existing_thread_appends_to_it_and_types_into_its_awake_conductor() {
    let c = setup("handoff-reuse");
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Search"]).1,
        "t2\n"
    );
    let r = herdr(&agents(&[("conductor-t2", "idle", "/x", "w9:p2")]));
    // An awake t2 conductor: its start is logged.
    let st = json!({"thread": "t2", "agent": "conductor-t2", "pane": "w9:p2", "workspace": "w9", "harness": "claude"});
    herdsman::events::append(&c.state, &Event::new(c.now, "sendmeter", None, "conductor-start", st)).unwrap();
    let (code, out) = run(
        &c,
        &r,
        &["thread", "handoff", "t1", "t2", "Burst size", "burst 100, not 50"],
    );
    assert_eq!((code, out.as_str()), (0, "t2\n"));
    assert_eq!(kinds(&c, "thread-new").len(), 2, "no new thread");
    assert!(
        r.called("herdr agent prompt conductor-t2 [t2] t1: burst 100, not 50"),
        "{:?}",
        r.calls.borrow()
    );
    assert!(!r.called("herdr agent start"));
    assert!(
        show(&c, "t2").ends_with("from t1: burst 100, not 50\n"),
        "{}",
        show(&c, "t2")
    );
    assert!(
        show(&c, "t1").ends_with("continued in t2: Burst size\n"),
        "{}",
        show(&c, "t1")
    );
}

#[test]
fn an_issue_filed_and_an_ask_answered_in_the_target_each_add_one_row_to_the_source() {
    let c = setup("handoff-flow");
    assert_eq!(
        run(
            &c,
            &herdr(NO_AGENTS),
            &["thread", "handoff", "t1", "sendmeter", "Rate limits", "go"]
        )
        .1,
        "t2\n"
    );
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
    // The number unknown (no URL printed): the title alone.
    ran(&c, "t2", "gh issue create --title='Burst docs' -F -", "");
    assert!(
        show(&c, "t1").ends_with("from t2: filed: Burst docs\n"),
        "{}",
        show(&c, "t1")
    );
    // Another command, or an issue filed in a thread nobody handed off to, adds nothing.
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
    let r = herdr(&agents(&[("conductor-t2", "idle", "/x", "w9:p1")]));
    assert_eq!(run(&c, &r, &["answer", "sendmeter", "q1", "1"]).0, 0);
    let t1 = show(&c, "t1");
    assert_eq!(t1.lines().count(), rows + 1, "{t1}");
    assert!(t1.ends_with("from t2: answered q1: Which scope? → small\n"), "{t1}");
}

#[test]
fn a_conductors_thread_new_is_refused_naming_handoff_and_the_owners_still_works() {
    let c = setup("handoff-perm");
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
    // Any conductor may hand off to any repo (`all` too).
    let r = herdr(NO_AGENTS);
    assert_eq!(
        run(&c, &r, &["thread", "handoff", "t1", "all", "Cross-repo", "both fleets"]).1,
        "t3\n"
    );
    assert_eq!(kinds(&c, "thread-new")[2].fleet, "all");
    let usage = run(&c, &Fake::default(), &["help", "--agent"]).1;
    assert!(
        usage.contains("`herdsman thread handoff <FROM> <TO> <TITLE> <TEXT>`"),
        "{usage}"
    );
    let brief = std::fs::read_to_string(c.state.with_file_name("conductor").join("t3/.brief.md")).unwrap();
    assert!(
        brief.contains("herdsman thread handoff <thread> <repo|t<n>>"),
        "{brief}"
    );
}

fn req(s: &Server, method: &str, path: &str, extra: &str, now: u64) -> String {
    let r = format!("{method} {path} HTTP/1.1\r\nHost: {ADDR}\r\n{extra}Content-Length: 0\r\n\r\n");
    let res = String::from_utf8_lossy(&s.respond(&r, now)).into_owned();
    res[res.find("\r\n\r\n").unwrap() + 4..].to_string()
}

/// A linked pair, t1 → t2, with a flow-back line in t1.
fn linked(name: &str) -> herdsman::Ctx {
    let c = setup(name);
    assert_eq!(
        run(
            &c,
            &herdr(NO_AGENTS),
            &["thread", "handoff", "t1", "sendmeter", "Rate limits", "go"]
        )
        .1,
        "t2\n"
    );
    ran(
        &c,
        "t2",
        "gh issue create -t \"Rate-limit search\"",
        "https://github.com/acme/x/issues/52",
    );
    c
}

#[test]
fn the_chat_tab_shows_a_chip_on_each_linked_row_and_thread_row() {
    let c = linked("handoff-tab");
    let r = Fake::default().reply("herdr agent list", NO_AGENTS);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", c.now);
    let j: Value = serde_json::from_str(&req(&s, "GET", "/frags?tab=chat", &cookie, c.now)).unwrap();
    let chat = j["chat"].as_str().unwrap();
    let view = |t: &str| {
        let v = &chat[chat.find(&format!("<div class=cvp data-t=\"{t}\">")).unwrap()..];
        v[..v.find("</form></div>").unwrap()].to_string()
    };
    let (go1, go2) = (
        "<span class=lk data-go=\"t1\">from t1</span>",
        "<span class=lk data-go=\"t2\">continued in t2</span>",
    );
    assert!(
        view("t1").contains(&format!("<p class=qs>{go2} · Rate limits</p>")),
        "{}",
        view("t1")
    );
    let back = "<p class=\"qs note\"><span class=lk data-go=\"t2\">from t2</span> filed #52: Rate-limit search</p>";
    assert!(view("t1").contains(back), "{}", view("t1"));
    assert!(
        view("t2").contains(&format!(
            "<details class=\"blk ai\" open><summary><span class=k>{go1}</span>"
        )),
        "{}",
        view("t2")
    );
    let row = |t: &str| {
        let r = &chat[chat.find(&format!("href=#chat data-t=\"{t}\">")).unwrap()..];
        r[..r.find("</a>").unwrap()].to_string()
    };
    assert!(row("t1").contains(&format!("<span class=tp>{go2} ")), "{}", row("t1"));
    assert!(row("t2").contains(&format!("<span class=tp>{go1} ")), "{}", row("t2"));
}

/// A tap on a chip, in a thread or in the list, opens the other thread (WebKit, serve.js's click handler).
#[test]
fn a_tap_on_a_chip_opens_the_other_thread() {
    if Command::new("swift").arg("--version").output().is_err() || !cfg!(target_os = "macos") {
        eprintln!("skipping: no swift/WebKit here");
        return;
    }
    let c = linked("handoff-tap");
    let r = Fake::default().reply("herdr agent list", NO_AGENTS);
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", c.now);
    let page = req(&s, "GET", "/", &format!("{cookie}Cookie: tab=chat\r\n"), c.now);
    let j: Value = serde_json::from_str(&req(&s, "GET", "/frags?tab=chat", &cookie, c.now)).unwrap();
    let html = page.replacen(
        "<section id=chat>",
        &format!("<section id=chat>{}", j["chat"].as_str().unwrap()),
        1,
    );
    assert!(html.contains("class=lk data-go"), "the chat tab is on the page");
    let d = tmpdir("handoff-tap-page");
    let (file, png) = (d.join("page.html"), d.join("tap.png"));
    std::fs::write(&file, html).unwrap();
    // 1: t1's handoff chip opens t2; 2: t2's message chip opens t1; 3: t2's row chip in the list opens t2.
    let js = "window.fetch = async () => new Response('', {status: 404}); const tap = s => document.querySelector(s).click(); \
        chatOpen('t1'); tap('.cvp[data-t=\"t1\"] p.qs .lk[data-go=\"t2\"]'); const a = THR === 't2' ? 1 : 0; \
        tap('.cvp[data-t=\"t2\"] summary .lk[data-go=\"t1\"]'); const b = THR === 't1' ? 1 : 0; \
        chatOpen(''); tap('.tr[data-t=\"t1\"] .lk[data-go=\"t2\"]'); const d = THR === 't2' ? 1 : 0; \
        window.measure = [a, b, d, document.querySelector('.cvp[data-t=\"t2\"] summary').parentElement.open ? 1 : 0]";
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/webview.swift");
    let o = swift()
        .args([
            script,
            file.to_str().unwrap(),
            "390",
            "chat",
            "horse",
            png.to_str().unwrap(),
            js,
        ])
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    let v: Vec<f64> = out.split_whitespace().filter_map(|n| n.parse().ok()).collect();
    assert_eq!(
        v.get(2..),
        Some(&[1.0, 1.0, 1.0, 1.0][..]),
        "{out} {}",
        String::from_utf8_lossy(&o.stderr)
    );
}
