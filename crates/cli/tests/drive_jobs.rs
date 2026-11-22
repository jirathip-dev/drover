mod common;
use common::*;
use drover::events::{append, read, Event};
use drover::runner::{Out, Runner};
use drover::sim::Point;
use serde_json::json;
use std::cell::Cell;
use std::path::PathBuf;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

const NOW: u64 = 1_000_000;
const COST_MS: u64 = 700;
const PREMERGE_MS: u64 = 600_000;
const SPAWN_MS: u64 = 180_000;
const GUARD: Duration = Duration::from_secs(60);

thread_local!(static SPENT: Cell<u64> = const { Cell::new(0) });
thread_local!(static START: Cell<u64> = const { Cell::new(0) });
static VERDICT: Mutex<Option<u64>> = Mutex::new(None);

fn clock(at_ms: u64) {
    SPENT.set(at_ms);
    drover::sim::set(Some(drover::sim::Hooks {
        now: Box::new(|| NOW + SPENT.get() / 1000),
        pid: std::process::id(),
        point: Box::new(|p| {
            if matches!(p, Point::Append(e) if e.is("verdict", "sendmeter", 3)) {
                VERDICT.lock().unwrap().get_or_insert(SPENT.get());
            }
        }),
    }));
}

fn charge(ms: u64, line: &str) {
    SPENT.set(SPENT.get() + ms);
    if line.contains("issue-2") {
        START.set(START.get() + ms);
    }
}

fn head(n: u64) -> String {
    format!("{n}{}", "a".repeat(39))
}

#[derive(Default)]
struct Held {
    what: Vec<&'static str>,
    released: bool,
    settled: usize,
}

struct Boats {
    o: OverSsh<'static>,
    local: &'static FakeShed,
    remote: Mutex<Vec<String>>,
    detached: Mutex<Vec<Vec<String>>>,
    held: Mutex<Held>,
    moved: Condvar,
}

impl Boats {
    fn hold(&self, line: &str) {
        let (what, ms) = match () {
            () if line.contains("slow-premerge") => ("premerge", PREMERGE_MS),
            () if line.contains("rev-parse --verify --quiet issue-2") => ("spawn", SPAWN_MS),
            () => return,
        };
        charge(ms, line);
        if !std::thread::current().name().is_some_and(|n| n.starts_with("job-")) {
            return;
        }
        let mut g = self.held.lock().unwrap();
        g.what.push(what);
        g.settled += 1;
        self.moved.notify_all();
        let (_g, t) = self.moved.wait_timeout_while(g, GUARD, |g| !g.released).unwrap();
        assert!(!t.timed_out(), "{what} never released");
    }
    fn holding(&self, what: &str) -> usize {
        self.held.lock().unwrap().what.iter().filter(|w| **w == what).count()
    }
    fn settled(&self) {
        self.held.lock().unwrap().settled += 1;
        self.moved.notify_all();
    }
    fn settle(&self, jobs: usize) {
        let g = self.held.lock().unwrap();
        let (_g, t) = self.moved.wait_timeout_while(g, GUARD, |g| g.settled < jobs).unwrap();
        assert!(!t.timed_out(), "a job neither ended nor reached its hold");
    }
    fn release(&self) {
        self.held.lock().unwrap().released = true;
        self.moved.notify_all();
    }
    fn call(&self, prog: &str, args: &[String]) {
        let line = format!("{prog} {}", args.join(" "));
        self.hold(&line);
        if let Some(end) = args.iter().position(|a| a == "--").filter(|_| prog == "ssh") {
            charge(COST_MS, &line);
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
    fn sleep(&self, secs: u64) {
        charge(secs * 1000, "");
    }
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
    clock(0);
    let (mut passes, mut waited, mut start) = (vec![], None, 0);
    std::thread::scope(|s| {
        let mut jobs = 0;
        let mut run_jobs = || {
            let at = SPENT.get();
            for a in std::mem::take(&mut *r.detached.lock().unwrap()) {
                let (c, r) = (&c, &r);
                jobs += 1;
                std::thread::Builder::new()
                    .name(format!("job-{}", issue_of(&a)))
                    .spawn_scoped(s, move || {
                        clock(at);
                        let a: Vec<&str> = a.iter().map(String::as_str).collect();
                        run_only(c, r, &a);
                        r.settled();
                    })
                    .unwrap();
            }
            r.settle(jobs);
        };
        let mut pass = || {
            let t = SPENT.get();
            let (code, out) = run_only(&c, &r, &["drive", "sendmeter"]);
            assert_eq!(code, 0, "{out}");
            passes.push(SPENT.get() - t);
        };
        look(&c, &r);
        r.remote.lock().unwrap().clear();
        START.set(0);
        pass();
        start = START.get();
        run_jobs();
        let in_flight = (r.holding("premerge"), r.holding("spawn"));
        std::fs::write(rev.join("verdict.md"), "PASS\nall good\n").unwrap();
        let t = SPENT.get();
        for _ in 0..10 {
            pass();
            run_jobs();
            if let Some(v) = *VERDICT.lock().unwrap() {
                waited = Some(v.saturating_sub(t));
                break;
            }
        }
        let still = (
            r.holding("premerge"),
            r.holding("spawn"),
            r.held.lock().unwrap().released,
        );
        r.release();
        let max = passes.iter().max().copied().unwrap_or_default();
        eprintln!(
            "#1321 perf (test clock: a 10 min premerge and a 3 min start in flight, 700 ms per remote call): longest pass {:.1} s over {} passes, verdict recorded {:.1} s after it was written, the first pass's start {:.1} s, remote calls {:?}",
            ms_secs(max),
            passes.len(),
            waited.map_or(f64::NAN, ms_secs),
            ms_secs(start),
            r.remote.lock().unwrap()
        );
        assert_eq!(in_flight, (1, 1), "a merge and a start in flight");
        assert_eq!(still, (1, 1, false), "both still in flight when the verdict landed");
        assert!(max < 5_000, "the longest pass took {max} ms (target < 5 s)");
        assert!(
            waited.is_some_and(|w| w < 5_000),
            "the verdict took {waited:?} ms (target < 5 s)"
        );
        assert!(start < 1_000, "a start held the plan {start} ms (target < 1 s)");
    });
    let ev = read(&c.state).unwrap();
    let refused =
        |e: &&Event| e.kind == "driven" && e.str("step") == Some("merge") && e.str("code") == Some("IN_FLIGHT");
    assert!(ev.iter().any(|e| refused(&e)), "a second merge waited for the lease");
}

fn ms_secs(ms: u64) -> f64 {
    Duration::from_millis(ms).as_secs_f64()
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

#[test]
fn a_finished_review_is_recorded_at_once_while_a_spawn_holds_its_start_and_the_fleets_only_job_slot() {
    static AT: Mutex<Option<u64>> = Mutex::new(None);
    let (c, r, rev) = fixture();
    let cfg = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(&c.config, cfg.replace("[limits]", "[limits]\njob_max = 1")).unwrap();
    let clock = |at: u64| {
        SPENT.set(at);
        drover::sim::set(Some(drover::sim::Hooks {
            now: Box::new(|| NOW + SPENT.get() / 1000),
            pid: std::process::id(),
            point: Box::new(|p| {
                if matches!(p, Point::Append(e) if e.is("verdict", "sendmeter", 3)) {
                    AT.lock().unwrap().get_or_insert(SPENT.get());
                }
            }),
        }));
    };
    clock(0);
    std::thread::scope(|s| {
        let mut jobs = 0;
        let mut run = |keep: &dyn Fn(&[String]) -> bool| {
            let at = SPENT.get();
            for a in std::mem::take(&mut *r.detached.lock().unwrap())
                .into_iter()
                .filter(|a| keep(a))
            {
                let (c, r) = (&c, &r);
                jobs += 1;
                std::thread::Builder::new()
                    .name(format!("job-{}", issue_of(&a)))
                    .spawn_scoped(s, move || {
                        clock(at);
                        let a: Vec<&str> = a.iter().map(String::as_str).collect();
                        run_only(c, r, &a);
                        r.settled();
                    })
                    .unwrap();
            }
            r.settle(jobs);
        };
        let (code, out) = run_only(&c, &r, &["drive", "sendmeter"]);
        assert_eq!(code, 0, "{out}");
        run(&|a| issue_of(a) == "2" && a.iter().any(|x| x == "drive"));
        assert_eq!(r.holding("spawn"), 1, "the start of #2 is in flight");
        std::fs::write(rev.join("verdict.md"), "PASS\nall good\n").unwrap();
        let (code, out) = run_only(&c, &r, &["drive", "sendmeter"]);
        assert_eq!(code, 0, "{out}");
        let t = SPENT.get();
        run(&|a| issue_of(a) == "3");
        let still = (r.holding("spawn"), r.held.lock().unwrap().released);
        r.release();
        assert_eq!(still, (1, false), "the start was still in flight");
        let waited = AT.lock().unwrap().map(|v| v.saturating_sub(t));
        assert!(
            waited.is_some_and(|w| w < 5_000),
            "the verdict of #3 took {waited:?} ms behind the start of #2 (target < 5 s)"
        );
    });
}
