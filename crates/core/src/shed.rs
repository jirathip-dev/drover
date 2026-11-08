//! #298: shed's wire (`[runtime] kind = "shed"`, docs/runtimes.md): shed-proto JSON-RPC lines over shed's Unix
//! socket, or through `shed connect` (over ssh on a lane's other host); one connection per call, no async. The
//! runtime itself (herdr's words as shed calls) is `crate::herdr::Shed`.
use crate::config::Config;
use crate::events::{append, Event};
use crate::runner::{argv, Out, Runner};
use crate::Ctx;
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use shed_proto::{EventKind, EventsParams, Hello, Method, Request, Response, VERSION};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

pub use shed_proto as proto;

/// #637: the shed a `shed-acp` host's bootstrap installs: the build proven with herdsman on the main host.
pub const GIT: &str = "https://github.com/jirathip-dev/shed";
pub const REV: &str = "8bac04fdb8542ef496c614b47126d85c50a7892e";

/// Where a host's shed server answers.
#[derive(Clone, Debug, PartialEq)]
pub enum Wire {
    /// This host's socket.
    Socket(PathBuf),
    /// `shed connect` through the runner: a lane's other host (`hosts::On` sends it over ssh).
    Connect,
}

/// The shed runtime a runner's lanes use: its wire, and the ACP agent its lanes run (`shed run --acp`), if any.
#[derive(Clone, Debug)]
pub struct Shed {
    pub wire: Wire,
    pub acp: Option<String>,
    pub policy: String,
}

/// shed's per-user socket (as `shed_server::Paths::user`): macOS `~/Library/Caches/shed/shed.sock`, elsewhere
/// `$XDG_RUNTIME_DIR/shed.sock`.
pub fn default_socket() -> PathBuf {
    if cfg!(target_os = "macos") {
        return crate::config::home().join("Library/Caches/shed/shed.sock");
    }
    PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_default()).join("shed.sock")
}

/// The `hello` line, then the request.
fn lines(m: Method, params: Value) -> String {
    let hello = json!(Hello::ours());
    let [a, b] = [Request::new(0, Method::Hello, hello), Request::new(1, m, params)].map(|q| json!(q).to_string());
    format!("{a}\n{b}\n")
}

/// The request's result from the server's two answer lines, or shed's `NAME: detail`.
fn answer(text: &str) -> Result<Value> {
    let mut it = text.lines().map(serde_json::from_str::<Response>);
    let mut next = || -> Result<Response> { Ok(it.next().context("shed: connection closed")??) };
    let hello = next()?;
    if let Some(e) = hello.error {
        bail!("{}", e.message);
    }
    let theirs = hello.result.and_then(|v| serde_json::from_value::<Hello>(v).ok());
    ensure!(
        theirs.as_ref() == Some(&Hello::ours()),
        "SHED_VERSION: herdsman speaks shed {VERSION}, the server is {}",
        theirs.map_or("?".into(), |h| h.label())
    );
    match next()? {
        Response { error: Some(e), .. } => bail!("{}", e.message),
        Response { result: Some(v), .. } => Ok(v),
        _ => bail!("shed: neither result nor error"),
    }
}

/// One call on a fresh connection (`hello`, then `m`): its result.
pub fn call(r: &dyn Runner, w: &Wire, m: Method, params: Value) -> Result<Value> {
    let text = match w {
        Wire::Socket(p) => {
            let mut s = UnixStream::connect(p).with_context(|| format!("no shed server on {}", p.display()))?;
            s.write_all(lines(m, params).as_bytes())?;
            let (mut rd, mut text) = (BufReader::new(s), String::new());
            for _ in 0..2 {
                rd.read_line(&mut text)?;
            }
            text
        }
        Wire::Connect => {
            let o = r.run_in("shed", &argv(&["connect", "--autostart"]), &lines(m, params))?;
            ensure!(
                !o.stdout.is_empty(),
                "shed connect failed ({}): {}",
                o.code,
                o.stderr.trim()
            );
            o.stdout
        }
    };
    answer(&text)
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

/// The kept events after `since` (shed keeps the last 1,000), read until the stream is quiet for `quiet`.
pub fn events(w: &Wire, since: u64, quiet: Duration) -> Result<Vec<shed_proto::Event>> {
    let mut out = vec![];
    follow(w, since, Some(quiet), |e| out.extend(e))?;
    Ok(out)
}

/// #529: `events` kept open: `f(None)` once the server took it, then `f(Some(event))` for each kept event after
/// `since` and each new one, until the stream ends (or is quiet for `quiet`).
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
    // #455: berth's name, read once if shed's is missing; the next write is shed's.
    std::fs::read_to_string(ctx.state.with_file_name("shed.seq"))
        .or_else(|_| std::fs::read_to_string(ctx.state.with_file_name("berth.seq")))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// One shed event as its lane's passive `lane-state` (#529: an exit or kill is `gone`); whether it was a lane's.
pub fn log(ctx: &Ctx, cfg: &Config, e: &shed_proto::Event) -> Result<bool> {
    let (Some(fleet), Some(issue)) = (
        e.labels.get("fleet"),
        e.labels.get("issue").and_then(|i| i.parse().ok()),
    ) else {
        return Ok(false);
    };
    let mut d = match &e.kind {
        EventKind::State(s) => json!({"state": s.state, "note": s.note}),
        EventKind::Exited { code } => json!({"state": "gone", "why": "exited", "code": code}),
        EventKind::Killed => json!({"state": "gone", "why": "killed"}),
        _ => return Ok(false),
    };
    (d["agent"], d["shed_seq"]) = (json!(e.name), json!(e.seq));
    if !cfg.fleet.contains_key(fleet) {
        return Ok(false);
    }
    append(&ctx.state, &Event::new(ctx.now, fleet, Some(issue), "lane-state", d))?;
    Ok(true)
}

/// #298: drive's intake of shed's state changes since the last pass (`shed.seq` beside the log): each lane's
/// reported state, exit or kill as a passive `lane-state` event. Only this host's server; the agent list every pass
/// polls still sees every lane, here and elsewhere. #529: a running service follows the stream and keeps `shed.seq`.
pub fn intake(ctx: &Ctx, cfg: &Config, w: &Wire) -> Result<usize> {
    let got = events(w, seen(ctx), Duration::from_millis(300))?;
    let mut n = 0;
    for e in &got {
        n += usize::from(log(ctx, cfg, e)?);
    }
    if let Some(last) = got.iter().map(|e| e.seq).max() {
        std::fs::write(ctx.state.with_file_name("shed.seq"), last.to_string())?;
    }
    Ok(n)
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
    fn sleep(&self, secs: u64) {
        self.0.sleep(secs);
    }
    fn detach(&self, exe: &str, args: &[String], log: &std::path::Path) -> Result<u32> {
        self.0.detach(exe, args, log)
    }
    fn secret(&self, svc: &str) -> anyhow::Result<Option<String>> {
        self.0.secret(svc)
    }
    fn set_secret(&self, svc: &str, value: Option<&str>) -> anyhow::Result<()> {
        self.0.set_secret(svc, value)
    }
    fn shed(&self) -> Option<Shed> {
        Some(self.1.clone())
    }
    fn local(&self) -> bool {
        self.0.local()
    }
}
