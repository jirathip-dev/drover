use crate::access::{civil_month_day, civil_year};
use crate::config::{Config, Fleet};
use crate::events::{append, emit, plain, working, Event, Json};
use crate::runner::{argv, ok, Runner};
use crate::Ctx;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::io::Write;

include!(concat!(env!("OUT_DIR"), "/jobs_files.rs"));

const HOUR: u64 = 3600;
const DAY: u64 = 86_400;

fn every<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<u64, D::Error> {
    let v = toml::Value::deserialize(d)?;
    let bad = || {
        serde::de::Error::custom(format!(
            "every = {v}: a duration like \"12h\" or \"7d\", or 0 or \"off\""
        ))
    };
    let s = match &v {
        toml::Value::Integer(0) => return Ok(0),
        toml::Value::String(s) if s == "off" || s == "0" => return Ok(0),
        toml::Value::String(s) => s,
        _ => return Err(bad()),
    };
    let (n, unit) = (s.strip_suffix('h').map(|n| (n, HOUR)))
        .or_else(|| s.strip_suffix('d').map(|n| (n, DAY)))
        .ok_or_else(bad)?;
    let n = n.parse::<u64>().ok().filter(|n| *n > 0).ok_or_else(bad)?;
    n.checked_mul(unit).ok_or_else(bad)
}

/// Where a job's issue goes in the queue.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Place {
    #[default]
    Top,
    End,
}

/// A job after its layers are merged and checked at load: what `run` trusts.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    #[serde(default, deserialize_with = "every")]
    pub every: u64,
    pub check: Option<String>,
    pub title: String,
    pub body: String,
    pub role: String,
    #[serde(default)]
    pub queue: Place,
    pub empty: Option<String>,
}

/// Fleet `fleet`'s jobs: each built-in entry, then each of `layers` over it key by key; a bad key or a title with an unknown placeholder is a load error.
pub fn entries(fleet: &str, layers: &[&BTreeMap<String, toml::Table>]) -> Result<BTreeMap<String, Job>> {
    let mut all: BTreeMap<String, toml::Table> = BTreeMap::new();
    for (file, text) in BUILTIN {
        let t = toml::from_str(text).with_context(|| format!("roles/jobs/{file}"))?;
        all.insert(file.trim_end_matches(".toml").into(), t);
    }
    for (name, t) in layers.iter().flat_map(|l| l.iter()) {
        all.entry(name.clone()).or_default().extend(t.clone());
    }
    let job = |(name, t): (String, toml::Table)| {
        let at = format!("fleet.{fleet}.job.{name}");
        let job: Job = toml::Value::Table(t).try_into().with_context(|| at.clone())?;
        let role = &job.role;
        if role.is_empty() || !role.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            bail!("{at}: role \"{role}\" is letters, digits and `-`");
        }
        if fill(&job.title, &[("{fleet}", fleet), ("{integration}", "x")], 0).contains('{') {
            bail!("{at}: title \"{}\" has a placeholder other than {{fleet}} {{integration}} {{date}} {{monday}} {{month}}", job.title);
        }
        Ok((name, job))
    };
    all.into_iter().map(job).collect()
}

fn ymd(day: u64) -> String {
    let (m, d) = civil_month_day(day);
    format!("{}-{m:02}-{d:02}", civil_year(day))
}

fn fill(text: &str, vars: &[(&str, &str)], now: u64) -> String {
    let day = now / DAY;
    let (m, _) = civil_month_day(day);
    let month = format!("{}-{m:02}", civil_year(day));
    let dates = [
        ("{date}", ymd(day)),
        ("{monday}", ymd(day.saturating_sub((day + 3) % 7))),
        ("{month}", month),
    ];
    let s = dates.iter().fold(text.to_string(), |s, (k, v)| s.replace(k, v));
    vars.iter().fold(s, |s, (k, v)| s.replace(k, v))
}

/// When `tick` last ran fleet `fleet`'s job `name` (its stamp).
pub fn last(ctx: &Ctx, name: &str, fleet: &str) -> Option<u64> {
    let at = ctx.state.with_file_name(format!("{name}-{fleet}.at"));
    std::fs::read_to_string(at).ok()?.trim().parse().ok()
}

fn filed<'a>(events: &'a [Event], fleet: &str, n: u64) -> Option<&'a Event> {
    events.iter().rfind(|e| e.is("job-filed", fleet, n))
}

/// The job that filed issue `n`, from its `job-filed`.
pub fn of<'a>(events: &'a [Event], fleet: &str, n: u64) -> Option<&'a str> {
    filed(events, fleet, n)?.str("job")
}

/// The role a job's issue `n` is worked by.
pub fn role<'a>(events: &'a [Event], fleet: &str, n: u64) -> Option<&'a str> {
    filed(events, fleet, n)?.str("role")
}

/// The line a job's lane that ends with nothing pushed closes issue `n` with; None: not such a job.
pub fn empty<'a>(cfg: &'a Config, events: &[Event], fleet: &str, n: u64) -> Option<&'a str> {
    cfg.fleet.get(fleet)?.jobs.get(of(events, fleet, n)?)?.empty.as_deref()
}

/// The one prompt a job's lane gets when its turn ends with no push and no `done`.
pub fn prompt(cfg: &Config, events: &[Event], (fleet, n): (&str, u64)) -> String {
    let line = empty(cfg, events, fleet, n).unwrap_or_default();
    format!("{line}? end with `drover done --no-change \"{line}\" {fleet} {n}`")
}

/// Close a job's issue `n` with `line` and the undo of each change its lane logged on it.
pub fn close(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, n): (&str, u64),
    line: &str,
) -> Result<()> {
    let events = working(&ctx.state)?;
    let undo = |e: &Event| Some(format!("\n- {} · undo: `{}`", plain(e), e.str("undo")?));
    let list: String = (events.iter().filter(|e| e.fleet == fleet && e.issue == Some(n)))
        .filter_map(undo)
        .collect();
    let why = format!("{line}{list}");
    let repo = &cfg.fleet(fleet)?.repo;
    ok(
        r,
        "gh",
        &["issue", "close", &format!("{n}"), "-R", repo, "--comment", &why],
    )?;
    let e = Event::new(ctx.now, fleet, Some(n), "closed", json!({"why": why}));
    emit(&ctx.state, cfg, e)?;
    Ok(writeln!(out, "closed {fleet}#{n}: {line}")?)
}

/// A job lane's `done --no-change <line>` before any push: its issue closes with that line.
pub fn done(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, n): (&str, u64),
    line: &str,
) -> Result<()> {
    let pushed = crate::gate::issue_state(ctx, fleet, n)?.is_some_and(|s| s.rounds > 0);
    if pushed || empty(cfg, &working(&ctx.state)?, fleet, n).is_none() {
        return Ok(());
    }
    close(ctx, cfg, r, out, (fleet, n), line)
}

fn open(events: &std::sync::Arc<crate::events::Log>, fleet: &str, name: &str) -> Option<u64> {
    let mine = |e: &&Event| e.kind == "job-filed" && e.fleet == fleet && e.str("job") == Some(name);
    (events.iter().filter(mine).filter_map(|e| e.issue))
        .rfind(|n| events.states.get(&(fleet.to_string(), *n)).is_none_or(|s| !s.done()))
}

/// What started a run: `tick` on the schedule, or by hand (`drover job`, the board, with the paired device in `by`).
#[derive(Clone, Copy)]
pub enum Trigger<'a> {
    Tick,
    Hand { by: Option<&'a str> },
}

/// A run's outcome.
pub enum Ran {
    Nothing(String),
    Filed { issue: u64, confirmed: bool },
}

fn checkout(ctx: &Ctx, r: &dyn Runner, f: &Fleet, fleet: &str) -> Result<String> {
    let local = f.local_path().to_string_lossy().into_owned();
    let old = ctx.state.with_file_name(format!("deps-{fleet}"));
    if old.exists() {
        let old = old.to_string_lossy();
        let rm = argv(&["-C", &local, "worktree", "remove", "--force", &old]);
        r.run("git", &rm).ok();
    }
    let dir = ctx.state.with_file_name(format!("job-{fleet}"));
    let (d, head) = (dir.to_string_lossy().into_owned(), format!("origin/{}", f.integration));
    let spec = crate::clone::refspec(&f.integration);
    ok(r, "git", &["-C", &local, "fetch", "-q", "origin", &spec])?;
    if dir.exists() {
        ok(r, "git", &["-C", &d, "checkout", "-q", "--force", "--detach", &head])?;
    } else {
        let add = ["-C", &local, "worktree", "add", "-q", "--detach", &d, &head];
        ok(r, "git", &add)?;
    }
    Ok(d)
}

/// Run fleet `fleet`'s job `name`: its check (a pass by `tick` is nothing to do), at most one open issue and never a title that exists, then file, queue and start it.
pub fn run(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, name): (&str, &str),
    how: Trigger,
) -> Result<Ran> {
    let f = cfg.fleet(fleet)?;
    let job =
        (f.jobs.get(name)).with_context(|| format!("{fleet} has no job {name}: `drover job {fleet}` lists them"))?;
    let mut vars = vec![
        ("{fleet}", fleet),
        ("{integration}", f.integration.as_str()),
        ("{drover}", ctx.exe.as_str()),
    ];
    let mut report = String::new();
    if let Some(cmd) = &job.check {
        let d = checkout(ctx, r, f, fleet)?;
        let sh = format!("cd \"$0\" && {}", fill(cmd, &vars, ctx.now));
        let o = r.run("sh", &argv(&["-c", &sh, &d]))?;
        let text = format!("{}{}", o.stdout, o.stderr).replace(&d, ".");
        let skip = text.lines().count().saturating_sub(200);
        report = text.lines().skip(skip).collect::<Vec<_>>().join("\n");
        if o.code == 0 && matches!(how, Trigger::Tick) {
            return Ok(Ran::Nothing("its check passed".into()));
        }
    }
    if let Some(n) = open(&working(&ctx.state)?, fleet, name) {
        return Ok(Ran::Nothing(format!("its issue #{n} is open")));
    }
    let title = fill(&job.title, &vars, ctx.now);
    if let Some(n) = crate::gh::titled(cfg, r, fleet, &title)? {
        return Ok(Ran::Nothing(format!("#{n} has the title {title:?}")));
    }
    vars.push(("{report}", &report));
    let body = fill(&job.body, &vars, ctx.now);
    let a = ["issue", "create", "-R", &f.repo, "--title", &title, "--body", &body];
    let url = crate::hygiene::gh(ctx, r, cfg, fleet, &a)?;
    let n: u64 = (url.trim().rsplit('/').next().and_then(|n| n.parse().ok())).context("gh issue create: no number")?;
    writeln!(out, "filed {fleet}#{n}: {title}")?;
    let by = match how {
        Trigger::Hand { by } => by,
        Trigger::Tick => None,
    };
    let mut d = json!({"job": name, "role": job.role, "title": title});
    let mut op = crate::queue::op("add", &[n], job.queue == Place::Top, None, None, None);
    if let Some(by) = by {
        d.put("by", json!(by));
        op.put("by", json!(by));
    }
    append(&ctx.state, &Event::new(ctx.now, fleet, Some(n), "job-filed", d))?;
    writeln!(out, "{}", crate::queue_change(ctx, cfg, fleet, op, by.map(|_| "web"))?)?;
    let o = crate::spawn::Opts::default();
    let confirmed =
        match (job.queue == Place::Top).then(|| crate::spawn::spawn(ctx, cfg, r, out, (fleet, n, &job.role), &o)) {
            Some(Err(e)) if e.downcast_ref::<crate::gate::Refusal>().is_some() => {
                writeln!(out, "{fleet}#{n} stays queued; drive starts it as {}: {e}", job.role)?;
                true
            }
            Some(res) => res?,
            None => true,
        };
    Ok(Ran::Filed { issue: n, confirmed })
}

/// `tick`'s one step for every job: each due job of each fleet not paused runs; a failure logs `job-failed`.
pub fn tick(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write) -> Result<()> {
    for (fleet, f) in cfg.fleet.iter().filter(|f| !f.1.paused) {
        for (name, job) in &f.jobs {
            if job.every == 0 || !crate::timer::due(ctx, &format!("{name}-{fleet}"), job.every)? {
                continue;
            }
            match run(ctx, cfg, r, out, (fleet, name), Trigger::Tick) {
                Ok(Ran::Nothing(why)) => writeln!(out, "{fleet} {name}: {why}")?,
                Ok(Ran::Filed { .. }) => {}
                Err(e) => {
                    writeln!(out, "{fleet} {name}: {e:#}")?;
                    let d = json!({"job": name, "why": format!("{e:#}")});
                    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, None, "job-failed", d))?;
                }
            }
        }
    }
    Ok(())
}

fn span(secs: u64) -> String {
    if secs.is_multiple_of(DAY) {
        format!("{}d", secs / DAY)
    } else {
        format!("{}h", secs / HOUR)
    }
}

/// `drover job <fleet> [name]`.
#[derive(clap::Args)]
pub struct Args {
    pub fleet: String,
    #[arg(help = "The job to run now (by hand: its check's verdict is ignored, at most one open issue still holds)")]
    pub name: Option<String>,
}

/// Run `drover job`: the fleet's jobs, or one run by hand; exit 4 when the lane's prompt was not confirmed.
pub fn cli(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, a: &Args) -> Result<i32> {
    let (fleet, f) = (a.fleet.as_str(), cfg.fleet(&a.fleet)?);
    let Some(name) = &a.name else {
        let events = working(&ctx.state)?;
        for (name, job) in &f.jobs {
            let at = |t: Option<u64>| t.map_or_else(|| "never".into(), |t| ymd(t / DAY));
            let last = last(ctx, name, fleet);
            let next = at(Some(last.map_or(ctx.now, |t| t + job.every)));
            let mut s = match job.every {
                0 => "off".into(),
                e => format!("every {}   last {}   next {next}", span(e), at(last)),
            };
            if let Some(n) = open(&events, fleet, name) {
                s += &format!("   open #{n}");
            }
            writeln!(out, "{name:<10} {s}")?;
        }
        return Ok(0);
    };
    match run(ctx, cfg, r, out, (fleet, name), Trigger::Hand { by: None })? {
        Ran::Nothing(why) => writeln!(out, "{fleet} {name}: {why}")?,
        Ran::Filed { confirmed: false, .. } => return Ok(4),
        Ran::Filed { .. } => {}
    }
    Ok(0)
}
