use super::pulse;
use crate::acp::audit::{self, Ran};
use crate::config::Config;
use crate::events::{emit, Event, Json};
use crate::runner::Runner;
use crate::Ctx;
use anyhow::Result;
use serde_json::json;
use shed_proto::EventKind;

/// A judged lane spawn `sp`'s shed event: its turn mark, the calls it asked about, a judge's refusals, the audit of an unasked call, and at a turn's end the refusals as its next prompt, the turn line and drive's kick.
pub fn on(ctx: &Ctx, cfg: &Config, r: &dyn Runner, sp: &Event, e: &shed_proto::Event) -> Result<()> {
    let (Some(issue), Some(role)) = (sp.issue, sp.str("role")) else {
        return Ok(());
    };
    let lane = format!("{}:{issue}:{role}", sp.fleet);
    let at = (sp.fleet.as_str(), issue);
    let name = e.name.as_str();
    match &e.kind {
        EventKind::Turn { .. } => {
            std::fs::write(crate::hook::turn_mark(&ctx.state, &lane), "prompt").ok();
            pulse::update(ctx, name, |p| {
                if let Some(p) = p {
                    (p.asked, p.refused, p.unasked) = (vec![], vec![], None);
                }
            })?;
        }
        EventKind::ApprovalRequest { tool, call, .. } => {
            let id = call
                .as_ref()
                .and_then(|c| c.at("toolCallId").as_str())
                .unwrap_or_default()
                .to_string();
            pulse::update(ctx, name, |p| {
                let p = p.get_or_insert_with(pulse::Pulse::default);
                p.asked.push(id);
                p.ask = Some(tool.clone());
            })?;
        }
        EventKind::Approved {
            by: Some(by),
            decision: Some(d),
            reason,
            ..
        } if by == "judge" && d == "deny" => {
            let why = reason.clone().unwrap_or_default();
            pulse::update(ctx, name, |p| {
                let p = p.get_or_insert_with(pulse::Pulse::default);
                let what = p.ask.clone().unwrap_or_else(|| "a call".into());
                p.refused
                    .push(format!("{what}: {}", why.trim_start_matches("drover: ")));
            })?;
        }
        EventKind::ToolCall {
            id,
            title,
            status,
            kind,
            locations,
            raw_input,
        } if status == "completed" => {
            let ran = Ran {
                id,
                title,
                kind: kind.as_deref(),
                locations,
                raw_input: raw_input.as_ref(),
            };
            let asked = pulse::of(ctx, name).is_some_and(|p| p.asked.contains(id));
            if ran.acts() && !asked {
                pulse::update(ctx, name, |p| {
                    if let Some(p) = p {
                        p.unasked = Some(p.unasked.unwrap_or(0) + 1);
                    }
                })?;
                if let Some(rule) = audit::replay(ctx, r, sp, &ran) {
                    return unguarded(ctx, cfg, r, sp, (name, title, &rule));
                }
            }
            let cwd = sp.str("worktree").unwrap_or(".");
            if let Some(crate::hook::Call::Shell { cmd, cwd }) =
                ran.call(cwd).filter(|_| kind.as_deref() == Some("execute"))
            {
                if crate::hook::push_dir(&json!({"tool_input": {"command": cmd}, "cwd": cwd})).is_some() {
                    crate::drive::kick_once(ctx, r, at, "push", Some(id));
                }
            }
        }
        EventKind::Reply { .. } => ended(ctx, cfg, r, sp, (name, &lane))?,
        _ => {}
    }
    Ok(())
}

fn ended(ctx: &Ctx, cfg: &Config, r: &dyn Runner, sp: &Event, (name, lane): (&str, &str)) -> Result<()> {
    let p = pulse::of(ctx, name).unwrap_or_default();
    let ev =
        |kind: &str, d: serde_json::Value| emit(&ctx.state, cfg, Event::new(ctx.now, &sp.fleet, sp.issue, kind, d));
    if let Some(n) = p.unasked {
        ev("acp-unasked", json!({"agent": name, "acp": sp.str("acp"), "n": n}))?;
    }
    let new: Vec<String> = p.refused.iter().filter(|x| !p.told.contains(x)).cloned().collect();
    let mark = crate::hook::turn_mark(&ctx.state, lane);
    let seen = std::fs::read_to_string(&mark).unwrap_or_default();
    let builds = crate::hook::lane_rules(ctx, lane).is_ok_and(|r| r.builds);
    let text = if new.is_empty() {
        (builds && !matches!(seen.as_str(), "turn" | "asked")).then(|| {
            std::fs::write(&mark, "asked").ok();
            crate::hook::TURN_ASK.to_string()
        })
    } else {
        let n = new.len();
        let lines: Vec<String> = new.iter().map(|x| format!("- {x}")).collect();
        Some(format!("drover refused {n} call(s):\n{}", lines.join("\n")))
    };
    pulse::update(ctx, name, |q| {
        if let Some(q) = q {
            q.told.extend(new.iter().cloned());
            (q.asked, q.refused, q.unasked) = (vec![], vec![], None);
        }
    })?;
    if let Some(t) = text {
        crate::shed::cli(r, &["send", name, &t])?;
    }
    if let Some(issue) = sp.issue {
        crate::drive::kick_once(ctx, r, (&sp.fleet, issue), "stop", None);
    }
    Ok(())
}

fn unguarded(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    sp: &Event,
    (name, tool, rule): (&str, &str, &str),
) -> Result<()> {
    let acp = sp.str("acp").unwrap_or_default();
    let d = json!({"agent": name, "acp": acp, "tool": tool, "rule": rule});
    emit(
        &ctx.state,
        cfg,
        Event::new(ctx.now, &sp.fleet, sp.issue, "lane-unguarded", d),
    )?;
    let why = format!("ran `{tool}` without asking, which a rule refuses");
    crate::acp::probe::stamp(ctx, cfg, acp, sp.str("version").unwrap_or_default(), Err(&why))?;
    crate::check::kill(ctx, cfg, r, &mut std::io::sink(), (name, None), true)
}
