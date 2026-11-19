mod common;
use common::*;
use drover::events::{append, fold, read, Event};
use drover::serve::Server;
use serde_json::{json, Value};

const ADDR: &str = "127.0.0.1:7777";
const IMPL: &str = "sendmeter-impl-12";
const ADD: &str = "git -C /tmp/does-not-matter/sendmeter worktree add";

fn sha(p: &str) -> String {
    format!("{p}{}", "0".repeat(40 - p.len()))
}

fn ev(c: &drover::Ctx, ts: u64, kind: &str, d: Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
}

fn setup(name: &str) -> (drover::Ctx, FakeShed) {
    let c = ctx(&tmpdir(name), CONFIG);
    let sh = FakeShed::attached(&c);
    for args in [
        &["queue", "sendmeter", "add", "11", "12", "13"][..],
        &["queue", "sendmeter", "block", "--on", "99", "11"],
        &["queue", "sendmeter", "block", "--on", "99", "13"],
    ] {
        assert_eq!(run(&c, &Fake::default(), args).0, 0, "{args:?}");
    }
    let wt = c.worktrees.join("sendmeter/issue-12");
    let d = json!({"role": "impl", "agent": IMPL, "pane": IMPL, "worktree": wt, "branch": "issue-12"});
    ev(&c, 10, "spawn", d);
    for (ts, p) in [(11, "1"), (12, "2"), (13, "3")] {
        ev(
            &c,
            ts,
            "verdict",
            json!({"verdict": "FAIL", "sha": sha(p), "body": "FAIL"}),
        );
    }
    let nh = json!({"code": "ROUND_LIMIT", "reason": "out of rounds"});
    ev(&c, 14, "needs-human", nh);
    (c, sh)
}

fn fake(level: &str) -> Fake {
    fake_on(level, Fake::default())
}

fn fake_on(level: &str, f: Fake) -> Fake {
    let pr = json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": sha("3"),
        "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}]});
    f.reply("sysctl -n kern.memorystatus_vm_pressure_level", &format!("{level}\n"))
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view 12", r#"{"title":"t","body":"b"}"#)
        .reply("gh pr view issue-12", &pr.to_string())
        .once("git -C /tmp/does-not-matter/sendmeter rev-parse --verify", 1, "")
        .reply("git -C", "")
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("kill -0", "")
}

fn server<'a>(c: &'a drover::Ctx, r: &'a drover::shed::With<'a>) -> Server<'a> {
    Server::new(c, r, Some(ADDR.into()))
}

fn with<'a>(c: &drover::Ctx, r: &'a Fake) -> drover::shed::With<'a> {
    drover::shed::With(r, drover::config::Config::load(&c.config).unwrap().shed().unwrap())
}

fn get(c: &drover::Ctx, r: &Fake, path: &str, now: u64) -> Value {
    let w = with(c, r);
    serde_json::from_str(&api(&server(c, &w), ADDR, "", now, path)).unwrap()
}

fn screen(c: &drover::Ctx, r: &Fake, path: &str, now: u64) -> Value {
    let w = with(c, r);
    let req = format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\nContent-Length: 0\r\n\r\n");
    let res = String::from_utf8_lossy(&server(c, &w).respond(&req, now)).into_owned();
    serde_json::from_str(res.split_once("\r\n\r\n").unwrap().1).unwrap()
}

fn cards(c: &drover::Ctx, r: &Fake, now: u64) -> Vec<(u64, String)> {
    let n = get(c, r, "needs", now);
    let i = n["issues"].as_array().unwrap().iter();
    let api: Vec<(u64, String)> = i
        .map(|i| (i["issue"].as_u64().unwrap(), i["why"].as_str().unwrap().into()))
        .collect();
    let cooee = screen(c, r, "/screens/cards", now).to_string();
    assert_eq!(
        cooee.contains("\"nhc-sendmeter-12\""),
        api.iter().any(|a| a.0 == 12),
        "cooee's cards match /api/v1: {cooee}"
    );
    api
}

fn status(c: &drover::Ctx, r: &Fake, now: u64) -> Value {
    let s = get(c, r, "state", now);
    let q = s["queue"].as_array().unwrap().iter().find(|q| q["issue"] == 12);
    q.unwrap()["status"].clone()
}

fn find<'a>(v: &'a Value, id: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) if m.get("id") == Some(&json!(id)) => Some(v),
        Value::Object(m) => m.values().find_map(|x| find(x, id)),
        Value::Array(a) => a.iter().find_map(|x| find(x, id)),
        _ => None,
    }
}

fn subtitle(c: &drover::Ctx, r: &Fake, now: u64) -> String {
    let q = screen(c, r, "/screens/queue", now);
    let row = find(&q, "q-sendmeter-12").expect("a queue row");
    row["subtitle"].as_str().unwrap_or_default().to_string()
}

fn order(c: &drover::Ctx) -> Vec<(u64, String)> {
    let (code, out) = run(c, &Fake::default(), &["queue", "sendmeter", "ls", "--json"]);
    assert_eq!(code, 0, "{out}");
    let v: Value = serde_json::from_str(&out).unwrap();
    let rows = v.as_array().unwrap().iter();
    rows.map(|r| (r["issue"].as_u64().unwrap(), r["state"].as_str().unwrap().into()))
        .collect()
}

fn kinds(c: &drover::Ctx, kind: &str) -> usize {
    read(&c.state).unwrap().iter().filter(|e| e.kind == kind).count()
}

fn started(sh: &FakeShed) -> usize {
    sh.all("run").iter().filter(|p| p["name"] == IMPL).count()
}

fn state(c: &drover::Ctx) -> String {
    fold(&read(&c.state).unwrap())[&("sendmeter".to_string(), 12)]
        .state
        .clone()
}

fn queued(n: &[u64]) -> Vec<(u64, String)> {
    n.iter().map(|&n| (n, "queued".to_string())).collect()
}

#[test]
fn an_extend_requeues_at_its_old_place_spawns_nothing_and_drive_starts_it_in_turn() {
    let (c, sh) = setup("extend-requeue");
    let calm = fake("1");
    assert_eq!(cards(&c, &calm, c.now).len(), 1, "the card before the tap");
    let (code, out) = run(&c, &calm, &["extend", "sendmeter", "12"]);
    assert!(code == 0 && out.ends_with("#12: extend\n"), "{out}");
    assert!(sh.all("run").is_empty(), "the tap spawns nothing: {out}");
    assert_eq!((kinds(&c, "round-extended"), kinds(&c, "spawn")), (1, 1));
    assert_eq!(order(&c), queued(&[11, 12, 13]), "back in its old place");
    assert_eq!(cards(&c, &calm, c.now), []);
    assert_eq!(status(&c, &calm, c.now), "extended · starting");

    let (code, out) = run(&c, &calm, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(started(&sh), 1, "{out}");
    assert_eq!(state(&c), "fix(3)");
    assert_eq!(status(&c, &calm, c.now), Value::Null);
    assert_eq!(cards(&c, &calm, c.now), []);
}

#[test]
fn an_extend_out_of_the_queue_goes_last() {
    let (c, sh) = setup("extend-requeue-last");
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "rm", "12"]).0, 0);
    let (code, out) = run(&c, &fake("1"), &["extend", "sendmeter", "12"]);
    assert_eq!(code, 0, "{out}");
    assert!(sh.all("run").is_empty(), "{out}");
    assert_eq!(order(&c), queued(&[11, 13, 12]));
}

#[test]
fn a_paused_then_busy_extend_waits_hours_with_no_card_and_starts_after_resume_with_no_tap() {
    let (mut c, sh) = setup("extend-wait");
    let (busy, calm) = (fake("4"), fake("1"));
    assert_eq!(run(&c, &calm, &["pause", "sendmeter"]).0, 0);
    let (code, out) = run(&c, &calm, &["extend", "sendmeter", "12"]);
    assert_eq!(code, 0, "{out}");
    c.now += 3600;
    assert_eq!(run(&c, &calm, &["drive"]).0, 0);
    assert_eq!(started(&sh), 0);
    assert_eq!(state(&c), "queued");
    assert_eq!(cards(&c, &calm, c.now), [], "an hour paused: no card");
    assert_eq!(status(&c, &calm, c.now), "extended · waiting: PAUSED");
    assert!(
        subtitle(&c, &calm, c.now).contains("extended · waiting: PAUSED"),
        "{}",
        subtitle(&c, &calm, c.now)
    );

    assert_eq!(run(&c, &calm, &["resume", "sendmeter"]).0, 0);
    let (code, out) = run(&c, &busy, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(started(&sh), 0, "{out}");
    c.now += 3600;
    assert_eq!(run(&c, &busy, &["drive"]).0, 0);
    assert_eq!(cards(&c, &calm, c.now), [], "an hour on a busy host: no card");
    assert_eq!(status(&c, &calm, c.now), "extended · waiting: HOST_BUSY");
    assert!(subtitle(&c, &calm, c.now).contains("extended · waiting: HOST_BUSY"));

    let (code, out) = run(&c, &calm, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(started(&sh), 1, "{out}");
    assert_eq!(kinds(&c, "round-extended"), 1, "no second tap");
    assert_eq!(cards(&c, &calm, c.now), []);
}

#[test]
fn a_hard_refusal_of_the_start_brings_the_card_back_naming_the_code() {
    let (c, sh) = setup("extend-hard");
    let calm = fake("1");
    assert_eq!(run(&c, &calm, &["extend", "sendmeter", "12"]).0, 0);
    let broken = fake("1").once_err(ADD, 1, "worktree exists");
    let (code, out) = run(&c, &broken, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(state(&c), "needs-human");
    let back = cards(&c, &calm, c.now);
    assert_eq!(back.len(), 1, "{back:?}");
    assert!(
        back[0].1.starts_with("one more round did not start: ERROR: "),
        "{back:?}"
    );

    let (code, out) = run(&c, &calm, &["extend", "sendmeter", "12"]);
    assert_eq!(code, 0, "the returned card's tap: {out}");
    assert_eq!(kinds(&c, "round-extended"), 1, "the round was granted already");
    assert_eq!((state(&c), cards(&c, &calm, c.now)), ("queued".into(), vec![]));
    assert_eq!(run(&c, &calm, &["drive"]).0, 0);
    assert_eq!(started(&sh), 1);
}

#[test]
fn a_975_pending_extend_is_requeued_once_and_its_card_never_comes_back() {
    let (mut c, sh) = setup("extend-migrate");
    let calm = fake("1");
    ev(&c, 20, "round-extended", json!({"by": "owner", "spawn": true}));
    let d = json!({"action": "extend", "role": "impl", "since": 20, "after": null,
        "reason": "HOST_BUSY", "failed": false});
    ev(&c, 21, "pending-action", d);
    assert_eq!(state(&c), "fix(3)");
    assert_eq!(
        cards(&c, &calm, c.now + 7200),
        [],
        "no 30-min return, even before the migration"
    );
    assert_eq!(run(&c, &calm, &["pause", "sendmeter"]).0, 0);
    c.now += 7200;
    for _ in 0..2 {
        assert_eq!(run(&c, &calm, &["drive"]).0, 0);
    }
    assert_eq!(kinds(&c, "extend-requeued"), 1, "migrated once");
    assert_eq!(order(&c), queued(&[11, 12, 13]));
    let p = drover::pending::of(&read(&c.state).unwrap(), "sendmeter", 12);
    assert_eq!(p, None);
    assert_eq!(cards(&c, &calm, c.now), []);
    assert_eq!(status(&c, &calm, c.now), "extended · waiting: PAUSED");
    assert_eq!(started(&sh), 0);

    assert_eq!(run(&c, &calm, &["resume", "sendmeter"]).0, 0);
    assert_eq!(run(&c, &calm, &["drive"]).0, 0);
    assert_eq!(started(&sh), 1);
    assert_eq!(state(&c), "fix(3)");
}

#[test]
fn a_board_tap_logs_and_requeues_at_once_and_never_bounces_on_a_running_drive() {
    let (c, _sh) = setup("extend-board");
    let calm = fake("1");
    lease_as(&c, "drive-sendmeter.lock", 4242, c.now, 3600);
    let b = Board::start(&c, fake("1"));
    let mt = b.get("needs")["mtime"].as_str().unwrap().to_string();
    let (_, stale) = b.post("extend", "fleet=sendmeter&issue=12&mtime=1");
    assert_eq!(stale["code"], "STALE", "{stale}");
    assert_eq!(kinds(&c, "round-extended"), 0, "a stale page decides nothing");
    let (_, drive) = b.post("drive", &format!("fleet=sendmeter&mtime={mt}"));
    assert_eq!(drive["code"], "ACCEPTED", "{drive}");
    let (_, res) = b.post("extend", &format!("fleet=sendmeter&issue=12&mtime={mt}"));
    assert_eq!(
        (&res["code"], &res["message"]),
        (&json!("OK"), &json!("#12: extend")),
        "{res}"
    );
    assert!(res["job"].is_null(), "an extend is no job: {res}");
    assert_eq!(kinds(&c, "round-extended"), 1, "logged at the tap");
    let at: Vec<u64> = order(&c).into_iter().map(|q| q.0).collect();
    assert_eq!(at, [11, 12, 13]);
    assert_eq!(cards(&c, &calm, c.now), []);
    let (_, again) = b.post("extend", &format!("fleet=sendmeter&issue=12&mtime={mt}"));
    assert_eq!(again["code"], "NOT_NEEDS_HUMAN", "{again}");
    assert_eq!(kinds(&c, "round-extended"), 1, "a re-tap logs nothing");
}

fn leftover(name: &str) -> (drover::Ctx, FakeShed, String) {
    let (c, sh) = setup(name);
    assert_eq!(run(&c, &fake("1"), &["extend", "sendmeter", "12"]).0, 0);
    let wt = c.worktrees.join("sendmeter/issue-12");
    std::fs::create_dir_all(&wt).unwrap();
    let w = wt.display().to_string();
    sh.lane(IMPL, "working", &w);
    (c, sh, w)
}

fn secret(w: &str) -> Fake {
    let f = Fake::default().reply(&format!("git -C {w} rev-list"), "1\n").reply(
        &format!("git -C {w} log -p"),
        concat!("+TOKEN = \"gh", "p_abcdefghijklmnopqrstu1234\"\n"),
    );
    fake_on("1", f)
}

#[test]
fn an_extend_with_the_last_rounds_agent_still_live_ends_it_and_starts_the_new_lane_in_one_pass() {
    let (c, sh, _) = leftover("extend-leftover");
    let calm = fake("1");
    let (code, out) = run(&c, &calm, &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(kinds(&c, "supersede"), 1, "{out}");
    assert_eq!(started(&sh), 1, "{out}");
    assert_eq!(state(&c), "fix(3)", "{out}");
    assert_eq!(status(&c, &calm, c.now), Value::Null);
}

#[test]
fn a_refused_wip_save_shows_on_the_row_and_the_card_comes_once_after_the_stall_threshold() {
    let (mut c, sh, w) = leftover("extend-leftover-secret");
    let calm = fake("1");
    let (code, out) = run(&c, &secret(&w), &["drive"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(started(&sh), 0, "{out}");
    assert_eq!(state(&c), "queued", "{out}");
    assert_eq!(status(&c, &calm, c.now), "extended · waiting: SECRET_IN_WIP");
    assert_eq!(cards(&c, &calm, c.now), [], "no card before the stall threshold");

    c.now += 29 * 60;
    assert_eq!(run(&c, &secret(&w), &["drive"]).0, 0);
    assert_eq!(cards(&c, &calm, c.now), []);
    c.now += 60;
    assert_eq!(run(&c, &secret(&w), &["drive"]).0, 0);
    let back = cards(&c, &calm, c.now);
    assert_eq!(back.len(), 1, "{back:?}");
    assert!(
        back[0]
            .1
            .starts_with("one more round did not start in 30m: SECRET_IN_WIP: "),
        "{back:?}"
    );
    c.now += 3600;
    assert_eq!(run(&c, &secret(&w), &["drive"]).0, 0);
    assert_eq!(kinds(&c, "needs-human"), 2, "one card per extension");
    assert_eq!(started(&sh), 0);
}

#[test]
fn an_extend_never_tried_for_the_stall_threshold_raises_one_card() {
    let (mut c, sh) = setup("extend-never-tried");
    let calm = fake("1");
    assert_eq!(run(&c, &calm, &["extend", "sendmeter", "12"]).0, 0);
    let kick = |c: &drover::Ctx| {
        let t = std::fs::read_to_string(&c.config).unwrap();
        std::fs::write(
            &c.config,
            t.replace("max_parallel_spawns = 0", "max_parallel_spawns = 6"),
        )
        .unwrap();
    };
    kick(&c);
    assert_eq!(run(&c, &calm, &["drive"]).0, 0);
    assert_eq!(status(&c, &calm, c.now), "extended · starting");
    c.now += 30 * 60;
    assert_eq!(run(&c, &calm, &["drive"]).0, 0);
    let back = cards(&c, &calm, c.now);
    assert_eq!(back.len(), 1, "{back:?}");
    assert!(back[0].1.starts_with("one more round did not start in 30m"), "{back:?}");
    c.now += 3600;
    assert_eq!(run(&c, &calm, &["drive"]).0, 0);
    assert_eq!(kinds(&c, "needs-human"), 2, "one card per extension");
    assert_eq!(started(&sh), 0);
}
