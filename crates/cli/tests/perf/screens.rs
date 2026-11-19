use super::*;

struct Live(Fake);
impl Runner for Live {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        std::thread::sleep(Duration::from_millis(if prog == "lsof" { 1_000 } else { 5 }));
        self.0.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        std::thread::sleep(Duration::from_millis(5));
        self.0.run_in(prog, args, input)
    }
    fn sleep(&self, _: u64) {}
}

fn live_shaped() -> (drover::Ctx, Fake, &'static FakeShed) {
    let cfg = format!("{CONFIG}\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n");
    let c = ctx(&tmpdir("perf-live"), &cfg);
    let sh = shed();
    sh.attach(&c);
    let ev = |ts, n: Option<u64>, kind, d| serde_json::to_string(&Event::new(ts, "sendmeter", n, kind, d)).unwrap();
    let mut log = vec![ev(
        NOW - 9_900,
        None,
        "queue",
        json!({"op": "add", "issues": (1..=150).collect::<Vec<u64>>()}),
    )];
    for t in 1..=3 {
        let id = format!("sendmeter-t{t}");
        log.push(ev(NOW - 9_800, None, "thread-new", json!({"thread": id, "title": id})));
        for m in 0..20 {
            let from = if m % 2 == 0 { "owner" } else { "orchestrator" };
            log.push(ev(
                NOW - 9_700 + m,
                None,
                "thread-msg",
                json!({"thread": id, "from": from, "text": "hi"}),
            ));
        }
    }
    for n in 1..=60u64 {
        let (agent, t) = (format!("sendmeter-impl-{n}"), NOW - 9_000 + n * 10);
        let d = json!({"role": "impl", "agent": agent, "pane": agent, "runtime": "shed", "worktree": format!("/wt/{agent}")});
        log.push(ev(t, Some(n), "spawn", d));
        log.push(ev(t + 1, Some(n), "pushed", json!({"sha": "a".repeat(40)})));
        if n > 10 {
            log.push(ev(
                t + 2,
                Some(n),
                "verdict",
                json!({"verdict": "FAIL", "sha": "a".repeat(40)}),
            ));
        }
        if n > 40 {
            log.push(ev(
                t + 3,
                Some(n),
                "merged",
                json!({"pr": n, "merge_sha": "m", "into": "staging"}),
            ));
        }
        if n <= 10 {
            sh.lane(&agent, "idle", &format!("/wt/{agent}"));
            sh.screen(&agent, "⏺ Working on it\n\n❯ \n");
        }
    }
    for i in 0..3_000 - log.len() as u64 {
        let d = json!({"step": "review", "code": "EXIT", "result": "exit 1: boom"});
        log.push(ev(NOW - 5_000 + i, Some(1 + i % 150), "driven", d));
    }
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    std::fs::write(&c.state, log.join("\n") + "\n").unwrap();
    let cache = c.state.with_file_name("cache");
    std::fs::create_dir_all(&cache).unwrap();
    let put = |kind: &str, n: u64, v: serde_json::Value| {
        std::fs::write(cache.join(format!("{kind}-sendmeter-{n}.json")), v.to_string()).unwrap();
    };
    let text = |n: usize| "lorem ipsum dolor sit amet ".repeat(n / 27);
    let checks: Vec<_> = (0..40)
        .map(|i| json!({"name": format!("check-{i}"), "status": "COMPLETED", "conclusion": "SUCCESS", "detailsUrl": text(200)}))
        .collect();
    let mut backlog = serde_json::Map::new();
    for n in 1..=1_250u64 {
        if !(61..=150).contains(&n) {
            let comments: Vec<_> = (0..20)
                .map(|i| json!({"author": {"login": "o"}, "body": text(4_300), "id": i}))
                .collect();
            put(
                "issue",
                n,
                json!({"title": format!("issue {n}"), "body": text(20_000), "updatedAt": "2026-11-01", "comments": comments, "url": "u"}),
            );
        }
        let pr = json!({"number": n, "title": format!("PR for {n}"), "state": "OPEN", "headRefOid": "a".repeat(40), "statusCheckRollup": checks});
        put("pr", n, json!({"ts": NOW, "pr": pr}));
        put("run", n, json!({"url": format!("https://ci/{n}")}));
        put(
            "diff",
            n,
            json!({"head": "a".repeat(40), "files": (0..300).map(|i| format!("crates/x/src/f{i}.rs")).collect::<Vec<_>>()}),
        );
        backlog.insert(n.to_string(), json!({"number": n, "title": format!("issue {n}: {}", text(80)), "state": "OPEN", "labels": ["bug"], "ac": true, "pr": false}));
    }
    for n in 1_251..=3_000u64 {
        backlog.insert(n.to_string(), json!({"number": n, "title": format!("issue {n}: {}", text(80)), "state": "OPEN", "labels": ["bug"], "ac": true, "pr": false}));
    }
    put(
        "backlog",
        0,
        json!({"issues": backlog, "since": "2026-11-01", "ts": NOW}),
    );
    let f = Fake::default().reply("git -C", &format!("{} {}\n", "a".repeat(40), NOW - 600));
    (c, f, sh)
}

fn fetch_as(addr: std::net::SocketAddr, path: &str, cookie: &str) -> u128 {
    let t = Instant::now();
    let res = send(addr, &format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n{cookie}\r\n"));
    assert!(
        res.starts_with("HTTP/1.1 200"),
        "{path}: {}",
        &res[..res.len().min(300)]
    );
    ms(t)
}

#[test]
fn screens_on_a_live_shaped_fixture() {
    let (c, f, sh) = live_shaped();
    let cache = c.state.with_file_name("cache");
    let files: Vec<_> = std::fs::read_dir(&cache).unwrap().flatten().collect();
    let mb = files.iter().map(|e| e.metadata().unwrap().len()).sum::<u64>() >> 20;
    let (c, _, addr) = serve(c, sh.on(Box::leak(Box::new(Live(f)))));
    let cookie = pair_on(addr, c);
    let mut p95 = vec![];
    let mut line = format!(
        "#1313 perf ({} gh cache files, {mb} MB, 3,000 events, 150 issues, 10 lanes): p95 of 20",
        files.len()
    );
    for path in ["/screens/queue", "/screens/hosts", "/screens/lanes", "/screens/threads"] {
        fetch_as(addr, path, &cookie);
        let ls = sh.methods().iter().filter(|m| *m == "ls").count();
        let mut t: Vec<u128> = (0..20).map(|_| fetch_as(addr, path, &cookie)).collect();
        let ls = (sh.methods().iter().filter(|m| *m == "ls").count() - ls) / 20;
        t.sort_unstable();
        line += &format!(" · {path} {} ms ({ls} shed ls)", t[18]);
        p95.push((path, t[18]));
    }
    report(&line);
    if cfg!(debug_assertions) {
        return eprintln!("debug build: budgets apply in release (cargo test --release --test perf)");
    }
    for (path, t) in p95 {
        let budget = if path == "/screens/hosts" { 50 } else { 150 };
        assert!(t < budget, "{path} p95 {t} ms (budget {budget})");
    }
}

#[test]
fn a_queue_build_lists_shed_once_and_neither_it_nor_the_hosts_sheet_starts_a_process() {
    let (c, f, sh) = big_with(10);
    let cfg = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(&c.config, format!("{cfg}\n[hosts.b1]\nmachine = \"b1\"\n")).unwrap();
    let r = NoSpawn(
        Slow(f, Mutex::default(), AtomicUsize::new(0), Some(sh), shed()),
        Mutex::default(),
    );
    let s = drover::serve::Server::new(&c, &r, Some(ADDR.into())).deferring();
    s.refresh(NOW);
    let ls = || sh.methods().iter().filter(|m| *m == "ls").count();
    for (path, most) in [("/screens/queue", 1), ("/screens/queue", 1), ("/screens/hosts", 0)] {
        let before = ls();
        let res = s.respond(&format!("GET {path} HTTP/1.1\r\nHost: {ADDR}\r\n\r\n"), NOW);
        let res = String::from_utf8_lossy(&res);
        assert!(
            res.starts_with("HTTP/1.1 200"),
            "{path}: {}",
            &res[..res.len().min(300)]
        );
        assert!(ls() - before <= most, "{path} listed shed {} times", ls() - before);
    }
    let ck = pair(&s, &c, "Mozilla/5.0 (iPhone)", NOW);
    let tap = json!({"screen": "hosts", "block": "hosts", "action": "host", "values": {"host": "b1", "to": "off"}})
        .to_string();
    let before = ls();
    let res = s.respond(
        &format!(
            "POST /action HTTP/1.1\r\nHost: {ADDR}\r\n{ck}Content-Length: {}\r\n\r\n{tap}",
            tap.len()
        ),
        NOW,
    );
    let res = String::from_utf8_lossy(&res);
    assert!(res.starts_with("HTTP/1.1 200") && res.contains("b1 off"), "{res}");
    assert_eq!(ls() - before, 0, "a host action listed shed");
    assert_eq!(
        *r.1.lock().unwrap(),
        Vec::<String>::new(),
        "processes started on a request"
    );
}
