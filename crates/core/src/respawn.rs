use crate::config::Config;
use crate::events::Event;
use crate::gate::refused;
use crate::Ctx;
use anyhow::Result;
use serde_json::json;

/// `RESPAWN_LOOP`: `respawn_max` failed starts of `role` (a lane dead or killed at start, a prompt never landed, a start that errored, once a pass) within the window refuse the next, behind one owner ask.
pub fn guard(ctx: &Ctx, cfg: &Config, events: &[Event], (fleet, issue, role): (&str, u64, &str)) -> Result<()> {
    let l = &cfg.limits;
    let (v, asked) = starts(events, fleet, issue, role);
    let since = ctx.now.saturating_sub(l.respawn_window_minutes * 60);
    let failed: Vec<String> = (v.iter())
        .filter(|s| s.0 >= since)
        .filter_map(|s| Some(format!("{} ago: {}", crate::events::age(ctx.now - s.0), s.1?)))
        .collect();
    if failed.len() < usize::try_from(l.respawn_max).unwrap_or(usize::MAX) {
        return Ok(());
    }
    let why = format!(
        "{} failed {role} starts within {} min ({}); answer the owner ask to try again or park",
        failed.len(),
        l.respawn_window_minutes,
        failed.join("; ")
    );
    if asked.is_none() {
        let q = format!("#{issue}'s {role} lane keeps failing at start: {}", failed.join("; "));
        let d = json!({"question": q, "options": ["try again", "park"], "recommend": null, "agent": null,
            "kind": "owner", "respawn_loop": role});
        let ev = Event::new(ctx.now, fleet, Some(issue), "needs-decision", d);
        crate::events::ask(&ctx.state, cfg, vec![ev], None)?;
    }
    refused("RESPAWN_LOOP", why)
}

fn starts<'a>(
    events: &'a [Event],
    fleet: &str,
    issue: u64,
    role: &str,
) -> (Vec<(u64, Option<&'a str>)>, Option<&'a str>) {
    let (mut v, mut asked, mut agent) = (vec![], None, None);
    let mut asks = vec![];
    let fail = |v: &mut Vec<(u64, Option<&str>)>, why| {
        if let Some(s) = v.last_mut().filter(|s| s.1.is_none()) {
            s.1 = Some(why);
        }
    };
    for e in events.iter().filter(|e| e.fleet == fleet && e.issue == Some(issue)) {
        let mine = e.str("role") == Some(role);
        match e.kind.as_str() {
            "spawn" if mine => {
                agent = e.str("agent");
                v.push((
                    e.ts,
                    (e.data.get("confirmed") == Some(&json!(false))).then_some("prompt never landed"),
                ));
            }
            "pushed" => (v, asked) = (vec![], None),
            "runtime-cleared" => v.clear(),
            "turn-end" if mine => (v, asked) = (vec![], None),
            "lane-dead" if agent.is_some_and(|a| e.str("key").is_some_and(|k| k.ends_with(&format!(":{a}")))) => {
                fail(&mut v, "lane died");
            }
            "supersede"
                if mine && agent.is_some_and(|a| e.val("killed").as_array().is_some_and(|k| k.contains(&json!(a)))) => {
            }
            "supersede" | "lane-killed" if mine => fail(&mut v, "killed"),
            "failed" if mine && e.str("error").is_some() && v.last().map(|s| s.0) != Some(e.ts) => {
                v.push((e.ts, e.str("error")));
            }
            "needs-decision" if e.str("respawn_loop") == Some(role) => {
                asks.extend(e.str("id"));
                asked = e.str("id");
            }
            "decision" if e.str("choice") == Some("try again") && asks.contains(&e.str("id").unwrap_or_default()) => {
                (v, asked) = (vec![], None);
            }
            _ => {}
        }
    }
    (v, asked)
}
