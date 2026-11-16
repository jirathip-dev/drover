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
    PathBuf::from(crate::config::env("XDG_RUNTIME_DIR").unwrap_or_default()).join("shed.sock")
}

fn lines(m: Method, params: Value) -> String {
    let hello = json!(Hello::ours());
    let [a, b] = [Request::new(0, Method::Hello, hello), Request::new(1, m, params)].map(|q| json!(q).to_string());
    format!("{a}\n{b}\n")
}

fn side(h: &Hello) -> String {
    format!("{} ({}) protocol {}", h.version, h.label(), h.protocol)
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
        theirs.as_ref().is_some_and(Hello::compatible),
        "SHED_VERSION: drover speaks shed {}, the server is {}",
        side(&Hello::ours()),
        theirs.as_ref().map_or("?".into(), side)
    );
    match next()? {
        Response { error: Some(e), .. } => bail!("{}", e.message),
        Response { result: Some(v), .. } => Ok((theirs.unwrap_or_else(Hello::ours), v)),
        _ => bail!("shed: neither result nor error"),
    }
}

/// #1031: doctor's `shed` line for the server on `w`: its version and protocol against drover's, `✗` on a mismatch, `!` when it does not answer.
pub fn line(r: &dyn Runner, w: &Wire) -> String {
    let theirs = exchange(r, w, Method::Ls, json!(proto::LsParams::default())).and_then(|t| answer(&t));
    match theirs {
        Ok(_) if unit(r).is_some_and(|a| a != "active") => {
            "✗ shed — server outside its service (shed.service not active) \
→ stop that shed serve, then systemctl --user reset-failed shed.service && systemctl --user start shed.service"
                .into()
        }
        Ok((h, _)) => format!("✓ shed — server {}, drover speaks {}", side(&h), side(&Hello::ours())),
        Err(e) if format!("{e:#}").starts_with("SHED_JOB") => format!("✗ shed — {e:#}"),
        Err(e) if format!("{e:#}").starts_with("SHED_VERSION") => format!(
            "✗ shed — {e:#} → cargo install --git {} --rev {REV} --locked --force shed, then restart shed serve",
            git()
        ),
        Err(e) => format!("! shed: {e:#}"),
    }
}

/// #1031: a `SHED_VERSION` refusal in `res` alerts `shed-version` once per distinct refusal, and `shed-version-ok` once a call gets through again; whether `res` was one.
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
    let plain = || r.run_in("shed", &argv(&["connect"]), q);
    let mut o = plain()?;
    if o.stdout.is_empty() && !matches!(o.code, 124 | 255) {
        match start_job(r) {
            None => o = r.run_in("shed", &argv(&["connect", "--autostart"]), q)?,
            Some(job) => {
                for _ in 0..10 {
                    if !o.stdout.is_empty() {
                        break;
                    }
                    r.sleep(1);
                    o = plain()?;
                }
                ensure!(
                    !o.stdout.is_empty(),
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

fn start_job(r: &dyn Runner) -> Option<&'static str> {
    if unit(r).is_some() {
        drop(r.run("systemctl", &argv(&["--user", "start", "shed.service"])));
        return Some("journalctl --user -u shed.service");
    }
    let plist = "p=\"$HOME/Library/LaunchAgents/dev.shed.serve.plist\"; [ -f \"$p\" ] && \
launchctl kickstart \"gui/$(id -u)/dev.shed.serve\"";
    let o = r.run("sh", &argv(&["-c", plist])).ok()?;
    (o.code == 0).then_some("launchctl print gui/$(id -u)/dev.shed.serve")
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
