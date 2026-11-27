use crate::config::{expand, Config, Role};
use crate::events::{self, emit, working, Event, Json};
use crate::gate::refused;
use crate::runner::Runner;
use crate::session::{Session, Status};
use crate::spawn::Plan;
use crate::{providers, term, Ctx};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::Write;

mod brief;
pub mod chat;

/// How many times `of` ran (`WALKS`, the events it looked at).
pub static SCANS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The events thread lookups (`of`) looked at: the whole log's off the index, the thread's own on it.
pub static WALKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The thread's events: its messages, asks, starts and sleeps. a cached log's from its thread index.
pub fn of<'a>(events: &'a [Event], id: &str) -> impl DoubleEndedIterator<Item = &'a Event> + use<'a> {
    SCANS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let at = events::cached(events).map(|l| l.threads.of(id).to_vec());
    WALKS.fetch_add(
        at.as_ref().map_or(events.len(), Vec::len),
        std::sync::atomic::Ordering::Relaxed,
    );
    let all = at.is_none().then(|| {
        let id = id.to_string();
        events.iter().filter(move |e| e.str("thread") == Some(id.as_str()))
    });
    let at = at.into_iter().flatten().filter_map(move |i| events.get(i));
    at.chain(all.into_iter().flatten())
        .filter(move |e| !events::repeat(events, e))
}

/// Every thread's `thread-new`, oldest first.
pub fn threads(events: &[Event]) -> Vec<&Event> {
    match events::cached(events) {
        Some(l) => l.threads.new.iter().filter_map(|i| events.get(*i)).collect(),
        None => events.iter().filter(|e| e.kind == "thread-new").collect(),
    }
}

/// The thread's `thread-new` (`NO_SUCH_THREAD`).
pub fn find<'a>(events: &'a [Event], id: &str) -> Result<&'a Event> {
    match of(events, id).find(|e| e.kind == "thread-new") {
        Some(t) => Ok(t),
        None => refused(
            "NO_SUCH_THREAD",
            format!("no thread {id}; `drover thread new` starts one"),
        ),
    }
}

pub(crate) fn link_of<'a>(events: &'a [Event], id: &str) -> Option<&'a Event> {
    let link = |e: &&Event| matches!(e.kind.as_str(), "orchestrator-attached" | "orchestrator-detached");
    events.iter().rfind(|e| link(e) && e.str("thread") == Some(id))
}

/// Cooee thread `id`'s `orchestrator-attached` while the link holds (not detached, not archived since).
pub fn attached<'a>(events: &'a [Event], id: &str) -> Option<&'a Event> {
    let since = |a: &Event| of(events, id).any(|e| e.kind == "thread-archive" && e.seq > a.seq);
    link_of(events, id).filter(|e| e.kind == "orchestrator-attached" && !since(e))
}

/// Thread `id`'s fleet: an attached cooee thread's, else drover's own thread's (`NO_SUCH_THREAD`).
pub fn fleet_of<'a>(events: &'a [Event], id: &str) -> Result<&'a str> {
    Ok(&attached(events, id).map_or_else(|| find(events, id), Ok)?.fleet)
}

fn hold(ctx: &Ctx, cfg: &Config, fleet: &str, id: &str, mut extra: Value) -> Result<Event> {
    let ok = |c: char| c.is_ascii_alphanumeric() || "-_.:".contains(c);
    if id.is_empty() || id.len() > 128 || !id.chars().all(ok) {
        return refused("BAD_VALUE", "thread: a cooee thread id (letters, digits, `-_.:`)");
    }
    if find(&working(&ctx.state)?, id).is_ok() {
        return refused("BAD_VALUE", format!("{id} is drover's own thread"));
    }
    extra.put("thread", json!(id));
    let t = Event::new(ctx.now, fleet, None, "orchestrator-attached", extra);
    emit(&ctx.state, cfg, t.clone())?;
    Ok(t)
}

/// `POST /api/v1/orchestrator-attach` and `drover attach`: chat `id` on `fleet`, a repo, `all` or `none`; its brief, settings and env.
pub fn attach(ctx: &Ctx, cfg: &Config, fleet: &str, id: &str) -> Result<Value> {
    let any = matches!(fleet, "all" | "none");
    let range = if any {
        None
    } else {
        cfg.fleet(fleet)?.port_range.as_deref()
    };
    let (_, role) = defaults(cfg, if any { "all" } else { fleet })?;
    let t = hold(ctx, cfg, fleet, id, json!({}))?;
    let p = role.profile_or("orchestrator");
    let acp = providers::get(&role.harness).acp().unwrap_or(&role.harness);
    let why = crate::spawn::unguarded(ctx, (&p, "orchestrator", &role.harness), acp);
    let port = crate::spawn::lane_port(range, &format!("{fleet}/orchestrator-{id}"), &[]);
    let settings = providers::profiles_root(ctx).join(&p).join("settings.json");
    let env = json!({"DROVER_LANE": format!("{fleet}:-:orchestrator"), "DROVER_THREAD": id,
        "DROVER_PORT": port.to_string()});
    let message = format!("{id} is {fleet}'s orchestrator thread");
    Ok(
        json!({"ok": true, "code": "OK", "message": message, "brief": brief::thread(cfg, &t, fleet),
        "settings": settings, "env": env, "guarded": why.is_empty(), "why": why}),
    )
}

/// `POST /api/v1/thread/<id>/said`: an attached chat's owner line, logged once per `item` for `answer --typed`.
pub fn said(ctx: &Ctx, cfg: &Config, id: &str, (item, from, text): (&str, &str, &str)) -> Result<String> {
    if from != "owner" || item.is_empty() {
        return refused(
            "BAD_VALUE",
            "said: an `id` and `from: owner`; the agent's lines stay in the client",
        );
    }
    text_ok(text)?;
    let events = working(&ctx.state)?;
    let Some(t) = attached(&events, id) else {
        return refused("NO_SUCH_THREAD", format!("{id} is no attached chat"));
    };
    if of(&events, id).any(|e| e.kind == "thread-msg" && e.str("item") == Some(item)) {
        return Ok(format!("{item} is logged already"));
    }
    let d = json!({"thread": id, "from": "owner", "text": text, "item": item, "via": "client"});
    emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, "thread-msg", d))?;
    Ok(format!("{item} logged in {id}"))
}

/// `POST /api/v1/orchestrator-detach`: the link ends (`orchestrator-detached`); `NO_SUCH_THREAD` unless attached.
pub fn detach(ctx: &Ctx, cfg: &Config, fleet: &str, id: &str) -> Result<String> {
    if attached(&working(&ctx.state)?, id).is_none_or(|e| e.fleet != fleet) {
        return refused("NO_SUCH_THREAD", format!("no cooee thread {id} attached to {fleet}"));
    }
    let d = json!({"thread": id});
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, fleet, None, "orchestrator-detached", d),
    )?;
    Ok(format!("{id} detached from {fleet}"))
}

/// The old role name a lane started before still carries (`DROVER_LANE`), read for one release.
pub fn is_role(role: &str) -> bool {
    matches!(role, "orchestrator" | "conductor")
}

/// A `by` in the old spelling (`conductor-<thread>`: a flag, a pre-event) as `orchestrator-<thread>`.
pub fn by(b: &str) -> String {
    b.strip_prefix("conductor-")
        .map_or_else(|| b.into(), |t| format!("orchestrator-{t}"))
}

/// In an orchestrator lane, its `DROVER_ORCHESTRATOR` id; every event it logs names it (`db::insert`).
pub fn acting() -> Option<String> {
    let lane = crate::config::env("DROVER_LANE")?;
    is_role(lane.split(':').nth(2)?).then(|| crate::config::env("DROVER_ORCHESTRATOR"))?
}

/// Who runs this command, the `by` its events carry: `drive`, `orchestrator-<id>`, `lane` or `owner`.
pub fn actor() -> String {
    match acting() {
        _ if crate::drive::in_act() => "drive".into(),
        Some(id) => format!("orchestrator-{id}"),
        None if crate::config::env("DROVER_LANE").is_some() => "lane".into(),
        None => "owner".into(),
    }
}

/// The orchestrator a thread from before (none named) belongs to, the one `[orchestrator]` itself names.
pub const DEFAULT: &str = "default";

/// Orchestrator `id`'s name, what every label shows (code keys off the role and the id only).
pub fn name<'a>(cfg: &'a Config, id: &str) -> &'a str {
    let o = &cfg.orchestrator;
    let n = if id == DEFAULT {
        o.name.as_deref()
    } else {
        o.more.get(id).and_then(|m| m.name.as_deref())
    };
    n.unwrap_or("Orchestrator")
}

/// `by` as a label: an orchestrator's (`orchestrator-<thread>`) with its name in front.
pub fn label(cfg: &Config, events: &[Event], by: &str) -> String {
    let t = by.strip_prefix("orchestrator-");
    t.map_or_else(|| by.into(), |t| format!("{} ({by})", name_of(cfg, events, t)))
}

/// Thread `t`'s orchestrator id (its `thread-new`'s).
pub fn id_of(t: &Event) -> &str {
    t.str("orchestrator").unwrap_or(DEFAULT)
}

/// The name of thread `id`'s orchestrator, or of orchestrator `id` itself.
pub fn name_of<'a>(cfg: &'a Config, events: &[Event], id: &str) -> &'a str {
    name(cfg, find(events, id).map_or(id, id_of))
}

fn assign(cfg: &Config, repo: &str, want: Option<&str>) -> Result<String> {
    let covers = |o: &crate::config::Orchestrator| o.fleets.iter().any(|f| f == repo || f == "all");
    let first = || cfg.orchestrator.more.iter().find(|o| covers(o.1)).map(|o| o.0.as_str());
    match want {
        None => Ok(first().unwrap_or(DEFAULT).into()),
        Some(w) if w == DEFAULT || cfg.orchestrator.more.get(w).is_some_and(covers) => Ok(w.into()),
        Some(w) => refused(
            "BAD_VALUE",
            format!("orchestrator {w:?} is not one whose fleets cover {repo}"),
        ),
    }
}

/// `drover thread new <fleet|all> <title> [--harness <h>]`: its id. the thread keeps its harness (`harness`, else the default's) for good.
pub fn new(ctx: &Ctx, cfg: &Config, repo: &str, title: &str, (harness, req): Opt2) -> Result<String> {
    make(ctx, cfg, repo, title, (harness, None), req)
}

/// `new`, the thread talking to orchestrator `orch` (`assign`).
pub fn new_with(ctx: &Ctx, cfg: &Config, repo: &str, title: &str, o: Opt2) -> Result<String> {
    make(ctx, cfg, repo, title, o, None)
}

/// The thread a `new` with request id `req` in `repo` made under `[limits] thread_resend_minutes` ago.
pub fn resent<'a>(ctx: &Ctx, cfg: &Config, events: &'a [Event], repo: &str, req: &str) -> Option<&'a str> {
    let since = ctx.now.saturating_sub(cfg.limits.thread_resend_minutes * 60);
    let made = |e: &&Event| e.kind == "thread-new" && e.fleet == repo && e.ts >= since && e.str("req") == Some(req);
    events
        .iter()
        .rev()
        .find(made)
        .and_then(|e| e.str("thread"))
        .filter(|_| !req.is_empty())
}

fn make(ctx: &Ctx, cfg: &Config, repo: &str, title: &str, (harness, orch): Opt2, req: Option<&str>) -> Result<String> {
    if repo != "all" {
        cfg.fleet(repo)?;
    }
    title_ok(title)?;
    let orch = assign(cfg, repo, orch)?;
    if let Some(h) = harness.filter(|h| !providers::lane_names().contains(h)) {
        return refused(
            "BAD_VALUE",
            format!("harness {h:?}: one of {:?}", providers::lane_names()),
        );
    }
    let h = harness
        .map(String::from)
        .or_else(|| defaults(cfg, repo).ok().map(|r| r.1.harness));
    let events = working(&ctx.state)?;
    let id = format!("t{}", events.iter().filter(|e| e.kind == "thread-new").count() + 1);
    let mut d = json!({"thread": id, "title": title, "harness": h});
    if orch != DEFAULT {
        d.put("orchestrator", json!(orch));
    }
    if let Some(r) = req.filter(|r| !r.is_empty()) {
        d.put("req", json!(r));
    }
    emit(&ctx.state, cfg, Event::new(ctx.now, repo, None, "thread-new", d))?;
    if role(cfg, repo).is_ok() {
        let waiting = |a: &&Event| {
            a.kind == "needs-decision"
                && a.fleet == repo
                && a.data.get("lane_ask").is_some()
                && crate::thread::route::routed_to(&events, a).is_none()
                && events::closed(&events, a).is_none()
        };
        for a in events.iter().filter(waiting) {
            crate::thread::route::route_row(ctx, cfg, &id, a)?;
        }
    }
    Ok(id)
}

type Opt2<'a> = (Option<&'a str>, Option<&'a str>);

fn title_ok(title: &str) -> Result<()> {
    if title.trim().is_empty() || title.chars().any(char::is_control) {
        return refused("BAD_VALUE", "a thread has a one-line title");
    }
    Ok(())
}

/// The thread's title: its last `thread title`, else the one it started with.
pub fn title<'a>(events: &'a [Event], id: &str) -> &'a str {
    let t = of(events, id).rfind(|e| matches!(e.kind.as_str(), "thread-new" | "thread-title"));
    t.and_then(|e| e.str("title")).unwrap_or_default()
}

/// `drover thread title <id> <title>`: the orchestrator's short summary title, once after its first reply.
pub fn retitle(ctx: &Ctx, cfg: &Config, id: &str, title: &str) -> Result<()> {
    title_ok(title)?;
    let t = find(&working(&ctx.state)?, id)?.clone();
    let d = json!({"thread": id, "title": title});
    emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, "thread-title", d))
}

/// The thread is archived (its last archive or restore an archive).
pub fn archived(events: &[Event], id: &str) -> bool {
    let a = of(events, id).rfind(|e| matches!(e.kind.as_str(), "thread-archive" | "thread-restore"));
    a.is_some_and(|e| e.kind == "thread-archive")
}

/// `thread archive` (`on`) closes the thread's live conductor (`conductor-sleep`; its folder, uploads and log stay) and folds the thread; `thread restore` unfolds it, and its next message resumes the same session.
pub fn archive(ctx: &Ctx, cfg: &Config, r: &dyn Runner, id: &str, on: bool) -> Result<()> {
    let events = working(&ctx.state)?;
    let t = link_of(&events, id).filter(|e| e.kind == "orchestrator-attached");
    let t = t.map_or_else(|| find(&events, id), Ok)?;
    let last = of(&events, id).rfind(|e| e.kind.starts_with("orchestrator-"));
    if let Some(s) = last.filter(|s| on && s.kind == "orchestrator-start") {
        if crate::session::live_on(r)?
            .iter()
            .any(|a| Some(a.name.as_str()) == s.str("agent"))
        {
            close(ctx, r, s)?;
        }
        let d = json!({"thread": id, "agent": s.str("agent"), "why": "archived"});
        emit(
            &ctx.state,
            cfg,
            Event::new(ctx.now, &t.fleet, None, "orchestrator-sleep", d),
        )?;
    }
    let kind = if on { "thread-archive" } else { "thread-restore" };
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, &t.fleet, None, kind, json!({"thread": id})),
    )
}

pub fn files_ok(files: &[(String, Vec<u8>)]) -> Result<()> {
    if files.len() > 10 {
        return refused("BAD_VALUE", "at most 10 files a message");
    }
    if files
        .iter()
        .any(|(n, b)| n.trim_start_matches('.').is_empty() || b.len() > 10 << 20)
    {
        return refused("BAD_VALUE", "a file has a name and is at most 10 MB");
    }
    Ok(())
}

/// A message's files (name, bytes) into thread `id`'s `uploads/`, as the thread logs them: at most 10, each named and at most 10 MB (checked before any is written); a name repeated in the message gets its place in front.
pub fn upload(ctx: &Ctx, cfg: &Config, id: &str, files: &[(String, Vec<u8>)]) -> Result<Vec<Value>> {
    files_ok(files)?;
    let dir = dir(ctx, cfg, id).join("uploads");
    std::fs::create_dir_all(&dir)?;
    let mut seen = std::collections::BTreeSet::new();
    let mut out = vec![];
    for (i, (name, bytes)) in files.iter().enumerate() {
        let name = if seen.insert(name) {
            name.clone()
        } else {
            format!("{}-{name}", i + 1)
        };
        let path = dir.join(format!("{}-{name}", ctx.now));
        std::fs::write(&path, bytes)?;
        out.push(json!({"name": name, "path": path, "size": bytes.len()}));
    }
    Ok(out)
}

/// `thread handoff`: to thread `to`, or to repo `to`'s (or `all`'s) desk thread, at once, linked; wakes its orchestrator as `from`.
pub fn handoff(ctx: &Ctx, cfg: &Config, r: &dyn Runner, [from, to, title, text]: [&str; 4]) -> Result<String> {
    text_ok(text)?;
    let events = working(&ctx.state)?;
    find(&events, from)?;
    let fwd = forwards(&events, from);
    let to = if is_id(to) {
        role(cfg, &find(&events, to)?.fleet)?;
        to.to_string()
    } else {
        role(cfg, to)?;
        title_ok(title)?;
        desk(ctx, cfg, &events, to)?
    };
    if to == from {
        return refused("BAD_VALUE", "a thread hands off to another thread");
    }
    send(ctx, cfg, r, (from, &to), (title, text), (fwd, None))
}

fn desk(ctx: &Ctx, cfg: &Config, events: &[Event], repo: &str) -> Result<String> {
    if let Some(d) = crate::thread::Registry::read(events).desk(repo) {
        return Ok(d.into());
    }
    let id = new(ctx, cfg, repo, "Desk", (None, None))?;
    let d = json!({"thread": id, "profile": repo, "desk": true});
    emit(&ctx.state, cfg, Event::new(ctx.now, repo, None, "thread-facts", d))?;
    Ok(id)
}

/// The handoff that moved handoff `seq` on, if one did.
pub fn moved(events: &[Event], seq: u64) -> Option<&Event> {
    events
        .iter()
        .find(|e| e.kind == "thread-handoff" && e.val("moves").as_u64() == Some(seq))
}

/// [move] on handoff `seq`: delivered again to thread `to` from its sender; its first copy is marked forwarded and that thread's orchestrator told to drop it.
pub fn move_handoff(ctx: &Ctx, cfg: &Config, r: &dyn Runner, seq: u64, to: &str) -> Result<String> {
    let events = working(&ctx.state)?;
    let Some(h) = events.iter().find(|e| e.seq == seq && e.kind == "thread-handoff") else {
        return refused("NO_SUCH_HANDOFF", format!("no handoff {seq}"));
    };
    let s = |k: &str| h.str(k).unwrap_or_default();
    if let Some(m) = moved(&events, seq) {
        let at = m.str("to").unwrap_or_default();
        if at == to {
            return Ok(at.into());
        }
        return refused(
            "BAD_VALUE",
            format!("handoff {seq} moved to {at} already; move that one"),
        );
    }
    if to == s("to") || to == s("from") {
        return refused(
            "BAD_VALUE",
            format!("handoff {seq} is from {} and in {}", s("from"), s("to")),
        );
    }
    role(cfg, &find(&events, to)?.fleet)?;
    let first = |m: &&Event| m.kind == "thread-msg" && m.seq > seq && m.str("from") == h.str("from");
    let copy = of(&events, s("to")).find(first).map(|m| m.seq);
    let sent = send(ctx, cfg, r, (s("from"), to), (s("title"), s("text")), (copy, Some(seq)))?;
    let line = format!("handoff \"{}\" moved to {to}: don't act on it here", s("title"));
    crate::thread::route::tell(ctx, cfg, r, s("to"), &line);
    Ok(sent)
}

fn send(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    (from, to): (&str, &str),
    (title, text): (&str, &str),
    (fwd, moves): (Option<u64>, Option<u64>),
) -> Result<String> {
    let events = working(&ctx.state)?;
    let target = format!("{} · {to}", self::title(&events, to));
    let d = json!({"thread": from, "from": from, "to": to, "title": title, "text": text, "target": target,
        "forwards": fwd, "moves": moves});
    let t = find(&events, from)?.fleet.clone();
    emit(&ctx.state, cfg, Event::new(ctx.now, &t, None, "thread-handoff", d))?;
    say(
        ctx,
        cfg,
        r,
        (to, &format!("handoff to {target}: {text}"), &[]),
        (from, false),
    )?;
    Ok(to.into())
}

fn forwards(events: &[Event], from: &str) -> Option<u64> {
    let owner = |e: &Event| {
        e.kind == "thread-msg"
            && e.str("from")
                .is_some_and(|f| !matches!(f, "orchestrator" | "system") && !is_id(f))
    };
    let sent = forwarded(events);
    let got = |e: &&Event| e.kind == "thread-msg" && e.str("from").is_some_and(is_id) && !sent.contains(&e.seq);
    of(events, from)
        .rev()
        .take_while(|e| !owner(e))
        .find(got)
        .map(|e| e.seq)
}

/// The seqs of the received handoff rows their thread sent on (a later handoff from that thread `forwards` them). one pass over the log; a row's check is a lookup.
pub fn forwarded(events: &[Event]) -> std::collections::BTreeSet<u64> {
    if let Some(l) = events::cached(events) {
        return l.threads.forwards.clone();
    }
    events
        .iter()
        .filter_map(|e| e.data.get("forwards").and_then(Value::as_u64))
        .collect()
}

/// A handoff row's text: its title and the target as `<title> · <id>`.
pub fn handoff_text(e: &Event) -> String {
    let title = e.str("title").unwrap_or_default();
    e.str("target").map_or(title.into(), |t| format!("{title} → {t}"))
}

/// An `ask-owner` row's lane ask.
pub fn lane_of<'a>(events: &'a [Event], e: &Event) -> Option<&'a Event> {
    events::ask_event(events, "needs-decision", &e.fleet, e.str("id").unwrap_or_default())
}

/// `s` is a drover thread id (`t<n>`).
pub fn is_id(s: &str) -> bool {
    s.len() > 1 && s.starts_with('t') && s[1..].bytes().all(|b| b.is_ascii_digit())
}

/// A linked row's chip: `from t5` (a message or flow-back row from t5) or `continued in t6` (a handoff).
pub fn link(e: &Event) -> Option<(&'static str, &str)> {
    match (e.kind.as_str(), e.str("from")) {
        ("thread-handoff", _) => Some(("continued in", e.str("to")?)),
        ("thread-msg", Some("system")) => Some(("from", e.str("via")?)),
        ("thread-msg", Some(f)) if is_id(f) => Some(("from", f)),
        _ => None,
    }
}

pub fn text_ok(text: &str) -> Result<()> {
    if text.trim().is_empty() || text.chars().count() > 20_000 {
        return refused("BAD_VALUE", "a message is non-empty text, at most 20,000 chars");
    }
    Ok(())
}

pub(crate) fn log(ctx: &Ctx, cfg: &Config, t: &Event, from: &str, (text, files): (&str, &[Value])) -> Result<()> {
    let mut d = json!({"thread": t.str("thread"), "from": from, "text": text});
    if !files.is_empty() {
        d.put("files", json!(files));
    }
    emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, "thread-msg", d))
}

/// The thread's conductor's working folder, one per thread: `<state dir>/conductor/<thread>/`, or `<[conductor] dir>/<thread>/`.
pub fn dir(ctx: &Ctx, cfg: &Config, id: &str) -> std::path::PathBuf {
    let (new, old) = (
        ctx.state.with_file_name("orchestrator"),
        ctx.state.with_file_name("conductor"),
    );
    let own = if !new.exists() && old.exists() { old } else { new };
    cfg.orchestrator.dir.as_deref().map_or(own, expand).join(id)
}

/// `p` resolved, if it lies in thread `id`'s orchestrator folder: the only files a thread shows or serves.
pub fn inside(ctx: &Ctx, cfg: &Config, id: &str, p: &std::path::Path) -> Option<std::path::PathBuf> {
    let root = dir(ctx, cfg, id).canonicalize().ok()?;
    p.canonicalize().ok().filter(|p| p.starts_with(root))
}

/// A file a message carries, as the thread logs it: its name, path in the orchestrator's folder and size.
pub fn file(ctx: &Ctx, cfg: &Config, t: &Event, p: &std::path::Path) -> Result<Value> {
    let id = t.str("thread").unwrap_or_default();
    let p = inside(ctx, cfg, id, p).map_or_else(|| copy_in(ctx, cfg, t, p), Ok)?;
    let size = std::fs::metadata(&p).with_context(|| p.display().to_string())?.len();
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
    Ok(json!({"name": name, "path": p, "size": size}))
}

fn copy_in(ctx: &Ctx, cfg: &Config, t: &Event, p: &std::path::Path) -> Result<std::path::PathBuf> {
    let id = t.str("thread").unwrap_or_default();
    let roots = (cfg.fleet.get(&t.fleet).into_iter())
        .flat_map(|f| [ctx.worktrees.join(f.repo_name()), expand(&f.local)])
        .filter_map(|r| r.canonicalize().ok());
    let from = (p.canonicalize().ok()).filter(|f| f.is_file() && roots.clone().any(|r| f.starts_with(r)));
    let (Some(from), Some(name)) = (from, p.file_name()) else {
        bail!("{}: not in the thread folder, a worktree or the checkout", p.display());
    };
    let to = dir(ctx, cfg, id).join("files");
    std::fs::create_dir_all(&to)?;
    let to = to.join(name);
    std::fs::remove_file(&to).ok();
    std::fs::copy(&from, &to).with_context(|| from.display().to_string())?;
    to.canonicalize().map_err(Into::into)
}

/// `drover thread reply [--file <path>]…`: the orchestrator's message to the owner, its files shown inline.
pub fn reply(ctx: &Ctx, cfg: &Config, id: &str, text: &str, files: &[std::path::PathBuf]) -> Result<()> {
    text_ok(text)?;
    let events = working(&ctx.state)?;
    let t = find(&events, id)?;
    let files = files.iter().map(|p| file(ctx, cfg, t, p)).collect::<Result<Vec<_>>>()?;
    log(ctx, cfg, t, "orchestrator", (text, &files))
}

/// `drover thread say` (and a thread ask's answer, `raw`: typed as is; the board's, with `files`): log `from`'s message, then type it into the thread's conductor, starting or resuming it first; the agent it went to.
pub fn say(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    (id, text, files): (&str, &str, &[Value]),
    (from, raw): (&str, bool),
) -> Result<String> {
    let job = post(ctx, cfg, (id, text, files), (from, raw))?;
    let Some((t, line)) = LATER.with_borrow_mut(|l| match l {
        Some(l) => {
            l.push(job);
            None
        }
        None => Some(job),
    }) else {
        return Ok(id.into());
    };
    deliver(ctx, cfg, r, &t, &line)
}

thread_local! {
    static LATER: std::cell::RefCell<Option<Vec<(Event, String)>>> = const { std::cell::RefCell::new(None) };
}

/// `f` with every `say` in it logging only (a board request never waits on a conductor start); the deliveries it left, for the caller to type in once its reply is out.
pub fn deferred<T>(f: impl FnOnce() -> T) -> (T, Vec<(Event, String)>) {
    LATER.set(Some(vec![]));
    let out = f();
    (out, LATER.take().unwrap_or_default())
}

/// `say`'s first half: log the message; the thread and the line `deliver` types.
pub fn post(
    ctx: &Ctx,
    cfg: &Config,
    (id, text, files): (&str, &str, &[Value]),
    (from, raw): (&str, bool),
) -> Result<(Event, String)> {
    text_ok(text)?;
    let t = find(&working(&ctx.state)?, id)?.clone();
    log(ctx, cfg, &t, from, (text, files))?;
    let path = |f: &Value| format!(" (attached: {})", f["path"].as_str().unwrap_or_default());
    let paths: String = files.iter().map(path).collect();
    let line = if raw {
        text.into()
    } else {
        format!("[{id}] {from}: {text}{paths}")
    };
    Ok((t, line))
}

/// `say`'s second half: type `line` into thread `t`'s orchestrator, starting or resuming it first.
pub fn deliver(ctx: &Ctx, cfg: &Config, r: &dyn Runner, t: &Event, line: &str) -> Result<String> {
    let id = t.str("thread").unwrap_or_default();
    let events = working(&ctx.state)?;
    if attached(&events, id).is_some() {
        return Ok(id.into());
    }
    let cur = thread_role(cfg, &events, t)?.1;
    let ms = harness_models(cfg, &cur.harness);
    if let Some(m) = cur.model.filter(|m| !ms.is_empty() && !ms.contains(&m.as_str())) {
        let text = format!("model {m} is not one of {ms:?}");
        return log(ctx, cfg, t, "system", (&text, &[])).and_then(|()| refused("BAD_VALUE", text));
    }
    let dir = dir(ctx, cfg, id);
    std::fs::create_dir_all(&dir)?;
    let lock = std::fs::File::create(dir.join(".deliver.lock"))?;
    let wait = 2 * START_SECS;
    for i in 0..=wait {
        match lock.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) if i < wait => r.sleep(1),
            Err(std::fs::TryLockError::WouldBlock) => {
                bail!("{id}'s previous message is still being delivered after {wait} s")
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
        }
    }
    once(ctx, cfg, r, t, line).or_else(|e| match e.downcast_ref::<crate::gate::Refusal>() {
        Some(_) => Err(e),
        None => once(ctx, cfg, r, t, line),
    })
}

const START_SECS: u64 = 120;

fn once(ctx: &Ctx, cfg: &Config, r: &dyn Runner, t: &Event, line: &str) -> Result<String> {
    let id = t.str("thread").unwrap_or_default();
    let events = working(&ctx.state)?;
    let last = of(&events, id).rfind(|e| e.kind.starts_with("orchestrator-"));
    let own = format!("orchestrator-{id}");
    let agent = last.and_then(|s| s.str("agent")).unwrap_or(&own);
    let found = crate::session::get(r, agent)?;
    if let Some(ended) = found.as_ref().filter(|a| a.status().is_none_or(|s| s == Status::Done)) {
        let was = ended.status().map_or("exited", Status::word);
        crate::session::transcript::archive(r, ctx, agent).ok();
        let w = r.shed().context(crate::config::NO_RUNTIME)?.wire;
        let target = json!(shed_proto::Target { name: agent.into() });
        crate::shed::call(r, &w, shed_proto::Method::Kill, target.clone()).ok();
        crate::shed::call(r, &w, shed_proto::Method::Rm, target)?;
        return start(ctx, cfg, r, (t, Some(was)), &events, line);
    }
    let agents: Vec<Session> = found.into_iter().collect();
    let begun = of(&events, id).rfind(|e| e.kind == "orchestrator-start");
    let picked = begun.is_some_and(|s| of(&events, id).any(|e| e.kind == "thread-model" && e.seq > s.seq));
    if last.is_some_and(|l| l.kind != "orchestrator-start") && !agents.is_empty() && !picked {
        let mut was = begun.map(|s| s.data.clone()).unwrap_or_default();
        was.remove("replaced");
        let mut d = Value::Object(was);
        d.put("thread", json!(id))
            .put("agent", json!(agent))
            .put("resumed", json!(true));
        started(ctx, cfg, t, d)?;
        return once(ctx, cfg, r, t, line);
    }
    let awake = last.filter(|s| s.kind == "orchestrator-start" && !agents.is_empty());
    let stuck = awake.and(hung(ctx, cfg, &events, &agents, id));
    let Some(s) = awake.filter(|_| !picked && stuck.is_none()) else {
        match (awake, stuck) {
            (Some(s), Some((_, quiet))) => end(ctx, cfg, r, (t, s), json!({"why": "hung", "quiet_secs": quiet}))?,
            (Some(s), None) => {
                close(ctx, r, s)?;
                let d = json!({"thread": id, "agent": s.str("agent"), "why": "model"});
                emit(
                    &ctx.state,
                    cfg,
                    Event::new(ctx.now, &t.fleet, None, "orchestrator-sleep", d),
                )?;
            }
            (None, _) if !agents.is_empty() => term::close(ctx, r, agent).map(drop)?,
            _ => {}
        }
        return start(ctx, cfg, r, (t, None), &working(&ctx.state)?, line);
    };
    let agent = s.str("agent").unwrap_or_default();
    crate::shed::cli(r, &["send", agent, line])?;
    held(ctx, cfg, r, t, &events, agents.first())?;
    Ok(agent.into())
}

/// Thread `id`'s awake ACP conductor reporting `working` with no CPU and no output for `stall_minutes`: its start and quiet seconds.
pub fn hung<'a>(
    ctx: &Ctx,
    cfg: &Config,
    events: &'a [Event],
    agents: &[Session],
    id: &str,
) -> Option<(&'a Event, u64)> {
    let last = of(events, id).rfind(|e| e.kind.starts_with("orchestrator-"));
    let s = last.filter(|e| e.kind == "orchestrator-start")?;
    let a = agents.iter().find(|a| Some(a.name.as_str()) == s.str("agent"))?;
    let out = crate::session::pulse::of(ctx, &a.name).map_or(0, |p| p.heard);
    let quiet = ctx.now.saturating_sub(out.max(s.ts));
    let idle = a.cpu_pct.is_some_and(|c| c < crate::stall::BUSY_CPU_PCT);
    (a.busy() && idle && quiet >= cfg.limits.stall_minutes * 60).then_some((s, quiet))
}

fn end(ctx: &Ctx, cfg: &Config, r: &dyn Runner, (t, s): (&Event, &Event), mut d: Value) -> Result<()> {
    let agent = s.str("agent").unwrap_or_default();
    crate::shed::cli(r, &["cancel", agent]).ok();
    crate::shed::cli(r, &["kill", agent])?;
    d.put("thread", json!(t.str("thread"))).put("agent", json!(agent));
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, &t.fleet, None, "orchestrator-sleep", d),
    )
}

/// `drover stop <thread>`: end its conductor (its ACP session cancelled and killed); logs `stop`.
pub fn stop(ctx: &Ctx, cfg: &Config, r: &dyn Runner, id: &str) -> Result<String> {
    let events = working(&ctx.state)?;
    let t = find(&events, id)?;
    let last = of(&events, id).rfind(|e| e.kind.starts_with("orchestrator-"));
    let Some(s) = last.filter(|e| e.kind == "orchestrator-start") else {
        return refused("NO_SUCH_AGENT", format!("thread {id}'s orchestrator is asleep"));
    };
    let agent = s.str("agent").unwrap_or_default();
    end(ctx, cfg, r, (t, s), json!({"why": "stopped"}))?;
    let d = json!({"agent": agent, "thread": id});
    emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, "stop", d))?;
    Ok(agent.into())
}

fn close(ctx: &Ctx, r: &dyn Runner, s: &Event) -> Result<()> {
    term::close(ctx, r, s.str("agent").unwrap_or_default()).map(drop)
}

/// The conductor role: the thread fleet's `[fleet.<n>.role.conductor]` (else its impl role on profile `lane-conductor`, `Config::parse`); for `all`, the first fleet's that has one.
pub fn role<'a>(cfg: &'a Config, repo: &str) -> Result<(&'a str, &'a Role)> {
    let has = |(n, f): (&'a String, &'a crate::config::Fleet)| Some((n.as_str(), f.role.get("orchestrator")?));
    let found = match repo {
        "all" => cfg.fleet.iter().find_map(has),
        _ => cfg.fleet.get_key_value(repo).and_then(has),
    };
    match found {
        Some(x) => Ok(x),
        None => refused(
            "NO_ORCHESTRATOR",
            format!("{repo} has no orchestrator or impl role; add [fleet.<n>.role.orchestrator]"),
        ),
    }
}

/// A new thread's conductor: the role, with `[chat]`'s harness (another harness drops the role's model), model and effort over it.
pub fn defaults<'a>(cfg: &'a Config, repo: &str) -> Result<(&'a str, Role)> {
    let (fleet, base) = role(cfg, repo)?;
    let c = &cfg.chat;
    let mut r = on(base, c.harness.as_deref());
    r.model = c.model.clone().or(r.model);
    r.effort = c.effort.clone().or(r.effort);
    Ok((fleet, r))
}

fn on(r: &Role, h: Option<&str>) -> Role {
    match h.filter(|h| *h != r.harness) {
        Some(h) => Role {
            harness: h.into(),
            model: None,
            ..r.clone()
        },
        None => r.clone(),
    }
}

/// Thread `t`'s conductor: the defaults, or the role on another harness it started with; then its last model/effort pick.
pub fn thread_role<'a>(cfg: &'a Config, events: &[Event], t: &Event) -> Result<(&'a str, Role)> {
    let (fleet, d) = defaults(cfg, &t.fleet)?;
    let mut r = match t.str("harness") {
        Some(h) if h != d.harness => on(role(cfg, &t.fleet)?.1, Some(h)),
        _ => d,
    };
    r.effort = r
        .effort
        .filter(|e| providers::get(&r.harness).efforts().contains(&e.as_str()));
    let id = t.str("thread").unwrap_or_default();
    if let Some(p) = of(events, id).rfind(|e| e.kind == "thread-model") {
        r.model = p.str("model").map(String::from).or(r.model);
        r.effort = p.str("effort").map(String::from).or(r.effort);
    }
    Ok((fleet, r))
}

/// The models a thread on harness `h` can pick: those its roles on `h` (or `[chat]`, where a fleet's default conductor is on `h`) use, any fleet; none where `h`'s profile owns the model.
pub fn harness_models<'a>(cfg: &'a Config, h: &str) -> Vec<&'a str> {
    if !providers::get(h).takes_model() {
        return vec![];
    }
    let roles = cfg
        .fleet
        .values()
        .flat_map(|f| f.role.values())
        .filter(|r| r.harness == h);
    let chat = cfg.chat.model.as_deref().filter(|_| {
        cfg.fleet
            .keys()
            .any(|n| defaults(cfg, n).is_ok_and(|d| d.1.harness == h))
    });
    let mut all: std::collections::BTreeSet<_> = roles.filter_map(|r| r.model.as_deref()).chain(chat).collect();
    all.extend(cfg.models.iter().map(String::as_str));
    all.into_iter().collect()
}

/// The composer chip's pick: thread `id`'s model and effort from its next turn (`deliver` resumes the conductor on them), logged `thread-model`, a system line in the thread.
pub fn pick(ctx: &Ctx, cfg: &Config, id: &str, model: &str, effort: &str) -> Result<()> {
    let events = working(&ctx.state)?;
    let t = find(&events, id)?;
    let h = thread_role(cfg, &events, t)?.1.harness;
    let ok = (model.is_empty() || harness_models(cfg, &h).contains(&model))
        && (effort.is_empty() || providers::get(&h).efforts().contains(&effort));
    if !ok || model.is_empty() && effort.is_empty() {
        return refused("BAD_VALUE", format!("{model} {effort}: not one of {h}'s choices"));
    }
    let mut d = json!({"thread": id});
    for (k, v) in [("model", model), ("effort", effort)]
        .into_iter()
        .filter(|p| !p.1.is_empty())
    {
        d.put(k, json!(v));
    }
    emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, "thread-model", d))
}

/// The models a fleet's roles use, the [config] sheet's orchestrator model choices.
pub fn models(f: &crate::config::Fleet) -> Vec<&str> {
    let all: std::collections::BTreeSet<_> = f.role.values().filter_map(|r| r.model.as_deref()).collect();
    all.into_iter().collect()
}

/// The [config] sheet's conductor choices (harness, model, effort) into `[fleet.<n>.role.conductor]` in place (a fleet on the default gets the table: its impl role's keys); refused `DOCTOR` if `doctor` with the new file shows a ✗ it did not before. The old file goes to `fleet.toml.bak` first.
pub fn set(ctx: &Ctx, r: &dyn Runner, fleet: &str, wanted: &[(&str, String)]) -> Result<String> {
    let text = std::fs::read_to_string(&ctx.config)?;
    let cfg = Config::parse(&text)?;
    let cur = role(&cfg, fleet)?.1;
    let models = models(cfg.fleet(fleet)?);
    let bad = |(k, v): &&(&str, String)| {
        *k == "model" && !models.contains(&v.as_str()) || v.chars().any(|c| c == '"' || c == '\\' || c.is_control())
    };
    if let Some((k, v)) = wanted.iter().find(bad) {
        return refused("BAD_VALUE", format!("orchestrator {k} {v:?}: not one of the choices"));
    }
    let mut new = text.clone();
    if cur.default {
        let raw: toml::Table = toml::from_str(&text)?;
        let imp = raw
            .get("fleet")
            .and_then(|f| f.get(fleet)?.get("role")?.get("impl")?.as_table());
        let mut t = imp.cloned().unwrap_or_default();
        t.insert("profile".into(), "lane-orchestrator".into());
        let nl = if text.ends_with('\n') { "" } else { "\n" };
        new += &format!("{nl}\n[fleet.{fleet}.role.orchestrator]\n{}", toml::to_string(&t)?);
    }
    for (k, v) in wanted {
        new = crate::config::set_value(&new, fleet, "orchestrator", k, v)?;
    }
    let new_cfg = save(ctx, r, &new)?;
    for (k, v) in wanted {
        let d = json!({"role": "orchestrator", "key": k, "new": v, "via": "web"});
        emit(&ctx.state, &new_cfg, Event::new(ctx.now, fleet, None, "set", d))?;
    }
    let line: Vec<String> = wanted.iter().map(|(k, v)| format!("{k} {v}")).collect();
    Ok(format!("{fleet} orchestrator: {}", line.join(", ")))
}

/// `[chat]`'s new-thread defaults (harness, model, effort) in place (the table added when missing), each checked as fleet.toml's load checks it (`BAD_VALUE`), then written as `set` writes; logs `set` (role `chat`) per key.
pub fn chat(ctx: &Ctx, r: &dyn Runner, wanted: &[(&str, String)]) -> Result<String> {
    let text = std::fs::read_to_string(&ctx.config)?;
    let cfg = Config::parse(&text)?;
    let models: Vec<&str> = cfg.fleet.values().flat_map(models).collect();
    let bad = |(k, v): &&(&str, String)| {
        *k == "model" && !models.contains(&v.as_str()) || v.chars().any(|c| c == '"' || c == '\\' || c.is_control())
    };
    if let Some((k, v)) = wanted.iter().find(bad) {
        return refused("BAD_VALUE", format!("chat {k} {v:?}: not one of the choices"));
    }
    let mut new = text.clone();
    if !toml::from_str::<toml::Table>(&text)?.contains_key("chat") {
        new += if text.ends_with('\n') {
            "\n[chat]\n"
        } else {
            "\n\n[chat]\n"
        };
    }
    for (k, v) in wanted {
        new = crate::config::set_in(&new, "[chat]", k, &format!("\"{v}\""))?;
    }
    if let Err(e) = Config::parse(&new) {
        return refused("BAD_VALUE", format!("{e:#}"));
    }
    let new_cfg = save(ctx, r, &new)?;
    for (k, v) in wanted {
        let d = json!({"role": "chat", "key": k, "new": v, "via": "web"});
        emit(&ctx.state, &new_cfg, Event::new(ctx.now, "all", None, "set", d))?;
    }
    let line: Vec<String> = wanted.iter().map(|(k, v)| format!("{k} {v}")).collect();
    Ok(format!("chat defaults: {}", line.join(", ")))
}

fn save(ctx: &Ctx, r: &dyn Runner, new: &str) -> Result<Config> {
    let new_cfg = Config::parse(new)?;
    let fails = |c: &Ctx| -> Result<Vec<String>> {
        let mut out = vec![];
        crate::doctor::doctor(c, r, &mut out, true, false)?;
        Ok(serde_json::from_slice::<Vec<String>>(&out)?
            .into_iter()
            .filter(|l| l.starts_with('✗'))
            .collect())
    };
    let (before, tmp) = (fails(ctx)?, crate::devices::tmp_name(&ctx.config));
    std::fs::write(&tmp, new)?;
    let after = fails(&Ctx {
        config: tmp.clone(),
        ..ctx.clone()
    });
    std::fs::remove_file(&tmp)?;
    let bad: Vec<String> = after?.into_iter().filter(|l| !before.contains(l)).collect();
    if !bad.is_empty() {
        return refused("DOCTOR", format!("doctor fails with it: {}", bad.join("; ")));
    }
    std::fs::copy(&ctx.config, ctx.config.with_extension("toml.bak"))?;
    crate::config::write_atomic(&ctx.config, new)?;
    Ok(new_cfg)
}

fn start(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    (t, replaced): (&Event, Option<&str>),
    events: &[Event],
    line: &str,
) -> Result<String> {
    let id = t.str("thread").unwrap_or_default();
    let (fleet, base) = thread_role(cfg, events, t)?;
    let f = cfg.fleet(fleet)?;
    let role = base;
    let h = providers::get(&role.harness);
    let dir = dir(ctx, cfg, id);
    let brief = dir.join(".brief.md");
    let on = crate::hosts::On::new(r, None);
    let thread = dir.join(".thread.md");
    on.write(&thread, &render(&items(events, id), name(cfg, id_of(t))))?;
    let kick = format!(
        "Read {} and act on it. The thread so far is in {}, the last message new.",
        brief.display(),
        thread.display()
    );
    let agent = format!("orchestrator-{id}");
    let head = format!("{} · orchestrator {id}", t.fleet);
    let mut p = Plan {
        agent: agent.clone(),
        role: "orchestrator".into(),
        stage: "orchestrator".into(),
        family: "orchestrator",
        harness: role.harness.clone(),
        profile: role.profile_or("orchestrator"),
        model: role.model.clone(),
        effort: role.effort_or(&cfg.limits),
        branch: String::new(),
        base: String::new(),
        worktree: dir.clone(),
        brief: brief.clone(),
        kick,
        lane: format!("{fleet}:-:orchestrator"),
        env: f.env.clone(),
        port: crate::spawn::lane_port(f.port_range.as_deref(), &format!("{fleet}/{agent}"), &[]),
        mcp: crate::access::mcp_servers(cfg, f, &role)?,
        mcp_file: dir.join(".mcp.lane.json"),
        title: crate::spawn::lane_title(&head, t.str("title").unwrap_or_default()),
        head,
        sandbox: None,
        account: Some(cfg.account(fleet, "orchestrator")),
    };
    p.env.insert("DROVER_THREAD".into(), id.into());
    p.env.insert("DROVER_ORCHESTRATOR".into(), id_of(t).into());
    let shed = cfg.shed_for(fleet).context(crate::config::NO_RUNTIME)?;
    let wire = r.shed().map_or(shed.wire.clone(), |w| w.wire);
    let all = crate::shed::accounts(r, &wire).ok();
    let hold = (p.account.as_ref()).and_then(|a| crate::accounts::of(all.as_deref()?, &p.harness, a));
    if let Some(h) = hold {
        return on_hold(ctx, cfg, r, t, events, &h);
    }
    let aid = shed.acp.clone().context(crate::config::NO_RUNTIME)?;
    let acp = match crate::spawn::acp_cmd(ctx, r, &p, &aid, None) {
        Ok(cmd) => cmd,
        Err(e) => {
            if e.downcast_ref::<crate::gate::Refusal>().is_some() {
                log(
                    ctx,
                    cfg,
                    t,
                    "system",
                    (&format!("the orchestrator does not start: {e:#}"), &[]),
                )?;
            }
            return Err(e);
        }
    };
    h.ready(ctx, r, &p)?;
    let text = brief::thread(cfg, t, fleet);
    on.write(&brief, &text)?;
    h.prepare(ctx, &on, &p)?;
    crate::spawn::write_acp_mcp(&on, &p, &acp)?;
    let _secrets = crate::spawn::deliver(ctx, &on, &p)?;
    let labels = [
        ("fleet", fleet),
        ("issue", "-"),
        ("role", "orchestrator"),
        ("harness", &p.harness),
        ("source", "drover"),
        ("group", f.repo_name()),
        ("thread", id),
    ]
    .map(|(k, v)| (k, v.to_string()));
    let b = term::Shed(crate::shed::Shed { wire, ..shed });
    b.start(ctx, r, &agent, &labels, &dir.to_string_lossy(), &acp)?;
    let kick = format!("The thread so far is in {}, the last message new.", thread.display());
    let kicked = b.kick_acp(r, &agent, &format!("{}\n\n{kick}\n\n{line}", text.trim_end()))?;
    let mut d = json!({"thread": id, "agent": agent, "pane": agent, "runtime": "shed-acp", "harness": p.harness,
        "model": p.model, "effort": p.effort, "resumed": false,
        "confirmed": kicked, "account": p.account.as_ref().map(crate::config::Account::word)});
    if let Some(was) = replaced {
        d.put("replaced", json!(was));
    }
    started(ctx, cfg, t, d)
}

fn started(ctx: &Ctx, cfg: &Config, t: &Event, d: Value) -> Result<String> {
    let agent = d.at("agent").as_str().unwrap_or_default().to_string();
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, &t.fleet, None, "orchestrator-start", d),
    )?;
    Ok(agent)
}

/// The thread as items: messages (`from`, `text`) and asks (`ask`, `question`, numbered `options` with the recommended one first, `open`).
pub fn items(events: &[Event], id: &str) -> Vec<Value> {
    let sent = forwarded(events);
    let item = |e: &Event| match e.kind.as_str() {
        "thread-msg" => {
            let mut m = json!({"ts": e.ts, "from": e.str("from"), "text": e.str("text")});
            if let Some(f) = e.data.get("files") {
                m.put("files", f.clone());
            }
            if let Some((w, t)) = link(e) {
                m.put("link", json!(format!("{w} {t}")));
            }
            if sent.contains(&e.seq) {
                m.put(
                    "text",
                    json!(format!("{} (forwarded)", e.str("text").unwrap_or_default())),
                );
            }
            Some(m)
        }
        "thread-model" => Some(json!({"ts": e.ts, "from": "system", "text": picked(e)})),
        "run-card" => {
            let open = crate::card::settled(events, e.str("id").unwrap_or_default()).is_none();
            Some(
                json!({"ts": e.ts, "from": "orchestrator", "text": crate::card::text(e), "card": e.data, "open": open}),
            )
        }
        "timer-install" => {
            let (by, w, l) = (e.str("by").unwrap_or("?"), &e.val("written"), &e.val("reloaded"));
            let text = format!("{by} ran `drover timer install`: wrote {w}, reloaded {l}");
            Some(json!({"ts": e.ts, "from": "system", "text": text}))
        }
        "thread-handoff" => Some(
            json!({"ts": e.ts, "from": "system", "text": handoff_text(e), "link": format!("continued in {}", e.str("to").unwrap_or_default())}),
        ),
        "needs-decision" | "ask-owner" => {
            let all = e.val("options").as_array().cloned().unwrap_or_default();
            let mut opts: Vec<Value> = all
                .into_iter()
                .zip(1..)
                .map(|(t, n)| json!({"n": n, "text": t}))
                .collect();
            let rec = e.val("recommend").as_u64().unwrap_or(0);
            opts.sort_by_key(|o| o["n"] != rec);
            let lane = if e.kind == "ask-owner" { lane_of(events, e)? } else { e };
            let open = events::closed(events, lane).is_none();
            Some(
                json!({"ts": e.ts, "from": "orchestrator", "ask": e.str("id"), "fleet": lane.fleet, "question": e.str("question"),
                "options": opts, "recommend": e.val("recommend"), "open": open}),
            )
        }
        _ => None,
    };
    of(events, id).filter_map(item).collect()
}

/// A pick's system line: `model sonnet · effort high from the next turn`.
pub fn picked(e: &Event) -> String {
    let kv = ["model", "effort"].map(|k| e.str(k).map(|v| format!("{k} {v}")));
    format!(
        "{} from the next turn",
        kv.into_iter().flatten().collect::<Vec<_>>().join(" · ")
    )
}

fn render(items: &[Value], who: &str) -> String {
    let s = |v: &Value, k: &str| v[k].as_str().unwrap_or_default().to_string();
    let line = |i: &Value| match i["ask"].as_str() {
        None => {
            let files = i["files"].as_array().into_iter().flatten();
            let files: String = files.map(|f| format!(" [file {}]", s(f, "path"))).collect();
            let from = i["from"].as_str().filter(|f| *f != "orchestrator").unwrap_or(who);
            format!("{}: {}{files}\n", i["link"].as_str().unwrap_or(from), s(i, "text"))
        }
        Some(q) => {
            let rec = |o: &Value| if o["n"] == i["recommend"] { " (recommended)" } else { "" };
            let opts = i["options"].as_array().into_iter().flatten();
            let opts: String = opts
                .map(|o| format!("  [{}] {}{}\n", o["n"], s(o, "text"), rec(o)))
                .collect();
            format!("{who} asks {q}: {}\n{opts}", s(i, "question"))
        }
    };
    items.iter().map(line).collect()
}

/// The orchestrator spoke (a message or an ask) since the owner last read or wrote.
pub fn unread(events: &[Event], id: &str) -> bool {
    let seen = |e: &&Event| {
        matches!(
            e.kind.as_str(),
            "thread-msg" | "thread-read" | "needs-decision" | "ask-owner"
        )
    };
    let last = of(events, id).rfind(seen);
    last.is_some_and(|e| {
        matches!(e.kind.as_str(), "needs-decision" | "ask-owner") || e.str("from") == Some("orchestrator")
    })
}

/// Log `thread-read` for thread `t` if the orchestrator spoke since the owner last read or wrote (`thread show`, the board).
pub fn mark_read(ctx: &Ctx, cfg: &Config, events: &[Event], t: &Event) -> Result<()> {
    let id = t.str("thread").unwrap_or_default();
    let read = Event::new(ctx.now, &t.fleet, None, "thread-read", json!({"thread": id}));
    if unread(events, id) {
        emit(&ctx.state, cfg, read)?;
    }
    Ok(())
}

/// `drover thread show <id>`: the conversation; marks it read.
pub fn show(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, id: &str, as_json: bool) -> Result<()> {
    let events = working(&ctx.state)?;
    let events = if archived(&events, id) {
        events::shared(&ctx.state)?
    } else {
        events
    };
    let t = find(&events, id)?;
    let items = items(&events, id);
    mark_read(ctx, cfg, &events, t)?;
    if as_json {
        let v = json!({"thread": id, "repo": t.fleet, "title": title(&events, id), "items": items});
        return Ok(writeln!(out, "{}", serde_json::to_string_pretty(&v)?)?);
    }
    writeln!(out, "{id} {}: {}", t.fleet, title(&events, id))?;
    Ok(write!(out, "{}", render(&items, name(cfg, id_of(t))))?)
}

/// `drover thread ls [fleet]`: per repo, its threads with title, last message, unread dot and conductor state.
pub fn ls(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, fleet: Option<&str>, as_json: bool) -> Result<()> {
    let events = working(&ctx.state)?;
    let mut rows: Vec<Value> = vec![];
    for t in events
        .iter()
        .filter(|e| e.kind == "thread-new" && fleet.is_none_or(|f| f == e.fleet))
    {
        let id = t.str("thread").unwrap_or_default();
        if archived(&events, id) {
            continue;
        }
        let items = items(&events, id);
        let who = name(cfg, id_of(t));
        let last = items.last().map(|i| render(std::slice::from_ref(i), who));
        let last = last.as_deref().and_then(|l| l.lines().next()).unwrap_or_default();
        let lane = of(&events, id).rfind(|e| e.kind.starts_with("orchestrator-"));
        let state = lane.map_or("none", |e| {
            if e.kind == "orchestrator-start" {
                "awake"
            } else {
                "asleep"
            }
        });
        rows.push(
            json!({"thread": id, "repo": t.fleet, "title": title(&events, id), "last": last,
            "at": items.last().map_or(t.ts, |i| i["ts"].as_u64().unwrap_or(t.ts)), "unread": unread(&events, id),
            "state": state, "orchestrator": id_of(t), "name": who}),
        );
    }
    rows.sort_by(|a, b| a["repo"].as_str().cmp(&b["repo"].as_str()));
    if as_json {
        return Ok(writeln!(out, "{}", serde_json::to_string_pretty(&rows)?)?);
    }
    let mut repo = "";
    for r in &rows {
        let s = |k: &str| r[k].as_str().unwrap_or_default();
        if s("repo") != repo {
            repo = s("repo");
            writeln!(out, "{repo}")?;
        }
        let dot = if r["unread"] == true { "•" } else { " " };
        let age = events::age(ctx.now.saturating_sub(r["at"].as_u64().unwrap_or(0)));
        writeln!(
            out,
            "  {} {dot} {}: {} ({age}, {} {})",
            s("thread"),
            s("title"),
            s("last"),
            s("name"),
            s("state")
        )?;
    }
    Ok(())
}

/// `check`: each awake conductor whose thread has been quiet `IDLE` and that is not working is closed (`conductor-sleep`); the next message resumes it.
pub fn sleep_idle(ctx: &Ctx, cfg: &Config, r: &dyn Runner, events: &[Event], agents: &[Session]) -> Result<()> {
    let all = std::cell::OnceCell::new();
    for t in events.iter().filter(|e| e.kind == "thread-new") {
        let id = t.str("thread").unwrap_or_default();
        let last = of(events, id).rfind(|e| e.kind.starts_with("orchestrator-"));
        let Some(s) = last.filter(|e| e.kind == "orchestrator-start") else {
            continue;
        };
        let heard = of(events, id).filter(|e| e.data.get("noreply").is_none());
        let quiet = heard.map(|e| e.ts).max().unwrap_or(0) + cfg.limits.orchestrator_idle_minutes * 60 <= ctx.now;
        let name = s.str("agent").unwrap_or_default();
        let listed = agents.iter().find(|a| a.name == name).cloned();
        let Ok(a) = listed.map_or_else(|| crate::session::get(r, name), |a| Ok(Some(a))) else {
            continue;
        };
        let a = a.as_ref();
        let out = account(s).filter(|_| a.is_some()).and_then(|(h, acct)| {
            let all = all.get_or_init(|| crate::accounts::ls(cfg, r));
            crate::accounts::of(all.as_deref()?, &h, &acct).filter(|h| h.why == crate::accounts::Why::SignedOut)
        });
        if let Some(h) = out {
            signed_out(ctx, cfg, r, t, s, &h)?;
            continue;
        }
        if a.is_some() && pending(events, id).is_some() {
            continue;
        }
        let gone = a.is_none();
        if !gone && (!quiet || a.is_some_and(Session::busy)) {
            continue;
        }
        if a.is_some() {
            close(ctx, r, s)?;
        }
        let d = json!({"thread": id, "agent": s.str("agent"), "why": if a.is_some() { "idle" } else { "gone" }});
        emit(
            &ctx.state,
            cfg,
            Event::new(ctx.now, &t.fleet, None, "orchestrator-sleep", d),
        )?;
    }
    Ok(())
}

/// Shed moved thread `id`'s orchestrator to account `now` (`why` `limit` or `owner`): one row, `<was> hit its limit; now on <now>` or `switched to <now>`; none at its `start`.
pub fn account_moved(
    ctx: &Ctx,
    cfg: &Config,
    (fleet, id): (&str, &str),
    (now, was): (&str, Option<&str>),
    why: &str,
) -> Result<()> {
    let text = match (why, was) {
        ("limit", Some(was)) => format!("{was} hit its limit; now on {now}"),
        ("limit" | "owner", _) => format!("switched to {now}"),
        _ => return Ok(()),
    };
    let d = json!({"thread": id, "from": "system", "text": text, "account": now});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, None, "thread-msg", d))
}

fn account(s: &Event) -> Option<(String, crate::config::Account)> {
    let a = match s.str("account")? {
        "auto" => crate::config::Account::Auto,
        n => crate::config::Account::Named(n.into()),
    };
    Some((s.str("harness")?.into(), a))
}

fn signed_out(ctx: &Ctx, cfg: &Config, r: &dyn Runner, t: &Event, s: &Event, h: &crate::accounts::Hold) -> Result<()> {
    let id = t.str("thread").unwrap_or_default();
    close(ctx, r, s)?;
    let d = json!({"thread": id, "agent": s.str("agent"), "why": "signed out"});
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, &t.fleet, None, "orchestrator-sleep", d),
    )?;
    let d = json!({"thread": id, "from": "system", "text": h.line(r), "signin": h.signin()});
    emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, "thread-msg", d))
}

fn human(e: &Event) -> bool {
    e.kind == "thread-msg"
        && e.str("from")
            .is_some_and(|f| !matches!(f, "orchestrator" | "system") && !is_id(f))
}

fn since(events: &[Event], id: &str, seq: u64, flag: &str) -> bool {
    of(events, id).any(|e| {
        e.seq > seq && e.kind == "thread-msg" && (e.str("from") == Some("orchestrator") || e.data.get(flag).is_some())
    })
}

fn pending<'a>(events: &'a [Event], id: &str) -> Option<&'a Event> {
    let h = of(events, id).rfind(|e| e.kind == "thread-msg" && e.data.get("held").is_some())?;
    let sent = of(events, id).any(|e| e.seq > h.seq && e.kind == "thread-continued");
    (!sent && !since(events, id, h.seq, "held")).then_some(h)
}

fn held(ctx: &Ctx, cfg: &Config, r: &dyn Runner, t: &Event, events: &[Event], a: Option<&Session>) -> Result<()> {
    let Some(a) = a.filter(|a| a.status() == Some(crate::session::Status::Limited)) else {
        return Ok(());
    };
    let at = crate::limit::reset_at(r, a.note.as_deref().unwrap_or_default(), ctx.now);
    let until = at.map_or("its reset".into(), |at| crate::usage::date(r, at, "+%a %H:%M"));
    let text = format!("orchestrator out of usage until {until}; your message is kept");
    hold_row(ctx, cfg, t, events, (&text, at))
}

fn hold_row(ctx: &Ctx, cfg: &Config, t: &Event, events: &[Event], (text, at): (&str, Option<u64>)) -> Result<()> {
    let id = t.str("thread").unwrap_or_default();
    let Some(m) = of(events, id)
        .rfind(|e| human(e))
        .filter(|m| !since(events, id, m.seq, "held"))
    else {
        return Ok(());
    };
    let d = json!({"thread": id, "from": "system", "text": text, "held": m.seq, "at": at});
    emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, "thread-msg", d))
}

fn on_hold(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    t: &Event,
    events: &[Event],
    h: &crate::accounts::Hold,
) -> Result<String> {
    use crate::accounts::Why;
    let line = h.line(r);
    match &h.why {
        Why::Setup(_) => {
            let why = format!("{line}; the orchestrator does not start");
            log(
                ctx,
                cfg,
                t,
                "system",
                (&format!("the orchestrator does not start: ACP_UNGUARDED: {why}"), &[]),
            )?;
            return refused("ACP_UNGUARDED", why);
        }
        Why::Limited(at) => {
            let text = format!("{line}; your message is kept. New threads will fail too until then.");
            hold_row(ctx, cfg, t, events, (&text, *at))?;
        }
        Why::SignedOut | Why::Missing => {
            let d = json!({"thread": t.str("thread"), "from": "system", "text": line, "signin": h.signin()});
            emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, "thread-msg", d))?;
        }
    }
    refused("LOGIN_NEEDED", format!("{line}; the orchestrator waits"))
}

/// `check`: per thread, its rate-limited orchestrator's `out of usage` row, one `continue` at that row's reset time, and a `no reply yet` row for an owner message unanswered `orchestrator_reply_minutes`.
pub fn silence(ctx: &Ctx, cfg: &Config, r: &dyn Runner, events: &[Event], agents: &[Session]) -> Result<()> {
    for t in threads(events) {
        let id = t.str("thread").unwrap_or_default();
        let last = of(events, id).rfind(|e| e.kind.starts_with("orchestrator-"));
        let s = last.filter(|e| e.kind == "orchestrator-start");
        let a = s.and_then(|s| agents.iter().find(|a| Some(a.name.as_str()) == s.str("agent")));
        held(ctx, cfg, r, t, events, a)?;
        let due = pending(events, id).filter(|h| h.val("at").as_u64().is_some_and(|at| ctx.now >= at));
        if let Some(h) = due {
            let sent = deliver(ctx, cfg, r, t, crate::limit::CONTINUE).is_ok();
            let d = json!({"thread": id, "held": h.seq, "sent": sent});
            emit(
                &ctx.state,
                cfg,
                Event::new(ctx.now, &t.fleet, None, "thread-continued", d),
            )?;
        }
        let Some(m) = of(events, id).rfind(|e| human(e)) else {
            continue;
        };
        if ctx.now < m.ts + cfg.limits.orchestrator_reply_minutes * 60 || since(events, id, m.seq, "noreply") {
            continue;
        }
        let shows = a.map_or("asleep".into(), |a| {
            let word = a.status().map_or("gone", crate::session::Status::word);
            a.note.as_deref().map_or(word.to_string(), |n| format!("{word}: {n}"))
        });
        let d = json!({"thread": id, "from": "system", "text": format!("no reply yet: the orchestrator is {shows}"), "noreply": m.seq});
        emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, "thread-msg", d))?;
    }
    Ok(())
}
