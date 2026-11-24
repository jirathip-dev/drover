use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Seek, Write};
use std::path::Path;

/// A lane's heavy kinds (`[fleet.<n>.heavy]`: kind → command patterns) under its host's slot limits.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct Heavy {
    pub kinds: BTreeMap<String, Vec<String>>,
    pub max: usize,
    pub kind_max: BTreeMap<String, usize>,
    pub wait_minutes: u64,
    /// `[fleet.<n>.remote]`: heavy kind → the build machines `runners run` sends its commands to.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub routes: BTreeMap<String, Route>,
    /// `NEVER_ROUTED` plus the fleet's `remote_deny`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub never: Vec<String>,
}

/// One heavy kind's build machines: `runners run --tag <tag>` with its optional flags.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub tag: String,
    pub wait: Option<u64>,
    pub setup: Option<String>,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub out: Vec<String>,
}

impl Route {
    fn flags(&self, wait: &str) -> Vec<String> {
        let mut a: Vec<String> = (self.wait.iter()).flat_map(|w| [wait.into(), w.to_string()]).collect();
        a.extend(self.setup.iter().flat_map(|s| ["--setup".into(), s.clone()]));
        a.extend(self.env.iter().flat_map(|e| ["--env".into(), e.clone()]));
        a.extend(self.out.iter().flat_map(|o| ["--out".into(), o.clone()]));
        a
    }
}

/// Commands never sent to a build machine, whatever a fleet's patterns: history readers, `gh`, `drover`, signing, release builds, containers, stdin and `.env` readers.
pub const NEVER_ROUTED: &[&str] = &[
    "git *",
    "gh *",
    "drover *",
    "*nx affected*",
    "*--changed*",
    "*merge-base*",
    "*origin/*",
    "*codesign*",
    "*notarytool*",
    "*--release*",
    "cargo install*",
    "docker *",
    "podman *",
    "colima *",
    "orb *",
    "*supabase start*",
    "*<*",
    "*.env*",
];

fn segments(cmd: &str) -> Vec<String> {
    (cmd.split(['\n', ';', '|', '&', '(', ')']))
        .map(|s| {
            let w: Vec<&str> = s.split_whitespace().collect();
            let at = w.iter().position(|w| !w.contains('=') || w.starts_with('-'));
            at.and_then(|i| w.get(i..)).map_or(String::new(), |t| t.join(" "))
        })
        .collect()
}

fn hits(p: &str, s: &str) -> bool {
    crate::config::glob(p.replace('*', "**").as_bytes(), s.as_bytes())
}

/// The `never` entry a routing pattern overlaps (either matches the other), for config load to refuse.
pub fn never_hit<'a>(pattern: &str, never: &'a [String]) -> Option<&'a str> {
    (never.iter())
        .find(|n| hits(n, pattern) || hits(pattern, n))
        .map(String::as_str)
}

impl Heavy {
    /// The first kind with a pattern matching one of `cmd`'s commands (leading `VAR=value` words skipped).
    pub fn kind(&self, cmd: &str) -> Option<&str> {
        let segs = segments(cmd);
        let hit = |p: &String| segs.iter().any(|s| hits(p, s));
        self.kinds
            .iter()
            .find(|(_, ps)| ps.iter().any(hit))
            .map(|(k, _)| k.as_str())
    }

    /// `kind`'s route for `cmd`, unless the line or one of its commands is never routed.
    pub fn route(&self, kind: &str, cmd: &str) -> Option<&Route> {
        let mut segs = segments(cmd);
        segs.push(cmd.trim().into());
        let never = |n: &String| segs.iter().any(|s| hits(n, s));
        self.routes.get(kind).filter(|_| !self.never.iter().any(never))
    }

    /// `cmd` as a shell line that runs it under `exe heavy`, holding a `kind` slot; with a route and the lane's worktree `dir`, on a build machine first.
    pub fn wrap(&self, exe: &str, kind: &str, cmd: &str, dir: Option<&str>) -> String {
        let mut a = [
            exe,
            "heavy",
            kind,
            "--max",
            &self.max.to_string(),
            "--wait",
            &self.wait_minutes.to_string(),
        ]
        .map(String::from)
        .to_vec();
        if let Some(m) = self.kind_max.get(kind) {
            a.extend(["--kind-max".into(), m.to_string()]);
        }
        if let (Some(r), Some(dir)) = (self.route(kind, cmd), dir) {
            a.extend(["--remote".into(), r.tag.clone(), "--dir".into(), dir.into()]);
            a.extend(r.flags("--remote-wait"));
        }
        a.extend(["--".into(), cmd.into()]);
        let q = |w: &String| format!("'{}'", w.replace('\'', r"'\''"));
        a.iter().map(q).collect::<Vec<_>>().join(" ")
    }
}

/// `runners run` for `cmd` in worktree `dir` on route `r`: `Some(code)` once it ran there (the command's own code), else `None` with the reason printed: run it here.
pub fn remote(run: &dyn crate::runner::Runner, r: &Route, dir: &str, cmd: &str) -> Option<i32> {
    let mut a: Vec<String> = ["run", "--tag", &r.tag, "--dir", dir].map(String::from).to_vec();
    a.extend(r.flags("--wait"));
    a.extend(["--", "bash", "-c", cmd].map(String::from));
    let why = match run.attached("runners", &a) {
        Ok(c @ (75 | 76 | 64 | 78)) => format!("exit {c}"),
        Ok(c) => return Some(c),
        Err(e) => match e.downcast_ref::<std::io::Error>().map(std::io::Error::kind) {
            Some(std::io::ErrorKind::NotFound) => "not found".into(),
            _ => format!("{e:#}"),
        },
    };
    crate::stderr!("runners: {why}, running here");
    None
}

/// `drover heavy`: a lane's heavy command, its host slot limits and its route's build machines.
#[derive(clap::Args)]
pub struct Args {
    kind: String,
    #[arg(long)]
    max: usize,
    #[arg(long)]
    kind_max: Option<usize>,
    #[arg(long)]
    wait: u64,
    #[arg(long)]
    remote: Option<String>,
    #[arg(long)]
    dir: Option<String>,
    #[arg(long)]
    remote_wait: Option<u64>,
    #[arg(long)]
    setup: Option<String>,
    #[arg(long)]
    env: Vec<String>,
    #[arg(long)]
    out: Vec<String>,
    #[arg(last = true)]
    cmd: String,
}

impl Args {
    /// Run it on its route's build machines, else as `holder` in slot folder `dir`'s line (`timeout` as `run`'s `late`).
    pub fn run(
        self,
        r: &dyn crate::runner::Runner,
        dir: &Path,
        holder: String,
        timeout: impl FnOnce(&Ticket, &str) -> Option<i32>,
    ) -> Result<i32> {
        if let (Some(tag), Some(wt)) = (self.remote, &self.dir) {
            let (wait, setup, env, out) = (self.remote_wait, self.setup, self.env, self.out);
            let route = Route {
                tag,
                wait,
                setup,
                env,
                out,
            };
            if let Some(code) = remote(r, &route, wt, &self.cmd) {
                return Ok(code);
            }
        }
        let me = Ticket::new(holder, self.kind, self.cmd, self.kind_max);
        run(r, dir, me, self.max, self.wait, &|| None, timeout)
    }
}

/// `runners forget --dir <wt>` for a removed worktree: the failure, if any (no `runners` here is none).
pub fn forget(r: &dyn crate::runner::Runner, wt: &str) -> Option<String> {
    let o = r.run("runners", &crate::runner::argv(&["forget", "--dir", wt]));
    match o {
        _ if crate::runner::not_found(&o) => None,
        Ok(o) if o.code == 0 => None,
        Ok(o) => Some(format!("exit {}: {}", o.code, o.stderr.trim())),
        Err(e) => Some(format!("{e:#}")),
    }
}

/// One command in a host's line, as its file in the slot folder records it (for `drover top`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ticket {
    pub lane: String,
    pub kind: String,
    pub cmd: String,
    /// When it joined the line.
    pub since: u64,
    pub running: bool,
    pub kind_max: Option<usize>,
}

impl Ticket {
    /// Holder `lane`'s `kind` command, in line from now.
    pub fn new(lane: String, kind: String, cmd: String, kind_max: Option<usize>) -> Self {
        let since = crate::now();
        Ticket {
            lane,
            kind,
            cmd,
            since,
            running: false,
            kind_max,
        }
    }
}

/// Every live command in the slot folder `dir`, in line order; a dead holder's file (its lock gone) is removed.
pub fn line(dir: &Path) -> Vec<(String, Ticket)> {
    let mut all: Vec<(String, Ticket)> = (std::fs::read_dir(dir).into_iter().flatten().flatten())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
        .filter_map(|e| {
            let f = File::open(e.path()).ok()?;
            if f.try_lock().is_ok() {
                std::fs::remove_file(e.path()).ok();
                return None;
            }
            let t = serde_json::from_reader(&f).ok()?;
            Some((e.file_name().to_string_lossy().into_owned(), t))
        })
        .collect();
    all.sort_by(|a, b| a.0.cmp(&b.0));
    all
}

fn admitted(dir: &Path, me: &str, max: usize) -> bool {
    let all = line(dir);
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, t) in all.iter().filter(|(_, t)| t.running) {
        *kinds.entry(&t.kind).or_default() += 1;
    }
    let mut used: usize = kinds.values().sum();
    for (n, t) in all.iter().filter(|(_, t)| !t.running) {
        let k = kinds.entry(&t.kind).or_default();
        if used < max && t.kind_max.is_none_or(|m| *k < m) {
            if n == me {
                return true;
            }
            used += 1;
            *k += 1;
        }
    }
    false
}

/// Wait in `dir`'s line as `me` until a slot is free and `busy` names no reason, run its command, free the slot on exit; past `wait_minutes` `late` gets the reason and its `Some(code)` ends the wait without running.
pub fn run(
    r: &dyn crate::runner::Runner,
    dir: &Path,
    mut me: Ticket,
    max: usize,
    wait_minutes: u64,
    busy: &dyn Fn() -> Option<String>,
    late: impl FnOnce(&Ticket, &str) -> Option<i32>,
) -> Result<i32> {
    std::fs::create_dir_all(dir)?;
    let guard = File::create(dir.join(".lock"))?;
    guard.lock()?;
    let nanos = crate::clock::nanos();
    let name = format!("{nanos:020}-{}.json", crate::runner::pid());
    let path = dir.join(&name);
    let mut mine = File::create_new(&path)?;
    mine.lock()?;
    let save = |f: &mut File, t: &Ticket| -> Result<()> {
        f.set_len(0)?;
        f.rewind()?;
        Ok(f.write_all(&serde_json::to_vec(t)?)?)
    };
    save(&mut mine, &me)?;
    guard.unlock()?;
    let limits = crate::config::limits();
    let mut late = Some(late);
    loop {
        guard.lock()?;
        let free = admitted(dir, &name, max);
        guard.unlock()?;
        let why = if free {
            busy()
        } else {
            Some(format!("every heavy slot is taken ({max} on this host)"))
        };
        let timed_out = crate::now() >= me.since + wait_minutes * 60;
        guard.lock()?;
        let go = why.is_none() && admitted(dir, &name, max);
        if !go && timed_out {
            if let Some(code) = late.take().and_then(|f| f(&me, why.as_deref().unwrap_or_default())) {
                guard.unlock()?;
                std::fs::remove_file(&path).ok();
                return Ok(code);
            }
        }
        if go || timed_out {
            me.running = true;
            save(&mut mine, &me)?;
            guard.unlock()?;
            break;
        }
        guard.unlock()?;
        let pause = if free && why.is_some() {
            std::time::Duration::from_secs(limits.hold_sample_secs)
        } else {
            std::time::Duration::from_millis(limits.poll_millis)
        };
        crate::clock::sleep(pause);
    }
    let code = r.attached("bash", &["-c".into(), me.cmd.clone()]);
    std::fs::remove_file(&path).ok();
    code
}

/// `drover hold`: run `cmd` as holder `ci:<repo>` once the main host passes spawn admission and a heavy slot is free; 75 past `wait` minutes.
pub fn hold(ctx: &crate::Ctx, r: &dyn crate::runner::Runner, wait: u64, cmd: &[String]) -> Result<i32> {
    let (prog, args) = cmd.split_first().context("hold: no command")?;
    let cfg = crate::config::Config::load(&ctx.config)?;
    let host = crate::hosts::cards(&cfg, r).into_iter().next().and_then(|c| c.1);
    let repo = crate::config::env("GITHUB_REPOSITORY")
        .and_then(|g| g.rsplit('/').next().map(String::from))
        .or_else(|| {
            Some(
                std::env::current_dir()
                    .ok()?
                    .file_name()?
                    .to_string_lossy()
                    .into_owned(),
            )
        })
        .unwrap_or_default();
    let kind_max = host.and_then(|h| h.heavy_kind_max.get("ci").copied());
    let me = Ticket::new(
        format!("ci:{repo}"),
        "ci".into(),
        crate::hosts::quote(prog, args),
        kind_max,
    );
    let max = host.and_then(|h| h.heavy_max).unwrap_or(3);
    let on = crate::hosts::On::new(r, None);
    let busy = || {
        crate::hosts::gate(ctx, &cfg, &on)
            .1
            .map(|(code, why)| format!("{code}: {why}"))
    };
    let dir = ctx.state.with_file_name("heavy");
    run(r, &dir, me, max, wait, &busy, |_, why| {
        crate::stderr!("drover hold: waited {wait} min, not run: {why}");
        Some(75)
    })
}
