//! The one seam for external tools (herdr, gh, git). Tests swap in a fake.
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::process::Command;

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
        let o = c.wait_with_output()?;
        let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        Ok(Out {
            code: o.status.code().unwrap_or(-1),
            stdout: text(&o.stdout),
            stderr: text(&o.stderr),
        })
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
        match entry(svc)?.get_password() {
            Err(keyring::Error::NoEntry) => Ok(None),
            x => Ok(Some(x.with_context(|| format!("secret store: {svc}"))?)),
        }
    }
    fn set_secret(&self, svc: &str, value: Option<&str>) -> Result<()> {
        let e = entry(svc)?;
        let done = value.map_or_else(|| e.delete_credential(), |v| e.set_password(v));
        done.with_context(|| format!("secret store: {svc}"))
    }
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
