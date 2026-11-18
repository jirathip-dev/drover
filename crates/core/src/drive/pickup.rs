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
        return Ok(Picked::No);
    };
    let path = format!("{}/verdict.md", sp.str("worktree").unwrap_or_default());
    let text = match crate::hosts::On::of(r, Some(sp)).run("cat", &crate::runner::argv(&[&path])) {
        Ok(o) if o.code == 0 => o.stdout,
        Ok(o) if o.stderr.contains("No such file") => return Ok(Picked::No),
        o => {
            let why = o.map_or_else(
                |e| format!("{e:#}"),
                |o| o.stderr.lines().next().map_or(format!("exit {}", o.code), String::from),
            );
            failed(ctx, out, ev, (&s.fleet, s.issue, &w.lane), &why)?;
            return Ok(Picked::Held);
        }
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
