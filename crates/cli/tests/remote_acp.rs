//! #637: remote hosts run lanes as shed-acp sessions with no herdr. A fake remote host behind ssh (`Boat`): its shed
//! answers `shed connect` lines and `shed run --acp`, its files are real files (written over ssh stdin), and its
//! drover is this build. Covers the survey, a remote ACP spawn through the token proxy, the guard there, placement
//! past an unguarded host, a shed-only bootstrap, and the proxy's lane keys.
mod common;
use common::*;
use drover::runner::{Out, Runner};
use drover::shed::proto::{Hello, Request, Response};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const TOKEN_C: &str = concat!("sk", "-ant-oat01-CCCC-secret");
const EXE: &str = env!("CARGO_BIN_EXE_drover");

/// One remote host: its shed's terminals, its `shed agents --json` and whether its `profiles sync` writes the hook.
#[derive(Default)]
struct Machine {
    terms: BTreeMap<String, Value>,
    agents: Value,
    hook: bool,
}

/// Remote hosts behind ssh (`machines`), the rest scripted by `f`. A remote command not handled here goes to `f` as
/// `@<machine> <argv>`.
struct Boat<'a> {
    f: &'a Fake,
    machines: Shared<BTreeMap<String, Machine>>,
    /// Each `shed run --acp`'s argv and the env its adapter ran in (the adapter is `env`).
    runs: Shared<Vec<(Vec<String>, String)>>,
    /// Where `profiles sync` there writes (this test's profiles folder: no `$HOME` in it, so the same path).
    profiles: PathBuf,
}

/// `hosts::quote`'s words back.
fn words(line: &str) -> Vec<String> {
    let (mut out, mut cur, mut q, mut any) = (vec![], String::new(), false, false);
    let mut it = line.chars();
    while let Some(c) = it.next() {
        match (q, c) {
            (true, '\'') => q = false,
            (true, c) => cur.push(c),
            (false, '\'') => (q, any) = (true, true),
            (false, '\\') => cur.extend(it.next()),
            (false, ' ') if any => (out.push(std::mem::take(&mut cur)), any = false).1,
            (false, ' ') => {}
            (false, c) => (cur.push(c), any = true).1,
        }
    }
    out.extend(any.then_some(cur));
    out
}

fn out(code: i32, stdout: &str) -> Out {
    Out {
        code,
        stdout: stdout.into(),
        stderr: String::new(),
    }
}

impl Boat<'_> {
    /// `hosts`: (machine, lists claude-acp, its `profiles sync` writes the hook).
    fn new<'a>(c: &drover::Ctx, f: &'a Fake, hosts: &[(&str, bool, bool)]) -> Boat<'a> {
        let acp = json!([{"id": "claude-acp", "version": "0.101.0", "distribution": "npx", "cmd": ["env"]}]);
        let m = hosts.iter().map(|(n, a, h)| {
            let agents = if *a { acp.clone() } else { json!([]) };
            (
                n.to_string(),
                Machine {
                    agents,
                    hook: *h,
                    ..Machine::default()
                },
            )
        });
        let b = Boat {
            f,
            machines: Shared::default(),
            runs: Shared::default(),
            profiles: c.config.with_file_name("claude"),
        };
        *b.machines.borrow_mut() = m.collect();
        b
    }
    /// The answer line to one shed request on `m`.
    fn rpc(&self, m: &str, q: &Request) -> Value {
        let mut all = self.machines.borrow_mut();
        let t = &mut all.get_mut(m).unwrap().terms;
        let name = q.params["name"].as_str().unwrap_or_default().to_string();
        let id = q.id.clone().unwrap();
        let res = match q.method.as_str() {
            "hello" => json!(Hello::ours()),
            "ls" => json!(t.values().collect::<Vec<_>>()),
            "get" | "wait" | "read" if t.contains_key(&name) => match q.method.as_str() {
                "get" => t[&name].clone(),
                "wait" => json!({"outcome": "matched", "terminal": t[&name]}),
                _ => json!({"text": "> the brief"}),
            },
            _ => {
                let e = json!({"code": -32001, "message": format!("no_such_terminal: {name}")});
                return json!({"jsonrpc": "2.0", "id": id, "error": e});
            }
        };
        json!(Response::ok(id, res))
    }
    fn remote(&self, m: &str, w: &[String], input: &str) -> anyhow::Result<Out> {
        // Its drover (this build, at its `$HOME`'s copy of the path) is this build.
        let mut w = w.to_vec();
        if w[0].ends_with("/drover") {
            w[0] = EXE.into();
        }
        let w = &w[..];
        let s: Vec<&str> = w.iter().map(String::as_str).collect();
        match s[..] {
            ["shed", "connect", ..] => {
                let lines = input
                    .lines()
                    .map(|l| self.rpc(m, &serde_json::from_str(l).unwrap()).to_string());
                Ok(out(0, &(lines.collect::<Vec<_>>().join("\n") + "\n")))
            }
            ["shed", "agents", "--json"] => Ok(out(0, &self.machines.borrow()[m].agents.to_string())),
            ["shed", "send", ..] => Ok(out(0, "")),
            ["shed", "run", ..] => {
                let name = &w[w.iter().position(|a| a == "--name").unwrap() + 1];
                let labels = json!({"fleet": "sendmeter", "issue": "12", "role": "impl", "harness": "claude"});
                let t = json!({"name": name, "labels": labels, "cwd": "/w", "cmd": ["shed"], "keep": false,
                    "state": "running", "exit": null, "pid": 3, "acp": "claude-acp"});
                let mut all = self.machines.borrow_mut();
                all.get_mut(m).unwrap().terms.insert(name.clone(), t);
                let i = w.iter().position(|a| a == "--").unwrap();
                let cwd = &w[w.iter().position(|a| a == "--cwd").unwrap() + 1];
                let o = Command::new(&w[i + 1])
                    .args(&w[i + 2..])
                    .current_dir(cwd)
                    .env_clear()
                    .env("PATH", std::env::var("PATH").unwrap())
                    .output()
                    .unwrap();
                self.runs
                    .borrow_mut()
                    .push((w.to_vec(), String::from_utf8(o.stdout).unwrap()));
                Ok(out(0, ""))
            }
            [exe, "profiles", "sync", "--profile", p, ..] if exe == EXE => {
                let hook = format!("'{EXE}' hook --role impl");
                let hooks = [json!({"matcher": "*", "hooks": [{"type": "command", "command": hook}]})];
                let hooks = if self.machines.borrow()[m].hook {
                    json!(hooks)
                } else {
                    json!([])
                };
                let s = json!({"hooks": {"PreToolUse": hooks}, "permissions": {"defaultMode": "bypassPermissions"}});
                let dir = self.profiles.join(p);
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(dir.join("settings.json"), s.to_string()).unwrap();
                std::fs::write(dir.join("role.md"), "role").unwrap();
                Ok(out(0, ""))
            }
            // Its files and its drover are real (paths under the test's own dirs).
            ["sh", "-c", script, "sh", _] if script.contains("cat >") || script.starts_with("test -e") => {
                real(w, input)
            }
            ["cat" | "rm", ..] => real(w, input),
            [exe, "--version"] if exe == EXE => real(w, input),
            _ => self.f.run_in(&format!("@{m}"), w, input),
        }
    }
}

fn real(w: &[String], input: &str) -> anyhow::Result<Out> {
    let mut ch = Command::new(&w[0])
        .args(&w[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    ch.stdin.take().unwrap().write_all(input.as_bytes())?;
    let o = ch.wait_with_output()?;
    Ok(Out {
        code: o.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&o.stdout).into(),
        stderr: String::from_utf8_lossy(&o.stderr).into(),
    })
}

impl Runner for Boat<'_> {
    fn run(&self, prog: &str, args: &[String]) -> anyhow::Result<Out> {
        self.run_in(prog, args, "")
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> anyhow::Result<Out> {
        if prog == "ssh" && args.iter().any(|a| a == "--") {
            let m = args[args.len() - 3].clone();
            self.f
                .calls
                .borrow_mut()
                .push(format!("ssh {m} {}", args.last().unwrap()));
            if self.machines.borrow().contains_key(&m) {
                return self.remote(&m, &words(args.last().unwrap()), input);
            }
        }
        self.f.run_in(prog, args, input)
    }
    fn sleep(&self, _: u64) {}
}

/// sendmeter on Claude (account C), herdr runtime; `hosts`: each a `shed-acp` remote host; mini (this one) off.
fn setup(name: &str, hosts: &[&str]) -> (drover::Ctx, PathBuf) {
    let base = CONFIG.replace(
        "harness  =  \"hermes\"    # toggled in tests",
        "harness = \"claude\"\naccount = \"C\"",
    );
    let home = tmpdir(&format!("{name}-remote"));
    let mut toml = format!(
        "{base}\n[accounts]\nC = \"CLAUDE_CODE_OAUTH_TOKEN_C\"\n\n[proxy]\nport = 9\n\n[hosts.mini]\nenabled = false\n"
    );
    let mut cache =
        json!({"mini": {"tags": ["os:macos"], "home": "/Users/me", "probed": 999_000, "seen": 999_000, "up": true}});
    for h in hosts {
        toml += &format!("\n[hosts.{h}]\nmachine = \"{h}\"\nruntime = \"shed-acp\"\n");
        cache[h] = json!({"tags": ["os:linux"], "home": home, "probed": 999_000, "seen": 999_000, "up": true,
            "load": 0.1, "mem": 12.0, "disk": 200.0});
    }
    let mut c = ctx(&tmpdir(name), &toml);
    c.exe = EXE.into();
    let st = c.state.parent().unwrap();
    std::fs::create_dir_all(st).unwrap();
    std::fs::write(st.join("hosts.json"), cache.to_string()).unwrap();
    (c, home)
}

/// A remote lane's spawn: the issue, its worktree's git there, the proxy started and the tunnel's ssh detached.
fn spawn_fake() -> Fake {
    Fake::default()
        .reply("gh issue view 12", r#"{"title":"Fix the meter","body":"AC: it works"}"#)
        .reply("@boat1 git", "")
        .reply("@boat2 git", "")
        .reply("@boat1 sh -c git", "")
        .reply("@boat2 sh -c git", "")
        .reply("@boat1 sh -c mkdir", "")
        .reply("@boat2 sh -c mkdir", "")
        .reply("sh -c nohup", "4242\n")
}

fn spawned(c: &drover::Ctx) -> Vec<drover::events::Event> {
    let ev = drover::events::read(&c.state).unwrap().to_vec();
    ev.into_iter().filter(|e| e.kind == "spawn").collect()
}

/// The lane's env as its adapter saw it (the fake adapter is `env`).
fn adapter_env(b: &Boat) -> BTreeMap<String, String> {
    let dump = b.runs.borrow().last().unwrap().1.clone();
    dump.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
}

/// The lane hook as the remote lane runs it: its env, its shipped rules file, no fleet.toml there.
fn hook_there(env: &BTreeMap<String, String>, dir: &Path, cmd: &str) -> String {
    let mut ch = Command::new(EXE);
    for k in [
        "DROVER_LANE",
        "DROVER_HOST",
        "DROVER_RULES",
        "DROVER_SCRATCH_TARGET",
        "FLEET_LANE",
    ] {
        ch.env_remove(k);
    }
    let keep = env
        .iter()
        .filter(|(k, _)| k.starts_with("DROVER_") && !["DROVER_OUTBOX", "DROVER_SERVICE"].contains(&k.as_str()));
    let mut child = ch
        .envs(keep)
        .args(["hook", "--role", "impl"])
        .env("FLEET_CONFIG", dir.join("none.toml"))
        .env("FLEET_STATE", dir.join("state/events.jsonl"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let input =
        json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "cwd": "/", "tool_input": {"command": cmd}});
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success());
    String::from_utf8(o.stdout).unwrap()
}

#[test]
fn a_shed_acp_host_with_no_herdr_is_up_and_runs_an_acp_lane_through_the_proxy_with_no_token_there() {
    let (c, home) = setup("racp-spawn", &["boat1"]);
    let f = spawn_fake();
    let b = Boat::new(&c, &f, &[("boat1", true, true)]);
    let cfg = drover::config::Config::load(&c.config).unwrap();
    // Up from its shed alone: herdr there is never asked.
    let live = drover::hosts::survey(&c, &cfg, &shed_runner(&b, &cfg), false);
    assert_eq!(live.get("boat1"), Some(&Some(vec![])), "{:?}", f.calls.borrow());
    assert!(!f.called("herdr --machine boat1"), "{:?}", f.calls.borrow());

    let (code, o) = run(&c, &b, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "{o} {:?}", f.calls.borrow());
    let sp = spawned(&c);
    assert_eq!(
        (sp[0].str("host"), sp[0].data["confirmed"].as_bool()),
        (Some("boat1"), Some(true))
    );
    let calls = f.calls.borrow().clone();
    assert!(
        !calls.iter().any(|l| l.starts_with("herdr --machine boat1")),
        "no herdr there: {calls:?}"
    );
    // An ACP session there: `shed run --acp` over ssh, then the brief as its first prompt.
    let run_at = calls
        .iter()
        .position(|l| l.starts_with("ssh boat1 'shed' 'run' '--acp' 'claude-acp'"))
        .unwrap();
    let wt = PathBuf::from(sp[0].str("worktree").unwrap());
    assert!(wt.starts_with(&home), "under its $HOME there: {}", wt.display());
    let brief = std::fs::read_to_string(wt.join(".brief.md")).unwrap();
    assert!(brief.contains("AC: it works"));
    let send = calls
        .iter()
        .position(|l| l.starts_with("ssh boat1 'shed' 'send' 'sendmeter-impl-12'"))
        .unwrap();
    assert!(send > run_at && calls[send].contains("AC: it works"), "{}", calls[send]);
    // The adapter signs in through the proxy with its lane key: no token in its env, argv, files or the log.
    let env = adapter_env(&b);
    assert_eq!(
        env.get("ANTHROPIC_BASE_URL").map(String::as_str),
        Some("http://127.0.0.1:9")
    );
    let key = env.get("ANTHROPIC_AUTH_TOKEN").unwrap();
    assert!(key.starts_with("hsk-"), "{key}");
    assert!(!env.contains_key("CLAUDE_CODE_OAUTH_TOKEN"), "{env:?}");
    let keys = drover::proxy::keys(&c);
    assert_eq!(
        (keys.len(), keys[0].agent.as_str(), keys[0].host.as_str()),
        (1, "sendmeter-impl-12", "boat1")
    );
    assert!(!std::fs::read_to_string(c.state.with_file_name("proxy-keys.json"))
        .unwrap()
        .contains(key.as_str()));
    let argv = b.runs.borrow()[0].0.join(" ");
    assert!(!argv.contains(TOKEN_C) && !argv.contains(key.as_str()), "{argv}");
    assert!(!grep(&home, TOKEN_C), "no Claude token on the remote host");
    assert!(
        !std::path::Path::new(&format!("{}.env", wt.display())).exists(),
        "secrets file removed"
    );
    assert!(!drover::events::jsonl(&c.state).unwrap().contains(TOKEN_C));
    // The proxy and the host's tunnel were started from here.
    assert!(
        calls.iter().any(|l| l.contains("nohup") && l.contains("proxy")),
        "{calls:?}"
    );
    assert!(
        calls
            .iter()
            .any(|l| l.contains("nohup 'ssh'") && l.contains("127.0.0.1:9:127.0.0.1:9") && l.contains("boat1")),
        "{calls:?}"
    );

    // A guarded command is refused inside that lane, by its hook there.
    let rules = env.get("DROVER_RULES").unwrap();
    assert!(Path::new(rules).exists(), "{rules}");
    let kill = ["p", "kill x"].concat();
    for (cmd, why) in [("git push origin staging", "staging"), (kill.as_str(), "NO_BROAD_KILL")] {
        let o = hook_there(&env, &home, cmd);
        assert!(o.contains("\"deny\"") && o.contains(why), "{cmd}: {o}");
    }
    assert_eq!(hook_there(&env, &home, "cargo test"), "");
}

/// The runner the CLI gives a fleet whose host runs shed (`shed::With`), for calls made outside `run`.
fn shed_runner<'a>(b: &'a Boat, cfg: &drover::config::Config) -> drover::shed::With<'a> {
    drover::shed::With(b, cfg.shed().unwrap())
}

fn grep(dir: &Path, needle: &str) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else { return false };
    rd.flatten().any(|e| match e.file_type().unwrap().is_dir() {
        true => grep(&e.path(), needle),
        false => std::fs::read_to_string(e.path()).is_ok_and(|s| s.contains(needle)),
    })
}

#[test]
fn a_host_whose_herdr_answers_but_whose_shed_does_not_is_offline() {
    let (c, _) = setup("racp-down", &["boat3"]);
    let f = Fake::default().reply("herdr --machine boat3 agent list", NO_AGENTS);
    let cfg = drover::config::Config::load(&c.config).unwrap();
    let r = drover::shed::With(&f, cfg.shed().unwrap());
    assert_eq!(drover::hosts::survey(&c, &cfg, &r, false).get("boat3"), Some(&None));
}

#[test]
fn a_host_missing_the_adapter_or_the_hook_refuses_acp_unguarded_and_the_lane_goes_elsewhere() {
    let (c, _) = setup("racp-place", &["boat1", "boat2"]);
    let f = spawn_fake();
    // boat1 (first by rank) lists no claude-acp; boat2 is whole.
    let b = Boat::new(&c, &f, &[("boat1", false, true), ("boat2", true, true)]);
    let (code, o) = run(&c, &b, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 3, "{o}");
    assert!(
        o.contains("REFUSED ACP_UNGUARDED") && o.contains("boat1") && o.contains("lists no claude-acp"),
        "{o}"
    );
    assert!(spawned(&c).is_empty() && drover::proxy::keys(&c).is_empty());
    assert!(b.runs.borrow().is_empty(), "nothing started");
    let (code, o) = run(&c, &b, &["spawn", "sendmeter", "12", "impl"]);
    assert_eq!(code, 0, "{o}");
    assert_eq!(spawned(&c)[0].str("host"), Some("boat2"));

    // A host whose profile there has no drover hook refuses too.
    let (c, _) = setup("racp-nohook", &["boat1"]);
    let b = Boat::new(&c, &f, &[("boat1", true, false)]);
    let (code, o) = run(&c, &b, &["spawn", "sendmeter", "12", "impl", "--host", "boat1"]);
    assert!(
        code == 3 && o.contains("ACP_UNGUARDED") && o.contains("no drover PreToolUse hook"),
        "{o}"
    );
    assert!(b.runs.borrow().is_empty());
}

#[test]
fn bootstrap_of_a_shed_acp_host_installs_shed_and_its_adapter_and_no_herdr() {
    let (c, _) = setup("racp-boot", &["boat1"]);
    // Nothing there yet: every check fails once, then passes after its install.
    let f = Fake::default()
        .reply("@boat1 sh -c uname", "Linux\nx86_64\n/home/me\n")
        .reply("@boat1 sh -c", "")
        .reply("sh -c nohup", "4242\n");
    let checks = [
        "command -v git",
        "command -v gh",
        "[ \"$(shed --version)\"",
        "shed doctor",
        "command -v node",
        "shed agents --json",
    ];
    let f = checks.iter().fold(f, |f, k| f.once(&format!("@boat1 sh -c export PATH=\"$HOME/.local/bin:$HOME/.cargo/bin:$PATH\"; apt_() {{ sudo apt-get update -qq && sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \"$@\"; }}; {k}"), 1, ""));
    let b = Boat::new(&c, &f, &[("boat1", true, true)]);
    let (code, o) = run(&c, &b, &["hosts", "bootstrap", "boat1"]);
    assert_eq!(code, 0, "{o}");
    for what in [
        "shed — installed",
        "shed job — installed",
        "node — installed",
        "claude-acp — installed",
        "git — installed",
    ] {
        assert!(o.contains(what), "{what}: {o}");
    }
    let calls = f.calls.borrow();
    let herdr = |l: &&String| {
        ["herdr --", "herdr machine", "'herdr'", "herdr status"]
            .iter()
            .any(|h| l.contains(h))
    };
    assert!(!calls.iter().any(|l| herdr(&l)), "no herdr: {calls:?}");
    let has = |s: &str| calls.iter().any(|l| l.contains(s));
    assert!(has(&format!(
        "cargo install --git https://github.com/jirathip-dev/shed --rev {}",
        drover::shed::REV
    )));
    assert!(
        has("shed install --force") && has("shed agents add claude-acp"),
        "{calls:?}"
    );
}

/// A fake Anthropic: each request's headers and body, answered `200` with a two-event stream.
fn upstream() -> (u16, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let (port, seen) = (
        l.local_addr().unwrap().port(),
        std::sync::Arc::new(std::sync::Mutex::new(vec![])),
    );
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for mut c in l.incoming().flatten() {
            let mut rd = BufReader::new(c.try_clone().unwrap());
            let (mut head, mut len) = (String::new(), 0);
            loop {
                let mut h = String::new();
                rd.read_line(&mut h).unwrap();
                if let Some(v) = h.to_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
                if h.trim().is_empty() {
                    break;
                }
                head += &h;
            }
            let mut body = vec![0; len];
            rd.read_exact(&mut body).unwrap();
            s2.lock().unwrap().push(head + &String::from_utf8(body).unwrap());
            let b = "event: message_start\ndata: {}\n\nevent: message_stop\ndata: {}\n\n";
            let _ = write!(
                c,
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{b}",
                b.len()
            );
        }
    });
    (port, seen)
}

/// One model call to the proxy at `port` with lane key `key`: the status line and body.
fn call(port: u16, key: &str) -> (String, String) {
    call_at(port, key, "/v1/messages?beta=true")
}

/// The same call to request path `path`.
fn call_at(port: u16, key: &str, path: &str) -> (String, String) {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    let body = r#"{"model":"opus","messages":[]}"#;
    write!(s, "POST {path} HTTP/1.1\r\nhost: x\r\nauthorization: Bearer {key}\r\nanthropic-beta: claude-code-20250219\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut text = String::new();
    s.read_to_string(&mut text).unwrap();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    (head.lines().next().unwrap().to_string(), body.to_string())
}

#[test]
fn a_lane_keys_model_call_goes_through_the_proxy_and_a_revoked_expired_or_ended_key_is_refused() {
    let (port, seen) = upstream();
    let (c, _) = setup("racp-proxy", &[]);
    let toml = std::fs::read_to_string(&c.config)
        .unwrap()
        .replace("port = 9", &format!("port = 9\nupstream = \"http://127.0.0.1:{port}\""));
    std::fs::write(&c.config, toml).unwrap();
    let cfg = drover::config::Config::load(&c.config).unwrap();
    // #707: account C's hand-placed 0600 token file is what the proxy signs with.
    let tf = c.config.with_file_name("tokens/C");
    std::fs::create_dir_all(tf.parent().unwrap()).unwrap();
    std::fs::write(&tf, TOKEN_C).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&tf, std::fs::Permissions::from_mode(0o600)).unwrap();
    let acct = format!("token-file:{}", tf.display());
    let mint = |agent: &str| {
        let k = drover::proxy::new_key().unwrap();
        let id = drover::proxy::mint(&c, &cfg, &k, ("sendmeter", 12, agent), ("boat1", acct.as_str())).unwrap();
        (k, id)
    };
    let ((live, _), (gone, gone_id), (late, _)) = (
        mint("sendmeter-impl-12"),
        mint("sendmeter-rev-12"),
        mint("sendmeter-fix-12"),
    );
    let f = Fake::default();
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let pport = l.local_addr().unwrap().port();
    // The proxy runs until the test binary ends: its state leaked for its thread.
    let now: &'static Shared<u64> = Box::leak(Box::new(Shared::new(c.now)));
    let dir = c.config.parent().unwrap().parent().unwrap();
    let pc: &'static drover::Ctx = Box::leak(Box::new(ctx(dir, &std::fs::read_to_string(&c.config).unwrap())));
    let pcfg: &'static drover::config::Config = Box::leak(Box::new(drover::config::Config::load(&c.config).unwrap()));
    let pf: &'static Fake = Box::leak(Box::new(f));
    let f = pf;
    std::thread::spawn(move || drover::proxy::serve_on(pc, pcfg, pf, &l, &|| now.get()));
    {
        let (status, body) = call(pport, &live);
        assert!(status.contains("200"), "{status} {body}");
        assert!(body.contains("message_stop"), "{body}");
        let up = seen.lock().unwrap().clone();
        assert_eq!(up.len(), 1);
        let h = up[0].to_lowercase();
        assert!(
            h.contains(&format!("authorization: bearer {}", TOKEN_C.to_lowercase())),
            "{h}"
        );
        assert!(
            h.contains("anthropic-beta: claude-code-20250219,oauth-2025-04-20"),
            "{h}"
        );
        assert!(
            !h.contains(&live) && up[0].ends_with(r#"{"model":"opus","messages":[]}"#),
            "{h}"
        );
        // A live key with a path that would move curl's host off `upstream` (`@host/`, no leading `/`, ...): refused.
        for p in [
            "@127.0.0.1:9/v1",
            "/v1@127.0.0.1:9",
            "\\@127.0.0.1:9/",
            "/v1#@x",
            "v1/messages",
        ] {
            let (status, body) = call_at(pport, &live, p);
            assert!(
                status.contains("400") && body.contains("bad request path"),
                "{p}: {status} {body}"
            );
        }
        assert_eq!(seen.lock().unwrap().len(), 1, "a hostile path never reaches upstream");
        // Revoked by the owner, unknown, and (past `key_hours`) expired: refused here, nothing goes upstream.
        assert_eq!(run(&c, f, &["hosts", "keys", "revoke", &gone_id]).0, 0);
        let (code, list) = run(&c, f, &["hosts", "keys"]);
        assert!(
            code == 0 && list.contains(&format!("{gone_id}  revoked sendmeter-rev-12 on boat1")),
            "{list}"
        );
        for (k, why) in [(gone.as_str(), "revoked"), ("hsk-nope", "unknown lane key")] {
            let (status, body) = call(pport, k);
            assert!(status.contains("401") && body.contains(why), "{status} {body}");
        }
        now.set(c.now + 12 * 3600);
        let (status, body) = call(pport, &late);
        assert!(status.contains("401") && body.contains("expired"), "{status} {body}");
        assert_eq!(seen.lock().unwrap().len(), 1, "refused calls never reach upstream");
    }
}

#[test]
fn a_remote_acp_lane_whose_role_has_no_account_is_refused_acp_unguarded() {
    let (c, _) = setup("racp-noacct", &["boat1"]);
    let toml = std::fs::read_to_string(&c.config).unwrap();
    std::fs::write(&c.config, toml.replacen("account = \"C\"", "", 1)).unwrap();
    let f = spawn_fake();
    let b = Boat::new(&c, &f, &[("boat1", true, true)]);
    let (code, o) = run(&c, &b, &["spawn", "sendmeter", "12", "impl", "--host", "boat1"]);
    assert!(
        code == 3 && o.contains("REFUSED ACP_UNGUARDED") && o.contains("[accounts] account"),
        "{o}"
    );
    assert!(spawned(&c).is_empty() && drover::proxy::keys(&c).is_empty());
    assert!(b.runs.borrow().is_empty(), "nothing started");
}
