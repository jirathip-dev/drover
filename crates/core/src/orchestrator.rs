use crate::config::{expand, Config, Role};
use crate::events::{self, emit, working, Event, Json};
use crate::gate::refused;
use crate::runner::Runner;
use crate::thread::find;
use crate::{providers, term, Ctx};
use anyhow::Result;
use serde_json::{json, Value};

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
    let events = working(&ctx.state)?;
    if find(&events, id).is_ok() && !archived(&events, id) {
        return refused(
            "BAD_VALUE",
            format!("{id} is an open drover thread; attach it once it is archived"),
        );
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
    let (_, role) = self::role(cfg, if any { "all" } else { fleet })?;
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

/// `check`: each open drover thread gets its closing row, its orchestrator ends, and it is archived (`by: migration`); once.
pub fn retire(ctx: &Ctx, cfg: &Config, r: &dyn Runner, events: &[Event]) -> Result<()> {
    for t in crate::thread::threads(events) {
        let id = t.str("thread").unwrap_or_default();
        if archived(events, id) {
            continue;
        }
        if !of(events, id).any(|e| e.val("closing") == true) {
            let text = format!(
                "drover threads have ended: attach {id} in a client (cooee, or `drover attach {} {id}`) to carry on \
                 there; the issues it filed route to that chat, and its history stays readable",
                t.fleet
            );
            let d = json!({"thread": id, "from": "system", "text": text, "closing": true});
            emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, "thread-msg", d))?;
        }
        let last = of(events, id).rfind(|e| e.kind.starts_with("orchestrator-"));
        if let Some(s) = last.filter(|s| s.kind == "orchestrator-start") {
            term::close(ctx, r, s.str("agent").unwrap_or_default()).ok();
            let d = json!({"thread": id, "agent": s.str("agent"), "why": "archived"});
            emit(
                &ctx.state,
                cfg,
                Event::new(ctx.now, &t.fleet, None, "orchestrator-sleep", d),
            )?;
        }
        let d = json!({"thread": id, "by": "migration"});
        emit(
            &ctx.state,
            cfg,
            Event::new(ctx.now, &t.fleet, None, "thread-archive", d),
        )?;
    }
    Ok(())
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

/// Thread `t`'s orchestrator id (its first event's).
pub fn id_of(t: &Event) -> &str {
    t.str("orchestrator").unwrap_or(DEFAULT)
}

/// The name of thread `id`'s orchestrator, or of orchestrator `id` itself.
pub fn name_of<'a>(cfg: &'a Config, events: &[Event], id: &str) -> &'a str {
    name(cfg, find(events, id).map_or(id, id_of))
}

/// The thread is archived (its last archive or restore an archive).
pub fn archived(events: &[Event], id: &str) -> bool {
    let a = of(events, id).rfind(|e| matches!(e.kind.as_str(), "thread-archive" | "thread-restore"));
    a.is_some_and(|e| e.kind == "thread-archive")
}

/// `thread handoff`: from attached chat `from` to attached chat `to`, or to repo `to`'s (or `all`'s) desk row, at once, linked.
pub fn handoff(ctx: &Ctx, cfg: &Config, r: &dyn Runner, [from, to, title, text]: [&str; 4]) -> Result<String> {
    text_ok(text)?;
    let events = working(&ctx.state)?;
    chat_of(&events, from)?;
    let fwd = forwards(&events, from);
    let desk = to == "all" || cfg.fleet.contains_key(to);
    if desk {
        role(cfg, to)?;
        if title.trim().is_empty() || title.chars().any(char::is_control) {
            return refused("BAD_VALUE", "a handoff has a one-line title");
        }
    } else {
        role(cfg, &chat_of(&events, to)?.fleet)?;
    }
    if to == from {
        return refused("BAD_VALUE", "a chat hands off to another chat or a repo");
    }
    send(ctx, cfg, r, (from, to), (title, text), (fwd, None))
}

fn chat_of<'a>(events: &'a [Event], id: &str) -> Result<&'a Event> {
    match attached(events, id) {
        Some(t) => Ok(t),
        None => refused("NO_SUCH_THREAD", format!("{id} is no attached chat")),
    }
}

/// The handoff that moved handoff `seq` on, if one did.
pub fn moved(events: &[Event], seq: u64) -> Option<&Event> {
    events
        .iter()
        .find(|e| e.kind == "thread-handoff" && e.val("moves").as_u64() == Some(seq))
}

/// [move] on handoff `seq`: sent again to attached chat `to` from its sender; the first one is marked forwarded and its chat told to drop it.
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
    role(cfg, &chat_of(&events, to)?.fleet)?;
    let sent = send(
        ctx,
        cfg,
        r,
        (s("from"), to),
        (s("title"), s("text")),
        (Some(seq), Some(seq)),
    )?;
    let line = format!("handoff \"{}\" moved to {to}: don't act on it here", s("title"));
    crate::thread::route::tell(ctx, cfg, r, s("to"), &line);
    Ok(sent)
}

fn send(
    ctx: &Ctx,
    cfg: &Config,
    _: &dyn Runner,
    (from, to): (&str, &str),
    (title, text): (&str, &str),
    (fwd, moves): (Option<u64>, Option<u64>),
) -> Result<String> {
    let events = working(&ctx.state)?;
    let desk = attached(&events, to).is_none();
    let d = json!({"thread": from, "from": from, "to": to, "title": title, "text": text, "target": to,
        "forwards": fwd, "moves": moves, "desk": desk});
    let t = chat_of(&events, from)?.fleet.clone();
    emit(&ctx.state, cfg, Event::new(ctx.now, &t, None, "thread-handoff", d))?;
    Ok(to.into())
}

fn forwards(events: &[Event], from: &str) -> Option<u64> {
    let owner = |e: &Event| e.kind == "thread-msg" && e.str("thread") == Some(from) && e.str("from") == Some("owner");
    let sent = forwarded(events);
    let got = |e: &&Event| e.kind == "thread-handoff" && e.str("to") == Some(from) && !sent.contains(&e.seq);
    events.iter().rev().take_while(|e| !owner(e)).find(got).map(|e| e.seq)
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

pub fn text_ok(text: &str) -> Result<()> {
    if text.trim().is_empty() || text.chars().count() > 20_000 {
        return refused("BAD_VALUE", "a message is non-empty text, at most 20,000 chars");
    }
    Ok(())
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
