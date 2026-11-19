use crate::runner::{argv, Out, Runner};
use crate::{Config, Ctx, Event};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use shed_proto::{EventsParams, Hello, Method, Request, Response};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

pub use shed_proto as proto;

/// shed's repo, beside drover's own: a new host's bootstrap installs its latest main once; shed updates itself after that.
pub fn git() -> String {
    env!("CARGO_PKG_REPOSITORY").replace("/herdsman", "/shed")
}

/// The oldest shed protocol drover accepts: any shed at or above it passes, whatever its build.
pub const MIN_PROTOCOL: u32 = 1;

/// The fix a `SHED_VERSION` refusal names: drover never installs shed.
pub const UPDATE: &str = "update shed (it updates itself from its own releases and hands serve over)";

/// Where a host's shed server answers.
#[derive(Clone, Debug, PartialEq)]
pub enum Wire {
    Socket(PathBuf),
    Connect,
}

/// The shed runtime a runner's lanes use: its wire, and the ACP agent its lanes run (`shed run --acp`), if any.
#[derive(Clone, Debug)]
pub struct Shed {
    pub wire: Wire,
    pub acp: Option<String>,
    pub policy: String,
}

/// shed's per-user socket (as `shed_server::Paths::user`): macOS `~/Library/Caches/shed/shed.sock`, elsewhere `$XDG_RUNTIME_DIR/shed.sock`.
pub fn default_socket() -> PathBuf {
    if cfg!(target_os = "macos") {
        return crate::config::home().join("Library/Caches/shed/shed.sock");
    }
    PathBuf::from(crate::config::env("XDG_RUNTIME_DIR").unwrap_or_default()).join("shed.sock")
}

fn lines(m: Method, params: Value) -> String {
    let hello = json!(Hello::ours());
    let [a, b] = [Request::new(0, Method::Hello, hello), Request::new(1, m, params)].map(|q| json!(q).to_string());
    format!("{a}\n{b}\n")
}

fn short(h: &Hello) -> String {
    format!("{} ({})", h.version, h.label())
}

fn side(h: &Hello) -> String {
    format!("{} protocol {}", short(h), h.protocol)
}

fn accepted(h: &Hello) -> bool {
    h.product == "shed" && h.protocol >= MIN_PROTOCOL
}

fn answer(text: &str) -> Result<(Hello, Value)> {
    let mut it = text.lines().map(serde_json::from_str::<Response>);
    let mut next = || -> Result<Response> { Ok(it.next().context("shed: connection closed")??) };
    let hello = next()?;
    if let Some(e) = hello.error {
        bail!("{}", e.message);
    }
    let theirs = hello.result.and_then(|v| serde_json::from_value::<Hello>(v).ok());
    ensure!(
        theirs.as_ref().is_some_and(accepted),
        "SHED_VERSION: shed serve is {}, drover needs shed protocol {MIN_PROTOCOL} or newer → {UPDATE}",
        theirs.as_ref().map_or("?".into(), side)
    );
    match next()? {
        Response { error: Some(e), .. } => bail!("{}", e.message),
        Response { result: Some(v), .. } => Ok((theirs.unwrap_or_else(Hello::ours), v)),
        _ => bail!("shed: neither result nor error"),
    }
}

/// Doctor's `shed` line for the server on `w`: its version and protocol against drover's minimum, `✗` under it, `!` when it does not answer.
pub fn line(r: &dyn Runner, w: &Wire) -> String {
    let theirs = exchange(r, w, Method::Ls, json!(proto::LsParams::default())).and_then(|t| answer(&t));
    match theirs {
        Ok(_) if unit(r).is_some_and(|a| a != "active") => {
            "✗ shed — server outside its service (shed.service not active) \
→ stop that shed serve, then systemctl --user reset-failed shed.service && systemctl --user start shed.service"
                .into()
        }
        Ok((h, _)) => {
            let other = on_path(r).filter(|b| b.build != h.build);
            let path = other.map_or(String::new(), |b| {
                format!("; shed on PATH {} (serve hands over itself)", short(&b))
            });
            format!(
                "✓ shed — server {}, drover needs protocol {MIN_PROTOCOL} or newer{path}",
                side(&h)
            )
        }
        Err(e)
            if ["SHED_JOB", "SHED_VERSION"]
                .iter()
                .any(|k| format!("{e:#}").starts_with(k)) =>
        {
            format!("✗ shed — {e:#}")
        }
        Err(e) => format!("! shed: {e:#}"),
    }
}

fn on_path(r: &dyn Runner) -> Option<Hello> {
    let o = r.run("shed", &argv(&["--version"])).ok().filter(|o| o.code == 0)?;
    let (version, build) = o.stdout.trim().strip_prefix("shed ")?.split_once(" (")?;
    let build = build.strip_suffix(')')?.into();
    Some(Hello {
        version: version.into(),
        build,
        ..Hello::ours()
    })
}

fn hello(r: &dyn Runner, w: &Wire) -> Result<(Hello, Value)> {
    answer(&exchange(r, w, Method::Hello, json!(Hello::ours()))?)
}

/// `shed on PATH <build>, serve <build>` as `r` reads them, `?` for one it cannot.
pub fn builds(r: &dyn Runner) -> String {
    let serve = r.shed().and_then(|w| hello(r, &w.wire).ok()).map(|h| h.0);
    let s = |h: Option<Hello>| h.as_ref().map_or("?".into(), short);
    format!("shed on PATH {}, serve {}", s(on_path(r)), s(serve))
}

/// A local spawn's shed: `SHED_MISSING` without one on PATH, `SHED_VERSION` (unless `dry`) when serve's protocol is under drover's minimum.
pub fn ready(r: &dyn Runner, w: &Wire, dry: bool) -> Result<()> {
    if crate::runner::ok(r, "which", &["shed"]).is_err() {
        let path = crate::config::env("PATH").unwrap_or_default();
        let fix = "`drover timer install` from a shell where `which shed` finds it";
        return crate::gate::refused("SHED_MISSING", format!("shed is not on PATH ({path}); {fix}"));
    }
    if dry {
        return Ok(());
    }
    match hello(r, w) {
        Err(e) if format!("{e:#}").starts_with("SHED_VERSION") => {
            crate::gate::refused("SHED_VERSION", format!("{e:#}").replacen("SHED_VERSION: ", "", 1))
        }
        _ => Ok(()),
    }
}

/// A `SHED_VERSION` refusal in `res` alerts `shed-version` once per distinct refusal, and `shed-version-ok` once a call gets through again; whether `res` was one.
pub fn version_alert<T>(ctx: &Ctx, cfg: &Config, res: &Result<T>) -> Result<bool> {
    let refused = res.as_ref().err().map(|e| format!("{e:#}"));
    let refused = refused.filter(|e| e.starts_with("SHED_VERSION"));
    let kinds = ["shed-version", "shed-version-ok"];
    let events = crate::events::working(&ctx.state)?;
    let last = events.iter().rev().find(|e| kinds.contains(&e.kind.as_str()));
    let was = last.filter(|e| e.kind == kinds[0]).and_then(|e| e.str("key"));
    let d = match (&refused, was) {
        (Some(why), _) if was != Some(why.as_str()) => Some((kinds[0], json!({"key": why, "reason": why}))),
        (None, Some(_)) if res.is_ok() => Some((kinds[1], json!({}))),
        _ => None,
    };
    if let Some((kind, d)) = d {
        crate::events::emit(&ctx.state, cfg, Event::new(ctx.now, "drover", None, kind, d))?;
    }
    Ok(refused.is_some())
}

/// One call on a fresh connection (`hello`, then `m`): its result.
pub fn call(r: &dyn Runner, w: &Wire, m: Method, params: Value) -> Result<Value> {
    answer(&exchange(r, w, m, params)?).map(|x| x.1)
}

fn exchange(r: &dyn Runner, w: &Wire, m: Method, params: Value) -> Result<String> {
    Ok(match w {
        Wire::Socket(p) => {
            let mut s = UnixStream::connect(p).with_context(|| format!("no shed server on {}", p.display()))?;
            s.write_all(lines(m, params).as_bytes())?;
            let (mut rd, mut text) = (BufReader::new(s), String::new());
            for _ in 0..2 {
                rd.read_line(&mut text)?;
            }
            text
        }
        Wire::Connect => connect(r, &lines(m, params))?,
    })
}

fn connect(r: &dyn Runner, q: &str) -> Result<String> {
    let limit = Duration::from_secs(crate::config::limits().ssh_call_secs);
    let send = |a: &[&str]| r.run_for("shed", &argv(a), q, limit);
    let lost = |o: &Out| matches!(o.code, 124 | 255);
    let mut o = send(&["connect"])?;
    if o.stdout.is_empty() && !lost(&o) {
        match start_job(r) {
            None => o = send(&["connect", "--autostart"])?,
            Some((_, job)) => {
                for _ in 0..10 {
                    if !o.stdout.is_empty() || lost(&o) {
                        break;
                    }
                    r.sleep(1);
                    o = send(&["connect"])?;
                }
                ensure!(
                    !o.stdout.is_empty() || lost(&o),
                    "SHED_JOB: the shed job is installed but its server is not up → {job}"
                );
            }
        }
    }
    ensure!(
        !o.stdout.is_empty(),
        "shed connect failed ({}): {}",
        o.code,
        o.stderr.trim()
    );
    Ok(o.stdout)
}

fn unit(r: &dyn Runner) -> Option<String> {
    let o = r
        .run(
            "systemctl",
            &argv(&["--user", "show", "-p", "LoadState,ActiveState", "shed.service"]),
        )
        .ok()
        .filter(|o| o.code == 0)?;
    let at = |k: &str| o.stdout.lines().find_map(|l| l.strip_prefix(k)).map(str::to_string);
    at("LoadState=").filter(|l| l == "loaded")?;
    at("ActiveState=")
}

/// Kick shed's own job (systemd `shed.service`, launchd `dev.shed.serve`): its start command and its log command; `None` with no job.
pub fn start_job(r: &dyn Runner) -> Option<(&'static str, &'static str)> {
    if unit(r).is_some() {
        r.run("systemctl", &argv(&["--user", "start", "shed.service"])).ok();
        return Some((
            "systemctl --user start shed.service",
            "journalctl --user -u shed.service",
        ));
    }
    let plist = "p=\"$HOME/Library/LaunchAgents/dev.shed.serve.plist\"; [ -f \"$p\" ] && \
launchctl kickstart \"gui/$(id -u)/dev.shed.serve\"";
    let o = r.run("sh", &argv(&["-c", plist])).ok()?;
    (o.code == 0).then_some((
        "launchctl kickstart gui/$(id -u)/dev.shed.serve",
        "launchctl print gui/$(id -u)/dev.shed.serve",
    ))
}

/// The `shed` CLI (ACP mode only: its bridge and prompt framing are shed's own); fails on a non-zero exit.
pub fn cli(r: &dyn Runner, a: &[&str]) -> Result<Out> {
    let o = r.run("shed", &argv(a))?;
    ensure!(
        o.code == 0,
        "shed {} failed ({}): {}",
        a.join(" "),
        o.code,
        o.stderr.trim()
    );
    Ok(o)
}

/// An agent's installed adapter command from `shed agents --json`: under `installed` since shed#129, else top level.
pub fn adapter(r: &dyn Runner, id: &str) -> Result<Option<Vec<String>>> {
    let agents: Value = serde_json::from_str(&cli(r, &["agents", "--json"])?.stdout)?;
    let a = agents.as_array().into_iter().flatten().find(|a| a["id"] == id);
    let a = a.map(|a| a.get("installed").unwrap_or(a));
    Ok(a.and_then(|a| serde_json::from_value(a.get("cmd")?.clone()).ok()))
}

/// The kept events after `since` (shed keeps the last 1,000), read until the stream is quiet for `quiet`.
pub fn events(w: &Wire, since: u64, quiet: Duration) -> Result<Vec<shed_proto::Event>> {
    let mut out = vec![];
    follow(w, since, Some(quiet), |e| out.extend(e))?;
    Ok(out)
}

/// `events` kept open: `f(None)` once the server took it, then `f(Some(event))` for each kept event after `since` and each new one, until the stream ends (or is quiet for `quiet`).
pub fn follow(
    w: &Wire,
    since: u64,
    quiet: Option<Duration>,
    mut f: impl FnMut(Option<shed_proto::Event>),
) -> Result<()> {
    let Wire::Socket(p) = w else {
        bail!("shed events: this host's server only");
    };
    let mut s = UnixStream::connect(p).with_context(|| format!("no shed server on {}", p.display()))?;
    let params = EventsParams {
        since: Some(since),
        ..Default::default()
    };
    s.write_all(lines(Method::Events, json!(params)).as_bytes())?;
    s.set_read_timeout(quiet)?;
    let (mut rd, mut head) = (BufReader::new(s), String::new());
    for _ in 0..2 {
        rd.read_line(&mut head)?;
    }
    answer(&head)?;
    f(None);
    let mut l = String::new();
    while rd.read_line(&mut l).is_ok_and(|n| n > 0) {
        if let Ok(e) = serde_json::from_str(&l) {
            f(Some(e));
        }
        l.clear();
    }
    Ok(())
}

/// The last shed `seq` taken in (`shed.seq` beside the log).
pub fn seen(ctx: &Ctx) -> u64 {
    std::fs::read_to_string(ctx.state.with_file_name("shed.seq"))
        .or_else(|_| std::fs::read_to_string(ctx.state.with_file_name("berth.seq")))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// A runner whose lanes run on shed: `r` for every program, `b` for the runtime.
pub struct With<'a>(pub &'a dyn Runner, pub Shed);

impl Runner for With<'_> {
    fn run(&self, prog: &str, args: &[String]) -> Result<Out> {
        self.0.run(prog, args)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> Result<Out> {
        self.0.run_in(prog, args, input)
    }
    fn run_for(&self, prog: &str, args: &[String], input: &str, limit: std::time::Duration) -> Result<Out> {
        self.0.run_for(prog, args, input, limit)
    }
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
    fn detach(&self, exe: &str, args: &[String], log: &std::path::Path) -> Result<u32> {
        self.0.detach(exe, args, log)
    }
    fn shed(&self) -> Option<Shed> {
        Some(self.1.clone())
    }
}
