use super::*;

const FLEET13: usize = 13;
const ISSUES: u64 = 308;
const THREADS: usize = 190;
const OPEN: usize = 20;

struct Offline(Fake, AtomicUsize, AtomicUsize);
impl Offline {
    fn down(&self, prog: &str, args: &[String]) -> Option<Out> {
        let to = prog == "ssh" && args.iter().any(|a| a == "bazzite");
        to.then(|| {
            self.1.fetch_add(1, Relaxed);
            if drover::runner::in_request() {
                self.2.fetch_add(1, Relaxed);
            }
            std::thread::sleep(Duration::from_millis(1_000));
            Out {
                code: 255,
                stderr: "ssh: connect to host bazzite port 22: Operation timed out".into(),
                ..Out::default()
            }
        })
    }
}
impl Runner for Offline {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.down(prog, args).map_or_else(|| self.0.run(prog, args), Ok)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.down(prog, args)
            .map_or_else(|| self.0.run_in(prog, args, input), Ok)
    }
    fn sleep(&self, _: u64) {}
}

fn fleet(i: usize) -> String {
    if i == 0 {
        "sendmeter".into()
    } else {
        format!("fleet{i:02}")
    }
}

fn live_fleet() -> (drover::Ctx, Offline, &'static FakeShed) {
    let mut cfg = CONFIG.to_string();
    for f in (1..FLEET13).map(fleet) {
        cfg += &format!(
            "\n[fleet.{f}]\nrepo = \"o/{f}\"\nlocal = \"/tmp/does-not-matter/{f}\"\nintegration = \"staging\"\n\
             [fleet.{f}.role.impl]\nharness = \"claude\"\n[fleet.{f}.role.rev]\nharness = \"claude\"\n"
        );
    }
    cfg += "\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n";
    let c = ctx(&tmpdir("perf-fleet"), &cfg);
    let sh = shed();
    sh.attach(&c);
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let seen = json!({"mini": {"home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
                      "bazzite": {"home": "/home/me", "probed": NOW - 7_200, "seen": NOW - 7_200, "up": false}});
    std::fs::write(st.join("hosts.json"), seen.to_string()).unwrap();
    let ev = |ts, f: &str, n: Option<u64>, kind, d| serde_json::to_string(&Event::new(ts, f, n, kind, d)).unwrap();
    let mut log = vec![];
    let line = "lorem ipsum dolor sit amet, consectetur adipiscing elit ".repeat(5);
    for i in 0..FLEET13 {
        let f = fleet(i);
        log.push(ev(
            NOW - 90_000,
            &f,
            None,
            "queue",
            json!({"op": "add", "issues": (1..=ISSUES).collect::<Vec<u64>>()}),
        ));
        for n in 1..=ISSUES {
            let (agent, t) = (format!("{f}-impl-{n}"), NOW - 80_000 + n * 60);
            let wt = format!("/wt/{agent}");
            let mut d = json!({"role": "impl", "agent": agent, "pane": agent, "runtime": "shed", "worktree": wt});
            let live = n > ISSUES - 3;
            if live && i % 3 == 0 {
                (d["host"], d["machine"]) = (json!("bazzite"), json!("bazzite"));
            } else if live {
                sh.lane(&agent, "idle", &wt);
                sh.screen(&agent, "⏺ Working on it\n\n❯ \n");
            }
            log.push(ev(t, &f, Some(n), "spawn", d));
            log.push(ev(t + 1, &f, Some(n), "pushed", json!({"sha": "a".repeat(40)})));
            log.push(ev(
                t + 2,
                &f,
                Some(n),
                "verdict",
                json!({"verdict": if n % 3 == 0 { "FAIL" } else { "PASS" }, "sha": "a".repeat(40)}),
            ));
            for k in 0..6 {
                let d = json!({"step": "review", "code": "EXIT", "result": "exit 1: boom"});
                log.push(ev(t + 3 + k, &f, Some(n), "driven", d));
            }
            if !live && n % 5 != 0 {
                log.push(ev(
                    t + 10,
                    &f,
                    Some(n),
                    "merged",
                    json!({"pr": n, "merge_sha": "m", "into": "staging"}),
                ));
            }
        }
    }
    for t in 0..THREADS {
        let (f, id) = (fleet(t % FLEET13), format!("t{t}"));
        let at = NOW - 70_000 + u64::try_from(t).unwrap() * 100;
        log.push(ev(
            at,
            &f,
            None,
            "thread-new",
            json!({"thread": id, "title": format!("thread {t}")}),
        ));
        let msgs = if t % 40 == 0 { 160 } else { 40 };
        for m in 0..msgs {
            let from = if m % 2 == 0 { "owner" } else { "orchestrator" };
            log.push(ev(
                at + 1 + m,
                &f,
                None,
                "thread-msg",
                json!({"thread": id, "from": from, "text": format!("{m}: {line}")}),
            ));
        }
        if t >= OPEN {
            log.push(ev(at + 99, &f, None, "thread-archive", json!({"thread": id})));
        }
    }
    std::fs::write(&c.state, log.join("\n") + "\n").unwrap();
    let f = Fake::default().reply("git -C", &format!("{} {}\n", "a".repeat(40), NOW - 600));
    (c, Offline(f, AtomicUsize::new(0), AtomicUsize::new(0)), sh)
}

fn timed(addr: std::net::SocketAddr, path: &str, cookie: &str) -> (u128, String) {
    let t = Instant::now();
    let res = send(addr, &format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n{cookie}\r\n"));
    (ms(t), res)
}

const ROUTES: [&str; 33] = [
    "/screens/queue",
    "/screens/lanes",
    "/screens/lanes/sendmeter-impl-308",
    "/screens/events",
    "/screens/fleets",
    "/screens/notify",
    "/screens/cards",
    "/screens/workflow",
    "/screens/drive-health",
    "/screens/scene",
    "/screens/hosts",
    "/screens/threads",
    "/screens/threads/t1",
    "/screens/backlog?repo=sendmeter",
    "/screens/queue/done?repo=sendmeter",
    "/screens/nope",
    "/api/v1/health",
    "/api/v1/state",
    "/api/v1/needs",
    "/api/v1/lanes",
    "/api/v1/herd",
    "/api/v1/chat",
    "/api/v1/thread/t1",
    "/api/v1/repo/sendmeter/threads",
    "/api/v1/queue",
    "/api/v1/host",
    "/api/v1/shed",
    "/api/v1/config",
    "/api/v1/roles?fleet=sendmeter",
    "/api/v1/events?since=0",
    "/api/v1/lessons",
    "/api/v1/issue/sendmeter/12",
    "/api/v1/nope",
];

#[test]
fn every_route_on_the_live_fleets_size_with_a_remote_host_offline() {
    let (c, r, sh) = live_fleet();
    let r: &'static Offline = Box::leak(Box::new(r));
    let (c, _, addr) = serve(c, sh.on(r));
    let cookie = pair_on(addr, c);
    let ssh = || r.2.load(Relaxed);
    for p in [
        "/live",
        "/screens/lanes/stream",
        "/screens/queue/stream",
        "/screens/threads/stream",
        "/screens/scene/stream",
        "/screens/cards/stream",
    ] {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        s.write_all(format!("GET {p} HTTP/1.1\r\nHost: {addr}\r\n{cookie}\r\n").as_bytes())
            .unwrap();
        std::thread::spawn(move || std::io::copy(&mut s, &mut std::io::sink()));
    }
    for p in ROUTES {
        timed(addr, p, &cookie);
    }
    let mut worst = vec![];
    let mut line = format!("#1622 perf ({FLEET13} fleets, {} issues, {THREADS} threads, {OPEN} open, bazzite offline, 6 streams held): median of 3, an event appended before each", FLEET13 as u64 * ISSUES);
    let before = ssh();
    for p in ROUTES {
        let mut t = vec![];
        let mut size = 0;
        for k in 0..3 {
            let d = json!({"step": "review", "code": "EXIT", "result": "exit 1: boom"});
            append(
                &c.state,
                &Event::new(drover::now() + k, "sendmeter", Some(1), "driven", d),
            )
            .unwrap();
            let (ms, res) = timed(addr, p, &cookie);
            size = res.len();
            t.push(ms);
        }
        t.sort_unstable();
        line += &format!("\n  {p} {} ms ({} KB)", t[1], size / 1024);
        worst.push((p, t[1], size));
    }
    report(&format!(
        "{line}\n  ssh to the offline host from a request: {}",
        ssh() - before
    ));
    let chat = worst.iter().find(|w| w.0 == "/api/v1/chat").unwrap();
    assert!(
        chat.2 < 100 * 1024,
        "/api/v1/chat is {} KB (budget 100 KB)",
        chat.2 / 1024
    );
    if cfg!(debug_assertions) {
        return eprintln!("debug build: budgets apply in release (cargo test --release --test perf)");
    }
    assert_eq!(ssh() - before, 0, "a request called ssh to the offline host");
    let slow: Vec<_> = worst.iter().filter(|w| w.1 >= 300).collect();
    assert!(slow.is_empty(), "routes at or over 300 ms: {slow:?}");
}
