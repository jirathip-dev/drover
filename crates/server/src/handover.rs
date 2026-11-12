use crate::Ctx;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::time::{Duration, Instant};

/// #894: one serve's side of the socket handover (docs/setup.md): its listeners, control socket and drain state.
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
            sock: crate::update::serve_sock(ctx, listeners[0].1.port()),
            listeners,
            inherited,
            stop: AtomicBool::new(false),
            left: AtomicUsize::new(0),
            exec: AtomicBool::new(false),
        })
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
            let _ = c.set_read_timeout(Some(Duration::from_secs(10)));
            let _ = writeln!(c, "ready {}", std::process::id());
            let _ = BufReader::new(c).read_line(&mut String::new());
        }
        let _ = std::fs::remove_file(&self.sock);
        let bound = UnixListener::bind(&self.sock);
        bound
            .map_err(|e| eprintln!("herdsman serve: no handover: {}: {e}", self.sock.display()))
            .ok()
    }

    /// `handover` starts a bridge on the listeners; its `ready <pid>` answers the asker `ok <pid>` and this one drains.
    pub fn control(&self, sock: &UnixListener) {
        let mut asked: Option<UnixStream> = None;
        for mut c in sock.incoming().flatten() {
            let mut line = String::new();
            let _ = c.set_read_timeout(Some(Duration::from_secs(5)));
            let _ = BufReader::new(&c).read_line(&mut line);
            let (verb, pid) = line.trim().split_once(' ').unwrap_or((line.trim(), ""));
            match verb {
                "handover" if asked.is_none() => match crate::runner::hand_fds(&argv(), &self.fds(), true) {
                    Ok(()) => asked = Some(c),
                    Err(e) => drop(writeln!(c, "error: the bridge: {e:#}")),
                },
                "ready" => {
                    let _ = writeln!(c, "ok");
                    if let Some(mut a) = asked.take() {
                        let _ = writeln!(a, "ok {pid}");
                        self.exec.store(true, SeqCst);
                    }
                    return self.halt();
                }
                "handover" => drop(writeln!(c, "error: a handover is under way")),
                _ => drop(writeln!(c, "error: unknown request {verb:?}")),
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
        let t = Instant::now();
        while !self.done() && t.elapsed() < Duration::from_secs(10) {
            for (_, a) in &self.listeners {
                drop(TcpStream::connect_timeout(a, Duration::from_millis(200)));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The loops stopped: wait out the requests in flight (`busy`), then exec in place (the supervisor's pid) or exit.
    pub fn finish(&self, busy: &AtomicUsize) -> ! {
        let t = Instant::now();
        while (busy.load(SeqCst) > 0 || !self.done()) && t.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(10));
        }
        if self.exec.load(SeqCst) {
            if let Err(e) = crate::runner::hand_fds(&argv(), &self.fds(), false) {
                eprintln!("herdsman serve: after the handover: {e:#}");
            }
        }
        std::process::exit(0)
    }
}

fn argv() -> Vec<std::ffi::OsString> {
    std::env::args_os().collect()
}
