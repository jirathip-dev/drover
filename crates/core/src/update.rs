use crate::config::{Config, Limits};
use crate::events::{emit, working, Event};
use crate::hosts::On;
use crate::release::{Builds, Found, Unstaged};
use crate::runner::{ok, Runner};
use crate::Ctx;
use anyhow::{bail, Context, Result};
use serde_json::json;
use std::io::Write;
use std::path::{Path, PathBuf};

/// `drover self-update [--dry-run | --pause | --resume]`.
pub fn self_update(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    mode: (bool, bool, bool),
) -> Result<i32> {
    let (dry, pause, resume) = mode;
    let events = working(&ctx.state)?;
    if pause || resume {
        let (kind, word) = if pause {
            ("self-update-paused", "paused")
        } else {
            ("self-update-resumed", "resumed")
        };
        let by = crate::config::env("DROVER_LANE").filter(|l| !l.is_empty());
        let d = json!({"by": by.as_deref().unwrap_or("owner")});
        emit(&ctx.state, cfg, Event::new(ctx.now, "drover", None, kind, d))?;
        writeln!(out, "✓ self-update {word}")?;
        return Ok(0);
    }
    let toggle = events
        .iter()
        .rfind(|e| e.kind == "self-update-paused" || e.kind == "self-update-resumed");
    if toggle.is_some_and(|e| e.kind == "self-update-paused") {
        writeln!(out, "= self-update paused → drover self-update --resume")?;
        return Ok(0);
    }
    let Some(f) = cfg.fleet.values().find(|f| ours(f)) else {
        writeln!(out, "= self-update: no fleet for the drover repo")?;
        return Ok(0);
    };
    let live = std::fs::canonicalize(&ctx.exe).unwrap_or_else(|_| ctx.exe.clone().into());
    let t = Target {
        branch: &cfg.self_update.branch,
        kind: Kind::Live,
        live,
    };
    let code = install(ctx, cfg, r, out, (f, &events), &t, dry)?;
    if !cfg.self_update.staging_serve {
        return Ok(code);
    }
    let live = ctx.state.with_file_name("update-staging").join("bin/drover");
    let t = Target {
        branch: "staging",
        kind: Kind::StagingServe,
        live,
    };
    Ok(code.max(install(ctx, cfg, r, out, (f, &events), &t, dry)?))
}

/// The control socket of the serve listening on `port` (`drover_server::handover`).
pub fn serve_sock(ctx: &Ctx, port: u16) -> PathBuf {
    ctx.state.with_file_name(format!("serve-{port}.sock"))
}

/// This repo's own fleet, by either name until the GitHub repo is renamed too.
pub fn ours(f: &crate::config::Fleet) -> bool {
    ["drover", "herdsman"].contains(&f.repo_name())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Live,
    StagingServe,
}

struct Target<'a> {
    branch: &'a str,
    kind: Kind,
    live: PathBuf,
}

impl Target<'_> {
    fn word(&self) -> &'static str {
        match self.kind {
            Kind::Live => "live",
            Kind::StagingServe => "staging-serve",
        }
    }
    fn what(&self) -> &'static str {
        match self.kind {
            Kind::Live => "",
            Kind::StagingServe => "staging serve: ",
        }
    }
}

struct Member<'a> {
    name: String,
    on: On<'a>,
    exe: String,
    conf: String,
    tags: Vec<String>,
    builds: Builds,
    failed: Option<Unstaged>,
}

impl Member<'_> {
    fn needs(&self, head: &str) -> bool {
        self.builds.live.as_deref() != Some(head) || self.builds.unsigned
    }
    fn staged(&self, head: &str) -> bool {
        self.builds.staged.as_deref() == Some(head)
    }
    fn has(&self) -> String {
        let sha = (self.builds.live.as_deref()).map_or("?".into(), |s| s.chars().take(7).collect());
        if self.builds.unsigned {
            format!("{sha} unsigned")
        } else {
            sha
        }
    }
}

struct Marks {
    seen: Option<u64>,
    staging: Option<u64>,
    waiting: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Step {
    Wait { alert: bool },
    Stuck,
    Swap,
}

fn builds(on: &On, exe: &str, tags: &[String]) -> Result<Builds> {
    let mut b = crate::release::builds(on, exe)?;
    b.unsigned = b.live.is_some() && tags.iter().any(|t| t == "os:macos") && !crate::release::signed(on, exe);
    Ok(b)
}

fn member<'a>(ctx: &Ctx, cfg: &'a Config, r: &'a dyn Runner, t: &Target) -> Member<'a> {
    let here = On::new(r, None);
    let exe = t.live.to_string_lossy().to_string();
    let arch = std::env::consts::ARCH.replace("aarch64", "arm64");
    let main = cfg
        .hosts
        .iter()
        .find(|h| h.1.machine.is_none())
        .map_or("local", |h| h.0.as_str());
    let seen = crate::hosts::cached(ctx);
    let probed = cfg.hosts.get(main).zip(seen.get(main).filter(|s| !s.tags.is_empty()));
    let tags = probed.map_or_else(
        || vec![format!("os:{}", std::env::consts::OS), format!("arch:{arch}")],
        |(h, s)| crate::hosts::tags(h, Some(s)),
    );
    Member {
        name: main.into(),
        builds: builds(&here, &exe, &tags).unwrap_or_default(),
        on: here,
        exe,
        conf: ctx.config.display().to_string(),
        tags,
        failed: None,
    }
}

fn step(now: u64, head: &str, found: &Found, m: &Member, marks: &Marks, l: &Limits) -> Step {
    if let Found::Missing(_) = found {
        let due = marks
            .seen
            .is_some_and(|s| now >= s.saturating_add(l.self_update_wait_minutes * 60));
        return Step::Wait {
            alert: due && !marks.waiting,
        };
    }
    if m.staged(head) {
        Step::Swap
    } else {
        Step::Stuck
    }
}

fn install(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (f, events): (&crate::config::Fleet, &[Event]),
    t: &Target,
    dry: bool,
) -> Result<i32> {
    let l = f.local_path();
    let git = |a: &[&str]| ok(r, "git", &[&["-C", &*l.to_string_lossy()], a].concat());
    let what = t.what();
    git(&["fetch", "-q", "origin", &crate::clone::refspec(t.branch)])?;
    let head = git(&["rev-parse", &format!("origin/{}", t.branch)])?;
    let head = head.trim();
    let found = crate::release::find(r, &f.repo, t.branch, head);
    let mut m = member(ctx, cfg, r, t);
    if !m.needs(head) {
        writeln!(out, "= {what}up to date @ {head:.7}")?;
        return Ok(0);
    }
    if dry {
        return plan(out, t, head, &found, &m);
    }
    let (word, d) = (t.word(), json!({"sha": head, "branch": t.branch, "target": t.word()}));
    let mark = |kind: &str| {
        let at = |e: &&Event| e.kind == kind && e.str("sha") == Some(head) && e.str("target") == Some(word);
        events.iter().find(at).map(|e| e.ts)
    };
    let marks = Marks {
        seen: mark("self-update-seen"),
        staging: mark("self-update-staging"),
        waiting: mark("self-update-waiting").is_some(),
    };
    let log = |kind: &str, d: serde_json::Value| {
        emit(&ctx.state, cfg, Event::new(ctx.now, "drover", None, kind, d).passive())
    };
    if marks.seen.is_none() {
        log("self-update-seen", d.clone())?;
    }
    if let Found::Release(rel) = &found {
        if marks.staging.is_none() {
            log("self-update-staging", d.clone())?;
        }
        let limit = std::time::Duration::from_secs(cfg.limits.self_update_stage_secs);
        if !m.staged(head) {
            let staged = rel
                .asset(&f.repo, &m.tags)
                .ok_or_else(|| Unstaged::NoAsset(m.tags.join(",")));
            let gate = m.on.exists(Path::new(&m.conf));
            match staged.and_then(|a| crate::release::stage(&m.on, &m.exe, rel, &a, gate, limit)) {
                Ok(()) => m.builds.staged = Some(head.into()),
                Err(u) => m.failed = Some(u),
            }
        }
    }
    if let Some(u) = &m.failed {
        writeln!(out, "✗ {what}{}: {head:.7} not staged: {u}; kept {}", m.name, m.exe)?;
        let why = u.to_string();
        let shed = why
            .lines()
            .find_map(|l| l.split_once("✗ shed"))
            .map(|(_, rest)| format!("✗ shed{rest}"));
        let key = shed.unwrap_or_else(|| format!("{} {head:.7}", m.name));
        let d = json!({"host": m.name, "sha": head, "reason": format!("self-update {head:.7}: {why}")});
        failed(ctx, cfg, events, &key, d)?;
    }
    match step(ctx.now, head, &found, &m, &marks, &cfg.limits) {
        Step::Wait { alert } => {
            let why = match &found {
                Found::Missing(w) => w.as_str(),
                Found::Release(_) => "",
            };
            writeln!(
                out,
                "= {what}{} @ {head:.7}: waiting for its release ({why}); nothing is built",
                t.branch
            )?;
            if alert {
                let (job, error) = crate::release::failure(r, &f.repo, head).unzip();
                let at =
                    (job.as_ref().zip(error.as_ref())).map_or(String::new(), |(j, e)| format!("; the {j} job: {e}"));
                let reason = format!("no release build of {} {head:.7}: {why}{at}", t.branch);
                let d = json!({"key": head, "sha": head, "branch": t.branch, "target": word, "job": job, "error": error, "reason": reason});
                emit(
                    &ctx.state,
                    cfg,
                    Event::new(ctx.now, "drover", None, "self-update-waiting", d).push(),
                )?;
            }
        }
        Step::Stuck => return Ok(1),
        Step::Swap => return swap(ctx, cfg, r, out, (t, events, head), &m),
    }
    Ok(0)
}

fn failed(ctx: &Ctx, cfg: &Config, events: &[Event], key: &str, mut d: serde_json::Value) -> Result<()> {
    if events
        .iter()
        .any(|e| e.kind == "self-update-failed" && e.str("key") == Some(key))
    {
        return Ok(());
    }
    if let Some(o) = d.as_object_mut() {
        o.insert("key".into(), key.into());
    }
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, "drover", None, "self-update-failed", d),
    )
}

fn plan(out: &mut dyn Write, t: &Target, head: &str, found: &Found, m: &Member) -> Result<i32> {
    let have = match found {
        Found::Release(rel) => format!("{} found", rel.tag),
        Found::Missing(w) => format!("waiting: {w}"),
    };
    writeln!(out, "{}{} @ {head:.7}: {have}", t.what(), t.branch)?;
    let (has, will) = if m.staged(head) {
        (format!("{}, staged {head:.7}", m.has()), "would swap")
    } else {
        (m.has(), "would stage, then swap")
    };
    writeln!(out, "  {:<12} {has:<24} {will}, last", m.name)?;
    let then = match t.kind {
        Kind::Live => "then: profiles sync, serve handed over",
        Kind::StagingServe => "then: the read-only staging serve restarts",
    };
    writeln!(out, "{then}")?;
    Ok(0)
}

fn swap(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (t, events, head): (&Target, &[Event], &str),
    m: &Member,
) -> Result<i32> {
    let what = t.what();
    let new = PathBuf::from(format!("{}.new", t.live.display()));
    std::fs::rename(&new, &t.live).with_context(|| format!("swap into {}", t.live.display()))?;
    let d = json!({"sha": head, "branch": t.branch, "target": t.word(), "hosts": [m.name]});
    emit(&ctx.state, cfg, Event::new(ctx.now, "drover", None, "self-updated", d))?;
    let then = match t.kind {
        Kind::StagingServe => staging_serve(ctx, cfg, r, &t.live).context("installed, then the staging serve"),
        Kind::Live => {
            let synced =
                ok(r, &t.live.to_string_lossy(), &["profiles", "sync"]).context("installed, then profiles sync");
            let served = synced.and_then(|_| restart_serve(ctx, cfg, r).context("installed, then the serve restart"));
            served.map(|s| {
                format!(
                    "profiles synced, {}",
                    [vec![s], crate::timer::heal(ctx, r)].concat().join("; ")
                )
            })
        }
    };
    match then {
        Ok(done) => {
            writeln!(out, "✓ {what}installed {head:.7} at {}; {done}", t.live.display())?;
            Ok(0)
        }
        Err(e) => {
            let why = format!("{head:.7}: {e:#}");
            writeln!(out, "✗ self-update {why}; now live at {}", t.live.display())?;
            if e.is::<ServeDown>() {
                let d = json!({"key": head, "reason": format!("serve down after self-update {why}")});
                emit(
                    &ctx.state,
                    cfg,
                    Event::new(ctx.now, "drover", None, "serve-down", d).push(),
                )?;
                return Ok(1);
            }
            let key = why.lines().find(|l| l.starts_with("✗ shed")).unwrap_or(head);
            failed(ctx, cfg, events, key, json!({"reason": format!("self-update {why}")}))?;
            Ok(1)
        }
    }
}

#[derive(Debug)]
struct ServeDown(String);

impl std::fmt::Display for ServeDown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ServeDown {}

fn restart_serve(ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> Result<String> {
    let (label, addr) = (
        cfg.serve.launchd_label.as_deref().unwrap_or("dev.drover.serve"),
        cfg.serve.addr(),
    );
    let first = crate::timer::restart(ctx, r, label);
    if first.is_ok() && crate::timer::serving(ctx, r, label, &addr) {
        return Ok("serve restarted".into());
    }
    let again = crate::timer::rebootstrap(ctx, r, label, false);
    if again.is_ok() && crate::timer::serving(ctx, r, label, &addr) {
        return Ok("serve restarted, bootstrapped again".into());
    }
    let said: Vec<String> = [first, again]
        .into_iter()
        .filter_map(Result::err)
        .map(|e| format!("{e:#}"))
        .collect();
    let said = if said.is_empty() {
        format!("{label} has no pid or {addr} does not answer")
    } else {
        said.join("; ")
    };
    Err(ServeDown(said).into())
}

/// Ask the serve on control socket `sock` to hand over; the bridge's pid once it serves.
pub fn ask_handover(sock: &Path, secs: u64) -> Result<String> {
    let mut c = std::os::unix::net::UnixStream::connect(sock)?;
    c.set_read_timeout(Some(std::time::Duration::from_secs(secs.max(1))))?;
    writeln!(c, "handover")?;
    let mut line = String::new();
    std::io::BufRead::read_line(&mut std::io::BufReader::new(c), &mut line)?;
    let pid = line.trim().strip_prefix("ok ");
    Ok(pid.with_context(|| format!("serve: {}", line.trim()))?.to_string())
}

/// The build sha `/api/v1/health` on `addr` reports (`?` from a build before it did); `None` if nothing answers.
pub fn serving(addr: &str, secs: u64) -> Option<String> {
    use std::io::Read;
    let wait = Some(std::time::Duration::from_secs(secs.max(1)));
    let mut s = std::net::TcpStream::connect(addr).ok()?;
    s.set_read_timeout(wait).ok()?;
    let req = format!("GET /api/v1/health HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).ok()?;
    let mut res = String::new();
    s.read_to_string(&mut res).ok()?;
    let body: serde_json::Value = serde_json::from_str(res.split_once("\r\n\r\n")?.1).ok()?;
    let sha = body.get("serve")?.get("sha").and_then(serde_json::Value::as_str);
    Some(sha.filter(|s| !s.is_empty()).unwrap_or("?").to_string())
}

fn staging_serve(ctx: &Ctx, cfg: &Config, r: &dyn Runner, live: &Path) -> Result<String> {
    let at = Ctx {
        exe: live.to_string_lossy().into(),
        ..ctx.clone()
    };
    let addr = &cfg.self_update.staging_addr;
    let args = ["serve", "--read-only", "--addr", addr];
    let job = crate::timer::install(&at, cfg, r, crate::timer::STAGING, &args, true);
    if job.starts_with('✗') {
        bail!("{job}");
    }
    if job.starts_with("= already") {
        crate::timer::restart(ctx, r, crate::timer::STAGING)?;
    }
    Ok(format!("staging serve on {addr}"))
}

/// `! self-update — paused since <time> → …` while the last toggle is a pause (doctor and the board).
pub fn paused_line(r: &dyn Runner, events: &[Event]) -> Option<String> {
    let e = events
        .iter()
        .rfind(|e| e.kind == "self-update-paused" || e.kind == "self-update-resumed")
        .filter(|e| e.kind == "self-update-paused")?;
    let at = crate::usage::date(r, e.ts, "+%b %-d %H:%M");
    Some(format!(
        "! self-update — paused since {at} → drover self-update --resume"
    ))
}
