use crate::config::Config;
use crate::events::{append, Event};
use crate::runner::Runner;
use crate::Ctx;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::time::Duration;

/// One serve's side of the socket handover (docs/setup.md): its listeners, control socket and drain state.
pub struct Handover {
    sock: PathBuf,
    listeners: Vec<(RawFd, SocketAddr)>,
    inherited: bool,
    stop: AtomicBool,
    left: AtomicUsize,
    exec: AtomicBool,
}

impl Handover {
    /// The board listener first, then the webhook one; `inherited`: a predecessor passed them down.
    pub fn new(ctx: &Ctx, ls: &[&TcpListener], inherited: bool) -> std::io::Result<Handover> {
        let listeners = ls
            .iter()
            .map(|l| Ok((l.as_raw_fd(), l.local_addr()?)))
            .collect::<std::io::Result<Vec<_>>>()?;
        Ok(Handover {
            sock: crate::update::serve_sock(ctx, listeners.first().map_or(0, |l| l.1.port())),
            listeners,
            inherited,
            stop: AtomicBool::new(false),
            left: AtomicUsize::new(0),
            exec: AtomicBool::new(false),
        })
    }

    /// A predecessor handed its listeners down: this start is no restart.
    pub fn inherited(&self) -> bool {
        self.inherited
    }

    /// A successor took over: the accept loops stop after the connection in hand.
    pub fn stopping(&self) -> bool {
        self.stop.load(SeqCst)
    }

    /// An accept loop has stopped and its requests are done.
    pub fn left(&self) {
        self.left.fetch_add(1, SeqCst);
    }

    /// Warm and about to accept: tell the predecessor (it drains), then own the control socket.
    pub fn ready(&self) -> Option<UnixListener> {
        if let Some(mut c) = self.inherited.then(|| UnixStream::connect(&self.sock).ok()).flatten() {
            c.set_read_timeout(Some(Duration::from_secs(10))).ok();
            writeln!(c, "ready {}", drover_core::runner::pid()).ok();
            BufReader::new(c).read_line(&mut String::new()).ok();
        }
        std::fs::remove_file(&self.sock).ok();
        let bound = UnixListener::bind(&self.sock).and_then(|l| {
            std::fs::set_permissions(&self.sock, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
            Ok(l)
        });
        bound
            .map_err(|e| drover_core::stderr!("drover serve: no handover: {}: {e}", self.sock.display()))
            .ok()
    }

    /// `handover` starts a bridge on the listeners; its `ready <pid>` answers the asker `ok <pid>` and this one drains.
    pub fn control(&self, sock: &UnixListener) {
        let mut asked: Option<UnixStream> = None;
        for mut c in sock.incoming().flatten() {
            let mut line = String::new();
            c.set_read_timeout(Some(Duration::from_secs(5))).ok();
            BufReader::new(&c).read_line(&mut line).ok();
            let (verb, pid) = line.trim().split_once(' ').unwrap_or((line.trim(), ""));
            match verb {
                "handover" if asked.is_none() => match crate::runner::hand_fds(&argv(), &self.fds(), true) {
                    Ok(()) => asked = Some(c),
                    Err(e) => drop(writeln!(c, "error: the bridge: {e:#}")),
                },
                "ready" => {
                    writeln!(c, "ok").ok();
                    if let Some(mut a) = asked.take() {
                        writeln!(a, "ok {pid}").ok();
                        self.exec.store(true, SeqCst);
                    }
                    return self.halt();
                }
                "handover" => drop(writeln!(c, "error: a handover is under way")),
                _ => drop(writeln!(c, "error: unknown request {verb:?}")),
            }
        }
    }

    /// Frees heap to the OS; past `serve_mem_mb` of RSS logs `serve-mem-high` once and hands over (docs/serve.md).
    pub fn watch_mem(&self, ctx: &Ctx, r: &dyn Runner) {
        let (pid, mut first, mut logged) = (drover_core::runner::pid().to_string(), None, false);
        while !self.stopping() {
            let limits = Config::kept(ctx).map(|c| c.limits).unwrap_or_default();
            drover_core::clock::sleep(Duration::from_secs(limits.serve_host_secs.max(1)));
            drover_sys::release_freed();
            let rss = crate::runner::ok(r, "ps", &["-o", "rss=", "-p", &pid]);
            let Some(mb) = rss.ok().and_then(|s| s.trim().parse::<u64>().ok()).map(|kb| kb >> 10) else {
                continue;
            };
            let fresh = *first.get_or_insert(mb >= limits.serve_mem_mb);
            if mb < limits.serve_mem_mb {
                continue;
            }
            if !std::mem::replace(&mut logged, true) {
                let d = serde_json::json!({"rss_mb": mb, "limit_mb": limits.serve_mem_mb, "handover": !fresh});
                append(
                    &ctx.state,
                    &Event::new(crate::now(), "drover", None, "serve-mem-high", d),
                )
                .ok();
            }
            if fresh {
                continue;
            }
            match crate::update::ask_handover(&self.sock, limits.serve_handover_secs) {
                Ok(_) => return,
                Err(e) => drover_core::stderr!("drover serve: no handover past {mb} MB: {e}"),
            }
        }
    }

    fn fds(&self) -> Vec<RawFd> {
        self.listeners.iter().map(|l| l.0).collect()
    }

    fn done(&self) -> bool {
        self.left.load(SeqCst) >= self.listeners.len()
    }

    fn halt(&self) {
        self.stop.store(true, SeqCst);
        let t = drover_core::clock::instant();
        while !self.done() && t.elapsed() < Duration::from_secs(10) {
            for (_, a) in &self.listeners {
                drop(TcpStream::connect_timeout(a, Duration::from_millis(200)));
            }
            drover_core::clock::sleep(Duration::from_millis(10));
        }
    }

    /// The loops stopped: wait until `idle` (requests, deliveries, board jobs), then exec in place or, a bridge, exit.
    pub fn finish(&self, idle: impl Fn() -> bool) -> ! {
        let t = drover_core::clock::instant();
        while !(idle() && self.done()) && t.elapsed() < Duration::from_secs(60) {
            drover_core::clock::sleep(Duration::from_millis(10));
        }
        if self.exec.load(SeqCst) {
            if let Err(e) = crate::runner::hand_fds(&argv(), &self.fds(), false) {
                drover_core::stderr!("drover serve: after the handover: {e:#}");
            }
        }
        drover_core::runner::exit(0)
    }
}

fn argv() -> Vec<std::ffi::OsString> {
    std::env::args_os().collect()
}
