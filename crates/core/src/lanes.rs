use crate::config::Config;
use crate::events::{last_spawn, Event, IssueState, States};
use crate::runner::{ok, Runner};
use crate::session::{stamp, Cause, Exit, Life, Seen, Session, Sessions, Status};
use crate::Ctx;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Default, Clone)]
pub struct Lane {
    /// The registered fleet the agent works for; None = Others.
    pub fleet: Option<String>,
    /// Agent name: the shed session's name.
    pub name: String,
    pub issue: Option<u64>,
    /// impl | rev | design | orchestrator, or `other` when drover did not spawn it.
    pub role: String,
    pub harness: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub port: Option<String>,
    /// What shed says of it (`Live`), or what the log and git say once shed no longer runs it.
    pub state: LaneState,
    /// Its running shed session; None once it is gone.
    pub session: Option<Session>,
    /// When drover spawned it (0: unknown).
    pub since: u64,
    /// #83: a log-only lane whose worktree is clean and pushed: its short head sha.
    pub parked: Option<String>,
    /// #157: the `[hosts]` host it runs on, when fleet.toml has hosts, and its ssh machine (none: this one).
    pub host: Option<String>,
    pub machine: Option<String>,
    /// #248: why `check` keeps this done lane's session open (its newest `lane-kept-dirty`, none once closed).
    pub kept: Option<String>,
    /// #260: its worktree, from the spawn.
    pub worktree: Option<String>,
    /// #630: its workspace root, from the spawn.
    pub root: Option<String>,
    /// #422: an orchestrator's chat thread (`t6`); with no session it is `asleep`.
    pub thread: Option<String>,
    /// #609: the runtime it was spawned in (`shed` | `shed-acp`), from its spawn: never the config's now.
    pub runtime: String,
    /// #609: where it runs, as every surface shows it: its host while 2+ hosts are enabled (`air`, or empty).
    pub place: String,
}

/// A lane's state on every surface.
#[derive(Clone, Debug, PartialEq)]
pub enum LaneState {
    Live(Status),
    Dead(Option<Cause>),
    Parked,
    Held(Vec<u64>),
    Asleep,
}

impl Default for LaneState {
    fn default() -> Self {
        LaneState::Live(Status::Unreported)
    }
}

impl LaneState {
    /// The word shown: working | blocked | idle | done | unknown | dead | parked | held by #N | asleep.
    pub fn word(&self) -> String {
        match self {
            LaneState::Live(s) => s.word().into(),
            LaneState::Dead(_) => "dead".into(),
            LaneState::Parked => "parked".into(),
            LaneState::Held(on) => format!("held by {}", crate::queue::hashes(on)),
            LaneState::Asleep => "asleep".into(),
        }
    }
    /// shed runs it and says `s`.
    pub fn is(&self, s: Status) -> bool {
        *self == LaneState::Live(s)
    }
}

/// A lane's health for check (the alert) and drive (the act); built only by `health`.
#[derive(Clone, Debug, PartialEq)]
pub enum Health {
    Fine,
    NeverStarted { since: u64 },
    Stalled { since: u64 },
    Dead(Option<Exit>),
    Closed,
    Unknown,
}

/// Join the shed sessions with the log.
pub fn join(cfg: &Config, r: &dyn Runner, events: &[Event], st: &States, sessions: &Sessions) -> Vec<Lane> {
    let by_name = |n: &str| crate::events::last_spawn(events, n);
    let (mut out, mut seen) = (vec![], BTreeSet::new());
    let starts = crate::events::of_kind(events, &["orchestrator-start"]).into_iter();
    let mut cs: BTreeMap<&str, &Event> = starts.filter_map(|e| Some((e.str("agent")?, e))).collect();
    for a in sessions.live() {
        let mut l = Lane {
            name: a.name.clone(),
            state: LaneState::Live(a.status().unwrap_or(Status::Unreported)),
            host: a.host.clone(),
            machine: a.machine.clone(),
            runtime: if a.acp { "shed-acp" } else { "shed" }.into(),
            session: Some(a.clone()),
            ..Lane::default()
        };
        if let Some(e) = by_name(&a.name) {
            l = from_spawn(e, l);
            if !seen.insert(a.name.clone()) {
                let rank = |s: &LaneState| SECTIONS.iter().position(|x| *x == s.word()).unwrap_or(2);
                if let Some(o) = out.iter_mut().find(|o: &&mut Lane| o.name == a.name) {
                    if rank(&l.state) < rank(&o.state) {
                        *o = l;
                    }
                }
                continue;
            }
        } else if let Some(e) = cs.remove(a.name.as_str()) {
            l = orchestrator(events, e, l);
        } else {
            l.fleet = a.labels.get("fleet").filter(|f| cfg.fleet.contains_key(*f)).cloned();
            l.role = "other".into();
            l.harness = a.labels.get("harness").map_or("shell", String::as_str).into();
        }
        out.push(l);
    }
    for s in st.values().filter(|s| cfg.fleet.contains_key(&s.fleet)) {
        let Some((_, agent)) = s.active_lane().filter(|(_, a)| !seen.contains(*a)) else {
            continue;
        };
        if let Some(e) = by_name(agent) {
            let r = &crate::hosts::On::of(r, Some(e));
            let wt = e
                .str("worktree")
                .filter(|wt| crate::spawn::dirty(r, Path::new(wt)).is_none());
            let head = wt.map(|wt| ok(r, "git", &["-C", wt, "rev-parse", "--short=7", "HEAD"]).unwrap_or_default());
            let on = crate::events::held_on(events, s);
            let state = match head {
                _ if !on.is_empty() => LaneState::Held(on.clone()),
                Some(_) => LaneState::Parked,
                None => LaneState::Dead(ended(events, sessions, e).map(|x| x.cause)),
            };
            out.push(from_spawn(
                e,
                Lane {
                    name: agent.into(),
                    state,
                    parked: head.filter(|_| on.is_empty()),
                    ..Lane::default()
                },
            ));
        }
    }
    out.extend(cs.into_values().map(|e| orchestrator(events, e, Lane::default())));
    for l in &mut out {
        let mark = |e: &&Event| matches!(e.kind.as_str(), "spawn" | "lane-closed" | "lane-kept-dirty");
        let last = crate::events::of_agent(events, &l.name).into_iter().rfind(mark);
        let why = last.filter(|e| e.kind == "lane-kept-dirty").and_then(|e| e.str("why"));
        l.kept = why.filter(|_| l.state.is(Status::Done)).map(String::from);
    }
    place(cfg, &mut out);
    out
}

fn ended(events: &[Event], sessions: &Sessions, sp: &Event) -> Option<Exit> {
    let agent = sp.str("agent")?;
    let logged = stamp::exit(events, agent);
    match sessions.seen(agent, sp.str("machine")) {
        Seen::Here(s) => match s.life {
            Life::Exited { code, last } => Some(Exit {
                code,
                cause: logged.map_or(Cause::of(code, last), |x| x.cause),
                note: s.note.clone(),
            }),
            Life::Running(_) => None,
        },
        _ => logged,
    }
}

/// The issue's active lane's health: one rule set for stall, never-started, dead and closed.
pub fn health(ctx: &Ctx, cfg: &Config, events: &[Event], sessions: &Sessions, s: &IssueState) -> Health {
    let Some((_, lane)) = s.active_lane() else {
        return Health::Unknown;
    };
    let Some(sp) = last_spawn(events, lane) else {
        return Health::Unknown;
    };
    let shut = |e: &Event| e.kind == "lane-closed" && crate::events::ends(e, sp) && e.ts >= sp.ts;
    let killed = |e: &Event| e.kind == "lane-killed" && e.str("agent") == Some(lane) && e.ts >= sp.ts;
    if (crate::events::of_kind(events, &["lane-closed", "lane-killed"]).into_iter()).any(|e| shut(e) || killed(e)) {
        return Health::Closed;
    }
    let live = match sessions.seen(lane, sp.str("machine")) {
        Seen::Unreachable => return Health::Unknown,
        Seen::Absent => return Health::Dead(stamp::exit(events, lane)),
        Seen::Here(x) => match x.status() {
            Some(st) => st,
            None => return Health::Dead(ended(events, sessions, sp)),
        },
    };
    let resent = |e: &&Event| e.kind == "start-resent" && e.str("agent") == Some(lane) && e.ts >= sp.ts;
    let start = events.iter().rfind(resent).map_or(sp.ts, |e| e.ts);
    if live == Status::Unreported {
        let due = ctx.now >= start + crate::stall::START_SECS;
        return if due {
            Health::NeverStarted { since: start }
        } else {
            Health::Fine
        };
    }
    let since = stamp::since(events, lane).unwrap_or(sp.ts).max(sp.ts);
    let turned = crate::waiting::turn(events, s).is_some();
    if live.quiet() && !turned && ctx.now >= since + cfg.limits.stall_minutes * 60 {
        return Health::Stalled { since };
    }
    Health::Fine
}

/// A session's fleet, issue and role from drover's labels on it, when its fleet is configured.
pub fn whose<'a>(cfg: &Config, s: &'a Session) -> Option<(&'a str, Option<u64>, Option<&'a str>)> {
    let fleet = s.labels.get("fleet").filter(|f| cfg.fleet.contains_key(*f))?;
    let issue = s.labels.get("issue").and_then(|i| i.parse().ok());
    Some((fleet, issue, s.labels.get("role").map(String::as_str)))
}

/// #1053: the running sessions labelled for a configured fleet that no `spawn`, adoption, conductor start or young `spawn-started` names.
pub fn orphans<'a>(ctx: &Ctx, cfg: &Config, events: &[Event], sessions: &'a Sessions) -> Vec<&'a Session> {
    let young = |e: &Event| e.kind == "spawn-started" && ctx.now.saturating_sub(e.ts) < crate::spawn::SPAWN_SECS;
    let kinds = ["spawn", "spawn-adopted", "spawn-started", "orchestrator-start"];
    let named = |e: &&Event| e.kind != "spawn-started" || young(e);
    let ours = crate::events::of_kind(events, &kinds).into_iter().filter(named);
    let ours: BTreeSet<&str> = ours.filter_map(|e| e.str("agent")).collect();
    let lone = |s: &&Session| !ours.contains(s.name.as_str()) && whose(cfg, s).is_some();
    sessions.live().filter(lone).collect()
}

fn place(cfg: &Config, all: &mut [Lane]) {
    let hosts = cfg.hosts.values().filter(|h| h.enabled).count() > 1;
    let main = cfg.hosts.iter().find(|h| h.1.machine.is_none()).map(|h| h.0.clone());
    for l in all {
        l.host = l.host.take().or_else(|| main.clone());
        l.place = if hosts { l.at() } else { String::new() };
    }
}

impl Lane {
    /// #609: its host as API v1 sends it: `local` with no `[hosts]`.
    pub fn at(&self) -> String {
        self.host.clone().unwrap_or_else(|| "local".into())
    }
    /// #630: its worktree's uncommitted files and unpushed commits, read on `r` when shown (never in a drive pass).
    pub fn unsaved(&self, r: &dyn Runner) -> Option<(u64, u64)> {
        let wt = self.worktree.as_deref().filter(|_| self.thread.is_none())?;
        crate::spawn::unsaved(&self.on(r), Path::new(wt))
    }
    /// #157: `r` on the lane's host.
    pub fn on<'a>(&'a self, r: &'a dyn Runner) -> crate::hosts::On<'a> {
        crate::hosts::On::new(r, self.machine.as_deref())
    }
    /// Its running session's name; None once it is gone.
    pub fn pane(&self) -> Option<&str> {
        self.session.as_ref().map(|s| s.name.as_str())
    }
    /// #1033: shed's note on its reported state (`running: cargo test (8m)`), masked.
    pub fn note(&self) -> Option<String> {
        self.session.as_ref()?.note.as_deref().map(mask)
    }
}

fn from_spawn(e: &Event, l: Lane) -> Lane {
    let s = |k: &str| e.str(k).map(String::from);
    Lane {
        fleet: Some(e.fleet.clone()),
        name: e.str("agent").unwrap_or_default().into(),
        issue: e.issue,
        role: s("role").unwrap_or_default(),
        harness: s("harness").unwrap_or_default(),
        model: s("model"),
        effort: s("effort"),
        port: e.data.get("port").and_then(Value::as_u64).map(|p| format!(":{p}")),
        since: e.ts,
        host: s("host").or(l.host),
        machine: s("machine").or(l.machine),
        worktree: s("worktree"),
        root: s("root"),
        runtime: s("runtime").unwrap_or_else(|| "shed".into()),
        ..l
    }
}

fn orchestrator(events: &[Event], e: &Event, l: Lane) -> Lane {
    let (id, mut l) = (e.str("thread").unwrap_or_default(), from_spawn(e, l));
    let t = crate::events::of_kind(events, &["thread-new"])
        .into_iter()
        .find(|t| t.str("thread") == Some(id));
    l.name = format!("{} · {id}", t.and_then(|t| t.str("title")).unwrap_or_default());
    (l.role, l.thread) = ("orchestrator".into(), Some(id.into()));
    if l.session.is_none() {
        l.state = LaneState::Asleep;
    }
    l
}

/// Section order on the page; any other state (unknown) counts as idle.
pub const SECTIONS: &[&str] = &["blocked", "working", "idle", "done", "parked", "dead"];

pub type Groups<'a> = Vec<(&'static str, Vec<(String, Vec<&'a Lane>)>)>;

/// Non-empty [`SECTIONS`], each split by fleet (alphabetical, Others last), newest `changed` first.
pub fn group(all: &[Lane], changed: impl Fn(&Lane) -> u64) -> Groups<'_> {
    let section = |l: &Lane| SECTIONS.iter().position(|s| *s == l.state.word()).unwrap_or(2);
    let mut v: Vec<&Lane> = all.iter().collect();
    v.sort_by_key(|l| {
        (
            section(l),
            l.fleet.is_none(),
            l.fleet.clone(),
            l.role != "orchestrator",
            std::cmp::Reverse(changed(l)),
        )
    });
    let mut out: Groups = vec![];
    for l in v {
        let f = (l.fleet.as_deref()).map_or("Others", |f| if f == "all" { "all repos" } else { f });
        let (s, f) = (SECTIONS.get(section(l)).copied().unwrap_or("idle"), f.to_string());
        if out.last().is_none_or(|x| x.0 != s) {
            out.push((s, vec![]));
        }
        let Some((_, subs)) = out.last_mut() else { continue };
        if subs.last().is_none_or(|x| x.0 != f) {
            subs.push((f, vec![]));
        }
        if let Some((_, ls)) = subs.last_mut() {
            ls.push(l);
        }
    }
    out
}

const HIDE: &str = "•••";
pub const PREFIXES: &[&str] = &["sk-", "ghp_", "gho_", "ghs_", "ghu_", "github_pat_", "xox", "xapp-"];

/// `l` with each value `secret` finds (`SECRET_IN_WIP`'s check) as `•••`.
pub fn redact(l: &str) -> String {
    let mut m = l.to_owned();
    while let Some(v) = secret(&m) {
        m = m.replace(v, HIDE);
    }
    m
}

/// The push guards' check (stricter than [`mask`]): the secret on line `l`, if any — a `PREFIXES`/AWS/Google token, a JWT, a PEM private key, or a high-entropy non-code value assigned to a `*KEY|TOKEN|SECRET|PASSWORD` name.
pub fn secret(l: &str) -> Option<&str> {
    let has = |s: &str, f: fn(&u8) -> bool| s.as_bytes().iter().any(f);
    let pre = |w: &str, p: &str| w.starts_with(p) && w.len() >= p.len() + 16 && has(&w[p.len()..], u8::is_ascii_digit);
    let b64 = |s: &str| !s.is_empty() && !has(s, |b| !b.is_ascii_alphanumeric() && !b"-_".contains(b));
    let jwt = |w: &str| w.starts_with("eyJ") && w.split('.').count() == 3 && w.split('.').all(b64);
    let mut words = l.split(|c: char| !c.is_ascii_alphanumeric() && !"-_.".contains(c));
    let pem = l.contains("-----BEGIN") && l.contains("PRIVATE KEY-----");
    let tok = words.find(|w| jwt(w) || PREFIXES.iter().chain(&["AKIA", "ASIA", "AIza"]).any(|p| pre(w, p)));
    let tg = l.split([' ', '"', '\'', '=', ',', '`', '/']).find_map(|w| {
        let (d, t) = w.split_once(":AA")?;
        let id = d.bytes().rev().take_while(u8::is_ascii_digit).count();
        (id > 7 && t.len() > 27).then(|| &w[d.len() - id..])
    });
    let named = |s: &str| {
        let up = s.split_whitespace().last().unwrap_or("").to_uppercase();
        ["KEY", "TOKEN", "SECRET", "PASSWORD"].iter().any(|k| up.contains(k))
    };
    let first = &l[..l.find(['=', ':']).unwrap_or(0)];
    tok.or(tg).or(pem.then_some(l.trim())).or_else(|| {
        l.match_indices(['=', ':']).find_map(|(i, _)| {
            let r = l[i + 1..].trim_start().trim_start_matches(['"', '\'']);
            let bearer = r.starts_with("Bearer ");
            let v = r
                .trim_start_matches("Bearer ")
                .split(['"', '\'', ' ', '\t'])
                .next()?
                .trim_end_matches([';', ',']);
            let f = |x: usize| f64::from(u32::try_from(x).unwrap_or(u32::MAX));
            let n = f(v.chars().count());
            let bits: f64 = v.chars().map(|c| -(f(v.matches(c).count()) / n).log2() / n).sum();
            let word = !has(v, u8::is_ascii_digit) || v.contains('_') && v == v.to_ascii_lowercase();
            let code = v.contains(['(', '{', ' ']) || v.contains("::") || v.starts_with(['/', '~', '.', '$']);
            ((named(&l[..i]) || named(first) || bearer) && n >= 20.0 && bits >= 3.5 && !word && !code).then_some(v)
        })
    })
}

/// Mask secret-looking runs: `PREFIXES` tokens, JWTs, 32+ char hex/base64, `*KEY=`/`*TOKEN=`/`*SECRET=` values.
pub fn mask(s: &str) -> String {
    let tok = |c: char| c.is_ascii_alphanumeric() || "-_+/=.".contains(c);
    let (mut out, mut rest, mut hide_next) = (String::new(), s, false);
    while let Some(i) = rest.find(tok) {
        out += &rest[..i];
        let n = rest[i..].find(|c| !tok(c)).unwrap_or(rest.len() - i);
        let w = &rest[i..i + n];
        let (word, after_eq) = if std::mem::take(&mut hide_next) {
            (HIDE.to_string(), false)
        } else {
            word(w)
        };
        out += &word;
        hide_next = after_eq && rest[i + n..].starts_with(['"', '\'']);
        rest = &rest[i + n..];
    }
    out + rest
}

fn word(w: &str) -> (String, bool) {
    if let Some((k, v)) = w.split_once('=') {
        let up = k.to_ascii_uppercase();
        if ["KEY", "TOKEN", "SECRET", "PASSWORD"].iter().any(|s| up.contains(s)) {
            return if v.is_empty() {
                (w.into(), true)
            } else {
                (format!("{k}={HIDE}"), false)
            };
        }
    }
    if let Some(p) = PREFIXES.iter().find(|p| w.starts_with(**p) && w.len() >= p.len() + 10) {
        return (format!("{p}{HIDE}"), false);
    }
    let jwt = w.starts_with("eyJ") && w.matches('.').count() >= 2;
    let long = |seg: &str| {
        seg.len() >= 32 && seg.chars().any(|c| c.is_ascii_digit()) && seg.chars().any(|c| c.is_ascii_alphabetic())
    };
    if jwt || w.split(['.', '/', '-']).any(long) {
        return (HIDE.into(), false);
    }
    (w.into(), false)
}

/// The text's last `n` non-empty lines, masked.
pub fn tail(screen: &str, n: usize) -> Vec<String> {
    let lines: Vec<&str> = screen.lines().filter(|l| !l.trim().is_empty()).collect();
    lines
        .iter()
        .skip(lines.len().saturating_sub(n))
        .map(|l| mask(l.trim_end()))
        .collect()
}

/// #611: each enabled host's shed sessions that are no lane or conductor, or `{host, error}` for a host whose shed fails.
pub fn others(cfg: &Config, r: &dyn Runner, events: &[Event]) -> Vec<Value> {
    let ours = crate::events::of_kind(events, &["spawn", "orchestrator-start"]).into_iter();
    let ours: BTreeSet<&str> = ours.filter_map(|e| e.str("agent")).collect();
    let mut out = vec![];
    for (host, h) in crate::hosts::cards(cfg, r) {
        let on = crate::hosts::On::new(r, h.and_then(|h| h.machine.as_deref()));
        if on.shed().is_none() || h.is_some_and(|h| !h.enabled) {
            continue;
        }
        match crate::session::on(&on) {
            Ok(all) => out.extend(all.into_iter().filter(|t| !ours.contains(t.name.as_str()) && !t.lane()).map(|t| {
                let state = match t.status() {
                    Some(Status::Unreported) => "running",
                    Some(s) => s.word(),
                    None => "exited",
                };
                let last = crate::session::transcript::read(&on, &t.name, None, Some(50)).ok().flatten();
                json!({"name": t.name, "host": host, "runtime": if t.acp { "acp" } else { "shed" },
                    "labels": t.labels, "state": state, "last": last.as_ref().map(crate::session::transcript::last_line).unwrap_or_default()})
            })),
            Err(e) => out.push(json!({"host": host, "error": format!("{e:#}")})),
        }
    }
    out
}
