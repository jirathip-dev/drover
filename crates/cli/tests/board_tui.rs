mod common;
use common::*;
use drover::events::{append, read, Event};
use drover_chat::{App, Local, Open};
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use serde_json::{json, Value};
use std::time::Duration;

const T: u64 = 1_000_000;

fn recorded(name: &str) -> (drover::Ctx, OnShed) {
    let c = ctx(&tmpdir(name), CONFIG);
    for args in [
        &["queue", "sendmeter", "add", "17", "20", "21"][..],
        &["queue", "sendmeter", "block", "--on", "17", "20"],
    ] {
        assert_eq!(run(&c, &Fake::default(), args).0, 0);
    }
    let ev = |ts: u64, n: Option<u64>, kind: &str, d: Value| {
        append(&c.state, &Event::new(ts, "sendmeter", n, kind, d)).unwrap();
    };
    let spawn = |ts, n, role: &str, agent: &str| {
        ev(
            ts,
            Some(n),
            "spawn",
            json!({"role": role, "agent": agent, "harness": "claude"}),
        );
    };
    spawn(T - 9_000, 9, "impl", "sendmeter-impl-9");
    ev(T - 600, Some(9), "merged", json!({"pr": 41, "into": "staging"}));
    spawn(T - 5_000, 12, "impl", "sendmeter-impl-12");
    spawn(T - 4_000, 12, "rev", "sendmeter-rev-12-r1");
    ev(
        T - 3_500,
        Some(12),
        "verdict",
        json!({"verdict": "FAIL", "sha": "a".repeat(40)}),
    );
    spawn(T - 3_000, 12, "fix", "sendmeter-impl-12");
    spawn(T - 8_000, 5, "impl", "sendmeter-impl-5");
    ev(
        T - 2_000,
        Some(5),
        "verdict",
        json!({"verdict": "FAIL", "sha": "b".repeat(40)}),
    );
    ev(
        T - 1_900,
        Some(5),
        "needs-human",
        json!({"code": "ROUND_LIMIT", "reason": "review rounds ran out (4/3)"}),
    );
    let ask = json!({"id": "q7", "question": "Move the branch out?", "options": ["move", "refuse"], "recommend": 1, "kind": "routine"});
    ev(T - 300, None, "needs-decision", ask);
    let card = json!({"thread": "t1", "id": "r1", "cmd": "git push origin --delete wip/x", "why": "Delete #9's wip snapshot",
        "blast": "origin: 1 wip/ branch", "dir": "/srv/sendmeter", "host": "main", "timeout": 60, "sha": "3b7e0c55a1d9".repeat(5)});
    ev(T - 120, None, "run-card", card);
    let issue = |t: &str, created: &str| json!({"title": t, "labels": ["ui"], "created": created, "updated": created, "ac": true, "pr": false});
    let bl = json!({"ts": T, "issues": {"40": issue("Dark mode", "1970-01-01T00:00:00Z"), "41": issue("Fix push", "1970-01-12T00:00:00Z")}});
    let p = c.state.with_file_name("cache");
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(p.join("backlog-sendmeter-0.json"), bl.to_string()).unwrap();
    let r = OnShed::new(Fake::default());
    r.1.lane("sendmeter-impl-12", "working", "/x");
    (c, r)
}

fn key(app: &mut App, code: KeyCode) {
    app.key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn ctrl(app: &mut App, ch: char) {
    app.key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL));
}

fn typed(app: &mut App, text: &str) {
    text.chars().for_each(|ch| key(app, KeyCode::Char(ch)));
}

fn term(w: u16, h: u16) -> Terminal<TestBackend> {
    Terminal::new(TestBackend::new(w, h)).unwrap()
}

fn shot(t: &mut Terminal<TestBackend>, app: &App) -> String {
    t.draw(|f| app.draw(f)).unwrap();
    let b = t.backend().buffer();
    let rows = (0..b.area.height).map(|y| (0..b.area.width).map(|x| b[(x, y)].symbol()).collect::<String>());
    rows.collect::<Vec<_>>().join("\n")
}

fn screen(t: &mut Terminal<TestBackend>, app: &mut App) -> String {
    app.tick(Duration::ZERO).unwrap();
    shot(t, app)
}

fn until(t: &mut Terminal<TestBackend>, app: &mut App, done: impl Fn(&str) -> bool) -> String {
    for _ in 0..80 {
        app.tick(Duration::from_millis(100)).unwrap();
        let s = shot(t, app);
        if done(&s) {
            return s;
        }
    }
    panic!("never settled:\n{}", shot(t, app));
}

fn cursor(s: &str) -> &str {
    s.lines()
        .find(|l| l.contains('›') && !l.contains("‹"))
        .unwrap_or_default()
}

fn goto(t: &mut Terminal<TestBackend>, app: &mut App, needle: &str) -> String {
    for code in [KeyCode::Down, KeyCode::Up] {
        for _ in 0..120 {
            let s = screen(t, app);
            if cursor(&s).contains(needle) {
                return s;
            }
            key(app, code);
        }
    }
    panic!("no hot with {needle}:\n{}", shot(t, app));
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state)
        .unwrap()
        .to_vec()
        .into_iter()
        .filter(|e| e.kind == kind)
        .collect()
}

fn board<R>(c: &drover::Ctx, r: &OnShed, f: impl FnOnce(&mut App, &mut Terminal<TestBackend>) -> R) -> R {
    let mut c = c.clone();
    c.now = T;
    let door = Local::new(&c, r);
    std::thread::scope(|s| {
        let mut app = App::new(s, &c, r, &door, &Open::Board(None));
        let mut t = term(40, 18);
        until(&mut t, &mut app, |s| s.contains("approve"));
        f(&mut app, &mut t)
    })
}

fn taps(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            for k in ["sheet", "open"] {
                out.extend(m.get("tap").and_then(|t| t[k].as_str()).map(String::from));
            }
            m.values().for_each(|x| taps(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| taps(x, out)),
        _ => {}
    }
}

fn texts(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) if m.contains_key("since") || m.contains_key("until") => {}
        Value::Object(m) => {
            for k in [
                "title", "subtitle", "value", "badge", "header", "footer", "label", "markdown",
            ] {
                if m.contains_key("menu") && k == "label" || m.contains_key("label") && k == "value" {
                    continue;
                }
                match m.get(k) {
                    Some(Value::String(t)) => out.push(t.clone()),
                    Some(Value::Object(o)) if o.get("text").is_some_and(Value::is_string) => {
                        out.push(o["text"].as_str().unwrap().into());
                    }
                    Some(x) if x.is_number() => out.push(x.to_string()),
                    _ => {}
                }
            }
            m.iter()
                .filter(|(k, _)| !["swipe", "main", "confirm"].contains(&k.as_str()))
                .for_each(|(_, x)| texts(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| texts(x, out)),
        _ => {}
    }
}

fn bare(s: &str) -> String {
    s.chars().filter(|c| !"*`[]()".contains(*c)).collect()
}

fn draws_all(tree: &Value, what: &str) {
    let mut want = vec![];
    texts(tree, &mut want);
    for w in [40, 120] {
        let lines = drover_chat::text(tree, w, T);
        for l in &lines {
            assert!(l.chars().count() <= w, "{what} at {w}: cut: {l}");
        }
        let drawn = bare(&lines.join("\n"));
        for t in &want {
            for word in bare(t).split_whitespace() {
                assert!(
                    drawn.contains(word),
                    "{what} at {w}: {word:?} of {t:?} not drawn:\n{}",
                    lines.join("\n")
                );
            }
        }
    }
}

#[test]
fn every_screen_the_phone_reaches_draws_every_title_value_badge_and_label_at_40_and_120() {
    let (c, r) = recorded("board-every");
    let door = Local::new(&c, &r);
    let places = Local::places();
    let labels: Vec<&str> = places.iter().map(|p| p.label.as_str()).collect();
    assert!(labels.contains(&"Hosts") && labels.contains(&"Workflow"), "{labels:?}");
    assert!(!labels.contains(&"Herd") && !labels.contains(&"Threads"), "{labels:?}");
    let mut paths: Vec<String> = places.iter().map(|p| p.path.clone()).collect();
    paths.push(Local::cards());
    let mut i = 0;
    while let Some(p) = paths.get(i).cloned() {
        i += 1;
        let Some(tree) = door.screen(T, &p) else { continue };
        draws_all(&tree, &p);
        let mut more = vec![];
        taps(&tree, &mut more);
        more.retain(|m| !paths.contains(m));
        paths.extend(more);
    }
    for want in [
        "/screens/queue",
        "/screens/events",
        "/screens/lanes",
        "/screens/hosts",
        "/screens/fleets",
        "/screens/workflow",
        "/screens/cards",
        "/screens/issue?",
        "/screens/backlog?",
        "/screens/legend",
        "/screens/lanes/",
    ] {
        assert!(
            paths.iter().any(|p| p.starts_with(want)),
            "{want} not reached: {paths:?}"
        );
    }
    assert!(
        !paths.iter().any(|p| p.contains("herd") || p.contains("proxy-token")),
        "{paths:?}"
    );
    assert_eq!(door.screen(T, "/screens/herd"), None);
}

#[test]
fn a_block_or_screen_no_terminal_code_names_draws_its_text_and_an_unknown_type_never_panics() {
    let fresh = json!({"type": "screen", "id": "audit", "title": "Audit trail", "badge": {"text": "3 new", "tone": "warn"},
        "blocks": [{"type": "section", "id": "s1", "header": "this week", "footer": "kept 30 days", "blocks": [
            {"type": "list", "id": "l1", "rows": [{"type": "row", "id": "a1", "title": "Key rotated on mini", "value": "2h",
                "badge": "ok", "subtitle": {"text": "by the owner", "tone": "mute"}, "swipe": [{"label": "undo", "action": "undo"}]}]},
            {"type": "button", "id": "b1", "label": "Export the trail", "action": "export"}]}]});
    draws_all(&fresh, "fresh");
    let odd = json!({"type": "screen", "id": "x", "blocks": [{"type": "hologram", "id": "h", "title": "new", "meta": "beta",
        "blocks": [{"type": "row", "id": "r1", "title": "inside it"}], "items": [{"type": "sparkle", "label": "deep"}], "action": "go"},
        {"type": 7}, {"type": "row"}, "plain", null, {"no": "type"}]});
    for w in [1, 2, 40, 120] {
        let drawn = drover_chat::text(&odd, w, T).join(" ");
        if w >= 40 {
            for t in ["new", "beta", "inside it", "deep"] {
                assert!(drawn.contains(t), "{t}: {drawn}");
            }
        }
    }
}

#[test]
fn a_queue_row_moves_by_two_presses_and_esc_sends_nothing() {
    let (c, r) = recorded("board-move");
    board(&c, &r, |app, _| {
        let t = &mut term(60, 30);
        let before = kinds(&c, "queue").len();
        goto(t, app, "#21");
        let bar = screen(t, app);
        assert!(
            bar.contains(" 1 ▲"),
            "the row's swipe actions are its numbered bar: {bar}"
        );
        key(app, KeyCode::Char('1'));
        let panel = screen(t, app);
        assert!(
            panel.contains("sends move") && panel.contains("q-sendmeter-21"),
            "{panel}"
        );
        assert!(panel.contains("issue 21"), "the values sent: {panel}");
        assert_eq!(kinds(&c, "queue").len(), before, "one press sends nothing");
        key(app, KeyCode::Esc);
        assert!(!screen(t, app).contains("sends move"));
        key(app, KeyCode::Char('1'));
        key(app, KeyCode::Backspace);
        assert!(!screen(t, app).contains("sends move"), "⌫ backs out too");
        assert_eq!(kinds(&c, "queue").len(), before);
        key(app, KeyCode::Char('1'));
        key(app, KeyCode::Enter);
        until(t, app, |s| s.contains('✓'));
        assert_eq!(kinds(&c, "queue").len(), before + 1);
        assert_eq!(kinds(&c, "queue").last().unwrap().str("op"), Some("mv"));
    });
}

#[test]
fn an_ask_unfolds_on_enter_and_its_option_answers_by_two_presses() {
    let (c, r) = recorded("board-ask");
    board(&c, &r, |app, _| {
        let t = &mut term(60, 40);
        goto(t, app, "Move the branch");
        key(app, KeyCode::Enter);
        let open = screen(t, app);
        assert!(
            open.contains("[1] move ★ recommended") && open.contains("[3] other…"),
            "{open}"
        );
        key(app, KeyCode::Down);
        key(app, KeyCode::Char('3'));
        typed(app, "later");
        let panel = screen(t, app);
        assert!(panel.contains("› later▌") && panel.contains("sends answer"), "{panel}");
        key(app, KeyCode::Esc);
        key(app, KeyCode::Char('2'));
        key(app, KeyCode::Enter);
        until(t, app, |_| !kinds(&c, "decision").is_empty());
        let d = kinds(&c, "decision");
        assert_eq!((d[0].str("id"), d[0].str("choice")), (Some("q7"), Some("refuse")));
    });
}

#[test]
fn a_run_card_shows_what_runs_and_a_refusal_shows_its_code_and_nothing_ran() {
    let (c, r) = recorded("board-run");
    board(&c, &r, |app, _| {
        let mut t = term(60, 30);
        key(app, KeyCode::Char('1'));
        let panel = screen(&mut t, app);
        assert!(
            panel.contains("approve · run-card") && panel.contains("$ git push origin --delete wip/x"),
            "{panel}"
        );
        assert!(
            panel.contains("choice approve") && panel.contains("sha 3b7e0c55a1d9"),
            "{panel}"
        );
        assert!(panel.contains("↵ run"), "{panel}");
        key(app, KeyCode::Esc);
        key(app, KeyCode::Char('2'));
        let mut other = c.clone();
        other.now = T;
        let deny = json!({"screen": "cards", "block": "r1", "action": "run-card", "values": {"choice": "deny", "sha": "3b7e0c55a1d9"}});
        assert!(Local::new(&other, &r).act(T, &deny).is_ok());
        key(app, KeyCode::Enter);
        let refused = until(&mut t, app, |s| s.contains("REFUSED"));
        assert!(
            refused.contains("REFUSED ALREADY_SETTLED") && refused.contains("nothing ran"),
            "{refused}"
        );
        key(app, KeyCode::Enter);
        assert!(!screen(&mut t, app).contains("nothing ran"), "Enter closes the refusal");
    });
}

#[test]
fn a_queue_change_made_elsewhere_shows_within_two_seconds_with_no_key_press() {
    let (c, r) = recorded("board-live");
    board(&c, &r, |app, _| {
        let mut t = term(60, 80);
        until(&mut t, app, |s| s.contains("#21"));
        let t0 = std::time::Instant::now();
        assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "33"]).0, 0);
        until(&mut t, app, |s| s.contains("#33"));
        assert!(t0.elapsed() < Duration::from_secs(2), "{:?}", t0.elapsed());
    });
}

#[test]
fn backlog_search_types_in_place_ticks_flip_locally_and_done_queues_them() {
    let (c, r) = recorded("board-backlog");
    board(&c, &r, |app, _| {
        let mut t = term(60, 40);
        goto(&mut t, app, "backlog");
        key(app, KeyCode::Enter);
        until(&mut t, app, |s| s.contains("‹ sendmeter backlog"));
        goto(&mut t, app, "Search");
        key(app, KeyCode::Enter);
        typed(app, "pusX");
        key(app, KeyCode::Left);
        key(app, KeyCode::Backspace);
        key(app, KeyCode::Right);
        typed(app, "h");
        assert!(screen(&mut t, app).contains("Search: puXh▌"), "{}", shot(&mut t, app));
        key(app, KeyCode::Esc);
        assert!(!screen(&mut t, app).contains("pusX"), "Esc undoes the edit");
        key(app, KeyCode::Enter);
        typed(app, "zz");
        ctrl(app, 'u');
        typed(app, "push");
        key(app, KeyCode::Enter);
        let panel = screen(&mut t, app);
        assert!(
            panel.contains("sends queue") && panel.contains("sendmeter.q push"),
            "{panel}"
        );
        key(app, KeyCode::Enter);
        until(&mut t, app, |s| s.contains("Fix push") && !s.contains("Dark mode"));
        goto(&mut t, app, "Fix push");
        key(app, KeyCode::Enter);
        assert!(screen(&mut t, app).contains("[x] #41 Fix push"), "a tick flips locally");
        assert!(kinds(&c, "queue").iter().all(|e| e.val("issues") != &json!([41])));
        goto(&mut t, app, "[Queue ticked]");
        key(app, KeyCode::Enter);
        key(app, KeyCode::Enter);
        until(&mut t, app, |s| !s.contains("‹ sendmeter backlog"));
        assert_eq!(kinds(&c, "queue").last().unwrap().val("issues"), &json!([41]));
    });
}

#[test]
fn places_menu_sheets_tab_paging_and_quit_keys_do_what_the_design_lists() {
    let (c, r) = recorded("board-keys");
    board(&c, &r, |app, _| {
        let mut t = term(60, 30);
        key(app, KeyCode::Char('l'));
        until(&mut t, app, |s| s.lines().any(|l| l.trim() == "Events"));
        key(app, KeyCode::Right);
        until(&mut t, app, |s| s.contains("working (1)"));
        key(app, KeyCode::Char('h'));
        key(app, KeyCode::Left);
        until(&mut t, app, |s| s.contains("[Fleets…]"));
        key(app, KeyCode::Char('m'));
        let menu = screen(&mut t, app);
        assert!(
            menu.contains("‹ Menu") && menu.contains("Hosts") && menu.contains("Workflow"),
            "{menu}"
        );
        assert!(!menu.contains("Herd"), "Herd is absent, not dead: {menu}");
        goto(&mut t, app, "Hosts");
        key(app, KeyCode::Enter);
        let hosts = until(&mut t, app, |s| s.contains("‹ Hosts"));
        assert!(!hosts.contains("How to set"), "{hosts}");
        key(app, KeyCode::Esc);
        assert!(screen(&mut t, app).contains("‹ Menu"));
        key(app, KeyCode::Backspace);
        key(app, KeyCode::Char('?'));
        let keys = screen(&mut t, app);
        assert!(keys.contains("Ctrl-C") && keys.contains("‹ Keys"), "{keys}");
        key(app, KeyCode::Esc);
        let root = screen(&mut t, app);
        let at = cursor(&root).to_string();
        ctrl(app, 'f');
        assert_ne!(cursor(&screen(&mut t, app)), at, "Ctrl-F pages down");
        ctrl(app, 'b');
        assert_eq!(cursor(&screen(&mut t, app)), at, "Ctrl-B pages back up");
        key(app, KeyCode::Char('j'));
        key(app, KeyCode::Char('k'));
        assert_eq!(cursor(&screen(&mut t, app)), at);
        key(app, KeyCode::Down);
        let place = cursor(&screen(&mut t, app)).to_string();
        key(app, KeyCode::Tab);
        assert!(screen(&mut t, app).contains("no threads"), "Tab: Threads");
        key(app, KeyCode::Tab);
        assert_eq!(cursor(&screen(&mut t, app)), place, "Tab back keeps the place");
        goto(&mut t, app, "#12");
        key(app, KeyCode::Enter);
        until(&mut t, app, |s| s.contains("‹ #12"));
        key(app, KeyCode::Char('q'));
        assert!(!app.quit, "q quits on the root only");
        key(app, KeyCode::Esc);
        key(app, KeyCode::Char('q'));
        assert!(app.quit);
        app.quit = false;
        ctrl(app, 'c');
        assert!(app.quit);
    });
}

#[test]
fn it_wraps_at_40x18_splits_at_120x40_and_never_panics_on_a_tiny_or_resized_screen() {
    let (c, r) = recorded("board-sizes");
    board(&c, &r, |app, t| {
        let narrow = screen(t, app);
        assert!(
            narrow.lines().nth(1).unwrap().contains("Queue · Events · Lanes"),
            "{narrow}"
        );
        let mut wide = term(120, 40);
        let w = until(&mut wide, app, |s| {
            s.contains("asks and cards") && s.contains("[Fleets…]")
        });
        let row = w.lines().find(|l| l.contains("[Fleets…]")).unwrap();
        assert!(row.contains('│'), "cards beside the queue at 120: {w}");
        for (w, h) in [(30, 10), (12, 4), (1, 1), (39, 18), (200, 3), (40, 18)] {
            t.backend_mut().resize(w, h);
            let small = screen(t, app);
            assert_eq!(small.lines().count(), usize::from(h), "{small}");
        }
    });
}

#[test]
fn piped_board_prints_todays_table_and_the_hook_refuses_only_the_chat_in_a_lane() {
    let (c, _) = recorded("board-pipe");
    let table = run(&c, &Fake::default(), &["board"]);
    drover::APP.set(drover_chat::main).ok();
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        assert_eq!(
            run(&c, &Fake::default(), &["board"]),
            table,
            "stdout no terminal: the table"
        );
    }
    for (cmd, refused) in [
        ("drover chat t1", true),
        ("drover board", false),
        ("drover board sendmeter", false),
    ] {
        let input = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": cmd}});
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_drover"))
            .env_remove("DROVER_SCRATCH_TARGET")
            .env("DROVER_LANE", "sendmeter:12:impl")
            .env("FLEET_CONFIG", &c.config)
            .env("FLEET_STATE", &c.state)
            .args(["hook", "--role", "impl"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut child.stdin.take().unwrap(), input.to_string().as_bytes()).unwrap();
        let out = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
        assert_eq!(out.contains("REFUSED OWNER_ONLY"), refused, "{cmd}: {out}");
    }
}
