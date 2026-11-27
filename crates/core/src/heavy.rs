use crate::runner::Runner;
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub routes: BTreeMap<String, Route>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub never: Vec<String>,
}

/// One heavy kind's build machines: `runners run --tag <tag>` with its optional flags.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, clap::Args)]
#[serde(default, deny_unknown_fields)]
pub struct Route {
    #[arg(long = "remote", required = false)]
    pub tag: String,
    #[arg(id = "remote_wait", long = "remote-wait")]
    pub wait: Option<u64>,
    #[arg(long)]
    pub setup: Option<String>,
    #[arg(long)]
    pub env: Vec<String>,
    #[arg(long)]
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
pub const NEVER_ROUTED: &str = "git *,gh *,drover *,*nx affected*,*--changed*,*merge-base*,*origin/*,*codesign*,*notarytool*,*--release*,cargo install*,docker *,podman *,colima *,orb *,*supabase start*,*<*,*.env*";

/// The `cargo` kind of a fleet with heavy kinds or routes that sets no `cargo` kind of its own.
pub const CARGO: &str = "cargo build*,cargo check*,cargo clippy*,cargo test*,cargo nextest*,cargo run*,cargo xtask*";

#[derive(Default)]
struct Part {
    words: Vec<String>,
    plain: Vec<String>,
    redirs: Vec<String>,
    op: String,
}

impl Part {
    fn args(&self) -> Vec<&str> {
        let mut w: Vec<&str> = (self.plain.iter()).map(String::as_str).filter(|w| !w.is_empty()).collect();
        let at = w.iter().position(|w| !w.contains('=') || w.starts_with('-'));
        w.split_off(at.unwrap_or(w.len()))
    }

    fn plain(&self) -> String {
        self.args().join(" ")
    }

    fn dirs(&self) -> Vec<&str> {
        let a = self.args();
        let cd = matches!(a.first(), Some(&("cd" | "pushd"))).then(|| a.get(1).copied().unwrap_or("~"));
        let flag = |(f, v): (&&str, &&str)| matches!(*f, "-C" | "--manifest-path").then_some(*v);
        let joined = a.iter().filter_map(|w| w.strip_prefix("--manifest-path="));
        cd.into_iter().chain(a.iter().zip(a.iter().skip(1)).filter_map(flag)).chain(joined).collect()
    }

    fn text(&self) -> String {
        self.words
            .iter()
            .chain(&self.redirs)
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Default)]
struct Line {
    parts: Vec<Part>,
    unsplit: Option<&'static str>,
    part: Part,
    w: String,
    m: String,
    word: bool,
    to: Option<String>,
}

impl Line {
    fn end_word(&mut self) {
        if !self.word {
            return;
        }
        self.word = false;
        let (w, m) = (std::mem::take(&mut self.w), std::mem::take(&mut self.m));
        if let Some(op) = self.to.take() {
            self.part.redirs.push(op + &w);
        } else {
            self.part.words.push(w);
            self.part.plain.push(m);
        }
    }

    fn end_part(&mut self, op: &str) {
        self.end_word();
        self.part.redirs.extend(self.to.take());
        let mut p = std::mem::take(&mut self.part);
        p.op = op.into();
        self.parts.push(p);
    }

    fn why(&mut self, why: &'static str) {
        self.unsplit.get_or_insert(why);
    }

    fn push(&mut self, c: char, plain: bool) {
        self.word = true;
        self.w.push(c);
        if plain {
            self.m.push(c);
        }
    }

    fn parse(cmd: &str) -> Line {
        let c: Vec<char> = cmd.chars().collect();
        let at = |i: usize| c.get(i).copied();
        let mut l = Line::default();
        let mut docs: Vec<String> = Vec::new();
        let mut i = 0;
        if cmd.contains(".env") {
            l.why(".env");
        }
        while let Some(ch) = at(i) {
            let n = at(i + 1);
            i += 1;
            match ch {
                '\\' => {
                    l.push(ch, false);
                    if let Some(e) = n {
                        l.push(e, true);
                        i += 1;
                    }
                }
                '\'' | '"' => {
                    l.push(ch, false);
                    while let Some(q) = at(i) {
                        i += 1;
                        l.push(q, false);
                        if q == ch {
                            break;
                        }
                        if ch == '"' && q == '\\' {
                            l.push(at(i).unwrap_or_default(), false);
                            i += 1;
                        } else if ch == '"' && (q == '`' || (q == '$' && at(i) == Some('('))) {
                            l.why("a subshell");
                        }
                    }
                }
                '`' => {
                    l.why("a subshell");
                    l.push(ch, true);
                }
                '$' if n == Some('(') => {
                    l.why("a subshell");
                    l.push(ch, true);
                }
                '#' if !l.word => {
                    while at(i).is_some_and(|c| c != '\n') {
                        i += 1;
                    }
                }
                '(' | ')' => {
                    l.why("a subshell");
                    l.end_part(";");
                }
                '<' if n == Some('<') && at(i + 1) != Some('<') => {
                    l.why("a here-doc");
                    i += 1;
                    i += usize::from(at(i) == Some('-'));
                    while at(i).is_some_and(|c| c == ' ' || c == '\t') {
                        i += 1;
                    }
                    let mut d = String::new();
                    while let Some(q) = at(i).filter(|q| !q.is_whitespace() && !";|&<>()".contains(*q)) {
                        if !"'\"\\".contains(q) {
                            d.push(q);
                        }
                        i += 1;
                    }
                    docs.push(d);
                }
                '<' => l.why("stdin (<)"),
                '>' => {
                    let fd = if l.word && l.to.is_none() && l.w.chars().all(|c| c.is_ascii_digit()) {
                        l.word = false;
                        l.m.clear();
                        std::mem::take(&mut l.w)
                    } else {
                        l.end_word();
                        String::new()
                    };
                    let mut op = fd + ">";
                    if let Some(e) = n.filter(|e| matches!(e, '>' | '&' | '|')) {
                        op.push(e);
                        i += 1;
                    }
                    l.to = Some(op);
                }
                '&' if n == Some('>') => {
                    l.end_word();
                    i += 1;
                    let mut op = String::from("&>");
                    if at(i) == Some('>') {
                        op.push('>');
                        i += 1;
                    }
                    l.to = Some(op);
                }
                '&' | '|' if n == Some(ch) => {
                    i += 1;
                    l.end_part(&format!("{ch}{ch}"));
                }
                '&' => {
                    l.why("a background job (&)");
                    l.end_part("&");
                }
                '|' => {
                    i += usize::from(n == Some('&'));
                    l.end_part("|");
                }
                ';' => l.end_part(";"),
                '\n' => {
                    l.end_part("\n");
                    for d in docs.drain(..) {
                        while i < c.len() {
                            let row: String = c.iter().skip(i).take_while(|&&c| c != '\n').collect();
                            i += row.chars().count() + 1;
                            if row.trim() == d {
                                break;
                            }
                        }
                    }
                }
                c if c.is_whitespace() => l.end_word(),
                c => l.push(c, true),
            }
        }
        l.end_part("");
        l
    }
}

fn inside(dir: &str, to: &str) -> bool {
    use std::path::Component;
    if to == "-" || to.contains(['$', '~', '`']) {
        return false;
    }
    let mut n = std::path::PathBuf::new();
    for c in Path::new(dir).join(to).components() {
        match c {
            Component::ParentDir if !n.pop() => return false,
            Component::ParentDir | Component::CurDir => {}
            c => n.push(c),
        }
    }
    n.starts_with(dir)
}

/// Whether shell pattern `p` (`*` any text) matches `s`.
pub fn hits(p: &str, s: &str) -> bool {
    crate::config::glob(p.replace('*', "**").as_bytes(), s.as_bytes())
}

fn matches(ps: &[String], p: &Part) -> bool {
    let s = p.plain();
    ps.iter().any(|q| hits(q, &s))
}

/// The kind whose `[remote.<kind>]` route a fleet's gate checks, done checks and `drover hold` take.
pub const GATE_KIND: &str = "cargo";

impl Heavy {
    /// Fleet `f`'s kinds, routes and never-routed list under host `h`'s slot limits.
    pub fn of(f: &crate::config::Fleet, h: Option<&crate::config::HostCfg>) -> Heavy {
        Heavy {
            kinds: f.heavy_kinds(),
            max: h.and_then(|h| h.heavy_max).unwrap_or(3),
            kind_max: h.map(|h| h.heavy_kind_max.clone()).unwrap_or_default(),
            wait_minutes: h.and_then(|h| h.heavy_wait_minutes).unwrap_or(30),
            routes: f.remote.clone(),
            never: f.never_routed(),
        }
    }

    /// Whether `cmd` goes whole to the `cargo` route's build machines: it has a `cargo` command and no never-routed one.
    pub fn gated(&self, cmd: &str) -> bool {
        let line = Line::parse(cmd);
        let ps = self.kinds.get(GATE_KIND).map_or(&[][..], Vec::as_slice);
        let never = |p: &Part| (self.never.iter()).any(|n| hits(n, &p.plain()) || hits(n, &p.text()));
        self.routes.contains_key(GATE_KIND)
            && line.unsplit.is_none()
            && line.parts.iter().any(|p| matches(ps, p))
            && !line.parts.iter().any(never)
    }

    /// Gate check `cmd` as a shell line: under `exe heavy` on the `cargo` route's build machines from worktree `dir` when `gated`, else unchanged.
    pub fn gate(&self, exe: &str, cmd: &str, dir: &str) -> String {
        if self.gated(cmd) {
            self.wrap(exe, GATE_KIND, cmd, Some(dir))
        } else {
            cmd.into()
        }
    }

    /// The first kind with a pattern matching one of `cmd`'s commands (leading `VAR=value` words, quoted text, comments and here-doc bodies skipped).
    pub fn kind(&self, cmd: &str) -> Option<&str> {
        let line = Line::parse(cmd);
        self.kinds
            .iter()
            .find(|(_, ps)| line.parts.iter().any(|p| matches(ps, p)))
            .map(|(k, _)| k.as_str())
    }

    fn slot(&self, exe: &str, kind: &str, on: Option<(&Route, &str)>, cmd: &str) -> String {
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
        if let Some((r, dir)) = on {
            a.extend(["--remote".into(), r.tag.clone(), "--dir".into(), dir.into()]);
            a.extend(r.flags("--remote-wait"));
        }
        a.extend(["--".into(), cmd.into()]);
        let q = |w: &String| format!("'{}'", w.replace('\'', r"'\''"));
        a.iter().map(q).collect::<Vec<_>>().join(" ")
    }

    /// `cmd` as a shell line under `exe heavy` in a `kind` slot; with a route and the lane's worktree `dir`, its `kind` commands on a build machine first and, in a line mixing them with never-routed ones, only those.
    pub fn wrap(&self, exe: &str, kind: &str, cmd: &str, dir: Option<&str>) -> String {
        let here = self.slot(exe, kind, None, cmd);
        let (Some(r), Some(dir)) = (self.routes.get(kind), dir) else {
            return here;
        };
        let line = Line::parse(cmd);
        let ps = self.kinds.get(kind).map_or(&[][..], Vec::as_slice);
        let never = |p: &Part| (self.never.iter()).any(|n| hits(n, &p.plain()) || hits(n, &p.text()));
        let routed: Vec<bool> = line.parts.iter().map(|p| matches(ps, p) && !never(p)).collect();
        let (mut prev, mut piped, mut split) = ("", false, false);
        for (p, r) in line.parts.iter().zip(&routed) {
            piped |= *r && prev == "|";
            split |= *r && (p.op == "|" || !p.redirs.is_empty());
            prev = &p.op;
        }
        if !routed.contains(&true) {
            return here;
        }
        let away = line.parts.iter().flat_map(Part::dirs).any(|to| !inside(dir, to));
        let why = line.unsplit.or(piped.then_some("a pipe"));
        if let Some(why) = why.or(away.then_some("another checkout")) {
            return format!("echo 'heavy: {why} in the line, running here' >&2; {here}");
        }
        if !split && !line.parts.iter().any(never) {
            return self.slot(exe, kind, Some((r, dir)), cmd);
        }
        let part = |(p, on): (&Part, &bool)| {
            let t = match on {
                true => [self.slot(exe, kind, Some((r, dir)), &p.words.join(" "))]
                    .into_iter()
                    .chain(p.redirs.iter().cloned())
                    .collect::<Vec<_>>()
                    .join(" "),
                false => p.text(),
            };
            match p.op.as_str() {
                "" | "\n" => format!("{t}{}", p.op),
                op => format!("{t} {op} "),
            }
        };
        line.parts
            .iter()
            .zip(&routed)
            .map(part)
            .collect::<String>()
            .trim_end()
            .into()
    }
}

/// `runners run` for `cmd` in worktree `dir` on route `r`: `Ok(code)` once it ran there (the command's own code), else the reason, printed: run it here.
pub fn remote(run: &dyn Runner, r: &Route, dir: &str, cmd: &str) -> Result<i32, String> {
    let mut a: Vec<String> = ["run", "--tag", &r.tag, "--dir", dir].map(String::from).to_vec();
    a.extend(r.flags("--wait"));
    a.extend(["--", "bash", "-c", cmd].map(String::from));
    let why = match run.attached("runners", &a) {
        Ok(c @ (75 | 76 | 64 | 78)) => format!("exit {c}"),
        Ok(c) => return Ok(c),
        Err(e) => match e.downcast_ref::<std::io::Error>().map(std::io::Error::kind) {
            Some(std::io::ErrorKind::NotFound) => "not found".into(),
            _ => format!("{e:#}"),
        },
    };
    crate::stderr!("runners: {why}, running here");
    Err(why)
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
    dir: Option<String>,
    #[command(flatten)]
    route: Option<Route>,
    #[arg(last = true)]
    cmd: String,
}

impl Args {
    /// Run it on its route's build machines, else as `who` in slot folder `dir`'s line; `fell` gets a routed run that falls back here.
    pub fn run(
        self,
        r: &dyn Runner,
        dir: &Path,
        who: String,
        fell: impl FnOnce(&Ticket),
        late: impl FnOnce(&Ticket, &str) -> Option<i32>,
    ) -> Result<i32> {
        let mut me = Ticket::new(who, self.kind, self.cmd, self.kind_max);
        if let (Some(route), Some(wt)) = (&self.route, &self.dir) {
            match remote(r, route, wt, &me.cmd) {
                Ok(code) => return Ok(code),
                Err(why) => me.here = Some(why),
            }
            fell(&me);
        }
        run(r, dir, me, self.max, self.wait, &|| None, late)
    }
}

/// `runners forget --dir <wt>` for a removed worktree: the failure, if any (no `runners` here is none).
pub fn forget(r: &dyn Runner, wt: &str) -> Option<String> {
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
    /// Why a routed command runs here instead of on its build machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub here: Option<String>,
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
            here: None,
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

/// `drover hold`: run `cmd` on its fleet's (the lane's, else the repo's) `cargo` route's build machines when `gated`, else as holder `ci:<repo>` once the main host passes spawn admission and a heavy slot is free; 75 past `wait` minutes.
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
    let line = crate::hosts::quote(prog, args);
    let of_lane = crate::config::env("DROVER_LANE").and_then(|l| Some(l.split(':').next()?.to_string()));
    let fleet = (of_lane.and_then(|l| cfg.fleet.get(&l)))
        .or_else(|| (cfg.fleet.values()).find(|f| f.repo.rsplit('/').next() == Some(repo.as_str())));
    let heavy = fleet.map(|f| Heavy::of(f, host)).filter(|h| h.gated(&cmd.join(" ")));
    let route = heavy.as_ref().and_then(|h| h.routes.get(GATE_KIND));
    let here = std::env::current_dir()?;
    if let Some(Ok(code)) = route.map(|rt| remote(r, rt, &here.to_string_lossy(), &line)) {
        return Ok(code);
    }
    let kind_max = host.and_then(|h| h.heavy_kind_max.get("ci").copied());
    let me = Ticket::new(format!("ci:{repo}"), "ci".into(), line, kind_max);
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
