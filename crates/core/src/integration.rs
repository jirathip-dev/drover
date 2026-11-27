use crate::config::Config;
use crate::events::{append, working, Event};
use crate::runner::{ok, Runner};
use crate::Ctx;
use anyhow::Result;
use serde_json::json;
use std::io::Write;

const KIND: &str = "integration-checked";
const JOB: &str = "integration";

fn checks<'a>(events: &'a [Event], fleet: &'a str) -> impl DoubleEndedIterator<Item = &'a Event> {
    events.iter().filter(move |e| e.kind == KIND && e.fleet == fleet)
}

/// Fleet `fleet`'s integration head at its last check: `<sha> green|red <age> ago`.
pub fn line(now: u64, events: &[Event], fleet: &str) -> Option<String> {
    let e = checks(events, fleet).next_back()?;
    let result = if e.val("green").as_bool() == Some(true) {
        "green"
    } else {
        "red"
    };
    let age = crate::events::age(now.saturating_sub(e.ts));
    Some(format!("integration {:.7} {result} {age} ago", e.str("sha")?))
}

/// `drover integration <fleet>`, the `integration` job's check in its checkout of the integration head: the premerge gate once per head; green closes the job's open issue, red prints the failing tail and the PRs merged since the last green head and exits 1.
pub fn check(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write, fleet: &str) -> Result<i32> {
    let f = cfg.fleet(fleet)?;
    let head = ok(r, "git", &["rev-parse", "HEAD"])?.trim().to_string();
    let events = working(&ctx.state)?;
    if checks(&events, fleet).any(|e| e.str("sha") == Some(&head)) {
        writeln!(out, "{head:.7} is checked already")?;
        return Ok(0);
    }
    let green = checks(&events, fleet).rfind(|e| e.val("green").as_bool() == Some(true));
    let base = green.and_then(|e| e.str("sha")).unwrap_or("HEAD^").to_string();
    let dir = std::env::current_dir()?;
    let heavy = crate::heavy::Heavy::at(cfg, f, None);
    let gate = crate::gate::premerge_checks(f);
    let fail = crate::gate::run_checks(r, (&heavy, &ctx.exe), &dir, &base, &gate);
    let d = json!({"sha": head, "green": fail.is_none()});
    append(&ctx.state, &Event::new(ctx.now, fleet, None, KIND, d))?;
    let Some((c, o)) = fail else {
        if let Some(n) = crate::jobs::open(&events, fleet, JOB) {
            crate::jobs::close(ctx, cfg, r, out, (fleet, n), &format!("{head:.7} is green"))?;
        }
        writeln!(out, "{head:.7} is green")?;
        return Ok(0);
    };
    let lines: Vec<&str> = o.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = lines.get(lines.len().saturating_sub(30)..).unwrap_or_default();
    writeln!(out, "`{}` failed at {head:.7}:\n{}", c.name, tail.join("\n"))?;
    let range = format!("{base}..HEAD");
    let log = ok(r, "git", &["log", "--first-parent", "--format=%s", &range]).unwrap_or_default();
    let prs: Vec<&str> = (log.lines())
        .filter_map(|s| s.strip_suffix(')')?.rsplit_once("(#").map(|p| p.1))
        .collect();
    let since = green.map_or("at this head (no green head yet)".into(), |_| {
        format!("since {base:.7}, the last green head")
    });
    let prs = if prs.is_empty() {
        "none named".into()
    } else {
        format!("#{}", prs.join(", #"))
    };
    writeln!(out, "\nPRs merged {since}: {prs}")?;
    Ok(1)
}
