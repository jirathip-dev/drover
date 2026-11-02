//! #157 multi-host lanes (docs/hosts.md): `[hosts.<name>]`, their detected tags, where a new lane goes, and a lane's host
//! as a Runner: herdr gets `--machine`, every other tool runs over `ssh`. No `[hosts]` = this host only, as before.
use crate::config::{expand, set_in, write_atomic, Config, Fleet, HostCfg};
use crate::events::{age, emit, fold, last_spawn, read, Event};
use crate::gate::refused;
use crate::host::{self, Host};
use crate::runner::{argv, ok, Out, Runner};
use crate::Ctx;
use anyhow::{bail, ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::cmp::Ordering::Equal;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// A lane's host; `machine` None is this one (every call and file exactly as before).
pub struct On<'a> {
    pub r: &'a dyn Runner,
    pub machine: Option<&'a str>,
}

impl<'a> On<'a> {
    pub fn new(r: &'a dyn Runner, machine: Option<&'a str>) -> Self {
        On { r, machine }
    }
    /// The host a `spawn` event's lane runs on (none: this one).
    pub fn of(r: &'a dyn Runner, e: Option<&'a Event>) -> Self {
        Self::new(r, e.and_then(|e| e.str("machine")))
    }
    fn wrap(&self, prog: &str, args: &[String]) -> (String, Vec<String>) {
        let Some(m) = self.machine else {
            return (prog.into(), args.to_vec());
        };
        if prog == crate::herdr::BIN {
            return (prog.into(), [argv(&["--machine", m]), args.to_vec()].concat());
        }
        let mut line = format!("'{prog}'");
        for a in args {
            line += &format!(" '{}'", a.replace('\'', "'\\''"));
        }
        let ssh = argv(&["-oBatchMode=yes", "-oConnectTimeout=10", m, "--", &line]);
        ("ssh".into(), ssh)
    }
    fn sh(&self, script: &str, p: &Path, input: &str) -> Result<Out> {
        self.run_in("sh", &argv(&["-c", script, "sh", &p.to_string_lossy()]), input)
    }
    pub fn exists(&self, p: &Path) -> bool {
        match self.machine {
            None => p.exists(),
            Some(_) => self.sh("test -e \"$1\"", p, "").is_ok_and(|o| o.code == 0),
        }
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
        self.r.run(&p, &a)
    }
    fn run_in(&self, prog: &str, args: &[String], input: &str) -> Result<Out> {
        let (p, a) = self.wrap(prog, args);
        self.r.run_in(&p, &a, input)
    }
    fn sleep(&self, secs: u64) {
        self.r.sleep(secs);
    }
}

/// What the last look at a host saw (`<state dir>/hosts.json`): detected tags and `$HOME` (probed hourly), reachability,
/// load/cpu, free memory and disk in GB.
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

/// Look at every host: its live agents (None: unreachable), load, memory and disk, its tags when an hour old (or
/// `refresh`); saved for the board, `hosts` and placement.
pub fn survey(ctx: &Ctx, cfg: &Config, r: &dyn Runner, refresh: bool) -> BTreeMap<String, Option<Vec<Value>>> {
    let (mut all, mut agents) = (cached(ctx), BTreeMap::new());
    if cfg.hosts.is_empty() {
        return agents;
    }
    for (name, h) in &cfg.hosts {
        let on = On::new(r, h.machine.as_deref());
        let s = all.entry(name.clone()).or_default();
        let a = host::agents(&on).ok();
        s.up = a.is_some();
        if s.up {
            s.seen = ctx.now;
            if refresh || s.probed + 3600 <= ctx.now {
                if let Some((tags, home)) = probe(&on) {
                    (s.tags, s.home, s.probed) = (tags, home, ctx.now);
                }
            }
            let hs = host::sample(&on);
            (s.load, s.mem, s.disk) = (hs.load_per_cpu, hs.free_mem_gb, host::free_gb(&on));
        }
        agents.insert(name.clone(), a);
    }
    let _ = std::fs::write(file(ctx), serde_json::to_string(&all).unwrap_or_default());
    agents
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
    n.extend(f.role.get(role).map(|r| r.needs.clone()).unwrap_or_default());
    n.extend(crate::queue::needs_of(events, fleet, issue));
    n.sort();
    n.dedup();
    n
}

/// Every remote host's live agents, each tagged `host`; an unreachable host adds none.
pub fn remote_agents(cfg: &Config, r: &dyn Runner) -> Vec<Value> {
    let mut all = vec![];
    for (name, m) in cfg.hosts.iter().filter_map(|(n, h)| Some((n, h.machine.as_deref()?))) {
        for mut a in host::agents(&On::new(r, Some(m))).unwrap_or_default() {
            (a["host"], a["machine"]) = (json!(name), json!(m));
            all.push(a);
        }
    }
    all
}

/// This host's live agents, then the remote ones.
pub fn all_agents(cfg: &Config, r: &dyn Runner) -> Result<Vec<Value>> {
    Ok([host::agents(r)?, remote_agents(cfg, r)].concat())
}

/// Admission on one host, in order `DISK_LOW`, `MEM_LOW`, `HOST_BUSY`: its sample and the first refusal.
pub fn gate(ctx: &Ctx, cfg: &Config, on: &On) -> (Host, Option<(&'static str, String)>) {
    let min = cfg.limits.min_free_gb;
    if let Some(gb) = host::free_gb(on).filter(|g| *g < min) {
        let why = format!("{gb:.1} GB free < {min}; `herdsman clean --yes`");
        return (Host::default(), Some(("DISK_LOW", why)));
    }
    let mut hs = host::sample(on);
    if on.machine.is_none() {
        host::window(&ctx.state, &mut hs);
    }
    let mem = cfg.limits.min_free_mem_gb;
    if let Some(gb) = hs.free_mem_gb.filter(|g| *g < mem) {
        let why = format!("only {gb:.1} GB of memory free (floor {mem} GB); a lane needs about 0.5–1.5 GB");
        return (hs, Some(("MEM_LOW", format!("{why}; retry when a lane finishes"))));
    }
    let busy = host::admit(&hs, &cfg.limits).err().map(|w| ("HOST_BUSY", w));
    (hs, busy)
}

/// Where a lane goes: its host's name and machine, the worktree root and repo checkout there, the remote `$HOME`, and the
/// host's admission refusal (spawn refuses it, or `--force`s a `HOST_BUSY`).
pub struct Placed {
    pub name: String,
    pub machine: Option<String>,
    pub root: PathBuf,
    pub local: PathBuf,
    pub home: Option<String>,
    pub refusal: Option<(&'static str, String)>,
    rank: (bool, f64, f64),
}

fn at(ctx: &Ctx, cfg: &Config, f: &Fleet, r: &dyn Runner, (name, machine): (&str, Option<&str>)) -> Placed {
    let on = On::new(r, machine);
    let mut seen = cached(ctx).remove(name).unwrap_or_default();
    if machine.is_some() && seen.home.is_empty() {
        seen.home = probe(&on).map(|p| p.1).unwrap_or_default();
    }
    let home = machine.and(Some(seen.home.clone()).filter(|h| !h.is_empty()));
    let ex = |p: &str| match (p.strip_prefix("~/"), &home) {
        (Some(rest), Some(h)) => Path::new(h).join(rest),
        _ => expand(p),
    };
    let def = if home.is_some() {
        ex("~/.herdr/worktrees")
    } else {
        ctx.worktrees.clone()
    };
    let root = cfg.hosts.get(name).and_then(|h| h.root.as_deref());
    let (hs, refusal) = match host::agents(&on) {
        Err(_) => (Host::default(), Some(("NO_HOST", "unreachable".to_string()))),
        Ok(_) => gate(ctx, cfg, &on),
    };
    Placed {
        name: name.into(),
        machine: machine.map(String::from),
        root: root.map_or(def, ex),
        local: ex(&f.local),
        rank: (
            refusal.is_some(),
            hs.load_per_cpu.unwrap_or(0.0),
            -hs.free_mem_gb.unwrap_or(0.0),
        ),
        refusal: refusal.map(|(c, w)| (c, format!("{name}: {w}"))),
        home,
    }
}

/// #157: the host a new lane runs on (None without `[hosts]`): `--host`, else the host of the issue's earlier lanes (its
/// worktree is there, even on a host turned off), else the least-busy enabled host whose tags cover the needs.
pub fn place(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    events: &[Event],
    (fleet, issue, role): (&str, u64, &str),
    force: Option<&str>,
) -> Result<Option<Placed>> {
    if cfg.hosts.is_empty() {
        return match force {
            Some(h) => refused("BAD_VALUE", format!("--host {h}: fleet.toml has no [hosts]")),
            None => Ok(None),
        };
    }
    let f = cfg.fleet(fleet)?;
    if let Some(h) = force {
        let Some(c) = cfg.hosts.get(h) else {
            return refused("BAD_VALUE", format!("--host {h}: no [hosts.{h}]"));
        };
        return Ok(Some(at(ctx, cfg, f, r, (h, c.machine.as_deref()))));
    }
    if let Some(e) = events.iter().rev().find(|e| e.is("spawn", fleet, issue)) {
        let was = (e.str("host").unwrap_or("local"), e.str("machine"));
        return Ok(Some(at(ctx, cfg, f, r, was)));
    }
    let needs = needs(f, role, events, (fleet, issue));
    let seen = cached(ctx);
    let fits: Vec<_> = cfg
        .hosts
        .iter()
        .filter(|(n, h)| needs.iter().all(|t| tags(h, seen.get(*n)).contains(t)))
        .collect();
    let mut on: Vec<Placed> = fits
        .iter()
        .filter(|h| h.1.enabled)
        .map(|(n, h)| at(ctx, cfg, f, r, (n, h.machine.as_deref())))
        .collect();
    if on.is_empty() {
        let (enabled, has) = (["enabled ", ""][usize::from(fits.is_empty())], needs.join(", "));
        let has = if has.is_empty() { has } else { format!(" has {has}") };
        return refused("NO_HOST", format!("no {enabled}host{has}"));
    }
    on.sort_by(|a, b| a.rank.partial_cmp(&b.rank).unwrap_or(Equal));
    Ok(Some(on.remove(0)))
}

/// `a` with this host's `$HOME` moved to the lane host's `home` (a remote lane's profile, prompt and env paths).
pub fn rehome(a: Vec<String>, home: Option<&str>) -> Vec<String> {
    let mine = crate::config::home().to_string_lossy().into_owned();
    match home.filter(|_| mine.len() > 1) {
        Some(h) => a.into_iter().map(|w| w.replace(&mine, h)).collect(),
        None => a,
    }
}

/// The agents of the open lanes on host `name` (a spawn without `host` ran on the machine-less one).
pub fn lanes_on(cfg: &Config, events: &[Event], name: &str) -> Vec<String> {
    let here = cfg.hosts.get(name).is_some_and(|h| h.machine.is_none());
    let on = |l: &&str| last_spawn(events, l).is_some_and(|e| e.str("host").map_or(here, |h| h == name));
    let st = fold(events);
    let open = st.values().filter(|s| !s.done()).filter_map(|s| s.active_lane());
    open.map(|l| l.1).filter(on).map(String::from).collect()
}

/// A host as the board and `hosts` say it: (tone, words). Offline and off are grey: neither pages.
pub fn shown(h: &HostCfg, s: &Seen, lanes: usize, now: u64) -> (&'static str, String) {
    if !s.up {
        let last = [age(now.saturating_sub(s.seen)), "never".into()][usize::from(s.seen == 0)].clone();
        return ("m", format!("offline · last seen {last}"));
    }
    match (h.enabled, lanes) {
        (false, 0) => ("m", "off · not used for lanes".into()),
        (false, n) => ("m", format!("off · draining {n} lane{}", if n == 1 { "" } else { "s" })),
        _ => ("g", "up".into()),
    }
}

/// `herdsman hosts [refresh]`: a fresh look at each host; `refresh` probes the tags again.
pub fn list(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, refresh: bool) -> Result<()> {
    if cfg.hosts.is_empty() {
        return Ok(writeln!(out, "no [hosts] in fleet.toml: every lane runs on this host")?);
    }
    survey(ctx, cfg, r, refresh);
    let (seen, events) = (cached(ctx), read(&ctx.state)?);
    let n = |x: Option<f64>, p: usize| x.map_or("?".into(), |x| format!("{x:.p$}"));
    for (name, h) in &cfg.hosts {
        let s = seen.get(name).cloned().unwrap_or_default();
        let lanes = lanes_on(cfg, &events, name);
        let (words, t) = (shown(h, &s, lanes.len(), ctx.now).1, tags(h, Some(&s)).join(","));
        let (l, m, d, k, ls) = (n(s.load, 2), n(s.mem, 1), n(s.disk, 0), lanes.len(), lanes.join(" "));
        writeln!(
            out,
            "{name} · {words} · tags {t} · load/cpu {l} · mem {m} GB · disk {d} GB free · {k} lane(s) {ls}"
        )?;
    }
    Ok(())
}

/// `herdsman hosts on|off <name>` (and the board's button): `[hosts.<name>] enabled`, logged `host-on`/`host-off`.
pub fn toggle(ctx: &Ctx, out: &mut dyn Write, name: &str, on: bool, by: &str) -> Result<()> {
    let text = std::fs::read_to_string(&ctx.config)?;
    let cfg = Config::parse(&text)?;
    if !cfg.hosts.contains_key(name) {
        bail!("no [hosts.{name}] in fleet.toml");
    }
    let new = set_in(&text, &format!("[hosts.{name}]"), "enabled", &on.to_string())?;
    write_atomic(&ctx.config, &new)?;
    let word = if on { "on" } else { "off" };
    let (kind, d) = (format!("host-{word}"), json!({"host": name, "by": by}));
    emit(&ctx.state, &cfg, Event::new(ctx.now, "herdsman", None, &kind, d))?;
    Ok(writeln!(out, "{name}: {word} for new lanes")?)
}
