use crate::config::{expand, set_in, write_atomic, Config, Fleet, HostCfg};
use crate::events::{age, append, emit, fold, last_spawn, open_asks, working, Event, IssueState, Json};
use crate::gate::refused;
use crate::host::{self, Host};
use crate::providers;
use crate::runner::{argv, ok, Out, Runner};
use crate::session::{Session, Status};
use crate::Ctx;
use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::cmp::Ordering::Equal;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `prog args` as one shell line, each word single-quoted.
pub fn quote(prog: &str, args: &[String]) -> String {
    let q = |a: &str| format!("'{}'", a.replace('\'', "'\\''"));
    std::iter::once(q(prog))
        .chain(args.iter().map(|a| q(a)))
        .collect::<Vec<_>>()
        .join(" ")
}

const CM: [&str; 3] = [
    "-oControlMaster=auto",
    "-oControlPath=/tmp/drover-ssh-%C",
    "-oControlPersist=10m",
];

/// `On::existing`'s script: the paths on stdin, a line each.
pub const EXISTS: &str = "while IFS= read -r p; do test -e \"$p\" && echo y || echo n; done";

/// The worktrees of `spawns` that exist, as `(machine, worktree)`: one `existing` call per host.
pub fn present<'e>(
    r: &dyn Runner,
    spawns: impl IntoIterator<Item = &'e Event>,
) -> std::collections::BTreeSet<(Option<&'e str>, &'e str)> {
    let mut by: BTreeMap<Option<&str>, Vec<&str>> = BTreeMap::new();
    for e in spawns {
        if let Some(w) = e.str("worktree") {
            by.entry(e.str("machine")).or_default().push(w);
        }
    }
    let mut there = std::collections::BTreeSet::new();
    for (m, mut ws) in by {
        ws.sort_unstable();
        ws.dedup();
        there.extend(On::new(r, m).existing(&ws).into_iter().map(|w| (m, w)));
    }
    there
}

/// A lane's host; `machine` None is this one (every call and file exactly as before).
pub struct On<'a> {
    pub r: &'a dyn Runner,
    pub machine: Option<&'a str>,
    connect: u64,
}

impl<'a> On<'a> {
    pub fn new(r: &'a dyn Runner, machine: Option<&'a str>) -> Self {
        On {
            r,
            machine,
            connect: crate::config::limits().ssh_connect_secs,
        }
    }
    /// A maintenance call that must not hold up a pass: a 5 s connect timeout.
    pub fn quick(r: &'a dyn Runner, machine: Option<&'a str>) -> Self {
        On {
            connect: crate::config::limits().ssh_quick_secs,
            ..Self::new(r, machine)
        }
    }
    /// The host a `spawn` event's lane runs on (none: this one).
    pub fn of(r: &'a dyn Runner, e: Option<&'a Event>) -> Self {
        Self::new(r, e.and_then(|e| e.str("machine")))
    }
    /// The program and argv actually run for `prog args` on this host.
    pub fn wrap(&self, prog: &str, args: &[String]) -> (String, Vec<String>) {
        let Some(m) = self.machine else {
            return (prog.into(), args.to_vec());
        };
        let line = quote(prog, args);
        let connect = format!("-oConnectTimeout={}", self.connect);
        let ssh = argv(&["-oBatchMode=yes", &connect, CM[0], CM[1], CM[2], m, "--", &line]);
        ("ssh".into(), ssh)
    }
    pub(crate) fn sh(&self, script: &str, p: &Path, input: &str) -> Result<Out> {
        self.run_in("sh", &argv(&["-c", script, "sh", &p.to_string_lossy()]), input)
    }
    pub fn exists(&self, p: &Path) -> bool {
        match self.machine {
            None => p.exists(),
            Some(_) => self.sh("test -e \"$1\"", p, "").is_ok_and(|o| o.code == 0),
        }
    }
    /// Which of `paths` exist here: one call (one ssh to a remote host), a `y` or `n` line back per path.
    pub fn existing<'p>(&self, paths: &[&'p str]) -> std::collections::BTreeSet<&'p str> {
        if self.machine.is_none() || paths.is_empty() {
            return paths.iter().copied().filter(|p| Path::new(p).exists()).collect();
        }
        let input: String = paths.iter().map(|p| format!("{p}\n")).collect();
        let o = self.run_in("sh", &argv(&["-c", EXISTS]), &input);
        let o = o.ok().filter(|o| o.code == 0).map(|o| o.stdout).unwrap_or_default();
        let seen: Vec<&str> = o.lines().collect();
        if seen.len() != paths.len() {
            return std::collections::BTreeSet::new();
        }
        paths
            .iter()
            .zip(seen)
            .filter(|(_, y)| *y == "y")
            .map(|(p, _)| *p)
            .collect()
    }
    pub fn read(&self, p: &Path) -> Option<String> {
        match self.machine {
            None => std::fs::read_to_string(p).ok(),
            Some(_) => ok(self, "cat", &[p.to_string_lossy()]).ok(),
        }
    }
    /// Write `text` to `p`, its folder made first.
    pub fn write(&self, p: &Path, text: &str) -> Result<()> {
        if self.machine.is_none() {
            if let Some(d) = p.parent() {
                std::fs::create_dir_all(d)?;
            }
            return Ok(std::fs::write(p, text)?);
        }
        let o = self.sh("mkdir -p \"$(dirname \"$1\")\" && cat > \"$1\"", p, text)?;
        ensure!(o.code == 0, "writing {}: {}", p.display(), o.stderr.trim());
        Ok(())
    }
    /// `write` a file only its owner reads (a lane's secrets).
    pub fn write_private(&self, p: &Path, text: &str) -> Result<()> {
        if self.machine.is_none() {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            if let Some(d) = p.parent() {
                std::fs::create_dir_all(d)?;
            }
            let mut f = std::fs::File::options()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(p)?;
            return Ok(f.write_all(text.as_bytes())?);
        }
        let o = self.sh("umask 077 && mkdir -p \"$(dirname \"$1\")\" && cat > \"$1\"", p, text)?;
        ensure!(o.code == 0, "writing {}: {}", p.display(), o.stderr.trim());
        Ok(())
    }
    /// Remove the file `p`; a missing one is fine.
    pub fn remove(&self, p: &Path) -> Result<()> {
        match self.machine {
            None if p.exists() => std::fs::remove_file(p)?,
            None => {}
            Some(_) => drop(ok(self, "rm", &["-f".into(), p.to_string_lossy()])?),
        }
        Ok(())
    }
}

impl Runner for On<'_> {
    fn run(&self, prog: &str, args: &[String]) -> Result<Out> {
        let (p, a) = self.wrap(prog, args);
        if p == "ssh" {
            return self.r.run_for(&p, &a, "", crate::runner::limit(prog, args));
        }
        self.r.run(&p, &a)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> Result<Out> {
        let (p, a) = self.wrap(prog, args);
        if p == "ssh" {
            return self.r.run_for(&p, &a, input, crate::runner::limit(prog, args));
        }
        self.r.run_in(&p, &a, input)
    }
    fn run_for(&self, prog: &str, args: &[String], input: &str, limit: std::time::Duration) -> Result<Out> {
        let (p, a) = self.wrap(prog, args);
        self.r.run_for(&p, &a, input, limit)
    }
    fn sleep(&self, secs: u64) {
        self.r.sleep(secs);
    }
    fn shed(&self) -> Option<crate::shed::Shed> {
        let b = self.r.shed()?;
        let wire = if self.machine.is_some() {
            crate::shed::Wire::Connect
        } else {
            b.wire
        };
        Some(crate::shed::Shed { wire, ..b })
    }
}

/// A check, clean or drive pass's runner. Each remote host is probed once (`ssh … true`, 5 s, no shared connection); an unreachable one answers 255 at once for the rest of the pass. Every remote call is killed after `ssh_call_secs`, and its host is offline for the rest of the pass; one refused through a dead shared connection drops its socket and runs once more without it. a machine no `[hosts]` entry names is never contacted. `mark` (the tick's `tick.step`, with the tick's start) names the step and the child it waits on, for `tick-stuck`.
pub struct Pass<'a> {
    r: &'a dyn Runner,
    probe: bool,
    up: std::sync::Mutex<BTreeMap<String, bool>>,
    mark: Option<(PathBuf, u64)>,
    step: std::sync::Mutex<(String, u64)>,
    t0: std::time::Instant,
    pub until: Option<std::time::Instant>,
    late: std::sync::Mutex<std::collections::BTreeSet<String>>,
    unsent: std::sync::atomic::AtomicU64,
    slowest: std::sync::Mutex<(String, u64)>,
    off: std::collections::BTreeSet<String>,
    log: Option<(&'a Ctx, &'a Config)>,
}

impl<'a> Pass<'a> {
    pub fn new(r: &'a dyn Runner, mark: Option<(PathBuf, u64)>) -> Self {
        let (up, step) = (std::sync::Mutex::default(), std::sync::Mutex::default());
        Pass {
            r,
            probe: true,
            up,
            mark,
            step,
            t0: crate::clock::instant(),
            until: None,
            late: std::sync::Mutex::default(),
            unsent: std::sync::atomic::AtomicU64::new(0),
            slowest: std::sync::Mutex::default(),
            off: std::collections::BTreeSet::new(),
            log: None,
        }
    }
    /// Never contact the machines `off`; a host's changed key logs `host-key-changed` once (with `log`).
    #[must_use]
    pub fn sparing(self, off: std::collections::BTreeSet<String>, log: Option<(&'a Ctx, &'a Config)>) -> Self {
        Pass { off, log, ..self }
    }
    pub fn over(&self) -> bool {
        self.until.is_some_and(|t| crate::clock::instant() >= t)
    }
    /// The hosts over budget this pass: a call there killed at its limit, or held back past the pass's budget.
    pub fn late(&self) -> Vec<String> {
        lock(&self.late).iter().cloned().collect()
    }
    /// The remote calls this pass did not send.
    pub fn unsent(&self) -> u64 {
        self.unsent.load(std::sync::atomic::Ordering::Relaxed)
    }
    /// The pass's slowest step so far and its seconds.
    pub fn slowest(&self) -> (String, u64) {
        let (step, at) = lock(&self.step).clone();
        let now = (step, self.t0.elapsed().as_secs().saturating_sub(at));
        let done = lock(&self.slowest).clone();
        if now.1 >= done.1 {
            now
        } else {
            done
        }
    }
    /// The fast step's one remote call per host, no probe; a call killed at 15 s marks its host down.
    pub fn fast(r: &'a dyn Runner) -> Self {
        Pass {
            probe: false,
            ..Self::new(r, None)
        }
    }
    pub fn step(&self, s: &str) {
        debug_assert!(
            crate::timer::TICK_STEPS.iter().any(|t| t.0 == s),
            "{s} is not in TICK_STEPS"
        );
        let ran = self.slowest();
        let mut slowest = lock(&self.slowest);
        if ran.1 >= slowest.1 {
            *slowest = ran;
        }
        drop(slowest);
        *lock(&self.step) = (s.into(), self.t0.elapsed().as_secs());
        self.note("");
    }
    fn note(&self, child: &str) {
        if let Some((p, since)) = &self.mark {
            let (step, at) = lock(&self.step).clone();
            let at = since + at;
            let m = json!({"since": since, "at": at, "step": step, "child": child, "pid": crate::sim::pid()});
            std::fs::write(p, m.to_string()).ok();
        }
    }
    fn host<'b>(prog: &str, args: &'b [String]) -> Option<&'b str> {
        let end = args.iter().position(|a| a == "--").filter(|_| prog == "ssh")?;
        let last = args.get(end.checked_sub(1)?)?;
        Some(last.as_str()).filter(|m| !m.starts_with('-'))
    }
    fn up(&self, m: &str) -> bool {
        if let Some(&u) = lock(&self.up).get(m) {
            return u;
        }
        if !self.probe {
            return true;
        }
        let secs = crate::config::limits().ssh_quick_secs;
        let connect = format!("-oConnectTimeout={secs}");
        let probe = argv(&["-oBatchMode=yes", &connect, "-oControlPath=none", m, "--", "true"]);
        let o = self.r.run_for("ssh", &probe, "", std::time::Duration::from_secs(secs));
        if let Ok(o) = &o {
            self.key_changed(m, o);
        }
        if o.as_ref().is_ok_and(|o| o.code == 124) {
            lock(&self.late).insert(m.into());
        }
        let u = o.is_ok_and(|o| !matches!(o.code, 124 | 255));
        lock(&self.up).insert(m.into(), u);
        u
    }
    fn call(&self, prog: &str, args: &[String], input: Option<&str>) -> Result<Out> {
        let Some(m) = Self::host(prog, args) else {
            return input.map_or_else(|| self.r.run(prog, args), |i| self.r.run_in(prog, args, i));
        };
        let unknown = self.log.is_some_and(|(_, cfg)| !configured(cfg, m));
        if self.off.contains(m) || unknown {
            let why = if unknown {
                "not configured"
            } else {
                "host disabled, no lane there"
            };
            let stderr = format!("{m}: not sent ({why})");
            return Ok(Out {
                code: 255,
                stderr,
                ..Out::default()
            });
        }
        self.note(&quote(prog, args));
        let core = matches!(lock(&self.step).0.as_str(), "check" | "drive");
        let over = !core && self.over();
        if over {
            lock(&self.late).insert(m.into());
        }
        if over || !self.up(m) {
            let why = if over {
                "the tick past its budget"
            } else {
                "host offline this pass"
            };
            self.note("");
            self.unsent.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let stderr = format!("{m}: not sent ({why})");
            return Ok(Out {
                code: 255,
                stderr,
                ..Out::default()
            });
        }
        let limit = std::time::Duration::from_secs(crate::config::limits().ssh_call_secs);
        let input = input.unwrap_or_default();
        let refused =
            |o: &Out| o.code == 255 && (o.stderr.contains("connect to host") || o.stderr.contains(KEY_CHANGED));
        let mut o = self.r.run_for(prog, args, input, limit)?;
        if matches!(o.code, 124 | 255) && !refused(&o) && args.iter().any(|a| a == CM[1]) {
            let g = ok(self.r, "ssh", &["-G", CM[1], m]).unwrap_or_default();
            if let Some(sock) = g.lines().find_map(|l| l.strip_prefix("controlpath ")) {
                std::fs::remove_file(sock).ok();
            }
        }
        if o.code == 255 && !refused(&o) && args.iter().any(|a| a == CM[1]) {
            let bare = args.iter().filter(|a| !CM.contains(&a.as_str())).cloned();
            let bare: Vec<String> = std::iter::once("-oControlPath=none".into()).chain(bare).collect();
            o = self.r.run_for(prog, &bare, input, limit)?;
        }
        self.key_changed(m, &o);
        if matches!(o.code, 124 | 255) {
            lock(&self.up).insert(m.into(), false);
        }
        if o.code == 124 {
            lock(&self.late).insert(m.into());
        }
        self.note("");
        Ok(o)
    }
    fn key_changed(&self, m: &str, o: &Out) {
        if !o.stderr.contains(KEY_CHANGED) {
            return;
        }
        lock(&self.up).insert(m.into(), false);
        let Some((ctx, cfg)) = self.log else {
            return;
        };
        let host = cfg
            .hosts
            .iter()
            .find(|h| h.1.machine.as_deref() == Some(m))
            .map_or(m, |h| h.0.as_str());
        let events = working(&ctx.state).unwrap_or_default();
        let kinds = ["host-key-changed", "host-online"];
        let last = events
            .iter()
            .rev()
            .find(|e| e.str("host") == Some(host) && kinds.contains(&e.kind.as_str()));
        if last.is_some_and(|e| e.kind == kinds[0]) {
            return;
        }
        let why = format!("host-key-changed {host}: ssh refuses its new host key; check it, then `ssh-keygen -R {m}`");
        let d = json!({"host": host, "reason": why});
        emit(&ctx.state, cfg, Event::new(ctx.now, "drover", None, kinds[0], d)).ok();
    }
}

const KEY_CHANGED: &str = "REMOTE HOST IDENTIFICATION HAS CHANGED";

/// The machines of disabled hosts with no open lane logged there: drive, check and status never contact them.
pub fn idle_off(cfg: &Config, events: &[Event]) -> std::collections::BTreeSet<String> {
    let idle = |(n, h): &(&String, &HostCfg)| !h.enabled && lanes_on(cfg, events, n).is_empty();
    cfg.hosts
        .iter()
        .filter(idle)
        .filter_map(|h| h.1.machine.clone())
        .collect()
}

impl Runner for Pass<'_> {
    fn run(&self, prog: &str, args: &[String]) -> Result<Out> {
        self.call(prog, args, None)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> Result<Out> {
        self.call(prog, args, Some(input))
    }
    fn sleep(&self, secs: u64) {
        self.r.sleep(secs);
    }
    fn detach(&self, exe: &str, args: &[String], log: &Path) -> Result<u32> {
        self.r.detach(exe, args, log)
    }
    fn shed(&self) -> Option<crate::shed::Shed> {
        self.r.shed()
    }
}

/// What the last look at a host saw (`<state dir>/hosts.json`): detected tags and `$HOME` (probed hourly), reachability, load/cpu, free memory and disk in GB.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(optional_fields = nullable, rename = "HostSeen"))]
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Seen {
    pub tags: Vec<String>,
    pub home: String,
    pub probed: u64,
    pub seen: u64,
    pub up: bool,
    pub load: Option<f64>,
    pub mem: Option<f64>,
    pub disk: Option<f64>,
    /// A remote host's failed checks from `health` (`✗` keeps lanes off it, `!` warns), with the probe.
    pub health: Vec<String>,
    /// Memory pressure 0 normal, 1 warn, 2 critical; swap-out pages/2s; the top memory users (remote hosts).
    pub pressure: Option<u8>,
    pub swap: Option<f64>,
    pub top: Vec<String>,
    /// Memory and disk size in GB (a bar shows the used part).
    pub mem_total: Option<f64>,
    pub disk_total: Option<f64>,
    /// When it last had a live agent (a cloud host's idle stop).
    pub busy: u64,
    /// A cloud host's gh token, as its card says it.
    pub github: String,
    /// Its set workspace roots.
    pub roots: Vec<crate::roots::Root>,
    /// A remote host's `drover --version` at the last look.
    pub drover: String,
}

fn file(ctx: &Ctx) -> PathBuf {
    ctx.state.with_file_name("hosts.json")
}

pub fn cached(ctx: &Ctx) -> BTreeMap<String, Seen> {
    let text = std::fs::read_to_string(file(ctx)).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}

const PROBE: &str = "uname -s; uname -m; echo \"$HOME\"; xcodebuild -version >/dev/null 2>&1 && echo xcode; \
docker info >/dev/null 2>&1 && echo docker; true";

/// The tags a host detects about itself (`os:macos|os:linux`, `arch:arm64|x86_64`, `xcode`, `docker`) and its `$HOME`.
pub fn probe(r: &dyn Runner) -> Option<(Vec<String>, String)> {
    let out = ok(r, "sh", &["-c", PROBE]).ok()?;
    let mut l = out.lines().map(str::trim);
    let os = l.next()?.replace("Darwin", "macos").to_lowercase();
    let arch = l.next()?.replace("aarch64", "arm64");
    let (home, mut tags) = (l.next()?.to_string(), vec![format!("os:{os}"), format!("arch:{arch}")]);
    tags.extend(l.filter(|t| !t.is_empty()).map(String::from));
    Some((tags, home))
}

/// Look at every host: its live agents (None: unreachable), load, memory and disk, its tags when an hour old (or `refresh`); saved for the board, `hosts` and placement.
pub fn survey(ctx: &Ctx, cfg: &Config, r: &dyn Runner, refresh: bool) -> BTreeMap<String, Option<Vec<Session>>> {
    let (mut all, mut agents, mut rows) = (cached(ctx), BTreeMap::new(), vec![]);
    if cfg.hosts.is_empty() {
        return agents;
    }
    let events = working(&ctx.state).unwrap_or_default();
    let off = idle_off(cfg, &events);
    for (name, h) in &cfg.hosts {
        if h.machine.as_ref().is_some_and(|m| off.contains(m)) {
            continue;
        }
        let on = On::new(r, h.machine.as_deref());
        let s = all.entry(name.clone()).or_default();
        let stopped = h.cloud.is_some() && crate::cloud::state(&events, name) == "stopped";
        if let Some(Err(e)) = (h.cloud.is_some() && !stopped).then(|| crate::cloud::moved(ctx, cfg, r, name, false)) {
            crate::stderr!("cloud {name}: {e:#}");
        }
        let a = (!stopped).then(|| crate::session::live_on(&on).ok()).flatten();
        s.up = a.is_some();
        let live = a.as_ref().is_some_and(|a| !a.is_empty());
        s.busy = if live { ctx.now } else { s.busy };
        if s.up {
            s.seen = ctx.now;
            let due = refresh || s.probed + 3600 <= ctx.now;
            if let Some((tags, home)) = due.then(|| probe(&on)).flatten() {
                (s.tags, s.home, s.probed) = (tags, home, ctx.now);
            }
            let mut landed = false;
            if h.machine.is_some() && h.enabled && !s.home.is_empty() {
                match sync(ctx, cfg, &on, (name, &s.home, &tags(h, Some(&*s)))) {
                    Ok((v, done)) => (s.drover, landed) = (v, done),
                    Err(why) => {
                        let first = !events
                            .iter()
                            .any(|e| e.kind == "host-sync-skipped" && e.str("host") == Some(name));
                        if first || !s.drover.is_empty() {
                            let d = json!({"host": name, "reason": why});
                            append(&ctx.state, &Event::new(ctx.now, "drover", None, "host-sync-skipped", d)).ok();
                        }
                        s.drover = String::new();
                    }
                }
            }
            if let Some(Err(e)) = h
                .machine
                .as_deref()
                .filter(|_| cfg.host_acp(name).is_some())
                .map(|m| crate::proxy::ensure(ctx, cfg, r, (name, m)))
            {
                crate::stderr!("proxy {name}: {e:#}");
            }
            if (due || landed) && h.machine.is_some() && h.enabled {
                s.health = health(ctx, cfg, &on, &tags(h, Some(&*s)), &s.home);
                if h.cloud.is_some() {
                    let (card, warn) = crate::cloud::github(&on, ctx.now);
                    (s.github, ()) = (card, s.health.extend(warn));
                }
            }
            let mut hs = host::sample(&on);
            host::pace(&ctx.state, on.machine, &mut hs, ctx.now);
            (s.load, s.mem, s.mem_total) = (hs.load_per_cpu, hs.free_mem_gb, hs.mem_total_gb);
            ((s.disk, s.disk_total), s.swap) = (host::df(&on).unzip(), hs.swapout);
            s.pressure = hs.pressure.as_deref().map(level);
            s.roots = crate::roots::rows(cfg, &events, &on, name, &s.home);
            if let Some(ag) = a.as_ref().filter(|_| h.machine.is_some()) {
                s.top = host::top(&on, ag);
                rows.push(row(ctx.now, name, s, (lanes_on(cfg, &events, name).len(), None)));
                import(ctx, cfg, &on, name, &s.home);
            }
        }
        agents.insert(name.clone(), a);
    }
    std::fs::write(file(ctx), serde_json::to_string(&all).unwrap_or_default()).ok();
    if let Err(e) = record(ctx, &rows, cfg.hosts.len()) {
        crate::stderr!("host history: {e:#}");
    }
    agents
}

fn import(ctx: &Ctx, cfg: &Config, on: &On, name: &str, home: &str) {
    let exe = rehome(vec![ctx.exe.clone()], Some(home)).remove(0);
    let text = ok(on, &exe, &["events", "--jsonl"]).ok();
    let Some(text) = text.or_else(|| on.read(&Path::new(home).join(".local/state/drover/events.jsonl"))) else {
        return;
    };
    let events = working(&ctx.state).unwrap_or_default();
    let (states, max_age) = (fold(&events), cfg.limits.import_max_age_hours * 3600);
    let mark = |e: &&Event| e.kind == "asks-imported" && e.str("host") == Some(name);
    let upto = events.iter().rev().find(mark).and_then(|e| e.val("upto_ts").as_u64());
    let remote: Vec<Event> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let new = |e: &&Event| e.kind == "needs-decision" && Some(e.ts) > upto;
    let open = open_asks(&remote);
    for e in remote.iter().filter(new) {
        let skip = e.ts + max_age < ctx.now || !open.iter().any(|o| std::ptr::eq(*o, e));
        let done = e.issue.and_then(|i| states.get(&(e.fleet.clone(), i)));
        let from = format!("{name}:{}", e.str("id").unwrap_or_default());
        if skip || done.is_some_and(IssueState::done) || events.iter().any(|m| m.str("from") == Some(&from)) {
            continue;
        }
        let lane = (events.iter().rev()).find(|s| s.kind == "spawn" && s.fleet == e.fleet && s.issue == e.issue);
        let mut d = Value::Object(e.data.clone());
        d.put("from", json!(from));
        if d.at("agent").is_null() {
            let a = lane
                .filter(|s| s.str("host") == Some(name))
                .and_then(|s| s.str("agent"));
            d.put("agent", json!(a));
        }
        crate::db::ask(
            &ctx.state,
            vec![Event::new(e.ts, &e.fleet, e.issue, "needs-decision", d)],
            None,
        )
        .ok();
    }
    if let Some(t) = remote.iter().filter(new).map(|e| e.ts).max() {
        let d = json!({"host": name, "upto_ts": t});
        append(&ctx.state, &Event::new(ctx.now, "drover", None, "asks-imported", d)).ok();
    }
}

/// Detected tags plus the hand ones from fleet.toml.
pub fn tags(h: &HostCfg, s: Option<&Seen>) -> Vec<String> {
    let mut t = s.map(|s| s.tags.clone()).unwrap_or_default();
    t.extend(h.tags.clone());
    t
}

/// The tags a lane of `role` on `fleet#issue` needs: the fleet's, the role's and its queue item's.
pub fn needs(f: &Fleet, role: &str, events: &[Event], (fleet, issue): (&str, u64)) -> Vec<String> {
    let mut n = f.needs.clone();
    n.extend(
        f.role
            .get(f.role_key(role))
            .map(|r| r.needs.clone())
            .unwrap_or_default(),
    );
    n.extend(crate::queue::needs_of(events, fleet, issue));
    n.sort();
    n.dedup();
    n
}

const IDLE: [&str; 6] = ["ready", "needs-human", "parked", "blocked", "merged", "closed"];
const SLOTS: [&str; 2] = ["working", "simulators"];

fn sim(cfg: &Config, events: &[Event], e: &Event) -> bool {
    let (role, n) = (e.str("role").unwrap_or_default(), e.issue.unwrap_or_default());
    let need = |f| needs(f, role, events, (&e.fleet, n));
    cfg.fleet(&e.fleet).is_ok_and(|f| need(f).iter().any(|t| t == "sim"))
}

/// A host's live lanes (`agents`) but `skip`'s: those running a turn, and those holding a simulator.
pub fn held_slots(cfg: &Config, events: &[Event], agents: &[Session], skip: Option<(&str, u64)>) -> [usize; 2] {
    let st = fold(events);
    let mut n = [0; 2];
    for a in agents {
        let sp = crate::events::last_spawn(events, &a.name);
        let Some((sp, i)) = sp.and_then(|e| Some((e, e.issue?))) else {
            continue;
        };
        let s = st.get(&(sp.fleet.clone(), i));
        if s.is_some_and(|s| !IDLE.contains(&s.state.as_str())) && skip != Some((sp.fleet.as_str(), i)) {
            n[0] += usize::from(!matches!(
                a.status().map(Status::word),
                Some("idle" | "done" | "blocked")
            ));
            n[1] += usize::from(sim(cfg, events, sp));
        }
    }
    n
}

/// Drive's check before a start: `waiting for a slot` while every enabled, up host the lane fits is full.
pub fn slot_before(ctx: &Ctx, cfg: &Config, r: &dyn Runner, ev: &[Event], me: Lane, start: &[Event]) -> Option<String> {
    let need = needs(cfg.fleet(me.0).ok()?, me.2, ev, (me.0, me.1));
    let (seen, sims) = (cached(ctx), need.iter().any(|t| t == "sim"));
    let others: Vec<&Event> = start
        .iter()
        .filter(|e| e.fleet != me.0 || e.issue != Some(me.1))
        .collect();
    let host_of = |p: &Event| {
        let at = |e: &&Event| p.issue.is_some_and(|n| e.is("spawn-started", &p.fleet, n)) && e.ts >= p.ts;
        ev.iter().rfind(at).and_then(|e| e.str("host"))
    };
    let count = |on: &dyn Fn(Option<&str>) -> bool| {
        let mine = others.iter().filter(|e| on(host_of(e)));
        [mine.clone().count(), mine.filter(|e| sim(cfg, ev, e)).count()]
    };
    let mut loose = count(&|h| h.is_none_or(|h| !cfg.hosts.contains_key(h)));
    let up = |n: &str| seen.get(n).is_none_or(|s| s.up);
    let fit =
        |(n, h): &(&String, &HostCfg)| h.enabled && up(n) && need.iter().all(|t| tags(h, seen.get(*n)).contains(t));
    let mut full = None;
    for (name, h) in cfg.hosts.iter().filter(fit) {
        let caps = [h.max_working, h.max_simulators.filter(|_| sims)];
        (caps != [None; 2]).then_some(())?;
        let agents = crate::session::live_on(&On::new(r, h.machine.as_deref())).unwrap_or_default();
        let held = held_slots(
            cfg,
            &crate::events::working(&ctx.state).unwrap_or_default(),
            &agents,
            Some((me.0, me.1)),
        );
        let bound = count(&|h| h == Some(name.as_str()));
        let mut used = [held[0] + bound[0], held[1] + bound[1]];
        let room = (caps.into_iter().zip(used))
            .filter_map(|(c, u)| Some(c?.saturating_sub(u)))
            .min();
        let take = room.unwrap_or_default().min(loose[0]);
        let take = [take, take.min(loose[1])];
        for ((u, l), t) in used.iter_mut().zip(loose.iter_mut()).zip(take) {
            *u += t;
            *l -= t;
        }
        let (u, c, s) =
            (caps.into_iter().zip(used).zip(SLOTS)).find_map(|((c, u), s)| c.filter(|&c| u >= c).map(|c| (u, c, s)))?;
        full.get_or_insert(format!("waiting for a slot: {u}/{c} {s} on {name}"));
    }
    full
}

/// Host `name`'s working or simulator slots are all held, counting the spawns `start` still starting there.
pub fn working_full(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    ev: &[Event],
    me: Lane,
    start: &[&Event],
    (name, h): (&str, &HostCfg),
) -> Option<String> {
    let sims = needs(cfg.fleet(me.0).ok()?, me.2, ev, (me.0, me.1))
        .iter()
        .any(|t| t == "sim");
    let others: Vec<&&Event> = start
        .iter()
        .filter(|e| e.fleet != me.0 || e.issue != Some(me.1))
        .collect();
    let start = [others.len(), others.iter().filter(|e| sim(cfg, ev, e)).count()];
    let caps = [h.max_working, h.max_simulators.filter(|_| sims)];
    (caps != [None; 2]).then_some(())?;
    let agents = crate::session::live_on(&On::new(r, h.machine.as_deref())).unwrap_or_default();
    let held = held_slots(
        cfg,
        &crate::events::working(&ctx.state).unwrap_or_default(),
        &agents,
        Some((me.0, me.1)),
    );
    let used = [held[0] + start[0], held[1] + start[1]];
    let (u, c, s) =
        (caps.into_iter().zip(used).zip(SLOTS)).find_map(|((c, u), s)| c.filter(|&c| u >= c).map(|c| (u, c, s)))?;
    Some(format!("waiting for a slot: {u}/{c} {s} on {name}"))
}

/// Each capped host's `name: working N/cap · simulators N/cap` line, from its live `agents` (`host`-tagged remote).
pub fn slots_line(cfg: &Config, events: &[Event], agents: &[Session]) -> String {
    let mut out = String::new();
    for (name, h) in &cfg.hosts {
        let on = |a: &&Session| a.host.as_deref().map_or(h.machine.is_none(), |x| x == name);
        let mine: Vec<Session> = agents.iter().filter(on).cloned().collect();
        let held = held_slots(cfg, events, &mine, None);
        let caps = [h.max_working, h.max_simulators];
        let v: Vec<String> = (SLOTS.into_iter().zip(held).zip(caps))
            .filter_map(|((s, n), c)| Some(format!("{s} {n}/{}", c?)))
            .collect();
        if !v.is_empty() {
            out += &format!("{name}: {}\n", v.join(" · "));
        }
    }
    out
}

/// Add `row` to host `name`'s last-seen health (a `✗` keeps new lanes off it until the next health look).
pub fn sick(ctx: &Ctx, name: &str, row: &str) {
    let mut all = cached(ctx);
    let s = all.entry(name.into()).or_default();
    if !s.health.iter().any(|w| w == row) {
        s.health.push(row.into());
        std::fs::write(file(ctx), serde_json::to_string(&all).unwrap_or_default()).ok();
    }
}

/// Some `[hosts]` entry runs on machine `m`.
pub fn configured(cfg: &Config, m: &str) -> bool {
    cfg.hosts.values().any(|h| h.machine.as_deref() == Some(m))
}

/// `host offline: <host>` for spawn `sp`'s lane when the last look (`survey`) could not reach its host; `host offline (not configured): <machine>` when no `[hosts]` entry runs on its machine.
pub fn offline(ctx: &Ctx, cfg: Option<&Config>, sp: Option<&Event>) -> Option<String> {
    let sp = sp?;
    let m = sp.str("machine")?;
    if cfg.is_some_and(|c| !configured(c, m)) {
        return Some(format!("host offline (not configured): {m}"));
    }
    let h = sp.str("host")?;
    cached(ctx)
        .get(h)
        .is_some_and(|s| !s.up)
        .then(|| format!("host offline: {h}"))
}

/// Spawn `sp`'s lane is on a host that is `off` or failed its last probe: a pass never reaches into it.
pub fn held(ctx: &Ctx, cfg: &Config, sp: &Event) -> bool {
    let off = |h: &str| cfg.hosts.get(h).is_some_and(|c| !c.enabled);
    offline(ctx, Some(cfg), Some(sp)).is_some()
        || sp.str("machine").is_some() && sp.str("host").is_some_and(off)
        || crate::roots::skip(ctx, cfg, sp).is_some()
}

/// When the host on `machine` (None: the main one) last had new lanes let back on (`host-on`), else 0.
pub fn held_since(events: &[Event], machine: Option<&str>) -> u64 {
    let on = |e: &&Event| e.kind == "host-on" && e.str("machine") == machine;
    events.iter().rfind(on).map_or(0, |e| e.ts)
}

/// The alert holding new lanes off the host on `machine`: a lane's usage limit or sign-out since its last `host-on`, or a proxy lane's sign-out once its retry window passed with no proxy call through since.
pub fn login_hold<'a>(events: &'a [Event], machine: Option<&str>, (now, retry): (u64, u64)) -> Option<&'a str> {
    let mine = |e: &Event| e.str("machine") == machine;
    let from = events
        .iter()
        .rposition(|e| e.kind == "host-on" && mine(e))
        .map_or(0, |i| i + 1);
    let retried = |e: &Event| {
        e.val("proxy").as_bool() == Some(true) && (now < e.ts + retry || proxy_ok(events, e.str("agent")) >= Some(e.ts))
    };
    let hold = |e: &&Event| ["usage-limit", "signed-out"].contains(&e.kind.as_str()) && mine(e) && !retried(e);
    events.iter().skip(from).rfind(hold)?.str("reason")
}

/// When the token proxy last relayed a good answer to lane `agent` after it signed out.
pub fn proxy_ok(events: &[Event], agent: Option<&str>) -> Option<u64> {
    events
        .iter()
        .rfind(|e| e.kind == "proxy-ok" && e.str("agent") == agent)
        .map(|e| e.ts)
}

/// The data of the alert that holds new lanes off spawn `sp`'s host because its lane `what` (keyed once per hold); a proxy lane's names the proxy, keyed again after each proxy call through.
pub fn hold_alert(events: &[Event], sp: &Event, what: &str) -> Value {
    let (host, m, lane) = (sp.str("host").unwrap_or("main"), sp.str("machine"), sp.str("agent"));
    let proxied = m.is_some() && sp.str("runtime") == Some("shed-acp");
    let mut key = format!("{host}:{}", held_since(events, m));
    let fix = if proxied {
        key += &format!(":{}:{}", lane.unwrap_or_default(), proxy_ok(events, lane).unwrap_or(0));
        let setup = crate::providers::proxied().map_or("", |p| p.setup);
        format!("it signs in through the token proxy: check the token proxy's `[proxy] token-file` on the main host (`{setup}` makes a new one)")
    } else {
        let login = crate::providers::get(sp.str("harness").unwrap_or_default()).login();
        format!("run `{}` there", login.unwrap_or("its CLI login"))
    };
    let why = format!(
        "{} lane {} {what} on {host}; new lanes there wait: {fix}, then `drover hosts on {host}`",
        sp.str("role").unwrap_or_default(),
        lane.unwrap_or_default()
    );
    json!({"key": key, "agent": lane, "host": host, "machine": m, "reason": why, "proxy": proxied})
}

/// Admission on one host, in order `LOGIN_NEEDED`, `DISK_LOW`, `MEM_LOW`, `HOST_BUSY`: its sample and the first refusal.
pub fn gate(ctx: &Ctx, cfg: &Config, on: &On) -> (Host, Option<(&'static str, String)>) {
    if let Some(why) = login_hold(
        &working(&ctx.state).unwrap_or_default(),
        on.machine,
        (ctx.now, cfg.limits.proxy_retry_secs),
    ) {
        return (Host::default(), Some(("LOGIN_NEEDED", why.to_string())));
    }
    if let Some(why) = disk_low(ctx, cfg, on) {
        return (Host::default(), Some(("DISK_LOW", why)));
    }
    let mut hs = host::sample(on);
    host::pace(&ctx.state, on.machine, &mut hs, ctx.now);
    if on.machine.is_none() {
        host::window(&ctx.state, &mut hs);
    }
    let mem = cfg.limits.min_free_mem_gb;
    if let Some(gb) = hs.free_mem_gb.filter(|g| *g < mem) {
        let why = format!("only {gb:.1} GB of memory free (floor {mem} GB); a lane needs about 0.5–1.5 GB");
        return (hs, Some(("MEM_LOW", format!("{why}; retry when a lane finishes"))));
    }
    let load = hs.load_per_cpu.is_some_and(|x| x > cfg.limits.load_per_cpu);
    let hog = || {
        let dirs = lane_dirs(&crate::events::read(&ctx.state).unwrap_or_default(), on.machine);
        let (lane, share, top) = host::lane_cpu(on, &dirs).into_iter().next()?;
        Some(format!("; {lane} holds {share:.0}% ({top})"))
    };
    let busy = host::admit(&hs, &cfg.limits).err();
    let busy = busy.map(|w| ("HOST_BUSY", w + &load.then(hog).flatten().unwrap_or_default()));
    (hs, busy)
}

/// Each open issue's lanes on `machine`: worktree, build and scratch folder → agent.
pub fn lane_dirs(events: &[Event], machine: Option<&str>) -> BTreeMap<PathBuf, String> {
    let (st, mut dirs) = (fold(events), BTreeMap::new());
    let open = |e: &&Event| {
        e.issue
            .is_none_or(|n| st.get(&(e.fleet.clone(), n)).is_none_or(|s| !s.done()))
    };
    let spawns = events
        .iter()
        .filter(|e| e.kind == "spawn" && e.str("machine") == machine);
    for sp in spawns.filter(open) {
        let (Some(agent), Some(wt)) = (sp.str("agent"), sp.str("worktree").map(PathBuf::from)) else {
            continue;
        };
        for d in build_dirs(sp).into_iter().chain([wt]) {
            dirs.insert(d, agent.to_string());
        }
    }
    dirs
}

/// Lane `sp`'s build and scratch folders under its root's `.build`.
pub fn build_dirs(sp: &Event) -> Vec<PathBuf> {
    let (Some(agent), Some(wt)) = (sp.str("agent"), sp.str("worktree").map(Path::new)) else {
        return vec![];
    };
    let root = sp
        .str("root")
        .map(PathBuf::from)
        .or_else(|| wt.ancestors().nth(2).map(Path::to_path_buf));
    let b = root.map(|r| r.join(crate::roots::BUILD));
    b.iter()
        .flat_map(|b| [b.join(agent), b.join(crate::spawn::SCRATCH).join(agent)])
        .collect()
}

/// The host under `min_free_gb`: its free GB and what `clean --yes` would free there, else `reclaim`.
pub fn disk_low(ctx: &Ctx, cfg: &Config, on: &On) -> Option<String> {
    let (min, events) = (cfg.limits.min_free_gb, working(&ctx.state).unwrap_or_default());
    let low = || host::free_gb(on).filter(|g| *g < min);
    let mut gb = low()?;
    let live = match on.machine {
        None => crate::check::live_lanes(ctx, cfg, on).ok(),
        Some(_) => Some(crate::check::Live::new()),
    };
    let trim = |l| crate::check::trim_on(ctx, on, cfg, 0.0, l, &mut std::io::sink()).unwrap_or(0);
    if live.as_ref().is_some_and(|l| trim(l) > 0) {
        gb = low()?;
    }
    let st = fold(&events);
    let lane = |e: &&Event| matches!(e.kind.as_str(), "spawn" | "spawn-started") && e.str("machine") == on.machine;
    let done = |e: &&Event| {
        e.issue
            .and_then(|n| st.get(&(e.fleet.clone(), n)))
            .is_some_and(IssueState::done)
    };
    let mut wts: Vec<&str> = crate::events::lanes(&events)
        .into_iter()
        .filter(lane)
        .filter(done)
        .filter_map(|e| e.str("worktree"))
        .collect();
    wts.sort_unstable();
    wts.dedup();
    let there = on.existing(&wts);
    wts.retain(|w| there.contains(w));
    let names: Vec<&str> = wts.iter().filter_map(|w| w.rsplit('/').next()).collect();
    let free = match names.len() {
        0 => "nothing for `drover clean --yes` there; `drover reclaim` frees idle build folders".into(),
        n => format!(
            "`drover clean --yes` frees {n} lane worktree(s) there: {}",
            names.join(", ")
        ),
    };
    Some(format!("{gb:.1} GB free < {min}; {free}"))
}

/// Where a lane goes: its host's name and machine, the worktree root and repo checkout there, the remote `$HOME`, and the host's admission refusal (spawn refuses it, or `--force`s a `HOST_BUSY`).
pub struct Placed {
    pub name: String,
    pub machine: Option<String>,
    pub root: PathBuf,
    pub local: PathBuf,
    pub home: Option<String>,
    pub refusal: Option<(&'static str, String)>,
    pub why: Option<String>,
    rank: (bool, bool, f64, f64),
}

type Pre<'a> = (&'a [Event], Option<&'a str>);

/// A host placement passes over, with the refusal it names (its start or working slots are full).
pub type Skip<'a> = &'a dyn Fn(&str) -> Option<(&'static str, String)>;

/// A lane by fleet, issue and role.
pub type Lane<'a> = (&'a str, u64, &'a str);

fn at(ctx: &Ctx, cfg: &Config, f: &Fleet, r: &dyn Runner, (name, machine): (&str, Option<&str>), pre: Pre) -> Placed {
    let on = On::new(r, machine);
    let mut seen = cached(ctx).remove(name).unwrap_or_default();
    let cloud = crate::cloud::held(ctx, cfg, name);
    if machine.is_some() && seen.home.is_empty() && cloud.is_none() {
        seen.home = probe(&on).map(|p| p.1).unwrap_or_default();
    }
    let home = machine.and(Some(seen.home.clone()).filter(|h| !h.is_empty()));
    let ex = |p: &str| under(p, home.as_deref());
    let def = if home.is_some() {
        ex(crate::roots::DEFAULT)
    } else {
        ctx.worktrees.clone()
    };
    let roots = crate::roots::listed(cfg, name, home.as_deref(), &def);
    let root = match &roots[..] {
        [one] => one.clone(),
        _ => crate::roots::pick(cfg, pre.0, &on, name, &roots, pre.1),
    };
    let reach = format!("can't reach {}:", f.repo);
    let sick = (seen.health.iter()).find(|w| w.starts_with('✗') && (!w.starts_with("✗ git ") || w.contains(&reach)));
    let mut full = false;
    let (hs, refusal) = match cloud.map_or_else(|| crate::session::live_on(&on).map_err(|_| None), |c| Err(Some(c))) {
        Err(Some(c)) => (Host::default(), Some(c)),
        Err(None) => (Host::default(), Some(("HOST_OFFLINE", "unreachable".to_string()))),
        Ok(_) if sick.is_some() => (Host::default(), sick.map(|w| ("NO_HOST", format!("unhealthy: {w}")))),
        Ok(agents) => {
            let cap = cfg.hosts.get(name).and_then(|h| h.max_working);
            full = cap.is_some_and(|c| held_slots(cfg, pre.0, &agents, None)[0] >= c);
            gate(ctx, cfg, &on)
        }
    };
    Placed {
        name: name.into(),
        machine: machine.map(String::from),
        root,
        local: ex(&f.local),
        rank: (
            refusal.is_some(),
            full,
            hs.load_per_cpu.unwrap_or(0.0),
            -hs.free_mem_gb.unwrap_or(0.0),
        ),
        refusal: refusal.map(|(c, w)| (c, format!("{name}: {w}"))),
        why: None,
        home,
    }
}

/// `p` with a leading `~/` under the host's `home` (none: this one's).
pub fn under(p: &str, home: Option<&str>) -> PathBuf {
    match (p.strip_prefix("~/"), home) {
        (Some(rest), Some(h)) => Path::new(h).join(rest),
        _ => expand(p),
    }
}

pub fn health(ctx: &Ctx, cfg: &Config, on: &On, tags: &[String], home: &str) -> Vec<String> {
    let at = |h: &(&String, &HostCfg)| h.1.machine.is_some() && h.1.machine.as_deref() == on.machine;
    if let Some(name) = cfg
        .hosts
        .iter()
        .find(at)
        .map(|h| h.0)
        .filter(|n| cfg.host_acp(n).is_some())
    {
        let mut bad = acp_health(ctx, cfg, on, (name, tags, home));
        bad.extend(shed_row(on));
        return bad;
    }
    let mut bad = vec![];
    let mut fail = |pass: bool, row: String| bad.extend((!pass).then_some(row));
    for (name, f) in cfg.fleet.iter().filter(|f| f.1.needs.iter().all(|t| tags.contains(t))) {
        let url = format!("https://github.com/{}.git", f.repo);
        let fix = "add the host's SSH key to GitHub and set `git config --global url.git@github.com:.insteadOf https://github.com/`";
        let o = on.run("git", &argv(&["ls-remote", &url, "HEAD"])).ok();
        let first = o.as_ref().and_then(|o| o.stderr.lines().next()).unwrap_or("no stderr");
        let line = format!("✗ git {name} — can't reach {}: {first} → {fix}", f.repo);
        fail(o.is_some_and(|o| o.code == 0), line);
    }
    let gh = format!("ssh {} gh auth login", on.machine.unwrap_or_default());
    let mut logins = vec![("gh", "gh auth status", gh.as_str())];
    logins.extend(providers::lanes().filter_map(|h| h.health_check()));
    for (tool, c, fix) in logins {
        fail(
            ok(on, "sh", &["-lc", c]).is_ok(),
            format!("✗ {tool} — logged out → {fix}"),
        );
    }
    let exe = real_exe(ctx, home);
    let root = ok(on, &exe, &["--version"]).map_or("missing".into(), |v| v.trim().to_string());
    let (theirs, split) = with_path(on, &exe, root);
    let mine = format!("drover {}", crate::VERSION);
    let (glyph, fix) = if split { ('✗', "relinks") } else { ('!', "updates") };
    let line = format!("{glyph} drover — {theirs} there, {mine} here → the next `check` {fix} it (docs/hosts.md)");
    fail(theirs == mine, line);
    bad.extend(shed_row(on));
    bad
}

/// Thaw a frozen shed service (its user manager, then the unit), then restart it.
pub const SHED_RESTART: &str =
    "{ sudo -n systemctl thaw \"user@$(id -u).service\"; systemctl --user thaw shed.service; } 2>/dev/null; \
systemctl --user restart shed.service";

/// A host's shed row unless it answers `✓`; one there that does not answer is `✗ shed` with the restart.
pub fn shed_row(on: &On) -> Option<String> {
    let line = on.shed().map_or_else(
        || crate::session::live_on(on).err().map(|e| format!("! shed: {e:#}")),
        |b| Some(crate::shed::line(on, &b.wire)),
    );
    let m = on.machine.unwrap_or_default();
    let dead = |l: String| match l.strip_prefix("! shed: ") {
        Some(e) if on.shed().is_some() => format!("✗ shed — does not answer: {e} → ssh {m} '{SHED_RESTART}'"),
        _ => l,
    };
    line.filter(|l| !l.starts_with('✓')).map(dead)
}

fn acp_health(ctx: &Ctx, cfg: &Config, on: &On, (name, tags, home): (&str, &[String], &str)) -> Vec<String> {
    let mut bad = vec![];
    let fits = cfg.fleet.iter().filter(|f| f.1.needs.iter().all(|t| tags.contains(t)));
    for (fleet, f) in fits {
        let url = format!("https://github.com/{}.git", f.repo);
        if !on
            .run("git", &argv(&["ls-remote", &url, "HEAD"]))
            .is_ok_and(|o| o.code == 0)
        {
            bad.push(format!(
                "✗ git {fleet} — can't reach {}: → hosts bootstrap {name}",
                f.repo
            ));
        }
    }
    for (profile, family, id) in acp_profiles(cfg, name, tags) {
        if let Some(why) = acp_guard(ctx, on, home, (&profile, family, id)) {
            bad.push(format!("✗ acp guard {profile} — {why}"));
        }
    }
    if ok(on, "sh", &["-lc", "gh auth status"]).is_err() {
        bad.push(format!(
            "✗ gh — logged out → ssh {} gh auth login",
            on.machine.unwrap_or_default()
        ));
    }
    bad
}

/// Each ACP profile, once, of the roles `place` can put on host `name` (none pinned elsewhere) of the fleets its `tags` fit: (profile, role family, adapter id).
pub fn acp_profiles(cfg: &Config, name: &str, tags: &[String]) -> Vec<(String, &'static str, &'static str)> {
    let mut seen = std::collections::BTreeSet::new();
    let fits = cfg.fleet.values().filter(|f| f.needs.iter().all(|t| tags.contains(t)));
    let roles = fits.flat_map(|f| f.role.iter().map(move |(role, rc)| (f, role, rc)));
    let main = |f: &crate::config::Fleet, role: &str| crate::events::STAGES.contains(&f.family(role)).then_some("main");
    roles
        .filter(|(f, role, rc)| rc.host.as_deref().or_else(|| main(f, role)).is_none_or(|h| h == name))
        .filter_map(|(f, role, rc)| Some((rc.profile_or(role), f.family(role), providers::get(&rc.harness).acp()?)))
        .filter(|p| seen.insert(p.0.clone()))
        .collect()
}

/// Why a remote ACP lane of `profile` would run unguarded on `on` (`$HOME` `home`), proven there: its drover is this one's version, `profiles sync` gave the profile drover's `PreToolUse` hook for `family`, and its shed lists the hooked adapter `id`. None: guarded.
pub fn acp_guard(ctx: &Ctx, on: &On, home: &str, (profile, family, id): (&str, &str, &str)) -> Option<String> {
    let exe = real_exe(ctx, home);
    let root = ok(on, &exe, &["--version"]).map_or("missing".into(), |v| v.trim().to_string());
    let theirs = with_path(on, &exe, root).0;
    let mine = format!("drover {}", crate::VERSION);
    if theirs != mine {
        return Some(format!("drover there is {theirs}, not {mine}"));
    }
    let dir = rehome(
        vec![providers::profiles_root(ctx).join(profile).display().to_string()],
        Some(home),
    )
    .remove(0);
    let s: Value = on
        .read(&Path::new(&dir).join("settings.json"))
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let pre = s.at("hooks").at("PreToolUse").as_array().into_iter().flatten();
    let mut hooks = pre.flat_map(|m| m["hooks"].as_array().into_iter().flatten());
    let hook = format!("drover' hook --role {family}");
    if !hooks.any(|h| {
        h["command"]
            .as_str()
            .is_some_and(|c| c.starts_with('\'') && c.ends_with(&hook))
    }) {
        return Some(format!(
            "profile {profile} there has no drover PreToolUse hook; `drover profiles sync` there"
        ));
    }
    let agents = crate::shed::cli(on, &["agents", "--json"]).ok();
    let agents: Value = agents
        .and_then(|o| serde_json::from_str(&o.stdout).ok())
        .unwrap_or_default();
    if !agents.as_array().into_iter().flatten().any(|a| a["id"] == id) {
        return Some(format!("shed there lists no {id} (`shed agents add {id}` there)"));
    }
    None
}

pub(crate) fn real_exe(ctx: &Ctx, home: &str) -> String {
    let exe = std::fs::canonicalize(&ctx.exe).map_or(ctx.exe.clone(), |p| p.to_string_lossy().into());
    rehome(vec![exe], Some(home)).remove(0)
}

fn with_path(on: &On, exe: &str, root: String) -> (String, bool) {
    match ok(on, "drover", &["--version"]).map(|v| v.trim().to_string()) {
        Ok(p) if p != root => (format!("{root} at {exe}, on ssh's PATH {p}"), true),
        _ => (root, false),
    }
}

/// `~/.local/bin/drover` and a `/usr/local/bin/drover` there links to `exe`, the install root's binary.
pub fn link_line(exe: &str) -> String {
    let (l, u) = ("\"$HOME/.local/bin/drover\"", "/usr/local/bin/drover");
    format!(
        "mkdir -p \"$HOME/.local/bin\" && {{ [ {l} -ef '{exe}' ] || ln -sf '{exe}' {l}; }} && \
{{ [ ! -e {u} ] || [ {u} -ef '{exe}' ] || sudo -n ln -sf '{exe}' {u}; }}"
    )
}

/// `cargo install` of drover at `sha` into `root`, with its `herdsman` link: `hosts bootstrap`'s and `sync`'s.
pub fn install_line(root: &str, sha: &str) -> String {
    let repo = env!("CARGO_PKG_REPOSITORY");
    format!(
        "cargo install --git {repo} --rev {sha} --locked --root '{root}' --force && ln -sf drover '{root}/bin/herdsman'"
    )
}

const SYNC_STATUS: &str = "\"$1\" --version 2>/dev/null || echo missing; d=\"$HOME/.drover-sync\"; \
if [ -f \"$d.done\" ]; then echo \"done $(cat \"$d.done\")\"; rm -f \"$d.done\"; fi; \
[ -f \"$d.pid\" ] && kill -0 \"$(cat \"$d.pid\")\" 2>/dev/null && echo busy; true";
const SYNC_START: &str =
    "V=\"$2\" nohup sh -c \"$1\" </dev/null >\"$HOME/.drover-sync.log\" 2>&1 & echo $! >\"$HOME/.drover-sync.pid\"";

fn sha_of(v: &str) -> &str {
    v.rsplit_once('(').and_then(|p| p.1.strip_suffix(')')).unwrap_or(v)
}

/// Host `name`'s drover `--version` and whether an install landed, its last install logged `host-sync`, a mismatch's install started there; Err: why its status went unread.
pub fn sync(
    ctx: &Ctx,
    cfg: &Config,
    on: &On,
    (name, home, tags): (&str, &str, &[String]),
) -> std::result::Result<(String, bool), String> {
    let (exe, sha) = (real_exe(ctx, home), env!("DROVER_SHA"));
    let o = on
        .run("sh", &argv(&["-c", SYNC_STATUS, "sh", &exe]))
        .map_err(|e| format!("{e:#}"))?;
    if o.code != 0 {
        return Err(format!("status exit {}: {}", o.code, o.stderr.trim()));
    }
    let (mut lines, mut done) = (o.stdout.lines().map(str::trim), false);
    let root = lines.next().unwrap_or_default().to_string();
    let theirs = with_path(on, &exe, root).0;
    let mut busy = false;
    for l in lines {
        busy |= l == "busy";
        if let Some((code, rest)) = l.strip_prefix("done ").and_then(|l| l.split_once(' ')) {
            done = true;
            let (to, from) = rest.split_once(' ').unwrap_or((rest, ""));
            let d = json!({"host": name, "from": sha_of(from), "to": to, "ok": code == "0"});
            append(&ctx.state, &Event::new(ctx.now, "drover", None, "host-sync", d)).ok();
        }
    }
    if busy || theirs == format!("drover {}", crate::VERSION) {
        return Ok((theirs, done));
    }
    let conf = rehome(vec![ctx.config.display().to_string()], Some(home)).remove(0);
    let root = Path::new(&exe).parent().and_then(Path::parent).map(Path::display);
    let install = install_line(&root.map_or(exe.clone(), |r| r.to_string()), sha);
    let link = link_line(&exe);
    let acp = cfg
        .host_acp(name)
        .map(|_| acp_profiles(cfg, name, tags))
        .unwrap_or_default();
    let acp: String = acp
        .iter()
        .map(|(p, family, _)| format!(" && '{exe}' profiles sync --profile '{p}' --role {family}"))
        .collect();
    let job = format!(
        "{}{{ {install} && {link} && {{ test ! -e '{conf}' || '{exe}' profiles sync; }}{acp}; }}; echo \"$? {sha} $V\" >\"$HOME/.drover-sync.done\"",
        crate::cloud::SH
    );
    if let Err(e) = ok(on, "sh", &["-c", SYNC_START, "sh", &job, &theirs]) {
        let d = json!({"host": name, "from": sha_of(&theirs), "to": sha, "ok": false, "error": format!("{e:#}")});
        append(&ctx.state, &Event::new(ctx.now, "drover", None, "host-sync", d)).ok();
    }
    Ok((theirs, done))
}

/// The host a new lane runs on (None without `[hosts]`): `--host`, else the host of the issue's last lane while it is enabled, up, admitting and its tags cover the needs, or its worktree there has tracked changes or unpushed commits, else the least-busy enabled host whose tags cover the needs; never one a `stall-move` since its last spawn left.
pub fn place(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    events: &[Event],
    (fleet, issue, role): (&str, u64, &str),
    (force, skip): (Option<&str>, Skip),
) -> Result<Option<Placed>> {
    if cfg.hosts.is_empty() {
        return match force {
            Some(h) => refused("BAD_VALUE", format!("--host {h}: fleet.toml has no [hosts]")),
            None => Ok(None),
        };
    }
    let f = cfg.fleet(fleet)?;
    let last = crate::events::lanes(events)
        .into_iter()
        .rev()
        .find(|e| e.is("spawn", fleet, issue));
    let pre = (events, last.and_then(|e| e.str("root")));
    let design = crate::events::STAGES.contains(&f.family(role));
    let pin = f.role.get(f.role_key(role)).and_then(|r| r.host.as_deref());
    if let Some(h) = force.or(pin).or(design.then_some("main")) {
        let main = cfg
            .hosts
            .iter()
            .find(|h| h.1.machine.is_none())
            .map_or("local", |h| h.0.as_str());
        let h = if h == "main" { main } else { h };
        let machine = match cfg.hosts.get(h) {
            Some(c) => c.machine.as_deref(),
            None if h == main => None,
            None => return refused("BAD_VALUE", format!("--host {h}: no [hosts.{h}]")),
        };
        if design && machine.is_some() {
            return refused(
                "DESIGN_MAIN_ONLY",
                format!("host {h}: {role} lanes run on the main host only"),
            );
        }
        if let Some((code, why)) = skip(h) {
            return refused(code, why);
        }
        return Ok(Some(at(ctx, cfg, f, r, (h, machine), pre)));
    }
    let needs = needs(f, role, events, (fleet, issue));
    let seen = cached(ctx);
    let fit = |n: &str, h: &HostCfg| needs.iter().all(|t| tags(h, seen.get(n)).contains(t));
    let left = left(events, fleet, issue);
    if let Some(e) = last.filter(|e| left != Some(e.str("host").unwrap_or("local"))) {
        let was = (e.str("host").unwrap_or("local"), e.str("machine"));
        let mut p = at(ctx, cfg, f, r, was, pre);
        let code = p.refusal.as_ref().map(|x| x.0);
        let busy = matches!(code, Some("DISK_LOW" | "HOST_BUSY" | "LOGIN_NEEDED" | "MEM_LOW"));
        let fits = cfg.hosts.get(was.0).map_or(needs.is_empty(), |h| fit(was.0, h));
        let wt = last_spawn(events, &format!("{fleet}-{role}-{issue}")).filter(|l| l.str("machine") == was.1);
        let wt = wt.and_then(|l| l.str("worktree")).map(Path::new);
        let unpushed = (busy || !fits) && wt.is_some_and(|w| unsaved(&On::new(r, was.1), w));
        if unpushed {
            let note = format!("stays on {}: its worktree there has unpushed work", was.0);
            match p.refusal.as_mut() {
                Some(x) => x.1 += &format!("; {note}"),
                None => p.why = Some(note),
            }
        }
        if cfg.hosts.get(was.0).is_none_or(|h| h.enabled) && (unpushed || fits && !busy && code != Some("HOST_OFFLINE"))
        {
            return match skip(was.0) {
                Some((code, why)) => refused(code, why),
                None => Ok(Some(p)),
            };
        }
    }
    let fits: Vec<_> = (cfg.hosts.iter())
        .filter(|(n, h)| fit(n, h) && left != Some(n.as_str()))
        .collect();
    let (mut full, mut open) = (vec![], vec![]);
    for h in fits.iter().filter(|h| h.1.enabled) {
        match skip(h.0) {
            Some(why) => full.push(why),
            None => open.push(h),
        }
    }
    if open.is_empty() && !full.is_empty() {
        let starting = full.iter().any(|w| w.0 == "SPAWN_PENDING");
        let code = if starting { "SPAWN_PENDING" } else { "HOST_FULL" };
        let whys: Vec<String> = full.into_iter().map(|w| w.1).collect();
        return refused(code, whys.join(", "));
    }
    let mut on: Vec<Placed> = (open.into_iter())
        .map(|(n, h)| at(ctx, cfg, f, r, (n, h.machine.as_deref()), pre))
        .collect();
    if on
        .iter()
        .all(|p| p.refusal.as_ref().is_some_and(|r| r.0 == "CLOUD_BUDGET"))
    {
        let has = if needs.is_empty() {
            "enabled host".into()
        } else {
            format!("host with {}", needs.join(", "))
        };
        let cloud = (cfg.hosts.values().any(|h| h.cloud.is_some())).then_some(" (cloud host stopped or over budget)");
        let cloud = cloud.unwrap_or_default();
        return refused("NO_HOST", format!("no {has}{cloud}"));
    }
    on.sort_by(|a, b| a.rank.partial_cmp(&b.rank).unwrap_or(Equal));
    let local = |p: &Placed| cfg.hosts.get(&p.name).is_some_and(|h| h.cloud.is_none());
    let held = held_for(ctx, on.iter().filter(|p| local(p)).all(|p| p.refusal.is_some()));
    let wake = held >= cfg.limits.overflow_after_minutes * 60 || !on.iter().any(local);
    let free = |p: &Placed| p.refusal.is_none() && !p.rank.1;
    let up = (!on.iter().any(|p| local(p) && free(p)))
        .then(|| on.iter().position(free))
        .flatten();
    let i = up.or_else(|| {
        (!wake && !on.first().is_some_and(local))
            .then(|| on.iter().position(local))
            .flatten()
    });
    let mut p = on.remove(i.unwrap_or(0));
    let busy = on.iter().filter(|p| local(p)).find_map(|p| p.refusal.as_ref());
    let busy = busy.map_or_else(|| "every local host at max_working".into(), |(c, w)| format!("{c} {w}"));
    let why = match (on.iter().any(local), up) {
        (true, Some(_)) => format!("{busy}; {} is up with room", p.name),
        (true, None) => format!("overflow: every local host busy {} min", held / 60),
        (false, ..) if needs.is_empty() => "no local host fits".into(),
        (false, ..) => format!("needs {}: no local host has it", needs.join(", ")),
    };
    p.why = (!local(&p)).then_some(why);
    Ok(Some(p))
}

/// Does worktree `wt` on its host hold changes to tracked files or unpushed commits (unreadable: no)? Untracked files never pin a lane.
pub fn unsaved(on: &On, wt: &Path) -> bool {
    let w = wt.to_string_lossy();
    let git = |a: &[&str]| ok(on, "git", &[&["-C", &*w][..], a].concat());
    let tracked = |st: String| st.lines().any(|l| !l.starts_with("??"));
    let ahead = |n: String| n.trim().parse::<u64>().is_ok_and(|n| n > 0);
    on.exists(wt)
        && (git(&["status", "--porcelain"]).is_ok_and(tracked)
            || git(&["rev-list", "--count", "HEAD", "--not", "--remotes"]).is_ok_and(ahead))
}

fn left<'a>(events: &'a [Event], fleet: &str, issue: u64) -> Option<&'a str> {
    let mine = crate::events::of_issue(events, fleet, issue);
    let since = mine.into_iter().rev().take_while(|e| e.kind != "spawn");
    since.into_iter().find(|e| e.kind == "stall-move")?.str("from")
}

fn held_for(ctx: &Ctx, held: bool) -> u64 {
    let p = ctx.state.with_file_name("held-since");
    let s = std::fs::read_to_string(&p).unwrap_or_default();
    let mut n = s.split_whitespace().map(|w| w.parse::<u64>().ok());
    let since = match (n.next().flatten(), n.next().flatten()) {
        (Some(a), Some(b)) if ctx.now.saturating_sub(b) <= 900 => a,
        _ => ctx.now,
    };
    if held {
        std::fs::write(&p, format!("{since} {}", ctx.now))
    } else {
        std::fs::remove_file(&p)
    }
    .ok();
    ctx.now.saturating_sub(since) * u64::from(held)
}

/// Start the stopped cloud host `place` picked (not on a dry run), then place on it afresh.
pub fn wake(ctx: &Ctx, cfg: &Config, r: &dyn Runner, f: (&Fleet, bool), p: Option<Placed>) -> Result<Option<Placed>> {
    match p {
        Some(p) if !f.1 && p.refusal.as_ref().is_some_and(|x| x.0 == "CLOUD_STOPPED") => {
            crate::cloud::start(ctx, cfg, r, &p.name)?;
            let why = p.why.clone();
            Ok(Some(Placed {
                why,
                ..at(ctx, cfg, f.0, r, (&p.name, p.machine.as_deref()), (&[], None))
            }))
        }
        p => Ok(p),
    }
}

/// `a` with this host's `$HOME` moved to the lane host's `home` (a remote lane's profile, prompt and env paths).
pub fn rehome(a: Vec<String>, home: Option<&str>) -> Vec<String> {
    let mine = crate::config::home().to_string_lossy().into_owned();
    match home.filter(|_| mine.len() > 1) {
        Some(h) => a.into_iter().map(|w| w.replace(&mine, h)).collect(),
        None => a,
    }
}

/// The agents of the open lanes on host `name` (a spawn without `host` ran on the machine-less one, or the only one).
pub fn lanes_on(cfg: &Config, events: &[Event], name: &str) -> Vec<String> {
    let here = cfg.hosts.get(name).is_none_or(|h| h.machine.is_none());
    let on = |l: &&str| last_spawn(events, l).is_some_and(|e| e.str("host").map_or(here, |h| h == name));
    crate::events::with_states(events, |st| {
        let open = st.values().filter(|s| !s.done()).filter_map(|s| s.active_lane());
        open.map(|l| l.1).filter(on).map(String::from).collect()
    })
}

/// A remote host unreachable 15 min straight: its open lanes restart on the main host from their pushed branch.
pub fn evacuate(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write) -> Result<()> {
    let seen = cached(ctx);
    for (name, h) in cfg.hosts.iter().filter(|h| h.1.machine.is_some()) {
        let events = working(&ctx.state)?;
        let stopped = h.cloud.is_some() && crate::cloud::state(&events, name) == "stopped";
        if stopped
            || !seen
                .get(name)
                .is_some_and(|s| !s.up && s.seen + cfg.limits.host_offline_minutes * 60 <= ctx.now)
        {
            continue;
        }
        let mut moved = vec![];
        for s in fold(&events).values().filter(|s| !s.done()) {
            let (Some((role, agent)), Ok(f)) = (s.active_lane(), cfg.fleet(&s.fleet)) else {
                continue;
            };
            if last_spawn(&events, agent).is_none_or(|e| e.str("host") != Some(name)) {
                continue;
            }
            let o = crate::spawn::Opts {
                supersede: true,
                host: Some("main".into()),
                ..Default::default()
            };
            if let Err(e) = crate::spawn::spawn(ctx, cfg, r, out, (&s.fleet, s.issue, role), &o) {
                writeln!(out, "{agent}: restart off {name} failed: {e:#}")?;
                continue;
            }
            let now = working(&ctx.state)?;
            let Some(to) = last_spawn(&now, agent).filter(|e| e.str("host") != Some(name)) else {
                continue;
            };
            let local = under(&f.local, None).display().to_string();
            let branch = format!("origin/{}", to.str("branch").unwrap_or_default());
            let sha = ok(r, "git", &["-C", &local, "rev-parse", "--verify", "--quiet", &branch]).ok();
            let d =
                json!({"agent": agent, "host": name, "to": to.str("host"), "sha": sha.map(|s| s.trim().to_string())});
            emit(
                &ctx.state,
                cfg,
                Event::new(ctx.now, &s.fleet, Some(s.issue), "lane-moved", d),
            )?;
            moved.push(agent.to_string());
        }
        if !moved.is_empty() {
            let why = format!(
                "{name} offline 15 min: {} restarted on the main host from the pushed branch; unpushed work stayed on {name}",
                moved.join(", ")
            );
            writeln!(out, "{why}")?;
            let d = json!({"host": name, "lanes": moved, "reason": why});
            emit(&ctx.state, cfg, Event::new(ctx.now, "drover", None, "lanes-moved", d))?;
        }
    }
    Ok(())
}

/// A host as the board and `hosts` say it: (tone, words). Offline and off are grey: neither pages.
pub fn shown(enabled: bool, s: &Seen, lanes: usize, now: u64) -> (&'static str, String) {
    if !s.up {
        let last = (s.seen != 0).then(|| age(now.saturating_sub(s.seen)));
        let last = last.unwrap_or_else(|| "never".into());
        return ("m", format!("offline · last seen {last}"));
    }
    let sick = |c: char| s.health.iter().find_map(|w| w.strip_prefix(c)).map(str::trim);
    match (enabled, lanes) {
        (false, 0) => ("m", "off · not used for lanes".into()),
        (false, n) => ("m", format!("off · draining {n} lane{}", if n == 1 { "" } else { "s" })),
        _ => match (sick('✗'), sick('!')) {
            (Some(w), _) => ("r", format!("unhealthy · {w}")),
            (_, Some(w)) => ("a", format!("up · {w}")),
            _ => ("g", "up".into()),
        },
    }
}

/// `drover hosts [refresh]`: a fresh look at each host; `refresh` probes the tags again.
pub fn list(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, refresh: bool) -> Result<()> {
    if cfg.hosts.is_empty() {
        return Ok(writeln!(out, "no [hosts] in fleet.toml: every lane runs on this host")?);
    }
    if refresh || cfg.hosts.keys().any(|h| !cached(ctx).contains_key(h)) {
        survey(ctx, cfg, r, refresh);
    }
    let (seen, events) = (cached(ctx), working(&ctx.state)?);
    let n = |x: Option<f64>, p: usize| x.map_or("?".into(), |x| format!("{x:.p$}"));
    for (name, h) in &cfg.hosts {
        let s = seen.get(name).cloned().unwrap_or_default();
        let lanes = lanes_on(cfg, &events, name);
        let (words, t) = (
            shown(h.enabled, &s, lanes.len(), ctx.now).1,
            tags(h, Some(&s)).join(","),
        );
        let (l, m, d, k, ls) = (n(s.load, 2), n(s.mem, 1), n(s.disk, 0), lanes.len(), lanes.join(" "));
        let v = match sha_of(&s.drover) {
            "" => String::new(),
            x if x == env!("DROVER_SHA") => format!(" · drover {x} ✓"),
            x => format!(" · drover {x} ≠ {}", env!("DROVER_SHA")),
        };
        writeln!(
            out,
            "{name} · {words} · tags {t}{v} · load/cpu {l} · mem {m} GB · disk {d} GB free · {k} lane(s) {ls} · seen {} ago",
            age(ctx.now.saturating_sub(s.seen))
        )?;
    }
    Ok(())
}

/// `drover hosts on|off <name>` (and the board's button): `[hosts.<name>] enabled`, logged `host-on`/`host-off`.
pub fn toggle(ctx: &Ctx, out: &mut dyn Write, name: &str, on: bool, by: &str) -> Result<()> {
    let text = std::fs::read_to_string(&ctx.config)?;
    let cfg = Config::parse(&text)?;
    let events = working(&ctx.state)?;
    let machine = match cfg.hosts.get(name) {
        None if on && name == "main" && login_hold(&events, None, (ctx.now, cfg.limits.proxy_retry_secs)).is_some() => {
            None
        }
        None => bail!("no [hosts.{name}] in fleet.toml"),
        Some(h) if !on && h.machine.is_none() => return refused("MAIN_HOST", "main host: edit fleet.toml by hand"),
        Some(h) => {
            let new = set_in(&text, &format!("[hosts.{name}]"), "enabled", &on.to_string())?;
            write_atomic(&ctx.config, &new)?;
            h.machine.as_deref()
        }
    };
    let word = if on { "on" } else { "off" };
    let (kind, d) = (
        format!("host-{word}"),
        json!({"host": name, "machine": machine, "by": by}),
    );
    emit(&ctx.state, &cfg, Event::new(ctx.now, "drover", None, &kind, d))?;
    Ok(writeln!(out, "{name}: {word} for new lanes")?)
}

/// Memory pressure as the history keeps it: 0 normal, 1 warn, 2 critical.
pub fn level(p: &str) -> u8 {
    match p {
        "warn" => 1,
        "critical" => 2,
        _ => 0,
    }
}

/// The Host tab's cards in order: the main host (no `machine`; `hostname -s` without `[hosts]`), then A→Z.
pub fn cards<'a>(cfg: &'a Config, r: &dyn Runner) -> Vec<(String, Option<&'a HostCfg>)> {
    if cfg.hosts.is_empty() {
        let name = ok(r, "hostname", &["-s"]).map(|s| s.trim().to_string()).ok();
        return vec![(
            name.filter(|n| !n.is_empty()).unwrap_or_else(|| "this host".into()),
            None,
        )];
    }
    let mut v: Vec<_> = cfg.hosts.iter().map(|(n, h)| (n.clone(), Some(h))).collect();
    v.sort_by_key(|(_, h)| h.is_some_and(|h| h.machine.is_some()));
    v
}

/// One row a minute per host in `<state dir>/host-history.jsonl`, written by the fast step; 24 h kept.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(optional_fields = nullable, rename = "HostRow"))]
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Row {
    pub ts: u64,
    pub host: String,
    pub load_cpu: Option<f64>,
    pub pressure: Option<u8>,
    pub swap_out: Option<f64>,
    pub mem_free_gb: Option<f64>,
    pub disk_free_gb: Option<f64>,
    pub lanes: usize,
    /// MB per `host::groups` group, only while `/live?tab=host` was seen in the last 2 min (else no `ps` walk).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub procs: Option<[u64; 5]>,
}

impl Row {
    /// Key `k` (load, swap, mem, disk, pressure, lanes): its value, and true when low is bad (free space).
    pub fn get(&self, k: &str) -> (Option<f64>, bool) {
        match k {
            "load" => (self.load_cpu, false),
            "swap" => (self.swap_out, false),
            "mem" => (self.mem_free_gb, true),
            "disk" => (self.disk_free_gb, true),
            "pressure" => (self.pressure.map(f64::from), false),
            _ => (Some(f64::from(u32::try_from(self.lanes).unwrap_or(u32::MAX))), false),
        }
    }
}

const DAY: u64 = 86_400;
/// The history's size bound per host: 1,600 rows (also its row cap) of up to 200 B, so 25 h fit.
pub const BOUND: usize = 320_000;

fn history_file(ctx: &Ctx) -> PathBuf {
    ctx.state.with_file_name("host-history.jsonl")
}

pub fn history(ctx: &Ctx) -> Vec<Row> {
    let text = std::fs::read_to_string(history_file(ctx)).unwrap_or_default();
    text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// Append `rows`; once the oldest row is over 25 h old or the file over `BOUND` for each of `hosts`, it is rewritten to the last 24 h and 1,600 rows each, newest first within 9/10 of the bound: once an hour at most.
pub fn record(ctx: &Ctx, rows: &[Row], hosts: usize) -> Result<()> {
    let (p, bound) = (history_file(ctx), BOUND * hosts.max(1));
    let new: String = rows
        .iter()
        .map(|r| serde_json::to_string(r).unwrap_or_default() + "\n")
        .collect();
    let ts = |l: &str| serde_json::from_str::<Row>(l).map_or(0, |r| r.ts);
    let first = std::fs::File::open(&p)
        .ok()
        .and_then(|f| std::io::BufRead::lines(std::io::BufReader::new(f)).next())
        .and_then(Result::ok);
    let len = std::fs::metadata(&p).map_or(0, |m| usize::try_from(m.len()).unwrap_or(usize::MAX)) + new.len();
    let fresh = first.is_none_or(|l| ts(&l) + DAY + 3600 >= ctx.now);
    if fresh && len <= bound {
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&p)?;
        return Ok(f.write_all(new.as_bytes())?);
    }
    let (text, mut size) = (std::fs::read_to_string(&p)? + &new, 0);
    let keep: Vec<&str> = (text.lines().rev().filter(|l| ts(l) + DAY >= ctx.now))
        .take(1_600 * hosts.max(1))
        .take_while(|l| {
            size += l.len() + 1;
            size <= bound / 10 * 9
        })
        .collect();
    write_atomic(&p, &keep.iter().rev().map(|l| format!("{l}\n")).collect::<String>())
}

fn row(ts: u64, host: &str, s: &Seen, (lanes, procs): (usize, Option<[u64; 5]>)) -> Row {
    let q = |x: Option<f64>, m: f64| x.map(|x| (x * m).round() / m);
    Row {
        ts,
        host: host.into(),
        load_cpu: q(s.load, 100.0),
        pressure: s.pressure,
        swap_out: q(s.swap, 1.0),
        mem_free_gb: q(s.mem, 10.0),
        disk_free_gb: q(s.disk, 10.0),
        lanes,
        procs,
    }
}

/// The fast step: this host's history row, a minute apart (remote hosts' rows come from `survey`); `procs`.
pub fn sample(ctx: &Ctx, cfg: &Config, r: &dyn Runner, events: &[Event]) -> Result<()> {
    let mut hs = host::sample(r);
    host::pace(&ctx.state, None, &mut hs, ctx.now);
    let s = Seen {
        load: hs.load_per_cpu,
        mem: hs.free_mem_gb,
        disk: host::free_gb(r),
        pressure: hs.pressure.as_deref().map(level),
        swap: hs.swapout,
        ..Seen::default()
    };
    let cards = cards(cfg, r);
    let main = cards.iter().find(|c| c.1.is_none_or(|h| h.machine.is_none()));
    let watch = std::fs::read_to_string(ctx.state.with_file_name("host-watch.at")).unwrap_or_default();
    let procs = (watch.trim().parse::<u64>().is_ok_and(|t| t + 120 >= ctx.now)).then(|| host::groups(r));
    let rows: Vec<Row> = main
        .map(|(n, _)| row(ctx.now, n, &s, (lanes_on(cfg, events, n).len(), procs)))
        .into_iter()
        .collect();
    record(ctx, &rows, cards.len())
}

/// Host `h`'s key `k` over the `secs` up to `now` in `n` buckets, each its worst row (the max; for free space the min) and that row's time, so a peak is never averaged away; None where no row fell.
pub fn spark(rows: &[Row], h: &str, k: &str, (now, secs, n): (u64, u64, usize)) -> Vec<Option<(f64, u64)>> {
    let (mut out, from) = (vec![None; n], now.saturating_sub(secs));
    for r in rows.iter().filter(|r| r.host == h && r.ts > from && r.ts <= now) {
        let (Some(v), low) = r.get(k) else { continue };
        let i = usize::try_from((r.ts - from - 1) * u64::try_from(n).unwrap_or(u64::MAX) / secs.max(1)).unwrap_or(0);
        let Some(o) = out.get_mut(i.min(n - 1)) else { continue };
        if o.is_none_or(|(w, _)| if low { v < w } else { v > w }) {
            *o = Some((v, r.ts));
        }
    }
    out
}

fn ranges(rows: &[Row], (h, k, now, all): (&str, &str, u64, bool), f: &dyn Fn((f64, u64)) -> String) -> String {
    let one = |&(secs, n): &(u64, usize)| {
        let v = spark(rows, h, k, (now, secs, n));
        v.into_iter()
            .map(|v| v.map_or(String::new(), f))
            .collect::<Vec<_>>()
            .join(",")
    };
    let rs = [(DAY, 120), (21_600, 120), (3_600, 60)];
    rs.iter()
        .take(if all { 3 } else { 1 })
        .map(one)
        .collect::<Vec<_>>()
        .join(";")
}

/// `ranges` of the values, as few decimals as the row has.
pub fn series(rows: &[Row], h: &str, k: &str, now: u64, all: bool) -> String {
    let p = match k {
        "load" => 2,
        "mem" | "disk" => 1,
        _ => 0,
    };
    ranges(rows, (h, k, now, all), &|(v, _)| format!("{v:.p$}"))
}

/// An open chart's series, `|` apart: key `k`, the lanes and the memory pressure with every range, then the minutes ago of each of `k`'s worst rows (the read-out's time).
pub fn chart(rows: &[Row], h: &str, k: &str, now: u64) -> String {
    let c = [k, "lanes", "pressure"]
        .map(|x| series(rows, h, x, now, true))
        .join("|");
    c + "|"
        + &ranges(rows, (h, k, now, true), &|(_, ts)| {
            (now.saturating_sub(ts) / 60).to_string()
        })
}
