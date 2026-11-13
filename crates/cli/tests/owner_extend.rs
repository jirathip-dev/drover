mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::runner::{Out, Runner};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};

fn sha(p: &str) -> String {
    format!("{p}{}", "0".repeat(40 - p.len()))
}

fn ev(c: &drover::Ctx, n: u64, ts: u64, kind: &str, d: Value) {
    append(&c.state, &Event::new(ts, "sendmeter", Some(n), kind, d)).unwrap();
}

fn setup(name: &str, extends: u64) -> (drover::Ctx, FakeShed) {
    let c = ctx(
        &tmpdir(name),
        &CONFIG.replace("max_parallel_spawns = 0", "max_parallel_spawns = 4"),
    );
    let sh = FakeShed::attached(&c);
    let mut add = vec!["queue".to_string(), "sendmeter".into(), "add".into()];
    add.extend((1..=extends).map(|n| n.to_string()));
    let add: Vec<&str> = add.iter().map(String::as_str).collect();
    assert_eq!(run(&c, &Fake::default(), &add).0, 0);
    for n in 31..=34 {
        let e = json!({"ts": 1, "fleet": "sendmeter", "issue": n, "type": "spawn", "role": "impl", "pid": 4242});
        let key = format!("spawn-sendmeter-{n}.pending");
        let l = drover::db::take(&c.state, &key, c.now - 10, 300, &e.to_string(), &|_| true).unwrap();
        assert!(drover::db::renew(
            &c.state,
            &l,
            &drover::db::Lease { pid: 4242, ..l.clone() }
        ));
    }
    for n in 1..=extends {
        out_of_rounds(&c, n);
    }
    (c, sh)
}

fn out_of_rounds(c: &drover::Ctx, n: u64) {
    let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
    std::fs::create_dir_all(&wt).unwrap();
    let d = json!({"role": "impl", "agent": format!("sendmeter-impl-{n}"), "pane": "w9:x", "workspace": "w9",
        "worktree": wt, "branch": format!("issue-{n}")});
    ev(c, n, 10, "spawn", d);
    for (ts, p) in [(11, "1"), (12, "2"), (13, "3")] {
        ev(
            c,
            n,
            ts,
            "verdict",
            json!({"verdict": "FAIL", "sha": sha(p), "body": "FAIL"}),
        );
    }
    let nh = json!({"code": "ROUND_LIMIT", "reason": "out of rounds"});
    ev(c, n, 14, "needs-human", nh);
}

fn loaded() -> Fake {
    let pr = json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": sha("3"),
        "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}]});
    Fake::default()
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 40.00 40.00 40.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("gh pr view", &pr.to_string())
        .reply("git -C", "")
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("kill -0 4242", "")
        .reply("ps -o etime= -p 4242", "  00:10\n")
}

fn of_kind(c: &drover::Ctx, kind: &str) -> Vec<Event> {
    read(&c.state).unwrap().into_iter().filter(|e| e.kind == kind).collect()
}

fn pending(c: &drover::Ctx, n: u64) -> Option<drover::pending::Pending> {
    drover::pending::of(&read(&c.state).unwrap(), "sendmeter", n)
}

#[test]
fn ten_extends_each_spawn_at_once_past_load_four_pending_spawns_and_a_running_pass() {
    let (c, sh) = setup("owner-extend-ten", 10);
    lease_as(&c, "drive-sendmeter.lock", 4242, c.now, 3600);
    let r = loaded();
    for n in 1..=10 {
        let (code, out) = run(&c, &r, &["extend", "sendmeter", &n.to_string()]);
        assert!(code == 0 && out.ends_with(&format!("#{n}: extend\n")), "#{n}: {out}");
        assert!(sh.called("run", &format!("sendmeter-impl-{n}")), "#{n}: {out}");
        assert_eq!(pending(&c, n), None, "#{n} started, nothing pending");
    }
    let o = of_kind(&c, "override");
    assert_eq!(o.len(), 10, "{o:?}");
    for e in &o {
        assert_eq!(
            (e.str("code"), e.str("why")),
            (Some("HOST_BUSY"), Some("owner-extend")),
            "{e:?}"
        );
        assert!(e.str("reason").unwrap().starts_with("load/cpu 4.00 > 2.5"), "{e:?}");
    }
    assert!(
        lease_on(&c, "drive-sendmeter.lock").is_some(),
        "the running pass's lock is left alone"
    );
    assert!((31..=34).all(|n| lease_on(&c, &format!("spawn-sendmeter-{n}.pending")).is_some()));
}

#[test]
fn low_memory_still_holds_an_owner_extend_and_says_so() {
    let (c, sh) = setup("owner-extend-mem", 1);
    let vm = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free: 16384.\n\
        Pages inactive: 16384.\nSwapouts: 100.\n";
    let low = Fake::default().reply("vm_stat", vm);
    let low = Fake {
        replies: [low.replies, loaded().replies].concat(),
        ..Default::default()
    };
    let (code, out) = run(&c, &low, &["extend", "sendmeter", "1"]);
    assert!(code == 0 && out.contains("waiting for host (MEM_LOW)"), "{out}");
    assert!(sh.all("run").is_empty(), "{out}");
    assert_eq!(pending(&c, 1).unwrap().reason.as_deref(), Some("MEM_LOW"));
    assert!(of_kind(&c, "override").is_empty());
}

#[test]
fn extends_already_pending_start_on_the_first_pass_past_load_and_full_spawn_slots() {
    let (c, sh) = setup("owner-extend-pass", 10);
    for n in 1..=10 {
        ev(&c, n, 20, "round-extended", json!({"by": "owner", "spawn": true}));
        let d = json!({"action": "extend", "role": "impl", "since": 20, "after": null,
            "reason": drover::pending::LOAD, "failed": false});
        ev(&c, n, 21, "pending-action", d);
    }
    assert_eq!(run(&c, &Fake::default(), &["queue", "sendmeter", "add", "21"]).0, 0);
    out_of_rounds(&c, 21);
    ev(
        &c,
        21,
        15,
        "verdict",
        json!({"verdict": "FAIL", "sha": sha("3"), "body": "FAIL"}),
    );
    let r = loaded();
    let (code, out) = run(&c, &r, &["drive"]);
    assert_eq!(code, 0, "{out}");
    let kick = |n: u64| format!("sh -c nohup '/opt/drover/bin/drover' kick sendmeter {n} --why drive --now");
    assert!((1..=10).all(|n| r.count(&kick(n)) == 1), "{:?}", r.calls.borrow());
    let pass = of_kind(&c, "drive-pass").pop().unwrap();
    let acts = pass.data["actions"].as_array().unwrap();
    let code = |n: u64| acts.iter().find(|a| a["issue"] == n).map(|a| a["code"].clone());
    assert!((1..=10).all(|n| code(n) == Some(Value::Null)), "{acts:?}");
    assert_eq!(code(21), Some(json!("SPAWN_PENDING")), "{acts:?}");
    assert_eq!(r.count(&kick(21)), 0);

    let (code, out) = run(&c, &r, &["kick", "sendmeter", "1", "--why", "drive", "--now"]);
    assert_eq!(code, 0, "{out}");
    assert!(sh.called("run", "sendmeter-impl-1"), "{out}");
    assert_eq!(pending(&c, 1), None);
    let o = of_kind(&c, "override");
    assert_eq!(o.len(), 1, "{o:?}");
    assert_eq!(o[0].str("why"), Some("owner-extend"));
}

struct Slow(Fake, AtomicBool);
impl Runner for Slow {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        let add = prog == "git" && args.windows(2).any(|w| w == ["worktree", "add"]);
        if add && !self.1.swap(true, Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(1500));
        }
        self.0.run(prog, args)
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.0.run_in(prog, args, input)
    }
}

fn http(addr: &str, req: &str) -> String {
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.write_all(req.as_bytes()).unwrap();
    let mut res = String::new();
    s.read_to_string(&mut res).unwrap();
    res
}

#[test]
fn board_extends_are_never_refused_while_another_extend_or_a_drive_runs() {
    let (c, _sh) = setup("owner-extend-board", 2);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let fake = Fake::default();
    let s = drover::serve::Server::new(&c, &fake, Some("127.0.0.1:7777".into()));
    let cookie = pair(&s, &c, "Mozilla/5.0 (iPhone)", now);
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let c2 = c.clone();
    std::thread::spawn(move || drover::serve::serve_on(&c2, &Slow(loaded(), AtomicBool::new(false)), l, false));
    let post = |op: &str, body: &str| {
        let req = format!(
            "POST /api/v1/{op} HTTP/1.1\r\nHost: {addr}\r\n{cookie}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        http(&addr, &req)
    };
    let get = |path: &str| {
        let res = http(
            &addr,
            &format!("GET /api/v1/{path} HTTP/1.1\r\nHost: {addr}\r\n{cookie}Connection: close\r\n\r\n"),
        );
        serde_json::from_str::<Value>(res.split_once("\r\n\r\n").unwrap().1).unwrap()
    };
    let toasts = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let (t2, a2, k2) = (toasts.clone(), addr.clone(), cookie.clone());
    std::thread::spawn(move || {
        let mut s = std::net::TcpStream::connect(&a2).unwrap();
        s.write_all(format!("GET /live HTTP/1.1\r\nHost: {a2}\r\n{k2}\r\n").as_bytes())
            .unwrap();
        let mut buf = [0u8; 4096];
        while let Ok(n) = s.read(&mut buf) {
            if n == 0 {
                break;
            }
            t2.lock().unwrap().push_str(&String::from_utf8_lossy(&buf[..n]));
        }
    });
    std::thread::sleep(std::time::Duration::from_millis(300));
    let mt = get("needs")["mtime"].as_str().unwrap().to_string();
    let stale = post("extend", "fleet=sendmeter&issue=2&mtime=1");
    assert!(stale.contains("STALE"), "{stale}");
    assert_eq!(of_kind(&c, "round-extended").len(), 0, "a stale page decides nothing");
    let first = post("extend", &format!("fleet=sendmeter&issue=1&mtime={mt}"));
    assert!(first.contains("extend started for sendmeter#1"), "{first}");
    std::thread::sleep(std::time::Duration::from_millis(300));
    let drive = post("drive", &format!("fleet=sendmeter&mtime={mt}"));
    assert!(drive.contains("drive started for sendmeter"), "{drive}");
    let second = post("extend", &format!("fleet=sendmeter&issue=2&mtime={mt}"));
    assert!(second.contains("extend started for sendmeter#2"), "{second}");
    let needs = get("needs");
    let cards: Vec<&Value> = needs["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| &i["issue"])
        .collect();
    assert!(!cards.contains(&&json!(2)), "#2 left needs-human at the tap: {needs}");
    let started = || {
        ["#1: extend", "#2: extend"]
            .iter()
            .all(|t| toasts.lock().unwrap().contains(t))
            && (1..=2).all(|n| {
                read(&c.state)
                    .unwrap()
                    .iter()
                    .any(|e| e.is("spawn", "sendmeter", n) && e.ts > 10)
            })
    };
    let t0 = std::time::Instant::now();
    while !started() && t0.elapsed().as_secs() < 30 {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let tail: Vec<String> = read(&c.state)
        .unwrap()
        .iter()
        .filter(|e| e.ts > 14)
        .map(|e| format!("{e:?}"))
        .collect();
    assert!(
        started(),
        "both extends toasted and spawned: {tail:#?} {}",
        toasts.lock().unwrap()
    );
    assert_eq!(of_kind(&c, "round-extended").len(), 2);
}
