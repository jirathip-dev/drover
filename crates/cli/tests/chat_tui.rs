mod common;
use common::*;
use drover::events::{read, Event};
use drover_chat::App;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use std::time::Duration;

fn setup(name: &str, b: &FakeShed) -> drover::Ctx {
    let cfg = format!(
        "{CONFIG}\n[fleet.sendmeter.role.orchestrator]\nharness = \"claude\"\naccount = \"C\"\n\n\
         [fleet.sendmeter.runtime]\nkind = \"shed-acp\"\nsocket = \"{}\"\n\n[accounts]\nC = \"C\"\n",
        b.sock.display()
    );
    let mut c = ctx(&tmpdir(name), &cfg);
    c.exe = env!("CARGO_BIN_EXE_drover").into();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Board 502"]).1,
        "t1\n"
    );
    c
}

fn acp() -> Fake {
    let agents =
        serde_json::json!([{"id": "claude-acp", "version": "0.101.0", "distribution": "npx", "cmd": ["echo"]}]);
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

fn key(app: &mut App, code: KeyCode) {
    app.key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn typed(app: &mut App, text: &str) {
    text.chars().for_each(|ch| key(app, KeyCode::Char(ch)));
}

fn screen(t: &mut Terminal<TestBackend>, app: &mut App) -> String {
    app.tick(Duration::ZERO).unwrap();
    shot(t, app)
}

fn shot(t: &mut Terminal<TestBackend>, app: &App) -> String {
    t.draw(|f| app.draw(f)).unwrap();
    let b = t.backend().buffer();
    let rows = (0..b.area.height).map(|y| (0..b.area.width).map(|x| b[(x, y)].symbol()).collect::<String>());
    rows.collect::<Vec<_>>().join("\n")
}

fn until(app: &mut App, done: impl Fn(&App) -> bool) {
    for _ in 0..20 {
        if done(app) {
            return;
        }
        app.tick(Duration::from_secs(10)).unwrap();
    }
    panic!("never settled:\n{}", shot(&mut term(40, 18), app));
}

fn term(w: u16, h: u16) -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(w, h)).unwrap()
}

#[test]
fn with_no_serve_the_list_opens_a_thread_a_sent_line_reaches_the_orchestrator_and_its_reply_appears() {
    let b = FakeShed::new();
    let c = setup("chat-tui-send", &b);
    run(&c, &Fake::default(), &["thread", "reply", "t1", "serve is down"]);
    let f = acp();
    let cli = b.cli(&f);
    let cli = b.on(&cli);
    std::thread::scope(|s| {
        let mut app = App::new(s, &c, &cli, None);
        let mut t = term(40, 18);
        let list = screen(&mut t, &mut app);
        assert!(list.contains(" SENDMETER"), "{list}");
        assert!(list.contains("›● t1 Board 502"), "{list}");
        assert!(list.contains("↵ open · tab board"), "{list}");
        key(&mut app, KeyCode::Enter);
        let chat = screen(&mut t, &mut app);
        assert!(chat.contains("‹ t1 Board 502"), "{chat}");
        assert!(chat.contains("serve is down"), "{chat}");
        assert!(
            !drover::orchestrator::unread(&read(&c.state).unwrap(), "t1"),
            "opening marks it read"
        );
        until(&mut app, |a| {
            shot(&mut term(40, 18), a).lines().next().unwrap().contains("✗ serve")
        });
        typed(&mut app, "is anything lost?");
        assert!(screen(&mut t, &mut app).contains("› is anything lost?▌"));
        key(&mut app, KeyCode::Enter);
        let sent = kinds(&c, "thread-msg");
        let msg = sent.last().unwrap();
        assert_eq!(
            (msg.str("from"), msg.str("text")),
            (Some("owner"), Some("is anything lost?"))
        );
        assert!(
            screen(&mut t, &mut app).contains("› ▌message t1"),
            "the box clears once it is logged"
        );
        until(&mut app, |_| {
            f.calls
                .borrow()
                .iter()
                .any(|l| l.contains("[t1] owner: is anything lost?"))
        });
        run(
            &c,
            &Fake::default(),
            &["thread", "reply", "t1", "Nothing is lost: chat works"],
        );
        let chat = screen(&mut t, &mut app);
        assert!(
            chat.contains("Nothing is lost: chat"),
            "the reply appears with no key: {chat}"
        );
        app.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.quit);
    });
}

#[test]
fn keys_answer_the_open_ask_and_settle_a_card_and_an_expired_card_is_refused_as_the_cli_refuses_it() {
    let b = FakeShed::new();
    let c = setup("chat-tui-pick", &b);
    let ask = [
        "ask",
        "sendmeter",
        "-",
        "Restart serve?",
        "--option",
        "now",
        "--option",
        "wait",
        "--thread",
        "t1",
    ];
    assert_eq!(run(&c, &Fake::default(), &ask).1, "q1\n");
    let card = [
        "card",
        "post",
        "t1",
        "--cmd",
        "launchctl kickstart -k gui/501/dev.drover.serve",
        "--why",
        "502",
        "--blast",
        "the board",
        "--dir",
        "/srv",
    ];
    assert_eq!(run(&c, &Fake::default(), &card).1, "r1\n");
    let f = acp().reply("sh -c nohup", "4242");
    let cli = b.cli(&f);
    let cli = b.on(&cli);
    std::thread::scope(|s| {
        let mut app = App::new(s, &c, &cli, Some("t1"));
        let mut t = term(40, 18);
        let chat = screen(&mut t, &mut app);
        assert!(chat.contains("1-9 answer · a d card"), "{chat}");
        typed(&mut app, "x2");
        assert!(
            screen(&mut t, &mut app).contains("› x2▌"),
            "with text in the box a number only types"
        );
        app.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        key(&mut app, KeyCode::Char('2'));
        let pick = screen(&mut t, &mut app);
        assert!(pick.contains("answer q1"), "{pick}");
        assert!(pick.contains("sends [2] wait"), "{pick}");
        key(&mut app, KeyCode::Backspace);
        assert!(!screen(&mut t, &mut app).contains("answer q1"), "⌫ backs out");
        key(&mut app, KeyCode::Char('2'));
        key(&mut app, KeyCode::Enter);
        until(&mut app, |_| !kinds(&c, "decision").is_empty());
        let d = &kinds(&c, "decision")[0];
        assert_eq!(
            (d.str("id"), d.str("choice"), d.str("by")),
            (Some("q1"), Some("wait"), Some("owner")),
            "{d:?}"
        );
        key(&mut app, KeyCode::Char('a'));
        let pick = screen(&mut t, &mut app);
        assert!(pick.contains("approve r1"), "{pick}");
        assert!(pick.contains("launchctl kickstart -k"), "{pick}");
        assert!(pick.contains("gui/501/dev.drover.serve"), "the full command: {pick}");
        assert!(pick.contains("in /srv"), "{pick}");
        key(&mut app, KeyCode::Enter);
        until(&mut app, |_| !kinds(&c, "run-settled").is_empty());
        assert_eq!(kinds(&c, "run-settled")[0].val("approved"), &serde_json::json!(true));
    });
    let card = [
        "card", "post", "t1", "--cmd", "echo hi", "--why", "w", "--blast", "none", "--dir", "/srv",
    ];
    assert_eq!(run(&c, &Fake::default(), &card).1, "r2\n");
    let late = drover::Ctx {
        now: c.now + 1000,
        ..c.clone()
    };
    let f = acp();
    let cli = b.cli(&f);
    let cli = b.on(&cli);
    std::thread::scope(|s| {
        let mut app = App::new(s, &late, &cli, Some("t1"));
        let mut t = term(40, 18);
        assert!(screen(&mut t, &mut app).contains("$ r2 · expired"));
        key(&mut app, KeyCode::Char('a'));
        key(&mut app, KeyCode::Enter);
        until(&mut app, |a| shot(&mut term(40, 18), a).contains("REFUSED"));
        let shown = screen(&mut t, &mut app);
        assert!(shown.contains("REFUSED EXPIRED"), "{shown}");
        assert!(shown.contains("nothing ran"), "{shown}");
    });
    assert_eq!(kinds(&c, "run-settled").len(), 1, "the expired card never settles");
}

#[test]
fn it_wraps_and_never_cuts_at_40x18_splits_at_120x40_and_never_panics_on_a_tiny_or_resized_screen() {
    let b = FakeShed::new();
    let c = setup("chat-tui-sizes", &b);
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "all", "Docs: issue names"]).1,
        "t2\n"
    );
    let long = "Tonight's self-update swapped the binary at 09:30 and serve's restart failed. Its log:\n```\n09:30:12 error: Address already in use\n```";
    run(&c, &Fake::default(), &["thread", "reply", "t1", long]);
    let ask = [
        "ask",
        "sendmeter",
        "-",
        "Restart serve now, or wait for the fix?",
        "--option",
        "Restart it now",
        "--option",
        "Wait",
        "--recommend",
        "2",
        "--thread",
        "t1",
    ];
    assert_eq!(run(&c, &Fake::default(), &ask).1, "q1\n");
    let f = acp();
    let cli = b.cli(&f);
    let cli = b.on(&cli);
    std::thread::scope(|s| {
        let mut app = App::new(s, &c, &cli, None);
        let mut t = term(120, 40);
        let wide = screen(&mut t, &mut app);
        let at = |w: &str, s: &str| w.lines().position(|l| l.contains(s)).unwrap();
        assert!(at(&wide, "ALL REPOS") < at(&wide, "SENDMETER"), "{wide}");
        key(&mut app, KeyCode::Char('j'));
        let wide = screen(&mut t, &mut app);
        assert!(
            wide.contains("›● t1 Board 502") && wide.lines().nth(1).unwrap().contains("│‹ t1 Board 502"),
            "the list and the chat side by side: {wide}"
        );
        key(&mut app, KeyCode::Enter);
        let mut t = term(40, 18);
        let chat = screen(&mut t, &mut app);
        for word in long.split_whitespace().filter(|w| !w.starts_with("```")) {
            assert!(chat.contains(word), "{word} is cut: {chat}");
        }
        assert!(
            chat.contains("┌") && chat.contains("│ 09:30:12 error: Address"),
            "a fenced block is a box: {chat}"
        );
        assert!(chat.contains("[2] Wait (recommended)"), "{chat}");
        assert!(chat.lines().nth(16).unwrap().starts_with("› ▌message t1"), "{chat}");
        assert!(chat.lines().all(|l| l.chars().count() == 40));
        for (w, h) in [(30, 10), (12, 4), (1, 1), (39, 18), (200, 3)] {
            t.backend_mut().resize(w, h);
            let small = screen(&mut t, &mut app);
            assert_eq!(small.lines().count(), usize::from(h), "{small}");
        }
        key(&mut app, KeyCode::Esc);
        t.backend_mut().resize(40, 18);
        assert!(screen(&mut t, &mut app).contains("↵ open"), "Esc goes back to the list");
        key(&mut app, KeyCode::Char('q'));
        assert!(app.quit);
    });
}

#[test]
fn shed_down_is_started_with_its_job_and_the_top_line_says_so_or_gives_the_command_to_run() {
    let b = FakeShed::new();
    let c = setup("chat-tui-shed", &b);
    let gone = c.config.with_file_name("gone.sock");
    let toml = std::fs::read_to_string(&c.config)
        .unwrap()
        .replace(&b.sock.display().to_string(), &gone.display().to_string());
    std::fs::write(&c.config, toml).unwrap();
    for (f, top, row) in [
        (Fake::default().reply("lsof", ""), "✗ shed", "$ shed serve --detach"),
        (
            Fake::default().reply("lsof", "").reply("sh -c p=", ""),
            "↻ shed",
            "Starting it with the",
        ),
    ] {
        std::thread::scope(|s| {
            let mut app = App::new(s, &c, &f, Some("t1"));
            until(&mut app, |a| {
                shot(&mut term(40, 18), a).lines().next().unwrap().contains("shed")
            });
            let shown = screen(&mut term(40, 18), &mut app);
            assert!(shown.lines().next().unwrap().contains(top), "{shown}");
            assert!(!shown.contains("✗ serve"), "serve is up: {shown}");
            assert!(shown.contains(row), "{shown}");
            assert!(shown.contains("‹ t1 Board 502"), "the thread stays readable: {shown}");
        });
    }
    drover::CHAT.set(drover_chat::main).ok();
    assert_eq!(
        run(&c, &Fake::default(), &["chat", "t9"]),
        run(&c, &Fake::default(), &["thread", "show", "t9"])
    );
}

#[test]
fn the_hook_refuses_chat_in_every_lane() {
    let c = ctx(&tmpdir("chat-tui-hook"), CONFIG);
    let input =
        r#"{"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": "drover chat t1"}}"#;
    for lane in ["sendmeter:12:impl", "sendmeter:-:orchestrator"] {
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_drover"))
            .env_remove("DROVER_SCRATCH_TARGET")
            .env("DROVER_LANE", lane)
            .env("FLEET_CONFIG", &c.config)
            .env("FLEET_STATE", &c.state)
            .args(["hook", "--role", "impl"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut child.stdin.take().unwrap(), input.as_bytes()).unwrap();
        let out = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
        assert!(out.contains("REFUSED OWNER_ONLY: `drover chat`"), "{lane}: {out}");
    }
}

#[test]
fn every_key_of_the_designs_list_does_what_the_list_says() {
    let b = FakeShed::new();
    let c = setup("chat-tui-keys", &b);
    for n in 0..12 {
        run(&c, &Fake::default(), &["thread", "reply", "t1", &format!("line {n}")]);
    }
    let card = [
        "card", "post", "t1", "--cmd", "echo hi", "--why", "w", "--blast", "none", "--dir", "/srv",
    ];
    assert_eq!(run(&c, &Fake::default(), &card).1, "r1\n");
    let f = acp();
    let cli = b.cli(&f);
    let cli = b.on(&cli);
    let ctrl = |app: &mut App, ch: char| app.key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL));
    std::thread::scope(|s| {
        let mut app = App::new(s, &c, &cli, None);
        let mut t = term(40, 18);
        screen(&mut t, &mut app);
        key(&mut app, KeyCode::Tab);
        assert!(screen(&mut t, &mut app).contains("#1354"));
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Char('?'));
        assert!(screen(&mut t, &mut app).contains("Ctrl-C"));
        (0..9).for_each(|_| key(&mut app, KeyCode::Char('j')));
        assert!(
            screen(&mut t, &mut app).contains("Esc or ⌫"),
            "the sheet scrolls to its end"
        );
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Char('k'));
        key(&mut app, KeyCode::Enter);
        typed(&mut app, "zb");
        key(&mut app, KeyCode::Left);
        typed(&mut app, "X");
        assert!(screen(&mut t, &mut app).contains("› zX▌b"));
        key(&mut app, KeyCode::Right);
        key(&mut app, KeyCode::Backspace);
        assert!(screen(&mut t, &mut app).contains("› zX▌"));
        ctrl(&mut app, 'u');
        assert!(screen(&mut t, &mut app).contains("› ▌message t1"));
        let bottom = screen(&mut t, &mut app);
        key(&mut app, KeyCode::Up);
        assert_ne!(screen(&mut t, &mut app), bottom, "↑ scrolls");
        key(&mut app, KeyCode::Down);
        assert_eq!(screen(&mut t, &mut app), bottom);
        ctrl(&mut app, 'b');
        assert_ne!(screen(&mut t, &mut app), bottom, "Ctrl-B pages up");
        ctrl(&mut app, 'f');
        assert_eq!(screen(&mut t, &mut app), bottom, "Ctrl-F pages back down");
        key(&mut app, KeyCode::Char('d'));
        let pick = screen(&mut t, &mut app);
        assert!(
            pick.contains("deny r1") && pick.contains("nothing runs") && pick.contains("echo hi"),
            "{pick}"
        );
        key(&mut app, KeyCode::Esc);
        assert_eq!(screen(&mut t, &mut app), bottom, "Esc backs out");
        key(&mut app, KeyCode::Char('d'));
        key(&mut app, KeyCode::Enter);
        until(&mut app, |_| !kinds(&c, "run-settled").is_empty());
        assert_eq!(kinds(&c, "run-settled")[0].val("approved"), &serde_json::json!(false));
        typed(&mut app, "q");
        assert!(!app.quit, "q types in the chat");
        ctrl(&mut app, 'u');
        key(&mut app, KeyCode::Esc);
        assert!(screen(&mut t, &mut app).contains("↵ open"), "Esc: back to the list");
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Backspace);
        assert!(
            screen(&mut t, &mut app).contains("↵ open"),
            "⌫ with the box empty: back to the list"
        );
    });
}

#[test]
fn a_message_shed_did_not_take_stays_logged_and_goes_once_shed_answers() {
    let b = FakeShed::new();
    let c = setup("chat-tui-wait", &b);
    let f = acp().once_err("shed run", 1, "shed: no server");
    let cli = b.cli(&f);
    let cli = b.on(&cli);
    std::thread::scope(|s| {
        let mut app = App::new(s, &c, &cli, Some("t1"));
        let mut t = term(40, 18);
        screen(&mut t, &mut app);
        typed(&mut app, "still there?");
        key(&mut app, KeyCode::Enter);
        assert_eq!(
            kinds(&c, "thread-msg").last().unwrap().str("text"),
            Some("still there?")
        );
        until(&mut app, |_| f.count("shed run") == 2);
        let shown = screen(&mut t, &mut app);
        assert!(!shown.contains("not delivered"), "{shown}");
        assert!(shown.contains("you 0s ✓"), "{shown}");
    });
}
