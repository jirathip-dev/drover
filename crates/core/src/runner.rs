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
    /// #441: keychain service `svc`'s secret in the main host's OS store (None: not set); fakes and wrappers forward.
    fn secret(&self, svc: &str) -> Result<Option<String>> {
        bail!("no secret store on this host (wanted {svc})")
    }
    /// #441: save `svc`'s secret (None: delete it) in the main host's OS store.
    fn set_secret(&self, svc: &str, _value: Option<&str>) -> Result<()> {
        bail!("no secret store on this host (wanted {svc})")
    }
}

pub struct Real;

/// #441: the OS store's entry for `svc`, under the `herdsman` account `security` used (#387), so old items keep working.
fn entry(svc: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(svc, "herdsman").with_context(|| format!("secret store: {svc}"))
}

impl Runner for Real {
    fn run(&self, prog: &str, args: &[String]) -> Result<Out> {
        self.run_in(prog, args, "")
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> Result<Out> {
        self.within(prog, args, input, limit(prog, args))
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
    fn secret(&self, svc: &str) -> Result<Option<String>> {
        store_get(self, svc)
    }
    fn set_secret(&self, svc: &str, value: Option<&str>) -> Result<()> {
        store_set(self, svc, value)
    }
}

/// #441: `svc`'s secret in the OS store (None: not set); #548: on `r`'s clock, `KEYCHAIN_LOCKED` past `KEYCHAIN_SECS`.
pub fn store_get(r: &dyn Runner, svc: &str) -> Result<Option<String>> {
    let e = entry(svc)?;
    match keychain(r, svc, move || e.get_password())? {
        Err(keyring::Error::NoEntry) => Ok(None),
        x => Ok(Some(x.with_context(|| format!("secret store: {svc}"))?)),
    }
}

/// #441: save `svc`'s secret (None: delete it) in the OS store; #548: as `store_get`, never blocking.
pub fn store_set(r: &dyn Runner, svc: &str, value: Option<&str>) -> Result<()> {
    let (e, v) = (entry(svc)?, value.map(String::from));
    let done = keychain(r, svc, move || {
        v.map_or_else(|| e.delete_credential(), |v| e.set_password(&v))
    })?;
    done.with_context(|| format!("secret store: {svc}"))
}

/// #548: how long a keychain call may wait (a macOS access dialog nobody clicks blocks it for good).
pub const KEYCHAIN_SECS: u64 = 5;

/// #548: `f` (a keychain call on `svc`) on a worker thread, given `KEYCHAIN_SECS` (`r`'s clock) to answer; past that
/// `KEYCHAIN_LOCKED` with the fix, and the worker is left waiting on the dialog.
fn keychain<T: Send + 'static>(r: &dyn Runner, svc: &str, f: impl FnOnce() -> T + Send + 'static) -> Result<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || tx.send(f()));
    let mut got = rx.recv_timeout(Duration::from_millis(200)).ok();
    for _ in 0..KEYCHAIN_SECS {
        if got.is_some() {
            break;
        }
        r.sleep(1);
        got = rx.try_recv().ok();
    }
    got.map_or_else(|| locked(r, svc), Ok)
}

/// #548: the one-time fix that lets the herdsman with code hash `cdhash` read `svc` without a prompt: its partition
/// list admits that build (amendment A: an ad-hoc signed rebuild matches only its own `cdhash:`).
pub fn keychain_fix(cdhash: Option<&str>, svc: &str) -> String {
    let cd = cdhash.map(|h| format!(",cdhash:{h}")).unwrap_or_default();
    format!("security set-generic-password-partition-list -S apple-tool:,apple:,unsigned:{cd} -s {svc} -a herdsman")
}

/// #548: this binary's code hash (`codesign`'s `CDHash=`); None: unsigned or no `codesign` (not macOS).
pub fn cdhash(r: &dyn Runner) -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let o = r.run("codesign", &argv(&["-dvvv", &exe.to_string_lossy()])).ok()?;
    let all = o.stdout + &o.stderr;
    all.lines().find_map(|l| l.strip_prefix("CDHash=")).map(String::from)
}

/// #548: each `herdsman/` item's partition list in `security dump-keychain -a` output: (service, `apple-tool:, …`).
pub fn partitions(dump: &str) -> Vec<(String, String)> {
    let (mut out, mut svc, mut next) = (Vec::new(), None, false);
    for l in dump.lines().map(str::trim) {
        if l.starts_with("keychain: ") {
            (svc, next) = (None, false);
        } else if let Some(s) = l.strip_prefix("\"svce\"<blob>=\"").and_then(|s| s.strip_suffix('"')) {
            svc = s.starts_with("herdsman/").then(|| s.to_string());
        } else if l.ends_with("): partition_id") {
            next = true;
        } else if let (true, Some(p), Some(s)) = (next, l.strip_prefix("description: "), &svc) {
            out.push((s.clone(), p.to_string()));
            next = false;
        }
    }
    out
}

fn locked<T>(r: &dyn Runner, svc: &str) -> Result<T> {
    let host = ok(r, "hostname", &["-s"]).map_or_else(|_| "this host".into(), |h| h.trim().to_string());
    crate::gate::refused(
        "KEYCHAIN_LOCKED",
        format!(
            "approve herdsman in the Keychain dialog on {host}, or run {}",
            keychain_fix(cdhash(r).as_deref(), svc)
        ),
    )
}

impl Real {
    /// `run_in`, killed (exit 124) once `limit` has passed.
    pub fn within(&self, prog: &str, args: &[String], input: &str, limit: Option<Duration>) -> Result<Out> {
        use std::io::Write;
        use std::process::Stdio;
        let mut c = Command::new(prog)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("cannot run {prog}"))?;
        let _ = c.stdin.take().unwrap().write_all(input.as_bytes());
        let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        let Some(limit) = limit else {
            let o = c.wait_with_output()?;
            let code = o.status.code().unwrap_or(-1);
            return Ok(Out {
                code,
                stdout: text(&o.stdout),
                stderr: text(&o.stderr),
            });
        };
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
        let _ = (c.kill(), c.wait()); // its readers may outlive it while a grandchild (ssh) holds the pipes
        let stderr = format!("{prog} {}: no answer in {:?}", args.join(" "), limit);
        Ok(Out {
            code: 124,
            stderr,
            ..Out::default()
        })
    }
}

/// #478: another host's `herdr --machine m … list` (its ssh has no timeout of its own) answers in 20 s.
pub fn limit(prog: &str, args: &[String]) -> Option<Duration> {
    let remote = prog == crate::herdr::BIN && args.first().is_some_and(|a| a == "--machine");
    (remote && args.get(3).is_some_and(|a| a == "list")).then(|| Duration::from_secs(20))
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
