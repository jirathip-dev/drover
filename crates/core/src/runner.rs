//! The one seam for external tools (herdr, gh, git). Tests swap in a fake.
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default)]
pub struct Out {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub trait Runner: Sync {
    fn run(&self, prog: &str, args: &[String]) -> Result<Out>;
    /// `run` with `input` on stdin (secrets go here, never in argv); fakes ignore it.
    fn run_in(&self, prog: &str, args: &[String], _input: &str) -> Result<Out> {
        self.run(prog, args)
    }
    /// #561: `run_in`, its child killed (exit 124) once `limit` has passed; fakes answer at once.
    fn run_for(&self, prog: &str, args: &[String], input: &str, _limit: Duration) -> Result<Out> {
        self.run_in(prog, args, input)
    }
    /// Pause between polls/retries; fakes skip it.
    fn sleep(&self, secs: u64) {
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }
    /// #340: start `exe args` detached, stdin null, both streams appended to `log`; its pid. `Real` gives it its own
    /// session, out of reach of launchd's group kill when the tick/fast job exits; fakes see this `sh -c nohup` line.
    fn detach(&self, exe: &str, args: &[String], log: &std::path::Path) -> Result<u32> {
        let (args, log) = (args.join(" "), log.display());
        let sh = format!("nohup '{exe}' {args} >> '{log}' 2>&1 < /dev/null & echo $!");
        Ok(self.run("sh", &argv(&["-c", &sh]))?.stdout.trim().parse()?)
    }
    /// #298: the shed runtime this runner's lanes use (`[runtime] kind = "shed"`); None: herdr only.
    fn shed(&self) -> Option<crate::shed::Shed> {
        None
    }
    /// #529: whether `herdr` runs on this host's own (the service's streamed agent list stands for its `agent list`);
    /// fakes and other hosts: no.
    fn local(&self) -> bool {
        false
    }
    /// #637: `prog args` with `input` on stdin, its stdout as it arrives (the token proxy relays a streamed model call);
    /// the child is reaped when the reader drops. Fakes run the real child too: the proxy's tests use a local upstream.
    fn stream(&self, prog: &str, args: &[String], input: &str) -> Result<Box<dyn std::io::Read + Send>> {
        Real::stream(prog, args, input)
    }
    /// #707: herdsman keeps no secret store (`crate::secret` reads 0600 files); always refused. Kept only while the
    /// frozen board (#552) still forwards it.
    fn secret(&self, svc: &str) -> Result<Option<String>> {
        bail!("herdsman keeps no secret store (wanted {svc})")
    }
    /// #707: as `secret`.
    fn set_secret(&self, svc: &str, _value: Option<&str>) -> Result<()> {
        bail!("herdsman keeps no secret store (wanted {svc})")
    }
}

pub struct Real;

impl Runner for Real {
    fn local(&self) -> bool {
        true
    }
    fn run(&self, prog: &str, args: &[String]) -> Result<Out> {
        self.run_in(prog, args, "")
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> Result<Out> {
        self.within(prog, args, input.as_bytes(), limit(prog, args))
    }
    fn run_for(&self, prog: &str, args: &[String], input: &str, limit: Duration) -> Result<Out> {
        self.within(prog, args, input.as_bytes(), limit)
    }
    fn detach(&self, exe: &str, args: &[String], log: &std::path::Path) -> Result<u32> {
        use std::os::unix::process::CommandExt;
        extern "C" {
            fn setsid() -> i32;
        }
        let log = std::fs::File::options().create(true).append(true).open(log)?;
        let mut c = Command::new(exe);
        c.args(args)
            .stdin(std::process::Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        // SAFETY: setsid is async-signal-safe; between fork and exec it touches nothing else.
        unsafe {
            c.pre_exec(|| match setsid() {
                -1 => Err(std::io::Error::last_os_error()),
                _ => Ok(()),
            });
        }
        let mut child = c.spawn().with_context(|| format!("cannot run {exe}"))?;
        let pid = child.id();
        std::thread::spawn(move || child.wait()); // a long-lived parent (serve) reaps it
        Ok(pid)
    }
}

/// #637: a streamed child's stdout; dropping it waits for the child.
struct Streamed(std::process::Child, std::process::ChildStdout);

impl std::io::Read for Streamed {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.1.read(buf)
    }
}

impl Drop for Streamed {
    fn drop(&mut self) {
        let _ = self.0.wait();
    }
}

impl Real {
    fn stream(prog: &str, args: &[String], input: &str) -> Result<Box<dyn std::io::Read + Send>> {
        use std::io::Write;
        use std::process::Stdio;
        let mut c = Command::new(prog)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .with_context(|| format!("cannot run {prog}"))?;
        c.stdin.take().context("stdin")?.write_all(input.as_bytes())?;
        let out = c.stdout.take().context("stdout")?;
        Ok(Box::new(Streamed(c, out)))
    }
    /// `run_in` with bytes on stdin, killed (exit 124) once `limit` has passed.
    pub fn within(&self, prog: &str, args: &[String], input: &[u8], limit: Duration) -> Result<Out> {
        use std::io::Write;
        use std::process::Stdio;
        // #776: a `-C <dir>` on a disk that stopped answering fails at once; its child would never exit.
        if let Some(dir) = args.iter().position(|a| a == "-C").and_then(|i| args.get(i + 1)) {
            if hung(dir, PROBE, list) {
                let stderr = format!("{prog} {}: {dir} is on a disk that is not answering", args.join(" "));
                return Ok(Out {
                    code: 124,
                    stderr,
                    ..Out::default()
                });
            }
        }
        let mut c = Command::new(prog)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("cannot run {prog}"))?;
        let _ = c.stdin.take().unwrap().write_all(input);
        let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        let (so, se) = (drain(c.stdout.take()), drain(c.stderr.take()));
        let end = Instant::now() + limit;
        while Instant::now() < end {
            if let Some(st) = c.try_wait()? {
                let (stdout, stderr) = (
                    text(&so.join().unwrap_or_default()),
                    text(&se.join().unwrap_or_default()),
                );
                return Ok(Out {
                    code: st.code().unwrap_or(-1),
                    stdout,
                    stderr,
                });
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // #775: reaped off this thread: a child stuck in disk I/O outlives SIGKILL, and waiting here would never return.
        // Its readers may outlive it too while a grandchild (ssh) holds the pipes.
        let _ = c.kill();
        std::thread::spawn(move || c.wait());
        // #776: a `-C <dir>` call that ran out of time marks its whole disk: a folder macOS still lists from memory
        // passes the probe while its files hang.
        if let Some(dir) = args.iter().position(|a| a == "-C").and_then(|i| args.get(i + 1)) {
            sicken(dir);
        }
        let stderr = format!("{prog} {}: no answer in {:?}", args.join(" "), limit);
        Ok(Out {
            code: 124,
            stderr,
            ..Out::default()
        })
    }
}

/// #713: every call's time limit by the program's file name, unless `run_for` gives one: a call that hits it is
/// killed and exits 124.
/// - `herdr --machine m … list`: 20 s; #478: another host's (its ssh has no timeout of its own) must not hold a tick.
/// - `security`, `codesign`: 10 s; they wait on the login Keychain, which can stop answering (11-08).
/// - `herdr … --timeout <ms>`: its own timeout and 30 s; `agent start` and the waits run up to it.
/// - `git clone|fetch|pull|push`: 10 min; a large repo over the network.
/// - `cp`, `rm`: 10 min; copying (spawn's build cache) or deleting a worktree's build folder of many GB.
/// - `cargo`: 30 min; self-update's release build.
/// - `ssh` through `hosts::On`: the limit of the command it carries; `hosts bootstrap`'s installs 30 min by `run_for`.
/// - anything else: 60 s.
pub fn limit(prog: &str, args: &[String]) -> Duration {
    let a = |i: usize| args.get(i).map_or("", String::as_str);
    let net = |w: &String| ["clone", "fetch", "pull", "push"].contains(&w.as_str());
    let wait = args
        .iter()
        .position(|w| w == "--timeout")
        .and_then(|i| a(i + 1).parse::<u64>().ok());
    let name = std::path::Path::new(prog)
        .file_name()
        .map_or(prog.into(), |n| n.to_string_lossy());
    let secs = match &*name {
        p if p == crate::herdr::BIN && a(0) == "--machine" && a(3) == "list" => 20,
        "security" | "codesign" => 10,
        p if p == crate::herdr::BIN => wait.map_or(60, |ms| ms / 1000 + 30),
        "git" if args.iter().any(net) => 600,
        "cp" | "rm" => 600,
        "cargo" => 1800,
        _ => 60,
    };
    Duration::from_secs(secs)
}

/// #776: how long a folder's listing may take before its disk counts as not answering.
pub const PROBE: Duration = Duration::from_secs(2);

/// #776: how long a disk that let a call run out of time stays hung: every path on it fails at once until then.
pub const SICK: Duration = Duration::from_secs(300);

/// #776: the probe `Real` uses: `dir`'s disk (its `st_dev`) and the first entry of its listing.
fn list(dir: &str) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    let dev = std::fs::metadata(dir).ok()?.dev();
    drop(std::fs::read_dir(dir).map(|mut e| e.next()));
    Some(dev)
}

/// A folder's last probe: whether it ended, the disk it named (0: none) and when it started.
struct Probe {
    done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    dev: std::sync::Arc<std::sync::atomic::AtomicU64>,
    at: Instant,
}

fn probes() -> std::sync::MutexGuard<'static, std::collections::HashMap<String, Probe>> {
    static SEEN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, Probe>>> =
        std::sync::OnceLock::new();
    let m = SEEN.get_or_init(std::sync::Mutex::default);
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn sick() -> std::sync::MutexGuard<'static, std::collections::HashMap<u64, Instant>> {
    static SICK_AT: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<u64, Instant>>> =
        std::sync::OnceLock::new();
    let m = SICK_AT.get_or_init(std::sync::Mutex::default);
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// #776: a hung disk's mark, shared by every herdsman process (drive and tick start fresh each run, so a mark kept
/// only in memory costs each run one timed-out call and one stuck child): `~/.cache/herdsman/sick-disks/<dev>`,
/// fresh for `SICK` after its last write.
fn mark(dev: u64) -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        std::path::Path::new(&home)
            .join(".cache/herdsman/sick-disks")
            .join(dev.to_string()),
    )
}

fn is_sick(dev: u64) -> bool {
    let fresh = |t: std::time::SystemTime| t.elapsed().is_ok_and(|e| e < SICK);
    dev != 0
        && (sick().get(&dev).is_some_and(|t| t.elapsed() < SICK)
            || mark(dev)
                .and_then(|p| p.metadata().ok()?.modified().ok())
                .is_some_and(fresh))
}

/// #776: mark the disk `dir`'s last probe named as hung for `SICK` (a call on it ran out of time).
pub fn sicken(dir: &str) {
    use std::sync::atomic::Ordering::Relaxed;
    let dev = probes().get(dir).map_or(0, |p| p.dev.load(Relaxed));
    if dev != 0 {
        sick().insert(dev, Instant::now());
        if let Some(p) = mark(dev) {
            let _ = p.parent().map(std::fs::create_dir_all);
            let _ = std::fs::write(p, b"");
        }
    }
}

/// #776: whether `dir`'s disk has stopped answering: `probe(dir)` (its disk and a listing) runs on its own thread and
/// must finish within `wait`. A probe still running is never repeated: the folder stays hung, with no new thread,
/// until it ends. A folder whose disk `sicken` marked is hung; one that answered is otherwise trusted for a minute.
pub fn hung(dir: &str, wait: Duration, probe: impl FnOnce(&str) -> Option<u64> + Send + 'static) -> bool {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
    use std::sync::Arc;
    let (done, dev) = {
        let mut m = probes();
        if let Some(p) = m.get(dir) {
            if !p.done.load(Relaxed) || is_sick(p.dev.load(Relaxed)) {
                return true;
            }
            if p.at.elapsed() < Duration::from_secs(60) {
                return false;
            }
        }
        let (done, dev) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicU64::new(0)));
        m.insert(
            dir.into(),
            Probe {
                done: done.clone(),
                dev: dev.clone(),
                at: Instant::now(),
            },
        );
        (done, dev)
    };
    let (d, v, owned) = (done.clone(), dev.clone(), dir.to_string());
    std::thread::spawn(move || {
        v.store(probe(&owned).unwrap_or(0), Relaxed);
        d.store(true, Relaxed);
    });
    let end = Instant::now() + wait;
    while Instant::now() < end {
        if done.load(Relaxed) {
            return is_sick(dev.load(Relaxed));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    !done.load(Relaxed)
}

/// #775: `path`'s bytes for a request thread (serve's screenshots, designs, thread files): None when its folder is
/// hung (#776) or the read takes over `PROBE`, which marks the folder's disk; the stuck read keeps its own thread.
pub fn read_file(path: impl AsRef<std::path::Path>) -> Option<Vec<u8>> {
    let path = path.as_ref().to_path_buf();
    let dir = path.parent()?.to_string_lossy().into_owned();
    if hung(&dir, PROBE, list) {
        return None;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || drop(tx.send(std::fs::read(path).ok())));
    rx.recv_timeout(PROBE).unwrap_or_else(|_| {
        sicken(&dir);
        None
    })
}

fn drain(p: Option<impl std::io::Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut v = vec![];
        let _ = p.map(|mut p| p.read_to_end(&mut v));
        v
    })
}

pub fn argv(args: &[&str]) -> Vec<String> {
    args.iter().map(std::string::ToString::to_string).collect()
}

/// Run and fail on a non-zero exit; returns stdout.
pub fn ok<S: AsRef<str>>(r: &dyn Runner, prog: &str, args: &[S]) -> Result<String> {
    let args: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
    let o = r.run(prog, &argv(&args))?;
    if o.code != 0 {
        bail!("{prog} {} failed ({}): {}", args.join(" "), o.code, o.stderr.trim());
    }
    Ok(o.stdout)
}

pub fn json<S: AsRef<str>>(r: &dyn Runner, prog: &str, args: &[S]) -> Result<Value> {
    let s = ok(r, prog, args)?;
    serde_json::from_str(&s).with_context(|| {
        format!(
            "{prog} {}: not JSON",
            args.iter().map(AsRef::as_ref).collect::<Vec<_>>().join(" ")
        )
    })
}
