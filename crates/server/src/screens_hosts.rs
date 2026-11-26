use super::{super::lock, working, Config, Ctx, Result, Server};
use crate::events::Json;
use anyhow::Context;
use serde_json::{json, Value};

pub(super) const HOST_ACTIONS: [&str; 3] = ["host", "reap", "orphans"];

pub(super) fn weather(host: &crate::api::HerdHost) -> (&'static str, &'static str) {
    let ratio = host.load.unwrap_or_default() / host.limit.max(0.01);
    match host.pressure.as_deref() {
        _ if host.gate.is_some() => ("storm", "held"),
        Some("warn" | "critical") => ("fog", "busy"),
        _ if ratio >= 0.5 => ("windy", "busy"),
        _ => ("clear", "calm"),
    }
}

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

fn disk(ctx: &Ctx, cfg: &Config) -> Value {
    let budget = cfg.limits.worktree_budget_gb;
    let mut wt = json!({"type": "row", "id": "disk-worktrees", "title": "worktrees"});
    match crate::check::worktree_du(ctx) {
        Some((gb, _)) => {
            wt.put("value", json!(format!("{gb:.1} / {budget:.0} GB")));
            if gb > budget {
                wt.put(
                    "subtitle",
                    json!({"text": "over budget: drive frees the idlest lanes' build folders", "tone": "warn"}),
                );
            }
        }
        None => {
            wt.put("value", json!(format!("not measured yet / {budget:.0} GB")));
        }
    }
    let n = crate::check::orphan_dirs(ctx, cfg).len();
    let mut orphans = json!({"type": "row", "id": "disk-orphans", "title": "orphan folders", "subtitle": "under the worktree root, no fleet's", "value": n.to_string()});
    if n > 0 {
        orphans.put("swipe", json!([{"label": format!("Remove {n}"), "tone": "bad", "action": "orphans", "values": {},
            "confirm": {"title": format!("Remove {}?", plural(n, "orphan folder")), "message": "No configured fleet owns them; they are deleted, not saved.", "ok": "Remove"}}]));
    }
    json!({"type": "section", "id": "disk-sec", "header": "fleet disk", "footer": "The worktree total is drive's last walk.",
        "blocks": [{"type": "list", "id": "disk-list", "rows": [wt, orphans]}]})
}

impl Server<'_> {
    pub(super) fn hosts_sheet(&self, ctx: &Ctx) -> Option<Value> {
        let (cfg, ev) = (Config::kept(ctx).ok()?, working(&ctx.state).ok()?);
        let left = *lock(&self.hosts_read);
        let (_, level) = weather(&super::super::herd_host(self.sample(), &cfg.limits));
        let rows: Vec<Value> = crate::hosts::cards(&cfg, &self.r)
            .into_iter()
            .map(|(name, h)| {
                let main = h.is_none_or(|h| h.machine.is_none());
                let kind = match h {
                    _ if main => "main host",
                    _ => "remote host",
                };
                let lanes = crate::hosts::lanes_on(&cfg, &ev, &name).len();
                let mut row = host_row(
                    &name,
                    kind,
                    h.is_none_or(|h| h.enabled),
                    lanes,
                    if main { left } else { 0 },
                );
                if main {
                    row.put("figure", json!({"load": level}));
                }
                row
            })
            .collect();
        let blocks = vec![
            json!({"type": "section", "id": "hosts-sec", "header": format!("hosts ({})", rows.len()),
            "footer": "Tap a host for its buttons. Host history and charts live in Beacon.",
            "blocks": [{"type": "list", "id": "hosts-list", "rows": rows}]}),
            disk(ctx, &cfg),
        ];
        Some(json!({"type": "sheet", "id": "hosts", "title": "Hosts", "cancel": "Close", "blocks": blocks}))
    }

    pub(crate) fn read_hosts(&self, now: u64) {
        let Ok(cfg) = Config::kept(self.ctx) else { return };
        let ctx = &Ctx {
            now,
            ..self.ctx.clone()
        };
        let left = crate::session::live_on(&self.r).map_or(0, |a| {
            crate::check::leftovers(ctx, &cfg, &self.r, &a, false).map_or(0, |l| l.len())
        });
        *lock(&self.hosts_read) = left;
    }

    pub(super) fn host_op(&self, ctx: &Ctx, op: &str, v: &Value) -> Result<Value> {
        let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        let line = self.form_op(ctx, op, &[("host", s("host")), ("to", s("to"))])?;
        let sh = self.hosts_sheet(ctx).context("no hosts")?;
        Ok(json!({"ops": [{"op": "screen", "screen": sh}], "toast": line}))
    }
}
