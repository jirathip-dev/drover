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
}

pub struct Real;

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
        let (so, se) = (drain(c.stdout.take()), drain(c.stderr.take()));
        let end = limit.and_then(|l| Instant::now().checked_add(l));
        let st = loop {
            match (c.try_wait()?, end) {
                (Some(st), _) => break st,
                (None, None) => break c.wait()?,
                (None, Some(end)) if Instant::now() >= end => {
                    let _ = (c.kill(), c.wait()); // its readers may outlive it while a grandchild (ssh) holds the pipes
                    let stderr = format!(
                        "{prog} {}: no answer in {:?}",
                        args.join(" "),
                        limit.unwrap_or_default()
                    );
                    return Ok(Out {
                        code: 124,
                        stderr,
                        ..Out::default()
                    });
                }
                _ => std::thread::sleep(Duration::from_millis(50)),
            }
        };
        Ok(Out {
            code: st.code().unwrap_or(-1),
            stdout: so.join().unwrap_or_default(),
            stderr: se.join().unwrap_or_default(),
        })
    }
}

/// #478: another host's `herdr --machine m … list` (its ssh has no timeout of its own) answers in 20 s.
pub fn limit(prog: &str, args: &[String]) -> Option<Duration> {
    let remote = prog == crate::herdr::BIN && args.first().is_some_and(|a| a == "--machine");
    (remote && args.get(3).is_some_and(|a| a == "list")).then(|| Duration::from_secs(20))
}

fn drain(p: Option<impl std::io::Read + Send + 'static>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut v = vec![];
        let _ = p.map(|mut p| p.read_to_end(&mut v));
        String::from_utf8_lossy(&v).into_owned()
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
