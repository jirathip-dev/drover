mod common;
use common::*;
use drover::events::{append, Event};
use drover::runner::{Out, Runner};
use serde_json::json;
use std::time::Duration;

const BOUND: Duration = Duration::from_secs(20);

fn within<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (tx, rx) = std::sync::mpsc::channel();
    let h = std::thread::spawn(move || drop(tx.send(f())));
    match rx.recv_timeout(BOUND) {
        Ok(v) => v,
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => std::panic::resume_unwind(h.join().unwrap_err()),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("{what} still blocked after {} s", BOUND.as_secs()),
    }
}

fn bounded<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let what = what.to_string();
    std::thread::spawn(move || {
        if rx.recv_timeout(BOUND) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
            eprintln!("{what} still blocked after {} s", BOUND.as_secs());
            std::process::exit(1);
        }
    });
    let v = f();
    drop(tx);
    v
}

fn base() -> String {
    CONFIG
        .replace("harness  =  \"claude\"", "harness = \"claude\"")
        .replace("[limits]\n", "[limits]\nshed_call_secs = 1\nssh_sessions = 64\n")
}

fn config(sh: &FakeShed) -> String {
    sh.config(&base())
}

fn spawn_fake() -> Fake {
    Fake::default()
        .once("git -C /tmp/does-not-matter/sendmeter rev-parse --verify", 1, "")
        .reply("gh issue view 12", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply("git -C", "")
        .reply("which shed", "/opt/shed/bin/shed\n")
}

fn spawn(c: &drover::Ctx) -> (i32, String) {
    bounded("spawn", || run(c, &spawn_fake(), &["spawn", "sendmeter", "12", "impl"]))
}

fn kinds(c: &drover::Ctx, kind: &str) -> Vec<drover::events::Event> {
    let all = drover::events::read(&c.state).map(|e| e.to_vec()).unwrap_or_default();
    all.into_iter().filter(|e| e.kind == kind).collect()
}

#[test]
fn a_spawn_behind_a_hung_shed_link_fails_within_its_bound_instead_of_waiting_forever() {
    let sh = FakeShed::new();
    let d = tmpdir("fault-hang");
    let c = ctx(&d, &config(&sh));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    sh.fault(Fault::Hang);
    let (code, out) = spawn(&c);
    assert_ne!(code, 0, "{out}");
    assert!(kinds(&c, "spawn").is_empty(), "no lane is recorded: {out}");
}

#[test]
fn a_prompt_to_a_lane_behind_a_hung_shed_link_fails_within_its_bound() {
    let sh = FakeShed::new();
    sh.add(IMPL, json!({"fleet": "sendmeter", "issue": "12", "role": "impl"}));
    let d = tmpdir("fault-prompt");
    ctx(&d, &config(&sh));
    drover::config::Config::load(&d.join("config/fleet.toml")).unwrap();
    sh.fault(Fault::Hang);
    let b = sh.shed();
    let err = within("prompt", move || {
        let f = Fake::default();
        let r = drover::shed::With(&f, b);
        drover::term::reply(&r, IMPL, IMPL, "go on").map_err(|e| format!("{e:#}"))
    });
    assert!(err.unwrap_err().contains("no answer"));
    assert_eq!(sh.sent(IMPL), Vec::<String>::new());
}

#[test]
fn a_spawn_over_a_slow_shed_link_still_starts_its_lane() {
    let sh = FakeShed::new();
    let d = tmpdir("fault-latency");
    let c = ctx(&d, &config(&sh));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    sh.fault(Fault::Latency(Duration::from_millis(300)));
    let (code, out) = spawn(&c);
    assert_eq!(code, 0, "{out}");
    let s = kinds(&c, "spawn");
    assert_eq!(s.len(), 1, "{out}");
    assert_eq!(s[0].val("confirmed"), true);
}

#[test]
fn a_spawn_whose_link_drops_a_connection_fails_cleanly_and_the_next_one_starts() {
    let sh = FakeShed::new();
    let d = tmpdir("fault-loss");
    let c = ctx(&d, &config(&sh));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    sh.fault(Fault::Loss(3));
    let (code, out) = spawn(&c);
    assert_ne!(code, 0, "{out}");
    assert!(kinds(&c, "spawn").is_empty(), "{out}");
    assert!(!sh.has(IMPL), "{:?}", sh.methods());
    let (code, out) = spawn(&c);
    assert_eq!(code, 0, "{out}");
    assert_eq!(kinds(&c, "spawn").len(), 1, "{out}");
}

#[test]
fn a_pass_after_the_lanes_shed_host_vanished_mid_lane_ends_within_its_bound() {
    let sh = FakeShed::new();
    let d = tmpdir("fault-gone");
    let c = ctx(&d, &config(&sh));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    assert_eq!(spawn(&c).0, 0);
    sh.fault(Fault::Gone);
    let later = drover::Ctx {
        now: c.now + 60,
        ..c.clone()
    };
    let (_, out) = within("drive", move || run(&later, &spawn_fake(), &["drive"]));
    assert!(
        out.contains("sendmeter: the main host gave no word (no shed server"),
        "{out}"
    );
    assert_eq!(
        kinds(&c, "spawn").len(),
        1,
        "no second lane over a vanished host: {out}"
    );
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Ssh {
    Up,
    Moved,
    Rekeyed,
    Gone,
    Hang,
}

struct Link<'a> {
    r: &'a dyn Runner,
    host: &'static str,
    fault: Shared<Ssh>,
}

impl Link<'_> {
    fn cut(&self, prog: &str, args: &[String]) -> Option<anyhow::Result<Out>> {
        if prog != "ssh" || !args.iter().any(|a| a == self.host) {
            return None;
        }
        let h = self.host;
        let (code, stderr) = match self.fault.get() {
            Ssh::Up => return None,
            Ssh::Moved => (255, format!("ssh: Could not resolve hostname {h}: nodename nor servname provided, or not known")),
            Ssh::Rekeyed => (255, format!("@@@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @@@\nHost key for {h} has changed and you have requested strict checking.\nHost key verification failed.")),
            Ssh::Gone => (255, format!("ssh: connect to host {h} port 22: Connection timed out")),
            Ssh::Hang => (124, String::new()),
        };
        Some(Ok(Out {
            code,
            stderr,
            ..Out::default()
        }))
    }
}

impl Runner for Link<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.cut(prog, args).unwrap_or_else(|| self.r.run(prog, args))
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.cut(prog, args).unwrap_or_else(|| self.r.run_in(prog, args, input))
    }
    fn run_for(&self, prog: &str, args: &[String], input: &str, limit: Duration) -> anyhow::Result<Out> {
        self.cut(prog, args)
            .unwrap_or_else(|| self.r.run_for(prog, args, input, limit))
    }
    fn sleep(&self, secs: u64) {
        self.r.sleep(secs);
    }
    fn shed(&self) -> Option<drover::shed::Shed> {
        self.r.shed()
    }
}

const NOW: u64 = 1_000_000;
const SSH: &str = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/drover-ssh-%C -oControlPersist=10m bazzite -- ";
const WT: &str = "/home/me/.local/share/drover/work/sendmeter/issue-12";
const REV: &str = "sendmeter-rev-12-r1";

fn head() -> String {
    "a".repeat(40)
}

fn hosts(base: &str) -> String {
    format!("{base}\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n")
}

fn review(name: &str) -> (drover::Ctx, String) {
    let rev = tmpdir(&format!("{name}-rev")).display().to_string();
    let c = ctx(&tmpdir(name), &hosts(&base()));
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
        "bazzite": {"tags": ["os:linux"], "home": "/home/me", "probed": NOW, "seen": NOW, "up": true,
                    "load": 0.1, "mem": 12.0, "disk": 200.0},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    let ev = |ts, d| append(&c.state, &Event::new(ts, "sendmeter", Some(12), "spawn", d)).unwrap();
    ev(
        NOW - 900,
        json!({"role": "impl", "agent": IMPL, "harness": "claude", "pane": IMPL, "workspace": "",
               "worktree": WT, "branch": "issue-12", "host": "bazzite", "machine": "bazzite"}),
    );
    ev(
        NOW - 600,
        json!({"role": "rev", "agent": REV, "harness": "claude", "pane": REV, "workspace": "", "worktree": rev,
               "branch": "rev-12-r1", "sha": head(), "host": "bazzite", "machine": "bazzite"}),
    );
    std::fs::write(
        format!("{rev}/verdict.md"),
        format!("PASS\n\nReviewed head: {}\n", head()),
    )
    .unwrap();
    (c, rev)
}

fn bazzite(rev: &str) -> Fake {
    let pr = json!({"number": 34, "state": "OPEN", "baseRefName": "staging", "headRefOid": head(),
                    "statusCheckRollup": [{"__typename": "CheckRun", "name": "ci", "status": "IN_PROGRESS"}]});
    Fake::default()
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("gh pr view issue-12", &pr.to_string())
        .reply(
            "gh api repos/jirathip-dev/sendmeter/branches/issue-12",
            &format!("{}\n", head()),
        )
        .reply(
            &format!("{SSH}'git' '--no-replace-objects' '-C' '{WT}' 'log'"),
            &format!("{} 20", head()),
        )
        .reply(&format!("{SSH}'sh' '-c' 'test -e"), "")
        .reply(
            &format!("{SSH}'git' '--no-replace-objects' '-C' '{rev}' 'rev-list'"),
            "0\n",
        )
        .reply(&format!("{SSH}'git' '--no-replace-objects' '-C' '{rev}' 'status'"), "")
        .reply(
            &format!("{SSH}'git' '--no-replace-objects' '-C' '{rev}' 'worktree' 'remove'"),
            "",
        )
        .reply(&format!("{SSH}'rm'"), "")
}

fn pickup_through(fault: Ssh) {
    let (c, rev) = review(&format!("fault-pickup-{fault:?}"));
    let (_local, far) = (FakeShed::attached(&c), FakeShed::new());
    far.lane(REV, "idle", &rev);
    far.exit(REV);
    let f = bazzite(&rev);
    let over = OverSsh {
        f: &f,
        hosts: vec![("bazzite", &far)],
    };
    let link = Link {
        r: &over,
        host: "bazzite",
        fault: Shared::new(fault),
    };
    let pass = |c: &drover::Ctx| {
        let t = std::time::Instant::now();
        let (code, out) = run(c, &link, &["drive"]);
        assert!(
            t.elapsed() < BOUND,
            "a pass over a {fault:?} link took {:?}",
            t.elapsed()
        );
        (code, out)
    };
    let (code, out) = pass(&c);
    assert_eq!(code, 0, "{out}");
    assert!(kinds(&c, "verdict").is_empty(), "{out}");
    assert_eq!(kinds(&c, "verdict-pickup-failed").len(), 1, "{out}");
    let revs = kinds(&c, "spawn")
        .iter()
        .filter(|e| e.str("role") == Some("rev"))
        .count();
    assert_eq!(revs, 1, "a finished review is kept, not redone: {out}");

    link.fault.set(Ssh::Up);
    let next = drover::Ctx {
        now: c.now + 60,
        ..c.clone()
    };
    let (code, out) = pass(&next);
    assert_eq!(code, 0, "{out}");
    let v = kinds(&c, "verdict");
    assert_eq!(v.len(), 1, "recorded once the link is back: {out}");
    assert_eq!(
        (v[0].str("verdict"), v[0].str("sha")),
        (Some("PASS"), Some(head().as_str()))
    );
    let seen = kinds(&c, "verdict-seen");
    assert_eq!(seen.len(), 1, "logged once for the one verdict.md: {seen:?}");
    assert_eq!(seen[0].str("first"), Some("PASS"));
}

#[test]
fn a_verdict_behind_a_host_whose_address_changed_is_kept_and_recorded_once_it_answers() {
    within("verdict pickup", || pickup_through(Ssh::Moved));
}

#[test]
fn a_verdict_behind_a_host_whose_key_changed_is_kept_and_recorded_once_it_answers() {
    within("verdict pickup", || pickup_through(Ssh::Rekeyed));
}

#[test]
fn a_verdict_on_a_host_that_vanished_mid_lane_is_kept_and_recorded_once_it_is_back() {
    within("verdict pickup", || pickup_through(Ssh::Gone));
}

#[test]
fn a_verdict_behind_a_hung_ssh_link_is_kept_and_recorded_once_it_answers() {
    within("verdict pickup", || pickup_through(Ssh::Hang));
}

struct Here(Fake);

impl Runner for Here {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.run_in(prog, args, "")
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        if let Some(o) = boat(self, prog, args, input) {
            return o;
        }
        let line = format!("{prog} {}", args.join(" "));
        let host_job = |sh: &&str| ["'shed'", "launchctl", "systemctl"].iter().any(|k| sh.contains(k));
        let Some(sh) = line.strip_prefix(SSH).filter(|sh| !host_job(sh)) else {
            return self.0.run_in(prog, args, input);
        };
        let mut ch = std::process::Command::new("sh")
            .args(["-c", sh])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        std::io::Write::write_all(&mut ch.stdin.take().unwrap(), input.as_bytes())?;
        let o = ch.wait_with_output()?;
        Ok(Out {
            code: o.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&o.stdout).into(),
            stderr: String::from_utf8_lossy(&o.stderr).into(),
        })
    }
}

fn drain_through(fault: Ssh) {
    let d = tmpdir(&format!("fault-drain-{fault:?}"));
    let c = ctx(&d, &hosts(&base()));
    let _local = FakeShed::attached(&c);
    let wt = d.join("worktree");
    std::fs::create_dir_all(wt.join(".drover")).unwrap();
    std::fs::write(wt.join(".git"), "gitdir: elsewhere\n").unwrap();
    let ask = json!({"args": ["ask", "sendmeter", "12", "Which way?", "--option", "a", "--option", "b"],
                     "lane": "sendmeter:12:impl", "ts": (NOW - 30) * 1000});
    let ob = wt.join(".drover/outbox.jsonl");
    std::fs::write(&ob, format!("{ask}\n")).unwrap();
    let d = json!({"role": "impl", "agent": IMPL, "harness": "claude", "pane": IMPL, "workspace": "",
                   "worktree": wt, "branch": "issue-12", "host": "bazzite", "machine": "bazzite"});
    append(&c.state, &Event::new(NOW - 60, "sendmeter", Some(12), "spawn", d)).unwrap();
    let here = Here(Fake::default());
    let link = Link {
        r: &here,
        host: "bazzite",
        fault: Shared::new(fault),
    };
    let t = std::time::Instant::now();
    run(&c, &link, &["drive"]);
    assert!(
        t.elapsed() < BOUND,
        "a pass over a {fault:?} link took {:?}",
        t.elapsed()
    );
    assert!(kinds(&c, "needs-decision").is_empty());
    assert!(
        std::fs::read_to_string(&ob).unwrap().contains("Which way?"),
        "the line waits on its host"
    );

    link.fault.set(Ssh::Up);
    for k in 1..=2 {
        let next = drover::Ctx {
            now: c.now + 60 * k,
            ..c.clone()
        };
        run(&next, &link, &["drive"]);
    }
    let asks = kinds(&c, "needs-decision");
    assert_eq!(
        asks.len(),
        1,
        "delivered once the link is back, and only once: {asks:?}"
    );
    assert_eq!(asks[0].str("question"), Some("Which way?"));
}

#[test]
fn an_outbox_behind_a_host_whose_address_changed_is_drained_once_it_answers() {
    within("outbox drain", || drain_through(Ssh::Moved));
}

#[test]
fn an_outbox_behind_a_host_whose_key_changed_is_drained_once_it_answers() {
    within("outbox drain", || drain_through(Ssh::Rekeyed));
}

#[test]
fn an_outbox_on_a_host_that_vanished_mid_lane_is_drained_once_it_is_back() {
    within("outbox drain", || drain_through(Ssh::Gone));
}

#[test]
fn an_outbox_behind_a_hung_ssh_link_is_drained_once_it_answers() {
    within("outbox drain", || drain_through(Ssh::Hang));
}

fn soak_ssh(host: &str) -> String {
    SSH.replace(" bazzite -- ", &format!(" {host} -- "))
}

#[test]
fn the_nightly_soak_logs_each_hosts_numbers_and_files_one_issue_per_missed_target_per_night() {
    let (air, boat) = (FakeShed::new(), FakeShed::new());
    let d = tmpdir("fault-soak");
    let soak = "\n[fleet.sendmeter.soak]\nhosts = [\"air\", \"boat\"]\nspawn_secs = 30\nverdict_minutes = 1\n";
    let hosts = "\n[hosts.air]\nmachine = \"air\"\n\n[hosts.boat]\nmachine = \"boat\"\n";
    let c = ctx(&d, &format!("{}{soak}{hosts}", base()));
    let _local = FakeShed::attached(&c);
    let filed = "[{\"number\": 7, \"title\": \"soak: sendmeter verdict target missed 1970-01-12\"}, {\"number\": 8, \"title\": \"soak: sendmeter stuck target missed 1970-01-12\"}]";
    let night = |list: &str| {
        Fake::default()
            .reply(
                &format!("{}'cat' '/tmp/drover-soak/sendmeter-soak-air.md'", soak_ssh("air")),
                "PASS\n",
            )
            .once_err(&format!("{}'cat'", soak_ssh("boat")), 1, "No such file")
            .reply(&format!("{}'cat'", soak_ssh("boat")), "")
            .reply("gh issue list", list)
            .reply(
                "gh issue create",
                "https://github.com/jirathip-dev/sendmeter/issues/7\n",
            )
    };
    let f = night("[]");
    let over = OverSsh {
        f: &f,
        hosts: vec![("air", &air), ("boat", &boat)],
    };
    let t = std::time::Instant::now();
    let (code, out) = run(&c, &over, &["soak", "sendmeter"]);
    assert!(t.elapsed() < BOUND, "{:?}", t.elapsed());
    assert_eq!(code, 1, "{out}");
    assert!(
        air.called("run", "sendmeter-soak-air") && air.called("rm", "sendmeter-soak-air"),
        "{:?}",
        air.methods()
    );
    assert!(
        !air.has("sendmeter-soak-air") && !boat.has("sendmeter-soak-boat"),
        "no soak lane is left behind"
    );
    let s = kinds(&c, "soak");
    assert_eq!(s.len(), 1, "{out}");
    assert_eq!(s[0].val("hosts")["air"]["stuck"], false, "{:?}", s[0]);
    assert!(s[0].val("hosts")["air"]["verdict_ms"].is_u64(), "{:?}", s[0]);
    assert!(s[0].val("hosts")["boat"]["verdict_ms"].is_null(), "{:?}", s[0]);
    assert_eq!(s[0].val("missed"), &json!(["verdict", "stuck"]));
    let created: Vec<String> = f
        .calls
        .borrow()
        .iter()
        .filter(|l| l.starts_with("gh issue create"))
        .cloned()
        .collect();
    assert_eq!(created.len(), 2, "{created:?}");
    assert!(
        created[0].contains("soak: sendmeter verdict target missed 1970-01-12") && created[0].contains("boat"),
        "{created:?}"
    );

    let f = night(filed);
    let over = OverSsh {
        f: &f,
        hosts: vec![("air", &air), ("boat", &boat)],
    };
    run(&c, &over, &["soak", "sendmeter"]);
    assert!(!f.called("gh issue create"), "one issue per target per night");
}

#[test]
fn the_tick_starts_the_soak_once_a_night_without_waiting_on_it() {
    let d = tmpdir("fault-soak-tick");
    let soak = "\n[fleet.sendmeter.soak]\nhosts = [\"mini\"]\nspawn_secs = 30\nverdict_minutes = 1\n";
    let c = ctx(&d, &format!("{}{soak}\n[hosts.mini]\n", base()));
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let f = Fake::default().reply("sh -c nohup", "4242\n");
    for later in [0, 3600, 86_400] {
        let at = drover::Ctx {
            now: c.now + later,
            ..c.clone()
        };
        drover::soak::nightly(&at, &cfg, &f).unwrap();
    }
    let started: Vec<String> = f
        .calls
        .borrow()
        .iter()
        .filter(|l| l.contains("soak sendmeter"))
        .cloned()
        .collect();
    assert_eq!(started.len(), 2, "{started:?}");
}
