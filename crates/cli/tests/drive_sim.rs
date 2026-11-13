mod common;
use common::*;
use herdsman::events::{append, read, Event};
use herdsman::runner::{Out, Runner};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

const T0: u64 = 1_000_000;
const HEAD: &str = "c856000000000000000000000000000000000000";
const TICK_PID: u32 = 4242;

#[derive(Clone, Copy, PartialEq)]
enum Reach {
    Down,
    Hang,
}

struct Pane {
    agent: String,
    status: &'static str,
    pane: String,
    cwd: String,
    screen: String,
    born: u64,
}

struct Pr {
    head: &'static str,
    draft: bool,
    state: &'static str,
    green_at: u64,
}

struct World {
    base: Fake,
    now: Shared<u64>,
    pr: Shared<Option<Pr>>,
    panes: Shared<Vec<Pane>>,
    hosts: Shared<BTreeMap<String, Reach>>,
    procs: Shared<BTreeMap<u32, (String, u64)>>,
    ws: Shared<u32>,
    trace: Shared<Vec<String>>,
    ticks: Shared<Vec<(u64, i32)>>,
    hung: (Mutex<Option<u32>>, Condvar),
}

thread_local!(static PID: std::cell::Cell<u32> = const { std::cell::Cell::new(0) });

fn out(code: i32, stdout: impl Into<String>) -> Out {
    Out {
        code,
        stdout: stdout.into(),
        stderr: String::new(),
    }
}

impl World {
    fn new() -> Arc<World> {
        let base = Fake::default()
            .reply(
                "herdr status",
                "client:\n  version: 0.9.5\n\nserver:\n  status: running\n  version: 0.9.5\n",
            )
            .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
            .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
            .reply("sysctl -n hw.ncpu", "10\n")
            .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n")
            .reply("gh issue view", r#"{"title":"t","body":"b"}"#)
            .reply("gh issue", "")
            .reply("git ", "")
            .reply("herdr pane", "{}")
            .reply("herdr agent", "{}")
            .reply("sh -c nohup", "4321");
        Arc::new(World {
            base,
            now: Shared::new(T0),
            pr: Shared::default(),
            panes: Shared::default(),
            hosts: Shared::default(),
            procs: Shared::default(),
            ws: Shared::new(0),
            trace: Shared::default(),
            ticks: Shared::default(),
            hung: (Mutex::new(None), Condvar::new()),
        })
    }

    fn now(&self) -> u64 {
        self.now.get()
    }

    fn pr_json(&self) -> Option<Value> {
        let pr = self.pr.borrow();
        let p = pr.as_ref()?;
        let check = if self.now() >= p.green_at {
            json!({"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "SUCCESS"})
        } else {
            json!({"__typename": "CheckRun", "name": "ci", "status": "IN_PROGRESS"})
        };
        Some(
            json!({"number": 100, "state": p.state, "isDraft": p.draft, "baseRefName": "staging",
            "headRefName": "issue-12", "headRefOid": p.head, "mergeable": "MERGEABLE", "mergeStateStatus": "CLEAN",
            "statusCheckRollup": [check], "labels": [],
            "mergeCommit": (p.state == "MERGED").then(|| json!({"oid": "m856"}))}),
        )
    }

    fn remote(&self, host: &str) -> Option<Out> {
        let reach = self.hosts.borrow().get(host).copied()?;
        match reach {
            Reach::Down => Some(Out {
                code: 255,
                stderr: format!("ssh: connect to host {host} port 22: Operation timed out"),
                ..Out::default()
            }),
            Reach::Hang => {
                let me = PID.get();
                if me == 0 {
                    return Some(Out {
                        code: 124,
                        ..Out::default()
                    });
                }
                let (lock, cv) = &self.hung;
                let mut g = lock.lock().unwrap();
                *g = Some(me);
                cv.notify_all();
                while self.procs.borrow().contains_key(&me) {
                    g = cv.wait(g).unwrap();
                }
                drop(g);
                self.hosts.borrow_mut().remove(host);
                std::panic::resume_unwind(Box::new("killed"));
            }
        }
    }

    fn call(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        let host = match (prog, &a[..]) {
            ("herdr", ["--machine", m, ..]) => Some(*m),
            ("ssh", _) => a.iter().position(|x| *x == "--").and_then(|i| a[..i].last().copied()),
            _ => None,
        };
        if let Some(o) = host.and_then(|h| self.remote(h)) {
            self.base.calls.borrow_mut().push(format!("{prog} {}", args.join(" ")));
            return Ok(o);
        }
        let now = self.now();
        let o = match (prog, &a[..]) {
            ("herdr", ["agent", "list"]) => {
                let rows: Vec<Value> = (self.panes.borrow().iter())
                    .map(|p| json!({"name": p.agent, "agent_status": p.status, "cwd": p.cwd, "pane_id": p.pane}))
                    .collect();
                out(0, json!({"result": {"agents": rows}}).to_string())
            }
            ("herdr", ["worktree", "create" | "open", ..]) => {
                let n = self.ws.get() + 1;
                self.ws.set(n);
                out(
                    0,
                    json!({"result": {"workspace": {"workspace_id": format!("w{n}")}}}).to_string(),
                )
            }
            ("herdr", ["pane", "list", "--workspace", ws]) => out(
                0,
                json!({"result": {"panes": [{"pane_id": format!("{ws}:p1"), "agent": null}]}}).to_string(),
            ),
            ("herdr", ["agent", "start", agent, "--kind", _, "--pane", pane, ..]) => {
                let cwd = format!("/w/{agent}");
                let p = Pane {
                    agent: agent.to_string(),
                    status: "working",
                    pane: pane.to_string(),
                    cwd,
                    screen: "Reading the brief\n> ".into(),
                    born: now,
                };
                self.panes.borrow_mut().push(p);
                out(0, "{}")
            }
            ("herdr", ["pane", "read", pane, ..]) => match self.panes.borrow().iter().find(|p| p.pane == *pane) {
                Some(p) => out(0, p.screen.clone()),
                None => out(1, ""),
            },
            ("herdr", ["pane", "close", pane]) => {
                self.panes.borrow_mut().retain(|p| p.pane != *pane);
                out(0, "{}")
            }
            ("herdr", ["workspace", "close", ws]) => {
                let pre = format!("{ws}:");
                self.panes.borrow_mut().retain(|p| !p.pane.starts_with(&pre));
                out(0, "{}")
            }
            ("gh", ["api", "graphql", ..]) => {
                let open = self.pr_json().filter(|p| p["state"] == "OPEN");
                out(0, open.map(|p| format!("{p}\n")).unwrap_or_default())
            }
            ("gh", ["pr", "view", "issue-12" | "100", ..]) => match self.pr_json() {
                Some(p) => out(0, p.to_string()),
                None => Out {
                    code: 1,
                    stderr: "no pull requests found".into(),
                    ..Out::default()
                },
            },
            ("gh", ["api", "repos/jirathip-dev/sendmeter/branches/issue-12", ..]) => {
                let head = self.pr.borrow().as_ref().map(|p| p.head);
                out(
                    i32::from(head.is_none()),
                    head.map(|h| format!("{h}\n")).unwrap_or_default(),
                )
            }
            ("gh", ["pr", "ready", "100", ..]) => {
                if let Some(p) = self.pr.borrow_mut().as_mut() {
                    (p.draft, p.green_at) = (false, now + 420);
                }
                out(0, "")
            }
            ("gh", ["pr", "merge", "100", ..]) => match self.pr.borrow_mut().as_mut() {
                Some(p) if !p.draft && now >= p.green_at => {
                    p.state = "MERGED";
                    out(0, "")
                }
                _ => out(1, "Pull request is not mergeable"),
            },
            ("kill", [sig, pid]) => {
                let pid: u32 = pid.parse().unwrap_or(0);
                let live = self.procs.borrow().contains_key(&pid);
                if live && *sig != "-0" {
                    self.procs.borrow_mut().remove(&pid);
                    let _g = self.hung.0.lock().unwrap();
                    self.hung.1.notify_all();
                }
                out(i32::from(!live), "")
            }
            ("ps", ["-o", field, "-p", pid]) => {
                let procs = self.procs.borrow();
                match procs.get(&pid.parse().unwrap_or(0)) {
                    Some((cmd, _)) if *field == "command=" => out(0, format!("{cmd}\n")),
                    Some((_, at)) => {
                        let up = now - at;
                        out(0, format!("{:02}:{:02}:{:02}\n", up / 3600, up / 60 % 60, up % 60))
                    }
                    None => out(1, ""),
                }
            }
            _ => return self.base.run(prog, args),
        };
        self.base.calls.borrow_mut().push(format!("{prog} {}", args.join(" ")));
        Ok(o)
    }
}

impl Runner for World {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.call(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], _input: &str) -> anyhow::Result<Out> {
        self.call(prog, args)
    }
    fn run_for(&self, prog: &str, args: &[String], _input: &str, _limit: Duration) -> anyhow::Result<Out> {
        self.call(prog, args)
    }
    fn sleep(&self, _secs: u64) {}
}

struct Sim {
    w: Arc<World>,
    c: herdsman::Ctx,
    tick: Option<std::thread::JoinHandle<()>>,
}

impl Sim {
    fn new(name: &str, extra: &str) -> Sim {
        let base = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
        let c = ctx(&tmpdir(name), &format!("{base}{extra}"));
        std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
        assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
        Sim {
            w: World::new(),
            c,
            tick: None,
        }
    }

    fn at(&self) -> herdsman::Ctx {
        herdsman::Ctx {
            now: self.w.now(),
            ..self.c.clone()
        }
    }

    fn log(&self, kind: &str, d: Value) {
        append(&self.c.state, &Event::new(self.w.now(), "sendmeter", Some(12), kind, d)).unwrap();
    }

    fn story(&self) -> String {
        let ev = read(&self.c.state).unwrap();
        let ev = ev.iter().map(|e| format!("{} {} {:?}", e.ts, e.kind, e.data));
        let trace = self.w.trace.borrow().join("\n");
        let waiting = run(&self.at(), &*self.w, &["waiting", "sendmeter"]).1;
        format!("{trace}\n{}\n{waiting}", ev.collect::<Vec<_>>().join("\n"))
    }

    fn events(&self, kind: &str) -> Vec<Event> {
        read(&self.c.state)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == kind)
            .collect()
    }

    fn start(&mut self, args: &'static [&'static str], pid: u32) {
        let (w, c) = (self.w.clone(), self.at());
        w.procs
            .borrow_mut()
            .insert(pid, (format!("/opt/herdsman/bin/herdsman {}", args.join(" ")), c.now));
        *w.hung.0.lock().unwrap() = None;
        let h = std::thread::spawn(move || {
            PID.set(pid);
            let now = c.now;
            herdsman::sim::set(Some(herdsman::sim::Hooks {
                now: Box::new(move || now),
                pid,
                point: Box::new(|_| ()),
            }));
            let (code, o) = run(&c, &*w, args);
            w.trace.borrow_mut().push(format!("{} {args:?} {code}: {o}", c.now));
            w.ticks.borrow_mut().push((c.now, code));
            w.procs.borrow_mut().remove(&pid);
            let _g = w.hung.0.lock().unwrap();
            w.hung.1.notify_all();
        });
        let (lock, cv) = &self.w.hung;
        let mut g = lock.lock().unwrap();
        while g.is_none() && self.w.procs.borrow().contains_key(&pid) {
            g = cv.wait_timeout(g, Duration::from_millis(50)).unwrap().0;
        }
        drop(g);
        self.tick = Some(h);
    }

    fn minutes(&mut self, mins: u64, mut lane: impl FnMut(&Sim)) {
        for _ in 0..mins {
            self.w.now.set(self.w.now() + 60);
            let running = self.w.procs.borrow().contains_key(&TICK_PID);
            if (self.w.now() - T0).is_multiple_of(300) && !running {
                if let Some(h) = self.tick.take() {
                    let _ = h.join();
                }
                self.start(&["tick"], TICK_PID);
            }
            let (code, o) = run(&self.at(), &*self.w, &["drive", "--fast"]);
            assert_eq!(code, 0, "{o}");
            self.w.trace.borrow_mut().push(format!("{} fast: {o}", self.w.now()));
            lane(self);
        }
    }
}

const AIR: &str = "\n[hosts.mini]\n\n[hosts.air]\nmachine = \"air\"\n";

fn hosts_seen(s: &Sim) {
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": T0, "seen": T0, "up": true},
        "air": {"tags": ["os:macos"], "home": "/Users/me", "probed": T0, "seen": T0, "up": true},
    });
    std::fs::write(s.c.state.with_file_name("hosts.json"), cache.to_string()).unwrap();
}

#[test]
fn a_tick_hung_on_ssh_is_stopped_within_the_slo_and_the_next_tick_runs() {
    let mut s = Sim::new("sim-stuck-tick", AIR);
    hosts_seen(&s);
    s.w.hosts.borrow_mut().insert("air".into(), Reach::Hang);
    s.minutes(30, |_| {});
    let heal = s.events("self-heal");
    assert_eq!(heal.len(), 1, "{}", s.story());
    assert_eq!(heal[0].str("did"), Some("stopped tick pid 4242 (SIGTERM)"));
    let tick = s.events("slo-tick");
    let since = T0 + 300;
    assert!(
        heal[0].ts <= since + 10 * 60,
        "stopped {} s after it started",
        heal[0].ts - since
    );
    assert_eq!(tick.len(), 1, "{}", s.story());
    assert!(s.events("stuck").is_empty(), "healed: no alert");
    let ended = s.w.ticks.borrow().clone();
    assert!(
        ended.iter().any(|t| t.0 > heal[0].ts && t.1 == 0),
        "a later tick ran to its end: {ended:?}"
    );
    assert!(!s.c.state.with_file_name("tick.step").exists());
}

#[test]
fn worktrees_sharing_files_count_once_and_hold_no_spawn() {
    let mut s = Sim::new("sim-disk", "");
    let toml = std::fs::read_to_string(&s.c.config).unwrap();
    let toml = toml.replace("[limits]\n", "[limits]\nworktree_budget_gb = 0.0015\n");
    std::fs::write(&s.c.config, toml).unwrap();
    let (a, b) = (
        s.c.worktrees.join("sendmeter/issue-9"),
        s.c.worktrees.join("sendmeter/issue-10"),
    );
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    std::fs::write(a.join("target.bin"), vec![7u8; 1 << 20]).unwrap();
    std::fs::hard_link(a.join("target.bin"), b.join("target.bin")).unwrap();
    assert_eq!(run(&s.c, &Fake::default(), &["queue", "sendmeter", "add", "12"]).0, 0);
    s.minutes(10, |_| {});
    let du = std::fs::read_to_string(s.c.state.with_file_name("worktrees.du")).unwrap();
    let total: u64 = du.split_whitespace().next().unwrap().parse().unwrap();
    assert!(
        (1 << 20..3 << 19).contains(&total),
        "counted {total} bytes for 1 MiB on disk"
    );
    let held: Vec<Event> = read(&s.c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.str("code") == Some("DISK_BUDGET"))
        .collect();
    assert!(held.is_empty(), "{held:?} {}", s.story());
    assert!(
        s.events("spawn")
            .iter()
            .any(|e| e.str("agent") == Some("sendmeter-impl-12")),
        "#12 started: {}",
        s.story()
    );
}

#[test]
fn a_passed_draft_pr_is_marked_ready_and_merged() {
    let mut s = Sim::new("sim-draft", "");
    s.log(
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"}),
    );
    s.log("pushed", json!({"sha": HEAD}));
    s.log("verdict", json!({"verdict": "PASS", "sha": HEAD, "body": "PASS"}));
    *s.w.pr.borrow_mut() = Some(Pr {
        head: HEAD,
        draft: true,
        state: "OPEN",
        green_at: T0,
    });
    s.minutes(30, |_| {});
    let ready = s.events("marked-ready");
    assert_eq!(ready.len(), 1, "{}", s.story());
    let merged = s.events("merged");
    assert_eq!(merged.len(), 1, "{}", s.story());
    assert!(
        merged[0].ts >= ready[0].ts + 420,
        "merged once the full CI ran: {}",
        s.story()
    );
    assert_eq!(s.w.pr.borrow().as_ref().map(|p| p.state), Some("MERGED"));
}

#[test]
fn a_silent_reviewer_is_superseded_and_the_review_completes() {
    let mut s = Sim::new("sim-silent-rev", "");
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12", "pane": "w0:p1"});
    s.log("spawn", d);
    s.w.panes.borrow_mut().push(Pane {
        agent: "sendmeter-impl-12".into(),
        status: "idle",
        pane: "w0:p1".into(),
        cwd: "/w/sendmeter-impl-12".into(),
        screen: "pushed\n> ".into(),
        born: T0,
    });
    s.log("pushed", json!({"sha": HEAD}));
    *s.w.pr.borrow_mut() = Some(Pr {
        head: HEAD,
        draft: false,
        state: "OPEN",
        green_at: T0 + 600,
    });
    s.minutes(160, |s| {
        let Some(at) = s.events("rev-stale-replaced").first().map(|e| e.ts) else {
            return;
        };
        let panes = s.w.panes.borrow();
        let fresh = (panes.iter())
            .filter(|p| p.agent.starts_with("sendmeter-rev-12") && p.born >= at)
            .any(|p| s.w.now() == p.born + 600);
        drop(panes);
        if !fresh {
            return;
        }
        let file = s.c.worktrees.join("verdict.md");
        std::fs::write(&file, "PASS\n\nreviewed\n").unwrap();
        let f = file.to_string_lossy().to_string();
        let (code, o) = run(
            &s.at(),
            &*s.w,
            &["verdict", "--sha", HEAD, "--file", &f, "sendmeter", "12", "PASS"],
        );
        assert_eq!(code, 0, "{o}");
    });
    let replaced = s.events("rev-stale-replaced");
    assert_eq!(replaced.len(), 1, "{}", s.story());
    assert_eq!(replaced[0].data["idle_minutes"].as_u64(), Some(120), "{}", s.story());
    let v = s.events("verdict");
    assert_eq!(v.len(), 1, "{}", s.story());
    assert_eq!((v[0].str("verdict"), v[0].str("sha")), (Some("PASS"), Some(HEAD)));
}

#[test]
fn an_offline_hosts_lanes_restart_on_the_main_host_after_15_minutes() {
    let mut s = Sim::new("sim-offline", AIR);
    hosts_seen(&s);
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude", "pane": "w0:p1",
        "workspace": "w0", "worktree": "/Users/me/.herdr/worktrees/sendmeter/issue-12", "branch": "issue-12",
        "host": "air", "machine": "air"});
    s.log("spawn", d);
    s.w.hosts.borrow_mut().insert("air".into(), Reach::Down);
    s.minutes(30, |_| {});
    let moved = s.events("lane-moved");
    assert_eq!(moved.len(), 1, "{}", s.story());
    assert_eq!(moved[0].str("host"), Some("air"));
    let down = T0;
    assert!(
        (down + 15 * 60..=down + 20 * 60).contains(&moved[0].ts),
        "moved {} s after the host went down",
        moved[0].ts - down
    );
    let sp = s.events("spawn");
    assert_eq!(sp.last().unwrap().str("host"), Some("mini"));
}
