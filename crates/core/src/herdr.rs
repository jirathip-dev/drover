//! The one herdr adapter (herdr ≥ 0.9.0, docs/setup.md): every `herdr` argv is built here. A `*_argv`
//! builder is for a command shown in a dry-run plan as well as run; the other functions run one operation.
//! #298: herdr's words are the lane runtime's interface (`Runtime`); shed (`[runtime] kind = "shed"`) answers them
//! too, for the lanes it runs (a pane without herdr's `:`), over `crate::shed`'s wire.
use crate::runner::{argv, Out, Runner};
use crate::shed;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use shed_proto::{KeysParams, LsParams, Method, ReadParams, RunParams, SendParams, Target, TerminalInfo, WaitParams};
use std::path::Path;

/// The program name, for PATH lookups and the Services row.
pub const BIN: &str = "herdr";

/// #298: a terminal runtime, spoken to in herdr's words (`*_argv`) and answered as herdr answers (its JSON, its
/// `{"error": {"code"}}`s).
pub trait Runtime {
    /// Run one herdr argv; the caller reads the exit code.
    fn exec(&self, r: &dyn Runner, a: &[String]) -> Result<Out>;
    /// Its live agents, as `herdr agent list` shows them.
    fn agents(&self, r: &dyn Runner) -> Result<Vec<Value>>;
}

pub struct Herdr;

impl Runtime for Herdr {
    fn exec(&self, r: &dyn Runner, a: &[String]) -> Result<Out> {
        r.run("herdr", a)
    }
    fn agents(&self, r: &dyn Runner) -> Result<Vec<Value>> {
        let v: Value =
            serde_json::from_str(&ok_on(self, r, &argv(&["agent", "list"]))?).context("herdr agent list: not JSON")?;
        Ok(v["result"]["agents"].as_array().cloned().unwrap_or_default())
    }
}

/// #298: shed's lanes: terminals named as their agents, labelled `fleet`, `issue`, `role` and `harness`.
pub struct Shed(pub shed::Shed);

impl Shed {
    fn call(&self, r: &dyn Runner, m: Method, params: impl serde::Serialize) -> Result<Value> {
        shed::call(r, &self.0.wire, m, json!(params))
    }
    /// shed's terminal `name`.
    fn get(&self, r: &dyn Runner, name: &str) -> Result<TerminalInfo> {
        Ok(serde_json::from_value(self.call(
            r,
            Method::Get,
            Target { name: name.into() },
        )?)?)
    }
    /// Whether herdr argv `a` is for one of shed's terminals: a pane without herdr's `ws:pane` colon, an agent shed runs.
    fn mine(&self, r: &dyn Runner, a: &[String]) -> bool {
        let t = a.get(2).map_or("", String::as_str);
        match (a[0].as_str(), request(a)) {
            (_, None) => false,
            ("agent", Some((Method::Send | Method::Wait, _))) => self.get(r, t).is_ok(),
            _ => !t.contains(':'),
        }
    }
    /// `run` params for lane `name` in `cwd`: a kept terminal of the user's login shell, which `typed` lines then drive.
    pub fn run_params(name: &str, labels: &[(&str, String)], cwd: &str) -> RunParams {
        let labels = labels.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect();
        let cmd = argv(&["sh", "-c", "exec \"${SHELL:-sh}\" -l"]);
        RunParams {
            name: Some(name.into()),
            labels,
            cwd: cwd.into(),
            keep: true,
            idle_after: None,
            cmd,
            acp: None,
        }
    }
    /// Start lane `name` (`run_params`), or with `[runtime] acp` that ACP agent (shed's CLI makes its bridge). A
    /// terminal of that name an earlier lane left exited is removed first.
    pub fn start(&self, r: &dyn Runner, name: &str, labels: &[(&str, String)], cwd: &str) -> Result<()> {
        if self.get(r, name).is_ok_and(|t| t.state == shed_proto::State::Exited) {
            self.call(r, Method::Rm, Target { name: name.into() })?;
        }
        let Some(id) = &self.0.acp else {
            return self.call(r, Method::Run, Self::run_params(name, labels, cwd)).map(drop);
        };
        let mut a = vec![
            "run",
            "--acp",
            id,
            "--policy",
            &self.0.policy,
            "--name",
            name,
            "--cwd",
            cwd,
        ];
        let kv: Vec<String> = labels.iter().map(|(k, v)| format!("{k}={v}")).collect();
        a.extend(kv.iter().flat_map(|l| ["--label", l.as_str()]));
        shed::cli(r, &a).map(drop)
    }
    /// The ACP lane's kick: once its session is ready, the prompt (framed by shed's CLI), confirmed by its state.
    pub fn kick_acp(&self, r: &dyn Runner, name: &str, text: &str) -> Result<bool> {
        let ready = WaitParams {
            name: name.into(),
            until: vec![],
            output: Some("ACP session ready".into()),
            timeout: Some(60.0),
        };
        if self.call(r, Method::Wait, ready)?["outcome"] != "matched" {
            return Ok(false);
        }
        shed::cli(r, &["send", name, text])?;
        let up = WaitParams {
            name: name.into(),
            until: argv(&["working", "blocked", "done"]),
            output: None,
            timeout: Some(20.0),
        };
        Ok(self.call(r, Method::Wait, up)?["outcome"] == "matched")
    }
}

/// herdr's agent (`agent list`) or pane (`pane get`) object for shed terminal `t`.
fn agent_of(t: &TerminalInfo) -> Value {
    let up = t.state == shed_proto::State::Running;
    json!({"name": t.name, "pane_id": t.name, "cwd": t.cwd, "agent": t.labels.get("harness").filter(|_| up),
        "agent_status": t.reported.as_ref().map_or("unknown", |s| s.state.as_str()), "terminal_title": "", "runtime": "shed"})
}

/// The shed call that does what herdr argv `a` does; None: herdr only (workspaces, worktrees, `agent start`).
fn request(a: &[String]) -> Option<(Method, Value)> {
    let w: Vec<&str> = a.iter().map(String::as_str).collect();
    let flag = |f: &str| w.windows(2).find(|p| p[0] == f).map(|p| p[1]);
    let secs = flag("--timeout")
        .and_then(|ms| ms.parse::<f64>().ok())
        .map(|ms| ms / 1000.0);
    Some(match w[..] {
        ["pane", "read", t, ..] => {
            let lines = flag("--lines").and_then(|n| n.parse().ok());
            (
                Method::Read,
                json!(ReadParams {
                    name: t.into(),
                    lines,
                    history: lines.is_some()
                }),
            )
        }
        ["pane", "send-keys", t, k] => {
            let k = if k == "Escape" { "esc" } else { k };
            (
                Method::Keys,
                json!(KeysParams {
                    name: t.into(),
                    keys: vec![k.into()]
                }),
            )
        }
        ["agent", "prompt", t, text] | ["pane", "send-text", t, text] => (
            Method::Send,
            json!(SendParams {
                name: t.into(),
                text: text.into()
            }),
        ),
        ["pane", "run", t, ref line @ ..] => (
            Method::Send,
            json!(SendParams {
                name: t.into(),
                text: line.join(" ") + "\r"
            }),
        ),
        ["agent", "wait", t, ..] => {
            let until = w
                .windows(2)
                .filter(|p| p[0] == "--until")
                .map(|p| p[1].to_string())
                .collect();
            (
                Method::Wait,
                json!(WaitParams {
                    name: t.into(),
                    until,
                    output: None,
                    timeout: secs
                }),
            )
        }
        ["pane", "wait-output", t, ..] => {
            let output = flag("--match").map(String::from);
            (
                Method::Wait,
                json!(WaitParams {
                    name: t.into(),
                    until: vec![],
                    output,
                    timeout: secs
                }),
            )
        }
        ["pane", "close", t] => (Method::Kill, json!(Target { name: t.into() })),
        ["pane", "get", t] | ["agent", "rename", t, _] => (Method::Get, json!(Target { name: t.into() })),
        _ => return None,
    })
}

/// herdr's failure answer: exit 1, `{"error": {"code", "message"}}` on stderr.
fn failed(code: &str, message: &str) -> Out {
    let stderr = json!({"error": {"code": code, "message": message}}).to_string();
    Out {
        code: 1,
        stdout: String::new(),
        stderr,
    }
}

impl Runtime for Shed {
    fn exec(&self, r: &dyn Runner, a: &[String]) -> Result<Out> {
        let Some((m, params)) = request(a) else {
            return Ok(failed("unsupported", &format!("shed runs no `herdr {}`", a.join(" "))));
        };
        let name = params["name"].as_str().unwrap_or_default().to_string();
        let acp = self.0.acp.is_some();
        let got = match m {
            // ACP: a prompt is framed by shed's CLI, and it needs no Enter.
            Method::Send if acp => {
                shed::cli(r, &["send", &name, params["text"].as_str().unwrap_or_default()]).map(|_| json!({}))
            }
            Method::Keys
                if acp
                    && params["keys"][0]
                        .as_str()
                        .is_some_and(|k| k.eq_ignore_ascii_case("enter")) =>
            {
                Ok(json!({}))
            }
            Method::Kill => self.call(r, m, &params).and_then(|_| self.call(r, Method::Rm, &params)),
            _ => self.call(r, m, &params),
        };
        let v = match got {
            Ok(v) => v,
            Err(e) if format!("{e:#}").starts_with("NO_SUCH_TERMINAL") => {
                return Ok(failed("pane_not_found", &format!("{e:#}")))
            }
            Err(e) => return Ok(failed("shed", &format!("{e:#}"))),
        };
        let stdout = match m {
            Method::Read => v["text"].as_str().unwrap_or_default().to_string(),
            Method::Get => json!({"result": {"pane": agent_of(&serde_json::from_value(v)?)}}).to_string(),
            Method::Wait if v["outcome"] != "matched" => {
                let code = if v["outcome"] == "timeout" {
                    "timeout"
                } else {
                    "agent_exited"
                };
                return Ok(failed(code, &format!("shed wait {name}: {}", v["outcome"])));
            }
            _ => v.to_string(),
        };
        Ok(Out {
            code: 0,
            stdout,
            stderr: String::new(),
        })
    }
    fn agents(&self, r: &dyn Runner) -> Result<Vec<Value>> {
        let all: Vec<TerminalInfo> = serde_json::from_value(self.call(r, Method::Ls, LsParams::default())?)?;
        let lanes = all
            .iter()
            .filter(|t| t.state == shed_proto::State::Running && t.labels.contains_key("fleet"));
        Ok(lanes.map(agent_of).collect())
    }
}

/// #298: a harness launch (`start_cmd`) as the line a shed lane's shell runs (`pane run`): `agent start … --kind <k>
/// … -- <args>` as `exec <k> <args>`, a typed line as `exec <line>`.
pub fn typed(pane: &str, start: &[String]) -> Vec<String> {
    let w: Vec<&str> = start.iter().map(String::as_str).collect();
    let line = match w[..] {
        ["agent", "start", ..] => {
            let kind = w.windows(2).find(|p| p[0] == "--kind").map_or("", |p| p[1]);
            let args = w.iter().position(|a| *a == "--").map_or(&[][..], |i| &start[i + 1..]);
            crate::hosts::quote(kind, args)
        }
        _ => start.get(3..).unwrap_or_default().join(" "),
    };
    run_argv(pane, &["exec", &line])
}

/// #298: herdr argv `a` as the shed request a plan shows (`shed <method> <params>`); `a` itself when shed runs none.
pub fn shed_line(a: &[String]) -> Vec<String> {
    request(a).map_or_else(|| line(a.to_vec()), |(m, v)| argv(&["shed", m.name(), &v.to_string()]))
}

/// The runtime herdr argv `a` goes to on `r`: shed for its lanes, else herdr.
fn runtime(r: &dyn Runner, a: &[String]) -> Box<dyn Runtime> {
    match r.shed().map(Shed).filter(|b| b.mine(r, a)) {
        Some(b) => Box::new(b),
        None => Box::new(Herdr),
    }
}

/// Run a herdr argv built here (`*_argv`) on its lane's runtime; the caller reads the exit code.
pub fn exec(r: &dyn Runner, a: &[String]) -> Result<Out> {
    runtime(r, a).exec(r, a)
}

/// The full command line of a herdr argv, as a dry-run plan shows it.
pub fn line(a: Vec<String>) -> Vec<String> {
    std::iter::once("herdr".to_string()).chain(a).collect()
}

/// `rt.exec`, failing on a non-zero exit; returns stdout.
fn ok_on(rt: &dyn Runtime, r: &dyn Runner, a: &[String]) -> Result<String> {
    let o = rt.exec(r, a)?;
    if o.code != 0 {
        bail!("herdr {} failed ({}): {}", a.join(" "), o.code, o.stderr.trim());
    }
    Ok(o.stdout)
}

/// `exec`, failing on a non-zero exit; returns stdout.
pub fn ok(r: &dyn Runner, a: &[String]) -> Result<String> {
    ok_on(&*runtime(r, a), r, a)
}

/// `exec` as JSON, failing on a non-zero exit.
pub fn json(r: &dyn Runner, a: &[String]) -> Result<Value> {
    serde_json::from_str(&ok(r, a)?).with_context(|| format!("herdr {}: not JSON", a.join(" ")))
}

/// `herdr status`: client and server versions (`herdsman doctor`).
pub fn status(r: &dyn Runner) -> Result<String> {
    ok(r, &argv(&["status"]))
}

/// `herdr agent list` or `workspace list` (`what`), as JSON; #298: the agents shed's lanes too.
pub fn list(r: &dyn Runner, what: &str) -> Result<Value> {
    if what == "agent" && r.shed().is_some() {
        return Ok(json!({"result": {"agents": agents(r)?}}));
    }
    json(r, &argv(&[what, "list"]))
}

/// Live agents (`herdr agent list`); #298: with shed, herdr's (if it answers; a cached list's shed rows dropped) and
/// shed's lanes.
pub fn agents(r: &dyn Runner) -> Result<Vec<Value>> {
    let Some(b) = r.shed() else {
        return Herdr.agents(r);
    };
    let herdr = Herdr
        .agents(r)
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a["runtime"] != "shed");
    Ok(herdr.chain(Shed(b).agents(r)?).collect())
}

/// `herdr agent list`'s stdout when it ran OK (serve's watcher hashes and caches it); #298: with shed, both lists.
pub fn agent_list(r: &dyn Runner) -> Option<String> {
    if r.shed().is_some() {
        return list(r, "agent").ok().map(|v| v.to_string());
    }
    exec(r, &argv(&["agent", "list"]))
        .ok()
        .filter(|o| o.code == 0)
        .map(|o| o.stdout)
}

pub fn read_argv(pane: &str) -> Vec<String> {
    argv(&["pane", "read", pane, "--source", "visible"])
}

/// The pane's visible screen.
pub fn read(r: &dyn Runner, pane: &str) -> Result<String> {
    ok(r, &read_argv(pane))
}

/// The pane's last `lines` lines (herdr's whole `recent` window when shorter), with their SGR colours (#138).
pub fn recent(r: &dyn Runner, pane: &str, lines: &str) -> Result<String> {
    let a = format!("pane read {pane} --source recent --lines {lines} --format ansi");
    ok(r, &argv(&a.split(' ').collect::<Vec<_>>()))
}

pub fn keys_argv(pane: &str, key: &str) -> Vec<String> {
    argv(&["pane", "send-keys", pane, key])
}

/// Send one key (`Enter`, `Escape`, a menu digit) to the pane.
pub fn keys(r: &dyn Runner, pane: &str, key: &str) -> Result<String> {
    ok(r, &keys_argv(pane, key))
}

pub fn prompt_argv(agent: &str, text: &str) -> Vec<String> {
    argv(&["agent", "prompt", agent, text])
}

/// Type `text` into the agent's prompt (no Enter).
pub fn prompt(r: &dyn Runner, agent: &str, text: &str) -> Result<String> {
    ok(r, &prompt_argv(agent, text))
}

pub fn send_text_argv(pane: &str, text: &str) -> Vec<String> {
    argv(&["pane", "send-text", pane, text])
}

/// The page's reply: the text (dsh: `pane send-text`, else `agent prompt`), then Enter.
pub fn reply(r: &dyn Runner, dsh: bool, agent: &str, pane: &str, text: &str) -> Result<()> {
    // No `--`: herdr 0.9 reads the 2nd positional as literal text (even `-x`), and a `--` there would be sent as text.
    let first = if dsh {
        send_text_argv(pane, text)
    } else {
        prompt_argv(agent, text)
    };
    for a in [first, keys_argv(pane, "Enter")] {
        let o = exec(r, &a)?;
        if o.code != 0 {
            bail!("herdr {} {}: {}", a[0], a[1], o.stderr.trim());
        }
    }
    Ok(())
}

/// #143: whether a prompt the caller just entered moved the agent off idle within 20 s, typing it once more if not.
pub fn landed(r: &dyn Runner, agent: &str, pane: &str, text: &str) -> bool {
    let up = || exec(r, &wait_working_argv(agent, "20000")).is_ok_and(|o| o.code == 0);
    up() || (reply(r, false, agent, pane, text).is_ok() && up())
}

/// Close the pane; one already gone (#249: a worktree remove closed its workspace) is closed.
pub fn close(r: &dyn Runner, pane: &str) -> Result<String> {
    let o = exec(r, &argv(&["pane", "close", pane]))?;
    if o.code != 0 && ![&o.stderr, &o.stdout].iter().any(|s| s.contains("pane_not_found")) {
        bail!("herdr pane close {pane} failed ({}): {}", o.code, o.stderr.trim());
    }
    Ok(o.stdout)
}

/// herdr's `{"error": {"code": <code>}}` answer (on stderr or stdout), e.g. `timeout`.
pub fn failed_with(o: &Out, code: &str) -> bool {
    [&o.stderr, &o.stdout]
        .iter()
        .any(|s| serde_json::from_str::<Value>(s.trim()).is_ok_and(|v| v["error"]["code"] == code))
}

pub fn rename_argv(pane: &str, agent: &str) -> Vec<String> {
    argv(&["agent", "rename", pane, agent])
}

/// #359: how long a lane's startup is waited for (confirm, rename).
pub const CONFIRM_SECS: u64 = 120;

/// #359: name the agent in `pane`; `agent_launch_pending` (startup not done) waits 5 s and tries again, up to 120 s.
pub fn rename(r: &dyn Runner, pane: &str, agent: &str) -> Result<()> {
    let (a, mut n) = (rename_argv(pane, agent), CONFIRM_SECS / 5);
    let mut o = exec(r, &a)?;
    while n > 0 && failed_with(&o, "agent_launch_pending") {
        r.sleep(5);
        (n, o) = (n - 1, exec(r, &a)?);
    }
    anyhow::ensure!(o.code == 0, "herdr agent rename {pane} failed: {}", o.stderr.trim());
    Ok(())
}

pub fn pane_get_argv(pane: &str) -> Vec<String> {
    argv(&["pane", "get", pane])
}

/// `herdr pane get`'s pane, when a harness (any detected agent) runs in it.
pub fn harness_in(r: &dyn Runner, pane: &str) -> Option<Value> {
    let v = json(r, &pane_get_argv(pane)).ok()?;
    Some(v["result"]["pane"].clone()).filter(|p| !p["agent"].is_null())
}

/// `herdr pane run <pane> <words…>`: type a line into the pane's shell.
pub fn run_argv(pane: &str, words: &[&str]) -> Vec<String> {
    argv(&[&["pane", "run", pane][..], words].concat())
}

/// #235: type `line` into a pane of a throwaway workspace at `cwd`, wait up to 20 s for `done`, read the screen,
/// close the workspace; the screen (None: no pane).
pub fn in_pane(r: &dyn Runner, cwd: &str, line: &str, done: &str) -> Option<String> {
    let pane = workspace_create(r, cwd, "herdsman-health").ok()?;
    let _ = exec(r, &run_argv(&pane, &[line]));
    let _ = exec(r, &wait_output_argv(&pane, done, "20000"));
    let text = read(r, &pane).unwrap_or_default();
    let _ = exec(
        r,
        &argv(&["workspace", "close", pane.split(':').next().unwrap_or(&pane)]),
    );
    Some(text)
}

/// `herdr agent start` for the harnesses herdr knows (`--kind`), `args` going to the harness.
pub fn start_argv(agent: &str, kind: &str, pane: &str, args: Vec<String>) -> Vec<String> {
    let mut c = argv(&["agent", "start", agent, "--kind", kind, "--pane", pane]);
    c.extend(argv(&["--timeout", "90000", "--"]));
    c.extend(args);
    c
}

/// `herdr agent wait` until the kick shows as working or blocked.
pub fn wait_working_argv(agent: &str, ms: &str) -> Vec<String> {
    let a = format!("agent wait {agent} --until working --until blocked --timeout {ms}");
    argv(&a.split(' ').collect::<Vec<_>>())
}

pub fn wait_output_argv(pane: &str, pattern: &str, ms: &str) -> Vec<String> {
    argv(&[
        "pane",
        "wait-output",
        pane,
        "--match",
        pattern,
        "--source",
        "visible",
        "--timeout",
        ms,
    ])
}

pub fn worktree_create_argv(local: &str, branch: &str, base: &str, path: &str, label: &str) -> Vec<String> {
    argv(&[
        "worktree",
        "create",
        "--cwd",
        local,
        "--branch",
        branch,
        "--base",
        base,
        "--path",
        path,
        "--label",
        label,
        "--no-focus",
    ])
}

pub fn worktree_open_argv(local: &str, path: &str, label: &str) -> Vec<String> {
    argv(&["worktree", "open", "--cwd", local, "--path", path, "--label", label])
}

/// A new workspace at `cwd`, unfocused: its root pane.
pub fn workspace_create(r: &dyn Runner, cwd: &str, label: &str) -> Result<String> {
    let ws = json(
        r,
        &argv(&["workspace", "create", "--cwd", cwd, "--label", label, "--no-focus"]),
    )?;
    Ok(ws["result"]["root_pane"]["pane_id"]
        .as_str()
        .context("herdr: no root pane")?
        .into())
}

pub fn workspace_rename(r: &dyn Runner, ws: &str, label: &str) -> Result<Out> {
    exec(r, &argv(&["workspace", "rename", ws, label]))
}

pub fn workspace_close(r: &dyn Runner, ws: &str) -> Result<Out> {
    exec(r, &argv(&["workspace", "close", ws]))
}

/// Remove the lane's worktree through herdr (by its workspace); false when herdr would not.
pub fn worktree_remove(r: &dyn Runner, ws: &str) -> Result<bool> {
    Ok(exec(r, &argv(&["worktree", "remove", "--workspace", ws]))?.code == 0)
}

/// Pick an idle shell pane in the workspace other than `skip`, splitting one off if all are busy.
pub fn free_pane(r: &dyn Runner, ws: &str, wt: &Path, skip: Option<&str>) -> Result<String> {
    let panes = json(r, &argv(&["pane", "list", "--workspace", ws]))?;
    let list = panes["result"]["panes"].as_array().cloned().unwrap_or_default();
    if let Some(p) = list
        .iter()
        .find(|p| p["agent"].is_null() && p["pane_id"].as_str() != skip)
    {
        return Ok(p["pane_id"].as_str().unwrap_or_default().into());
    }
    let first = list
        .first()
        .and_then(|p| p["pane_id"].as_str())
        .context("workspace has no panes")?;
    let wt = wt.to_string_lossy();
    let s = json(
        r,
        &argv(&[
            "pane",
            "split",
            first,
            "--direction",
            "right",
            "--cwd",
            &wt,
            "--no-focus",
        ]),
    )?;
    Ok(s["result"]["pane"]["pane_id"]
        .as_str()
        .context("pane split: no pane_id")?
        .into())
}
