use crate::config::Config;
use crate::events::{append, Event, IssueState, Json};
use crate::runner::Runner;
use crate::waiting::{self, Waiting};
use crate::Ctx;
use anyhow::Result;
use serde_json::{json, Value};
use std::io::Write;

pub(super) enum Picked {
    No,
    Held,
    Fetched(Waiting),
}

pub(super) fn fetch(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    ev: &[Event],
    (s, w): (&IssueState, &Waiting),
    pr: &Value,
) -> Result<Picked> {
    let stuck = ["rev idle without", "reviewer exited without", waiting::PICKUP];
    let sp = s.check_lane().and_then(|l| crate::events::last_spawn(ev, l));
    let sp = sp.filter(|e| e.str("machine").is_some() && stuck.iter().any(|p| w.what.starts_with(p)));
    let (Some(sp), Some(head)) = (sp, pr.at("headRefOid").as_str()) else {
        let spawn = w.next_cmd.as_deref().is_some_and(|c| c.starts_with("drover spawn "));
        let held = spawn && ingest(ctx, cfg, r, out, ev, (s, &w.lane), pr)?;
        return Ok(if held { Picked::Held } else { Picked::No });
    };
    let path = format!("{}/verdict.md", sp.str("worktree").unwrap_or_default());
    let view = crate::link::view::View::load(ctx, cfg)?;
    let text = match view.worktree(sp) {
        Some(w) => match &w.verdict {
            Some(v) => v.text.clone(),
            None => return Ok(Picked::No),
        },
        None => match view.down(sp.str("machine")) {
            Some(why) => {
                failed(ctx, out, ev, (&s.fleet, s.issue, &w.lane), &why)?;
                return Ok(Picked::Held);
            }
            None => return Ok(Picked::No),
        },
    };
    let v = text.lines().next().unwrap_or_default().trim();
    let v = if v == "FAIL (form-only)" { "FAIL" } else { v };
    if !matches!(v, "PASS" | "FAIL") || !waiting::reviewed_head(&text).is_some_and(|h| h.eq_ignore_ascii_case(head)) {
        return Ok(Picked::No);
    }
    let mut x = w.clone();
    x.who = "drive";
    x.next_cmd = Some(format!(
        "drover verdict {} {} {v} --sha {head} --file {path}",
        s.fleet, s.issue
    ));
    x.say(&crate::promise::Say::Record("review done".into()));
    Ok(Picked::Fetched(x))
}

pub(super) fn failed(
    ctx: &Ctx,
    out: &mut dyn Write,
    ev: &[Event],
    (fleet, n, lane): (&str, u64, &str),
    why: &str,
) -> Result<()> {
    writeln!(out, "{fleet} #{n} {}{why}", waiting::PICKUP)?;
    let last = (ev.iter().rev())
        .filter(|e| e.fleet == fleet && e.issue == Some(n))
        .take_while(|e| !(e.kind == "spawn" && crate::events::checks(e)))
        .find(|e| e.kind == "verdict-pickup-failed");
    if last.is_some_and(|e| e.str("why") == Some(why)) {
        return Ok(());
    }
    let d = json!({"agent": lane, "why": why});
    append(
        &ctx.state,
        &Event::new(ctx.now, fleet, Some(n), "verdict-pickup-failed", d).passive(),
    )
}

fn ingest(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    events: &[Event],
    (s, lane): (&IssueState, &str),
    pr: &Value,
) -> Result<bool> {
    let (fleet, f) = (s.fleet.as_str(), cfg.fleet(&s.fleet)?);
    let sp = crate::events::last_spawn(events, lane);
    let role = sp.and_then(|e| e.str("role")).unwrap_or_default();
    let Some(sp) = sp.filter(|_| s.check_lane().map(String::as_str) == Some(lane) && f.flow().is_check(role)) else {
        return Ok(false);
    };
    let recorded = events.iter().any(|e| e.seq > sp.seq && e.is("verdict", fleet, s.issue));
    let Some(path) = sp
        .str("worktree")
        .filter(|_| !recorded)
        .map(|w| std::path::Path::new(w).join("verdict.md"))
    else {
        return Ok(false);
    };
    let view = crate::link::view::View::load(ctx, cfg)?;
    let seen = view.worktree(sp).and_then(|w| w.verdict.as_ref());
    let Some(text) = seen.filter(|v| v.mtime >= sp.ts).map(|v| v.text.clone()) else {
        return Ok(false);
    };
    let v = match text.lines().next().map(str::trim) {
        Some("PASS") => "PASS",
        Some(l) if l.starts_with("FAIL") => "FAIL",
        _ => return Ok(false),
    };
    let head = pr.at("headRefOid").as_str().unwrap_or_default();
    let sha = crate::waiting::reviewed_head(&text).or(sp.str("sha"));
    if head.is_empty() || !sha.is_some_and(|x| x.eq_ignore_ascii_case(head)) {
        return Ok(false);
    }
    let (n, file) = (s.issue.to_string(), path.to_string_lossy());
    let args = ["verdict", fleet, &n, v, "--sha", head, "--file", &file];
    writeln!(
        out,
        "{fleet} #{n} {role} lane's verdict.md recorded before drive acts on it"
    )?;
    Ok(super::act(ctx, cfg, r, out, events, (fleet, Some(s.issue), "verdict"), &args)?.is_none())
}
