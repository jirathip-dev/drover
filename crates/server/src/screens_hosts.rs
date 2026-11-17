use super::{working, Config, Ctx, Result, Server};
use anyhow::Context;
use serde_json::{json, Value};

pub(super) const HOST_ACTIONS: [&str; 2] = ["host", "reap"];

fn plural(n: usize, w: &str) -> String {
    format!("{n} {w}{}", if n == 1 { "" } else { "s" })
}

fn host_row(name: &str, kind: &str, on: bool, lanes: usize, left: usize) -> Value {
    let mut sub = kind.to_string();
    if left > 0 {
        sub += &format!(" · {}", plural(left, "leftover"));
    }
    if !on {
        sub += &format!(" · no new lanes, {} finishing", plural(lanes, "lane"));
    }
    let mut swipe = vec![];
    if !on {
        swipe.push(json!({"label": "On", "action": "host", "values": {"host": name, "to": "on"},
            "confirm": {"title": format!("Turn {name} on?"), "message": format!("New lanes may go to {name} again."), "ok": "Turn on"}}));
    } else if kind != "main host" {
        let live = if lanes == 1 {
            "1 live lane finishes".into()
        } else {
            format!("{lanes} live lanes finish")
        };
        swipe.push(json!({"label": "Off", "tone": "bad", "action": "host", "values": {"host": name, "to": "off"},
            "confirm": {"title": format!("Turn {name} off?"), "message": format!("No new lanes go to {name}; its {live} there."), "ok": "Turn off"}}));
    }
    if left > 0 {
        swipe.push(json!({"label": format!("Reap {left}"), "tone": "bad", "action": "reap", "values": {},
            "confirm": {"title": format!("Reap {left} leftover process{} on {name}?", if left == 1 { "" } else { "es" }),
                "message": "They run in closed lanes' worktrees. TERM, then KILL.", "ok": "Reap"}}));
    }
    let badge = if on {
        json!({"text": "on", "tone": "ok"})
    } else {
        json!({"text": "off", "tone": "warn"})
    };
    json!({"type": "row", "id": format!("h-{name}"), "title": name, "badge": badge, "subtitle": {"text": sub}, "value": plural(lanes, "lane"), "swipe": swipe})
}

impl Server<'_> {
    pub(super) fn hosts_sheet(&self, ctx: &Ctx) -> Option<Value> {
        let (cfg, ev) = (Config::kept(ctx).ok()?, working(&ctx.state).ok()?);
        let left = crate::session::live_on(&self.r).map_or(0, |a| {
            crate::check::leftovers(ctx, &cfg, &self.r, &a, false).map_or(0, |l| l.len())
        });
        let rows: Vec<Value> = crate::hosts::cards(&cfg, &self.r)
            .into_iter()
            .map(|(name, h)| {
                let main = h.is_none_or(|h| h.machine.is_none());
                let kind = match h {
                    _ if main => "main host",
                    Some(h) if h.cloud.is_some() => "cloud host",
                    _ => "remote host",
                };
                let lanes = crate::hosts::lanes_on(&cfg, &ev, &name).len();
                host_row(
                    &name,
                    kind,
                    h.is_none_or(|h| h.enabled),
                    lanes,
                    if main { left } else { 0 },
                )
            })
            .collect();
        Some(
            json!({"type": "sheet", "id": "hosts", "title": "Hosts", "cancel": "Close", "blocks": [{"type": "section",
            "id": "hosts-sec", "header": format!("hosts ({})", rows.len()),
            "footer": "Tap a host for its buttons. Host history and charts live in Beacon.",
            "blocks": [{"type": "list", "id": "hosts-list", "rows": rows}]}]}),
        )
    }

    pub(super) fn host_op(&self, ctx: &Ctx, op: &str, host: String, to: String) -> Result<Value> {
        let line = self.form_op(ctx, op, &[("host", host), ("to", to)])?;
        let sh = self.hosts_sheet(ctx).context("no hosts")?;
        Ok(json!({"ops": [{"op": "screen", "screen": sh}], "toast": line}))
    }
}
