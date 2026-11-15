use crate::runner::{argv, Out, Runner};
use crate::Ctx;
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use shed_proto::{EventsParams, Hello, Method, Request, Response};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

pub use shed_proto as proto;

/// #637: the shed a `shed-acp` host's bootstrap installs: the build proven with drover on the main host. shed's repo, beside drover's own.
pub fn git() -> String {
    env!("CARGO_PKG_REPOSITORY").replace("/herdsman", "/shed")
}
pub const REV: &str = "9b88eb5be5bc9ba8c5c75480483b9861ddc6e993";

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
    PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_default()).join("shed.sock")
}

fn lines(m: Method, params: Value) -> String {
    let hello = json!(Hello::ours());
    let [a, b] = [Request::new(0, Method::Hello, hello), Request::new(1, m, params)].map(|q| json!(q).to_string());
    format!("{a}\n{b}\n")
}

fn answer(text: &str) -> Result<Value> {
    let mut it = text.lines().map(serde_json::from_str::<Response>);
    let mut next = || -> Result<Response> { Ok(it.next().context("shed: connection closed")??) };
    let hello = next()?;
    if let Some(e) = hello.error {
        bail!("{}", e.message);
    }
    let theirs = hello.result.and_then(|v| serde_json::from_value::<Hello>(v).ok());
    let side = |h: &Hello| format!("{} ({}) protocol {}", h.version, h.label(), h.protocol);
    ensure!(
        theirs.as_ref().is_some_and(Hello::compatible),
        "SHED_VERSION: drover speaks shed {}, the server is {}",
        side(&Hello::ours()),
        theirs.as_ref().map_or("?".into(), side)
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

/// #529: `events` kept open: `f(None)` once the server took it, then `f(Some(event))` for each kept event after `since` and each new one, until the stream ends (or is quiet for `quiet`).
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
