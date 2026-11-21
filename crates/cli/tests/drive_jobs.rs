mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::runner::{Out, Runner};
use serde_json::json;
use std::path::PathBuf;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

const NOW: u64 = 1_000_000;
const HOLD: Duration = Duration::from_secs(20);

fn head(n: u64) -> String {
    format!("{n}{}", "a".repeat(39))
}

struct Boats {
    o: OverSsh<'static>,
    local: &'static FakeShed,
    remote: Mutex<Vec<String>>,
    detached: Mutex<Vec<Vec<String>>>,
    held: Mutex<(Vec<&'static str>, bool)>,
    moved: Condvar,
}

impl Boats {
    fn hold(&self, line: &str) {
        let job2 = std::thread::current().name() == Some("job-2");
        let what = match () {
            () if line.contains("slow-premerge") => "premerge",
            () if job2 && line.contains("issue-2") => "spawn",
            () => return,
        };
        let mut g = self.held.lock().unwrap();
        g.0.push(what);
        self.moved.notify_all();
        let _ = self.moved.wait_timeout_while(g, HOLD, |g| !g.1).unwrap();
    }
    fn holding(&self, what: &str) -> usize {
        self.held.lock().unwrap().0.iter().filter(|w| **w == what).count()
    }
    fn release(&self) {
        self.held.lock().unwrap().1 = true;
        self.moved.notify_all();
    }
    fn call(&self, prog: &str, args: &[String]) {
        self.hold(&format!("{prog} {}", args.join(" ")));
        if let Some(end) = args.iter().position(|a| a == "--").filter(|_| prog == "ssh") {
            std::thread::sleep(Duration::from_millis(700));
            self.remote.lock().unwrap().push(args[end - 1].clone());
        }
    }
}

impl Runner for Boats {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.call(prog, args);
        self.o.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        self.call(prog, args);
        self.o.run_in(prog, args, input)
    }
    fn sleep(&self, _: u64) {}
    fn shed(&self) -> Option<drover::shed::Shed> {
        Some(self.local.shed())
    }
    fn detach(&self, _: &str, args: &[String], _: &std::path::Path) -> anyhow::Result<u32> {
        self.detached.lock().unwrap().push(args.to_vec());
        Ok(4242)
    }
}

fn pr(n: u64, ci: &str) -> String {
    let check = match ci {
        "green" => json!({"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"}),
        _ => json!({"__typename": "CheckRun", "name": "ci", "status": "IN_PROGRESS"}),
    };
    json!({"number": 100 + n, "state": "OPEN", "baseRefName": "staging", "headRefOid": head(n),
           "mergeable": "MERGEABLE", "statusCheckRollup": [check]})
    .to_string()
}

fn spawned(c: &drover::Ctx, n: u64, role: &str, wt: &str, host: Option<&str>) {
    let agent = format!("sendmeter-{role}-{n}");
    let mut d = json!({"role": role, "agent": agent, "harness": "claude", "pane": agent, "runtime": "shed",
                       "worktree": wt, "branch": format!("issue-{n}")});
    if let Some(h) = host {
        (d["host"], d["machine"]) = (json!(h), json!(h));
    }
    append(&c.state, &Event::new(NOW - 900, "sendmeter", Some(n), "spawn", d)).unwrap();
}

fn fixture() -> (drover::Ctx, Boats, PathBuf) {
    let d = tmpdir("drive-jobs");
    let local = d.join("local");
    std::fs::create_dir_all(&local).unwrap();
    let base = CONFIG
        .replace("max_parallel_spawns = 0", "max_parallel_spawns = 6")
        .replace("/tmp/does-not-matter/sendmeter", &local.to_string_lossy())
        .replace(
            "integration = \"staging\"",
            "integration = \"staging\"\nmax_in_flight = 0\npremerge = [\"slow-premerge\"]",
        );
    let sh = Box::leak(Box::new(FakeShed::new()));
    let b1 = Box::leak(Box::new(FakeShed::new()));
    let c = ctx(
        &d,
        &sh.config(&format!(
            "{base}\n[hosts.mini]\n\n[hosts.b1]\nmachine = \"b1\"\nstart_max = 0\n"
        )),
    );
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let seen = json!({"mini": {"home": "/Users/me", "probed": NOW, "seen": NOW, "up": true},
                      "b1": {"home": "/home/me", "probed": NOW, "seen": NOW, "up": true}});
    std::fs::write(c.state.parent().unwrap().join("hosts.json"), seen.to_string()).unwrap();
    assert_eq!(run_only(&c, &Fake::default(), &["queue", "sendmeter", "add", "2"]).0, 0);
    let mut f = Fake::default()
        .reply("kill -0", "")
        .reply("ps -o etime=", "00:01\n")
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
        .reply("which shed", "/opt/shed/bin/shed\n")
        .reply("gh api -X POST repos/jirathip-dev/sendmeter/statuses/", "{}")
        .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
        .reply("gh issue", "")
        .reply("sh -c", "");
    for (n, ci) in [(1, "green"), (3, "pending"), (4, "green")] {
        let wt = c.worktrees.join(format!("sendmeter/issue-{n}"));
        std::fs::create_dir_all(&wt).unwrap();
        spawned(&c, n, "impl", &wt.to_string_lossy(), None);
        sh.lane(&format!("sendmeter-impl-{n}"), "idle", &wt.to_string_lossy());
        f = f
            .reply(&format!("gh pr view issue-{n}"), &pr(n, ci))
            .reply(&format!("gh pr view {}", 100 + n), r#"{"mergeCommit":{"oid":"m3rg3"}}"#)
            .reply(
                &format!("gh api repos/jirathip-dev/sendmeter/branches/issue-{n}"),
                &head(n),
            );
        if ci == "green" {
            let v = json!({"verdict": "PASS", "sha": head(n), "body": "PASS"});
            append(&c.state, &Event::new(NOW - 800, "sendmeter", Some(n), "verdict", v)).unwrap();
        }
    }
    let rev = c.worktrees.join("sendmeter/rev-3-r1");
    std::fs::create_dir_all(&rev).unwrap();
    spawned(&c, 3, "rev", &rev.to_string_lossy(), None);
    sh.lane("sendmeter-rev-3", "idle", &rev.to_string_lossy());
    f = f.reply(&format!("git -C {} log", rev.display()), &format!("{} 20", head(3)));
    for n in 11..=14 {
        let wt = format!("/home/me/.herdr/worktrees/sendmeter/issue-{n}");
        spawned(&c, n, "impl", &wt, Some("b1"));
        b1.lane(&format!("sendmeter-impl-{n}"), "working", &wt);
    }
    let f = f.reply("git -C", "").reply("ssh", "");
    let o = OverSsh {
        f: Box::leak(Box::new(f)),
        hosts: vec![("b1", &*b1)],
    };
    let r = Boats {
        o,
        local: sh,
        remote: Mutex::default(),
        detached: Mutex::default(),
        held: Mutex::default(),
        moved: Condvar::new(),
    };
    (c, r, rev)
}

fn issue_of(a: &[String]) -> String {
    a.get(2).cloned().unwrap_or_default()
}

#[test]
fn with_a_long_premerge_and_spawn_in_flight_every_pass_ends_under_5s_and_a_verdict_lands_within_5s() {
    let (c, r, rev) = fixture();
    let verdict = || read(&c.state).unwrap().iter().any(|e| e.is("verdict", "sendmeter", 3));
    let (mut passes, mut waited) = (vec![], Duration::ZERO);
    std::thread::scope(|s| {
        let jobs = || {
            for a in std::mem::take(&mut *r.detached.lock().unwrap()) {
                let (c, r) = (&c, &r);
                std::thread::Builder::new()
                    .name(format!("job-{}", issue_of(&a)))
                    .spawn_scoped(s, move || {
                        let a: Vec<&str> = a.iter().map(String::as_str).collect();
                        run_only(c, r, &a)
                    })
                    .unwrap();
            }
        };
        look(&c, &r);
        r.remote.lock().unwrap().clear();
        let first = s.spawn(|| {
            let t = Instant::now();
            let (code, out) = run_only(&c, &r, &["drive", "sendmeter"]);
            (t.elapsed(), code, out)
        });
        let t = Instant::now();
        while (r.holding("premerge") == 0 || r.holding("spawn") == 0) && t.elapsed() < Duration::from_secs(30) {
            jobs();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            (r.holding("premerge"), r.holding("spawn")),
            (1, 1),
            "a merge and a start in flight"
        );
        std::fs::write(rev.join("verdict.md"), "PASS\nall good\n").unwrap();
        let t = Instant::now();
        while !verdict() && t.elapsed() < Duration::from_secs(40) {
            let p = Instant::now();
            let (code, out) = run_only(&c, &r, &["drive", "sendmeter"]);
            passes.push(p.elapsed());
            assert_eq!(code, 0, "{out}");
            jobs();
            let p = Instant::now();
            while !verdict() && p.elapsed() < Duration::from_secs(1) {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        waited = t.elapsed();
        let in_flight = (r.holding("premerge"), r.holding("spawn"), r.held.lock().unwrap().1);
        let (secs, code, out) = first.join().unwrap();
        passes.push(secs);
        assert_eq!(code, 0, "{out}");
        r.release();
        let max = passes.iter().max().copied().unwrap_or_default().as_secs_f64();
        eprintln!(
            "#1321 perf (a premerge and a spawn held in flight, +700 ms per remote call): longest pass {max:.1} s over {} passes, verdict recorded {:.1} s after it was written, remote calls {:?}",
            passes.len(),
            waited.as_secs_f64(),
            r.remote.lock().unwrap()
        );
        assert_eq!(in_flight, (1, 1, false), "both still in flight when the verdict landed");
        assert!(max < 5.0, "the longest pass took {max:.1} s (target < 5)");
        assert!(waited.as_secs_f64() < 5.0, "the verdict took {waited:?} (target < 5 s)");
    });
    let ev = read(&c.state).unwrap();
    let plan = ev
        .iter()
        .find(|e| e.kind == "drive-pass" && e.val("kick").is_null())
        .unwrap();
    let refused =
        |e: &&Event| e.kind == "driven" && e.str("step") == Some("merge") && e.str("code") == Some("IN_FLIGHT");
    assert!(ev.iter().any(|e| refused(&e)), "a second merge waited for the lease");
    let start = plan.val("steps")["spawns"].as_f64().unwrap();
    eprintln!("#1321 perf: the first pass's start step took {start:.1} s");
    assert!(start < 1.0, "a start held the plan {start:.1} s (target < 1)");
}

#[test]
fn a_kick_with_every_job_slot_of_its_fleet_busy_drives_nothing() {
    let (c, r, rev) = fixture();
    std::fs::write(rev.join("verdict.md"), "PASS\n").unwrap();
    (0..4).for_each(|i| lease_as(&c, &format!("job-sendmeter-{i}.lock"), std::process::id(), NOW, 600));
    let (code, out) = run_only(&c, &r, &["kick", "sendmeter", "3", "--why", "plan", "--now"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("kick sendmeter #3: every job_max slot of sendmeter is busy"),
        "{out}"
    );
    assert!(
        !read(&c.state).unwrap().iter().any(|e| e.is("verdict", "sendmeter", 3)),
        "{out}"
    );
    drover::db::release(&c.state, "job-sendmeter-0.lock", None);
    let (code, out) = run_only(&c, &r, &["kick", "sendmeter", "3", "--why", "plan", "--now"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        read(&c.state).unwrap().iter().any(|e| e.is("verdict", "sendmeter", 3)),
        "{out}"
    );
}
