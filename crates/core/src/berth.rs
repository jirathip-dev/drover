//! #298: berth's wire (`[runtime] kind = "berth"`, docs/runtimes.md): berth-proto JSON-RPC lines over berth's Unix
//! socket, or through `berth connect` (over ssh on a lane's other host); one connection per call, no async. The
//! runtime itself (herdr's words as berth calls) is `crate::herdr::Berth`.
use crate::config::Config;
use crate::events::{append, Event};
use crate::runner::{argv, Out, Runner};
use crate::Ctx;
use anyhow::{bail, ensure, Context, Result};
use berth_proto::{EventKind, EventsParams, Hello, Method, Request, Response, VERSION};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

pub use berth_proto as proto;

/// Where a host's berth server answers.
#[derive(Clone, Debug, PartialEq)]
pub enum Wire {
    /// This host's socket.
    Socket(PathBuf),
    /// `berth connect` through the runner: a lane's other host (`hosts::On` sends it over ssh).
    Connect,
}

/// The berth runtime a runner's lanes use: its wire, and the ACP agent its lanes run (`berth run --acp`), if any.
#[derive(Clone, Debug)]
pub struct Berth {
    pub wire: Wire,
    pub acp: Option<String>,
    pub policy: String,
}

/// berth's per-user socket (as `berth_server::Paths::user`): macOS `~/Library/Caches/berth/berth.sock`, elsewhere
/// `$XDG_RUNTIME_DIR/berth.sock`.
pub fn default_socket() -> PathBuf {
    if cfg!(target_os = "macos") {
        return crate::config::home().join("Library/Caches/berth/berth.sock");
    }
    PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_default()).join("berth.sock")
}

/// The `hello` line, then the request.
fn lines(m: Method, params: Value) -> String {
    let hello = json!(Hello {
        version: VERSION.into()
    });
    let [a, b] = [Request::new(0, Method::Hello, hello), Request::new(1, m, params)].map(|q| json!(q).to_string());
    format!("{a}\n{b}\n")
}

/// The request's result from the server's two answer lines, or berth's `NAME: detail`.
fn answer(text: &str) -> Result<Value> {
    let mut it = text.lines().map(serde_json::from_str::<Response>);
    let mut next = || -> Result<Response> { Ok(it.next().context("berth: connection closed")??) };
    let hello = next()?;
    if let Some(e) = hello.error {
        bail!("{}", e.message);
    }
    let theirs = hello.result.as_ref().and_then(|v| v["version"].as_str()).unwrap_or("?");
    ensure!(
        theirs == VERSION,
        "BERTH_VERSION: herdsman speaks berth {VERSION}, the server is {theirs}"
    );
    match next()? {
        Response { error: Some(e), .. } => bail!("{}", e.message),
        Response { result: Some(v), .. } => Ok(v),
        _ => bail!("berth: neither result nor error"),
    }
}

/// One call on a fresh connection (`hello`, then `m`): its result.
pub fn call(r: &dyn Runner, w: &Wire, m: Method, params: Value) -> Result<Value> {
    let text = match w {
        Wire::Socket(p) => {
            let mut s = UnixStream::connect(p).with_context(|| format!("no berth server on {}", p.display()))?;
            s.write_all(lines(m, params).as_bytes())?;
            let (mut rd, mut text) = (BufReader::new(s), String::new());
            for _ in 0..2 {
                rd.read_line(&mut text)?;
            }
            text
        }
        Wire::Connect => {
            let o = r.run_in("berth", &argv(&["connect", "--autostart"]), &lines(m, params))?;
            ensure!(
                !o.stdout.is_empty(),
                "berth connect failed ({}): {}",
                o.code,
                o.stderr.trim()
            );
            o.stdout
        }
    };
    answer(&text)
}

/// The `berth` CLI (ACP mode only: its bridge and prompt framing are berth's own); fails on a non-zero exit.
pub fn cli(r: &dyn Runner, a: &[&str]) -> Result<Out> {
    let o = r.run("berth", &argv(a))?;
    ensure!(
        o.code == 0,
        "berth {} failed ({}): {}",
        a.join(" "),
        o.code,
        o.stderr.trim()
    );
    Ok(o)
}

/// The kept events after `since` (berth keeps the last 1,000), read until the stream is quiet for `quiet`.
pub fn events(w: &Wire, since: u64, quiet: Duration) -> Result<Vec<berth_proto::Event>> {
    let Wire::Socket(p) = w else {
        bail!("berth events: this host's server only");
    };
    let mut s = UnixStream::connect(p).with_context(|| format!("no berth server on {}", p.display()))?;
    let params = EventsParams {
        since: Some(since),
        ..Default::default()
    };
    s.write_all(lines(Method::Events, json!(params)).as_bytes())?;
    s.set_read_timeout(Some(quiet))?;
    let (mut rd, mut head) = (BufReader::new(s), String::new());
    for _ in 0..2 {
        rd.read_line(&mut head)?;
    }
    answer(&head)?;
    let (mut out, mut l) = (vec![], String::new());
    while rd.read_line(&mut l).is_ok_and(|n| n > 0) {
        out.extend(serde_json::from_str(&l).ok());
        l.clear();
    }
    Ok(out)
}

/// #298: drive's intake of berth's state changes since the last pass (`berth.seq` beside the log): each lane's
/// reported state, exit or kill as a passive `lane-state` event. Only this host's server; the agent list every pass
/// polls still sees every lane, here and elsewhere.
pub fn intake(ctx: &Ctx, cfg: &Config, w: &Wire) -> Result<usize> {
    let seq = ctx.state.with_file_name("berth.seq");
    let since = std::fs::read_to_string(&seq)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    let got = events(w, since, Duration::from_millis(300))?;
    let mut n = 0;
    for e in &got {
        let (Some(fleet), Some(issue)) = (
            e.labels.get("fleet"),
            e.labels.get("issue").and_then(|i| i.parse().ok()),
        ) else {
            continue;
        };
        let mut d = match &e.kind {
            EventKind::State(s) => json!({"state": s.state, "note": s.note}),
            EventKind::Exited { code } => json!({"state": "exited", "code": code}),
            EventKind::Killed => json!({"state": "killed"}),
            _ => continue,
        };
        (d["agent"], d["berth_seq"]) = (json!(e.name), json!(e.seq));
        if cfg.fleet.contains_key(fleet) {
            append(&ctx.state, &Event::new(ctx.now, fleet, Some(issue), "lane-state", d))?;
            n += 1;
        }
    }
    if let Some(last) = got.iter().map(|e| e.seq).max() {
        std::fs::write(&seq, last.to_string())?;
    }
    Ok(n)
}

/// A runner whose lanes run on berth: `r` for every program, `b` for the runtime.
pub struct With<'a>(pub &'a dyn Runner, pub Berth);

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
    fn berth(&self) -> Option<Berth> {
        Some(self.1.clone())
    }
}
