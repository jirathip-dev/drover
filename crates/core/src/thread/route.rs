use crate::config::Config;
use crate::events::{emit, working, Event};
use crate::orchestrator::{attached, of};
use crate::runner::Runner;
use crate::thread::{Registry, Target};
use crate::Ctx;
use anyhow::Result;
use serde_json::json;

/// Where `fleet` #`issue`'s rows go: the open thread that filed or queued it, else `{"k": "desk", "repo": fleet}`.
pub fn home(events: &[Event], fleet: &str, issue: Option<u64>) -> Target {
    Registry::read(events).home(fleet, issue)
}

/// The chat a lane's ask on `fleet` goes to: `[fleet.<n>] ask_thread`, else the repo's most recently active attached chat.
pub fn route<'a>(cfg: &Config, events: &'a [Event], fleet: &str) -> Option<&'a str> {
    let chat = |e: &Event| e.str("via") == Some("profile");
    let repo = |e: &&'a Event| e.fleet == fleet && e.kind == "orchestrator-attached" && !chat(e);
    let ids = events.iter().filter(repo).filter_map(|e| e.str("thread"));
    let live = |id: &&str| attached(events, id).is_some_and(|e| e.fleet == fleet);
    let ids: Vec<&str> = ids.filter(live).collect();
    let pin = cfg.fleet.get(fleet).and_then(|f| f.ask_thread.as_deref());
    let newest = || ids.iter().max_by_key(|id| of(events, id).map(|e| e.seq).max()).copied();
    ids.iter().find(|id| Some(**id) == pin).copied().or_else(newest)
}

/// Lane ask `a` as a system row in attached chat `id`: its client types the row's prompt.
pub fn lane_ask(ctx: &Ctx, cfg: &Config, id: &str, a: &Event) -> Result<String> {
    let s = |k| a.str(k).unwrap_or_default();
    let rec = a.val("recommend").as_u64();
    let opts = a.val("options").as_array().into_iter().flatten().zip(1..);
    let opts: String = opts
        .map(|(o, n)| {
            let r = if rec == Some(n) { " (recommended)" } else { "" };
            format!(" [{n}] {}{r}", o.as_str().unwrap_or_default())
        })
        .collect();
    let at = a.issue.map_or("the fleet".into(), |n| format!("#{n}"));
    let text = format!(
        "lane ask {} on {at} from {} ({}): {}{opts}. Answer it only if the issue spec or a past decision settles it: \
         `drover answer {} {} <n> --basis \"<verbatim spec quote | q<id>>\"`; else run it without --basis and it \
         goes to the owner.",
        s("id"),
        a.str("agent").unwrap_or("a lane"),
        s("kind"),
        s("question"),
        a.fleet,
        s("id")
    );
    let d = json!({"thread": id, "from": "system", "text": text, "lane_ask": a.str("id")});
    emit(&ctx.state, cfg, Event::new(ctx.now, &a.fleet, None, "thread-msg", d))?;
    Ok(text)
}

/// The thread lane ask `a` was routed to: when asked, else its routing row in a thread opened after.
pub fn routed_to<'a>(events: &'a [Event], a: &'a Event) -> Option<&'a str> {
    let row = |e: &&Event| e.kind == "thread-msg" && e.fleet == a.fleet && e.str("lane_ask") == a.str("id");
    a.str("routed")
        .or_else(|| events.iter().find(row).and_then(|e| e.str("thread")))
}

/// Lane ask `a` put to the owner as a choice row in thread `id` (once), `rec` (else its own) recommended; it pushes as an ask.
pub fn to_owner(ctx: &Ctx, cfg: &Config, id: &str, a: &Event, rec: Option<u64>, why: &str) -> Result<()> {
    let events = working(&ctx.state)?;
    if of(&events, id).any(|e| e.kind == "ask-owner" && e.str("id") == a.str("id")) {
        return Ok(());
    }
    let rec = rec.or_else(|| a.val("recommend").as_u64());
    let d = json!({"thread": id, "id": a.str("id"), "question": a.str("question"), "options": a.val("options"),
        "recommend": rec, "agent": a.str("agent"), "why": why});
    emit(&ctx.state, cfg, Event::new(ctx.now, &a.fleet, a.issue, "ask-owner", d))
}

/// A one-line row from chat `id` into each chat that handed off to it (an issue filed, an ask answered).
pub fn flow(ctx: &Ctx, cfg: &Config, id: &str, text: &str) -> Result<()> {
    let events = working(&ctx.state)?;
    let to_id = |e: &&Event| {
        e.kind == "thread-handoff" && e.str("to") == Some(id) && crate::orchestrator::moved(&events, e.seq).is_none()
    };
    let srcs: std::collections::BTreeMap<_, _> = (events.iter().filter(to_id))
        .filter_map(|e| Some((e.str("from")?, e.fleet.as_str())))
        .collect();
    for (s, fleet) in srcs {
        let d = json!({"thread": s, "from": "system", "via": id, "text": text});
        emit(&ctx.state, cfg, Event::new(ctx.now, fleet, None, "thread-msg", d))?;
    }
    Ok(())
}

/// A card's settle line into attached chat `id` as a `system` row; its client types the row's prompt.
pub fn tell(ctx: &Ctx, cfg: &Config, _: &dyn Runner, id: &str, line: &str) {
    let Some(t) = working(&ctx.state).ok().and_then(|e| attached(&e, id).cloned()) else {
        return;
    };
    let d = json!({"thread": id, "from": "system", "text": line});
    emit(&ctx.state, cfg, Event::new(ctx.now, &t.fleet, None, "thread-msg", d)).ok();
}
