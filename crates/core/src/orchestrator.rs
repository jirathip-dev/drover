//! #355: conductor chat. A thread belongs to a fleet (or `all`); one conductor lane (role `conductor`, any harness)
//! talks it through in a working folder without a worktree, sleeps after `IDLE` and wakes with its saved session.
use crate::config::{expand, Config, Role};
use crate::events::{self, emit, shared, Event};
use crate::gate::refused;
use crate::runner::Runner;
use crate::spawn::Plan;
use crate::{herdr, providers, Ctx};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::Write;

/// An orchestrator whose thread is quiet this long (and that is not working) closes on the next `check`.
pub const IDLE: u64 = 30 * 60;

/// #620: how many times `of` ran (#701: `WALKS`, the events it looked at).
pub static SCANS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// #701: the events thread lookups (`of`) looked at: the whole log's off the index, the thread's own on it.
pub static WALKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The thread's events: its messages, asks, starts and sleeps. #701: a cached log's from its thread index.
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
    let at = at.into_iter().flatten().map(move |i| &events[i]);
    at.chain(all.into_iter().flatten())
}

/// #701: every thread's `thread-new`, oldest first.
pub fn threads(events: &[Event]) -> Vec<&Event> {
    match events::cached(events) {
        Some(l) => l.threads.new.iter().map(|i| &events[*i]).collect(),
        None => events.iter().filter(|e| e.kind == "thread-new").collect(),
    }
}

/// The thread's `thread-new` (`NO_SUCH_THREAD`).
pub fn find<'a>(events: &'a [Event], id: &str) -> Result<&'a Event> {
    match of(events, id).find(|e| e.kind == "thread-new") {
        Some(t) => Ok(t),
        None => refused(
            "NO_SUCH_THREAD",
            format!("no thread {id}; `herdsman thread new` starts one"),
        ),
    }
}

/// #633: cooee thread `id`'s last `conductor-attached` / `conductor-detached`.
fn link_of<'a>(events: &'a [Event], id: &str) -> Option<&'a Event> {
    let link = |e: &&Event| matches!(e.kind.as_str(), "orchestrator-attached" | "orchestrator-detached");
    events.iter().rfind(|e| link(e) && e.str("thread") == Some(id))
}

/// #633: cooee thread `id`'s `orchestrator-attached` while the link holds (not detached, not archived since).
pub fn attached<'a>(events: &'a [Event], id: &str) -> Option<&'a Event> {
    let since = |a: &Event| of(events, id).any(|e| e.kind == "thread-archive" && e.seq > a.seq);
    link_of(events, id).filter(|e| e.kind == "orchestrator-attached" && !since(e))
}

/// Thread `id`'s fleet: an attached cooee thread's, else herdsman's own thread's (`NO_SUCH_THREAD`).
pub fn fleet_of<'a>(events: &'a [Event], id: &str) -> Result<&'a str> {
    Ok(&attached(events, id).map_or_else(|| find(events, id), Ok)?.fleet)
}

/// #633: `POST /api/v1/conductor-attach`: cooee thread `id` becomes `fleet`'s conductor thread (`conductor-attached`);
/// what its profile needs: the brief, the synced settings, the env, and whether #565's ACP guard holds (`why` not).
pub fn attach(ctx: &Ctx, cfg: &Config, r: &dyn Runner, fleet: &str, id: &str) -> Result<Value> {
    let f = cfg.fleet(fleet)?;
    let (_, role) = defaults(cfg, fleet)?;
    let ok = |c: char| c.is_ascii_alphanumeric() || "-_.:".contains(c);
    if id.is_empty() || id.len() > 128 || !id.chars().all(ok) {
        return refused("BAD_VALUE", "thread: a cooee thread id (letters, digits, `-_.:`)");
    }
    let events = shared(&ctx.state)?;
    if find(&events, id).is_ok() {
        return refused("BAD_VALUE", format!("{id} is herdsman's own thread"));
    }
    let t = Event::new(ctx.now, fleet, None, "orchestrator-attached", json!({"thread": id}));
    emit(&ctx.state, cfg, t.clone())?;
    let p = role.profile_or("orchestrator");
    let acp = providers::get(&role.harness).acp().unwrap_or(&role.harness);
    // #683: the account's env is the thread's sign-in (its own login); the profile's settings go on top.
    let acct = crate::access::pick(ctx, cfg, &events, &role).ok().flatten();
    let acct = acct
        .map(|a| crate::access::account_env(cfg, r, &role.harness, &a.0))
        .transpose()?;
    let acct = acct.unwrap_or_default();
    let why = crate::spawn::unguarded(ctx, (&p, "orchestrator", &role.harness), acp, (!acct.is_empty(), false));
    let port = crate::spawn::lane_port(f.port_range.as_deref(), &format!("{fleet}/orchestrator-{id}"), &[]);
    let settings = providers::profiles_root(ctx).join(&p).join("settings.json");
    let mut env = json!({"HERDSMAN_LANE": format!("{fleet}:-:orchestrator"), "HERDSMAN_THREAD": id,
        "HERDSMAN_PORT": port.to_string()});
    for (k, v) in acct {
        env[k] = json!(v);
    }
    let message = format!("{id} is {fleet}'s orchestrator thread");
    Ok(
        json!({"ok": true, "code": "OK", "message": message, "brief": profile(cfg, &t, fleet),
        "settings": settings, "env": env, "guarded": why.is_empty(), "why": why}),
    )
}

/// #633: `POST /api/v1/orchestrator-detach`: the link ends (`orchestrator-detached`); `NO_SUCH_THREAD` unless attached.
pub fn detach(ctx: &Ctx, cfg: &Config, fleet: &str, id: &str) -> Result<String> {
    if attached(&shared(&ctx.state)?, id).is_none_or(|e| e.fleet != fleet) {
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

/// #780: the old role name a lane started before #780 still carries (`HERDSMAN_LANE`), read for one release.
pub fn is_role(role: &str) -> bool {
    matches!(role, "orchestrator" | "conductor")
}

/// #780: a `by` in the old spelling (`conductor-<thread>`: a flag, a pre-#780 event) as `orchestrator-<thread>`.
pub fn by(b: &str) -> String {
    b.strip_prefix("conductor-")
        .map_or_else(|| b.into(), |t| format!("orchestrator-{t}"))
}

/// #780: in an orchestrator lane, its `HERDSMAN_ORCHESTRATOR` id; every event it logs names it (`db::insert`).
pub fn acting() -> Option<String> {
    let lane = std::env::var("HERDSMAN_LANE").ok()?;
    is_role(lane.split(':').nth(2)?).then(|| std::env::var("HERDSMAN_ORCHESTRATOR").ok())?
}

/// #780: the orchestrator a thread from before #780 (none named) belongs to, the one `[orchestrator]` itself names.
pub const DEFAULT: &str = "default";

/// #780: orchestrator `id`'s name, what every label shows (code keys off the role and the id only).
pub fn name<'a>(cfg: &'a Config, id: &str) -> &'a str {
    let o = &cfg.orchestrator;
    let n = if id == DEFAULT {
        o.name.as_deref()
    } else {
        o.more.get(id).and_then(|m| m.name.as_deref())
    };
    n.unwrap_or("Orchestrator")
}

/// #780: `by` as a label: an orchestrator's (`orchestrator-<thread>`) with its name in front.
pub fn label(cfg: &Config, events: &[Event], by: &str) -> String {
    let t = by.strip_prefix("orchestrator-");
    t.map_or_else(|| by.into(), |t| format!("{} ({by})", name_of(cfg, events, t)))
}

/// #780: thread `t`'s orchestrator id (its `thread-new`'s).
pub fn id_of(t: &Event) -> &str {
    t.str("orchestrator").unwrap_or(DEFAULT)
}

/// #780: the name of thread `id`'s orchestrator, or of orchestrator `id` itself.
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

/// `herdsman thread new <fleet|all> <title> [--harness <h>]`: its id. #404: the thread keeps its harness (`harness`,
/// else the default's) for good.
pub fn new(ctx: &Ctx, cfg: &Config, repo: &str, title: &str, harness: Option<&str>) -> Result<String> {
    new_with(ctx, cfg, repo, title, (harness, None))
}

/// `new`, #780: the thread talking to orchestrator `orch` (`assign`).
pub fn new_with(ctx: &Ctx, cfg: &Config, repo: &str, title: &str, (harness, orch): Opt2) -> Result<String> {
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
    let events = shared(&ctx.state)?;
    let id = format!("t{}", events.iter().filter(|e| e.kind == "thread-new").count() + 1);
    let mut d = json!({"thread": id, "title": title, "harness": h});
    if orch != DEFAULT {
        d["orchestrator"] = json!(orch);
    }
    emit(&ctx.state, cfg, Event::new(ctx.now, repo, None, "thread-new", d))?;
    Ok(id)
}

type Opt2<'a> = (Option<&'a str>, Option<&'a str>);

fn title_ok(title: &str) -> Result<()> {
    if title.trim().is_empty() || title.chars().any(char::is_control) {
        return refused("BAD_VALUE", "a thread has a one-line title");
    }
    Ok(())
}

/// #405: the thread's title: its last `thread title`, else the one it started with.
pub fn title<'a>(events: &'a [Event], id: &str) -> &'a str {
    let t = of(events, id).rfind(|e| matches!(e.kind.as_str(), "thread-new" | "thread-title"));
    t.and_then(|e| e.str("title")).unwrap_or_default()
}

/// #405: `herdsman thread title <id> <title>`: the orchestrator's short summary title, once after its first reply.
pub fn retitle(ctx: &Ctx, cfg: &Config, id: &str, title: &str) -> Result<()> {
    title_ok(title)?;
    let t = find(&shared(&ctx.state)?, id)?.clone();
    let d = json!({"thread": id, "title": title});
    emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, "thread-title", d))
}

/// #405: the thread is archived (its last archive or restore an archive).
pub fn archived(events: &[Event], id: &str) -> bool {
    let a = of(events, id).rfind(|e| matches!(e.kind.as_str(), "thread-archive" | "thread-restore"));
    a.is_some_and(|e| e.kind == "thread-archive")
}

/// #405: `thread archive` (`on`) closes the thread's live conductor (`conductor-sleep`; its folder, uploads and log
/// stay) and folds the thread; `thread restore` unfolds it, and its next message resumes the same session.
pub fn archive(ctx: &Ctx, cfg: &Config, r: &dyn Runner, id: &str, on: bool) -> Result<()> {
    let events = shared(&ctx.state)?;
    // #633: archiving an attached cooee thread ends its link.
    let t = link_of(&events, id).filter(|e| e.kind == "orchestrator-attached");
    let t = t.map_or_else(|| find(&events, id), Ok)?;
    let last = of(&events, id).rfind(|e| e.kind.starts_with("orchestrator-"));
    if let Some(s) = last.filter(|s| on && s.kind == "orchestrator-start") {
        if herdr::agents(r)?.iter().any(|a| a["name"] == s.data["agent"]) {
            close(r, s)?;
        }
        let d = json!({"thread": id, "agent": s.str("agent"), "why": "archived"});
        emit(
            &ctx.state,
            cfg,
            Event::new(ctx.now, &t.fleet, None, "orchestrator-sleep", d),
        )?;
    }
    let kind = ["thread-restore", "thread-archive"][usize::from(on)];
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

/// #405: a message's files (name, bytes) into thread `id`'s `uploads/`, as the thread logs them: at most 10, each
/// named and at most 10 MB (checked before any is written); a name repeated in the message gets its place in front.
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

/// #413: `thread handoff`: to thread `to` at once, linked; wakes its conductor as `from`. #492: to a repo (or `all`), a
/// choice card in `from` (an owner ask with `handoff`): the repo's threads or a new one; `pick` delivers on the tap.
pub fn handoff(ctx: &Ctx, cfg: &Config, r: &dyn Runner, [from, to, title, text]: [&str; 4]) -> Result<String> {
    text_ok(text)?;
    let events = shared(&ctx.state)?;
    let t = find(&events, from)?;
    if to == from {
        return refused("BAD_VALUE", "a thread hands off to another thread");
    }
    let fwd = forwards(&events, from);
    // The target's conductor is checked first: no thread or link is left behind without delivery.
    if is_id(to) {
        role(cfg, &find(&events, to)?.fleet)?;
        return send(ctx, cfg, r, (from, to, to), (title, text), (fwd, None));
    }
    role(cfg, to)?;
    title_ok(title)?;
    let open = |e: &&Event| e.kind == "thread-new" && e.fleet == to;
    let mut rows: Vec<(u64, &str)> = (events.iter().filter(open))
        .filter_map(|e| e.str("thread"))
        .filter(|id| *id != from && !archived(&events, id))
        .map(|id| (of(&events, id).map(|e| e.ts).max().unwrap_or(0), id))
        .collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.0));
    let row = |(ts, id): &(u64, &str)| {
        let last = items(&events, id).pop().unwrap_or_default();
        let last = last["text"].as_str().or(last["question"].as_str()).unwrap_or_default();
        let topic: String = last
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(60)
            .collect();
        let age = events::age(ctx.now.saturating_sub(*ts));
        let topic = if topic.is_empty() {
            String::new()
        } else {
            format!(" — {topic}")
        };
        format!("{} · {id}{topic} ({age} ago)", self::title(&events, id))
    };
    let mut opts: Vec<String> = rows.iter().map(row).collect();
    let mut targets: Vec<&str> = rows.iter().map(|r| r.1).collect();
    // The newest 4, then the new thread; the rest after it, behind the board's `more` row.
    opts.insert(opts.len().min(4), format!("new thread: {title}"));
    targets.insert(targets.len().min(4), "new");
    let q = format!("q{}", events.iter().filter(|e| e.kind == "needs-decision").count() + 1);
    let question = format!("Hand off \"{title}\" to which {to} thread? Nothing is delivered until you tap one.");
    let d = json!({"id": q, "question": question, "options": opts, "recommend": null, "kind": "owner", "thread": from,
        "handoff": {"repo": to, "title": title, "text": text, "targets": targets}, "forwards": fwd});
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, &t.fleet, None, "needs-decision", d),
    )?;
    Ok(format!(
        "{q}: the owner picks the thread; nothing is delivered until then"
    ))
}

/// #492: the owner's tap on a handoff card (`choice`: an option number or its text, or a thread id): delivered there
/// (`new`: a new thread in the card's repo); the target, and whether this tap delivered it. #621: once per card: a tap
/// waits for one in flight and gets the delivered one's target; after a failed delivery it retries that one.
pub fn land(ctx: &Ctx, cfg: &Config, r: &dyn Runner, ask: &Event, choice: &str) -> Result<(String, bool)> {
    let (h, from) = (&ask.data["handoff"], ask.str("thread").unwrap_or_default());
    let id = ask.str("id").unwrap_or_default();
    let dir = dir(ctx, cfg, from);
    std::fs::create_dir_all(&dir)?;
    let lock = std::fs::File::create(dir.join(format!(".handoff-{id}.lock")))?;
    lock.lock()?;
    let events = shared(&ctx.state)?;
    let mine = |e: &&Event| e.kind == "thread-handoff" && e.str("thread") == Some(from) && e.str("ask") == Some(id);
    let failed = |e: &Event| {
        events
            .iter()
            .any(|f| f.kind == "ask-failed" && f.str("id") == Some(id) && f.seq > e.seq)
    };
    let (to, sent) = match events.iter().rfind(mine) {
        Some(e) if !failed(e) => return Ok((e.str("to").unwrap_or_default().into(), false)),
        Some(e) => {
            let to = e.str("to").unwrap_or_default();
            let msg = |m: &&Event| m.kind == "thread-msg" && m.seq > e.seq && m.str("from") == Some(from);
            let text = (of(&events, to).find(msg))
                .and_then(|m| m.str("text"))
                .unwrap_or_default();
            let line = format!("[{to}] {from}: {text}");
            (
                to.to_string(),
                deliver(ctx, cfg, r, find(&events, to)?, &line).map(|_| to.into()),
            )
        }
        None => {
            let opts = ask.data["options"].as_array().cloned().unwrap_or_default();
            let n = choice
                .parse::<usize>()
                .ok()
                .or_else(|| opts.iter().position(|o| o == choice).map(|i| i + 1));
            let to = match n.and_then(|n| h["targets"].get(n.checked_sub(1)?)?.as_str()) {
                Some(to) => to,
                None if is_id(choice) && choice != from => find(&events, choice).map(|_| choice)?,
                None => return refused("BAD_VALUE", "tap a row, or name a thread id"),
            };
            let s = |k: &str| h[k].as_str().unwrap_or_default();
            let send = || {
                send(
                    ctx,
                    cfg,
                    r,
                    (from, to, s("repo")),
                    (s("title"), s("text")),
                    (None, Some(id)),
                )
            };
            (to.to_string(), role(cfg, s("repo")).and_then(|_| send()))
        }
    };
    // #584: a handoff that fails to go out closes its card as failed; #621: its full error is a row in the source.
    if let Err(e) = &sent {
        let d = json!({"id": id, "to": to, "reason": format!("{e:#}")});
        emit(&ctx.state, cfg, Event::new(ctx.now, &ask.fleet, None, "ask-failed", d))?;
        let text = format!("✗ {id} not delivered to {to}: {e:#}");
        log(ctx, cfg, find(&events, from)?, "system", (&text, &[]))?;
    }
    sent.map(|to| (to, true))
}

/// A handoff from `from` into thread `to` (`new`: a new thread in `repo`): the linked `thread-handoff` row and the text
/// typed into its conductor, both naming the target as `<title> · <id>`.
fn send(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    (from, to, repo): (&str, &str, &str),
    (title, text): (&str, &str),
    (fwd, ask): (Option<u64>, Option<&str>),
) -> Result<String> {
    let to = if to == "new" {
        new(ctx, cfg, repo, title, None)?
    } else {
        to.into()
    };
    let target = format!("{} · {to}", self::title(&shared(&ctx.state)?, &to));
    let d = json!({"thread": from, "from": from, "to": to, "title": title, "text": text, "target": target,
        "forwards": fwd, "ask": ask});
    let t = find(&shared(&ctx.state)?, from)?.fleet.clone();
    emit(&ctx.state, cfg, Event::new(ctx.now, &t, None, "thread-handoff", d))?;
    say(
        ctx,
        cfg,
        r,
        (&to, &format!("handoff to {target}: {text}"), &[]),
        (from, false),
    )?;
    Ok(to)
}

/// #492: a handoff from `from` forwards the newest handoff it received that the owner has not written after and that
/// is not forwarded yet (its seq): the conductor sends on what was not meant for it.
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

/// #492: the seqs of the received handoff rows their thread sent on (a later handoff from that thread `forwards`
/// them). #620: one pass over the log; a row's check is a lookup.
pub fn forwarded(events: &[Event]) -> std::collections::BTreeSet<u64> {
    if let Some(l) = events::cached(events) {
        return l.threads.forwards.clone();
    }
    events
        .iter()
        .filter_map(|e| e.data.get("forwards").and_then(Value::as_u64))
        .collect()
}

/// #492: a handoff row's text: its title and the target as `<title> · <id>`.
pub fn handoff_text(e: &Event) -> String {
    let title = e.str("title").unwrap_or_default();
    e.str("target").map_or(title.into(), |t| format!("{title} → {t}"))
}

/// #541: the thread a lane's ask on `fleet` goes to: `[fleet.<n>] ask_thread`, else the repo's most recently active
/// one (neither archived); none without a conductor role. #633: an attached cooee thread is one of them.
pub fn route<'a>(cfg: &Config, events: &'a [Event], fleet: &str) -> Option<&'a str> {
    let own = role(cfg, fleet).is_ok();
    let repo = |e: &&'a Event| e.fleet == fleet && (own && e.kind == "thread-new" || e.kind == "orchestrator-attached");
    let ids = events.iter().filter(repo).filter_map(|e| e.str("thread"));
    let live = |id: &&str| match link_of(events, id) {
        Some(_) => attached(events, id).is_some_and(|e| e.fleet == fleet),
        None => !archived(events, id),
    };
    let ids: Vec<&str> = ids.filter(live).collect();
    let pin = cfg.fleet.get(fleet).and_then(|f| f.ask_thread.as_deref());
    let newest = || ids.iter().max_by_key(|id| of(events, id).map(|e| e.seq).max()).copied();
    ids.iter().find(|id| Some(**id) == pin).copied().or_else(newest)
}

/// #541: lane ask `a` typed into thread `id`'s orchestrator (started or resumed) as a system line the board hides.
pub fn lane_ask(ctx: &Ctx, cfg: &Config, r: &dyn Runner, id: &str, a: &Event) -> Result<String> {
    let s = |k| a.str(k).unwrap_or_default();
    let rec = a.data["recommend"].as_u64();
    let opts = a.data["options"].as_array().into_iter().flatten().zip(1..);
    let opts: String = opts
        .map(|(o, n)| {
            let r = if rec == Some(n) { " (recommended)" } else { "" };
            format!(" [{n}] {}{r}", o.as_str().unwrap_or_default())
        })
        .collect();
    let at = a.issue.map_or("the fleet".into(), |n| format!("#{n}"));
    let text = format!(
        "lane ask {} on {at} from {} ({}): {}{opts}. Answer it only if the issue spec or a past decision settles it: \
         `herdsman answer {} {} <n> --basis \"<verbatim spec quote | q<id>>\"`; else run it without --basis and it \
         goes to the owner.",
        s("id"),
        a.str("agent").unwrap_or("a lane"),
        s("kind"),
        s("question"),
        a.fleet,
        s("id")
    );
    let t = find(&shared(&ctx.state)?, id)?.clone();
    let d = json!({"thread": id, "from": "system", "text": text, "lane_ask": a.str("id")});
    emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, "thread-msg", d))?;
    deliver(ctx, cfg, r, &t, &format!("[{id}] system: {text}"))
}

/// #541: lane ask `a` put to the owner as a choice row in thread `id` (once), `rec` (else its own) recommended; it
/// pushes as an ask.
pub fn to_owner(ctx: &Ctx, cfg: &Config, id: &str, a: &Event, rec: Option<u64>, why: &str) -> Result<()> {
    let events = shared(&ctx.state)?;
    if of(&events, id).any(|e| e.kind == "ask-owner" && e.str("id") == a.str("id")) {
        return Ok(());
    }
    let rec = rec.or_else(|| a.data["recommend"].as_u64());
    let d = json!({"thread": id, "id": a.str("id"), "question": a.str("question"), "options": a.data["options"],
        "recommend": rec, "agent": a.str("agent"), "why": why});
    emit(&ctx.state, cfg, Event::new(ctx.now, &a.fleet, a.issue, "ask-owner", d))
}

/// #541: an `ask-owner` row's lane ask.
pub fn lane_of<'a>(events: &'a [Event], e: &Event) -> Option<&'a Event> {
    events::ask_event(events, "needs-decision", &e.fleet, e.str("id").unwrap_or_default())
}

/// #413: a one-line row from thread `id` into each thread that handed off to it (an issue filed, an ask answered).
pub fn flow(ctx: &Ctx, cfg: &Config, id: &str, text: &str) -> Result<()> {
    let events = shared(&ctx.state)?;
    let to_id = |e: &&Event| e.kind == "thread-handoff" && e.str("to") == Some(id);
    let srcs: std::collections::BTreeSet<_> = events.iter().filter(to_id).filter_map(|e| e.str("from")).collect();
    for s in srcs {
        let d = json!({"thread": s, "from": "system", "via": id, "text": text});
        let e = Event::new(ctx.now, &find(&events, s)?.fleet, None, "thread-msg", d);
        emit(&ctx.state, cfg, e)?;
    }
    Ok(())
}

/// A thread id: `t` and a number.
fn is_id(s: &str) -> bool {
    s.len() > 1 && s.starts_with('t') && s[1..].bytes().all(|b| b.is_ascii_digit())
}

/// #413: a linked row's chip: `from t5` (a message or flow-back row from t5) or `continued in t6` (a handoff).
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

fn log(ctx: &Ctx, cfg: &Config, t: &Event, from: &str, (text, files): (&str, &[Value])) -> Result<()> {
    let mut d = json!({"thread": t.str("thread"), "from": from, "text": text});
    if !files.is_empty() {
        d["files"] = json!(files);
    }
    emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, "thread-msg", d))
}

/// The thread's conductor's working folder, one per thread: `<state dir>/conductor/<thread>/`, or (#386)
/// `<[conductor] dir>/<thread>/`.
pub fn dir(ctx: &Ctx, cfg: &Config, id: &str) -> std::path::PathBuf {
    let (new, old) = (
        ctx.state.with_file_name("orchestrator"),
        ctx.state.with_file_name("conductor"),
    );
    let own = if !new.exists() && old.exists() { old } else { new };
    cfg.orchestrator.dir.as_deref().map_or(own, expand).join(id)
}

/// `p` resolved, if it lies in thread `id`'s orchestrator folder: the only files a thread shows or serves (#386).
pub fn inside(ctx: &Ctx, cfg: &Config, id: &str, p: &std::path::Path) -> Option<std::path::PathBuf> {
    let root = dir(ctx, cfg, id).canonicalize().ok()?;
    p.canonicalize().ok().filter(|p| p.starts_with(root))
}

/// A file a message carries, as the thread logs it: its name, path in the orchestrator's folder and size.
pub fn file(ctx: &Ctx, cfg: &Config, t: &Event, p: &std::path::Path) -> Result<Value> {
    let id = t.str("thread").unwrap_or_default();
    let p = match inside(ctx, cfg, id, p) {
        Some(p) => p,
        None => copy_in(ctx, cfg, t, p)?,
    };
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
        let d = dir(ctx, cfg, id).display().to_string();
        bail!(
            "{}: not in the thread's folder {d}, a lane worktree of fleet {} or its repo checkout",
            p.display(),
            t.fleet
        );
    };
    let to = dir(ctx, cfg, id).join("files");
    std::fs::create_dir_all(&to)?;
    let to = to.join(name);
    let _ = std::fs::remove_file(&to);
    std::fs::copy(&from, &to).with_context(|| from.display().to_string())?;
    to.canonicalize().map_err(Into::into)
}

/// `herdsman thread reply [--file <path>]…`: the orchestrator's message to the owner, its files shown inline.
pub fn reply(ctx: &Ctx, cfg: &Config, id: &str, text: &str, files: &[std::path::PathBuf]) -> Result<()> {
    text_ok(text)?;
    let events = shared(&ctx.state)?;
    let t = find(&events, id)?;
    let files = files.iter().map(|p| file(ctx, cfg, t, p)).collect::<Result<Vec<_>>>()?;
    log(ctx, cfg, t, "orchestrator", (text, &files))
}

/// `herdsman thread say` (and a thread ask's answer, `raw`: typed as is; #386: the board's, with `files`): log
/// `from`'s message, then type it into the thread's conductor, starting or resuming it first; the agent it went to.
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
    /// #715: set inside `deferred`: `say` only logs, leaving its delivery here.
    static LATER: std::cell::RefCell<Option<Vec<(Event, String)>>> = const { std::cell::RefCell::new(None) };
}

/// #715: `f` with every `say` in it logging only (a board request never waits on a conductor start); the
/// deliveries it left, for the caller to type in once its reply is out.
pub fn deferred<T>(f: impl FnOnce() -> T) -> (T, Vec<(Event, String)>) {
    LATER.set(Some(vec![]));
    let out = f();
    (out, LATER.take().unwrap_or_default())
}

/// #403: `say`'s first half: log the message; the thread and the line `deliver` types.
pub fn post(
    ctx: &Ctx,
    cfg: &Config,
    (id, text, files): (&str, &str, &[Value]),
    (from, raw): (&str, bool),
) -> Result<(Event, String)> {
    text_ok(text)?;
    let t = find(&shared(&ctx.state)?, id)?.clone();
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

/// #403: `say`'s second half: type `line` into thread `t`'s orchestrator, starting or resuming it first.
pub fn deliver(ctx: &Ctx, cfg: &Config, r: &dyn Runner, t: &Event, line: &str) -> Result<String> {
    let id = t.str("thread").unwrap_or_default();
    // #477: one delivery per thread at a time (across processes too): a message waits for a start in flight.
    let dir = dir(ctx, cfg, id);
    std::fs::create_dir_all(&dir)?;
    let lock = std::fs::File::create(dir.join(".deliver.lock"))?;
    // #548: a delivery stuck past the bound (a Keychain dialog) fails the next one instead of hanging it; #623: the
    // bound covers a slow start's wait for its agent.
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
    // #623: a failed delivery (not a refusal) is tried once more before the thread shows it.
    once(ctx, cfg, r, t, line).or_else(|e| match e.downcast_ref::<crate::gate::Refusal>() {
        Some(_) => Err(e),
        None => once(ctx, cfg, r, t, line),
    })
}

/// #623: how long a timed-out start waits for its agent to show up (twice that while the host is busy).
const START_SECS: u64 = 120;

/// One try at `deliver`, under its lock.
fn once(ctx: &Ctx, cfg: &Config, r: &dyn Runner, t: &Event, line: &str) -> Result<String> {
    let id = t.str("thread").unwrap_or_default();
    let events = shared(&ctx.state)?;
    let last = of(&events, id).rfind(|e| e.kind.starts_with("orchestrator-"));
    let agents = herdr::agents(r)?;
    let awake = last.filter(|s| s.kind == "orchestrator-start" && agents.iter().any(|a| a["name"] == s.data["agent"]));
    // #404: a model/effort pick since the conductor started resumes it on them.
    let picked = awake.is_some_and(|s| of(&events, id).any(|e| e.kind == "thread-model" && e.seq > s.seq));
    let Some(s) = awake.filter(|_| !picked) else {
        if let Some(s) = awake {
            close(r, s)?;
            let d = json!({"thread": id, "agent": s.str("agent"), "why": "model"});
            emit(
                &ctx.state,
                cfg,
                Event::new(ctx.now, &t.fleet, None, "orchestrator-sleep", d),
            )?;
        }
        return start(ctx, cfg, r, t, &shared(&ctx.state)?, line);
    };
    let (agent, pane) = (s.str("agent").unwrap_or_default(), s.str("pane").unwrap_or_default());
    // #608: an ACP conductor takes the line as one prompt, multi-line as it is (#397's one line is herdr's).
    if acp(s) {
        crate::shed::cli(r, &["send", agent, line])?;
        return Ok(agent.into());
    }
    herdr::reply(
        r,
        s.str("harness") == Some("dsh"),
        agent,
        pane,
        &line.replace('\n', " "),
    )?;
    Ok(agent.into())
}

/// #608: a conductor that runs as an ACP session in shed.
fn acp(s: &Event) -> bool {
    s.str("runtime") == Some("shed-acp")
}

/// Close conductor `s`: its shed terminal (#608), else its herdr workspace.
fn close(r: &dyn Runner, s: &Event) -> Result<()> {
    if acp(s) {
        return herdr::close(r, s.str("agent").unwrap_or_default()).map(drop);
    }
    herdr::workspace_close(r, s.str("workspace").unwrap_or_default()).map(drop)
}

/// The conductor role: the thread fleet's `[fleet.<n>.role.conductor]` (#401: else its impl role on profile
/// `lane-conductor`, `Config::parse`); for `all`, the first fleet's that has one.
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

/// #404: a new thread's conductor: the role, with `[chat]`'s harness (another harness drops the role's model and
/// accounts), model, effort and account(s) over it.
pub fn defaults<'a>(cfg: &'a Config, repo: &str) -> Result<(&'a str, Role)> {
    let (fleet, base) = role(cfg, repo)?;
    let c = &cfg.chat;
    let mut r = on(base, c.harness.as_deref());
    r.model = c.model.clone().or(r.model);
    r.effort = c.effort.clone().or(r.effort);
    if c.account.is_some() || !c.accounts.is_empty() {
        (r.account, r.accounts) = (c.account.clone(), c.accounts.clone());
    }
    Ok((fleet, r))
}

/// `r` on harness `h`: unchanged on its own, else without its model and accounts.
fn on(r: &Role, h: Option<&str>) -> Role {
    match h.filter(|h| *h != r.harness) {
        Some(h) => Role {
            harness: h.into(),
            model: None,
            account: None,
            accounts: vec![],
            ..r.clone()
        },
        None => r.clone(),
    }
}

/// #404: thread `t`'s conductor: the defaults, or the role on another harness it started with; then its last
/// model/effort pick.
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

/// #404: the models a thread on harness `h` can pick: those its roles on `h` (or `[chat]`, where a fleet's default
/// conductor is on `h`) use, any fleet;
/// none where `h`'s profile owns the model.
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
    let all: std::collections::BTreeSet<_> = roles.filter_map(|r| r.model.as_deref()).chain(chat).collect();
    all.into_iter().collect()
}

/// #404: the composer chip's pick: thread `id`'s model and effort from its next turn (`deliver` resumes the
/// conductor on them), logged `thread-model`, a system line in the thread.
pub fn pick(ctx: &Ctx, cfg: &Config, id: &str, model: &str, effort: &str) -> Result<()> {
    let events = shared(&ctx.state)?;
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
        d[k] = json!(v);
    }
    emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, "thread-model", d))
}

/// #401: the models a fleet's roles use, the [config] sheet's orchestrator model choices.
pub fn models(f: &crate::config::Fleet) -> Vec<&str> {
    let all: std::collections::BTreeSet<_> = f.role.values().filter_map(|r| r.model.as_deref()).collect();
    all.into_iter().collect()
}

/// #401: the [config] sheet's conductor choices (harness, model, effort) into `[fleet.<n>.role.conductor]` in place (a
/// fleet on the default gets the table: its impl role's keys); refused `DOCTOR` if `doctor` with the new file shows a ✗
/// it did not before. The old file goes to `fleet.toml.bak` first.
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

/// #555: `[chat]`'s new-thread defaults (harness, model, effort) in place (the table added when missing), each checked
/// as fleet.toml's load checks it (`BAD_VALUE`), then written as `set` writes; logs `set` (role `chat`) per key.
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

/// fleet.toml as `new`, unless `doctor` with it shows a ✗ it did not before (`DOCTOR`); the old file goes to
/// `fleet.toml.bak` first.
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

/// A random v4 UUID, a new conductor's session id.
fn uuid() -> Result<String> {
    let mut b = crate::devices::random(16)?;
    (b[6], b[8]) = (b[6] & 0x0f | 0x40, b[8] & 0x3f | 0x80);
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    ))
}

/// Start the thread's conductor with `line`: its first start, or a resume of its session (a harness that can't
/// resume is pointed at the conversation so far instead).
fn start(ctx: &Ctx, cfg: &Config, r: &dyn Runner, t: &Event, events: &[Event], line: &str) -> Result<String> {
    let id = t.str("thread").unwrap_or_default();
    let (fleet, base) = thread_role(cfg, events, t)?;
    let f = cfg.fleet(fleet)?;
    let (role, resting) = crate::access::fallback(cfg, events, &base, ctx.now)?;
    let h = providers::get(&role.harness);
    // Only a session this harness started resumes (a fallback to another harness starts its own).
    // #608: an ACP session is never resumed (shed's bridge loads none): it starts fresh with the thread file.
    let first =
        of(events, id).find(|e| e.kind == "orchestrator-start" && e.str("harness") == Some(&role.harness) && !acp(e));
    let session = first
        .and_then(|e| e.str("session"))
        .map_or_else(uuid, |s| Ok(s.into()))?;
    let resume = first.is_some() && h.resumes();
    let dir = dir(ctx, cfg, id);
    let brief = dir.join(".brief.md");
    // #397: the kick is one line (herdr refuses a multi-line `agent start` argument); the thread is in a file.
    let on = crate::hosts::On::new(r, None);
    let thread = dir.join(".thread.md");
    on.write(&thread, &render(&items(events, id), name(cfg, id_of(t))))?;
    // #621: herdr types the start into a shell that takes lines of at most 1,024 bytes: a long line stays in the file.
    let kick = if resume && line.len() <= 400 {
        line.replace(char::is_control, " ")
    } else {
        format!(
            "Read {} and act on it. The thread so far is in {}, the last message new.",
            brief.display(),
            thread.display()
        )
    };
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
        account: crate::access::pick(ctx, cfg, events, &role)?,
        env: f.env.clone(),
        port: crate::spawn::lane_port(f.port_range.as_deref(), &format!("{fleet}/{agent}"), &[]),
        mcp: crate::access::mcp_servers(cfg, f, &role)?,
        mcp_file: dir.join(".mcp.lane.json"),
        title: crate::spawn::lane_title(&head, t.str("title").unwrap_or_default()),
        head,
        resting,
        session: Some((session.clone(), resume)),
        lane_key: None,
        token: None,
    };
    p.env.insert("HERDSMAN_THREAD".into(), id.into());
    p.env.insert("HERDSMAN_ORCHESTRATOR".into(), id_of(t).into());
    // #683: the conductor runs on this host: its account's env is its sign-in.
    crate::access::signed(ctx, cfg, r, (fleet, &role, false), &mut p)?;
    // #608: a shed-acp fleet's conductor is an ACP session under the same guard as its lanes (#565), or none at all.
    let shed = cfg.shed_for(fleet).filter(|b| b.acp.is_some());
    let acp = match shed.map(|b| (b.acp.clone().unwrap_or_default(), herdr::Shed(b))) {
        Some((aid, b)) => match crate::spawn::acp_cmd(ctx, r, &p, &aid, None) {
            Ok(cmd) => Some((b, cmd)),
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
        },
        None => None,
    };
    h.ready(ctx, r, &p)?;
    let text = profile(cfg, t, fleet);
    on.write(&brief, &text)?;
    h.prepare(ctx, &on, &p)?;
    let _secrets = crate::spawn::deliver(ctx, &on, cfg, &p)?;
    if let Some((b, cmd)) = acp {
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
        b.start(r, &agent, &labels, &dir.to_string_lossy(), &cmd)?;
        let kick = format!("The thread so far is in {}, the last message new.", thread.display());
        let kicked = b.kick_acp(r, &agent, &format!("{}\n\n{kick}\n\n{line}", text.trim_end()))?;
        let d = json!({"thread": id, "agent": agent, "pane": agent, "runtime": "shed-acp", "harness": p.harness,
            "model": p.model, "effort": p.effort, "account": p.account.as_ref().map(|a| &a.0), "resumed": false,
            "confirmed": kicked});
        return started(ctx, cfg, r, t, &p, d);
    }
    let ws = |p: &str| p.split(':').next().unwrap_or(p).to_string();
    let mine =
        |a: &&Value| a["name"] == agent.as_str() && a["cwd"].as_str().map(std::path::Path::new) == Some(dir.as_path());
    let find = |list: &[Value]| {
        list.iter()
            .find(mine)
            .and_then(|a| a["pane_id"].as_str())
            .map(String::from)
    };
    let (mut retried, mut answered, mut screen) = (false, false, String::new());
    let (new, o, up) = loop {
        let new = herdr::workspace_create(r, &dir.to_string_lossy(), &agent)?;
        herdr::ok(r, &p.lane_cmd(&new))?;
        let o = herdr::exec(r, &p.start_cmd(ctx, &new))?;
        if o.code == 0 {
            break (new, o, None);
        }
        // #477: a start that timed out (or found its name taken) whose agent is up in the thread's folder is adopted.
        let has = |c: &str| herdr::failed_with(&o, c);
        // #736: a first-start prompt (an account's config folder that never trusted this one) is answered as spawn's.
        if has("agent_not_ready") {
            let d = |prompt: &str| json!({"thread": id, "agent": agent, "pane": new, "prompt": prompt});
            let ev = |prompt: &str| Event::new(ctx.now, &t.fleet, None, "startup-prompt", d(prompt));
            (answered, screen) = crate::spawn::unblock(r, h, &new, |p| emit(&ctx.state, cfg, ev(p)))?;
            if answered {
                break (new.clone(), o, Some(new));
            }
        }
        let lost = has("agent_name_not_found") || herdr::message(&o).contains("no longer owns the target terminal");
        let seen = if has("timeout") || has("agent_name_taken") || lost {
            herdr::agents(r)?
        } else {
            vec![]
        };
        let mut up = find(&seen);
        // #623: on a loaded host the agent may still be coming up: poll for it (backoff) up to the bound.
        if up.is_none() && has("timeout") {
            let busy = crate::host::admit(&crate::host::sample(r), &cfg.limits).is_err();
            let (bound, mut waited, mut step) = (START_SECS << u8::from(busy), 0, 1);
            while up.is_none() && waited < bound {
                r.sleep(step);
                (waited, step) = (waited + step, (step * 2).min(10));
                up = find(&herdr::agents(r).unwrap_or_default());
            }
        }
        if up.as_deref().is_none_or(|p| ws(p) != ws(&new)) {
            let _ = herdr::workspace_close(r, &ws(&new));
        }
        // #623: a name a dead terminal holds (no live agent has it): clear its last workspace and start once more.
        let taken = has("agent_name_taken") && !seen.iter().any(|a| a["name"] == agent.as_str());
        if up.is_none() && (lost || taken) && !retried {
            let prev = of(events, id).rfind(|e| e.kind == "orchestrator-start");
            if let Some(w) = prev.and_then(|e| e.str("workspace")).filter(|w| *w != ws(&new)) {
                let _ = herdr::workspace_close(r, w);
            }
            retried = true;
            continue;
        }
        break (new, o, up);
    };
    let pane = match (o.code, up) {
        (0, _) => new.clone(),
        (_, Some(p)) => p,
        _ if screen.is_empty() => bail!("herdr agent start failed: {}", herdr::message(&o)),
        _ => bail!("herdr agent start failed: {}\nscreen:\n{screen}", herdr::message(&o)),
    };
    // Its own new pane gets the kick; one elsewhere (the name taken) or a kick that didn't land gets the line.
    let kicked = pane == new && h.kick(r, &p, &pane).unwrap_or(false);
    if o.code != 0 && !kicked {
        herdr::reply(r, p.harness == "dsh", &agent, &pane, &line.replace('\n', " "))?;
    }
    let mut d = json!({"thread": id, "agent": agent, "pane": pane, "workspace": ws(&pane), "harness": p.harness, "model": p.model, "effort": p.effort,
        "account": p.account.as_ref().map(|a| &a.0), "session": session, "resumed": resume, "confirmed": kicked});
    if o.code != 0 && !answered {
        d["adopted"] = json!(true);
    }
    started(ctx, cfg, r, t, &p, d)
}

/// Log conductor start `d` (and a fallback's `switched to` line); the agent.
fn started(ctx: &Ctx, cfg: &Config, r: &dyn Runner, t: &Event, p: &Plan, d: Value) -> Result<String> {
    let agent = d["agent"].as_str().unwrap_or_default().to_string();
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, &t.fleet, None, "orchestrator-start", d),
    )?;
    if let Some((on, until)) = &p.resting {
        log(
            ctx,
            cfg,
            t,
            "system",
            (
                &format!("switched to {} {})", &on[3..], crate::usage::date(r, *until, "+%H:%M")),
                &[],
            ),
        )?;
    }
    Ok(agent)
}

/// The conductor's `.brief.md`: its thread, then `[conductor]`'s instructions, memory notes and skills index.
fn profile(cfg: &Config, t: &Event, fleet: &str) -> String {
    let (id, title) = (t.str("thread").unwrap_or_default(), t.str("title").unwrap_or_default());
    let repo = cfg.fleet.get(&t.fleet).map_or("all repos".into(), |f| {
        format!("{} (fleet {}, checkout {})", f.repo, t.fleet, f.local)
    });
    let mut s = format!(
        "# Orchestrator brief: thread {id}\n\n{}\n\nThread: {id}, {title}\nRepo: {repo}\nFleet for asks: {fleet}\n",
        crate::spawn::role_prompt("orchestrator").trim_end()
    );
    let c = &cfg.orchestrator;
    if let Some(p) = &c.instructions {
        let text = std::fs::read_to_string(expand(p)).unwrap_or_else(|e| format!("({p} unreadable: {e})"));
        s += &format!("\n## Instructions ({p})\n\n{}\n", text.trim_end());
    }
    let files = |d: &Option<String>| {
        let all = d.as_deref().and_then(|d| std::fs::read_dir(expand(d)).ok());
        let mut v: Vec<_> = all.into_iter().flatten().flatten().map(|e| e.path()).collect();
        v.sort();
        v
    };
    for m in files(&c.memory)
        .iter()
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
    {
        let text = std::fs::read_to_string(m).unwrap_or_default();
        s += &format!("\n## Memory: {}\n\n{}\n", m.display(), text.trim_end());
    }
    // #538: what this fleet lets the conductor run.
    s += "\n## Capability groups ([orchestrator] allow)\n\n";
    for (g, verbs) in crate::config::GROUPS {
        let on = ["off", "on"][usize::from(c.groups().contains(g))];
        s += &format!("- `{g}` ({on}): {verbs}\n");
    }
    s += "\nA verb of an off group is refused `GROUP_OFF`: propose `[orchestrator] allow` with `herdsman config propose`.\n";
    let skills = files(&c.skills);
    if !skills.is_empty() {
        s += "\n## Skills (read one when it applies)\n\n";
        s += &skills
            .iter()
            .map(|p| format!("- {}\n", p.display()))
            .collect::<String>();
    }
    s
}

/// The thread as items: messages (`from`, `text`) and asks (`ask`, `question`, numbered `options` with the
/// recommended one first, `open`).
pub fn items(events: &[Event], id: &str) -> Vec<Value> {
    let sent = forwarded(events);
    let item = |e: &Event| match e.kind.as_str() {
        "thread-msg" => {
            let mut m = json!({"ts": e.ts, "from": e.str("from"), "text": e.str("text")});
            if let Some(f) = e.data.get("files") {
                m["files"] = f.clone();
            }
            if let Some((w, t)) = link(e) {
                m["link"] = json!(format!("{w} {t}"));
            }
            if sent.contains(&e.seq) {
                m["text"] = json!(format!("{} (forwarded)", e.str("text").unwrap_or_default()));
            }
            Some(m)
        }
        "thread-model" => Some(json!({"ts": e.ts, "from": "system", "text": picked(e)})),
        // #538: a run card; its settled line follows as a system message.
        "run-card" => Some(json!({"ts": e.ts, "from": "orchestrator", "text": crate::card::text(e)})),
        // #461: a conductor's `timer install`, a system line.
        "timer-install" => {
            let (by, w, l) = (e.str("by").unwrap_or("?"), &e.data["written"], &e.data["reloaded"]);
            let text = format!("{by} ran `herdsman timer install`: wrote {w}, reloaded {l}");
            Some(json!({"ts": e.ts, "from": "system", "text": text}))
        }
        "thread-handoff" => Some(
            json!({"ts": e.ts, "from": "system", "text": handoff_text(e), "link": format!("continued in {}", e.str("to").unwrap_or_default())}),
        ),
        "needs-decision" | "ask-owner" => {
            let all = e.data["options"].as_array().cloned().unwrap_or_default();
            let mut opts: Vec<Value> = all
                .into_iter()
                .zip(1..)
                .map(|(t, n)| json!({"n": n, "text": t}))
                .collect();
            let rec = e.data["recommend"].as_u64().unwrap_or(0);
            opts.sort_by_key(|o| o["n"] != rec);
            let lane = if e.kind == "ask-owner" { lane_of(events, e)? } else { e };
            let open = events::closed(events, lane).is_none();
            Some(
                json!({"ts": e.ts, "from": "orchestrator", "ask": e.str("id"), "question": e.str("question"),
                "options": opts, "recommend": e.data["recommend"], "open": open}),
            )
        }
        _ => None,
    };
    of(events, id).filter_map(item).collect()
}

/// #404: a pick's system line: `model sonnet · effort high from the next turn`.
pub fn picked(e: &Event) -> String {
    let kv = ["model", "effort"].map(|k| e.str(k).map(|v| format!("{k} {v}")));
    format!(
        "{} from the next turn",
        kv.into_iter().flatten().collect::<Vec<_>>().join(" · ")
    )
}

/// Items as lines: `<from>: <text>`, an ask as `<from> asks <id>: <question>` and its `[n]` options.
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

/// `herdsman thread show <id>`: the conversation; marks it read.
pub fn show(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, id: &str, as_json: bool) -> Result<()> {
    let events = shared(&ctx.state)?;
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

/// `herdsman thread ls [fleet]`: per repo, its threads with title, last message, unread dot and conductor state.
pub fn ls(ctx: &Ctx, cfg: &Config, out: &mut dyn Write, fleet: Option<&str>, as_json: bool) -> Result<()> {
    let events = shared(&ctx.state)?;
    let mut rows: Vec<Value> = vec![];
    for t in events
        .iter()
        .filter(|e| e.kind == "thread-new" && fleet.is_none_or(|f| f == e.fleet))
    {
        let id = t.str("thread").unwrap_or_default();
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

/// `check`: each awake conductor whose thread has been quiet `IDLE` and that is not working is closed
/// (`conductor-sleep`); the next message resumes it.
pub fn sleep_idle(ctx: &Ctx, cfg: &Config, r: &dyn Runner, events: &[Event], agents: &[Value]) -> Result<()> {
    for t in events.iter().filter(|e| e.kind == "thread-new") {
        let id = t.str("thread").unwrap_or_default();
        let last = of(events, id).rfind(|e| e.kind.starts_with("orchestrator-"));
        let Some(s) = last.filter(|e| e.kind == "orchestrator-start") else {
            continue;
        };
        let quiet = of(events, id).map(|e| e.ts).max().unwrap_or(0) + IDLE <= ctx.now;
        let a = agents.iter().find(|a| a["name"] == s.data["agent"]);
        if a.is_some() && switch(ctx, cfg, r, t, s)? {
            continue;
        }
        // #608: an ACP conductor shed no longer runs is asleep at once (the board's dot is shed's state).
        let gone = a.is_none() && acp(s);
        if !gone && (!quiet || a.is_some_and(|a| a["agent_status"] == "working")) {
            continue;
        }
        if a.is_some() {
            close(r, s)?;
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

/// #387: an awake conductor showing a usage limit (or signed out) rests its account (else its harness) and resumes
/// the same session on the role's next account or `fallback` entry; signed out, the thread gets a [sign in] line.
fn switch(ctx: &Ctx, cfg: &Config, r: &dyn Runner, t: &Event, s: &Event) -> Result<bool> {
    let (id, account, hn) = (
        t.str("thread").unwrap_or_default(),
        s.str("account"),
        s.str("harness").unwrap_or_default(),
    );
    // No account and no `fallback`: nothing to switch to, but a sign-out still gets its [sign in] line.
    let chain = account.is_some() || role(cfg, &t.fleet).is_ok_and(|r| !r.1.fallback.is_empty());
    let (screen, h) = (
        herdr::read(r, s.str("pane").unwrap_or_default()).unwrap_or_default(),
        providers::get(hn),
    );
    // #640: an account's banner caps only when its usage reading agrees; a cap is never a sign-out.
    let limit = chain.then(|| h.usage_limited(cfg, r, &screen, ctx.now)).flatten();
    let seen = limit.map(|u| crate::access::confirm(ctx, account, u));
    let (seen, limit) = seen.map_or((None, None), |s| (Some(s.0), s.1));
    let out = seen.map_or(h.signed_out(&screen), |k| k == "account-signed-out");
    let rest = out.then(|| ctx.now + cfg.limits.fallback_rest_minutes * 60);
    let Some(until) = limit.or(rest) else {
        return Ok(false);
    };
    if chain {
        let kind = ["account-capped", "account-signed-out"][usize::from(out)];
        let kind = account.map_or("agent-unavailable", |_| kind);
        let until = (!out || account.is_none()).then_some(until);
        let d = json!({"agent": s.str("agent"), "harness": hn, "account": account, "until": until});
        emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, kind, d))?;
    }
    close(r, s)?;
    let why = if out { "signed out" } else { "usage limit" };
    let d = json!({"thread": id, "agent": s.str("agent"), "why": why});
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, &t.fleet, None, "orchestrator-sleep", d),
    )?;
    if out {
        let text = format!(
            "{hn} is signed out{}: sign in",
            account.map_or(String::new(), |a| format!(" (account {a})"))
        );
        let named = account.map(|a| format!("{hn} {a}"));
        let su = named.filter(|n| crate::setup::all(cfg).iter().any(|s| &s.0 == n));
        let d = json!({"thread": id, "from": "system", "text": text, "signin": su.as_deref().unwrap_or(hn)});
        emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, "thread-msg", d))?;
    }
    if !chain {
        return Ok(true);
    }
    let line = format!("[{id}] system: {why}; you resumed on the next account. Carry on where you left off.");
    if let Err(e) = start(ctx, cfg, r, t, &shared(&ctx.state)?, &line) {
        log(
            ctx,
            cfg,
            t,
            "system",
            (&format!("the orchestrator stays asleep: {e:#}"), &[]),
        )?;
    }
    Ok(true)
}
