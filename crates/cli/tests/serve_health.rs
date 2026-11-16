mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::runner::{in_request, Out, Runner};
use drover::serve::Server;
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const ADDR: &str = "127.0.0.1:7777";
const T: u64 = 1_000_000;

const READS: &[&str] = &[
    "/api/v1/state",
    "/api/v1/events?since=0",
    "/api/v1/host",
    "/api/v1/queue",
    "/api/v1/needs",
    "/api/v1/lanes",
    "/api/v1/shed",
    "/api/v1/chat",
    "/api/v1/repo/sendmeter/threads",
    "/api/v1/lane/sendmeter-impl-12",
    "/api/v1/herd",
    "/api/v1/roles?fleet=sendmeter",
    "/api/v1/issue/sendmeter/12",
    "/api/v1/lessons",
    "/api/v1/lessons/sendmeter/digest?days=7",
    "/api/v1/health",
    "/screens/queue",
    "/screens/lanes",
    "/screens/events",
    "/screens/fleets",
    "/screens/cards",
];

struct Guard(OnShed, Mutex<Vec<String>>);

impl Runner for Guard {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.seen(prog, args);
        self.0.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.seen(prog, args);
        self.0.run_in(prog, args, input)
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
    fn shed(&self) -> Option<drover::shed::Shed> {
        self.0.shed()
    }
}

impl Guard {
    fn seen(&self, prog: &str, args: &[String]) {
        if in_request() && ["git", "gh", "ssh"].contains(&prog) {
            self.1.lock().unwrap().push(format!("{prog} {}", args.join(" ")));
        }
    }
}

fn board(name: &str) -> (drover::Ctx, Guard) {
    let (c, r) = v1_board_on(name, Fake::default().reply("gh", "").reply("git", ""));
    let cfg = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(&c.config, cfg.replace("[limits]\n", "[limits]\ndb_busy_secs = 1\n")).unwrap();
    (c, Guard(r, Mutex::default()))
}

fn get(s: &Server, cookie: &str, path: &str, now: u64) -> String {
    let req = format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n{cookie}\r\n");
    String::from_utf8_lossy(&s.respond(&req, now)).into_owned()
}

fn answers(s: &Server, c: &drover::Ctx) -> Vec<String> {
    let cookie = pair(s, c, "Mozilla/5.0 (iPhone)", T);
    s.refresh(T);
    let db = rusqlite::Connection::open(drover::db::file(&c.state)).unwrap();
    db.execute_batch("BEGIN IMMEDIATE; INSERT INTO leases VALUES ('drive', 1, 'h', 0, 0, '')")
        .unwrap();
    let bad = (READS.iter())
        .map(|p| (p, get(s, &cookie, p, T)))
        .filter(|(_, r)| !r.starts_with("HTTP/1.1 200"))
        .map(|(p, r)| format!("{p}: {}", r.lines().next().unwrap_or_default()))
        .collect();
    db.execute_batch("COMMIT").unwrap();
    bad
}

#[test]
fn every_read_answers_with_the_db_write_locked_and_runs_no_git_gh_or_ssh() {
    let (c, r) = board("guard-ok");
    let s = Server::new(&c, &r, Some(ADDR.into())).deferring();
    assert_eq!(answers(&s, &c), Vec::<String>::new(), "every route answers 200");
    assert_eq!(*r.1.lock().unwrap(), Vec::<String>::new(), "no git/gh/ssh on a request");
}

#[test]
fn a_read_handler_that_calls_gh_fails_the_request_path_guard() {
    let (c, r) = board("guard-planted");
    let s = Server::new(&c, &r, Some(ADDR.into()));
    answers(&s, &c);
    drover::runner::request(|| drop(r.run("gh", &["pr".into(), "view".into()])));
    let calls = r.1.lock().unwrap().clone();
    for prog in ["git ", "ssh ", "gh pr view"] {
        assert!(calls.iter().any(|c| c.starts_with(prog)), "{prog}: {calls:?}");
    }
}

#[test]
fn each_route_groups_p95_stays_within_50ms_on_the_fixture_state() {
    let (c, r) = board("bench");
    let s = Server::new(&c, &r, Some(ADDR.into())).deferring();
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", T);
    s.refresh(T);
    let mut screens = vec![];
    for _ in 0..10 {
        for p in READS {
            let t = Instant::now();
            assert!(get(&s, &cookie, p, T).starts_with("HTTP/1.1 200"), "{p}");
            if p.starts_with("/screens/") {
                screens.push(t.elapsed());
            }
        }
    }
    let h = get(&s, &cookie, "/api/v1/health", T);
    let h: Value = serde_json::from_str(h.split_once("\r\n\r\n").unwrap().1).unwrap();
    let groups = h["serve"]["groups"].as_array().unwrap();
    let api = groups
        .iter()
        .find(|g| g["route"] == "/api/v1 reads")
        .unwrap_or_else(|| panic!("{h}"));
    eprintln!("/api/v1 reads: {api}");
    assert!(api["count"].as_u64().unwrap() >= 150, "{api}");
    assert!(api["p95"].as_u64().unwrap() <= 50, "/api/v1 reads over budget: {api}");
    screens.sort();
    let p95 = screens[(screens.len() * 95).div_ceil(100) - 1];
    eprintln!("/screens p95 {p95:?}");
    assert!(p95 <= Duration::from_millis(50), "/screens over budget: {p95:?}");
}

struct Slow(Fake);
impl Runner for Slow {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        if in_request() && self.0.calls.borrow().is_empty() {
            std::thread::sleep(Duration::from_secs(3));
        }
        self.0.run(prog, args)
    }
    fn sleep(&self, _: u64) {}
}

fn alerts(c: &drover::Ctx, prefix: &str) -> Vec<String> {
    let ev = read(&c.state).unwrap().to_vec().into_iter();
    ev.filter(|e| e.kind.starts_with(prefix)).map(|e| e.kind).collect()
}

fn at(c: &drover::Ctx, now: u64) -> drover::Ctx {
    drover::Ctx { now, ..c.clone() }
}

#[test]
fn a_3s_read_for_5_minutes_alerts_serve_slow_once_in_that_state() {
    let c = ctx(&tmpdir("serve-slow"), CONFIG);
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let r = Slow(Fake::default());
    let s = Server::new(&c, &r, Some(ADDR.into()));
    let f = Fake::default();
    for m in 0..5 {
        r.0.calls.borrow_mut().clear();
        assert!(get(&s, "", "/api/v1/state", T + 60 * m).starts_with("HTTP/1.1 200"));
        s.slo(T + 60 * m);
        assert_eq!(run(&at(&c, T + 60 * m), &f, &["check"]).0, 0);
        let want: &[&str] = if m < 4 { &[] } else { &["serve-slow"] };
        assert_eq!(alerts(&c, "serve-slow"), want, "minute {}", m + 1);
    }
    let h: Value =
        serde_json::from_slice(&std::fs::read(c.state.with_file_name("serve-health.json")).unwrap()).unwrap();
    let api = &h["groups"][0];
    assert_eq!(
        (&api["route"], api["max"].as_u64() >= Some(3000)),
        (&json!("/api/v1 reads"), true),
        "{h}"
    );
    assert_eq!(run(&at(&c, T + 260), &f, &["check"]).0, 0);
    assert_eq!(
        alerts(&c, "serve-slow"),
        ["serve-slow"],
        "the same state: no second alert"
    );
    let ev = read(&c.state).unwrap().to_vec();
    let e = ev.iter().find(|e| e.kind == "serve-slow").unwrap();
    assert!(e.str("reason").unwrap().contains("/api/v1/state p95 "), "{e:?}");
}

#[test]
fn a_fourth_cold_serve_start_in_a_day_alerts_serve_restarts_once() {
    let c = ctx(&tmpdir("serve-restarts"), CONFIG);
    append(&c.state, &Event::new(T, "sendmeter", None, "note", json!({}))).unwrap();
    let starts = c.state.with_file_name("serve-starts");
    let f = Fake::default();
    for n in 1..=4 {
        let (c2, l) = (c.clone(), std::net::TcpListener::bind("127.0.0.1:0").unwrap());
        std::thread::spawn(move || drover::serve::serve_on(&c2, &OnShed::new(Fake::default()), l, false));
        let end = Instant::now() + Duration::from_secs(30);
        while std::fs::read_to_string(&starts).unwrap_or_default().lines().count() < n {
            assert!(Instant::now() < end, "start {n} never logged");
            std::thread::yield_now();
        }
        assert_eq!(run(&c, &f, &["check"]).0, 0);
        let want: &[&str] = if n < 4 { &[] } else { &["serve-restarts"] };
        assert_eq!(alerts(&c, "serve-restarts"), want, "after start {n}");
    }
    assert_eq!(run(&c, &f, &["check"]).0, 0);
    assert_eq!(alerts(&c, "serve-restarts"), ["serve-restarts"]);
    assert_eq!(run(&at(&c, T + 86_401), &f, &["check"]).0, 0);
    assert_eq!(
        alerts(&c, "serve-restarts"),
        ["serve-restarts", "serve-restarts-ok"],
        "a day on"
    );
}

#[test]
fn a_fleets_pass_over_2_minutes_alerts_drive_slow_naming_its_slowest_step() {
    let c = ctx(&tmpdir("drive-slow"), CONFIG);
    let pass = |secs: f64| json!({"secs": secs, "step": "spawns", "step_secs": secs - 1.0});
    let f = Fake::default();
    for (t, secs, want) in [
        (T, 30.0, vec![]),
        (T + 300, 150.0, vec!["drive-slow"]),
        (T + 600, 160.0, vec!["drive-slow"]),
    ] {
        append(&c.state, &Event::new(t, "sendmeter", None, "drive-pass", pass(secs))).unwrap();
        assert_eq!(run(&at(&c, t), &f, &["check"]).0, 0);
        assert_eq!(alerts(&c, "drive-slow"), want);
    }
    let ev = read(&c.state).unwrap().to_vec();
    let e = ev.iter().find(|e| e.kind == "drive-slow").unwrap();
    assert_eq!(
        (e.fleet.as_str(), e.str("reason")),
        (
            "sendmeter",
            Some("sendmeter's pass took 150 s; slowest step: spawns (149.0 s)")
        )
    );
}

#[test]
fn a_serve_on_the_port_older_than_the_installed_binary_alerts_serve_stale_once() {
    let c = ctx(&tmpdir("serve-stale"), CONFIG);
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let (f, health) = (Fake::default(), c.state.with_file_name("serve-health.json"));
    let s = Server::new(&c, &f, Some(ADDR.into()));
    s.slo(T);
    assert_eq!(run(&at(&c, T), &f, &["check"]).0, 0);
    let mut h: Value = serde_json::from_slice(&std::fs::read(&health).unwrap()).unwrap();
    assert_eq!(
        (h["sha"].as_str(), alerts(&c, "serve-stale")),
        (Some(drover::SHA), vec![])
    );
    h["sha"] = json!("0ld0ld0ld");
    std::fs::write(&health, h.to_string()).unwrap();
    for t in [T + 60, T + 120] {
        assert_eq!(run(&at(&c, t), &f, &["check"]).0, 0);
        assert_eq!(alerts(&c, "serve-stale"), ["serve-stale"], "once at {t}");
    }
    let ev = read(&c.state).unwrap().to_vec();
    let why = ev
        .iter()
        .find(|e| e.kind == "serve-stale")
        .unwrap()
        .str("reason")
        .unwrap()
        .to_string();
    assert!(why.contains("runs 0ld0ld0, not the installed"), "{why}");
    s.slo(T + 180);
    assert_eq!(run(&at(&c, T + 180), &f, &["check"]).0, 0);
    assert_eq!(alerts(&c, "serve-stale"), ["serve-stale", "serve-stale-ok"]);
}

#[test]
fn a_serve_that_cannot_take_its_port_logs_each_failure_and_a_fourth_in_an_hour_alerts_serve_loop() {
    let c = ctx(&tmpdir("serve-loop"), &format!("{CONFIG}serve_handover_secs = 1\n"));
    append(&c.state, &Event::new(T, "sendmeter", None, "note", json!({}))).unwrap();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    std::thread::spawn(move || l.incoming().for_each(drop));
    let f = Fake::default();
    for n in 1..=4 {
        let e = drover::hooks::serve(&c, &f, &addr, false).unwrap_err();
        assert!(format!("{e:#}").contains("in use"), "{e:#}");
        assert_eq!(run(&c, &f, &["check"]).0, 0);
        let want: &[&str] = if n < 4 { &[] } else { &["serve-loop"] };
        assert_eq!(alerts(&c, "serve-loop"), want, "after failure {n}");
    }
    assert_eq!(run(&at(&c, T + 3_601), &f, &["check"]).0, 0);
    assert_eq!(alerts(&c, "serve-loop"), ["serve-loop", "serve-loop-ok"], "an hour on");
}
