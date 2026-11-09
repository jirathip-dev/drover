//! #298: the shed runtime, against a fake shed server on a real Unix socket (shed-proto JSON-RPC lines): every
//! adapter call, a lane's spawn, the lane hook's state report, drive's event intake, ACP mode and `--on` hosts.
mod common;
use common::*;
use herdsman::events::Event;
use herdsman::herdr;
use herdsman::hosts::On;
use herdsman::runner::Runner;
use herdsman::shed::proto::{ErrorCode, Request, Response, RpcError};
use herdsman::shed::{self, Shed, Wire};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

type Terms = Arc<Mutex<BTreeMap<String, Value>>>;

/// A fake shed server: every request it got (`method`, params), its terminals, and the events `events` streams.
struct Server {
    sock: PathBuf,
    seen: Arc<Mutex<Vec<(String, Value)>>>,
    terms: Terms,
    /// #662: the kept events `events` streams (those after its `since`); a test adds more as its lanes exit.
    events: Arc<Mutex<Vec<Value>>>,
}

impl Server {
    fn new(version: &str, events: Vec<Value>) -> Server {
        static N: AtomicU32 = AtomicU32::new(0);
        let sock = PathBuf::from(format!(
            "/tmp/hsb-{}-{}.sock",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_file(&sock);
        let l = UnixListener::bind(&sock).unwrap();
        let (seen, terms) = (Arc::new(Mutex::new(vec![])), Arc::new(Mutex::new(BTreeMap::new())));
        let events = Arc::new(Mutex::new(events));
        let (s2, t2, v, e2) = (seen.clone(), terms.clone(), version.to_string(), events.clone());
        std::thread::spawn(move || {
            for c in l.incoming().flatten() {
                let (s, t, v, e) = (s2.clone(), t2.clone(), v.clone(), e2.clone());
                std::thread::spawn(move || conn(c, &s, &t, &v, &e));
            }
        });
        Server {
            sock,
            seen,
            terms,
            events,
        }
    }
    fn shed(&self) -> Shed {
        Shed {
            wire: Wire::Socket(self.sock.clone()),
            acp: None,
            policy: "ask".into(),
        }
    }
    fn methods(&self) -> Vec<String> {
        self.seen.lock().unwrap().iter().map(|s| s.0.clone()).collect()
    }
    fn last(&self, method: &str) -> Value {
        let seen = self.seen.lock().unwrap();
        seen.iter()
            .rev()
            .find(|s| s.0 == method)
            .map(|s| s.1.clone())
            .unwrap_or_default()
    }
    fn add(&self, name: &str, labels: Value) {
        let t = json!({"name": name, "labels": labels, "cwd": "/w/issue-12", "cmd": ["sh"], "keep": true,
            "state": "running", "exit": null, "pid": 7, "reported": {"state": "working"}});
        self.terms.lock().unwrap().insert(name.into(), t);
    }
}

fn conn(c: UnixStream, seen: &Mutex<Vec<(String, Value)>>, terms: &Terms, version: &str, events: &Mutex<Vec<Value>>) {
    let mut w = c.try_clone().unwrap();
    for line in BufReader::new(c).lines().map_while(Result::ok) {
        let q: Request = serde_json::from_str(&line).unwrap();
        if q.method != "hello" {
            seen.lock().unwrap().push((q.method.clone(), q.params.clone()));
        }
        let res = match q.method.as_str() {
            "hello" => Ok(json!({"product": "shed", "version": version})),
            m => handle(m, &q.params, &mut terms.lock().unwrap()),
        };
        let id = q.id.unwrap();
        let resp = res.map_or_else(|e| Response::err(id.clone(), e), |v| Response::ok(id.clone(), v));
        writeln!(w, "{}", serde_json::to_string(&resp).unwrap()).unwrap();
        if q.method == "events" {
            let since = q.params["since"].as_u64().unwrap_or(0);
            let new = events
                .lock()
                .unwrap()
                .clone()
                .into_iter()
                .filter(|e| e["seq"].as_u64() > Some(since));
            new.for_each(|e| drop(writeln!(w, "{e}")));
            std::thread::sleep(std::time::Duration::from_secs(3)); // shed never closes the stream
            return;
        }
    }
}

fn handle(m: &str, p: &Value, terms: &mut BTreeMap<String, Value>) -> Result<Value, RpcError> {
    let name = p["name"].as_str().unwrap_or_default().to_string();
    let gone = || RpcError::new(ErrorCode::NoSuchTerminal, &name);
    match m {
        "run" => {
            let t = json!({"name": name, "labels": p["labels"], "cwd": p["cwd"], "cmd": p["cmd"], "keep": p["keep"],
                "state": "running", "exit": null, "pid": 9});
            terms.insert(name, t.clone());
            Ok(t)
        }
        "ls" => Ok(json!(terms.values().collect::<Vec<_>>())),
        "events" => Ok(json!({})),
        _ if !terms.contains_key(&name) => Err(gone()),
        "get" => Ok(terms[&name].clone()),
        // #666: a terminal's own `text` (an ACP session's transcript), else a screen.
        "read" => Ok(json!({"text": terms[&name]["text"].as_str().map_or(format!("screen of {name}"), String::from)})),
        "send" | "keys" => Ok(json!({})),
        "wait" => {
            let outcome = if name == "slow" { "timeout" } else { "matched" };
            Ok(json!({"outcome": outcome, "terminal": terms[&name]}))
        }
        "kill" => {
            terms.get_mut(&name).unwrap()["state"] = json!("exited");
            Ok(terms[&name].clone())
        }
        "rm" => Ok(terms.remove(&name).unwrap()),
        "state" => {
            terms.get_mut(&name).unwrap()["reported"] = json!({"state": p["state"]});
            Ok(terms[&name].clone())
        }
        _ => Err(RpcError::new(ErrorCode::MethodNotFound, m)),
    }
}

fn lane_labels() -> Value {
    json!({"fleet": "sendmeter", "issue": "12", "role": "impl", "harness": "claude"})
}

#[test]
fn every_adapter_call_is_a_shed_request() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    b.add("sendmeter-impl-12", lane_labels());
    b.add("scratch", json!({})); // no `fleet` label: no lane
    let f = Fake::default();
    let r = shed::With(&f, b.shed());
    let lane = "sendmeter-impl-12";

    assert_eq!(herdr::read(&r, lane).unwrap(), "screen of sendmeter-impl-12");
    assert_eq!(b.last("read"), json!({"name": lane, "history": false}));
    herdr::recent(&r, lane, "200").unwrap();
    assert_eq!(b.last("read"), json!({"name": lane, "lines": 200, "history": true}));
    herdr::keys(&r, lane, "Escape").unwrap();
    assert_eq!(b.last("keys"), json!({"name": lane, "keys": ["esc"]}));
    // The page's reply: the text, then Enter (dsh types it, others prompt the agent by name).
    herdr::reply(&r, false, lane, lane, "go on").unwrap();
    assert_eq!(b.last("send"), json!({"name": lane, "text": "go on"}));
    assert_eq!(b.last("keys"), json!({"name": lane, "keys": ["Enter"]}));
    herdr::reply(&r, true, lane, lane, "typed").unwrap();
    assert_eq!(b.last("send"), json!({"name": lane, "text": "typed"}));
    assert!(herdr::landed(&r, lane, lane, "x"));
    assert_eq!(
        b.last("wait"),
        json!({"name": lane, "until": ["working", "blocked"], "timeout": 20.0})
    );
    let pane = herdr::harness_in(&r, lane).unwrap();
    assert_eq!(
        (pane["agent"].as_str(), pane["pane_id"].as_str()),
        (Some("claude"), Some(lane))
    );
    herdr::rename(&r, lane, lane).unwrap();
    let o = herdr::exec(&r, &herdr::wait_output_argv(lane, "❯", "60000")).unwrap();
    assert_eq!(o.code, 0);
    assert_eq!(
        b.last("wait"),
        json!({"name": lane, "until": [], "output": "❯", "timeout": 60.0})
    );
    herdr::ok(&r, &herdr::run_argv(lane, &["exec", "'dsh-tui'"])).unwrap();
    assert_eq!(b.last("send"), json!({"name": lane, "text": "exec 'dsh-tui'\r"}));
    // list: shed's lanes (running, `fleet`-labelled) beside herdr's, which is absent here.
    let a = herdr::agents(&r).unwrap();
    assert_eq!(a.len(), 1, "{a:?}");
    assert_eq!(
        (a[0]["name"].as_str(), a[0]["agent_status"].as_str()),
        (Some(lane), Some("working"))
    );
    assert_eq!(herdr::list(&r, "agent").unwrap()["result"]["agents"], json!(a));
    // close: kill, then rm (so the next lane may take the name); a gone terminal is closed already.
    herdr::close(&r, lane).unwrap();
    assert_eq!(b.methods().iter().rev().take(2).collect::<Vec<_>>(), ["rm", "kill"]);
    assert!(herdr::agents(&r).unwrap().is_empty());
    herdr::close(&r, lane).unwrap();
    let o = herdr::exec(&r, &herdr::read_argv(lane)).unwrap();
    assert!(herdr::failed_with(&o, "pane_not_found"), "{o:?}");
    // herdr's own panes (`ws:pane`) and workspaces never go to shed.
    assert!(herdr::read(&r, "w1:p1").is_err());
    assert!(f.called("herdr pane read w1:p1"));
    assert!(f.called("herdr agent list"));
}

#[test]
fn a_wait_that_times_out_answers_as_herdr() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    b.add("slow", lane_labels());
    let f = Fake::default();
    let r = shed::With(&f, b.shed());
    let o = herdr::exec(&r, &herdr::wait_working_argv("slow", "20000")).unwrap();
    assert!(o.code != 0 && herdr::failed_with(&o, "timeout"), "{o:?}");
}

#[test]
fn another_shed_version_is_refused() {
    let b = Server::new("9.9.9", vec![]);
    let e = shed::call(&Fake::default(), &b.shed().wire, shed::proto::Method::Ls, json!({})).unwrap_err();
    assert!(format!("{e:#}").starts_with("SHED_VERSION"), "{e:#}");
}

#[test]
fn another_host_is_reached_through_shed_connect_over_ssh() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    let hello = json!(Response::ok(json!(0), json!(shed::proto::Hello::ours())));
    let read = json!(Response::ok(json!(1), json!({"text": "remote screen"})));
    let f = Fake::default().reply("ssh", &format!("{hello}\n{read}\n"));
    let base = shed::With(&f, b.shed());
    let on = On::new(&base, Some("box"));
    assert_eq!(herdr::read(&on, "sendmeter-impl-12").unwrap(), "remote screen");
    let calls = f.calls.borrow();
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with("ssh ") && c.contains("box -- 'shed' 'connect' '--autostart'")),
        "{calls:?}"
    );
    assert!(b.methods().is_empty(), "nothing went to this host's server");
}

fn shed_config(sock: &std::path::Path, extra: &str) -> String {
    kind_config("shed", sock, extra)
}

fn kind_config(kind: &str, sock: &std::path::Path, extra: &str) -> String {
    let cfg = CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\"");
    format!(
        "{cfg}\n[runtime]\nkind = \"{kind}\"\nsocket = \"{}\"\n{extra}",
        sock.display()
    )
}

/// A shed host's spawn: no herdr there (its agent list fails), the branch missing locally.
fn spawn_fake() -> Fake {
    Fake::default()
        .once("git -C /tmp/does-not-matter/sendmeter rev-parse --verify", 1, "")
        .reply("gh issue view 12", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply("git -C", "")
        .reply("which shed", "/opt/shed/bin/shed\n")
}

#[test]
fn a_lane_spawns_on_shed() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    let d = tmpdir("shed-spawn");
    let c = ctx(&d, &shed_config(&b.sock, ""));
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let settings = std::fs::read_to_string(c.config.parent().unwrap().join("claude/fleet-impl/settings.json")).unwrap();
    let s: Value = serde_json::from_str(&settings).unwrap();
    assert!(
        s["hooks"]["UserPromptSubmit"].is_array() && s["hooks"]["Notification"].is_array(),
        "{s}"
    );

    let (code, out) = run(
        &c,
        &Fake::default().reply("which shed", "/opt/shed/bin/shed\n"),
        &["spawn", "sendmeter", "12", "impl", "--dry-run", "--json"],
    );
    assert_eq!(code, 0, "{out}");
    let plan: Value = serde_json::from_str(&out).unwrap();
    let cmds = plan["commands"].as_array().unwrap();
    assert_eq!(
        cmds[1].as_array().unwrap()[3..6],
        [json!("worktree"), json!("add"), json!("-b")]
    );
    assert_eq!((cmds[2][0].as_str(), cmds[2][1].as_str()), (Some("shed"), Some("run")));
    assert!(b.methods().is_empty(), "a dry run calls nothing");

    let f = spawn_fake();
    let (code, out) = run(&c, &f, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "{out} {:?}", f.calls.borrow());
    let wt = c.worktrees.join("sendmeter/issue-12");
    assert!(f.called(&format!(
        "git -C /tmp/does-not-matter/sendmeter worktree add -b issue-12 {} origin/staging",
        wt.display()
    )));
    assert!(
        !f.called("herdr agent start") && !f.called("herdr worktree"),
        "{:?}",
        f.calls.borrow()
    );
    let run_p = b.last("run");
    assert_eq!(run_p["name"], "sendmeter-impl-12");
    assert_eq!(run_p["labels"], lane_labels());
    assert_eq!(
        (run_p["cwd"].as_str(), run_p["keep"].as_bool()),
        (wt.to_str(), Some(true))
    );
    assert_eq!(
        run_p["cmd"],
        json!(["sh", "-c", "exec \"${SHELL:-sh}\" -l"]),
        "no secret in the terminal's argv"
    );
    // The lane's exports, then the harness, typed into its shell; the kick waits for its reported state.
    let sends: Vec<String> = b
        .seen
        .lock()
        .unwrap()
        .iter()
        .filter(|s| s.0 == "send")
        .map(|s| s.1["text"].as_str().unwrap().into())
        .collect();
    assert!(
        sends[0].starts_with("export HERDSMAN_LANE='sendmeter:12:impl'") && sends[0].ends_with('\r'),
        "{sends:?}"
    );
    assert!(
        sends[1].starts_with("exec 'claude' '--dangerously-skip-permissions'"),
        "{sends:?}"
    );
    assert_eq!(b.last("wait")["until"], json!(["working", "blocked"]));
    let ev = herdsman::events::read(&c.state)
        .unwrap()
        .into_iter()
        .rfind(|e| e.kind == "spawn")
        .unwrap();
    assert_eq!(
        (ev.str("runtime"), ev.str("pane"), ev.str("workspace")),
        (Some("shed"), Some("sendmeter-impl-12"), Some(""))
    );
    assert_eq!(ev.data["confirmed"], true);
    assert!(std::fs::read_to_string(wt.join(".brief.md"))
        .unwrap()
        .contains("AC: it works"));

    // The live lane is seen: a second spawn is refused LANE_LIVE.
    let (code, out) = run(&c, &spawn_fake(), &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("LANE_LIVE"), "{out}");
}

/// #565: the fleet piloting ACP lanes: its own `[fleet.<n>.runtime]` (no global one), a `harness` role signed in with
/// account C (`account`), #683: its default login folder (no `account_env`).
fn acp_config(sock: &std::path::Path, harness: &str, account: bool) -> String {
    let acct = if account { "\naccount = \"C\"" } else { "" };
    let cfg = CONFIG.replace(
        "harness  =  \"hermes\"    # toggled in tests",
        &format!("harness = \"{harness}\"{acct}"),
    );
    format!(
        "{cfg}\n[fleet.sendmeter.runtime]\nkind = \"shed-acp\"\nsocket = \"{}\"\n\n[accounts]\nC = \"C\"\n",
        sock.display()
    )
}

/// #683: account C's default config folder, its own `claude /login`.
fn acct_c() -> String {
    herdsman::config::home()
        .join(".config/herdsman/accounts/C")
        .display()
        .to_string()
}

const TOKEN_C: &str = concat!("sk", "-ant-oat01-CCCC-secret");

/// shed's CLI as the fake server sees it: `shed run --acp` makes a running ACP terminal of that name and, as shed's
/// bridge does, starts the adapter command after `--` at once (in the terminal's env and `--cwd`, `$SHED` set): its
/// stdout kept.
struct Cli<'a>(&'a Fake, Terms, Shared<Vec<String>>);

impl Runner for Cli<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<herdsman::runner::Out> {
        if prog == "shed" && args[0] == "run" {
            let name = &args[args.iter().position(|a| a == "--name").unwrap() + 1];
            let t = json!({"name": name, "labels": lane_labels(), "cwd": "/w", "cmd": ["shed"], "keep": false,
                "state": "running", "exit": null, "pid": 3, "acp": "claude-acp"});
            self.1.lock().unwrap().insert(name.clone(), t);
            if let Some(i) = args.iter().position(|a| a == "--") {
                let cwd = &args[args.iter().position(|a| a == "--cwd").unwrap() + 1];
                let o = std::process::Command::new(&args[i + 1])
                    .current_dir(cwd)
                    .args(&args[i + 2..])
                    .env_clear()
                    .env("PATH", std::env::var("PATH").unwrap())
                    .env("SHED", name)
                    .output()
                    .unwrap();
                self.2.borrow_mut().push(String::from_utf8(o.stdout).unwrap());
            }
        }
        self.0.run(prog, args)
    }
    fn sleep(&self, _: u64) {}
}

/// A shed-acp spawn's fake: shed's CLI answers, and the installed agent is `adapter` (`shed agents --json`).
fn acp_fake(adapter: &[&str]) -> Fake {
    let agents = json!([{"id": "claude-acp", "version": "0.101.0", "distribution": "npx", "cmd": adapter}]);
    spawn_fake()
        .reply("shed agents --json", &agents.to_string())
        .reply("shed run", "")
        .reply("shed send", "")
}

/// An ACP lane's ctx: its profiles synced, the real binary as their hook.
fn acp_ctx(name: &str, cfg: &str) -> herdsman::Ctx {
    let mut c = ctx(&tmpdir(name), cfg);
    c.exe = env!("CARGO_BIN_EXE_herdsman").into();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    c
}

#[test]
fn a_shed_acp_fleet_starts_its_lane_as_an_acp_session_with_the_brief_as_its_first_prompt() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    b.add("sendmeter-impl-12", lane_labels()); // the lane before left it exited: removed first
    b.terms.lock().unwrap().get_mut("sendmeter-impl-12").unwrap()["state"] = json!("exited");
    let c = acp_ctx("acp-spawn", &acp_config(&b.sock, "claude", true));
    let f = acp_fake(&["echo", "claude-agent-acp"]);
    let cli = Cli(&f, b.terms.clone(), Shared::default());
    let (code, out) = run(&c, &cli, &["spawn", "sendmeter", "12", "impl", "--supersede"]);
    assert_eq!(code, 0, "{out} {:?}", f.calls.borrow());
    let wt = c.worktrees.join("sendmeter/issue-12");
    let calls = f.calls.borrow();
    let start = calls.iter().find(|c| c.starts_with("shed run")).unwrap();
    let head = format!(
        "shed run --acp claude-acp --policy allow --name sendmeter-impl-12 --cwd {} --label fleet=sendmeter \
         --label issue=12 --label role=impl --label harness=claude --label source=drover --label group=sendmeter \
         --label item=#12 --label round=0 --label url=https://github.com/jirathip-dev/sendmeter/issues/12 \
         --label drover.fleet=sendmeter --label drover.issue=12 --label drover.role=impl --label drover.round=0 -- sh -c ",
        wt.display()
    );
    assert!(start.starts_with(&head), "{start}");
    assert!(start.ends_with(" sh echo claude-agent-acp"), "{start}");
    assert!(
        !start.contains(TOKEN_C) && !out.contains(TOKEN_C),
        "no token in argv: {start}"
    );
    // The brief itself is the session's first prompt; no Enter, nothing typed into a shell.
    let brief = std::fs::read_to_string(wt.join(".brief.md")).unwrap();
    assert!(brief.contains("AC: it works"));
    let sends: Vec<_> = calls.iter().filter(|c| c.starts_with("shed send")).collect();
    assert_eq!(sends, [&format!("shed send sendmeter-impl-12 {brief}")]);
    let m = b.methods();
    assert_eq!(m.iter().filter(|m| *m == "rm").count(), 1, "{m:?}");
    assert!(!m.iter().any(|m| m == "send" || m == "keys" || m == "run"), "{m:?}");
    assert_eq!(b.last("wait")["until"], json!(["working", "blocked", "done"]));
    let ev = herdsman::events::read(&c.state).unwrap();
    let sp = ev.iter().rfind(|e| e.kind == "spawn").unwrap();
    assert_eq!(
        (sp.str("pane"), sp.data["confirmed"].as_bool(), sp.str("runtime")),
        (Some("sendmeter-impl-12"), Some(true), Some("shed-acp")) // #609: the lane's runtime, as spawned
    );
    assert!(!herdsman::events::jsonl(&c.state).unwrap().contains(TOKEN_C));

    // A DECISION is a prompt through shed's CLI (the terminal is ACP; the runtime is the fleet's), with no Enter.
    drop(calls);
    let (none, s) = (Fake::default(), b.shed());
    let none = shed::With(&none, s);
    assert!(
        herdr::agents(&On::new(&none, Some("bazzite"))).is_err(),
        "neither answers: unreachable"
    );
    let cfg = herdsman::config::Config::load(&c.config).unwrap();
    let d = json!({"id": "q1", "agent": "sendmeter-impl-12"});
    let ask = Event::new(c.now, "sendmeter", Some(12), "needs-decision", d);
    herdsman::events::append(&c.state, &ask).unwrap();
    let r = shed::With(&f, cfg.shed().unwrap());
    let to = herdsman::deliver(&c, &cfg, &r, &ask, "DECISION q1: go on", false).unwrap();
    assert_eq!(to.as_deref(), Some("sendmeter-impl-12"));
    assert!(f.called("shed send sendmeter-impl-12 DECISION q1: go on"));
    assert!(
        !b.methods().iter().any(|m| m == "send" || m == "keys"),
        "{:?}",
        b.methods()
    );
}

/// A `PreToolUse` call of Claude's `Bash` tool for `cmd`, as Claude Code hands it to a hook.
fn bash_call(cmd: &str, cwd: &std::path::Path) -> String {
    json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": cmd}, "cwd": cwd})
        .to_string()
}

#[test]
fn a_guarded_command_is_refused_inside_an_acp_lane() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    let c = acp_ctx("acp-guard", &acp_config(&b.sock, "claude", true));
    // The fake ACP agent is `env`: the bridge gets back the environment the lane's adapter runs in.
    let cli = Cli(&acp_fake(&["env"]), b.terms.clone(), Shared::default());
    let (code, out) = run(&c, &cli, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "{out}");
    let dump = cli.2.borrow().concat();
    let env: BTreeMap<&str, &str> = dump.lines().filter_map(|l| l.split_once('=')).collect();
    // #683: the account's folder is its config folder (its own login); no token reaches the lane.
    assert_eq!(env.get("CLAUDE_CONFIG_DIR"), Some(&acct_c().as_str()));
    assert!(!env.contains_key("CLAUDE_CODE_OAUTH_TOKEN"), "{env:?}");
    assert!(!dump.contains(TOKEN_C));
    assert_eq!(env.get("HERDSMAN_LANE"), Some(&"sendmeter:12:impl"));
    assert_eq!(env.get("ANTHROPIC_MODEL"), Some(&"opus"));
    assert!(!env.contains_key("SHED"), "the session reports its state, not the hook");
    // As Claude Code in that adapter: the local project settings' (`<worktree>/.claude/settings.local.json`, the
    // profile's, copied there before the adapter starts) `PreToolUse` hook runs in the adapter's env before each tool,
    // and its deny stops the tool.
    let wt = c.worktrees.join("sendmeter/issue-12");
    let local = std::fs::read_to_string(wt.join(".claude/settings.local.json")).unwrap();
    let s: Value = serde_json::from_str(&local).unwrap();
    assert_eq!(s["permissions"]["defaultMode"], "bypassPermissions");
    assert!(std::fs::read_to_string(wt.join("CLAUDE.local.md"))
        .unwrap()
        .contains("herdsman"));
    let hook = s["hooks"]["PreToolUse"][0]["hooks"][0]["command"].as_str().unwrap();
    let tool = |cmd: &str| {
        let mut ch = std::process::Command::new("sh")
            .args(["-c", hook])
            .env_clear()
            .envs(env.iter().map(|(k, v)| (*k, *v)))
            .env("FLEET_CONFIG", &c.config)
            .env("FLEET_STATE", &c.state)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        ch.stdin
            .take()
            .unwrap()
            .write_all(bash_call(cmd, &wt).as_bytes())
            .unwrap();
        let o = ch.wait_with_output().unwrap();
        assert!(o.status.success());
        String::from_utf8(o.stdout).unwrap()
    };
    let kill = ["p", "kill x"].concat();
    for (cmd, why) in [
        ("git push origin staging", "staging"),
        (kill.as_str(), "NO_BROAD_KILL"),
        ("gh pr merge 3", "merge"),
    ] {
        let o = tool(cmd);
        let v: Value = serde_json::from_str(&o).unwrap_or_else(|_| panic!("{cmd}: {o}"));
        let d = &v["hookSpecificOutput"];
        assert_eq!(d["permissionDecision"], "deny", "{cmd}: {o}");
        assert!(
            d["permissionDecisionReason"].as_str().unwrap().contains(why),
            "{cmd}: {o}"
        );
    }
    assert_eq!(tool("cargo test"), "", "an ordinary command runs");
}

#[test]
fn an_unguardable_acp_lane_refuses_to_start() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    let unsynced = ctx(&tmpdir("acp-nohook"), &acp_config(&b.sock, "claude", true));
    let cases = [
        (
            acp_ctx("acp-noacct", &acp_config(&b.sock, "claude", false)),
            "[accounts] account env",
        ),
        (
            acp_ctx("acp-hermes", &acp_config(&b.sock, "hermes", true)),
            "runs no hermes hook",
        ),
        (unsynced, "no herdsman PreToolUse hook"),
    ];
    for (c, why) in cases {
        let f = acp_fake(&["env"]);
        let cli = Cli(&f, b.terms.clone(), Shared::default());
        let (code, out) = run(&c, &cli, &["spawn", "sendmeter", "12", "impl"]);
        assert_eq!(code, 3, "{out}");
        assert!(out.contains("REFUSED ACP_UNGUARDED") && out.contains(why), "{out}");
        assert!(
            !f.called("shed run") && !f.called("git"),
            "nothing started: {:?}",
            f.calls.borrow()
        );
        let (code, out) = run(&c, &f, &["spawn", "sendmeter", "12", "impl", "--dry-run"]);
        assert!(code == 3 && out.contains("ACP_UNGUARDED"), "the dry run too: {out}");
    }
    assert!(b.methods().is_empty(), "{:?}", b.methods());
}

/// #600: drive's launchd PATH had no shed: spawn named it before making anything, doctor checks the timer's PATH
/// (shed and the ACP adapter's command), and `timer install` puts the resolved shed's folder on it.
#[test]
fn a_missing_shed_is_named_by_spawn_and_doctor_and_timer_install_adds_it() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    let c = acp_ctx("shed-missing", &acp_config(&b.sock, "claude", true));
    let f = acp_fake(&["no-such-adapter-600"]).once("which shed", 1, "");
    let (code, out) = run(&c, &f, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REFUSED SHED_MISSING: shed is not on PATH ("), "{out}");
    assert!(!out.contains("os error 2"), "{out}");
    assert!(!c.worktrees.join("sendmeter/issue-12").exists(), "no worktree");
    assert!(!f.called("shed ") && !f.called("git"), "{:?}", f.calls.borrow());

    // The timer's PATH (as installed, no shed resolved) lacks shed: doctor names it, and the adapter's command.
    let agents = json!([{"id": "claude-acp", "cmd": ["no-such-adapter-600"]}]);
    let none = Fake::default().reply("shed agents --json", &agents.to_string());
    assert_eq!(run(&c, &none, &["timer", "install", "--no-load"]).0, 0);
    let (_, out) = run(&c, &none, &["doctor"]);
    assert!(out.contains("✗ shed — not on the timer's PATH → "), "{out}");
    assert!(
        out.contains("✗ shed agent claude-acp — no-such-adapter-600 not on the timer's PATH → "),
        "{out}"
    );

    // `timer install` from a shell where shed resolves puts its folder on the jobs' PATH; doctor is satisfied.
    let bin = c.home.join("cargo-bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("shed"), "").unwrap();
    let r = none.reply("which shed", &format!("{}\n", bin.join("shed").display()));
    assert_eq!(run(&c, &r, &["timer", "install", "--no-load"]).0, 0);
    let job = [
        "Library/LaunchAgents/dev.herdsman.tick.plist",
        ".config/systemd/user/dev.herdsman.tick.service",
    ];
    let text = job
        .iter()
        .find_map(|j| std::fs::read_to_string(c.home.join(j)).ok())
        .unwrap();
    let path = text.split_once("PATH").unwrap().1;
    assert!(path.contains(&format!("{}:", bin.display())), "{text}");
    let (_, out) = run(&c, &r, &["doctor"]);
    assert!(
        out.contains(&format!("✓ shed — {} on the timer's PATH", bin.join("shed").display())),
        "{out}"
    );
}

#[test]
fn an_acp_lanes_state_follows_its_turn_events() {
    let ev = |seq: u64, kind: Value| {
        let mut e = json!({"seq": seq, "name": "sendmeter-impl-12", "labels": lane_labels()});
        e.as_object_mut().unwrap().extend(kind.as_object().unwrap().clone());
        e
    };
    let events = vec![
        ev(1, json!({"event": "turn", "prompt": "# Brief"})),
        ev(2, json!({"event": "state", "state": "working"})),
        ev(
            3,
            json!({"event": "tool_call", "id": "t1", "title": "git push", "status": "pending"}),
        ),
        ev(
            4,
            json!({"event": "approval_request", "tool": "git push", "options": []}),
        ),
        ev(5, json!({"event": "state", "state": "blocked", "note": "git push"})),
        ev(6, json!({"event": "reply", "text": "done", "stop_reason": "end_turn"})),
        ev(7, json!({"event": "state", "state": "done", "note": "end_turn"})),
        ev(8, json!({"event": "exited", "code": 0})),
    ];
    let b = Server::new(shed::proto::VERSION, events);
    let c = ctx(&tmpdir("acp-state"), &acp_config(&b.sock, "claude", true));
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let cfg = herdsman::config::Config::load(&c.config).unwrap();
    assert!(cfg.runtime.shed().is_none(), "no global runtime: the fleet's own");
    let sh = cfg.shed_for("sendmeter").unwrap();
    assert_eq!((sh.acp.as_deref(), sh.policy.as_str()), (Some("claude-acp"), "allow"));
    assert_eq!(shed::intake(&c, &cfg, &cfg.shed().unwrap().wire).unwrap(), 4);
    let states: Vec<_> = herdsman::events::read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "lane-state")
        .map(|e| (e.str("state").map(String::from), e.str("note").map(String::from)))
        .collect();
    let s = |a: &str, n: Option<&str>| (Some(a.to_string()), n.map(String::from));
    assert_eq!(
        states,
        [
            s("working", None),
            s("blocked", Some("git push")),
            s("done", Some("end_turn")),
            s("gone", None)
        ]
    );
}

/// `herdsman hook` in a shed terminal (`$SHED`) on `event`: its stderr.
fn hook(sock: &std::path::Path, event: &str) -> String {
    hook_in("shed", "SHED", sock, event)
}

/// `herdsman hook` with config `kind` and only env `var` naming the terminal, on `event`: its stderr.
fn hook_in(kind: &str, var: &str, sock: &std::path::Path, event: &str) -> String {
    let d = tmpdir("shed-hook");
    std::fs::write(d.join("fleet.toml"), kind_config(kind, sock, "")).unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_herdsman"))
        .args(["hook", "--role", "impl"])
        .env("HERDSMAN_LANE", "sendmeter:-:impl") // no issue number: a turn's end kicks no real drive here
        .env_remove("SHED")
        .env_remove("BERTH")
        .env(var, "sendmeter-impl-12")
        .env("FLEET_CONFIG", d.join("fleet.toml"))
        .env("FLEET_STATE", d.join("state/events.jsonl"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let input = json!({"hook_event_name": event, "session_id": "s"}).to_string();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success());
    String::from_utf8(o.stderr).unwrap()
}

#[test]
fn the_lane_hook_reports_its_state_to_shed() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    b.add("sendmeter-impl-12", lane_labels());
    for (event, state) in [
        ("UserPromptSubmit", "working"),
        ("Notification", "blocked"),
        ("Stop", "done"),
    ] {
        hook(&b.sock, event);
        assert_eq!(
            b.last("state"),
            json!({"name": "sendmeter-impl-12", "state": state}),
            "{event}"
        );
    }
}

#[test]
fn drive_logs_sheds_state_changes_since_its_last_pass() {
    let ev = |seq: u64, name: &str, labels: Value, kind: Value| {
        let mut e = json!({"seq": seq, "name": name, "labels": labels});
        e.as_object_mut().unwrap().extend(kind.as_object().unwrap().clone());
        e
    };
    let events = vec![
        ev(
            4,
            "sendmeter-impl-12",
            lane_labels(),
            json!({"event": "state", "state": "done", "note": "end_turn"}),
        ),
        ev(5, "scratch", json!({}), json!({"event": "state", "state": "working"})),
        ev(
            6,
            "sendmeter-impl-12",
            lane_labels(),
            json!({"event": "resources", "rss_mb": 900, "cpu_pct": 50}),
        ),
        ev(
            7,
            "sendmeter-impl-12",
            lane_labels(),
            json!({"event": "exited", "code": 0}),
        ),
    ];
    let b = Server::new(shed::proto::VERSION, events);
    let d = tmpdir("shed-drive");
    let c = ctx(&d, &shed_config(&b.sock, ""));
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let cfg = herdsman::config::Config::load(&c.config).unwrap();
    assert_eq!(shed::intake(&c, &cfg, &b.shed().wire).unwrap(), 2);
    assert_eq!(b.last("events"), json!({"labels": {}, "since": 0}));
    let logged: Vec<_> = herdsman::events::read(&c.state)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind == "lane-state")
        .collect();
    let states: Vec<_> = logged
        .iter()
        .map(|e| (e.issue, e.str("state"), e.str("agent")))
        .collect();
    assert_eq!(
        states,
        [
            (Some(12), Some("done"), Some("sendmeter-impl-12")),
            (Some(12), Some("gone"), Some("sendmeter-impl-12"))
        ]
    );
    assert_eq!(
        std::fs::read_to_string(c.state.with_file_name("shed.seq")).unwrap(),
        "7"
    );
    shed::intake(&c, &cfg, &b.shed().wire).unwrap();
    assert_eq!(
        b.last("events")["since"],
        7,
        "the next pass asks only for what it has not seen"
    );
    // No server: an error for drive to print; the polled agent list still covers every lane.
    let none = Wire::Socket(d.join("none.sock"));
    assert!(shed::intake(&c, &cfg, &none).is_err());
}

#[test]
fn the_default_runtime_is_herdr() {
    let cfg = herdsman::config::Config::parse(CONFIG).unwrap();
    assert!(cfg.runtime.shed().is_none());
    assert!(Fake::default().shed().is_none());
    let bad = format!("{CONFIG}\n[runtime]\nkind = \"tmux\"\n");
    assert!(herdsman::config::Config::parse(&bad).is_err());
}

#[test]
fn another_product_at_the_same_version_is_refused() {
    // A berth server: its hello names no product.
    let hello = json!(Response::ok(json!(0), json!({"version": shed::proto::VERSION})));
    let f = Fake::default().reply("ssh", &format!("{hello}\n"));
    let on = On::new(&f, Some("box"));
    let e = shed::call(&on, &Wire::Connect, shed::proto::Method::Ls, json!({})).unwrap_err();
    assert!(
        format!("{e:#}").starts_with("SHED_VERSION") && format!("{e:#}").contains("unnamed"),
        "{e:#}"
    );
}

#[test]
fn the_old_kind_and_env_still_work_with_a_warning() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    b.add("sendmeter-impl-12", lane_labels());
    let err = hook_in("berth", "BERTH", &b.sock, "Stop");
    assert_eq!(b.last("state"), json!({"name": "sendmeter-impl-12", "state": "done"}));
    assert!(
        err.contains("kind = \"berth\" is deprecated, berth is now shed: set kind = \"shed\""),
        "{err}"
    );
    assert!(!hook(&b.sock, "Stop").contains("deprecated"));
    let d = tmpdir("shed-doctor");
    let c = ctx(&d, &kind_config("berth", &b.sock, ""));
    let (_, out) = run(&c, &Fake::default(), &["doctor"]);
    assert!(
        out.contains("! runtime — kind = \"berth\" is deprecated, berth is now shed → set kind = \"shed\" in "),
        "{out}"
    );
    let c = ctx(&tmpdir("shed-doctor-new"), &shed_config(&b.sock, ""));
    assert!(!run(&c, &Fake::default(), &["doctor"]).1.contains("! runtime"));
}

#[test]
fn drive_reads_berths_seq_file_once_when_sheds_is_missing() {
    let ev = json!({"seq": 9, "name": "sendmeter-impl-12", "labels": lane_labels(), "event": "killed"});
    let b = Server::new(shed::proto::VERSION, vec![ev]);
    let d = tmpdir("shed-old-seq");
    let c = ctx(&d, &shed_config(&b.sock, ""));
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    std::fs::write(c.state.with_file_name("berth.seq"), "8").unwrap();
    let cfg = herdsman::config::Config::load(&c.config).unwrap();
    assert_eq!(shed::intake(&c, &cfg, &b.shed().wire).unwrap(), 1);
    assert_eq!(b.last("events")["since"], 8);
    assert_eq!(
        std::fs::read_to_string(c.state.with_file_name("shed.seq")).unwrap(),
        "9"
    );
    shed::intake(&c, &cfg, &b.shed().wire).unwrap();
    assert_eq!(b.last("events")["since"], 9, "shed's file wins once it exists");
}

/// #529: the service follows shed's stream: each lane's state is logged as it comes (an exit: gone), `shed.seq`
/// kept, and the stream's end is logged as down.
#[test]
fn the_service_follows_sheds_events_and_says_when_the_stream_drops() {
    let ev = |seq: u64, kind: Value| {
        let mut e = json!({"seq": seq, "name": "sendmeter-impl-12", "labels": lane_labels()});
        e.as_object_mut().unwrap().extend(kind.as_object().unwrap().clone());
        e
    };
    let events = vec![
        ev(3, json!({"event": "state", "state": "working"})),
        ev(4, json!({"event": "exited", "code": 137})),
    ];
    let b = Server::new(shed::proto::VERSION, events);
    let d = tmpdir("shed-service");
    let c = ctx(&d, &shed_config(&b.sock, ""));
    std::fs::create_dir_all(c.state.parent().unwrap()).unwrap();
    let api = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let r: &'static Fake = Box::leak(Box::new(Fake::default().reply("herdr", "{}")));
    let sc = c.clone();
    std::thread::spawn(move || herdsman_service::run(sc, r, || 1_000_000, api, None));
    let logged = |kind: &str| -> Vec<herdsman::events::Event> {
        let all = herdsman::events::read(&c.state).unwrap_or_default();
        all.into_iter().filter(|e| e.kind == kind).collect()
    };
    let t0 = std::time::Instant::now();
    while logged("lane-state").len() < 2 {
        assert!(t0.elapsed().as_secs() < 2, "{:?}", logged("lane-state"));
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let states: Vec<_> = logged("lane-state")
        .iter()
        .map(|e| (e.issue, e.str("state").map(String::from)))
        .collect();
    assert_eq!(
        states,
        [(Some(12), Some("working".into())), (Some(12), Some("gone".into()))]
    );
    assert_eq!(logged("lane-state")[1].str("why"), Some("exited"));
    assert_eq!(
        std::fs::read_to_string(c.state.with_file_name("shed.seq")).unwrap(),
        "4"
    );
    // The fake server ends the stream after 3 s: down, with the passes on the polled list.
    while !logged("lane-stream").iter().any(|e| e.str("state") == Some("down")) {
        assert!(t0.elapsed().as_secs() < 5, "{:?}", logged("lane-stream"));
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let s: Vec<_> = logged("lane-stream")
        .iter()
        .map(|e| (e.str("runtime").map(String::from), e.str("state").map(String::from)))
        .collect();
    assert_eq!(
        s[..2],
        [
            (Some("shed".into()), Some("live".into())),
            (Some("shed".into()), Some("down".into()))
        ]
    );
}

/// #603: a fleet-only `shed-acp` runtime (the board handed a runner with no shed, as `service --board` does): the
/// Lanes tab lists shed's running lane live, with its shed state, never dead, parked or gone.
#[test]
fn the_lanes_tab_lists_a_running_shed_acp_lane_live() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    b.add("sendmeter-impl-12", lane_labels());
    let c = ctx(&tmpdir("acp-lanes-tab"), &acp_config(&b.sock, "claude", true));
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude", "runtime": "shed",
        "pane": "sendmeter-impl-12", "worktree": "/w/issue-12"});
    herdsman::events::append(&c.state, &Event::new(999_000, "sendmeter", Some(12), "spawn", d)).unwrap();
    let r: &'static Fake = Box::leak(Box::new(Fake::default().reply("herdr agent list", NO_AGENTS)));
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let sc = c.clone();
    std::thread::spawn(move || herdsman::serve::serve_on(&sc, r, l, false));
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    write!(s, "GET /api/v1/lanes HTTP/1.1\r\nHost: {addr}\r\n\r\n").unwrap();
    let mut body = String::new();
    std::io::Read::read_to_string(&mut s, &mut body).unwrap();
    let v: Value = serde_json::from_str(body.split_once("\r\n\r\n").expect(&body).1).expect(&body);
    let row = v["sections"]
        .as_array()
        .unwrap_or_else(|| panic!("{v}"))
        .iter()
        .flat_map(|s| {
            s["repos"]
                .as_array()
                .unwrap()
                .iter()
                .map(move |r| (s["name"].clone(), r))
        })
        .flat_map(|(sec, r)| {
            r["lanes"]
                .as_array()
                .unwrap()
                .iter()
                .map(move |l| (sec.clone(), l.clone()))
        })
        .find(|(_, l)| l["name"] == "sendmeter-impl-12")
        .unwrap_or_else(|| panic!("{v}"));
    assert_eq!(
        (row.0.as_str(), row.1["state"].as_str(), row.1["gone"].as_bool()),
        (Some("working"), Some("working"), Some(false)),
        "{v}"
    );
}

/// #601: with a shed runtime, a remote host whose herdr answers is up though shed is not installed there (its
/// `shed connect` over ssh fails): its herdr agents count, spawn there is not `HOST_OFFLINE`, its lanes are not
/// `host offline`, and the shed failure is one `! shed:` health row (`hosts refresh`, `doctor`).
#[test]
fn a_remote_host_without_shed_is_up_when_its_herdr_answers() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    let d = tmpdir("shed-remote-up");
    let c = ctx(
        &d,
        &shed_config(&b.sock, "\n[hosts.mini]\n\n[hosts.bazzite]\nmachine = \"bazzite\"\n"),
    );
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    let cache = json!({
        "mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": 1, "seen": 1, "up": true},
        "bazzite": {"tags": ["os:linux"], "home": "/home/me", "probed": 1, "seen": 1, "up": true},
    });
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let wt = "/home/me/.herdr/worktrees/sendmeter/issue-12";
    let ssh = "ssh -oBatchMode=yes -oConnectTimeout=10 -oControlMaster=auto -oControlPath=/tmp/herdsman-ssh-%C \
               -oControlPersist=10m bazzite -- ";
    let sp = json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude", "pane": "w9:p1",
        "workspace": "w9", "worktree": wt, "branch": "issue-12", "host": "bazzite", "machine": "bazzite"});
    herdsman::events::append(&c.state, &Event::new(c.now - 900, "sendmeter", Some(12), "spawn", sp)).unwrap();
    let f = Fake::default()
        .reply("herdr agent list", NO_AGENTS)
        .reply(
            "herdr --machine bazzite agent list",
            &agents(&[("sendmeter-impl-12", "working", wt, "w9:p1")]),
        )
        .reply("gh issue view", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply("git ", "")
        .reply(&format!("{ssh}'sh' '-c' 'uname"), "Linux\nx86_64\n/home/me\n");
    let base = shed::With(&f, b.shed());
    let on = On::new(&base, Some("bazzite"));
    assert!(on.shed().is_some(), "shed runtime on");
    let listed = herdr::agents(&on).expect("herdr answers there: the host is up");
    assert_eq!(listed[0]["name"], "sendmeter-impl-12", "{listed:?}");
    let cfg = herdsman::config::Config::load(&c.config).unwrap();
    let remote = herdsman::hosts::remote_agents(&cfg, &base);
    assert_eq!(remote.len(), 1, "its herdr agents are listed: {remote:?}");

    let (_, out) = run(&c, &f, &["spawn", "sendmeter", "13", "impl", "--host", "bazzite"]);
    assert!(!out.contains("HOST_OFFLINE"), "{out}");
    // Past placement, the offline refusal and the duplicate guard: the lane's clone runs there.
    assert!(
        f.called(&format!("{ssh}'sh' '-c' 'git clone")),
        "{out} {:?}",
        f.calls.borrow()
    );
    let (code, out) = run(&c, &f, &["hosts", "refresh"]);
    assert_eq!(code, 0, "{out}");
    assert!(!out.contains("offline") && !out.contains("unreachable"), "{out}");
    let seen: Value = serde_json::from_str(&std::fs::read_to_string(st.join("hosts.json")).unwrap()).unwrap();
    let health = seen["bazzite"]["health"].as_array().unwrap();
    let shed_rows = health
        .iter()
        .filter(|w| w.as_str().unwrap().starts_with("! shed: shed connect failed"));
    assert_eq!(
        (seen["bazzite"]["up"].as_bool(), shed_rows.count()),
        (Some(true), 1),
        "{health:?}"
    );
    for _ in 0..2 {
        assert_eq!(run(&c, &f, &["check"]).0, 0);
    }
    let evs = herdsman::events::read(&c.state).unwrap();
    assert!(!evs.iter().any(|e| e.kind == "host-offline"), "{evs:?}");
    let (_, status) = run(&c, &f, &["status"]);
    assert!(!status.contains("host offline"), "{status}");
    let (_, doc) = run(&c, &f, &["doctor"]);
    assert!(doc.contains("host bazzite shed: shed connect failed"), "{doc}");
    // #647 amendment A: a `!` row, never `✗ unreachable`.
    assert!(
        doc.contains("\n! host bazzite shed: ") && !doc.contains("unreachable"),
        "{doc}"
    );
}

/// #666: shed's CLI where a `shed send` lands: the ACP session's transcript then shows the turn.
struct Lands<'a>(&'a Fake, Terms);

impl Runner for Lands<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<herdsman::runner::Out> {
        if prog == "shed" && args[0] == "send" {
            let mut t = self.1.lock().unwrap();
            t.get_mut(&args[1]).unwrap()["text"] = json!(format!("> {}", args[2]));
        }
        self.0.run(prog, args)
    }
    fn sleep(&self, _: u64) {}
}

/// #666 amendment A: herdsman-rev-603-r2 sat 8 h, its shed session `running` with an empty transcript: the brief never
/// landed as its first prompt (the spawn's `shed send` was dropped). Past the start window drive sends it again, once.
#[test]
fn an_acp_lane_whose_first_prompt_was_dropped_gets_its_brief_again_once() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    let lane = "sendmeter-rev-12-r1";
    b.add(
        lane,
        json!({"fleet": "sendmeter", "issue": "12", "role": "rev", "harness": "claude"}),
    );
    {
        let mut terms = b.terms.lock().unwrap();
        let t = terms.get_mut(lane).unwrap();
        (t["acp"], t["reported"], t["text"]) = (json!("claude-acp"), Value::Null, json!(""));
    }
    let c = ctx(&tmpdir("acp-dropped"), &acp_config(&b.sock, "claude", true));
    let head = "a".repeat(40);
    let wt = c.worktrees.join("sendmeter/rev-12-r1");
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(wt.join(".brief.md"), "# Brief — review #12\n").unwrap();
    let ev = |ts: u64, kind: &str, d: Value| {
        herdsman::events::append(&c.state, &Event::new(ts, "sendmeter", Some(12), kind, d)).unwrap();
    };
    ev(
        10,
        "spawn",
        json!({"role": "impl", "agent": "sendmeter-impl-12", "branch": "issue-12"}),
    );
    ev(20, "pushed", json!({"sha": head}));
    let t0 = c.now - 60;
    ev(
        t0,
        "spawn",
        json!({"role": "rev", "check": true, "agent": lane, "pane": lane, "worktree": wt, "branch": "rev-12-r1",
            "sha": head, "runtime": "shed", "confirmed": true}),
    );
    let pr = json!({"number": 100, "state": "OPEN", "baseRefName": "staging", "headRefOid": head,
        "mergeable": "MERGEABLE", "statusCheckRollup": []});
    let f = Fake::default()
        .reply("gh pr view issue-12", &pr.to_string())
        .reply("git -C", "")
        .reply("shed send", "");
    let r = Lands(&f, b.terms.clone());
    let at = |s: u64| herdsman::Ctx { now: s, ..c.clone() };
    let sends = || f.count(&format!("shed send {lane}"));
    let drive = |c: &herdsman::Ctx| {
        let (code, out) = run(c, &r, &["drive"]);
        assert_eq!(code, 0, "{out} {:?}", f.calls.borrow());
        out
    };
    drive(&c);
    assert_eq!(sends(), 0, "inside the start window");
    let out = drive(&at(t0 + 601));
    assert_eq!(sends(), 1, "{out} {:?}", f.calls.borrow());
    assert!(
        f.called(&format!("shed send {lane} # Brief — review #12")),
        "the brief itself"
    );
    let ev = herdsman::events::read(&c.state).unwrap();
    assert!(
        ev.iter()
            .any(|e| e.kind == "start-resent" && e.str("agent") == Some(lane)),
        "{out}"
    );
    // It landed this time: the session works on; nothing is sent again, superseded or escalated.
    drive(&at(t0 + 1202));
    assert_eq!(sends(), 1);
    let ev = herdsman::events::read(&c.state).unwrap();
    assert!(
        !ev.iter().any(|e| matches!(e.kind.as_str(), "respawn" | "needs-human")),
        "{ev:?}"
    );
}

/// #634: over `build_budget_gb`, drive's pass never trims a running shed ACP lane's scratch (shed lists it, herdr does
/// not), nor (#630) its build folder's profiles; an exited shed lane's goes as before, its line naming why.
#[test]
fn the_build_trim_keeps_a_running_shed_lanes_scratch() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    b.add("sendmeter-impl-12", lane_labels());
    b.terms.lock().unwrap().get_mut("sendmeter-impl-12").unwrap()["acp"] = json!("claude-acp");
    let mut gone = lane_labels();
    gone["issue"] = json!("13");
    b.add("sendmeter-impl-13", gone);
    b.terms.lock().unwrap().get_mut("sendmeter-impl-13").unwrap()["state"] = json!("exited");
    let d = tmpdir("shed-trim");
    let cfg = CONFIG.replace(
        "build_root = \"\"",
        &format!("build_budget_gb = {}", 2000.0 / 1_073_741_824.0),
    );
    let c = ctx(&d, &cfg);
    let root = c.worktrees.join(".build"); // #630: the root's build folder
    let scratch = |agent: &str, n: u64, at: u64| {
        let w = c.worktrees.join("sendmeter").join(agent);
        std::fs::create_dir_all(&w).unwrap();
        let ev = json!({"role": "impl", "agent": agent, "worktree": w, "pane": agent, "workspace": agent});
        herdsman::events::append(&c.state, &Event::new(10, "sendmeter", Some(n), "spawn", ev)).unwrap();
        let p = root.join(".scratch").join(agent);
        std::fs::create_dir_all(p.join("target")).unwrap();
        std::fs::write(p.join("target/blob"), vec![0u8; 4096]).unwrap();
        let at = std::time::UNIX_EPOCH + std::time::Duration::from_secs(at);
        for f in [p.join("target/blob"), p.join("target"), p.clone()] {
            std::fs::File::open(&f).unwrap().set_modified(at).unwrap();
        }
        p
    };
    let (live, exited) = (
        scratch("sendmeter-impl-12", 12, 1_000),
        scratch("sendmeter-impl-13", 13, 2_000),
    );
    // #630: and its own build folder's profile (`<root>/.build/<agent>/<profile>`), older still.
    let profile = root.join("sendmeter-impl-12/release");
    std::fs::create_dir_all(&profile).unwrap();
    std::fs::write(profile.join(".cargo-lock"), "").unwrap();
    std::fs::write(profile.join("blob"), vec![0u8; 4096]).unwrap();
    std::fs::File::open(&profile)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(500))
        .unwrap();
    let f = Fake::default().reply("herdr agent list", NO_AGENTS);
    let r = shed::With(&f, b.shed());
    let mut out = vec![];
    let cfg = herdsman::config::Config::load(&c.config).unwrap();
    herdsman::check::budget(&c, &cfg, &r, &mut out).unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(profile.exists(), "a running lane's own build is never trimmed: {out}");
    assert!(
        live.exists(),
        "a running ACP lane's scratch is never trimmed, oldest or not: {out}"
    );
    assert!(!exited.exists(), "an exited lane's goes over budget: {out}");
    let line = format!("builds: trimmed {} ", exited.display());
    assert!(out.contains(&line) && out.contains("no live agent"), "{out}");
}

/// #662: `git` for real in `dir` (a test repo), failing loudly.
fn git(dir: &std::path::Path, a: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "init.defaultBranch=main",
            "-C",
        ])
        .arg(dir)
        .args(a)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {a:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

/// #662: an ACP lane's runner whose `git` in its worktree (a real repo, the `PathBuf`) is real; the rest is `Cli`'s.
/// #722: also `git -C` anywhere under that path.
struct Push<'a>(Cli<'a>, PathBuf);

impl Runner for Push<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<herdsman::runner::Out> {
        if prog == "git" && args.len() > 1 && args[0] == "-C" && std::path::Path::new(&args[1]).starts_with(&self.1) {
            self.0 .0.calls.borrow_mut().push(format!("git {}", args.join(" ")));
            let o = std::process::Command::new("git").args(args).output()?;
            let s = |b: Vec<u8>| String::from_utf8_lossy(&b).into_owned();
            return Ok(herdsman::runner::Out {
                code: o.status.code().unwrap_or(1),
                stdout: s(o.stdout),
                stderr: s(o.stderr),
            });
        }
        self.0.run(prog, args)
    }
    fn sleep(&self, _: u64) {}
}

/// #662: the ACP fleet with accounts C then D (#683: folders), and #12's impl lane spawned on C in a real worktree (its `origin` a
/// bare repo holding `issue-12` at the base commit): (ctx, server, fake, worktree, origin).
fn acp_lane(name: &str) -> (herdsman::Ctx, Server, Fake, PathBuf, PathBuf) {
    std::env::set_var("HERDSMAN_LANE", "sendmeter:12:impl");
    let b = Server::new(shed::proto::VERSION, vec![]);
    let cfg =
        acp_config(&b.sock, "claude", true).replace("account = \"C\"", "accounts = [\"C\", \"D\"]") + "D = \"D\"\n";
    let c = acp_ctx(name, &cfg);
    let (wt, origin) = (c.worktrees.join("sendmeter/issue-12"), c.worktrees.join("origin.git"));
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q", "--bare"]);
    git(&wt, &["init", "-q", "-b", "issue-12"]);
    git(&wt, &["remote", "add", "origin", origin.to_str().unwrap()]);
    git(&wt, &["commit", "-q", "--allow-empty", "-m", "base"]);
    git(&wt, &["push", "-q", "origin", "issue-12"]);
    let f = acp_fake(&["true"])
        .reply("sysctl -n kern.memorystatus_vm_pressure_level", "1\n")
        .reply("sysctl -n vm.loadavg", "{ 1.50 1.20 1.00 }\n")
        .reply("sysctl -n hw.ncpu", "10\n")
        .reply("vm_stat", "Pages free: 1.\nSwapouts:                     100.\n");
    let (code, out) = run(
        &c,
        &Push(Cli(&f, b.terms.clone(), Shared::default()), wt.clone()),
        &["spawn", "sendmeter", "12", "impl"],
    );
    assert_eq!(code, 0, "{out}");
    (c, b, f, wt, origin)
}

/// #662: the lane's shed session ends: its terminal exits, shed's kept events say how (`kind`s after a turn's start).
fn exits(b: &Server, kinds: &[Value]) {
    b.terms.lock().unwrap().get_mut("sendmeter-impl-12").unwrap()["state"] = json!("exited");
    let mut ev = b.events.lock().unwrap();
    let start = ev.len() as u64 * 10;
    let all = [&[json!({"event": "turn", "prompt": "# Brief"})][..], kinds].concat();
    for (i, k) in all.iter().enumerate() {
        let mut e = json!({"seq": start + i as u64 + 1, "name": "sendmeter-impl-12", "labels": lane_labels()});
        e.as_object_mut().unwrap().extend(k.as_object().unwrap().clone());
        ev.push(e);
    }
}

fn later(c: &herdsman::Ctx, secs: u64) -> herdsman::Ctx {
    herdsman::Ctx {
        now: c.now + secs,
        ..c.clone()
    }
}

fn logged(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    let all = herdsman::events::read(&c.state).unwrap();
    all.into_iter().filter(|e| e.kind == kind).collect()
}

#[test]
fn an_acp_lane_that_exits_on_an_account_limit_is_pushed_and_resumed_in_its_worktree_on_another_account() {
    let (c, b, f, wt, origin) = acp_lane("acp-resume");
    assert_eq!(logged(&c, "spawn")[0].str("account"), Some("C"));
    // The lane made two commits (the test, then the fix) and staged more, then its session exited 0 on C's limit.
    for m in ["test", "fix"] {
        git(&wt, &["commit", "-q", "--allow-empty", "-m", m]);
    }
    std::fs::write(wt.join("more.rs"), "staged").unwrap();
    git(&wt, &["add", "more.rs"]);
    let head = git(&wt, &["rev-parse", "HEAD"]);
    let limit = "Claude AI usage limit reached ∙ resets 5pm";
    exits(
        &b,
        &[
            json!({"event": "text", "text": limit}),
            json!({"event": "exited", "code": 0}),
        ],
    );
    let cli = Push(Cli(&f, b.terms.clone(), Shared::default()), wt.clone());
    let (code, out) = run(&later(&c, 300), &cli, &["drive"]);
    assert_eq!(code, 0, "{out}");
    // Its commits are on the issue branch, its staged file is not; it runs again in the same worktree, on D.
    assert_eq!(git(&origin, &["rev-parse", "issue-12"]), head, "{out}");
    assert!(git(&origin, &["ls-tree", "--name-only", "issue-12"]).is_empty());
    assert_eq!(
        git(&wt, &["diff", "--cached", "--name-only"]),
        "more.rs",
        "the staged change is kept"
    );
    let sp = logged(&c, "spawn");
    assert_eq!(sp.len(), 2, "{out}");
    assert_eq!(sp[1].str("account"), Some("D"), "{out}");
    assert_eq!(sp[1].str("worktree"), wt.to_str());
    let brief = std::fs::read_to_string(wt.join(".brief.md")).unwrap();
    assert!(brief.contains("local commits and staged changes are kept"), "{brief}");
    // It keeps exiting mid-turn (the adapter crashes): each time it is pushed and resumed, never needs-human.
    for i in 1..=3 {
        exits(&b, &[json!({"event": "exited", "code": 1})]);
        let (code, out) = run(&later(&c, 300 + i * 1900), &cli, &["drive"]);
        assert_eq!(code, 0, "{out}");
        assert_eq!(logged(&c, "spawn").len(), 2 + i as usize, "{out}");
    }
    assert!(logged(&c, "needs-human").is_empty());
    let (_, st) = run(&later(&c, 6000), &cli, &["status", "sendmeter"]);
    assert!(!st.contains("needs-human"), "{st}");
}

#[test]
fn lane_dead_and_waiting_carry_an_acp_lanes_exit_cause() {
    let (c, b, f, wt, _) = acp_lane("acp-cause");
    exits(&b, &[json!({"event": "exited", "code": 139})]);
    let cfg = herdsman::config::Config::load(&c.config).unwrap();
    let c = later(&c, 300);
    shed::intake(&c, &cfg, &cfg.shed().unwrap().wire).unwrap();
    let cli = Push(Cli(&f, b.terms.clone(), Shared::default()), wt.clone());
    let (code, out) = run(&c, &cli, &["check"]);
    assert_eq!(code, 0, "{out}");
    let dead = logged(&c, "lane-dead");
    assert_eq!(dead.len(), 1, "{out}");
    assert_eq!(dead[0].str("cause"), Some("adapter crash"));
    assert!(dead[0].str("reason").unwrap().contains("adapter crash"), "{dead:?}");
    let (_, w) = run(&c, &cli, &["waiting", "sendmeter"]);
    assert!(w.contains("lane gone (adapter crash), no push: respawning impl"), "{w}");
}

/// #608: a shed-acp fleet's thread, its conductor's events of `kind`.
fn thread_events(c: &herdsman::Ctx, kind: &str) -> Vec<Event> {
    let all = herdsman::events::read(&c.state).unwrap();
    all.into_iter().filter(|e| e.kind == kind).collect()
}

fn at(c: &herdsman::Ctx, now: u64) -> herdsman::Ctx {
    herdsman::Ctx { now, ..c.clone() }
}

#[test]
fn a_shed_acp_fleets_thread_runs_its_conductor_as_an_acp_session_that_sleeps_and_restarts() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    let c = acp_ctx("acp-conductor", &acp_config(&b.sock, "claude", true));
    assert_eq!(
        run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Grill #12"]).1,
        "t1\n"
    );
    let f = acp_fake(&["echo", "claude-agent-acp"]);
    let cli = Cli(&f, b.terms.clone(), Shared::default());
    let (code, out) = run(&c, &cli, &["thread", "say", "t1", "what should #12 cover?"]);
    assert_eq!(
        (code, out.as_str()),
        (0, "t1: -> conductor-t1\n"),
        "{:?}",
        f.calls.borrow()
    );
    let dir = c.state.with_file_name("conductor").join("t1");
    let calls = f.calls.borrow().clone();
    let start = calls.iter().find(|c| c.starts_with("shed run")).unwrap();
    let head = format!(
        "shed run --acp claude-acp --policy allow --name conductor-t1 --cwd {} --label fleet=sendmeter --label issue=- \
         --label role=conductor --label harness=claude --label source=drover --label group=sendmeter --label thread=t1 \
         -- sh -c ",
        dir.display()
    );
    assert!(start.starts_with(&head), "{start}");
    let herdr = |c: &&String| c.starts_with("herdr") && *c != "herdr agent list";
    assert!(!calls.iter().any(|c| herdr(&c)), "no herdr pane: {calls:?}");
    // The brief and the kick are the session's first prompt.
    let brief = std::fs::read_to_string(dir.join(".brief.md")).unwrap();
    let thread = dir.join(".thread.md");
    let first = format!(
        "shed send conductor-t1 {}\n\nThe thread so far is in {}, the last message new.\n\n[t1] owner: what should #12 cover?",
        brief.trim_end(),
        thread.display()
    );
    let sends: Vec<_> = calls.iter().filter(|c| c.starts_with("shed send")).collect();
    assert_eq!(sends, [&first]);
    assert!(brief.contains("Thread: t1, Grill #12"));
    let st = &thread_events(&c, "conductor-start")[0];
    assert_eq!(
        (st.str("runtime"), st.str("pane")),
        (Some("shed-acp"), Some("conductor-t1"))
    );
    assert_eq!(st.data["confirmed"], json!(true));

    // Awake: a multi-line message goes in as it is, through `shed send`.
    let f = acp_fake(&["echo"]);
    let cli = Cli(&f, b.terms.clone(), Shared::default());
    assert_eq!(
        run(&at(&c, c.now + 60), &cli, &["thread", "say", "t1", "scope\nthen tests"]).0,
        0
    );
    assert_eq!(
        f.calls
            .borrow()
            .iter()
            .filter(|c| c.starts_with("shed"))
            .collect::<Vec<_>>(),
        [&"shed send conductor-t1 [t1] owner: scope\nthen tests".to_string()]
    );
    assert!(
        !b.methods().iter().any(|m| m == "send" || m == "keys"),
        "{:?}",
        b.methods()
    );

    // Idle 30 min: the session ends (shed kills and removes it); asleep.
    let quiet = at(&c, c.now + 60 + 31 * 60);
    assert_eq!(run(&quiet, &cli, &["check"]).0, 0);
    assert_eq!(
        b.methods().iter().filter(|m| *m == "kill").count(),
        1,
        "{:?}",
        b.methods()
    );
    assert!(!b.terms.lock().unwrap().contains_key("conductor-t1"));
    let sl = thread_events(&c, "conductor-sleep");
    assert_eq!(sl.iter().map(|e| e.str("why")).collect::<Vec<_>>(), [Some("idle")]);
    let (_, ls) = run(&quiet, &Fake::default(), &["thread", "ls", "--json"]);
    assert!(ls.contains("\"conductor\": \"asleep\""), "{ls}");

    // The next message starts a fresh session (shed's bridge loads none) with the thread so far.
    let f = acp_fake(&["echo"]);
    let cli = Cli(&f, b.terms.clone(), Shared::default());
    assert_eq!(
        run(&at(&c, quiet.now + 60), &cli, &["thread", "say", "t1", "third"]).0,
        0
    );
    assert_eq!(f.count("shed run --acp claude-acp"), 1);
    let sends: Vec<_> = f
        .calls
        .borrow()
        .iter()
        .filter(|c| c.starts_with("shed send"))
        .cloned()
        .collect();
    assert!(sends.len() == 1 && sends[0].ends_with("[t1] owner: third"), "{sends:?}");
    let history = std::fs::read_to_string(&thread).unwrap();
    assert!(
        history.contains("owner: scope\nthen tests") && history.ends_with("owner: third\n"),
        "{history}"
    );
    let st = thread_events(&c, "conductor-start");
    assert_eq!((st.len(), st[1].data["resumed"].clone()), (2, json!(false)));

    // A session shed no longer runs is asleep on the next check, quiet or not.
    b.terms.lock().unwrap().remove("conductor-t1");
    assert_eq!(run(&at(&c, quiet.now + 120), &cli, &["check"]).0, 0);
    let sl = thread_events(&c, "conductor-sleep");
    assert_eq!(sl.last().and_then(|e| e.str("why")), Some("gone"));
}

#[test]
fn an_acp_conductor_runs_under_the_conductor_guard() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    let c = acp_ctx("acp-conductor-guard", &acp_config(&b.sock, "claude", true));
    run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Grill"]);
    let cli = Cli(&acp_fake(&["env"]), b.terms.clone(), Shared::default());
    assert_eq!(run(&c, &cli, &["thread", "say", "t1", "hi"]).0, 0);
    let dump = cli.2.borrow().concat();
    let env: BTreeMap<&str, &str> = dump.lines().filter_map(|l| l.split_once('=')).collect();
    assert_eq!(env.get("HERDSMAN_LANE"), Some(&"sendmeter:-:conductor"));
    assert_eq!(env.get("HERDSMAN_THREAD"), Some(&"t1"));
    assert!(!env.contains_key("CLAUDE_CODE_OAUTH_TOKEN"), "{env:?}");
    assert_eq!(env.get("CLAUDE_CONFIG_DIR"), Some(&acct_c().as_str()));
    // The profile's `PreToolUse` hook, from its session folder's local settings, run in the adapter's env as Claude
    // Code runs it.
    let cwd = c.state.with_file_name("conductor").join("t1");
    let local = std::fs::read_to_string(cwd.join(".claude/settings.local.json")).unwrap();
    let s: Value = serde_json::from_str(&local).unwrap();
    let hook = s["hooks"]["PreToolUse"][0]["hooks"][0]["command"].as_str().unwrap();
    let tool = |cmd: &str| {
        let mut ch = std::process::Command::new("sh")
            .args(["-c", hook])
            .env_clear()
            .envs(env.iter().map(|(k, v)| (*k, *v)))
            .env("FLEET_CONFIG", &c.config)
            .env("FLEET_STATE", &c.state)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        ch.stdin
            .take()
            .unwrap()
            .write_all(bash_call(cmd, &cwd).as_bytes())
            .unwrap();
        let o = ch.wait_with_output().unwrap();
        assert!(o.status.success());
        String::from_utf8(o.stdout).unwrap()
    };
    let o = tool("herdsman card approve c1 --sha abc");
    let v: Value = serde_json::from_str(&o).unwrap_or_else(|_| panic!("{o}"));
    assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "deny", "{o}");
    assert!(o.contains("REFUSED OWNER_ONLY"), "{o}");
    // A conductor's own verb (an impl lane's CONDUCTOR_ONLY) runs.
    assert_eq!(
        tool("herdsman amend sendmeter 12 'x' --basis q1"),
        "",
        "a conductor amends"
    );
}

#[test]
fn an_unguardable_acp_conductor_refuses_with_a_system_line_and_never_falls_back_to_herdr() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    let c = acp_ctx("acp-conductor-noacct", &acp_config(&b.sock, "claude", false));
    run(&c, &Fake::default(), &["thread", "new", "sendmeter", "Grill"]);
    let f = acp_fake(&["env"]);
    let cli = Cli(&f, b.terms.clone(), Shared::default());
    let (code, out) = run(&c, &cli, &["thread", "say", "t1", "hi"]);
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("REFUSED ACP_UNGUARDED") && out.contains("[accounts] account env"),
        "{out}"
    );
    let calls = f.calls.borrow();
    assert!(
        !calls
            .iter()
            .any(|c| c.starts_with("shed run") || c.starts_with("herdr w") || c.starts_with("herdr agent s")),
        "{calls:?}"
    );
    let msgs = thread_events(&c, "thread-msg");
    let sys: Vec<_> = msgs.iter().filter(|e| e.str("from") == Some("system")).collect();
    assert_eq!(sys.len(), 1, "{msgs:?}");
    assert!(sys[0].str("text").unwrap().contains("ACP_UNGUARDED"), "{:?}", sys[0]);
    assert!(thread_events(&c, "conductor-start").is_empty());
}

/// #609: `GET path` on a fresh board over runner `r` (a server per call: its lane cache starts empty).
fn board_get(c: &herdsman::Ctx, r: Fake, path: &str) -> Value {
    let r: &'static Fake = Box::leak(Box::new(r));
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let sc = c.clone();
    std::thread::spawn(move || herdsman::serve::serve_on(&sc, r, l, false));
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: {addr}\r\n\r\n").unwrap();
    let mut body = String::new();
    std::io::Read::read_to_string(&mut s, &mut body).unwrap();
    serde_json::from_str(body.split_once("\r\n\r\n").expect(&body).1).expect(&body)
}

/// #609: each named lane's `(runtime, host, where)` in `/api/v1/lanes`, `/api/v1/state` and `/herd.json`, which agree.
fn wheres(c: &herdsman::Ctx, r: impl Fn() -> Fake) -> BTreeMap<String, (String, String, String)> {
    let s = |v: &Value| v.as_str().unwrap_or("?").to_string();
    let lanes = board_get(c, r(), "/api/v1/lanes");
    let rows = lanes["sections"].as_array().unwrap_or_else(|| panic!("{lanes}"));
    let rows = rows.iter().flat_map(|s| {
        s["repos"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|r| r["lanes"].as_array().unwrap())
    });
    let got: BTreeMap<_, _> = rows
        .map(|l| (s(&l["name"]), (s(&l["runtime"]), s(&l["host"]), s(&l["where"]))))
        .collect();
    let state = board_get(c, r(), "/api/v1/state");
    for l in state["lanes"].as_array().unwrap() {
        assert_eq!(
            got[&s(&l["name"])],
            (s(&l["runtime"]), s(&l["host"]), s(&l["where"])),
            "{state}"
        );
    }
    let herd = board_get(c, r(), "/herd.json");
    for l in herd["fleets"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|f| f["lanes"].as_array().unwrap())
    {
        assert_eq!(
            (s(&l["runtime"]), s(&l["where"])),
            (got[&s(&l["name"])].0.clone(), got[&s(&l["name"])].2.clone())
        );
    }
    got
}

/// #609: a herdr lane on another host (`air`) and a shed-acp lane on the main one (`mini`): `/api/v1/lanes` and
/// `/api/v1/queue` give each its runtime, host and `where`; `where` drops the host while one host is enabled and the
/// runtime once the last herdr lane is gone (each shape is in tests/golden/api_v1/where.json).
#[test]
fn every_lane_says_where_it_runs() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    b.add("sendmeter-impl-12", lane_labels());
    let hosts = |air: bool| format!("\n[hosts.mini]\n\n[hosts.air]\nmachine = \"air\"\nenabled = {air}\n");
    let c = ctx(
        &tmpdir("where-lanes"),
        &(acp_config(&b.sock, "claude", true) + &hosts(true)),
    );
    let ev = |n, d: Value| {
        herdsman::events::append(&c.state, &Event::new(999_000, "sendmeter", Some(n), "spawn", d)).unwrap()
    };
    ev(
        12,
        json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude", "runtime": "shed-acp",
        "pane": "sendmeter-impl-12", "worktree": "/w/issue-12", "host": "mini"}),
    );
    ev(
        9,
        json!({"role": "rev", "agent": "sendmeter-rev-9", "harness": "claude", "pane": "w9:p1",
        "worktree": "/w/issue-9", "host": "air", "machine": "air"}),
    );
    let air = agents(&[("sendmeter-rev-9", "working", "/w/issue-9", "w9:p1")]);
    let both = || {
        Fake::default()
            .reply("herdr agent list", NO_AGENTS)
            .reply("herdr --machine air agent list", &air)
    };
    let shed_only = || {
        Fake::default()
            .reply("herdr agent list", NO_AGENTS)
            .reply("herdr --machine air agent list", NO_AGENTS)
    };
    let t = |rt: &str, h: &str, w: &str| (rt.to_string(), h.to_string(), w.to_string());
    let (rev, imp) = ("sendmeter-rev-9", "sendmeter-impl-12");
    let mut shapes = vec![];

    // Two hosts, both runtimes live: host and runtime.
    let got = wheres(&c, both);
    assert_eq!(
        (&got[rev], &got[imp]),
        (&t("herdr", "air", "air · herdr"), &t("shed-acp", "mini", "mini · shed"))
    );
    shapes.push(json!(got));
    let q = board_get(&c, both(), "/api/v1/queue");
    let rows: Vec<_> = q["fleets"][0]["rows"]
        .as_array()
        .unwrap_or_else(|| panic!("{q}"))
        .iter()
        .map(|r| {
            (
                r["issue"].as_u64().unwrap(),
                t(
                    r["runtime"].as_str().unwrap(),
                    r["host"].as_str().unwrap(),
                    r["where"].as_str().unwrap(),
                ),
            )
        })
        .collect();
    assert!(rows.contains(&(9, t("herdr", "air", "air · herdr"))), "{rows:?}");
    assert!(rows.contains(&(12, t("shed-acp", "mini", "mini · shed"))), "{rows:?}");

    // `air` disabled: one host enabled, so the runtime alone.
    std::fs::write(&c.config, acp_config(&b.sock, "claude", true) + &hosts(false)).unwrap();
    let got = wheres(&c, both);
    assert_eq!((&got[rev].2, &got[imp].2), (&"herdr".to_string(), &"shed".to_string()));
    shapes.push(json!(got));

    // Both hosts again, the last herdr lane ended: the host alone.
    std::fs::write(&c.config, acp_config(&b.sock, "claude", true) + &hosts(true)).unwrap();
    let got = wheres(&c, shed_only);
    assert_eq!(got[imp], t("shed-acp", "mini", "mini"));
    shapes.push(json!(got));

    // Neither varies: no label.
    std::fs::write(&c.config, acp_config(&b.sock, "claude", true) + &hosts(false)).unwrap();
    let got = wheres(&c, shed_only);
    assert_eq!(got[imp], t("shed-acp", "mini", ""));
    shapes.push(json!(got));

    // The CLI shows the same label: status's HOST column, board's row, waiting's line.
    std::fs::write(&c.config, acp_config(&b.sock, "claude", true) + &hosts(true)).unwrap();
    for cmd in ["status", "board", "waiting"] {
        let (code, out) = run(&c, &both(), &[cmd, "sendmeter"]);
        assert!(
            code == 0 && out.contains("mini · shed") && out.contains("air · herdr"),
            "{cmd}: {out}"
        );
    }

    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/api_v1/where.json");
    let fresh = serde_json::to_string_pretty(
        &json!({"route": "GET /api/v1/lanes, state, queue, herd: each lane's [runtime, host, where]",
        "shapes": shapes}),
    )
    .unwrap()
        + "\n";
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&file, &fresh).unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(&file).unwrap_or_default(),
        fresh,
        "`BLESS=1` rewrites it"
    );
}

/// #609: a lane keeps the runtime it was spawned with (a shed-acp spawn logs `shed-acp`, the test above): the board
/// still says `shed-acp` once the fleet's runtime config is gone (new lanes start in herdr).
#[test]
fn a_lane_keeps_its_spawn_runtime_after_the_config_changes() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    b.add("sendmeter-impl-12", lane_labels());
    let c = ctx(&tmpdir("acp-keeps-runtime"), &acp_config(&b.sock, "claude", true));
    let d = json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude", "runtime": "shed-acp",
        "pane": "sendmeter-impl-12", "worktree": "/w/issue-12"});
    herdsman::events::append(&c.state, &Event::new(999_000, "sendmeter", Some(12), "spawn", d)).unwrap();
    let r = || Fake::default().reply("herdr agent list", NO_AGENTS);
    assert_eq!(wheres(&c, r)["sendmeter-impl-12"].0, "shed-acp");
    std::fs::write(
        &c.config,
        CONFIG.replace("harness  =  \"hermes\"", "harness = \"claude\""),
    )
    .unwrap();
    let cfg = herdsman::config::Config::load(&c.config).unwrap();
    assert!(
        cfg.shed_for("sendmeter").is_none(),
        "the fleet's new lanes start in herdr"
    );
    let got = wheres(&c, r);
    assert_eq!(got["sendmeter-impl-12"].0, "shed-acp", "{got:?}");
}

/// #722: #12's branch checked out in its herdr-era worktree (`old`, clean and pushed) of a real `local` repo whose
/// origin is a bare repo; with `fix`, its impl and rev lanes logged and the verdict FAIL, so the next impl spawn is a fix
/// round on shed at the new root (else no lane logged it); herdr lists `live` agents in `old`: (ctx, fake, local, old,
/// origin).
fn herdr_era_lane(
    name: &str,
    sock: &std::path::Path,
    fix: bool,
    live: &[&str],
) -> (herdsman::Ctx, Fake, PathBuf, PathBuf, PathBuf) {
    let d = std::fs::canonicalize(tmpdir(name)).unwrap(); // git names worktrees by their real path
    let (local, old, origin) = (
        d.join("sendmeter"),
        d.join("herdr/sendmeter/issue-12"),
        d.join("origin.git"),
    );
    for p in [&local, &origin] {
        std::fs::create_dir_all(p).unwrap();
    }
    git(&origin, &["init", "-q", "--bare"]);
    git(&local, &["init", "-q", "-b", "staging"]);
    git(&local, &["remote", "add", "origin", origin.to_str().unwrap()]);
    git(&local, &["commit", "-q", "--allow-empty", "-m", "base"]);
    git(&local, &["push", "-q", "origin", "staging"]);
    git(
        &local,
        &["worktree", "add", "-q", "-b", "issue-12", old.to_str().unwrap()],
    );
    git(&old, &["commit", "-q", "--allow-empty", "-m", "impl"]);
    git(&old, &["push", "-q", "origin", "issue-12"]);
    let cfg = shed_config(sock, "").replace("/tmp/does-not-matter/sendmeter", local.to_str().unwrap());
    let c = ctx(&d, &cfg);
    assert_eq!(run(&c, &Fake::default(), &["profiles", "sync"]).0, 0);
    let sp = json!({"role": "impl", "agent": "sendmeter-impl-12", "harness": "claude", "pane": "w1:p1",
        "workspace": "w1", "worktree": old, "branch": "issue-12"});
    let rev = json!({"role": "rev", "agent": "sendmeter-rev-12", "harness": "claude", "pane": "w2:p1",
        "workspace": "w2", "worktree": d.join("herdr/sendmeter/rev-12-r1"), "branch": "rev-12-r1"});
    for (i, (kind, data)) in [
        ("spawn", sp),
        ("spawn", rev),
        ("verdict", json!({"verdict": "FAIL", "sha": "1".repeat(40)})),
    ]
    .into_iter()
    .enumerate()
    .filter(|_| fix)
    {
        let e = Event::new(c.now - 900 + i as u64 * 60, "sendmeter", Some(12), kind, data);
        herdsman::events::append(&c.state, &e).unwrap();
    }
    let rows: Vec<_> = (live.iter())
        .map(|n| (*n, "working", old.to_str().unwrap(), "w1:p2"))
        .collect();
    let f = Fake::default()
        .reply("gh issue view 12", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply("herdr agent list", &agents(&rows))
        .reply("which shed", "/opt/shed/bin/shed\n");
    (c, f, local, old, origin)
}

fn worktrees_of(local: &std::path::Path) -> String {
    git(local, &["worktree", "list", "--porcelain"])
}

#[test]
fn a_fix_round_moves_a_clean_herdr_era_worktree_to_the_new_root() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    let (c, f, local, old, _) = herdr_era_lane("moved-clean", &b.sock, true, &[]);
    let cli = Push(
        Cli(&f, b.terms.clone(), Shared::default()),
        local.parent().unwrap().into(),
    );
    let (code, out) = run(&c, &cli, &["spawn", "sendmeter", "12", "impl", "--supersede"]);
    assert_eq!(code, 0, "{out} {:?}", f.calls.borrow());
    let wt = c.worktrees.join("sendmeter/issue-12");
    assert_eq!(git(&wt, &["rev-parse", "--abbrev-ref", "HEAD"]), "issue-12");
    assert!(!old.exists(), "the old worktree is gone");
    assert!(
        !worktrees_of(&local).contains(old.to_str().unwrap()),
        "{}",
        worktrees_of(&local)
    );
    let moved = logged(&c, "worktree-moved");
    assert_eq!(moved.len(), 1, "{out}");
    assert_eq!(moved[0].str("old"), old.to_str());
    assert_eq!(moved[0].data["wip"], Value::Null);
}

#[test]
fn a_herdr_era_worktrees_unpushed_commit_is_saved_to_wip_before_it_moves() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    let (c, f, local, old, origin) = herdr_era_lane("moved-unpushed", &b.sock, false, &[]);
    git(&old, &["commit", "-q", "--allow-empty", "-m", "unpushed"]);
    let commit = git(&old, &["rev-parse", "HEAD"]);
    let cli = Push(
        Cli(&f, b.terms.clone(), Shared::default()),
        local.parent().unwrap().into(),
    );
    let (code, out) = run(&c, &cli, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "{out} {:?}", f.calls.borrow());
    let moved = logged(&c, "worktree-moved");
    assert_eq!(moved.len(), 1, "{out}");
    let w = moved[0].str("wip").expect("saved to wip").to_string();
    assert!(w.starts_with("wip/sendmeter-impl-12-"), "{w}");
    git(&origin, &["merge-base", "--is-ancestor", &commit, &w]);
    assert!(!old.exists(), "the old worktree is gone");
    let brief = std::fs::read_to_string(c.worktrees.join("sendmeter/issue-12/.brief.md")).unwrap();
    assert!(brief.contains(&format!("origin/{w}")), "{brief}");
}

#[test]
fn a_live_agent_in_the_herdr_era_worktree_refuses_lane_live() {
    let b = Server::new(shed::proto::VERSION, vec![]);
    let (c, f, local, old, _) = herdr_era_lane("moved-live", &b.sock, false, &["shell"]);
    let cli = Push(
        Cli(&f, b.terms.clone(), Shared::default()),
        local.parent().unwrap().into(),
    );
    let (code, out) = run(&c, &cli, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("LANE_LIVE") && out.contains(old.to_str().unwrap()),
        "{out}"
    );
    assert!(old.exists() && !c.worktrees.join("sendmeter/issue-12").exists());
    assert!(logged(&c, "worktree-moved").is_empty());
}
