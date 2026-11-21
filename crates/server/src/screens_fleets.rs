use super::{cut, Config, Ctx, Result, Server, LINE};
use crate::config::{Fleet, Notify, Role};
use serde_json::{json, Value};

fn roles(fc: &Fleet) -> Vec<(&str, &Role)> {
    let mut all: Vec<(&str, &Role)> = (fc.role.iter().filter(|r| !r.1.default))
        .map(|(k, r)| (k.as_str(), r))
        .collect();
    all.sort_by_key(|(k, _)| match *k {
        "impl" => 0,
        "rev" => 1,
        _ => 2,
    });
    all
}

fn effort(cfg: &Config, r: &Role) -> String {
    r.effort_or(&cfg.limits).unwrap_or_default()
}

pub(super) fn sheet(cfg: &Config) -> Value {
    let mut blocks = vec![];
    for (f, fc) in &cfg.fleet {
        let mut fields = vec![
            json!({"type": "toggle", "name": format!("{f}.drive"), "label": "Drive", "value": !fc.paused}),
            json!({"type": "text", "name": format!("{f}.integration"), "label": "Branch", "value": fc.integration, "placeholder": "integration branch"}),
        ];
        for (role, r) in roles(fc) {
            let label = match role {
                "impl" => "Impl",
                "rev" => "Review",
                r => r,
            };
            if !cfg.models.is_empty() {
                let value = r.model.clone().unwrap_or_default();
                fields.push(json!({"type": "picker", "name": format!("{f}.{role}"), "label": label, "value": value, "options": cfg.models}));
            }
            let efforts = crate::providers::get(&r.harness).efforts();
            fields.push(
                json!({"type": "picker", "name": format!("{f}.{role}.effort"), "label": format!("{label} effort"),
                "value": effort(cfg, r), "options": efforts}),
            );
        }
        blocks.push(json!({"type": "section", "id": format!("fleet-{f}"), "header": f, "footer": "Pause stops new lanes; live ones finish.",
            "blocks": [{"type": "form", "id": format!("f-{f}"), "fields": fields}]}));
    }
    let n = cfg.notify.len() + cfg.fleet.values().map(|f| f.notify.len()).sum::<usize>();
    let notify = format!("{n} notifier{} · Telegram off", if n == 1 { "" } else { "s" });
    blocks.push(json!({"type": "list", "id": "menu-list", "rows": [
        {"type": "row", "id": "menu-hosts", "title": "Hosts", "subtitle": "on/off, reap, disk and orphans", "accessory": "chevron", "tap": {"sheet": "/screens/hosts"}},
        {"type": "row", "id": "menu-notify", "title": "Notifications", "subtitle": notify, "accessory": "chevron", "tap": {"sheet": "/screens/notify"}}]}));
    for f in cfg.fleet.keys() {
        let confirm = json!({"title": format!("Remove {f}?"), "message": "Only its config section; its queue, lanes and log stay on disk.", "ok": "Remove"});
        blocks.push(
            json!({"type": "button", "id": format!("rm-{f}"), "label": format!("Remove {f}…"), "style": "destructive",
            "action": "remove-fleet", "values": {"fleet": f}, "confirm": confirm}),
        );
    }
    json!({"type": "sheet", "id": "fleets", "title": "Fleets", "cancel": "Cancel", "done": {"label": "Save", "action": "save-fleets"}, "blocks": blocks})
}

fn notifier(id: &str, n: &Notify) -> Value {
    let (kind, target) = if n.webpush() {
        ("webpush", n.sub.clone().unwrap_or_default())
    } else {
        ("command", crate::lanes::mask(&n.cmd))
    };
    let on = if !n.on.is_empty() {
        n.on.join(", ")
    } else if n.webpush() {
        "push events".into()
    } else {
        "pauses, reverts and stalls only".into()
    };
    let json = if n.format.as_deref() == Some("json") {
        " · json"
    } else {
        ""
    };
    json!({"type": "row", "id": id, "title": cut(&target, LINE), "subtitle": format!("{kind}{json} · {on}")})
}

fn notify_section(id: &str, header: &str, all: &[Notify]) -> Value {
    let mut rows: Vec<Value> = (all.iter().enumerate())
        .map(|(i, n)| notifier(&format!("{id}-{i}"), n))
        .collect();
    if rows.is_empty() {
        rows.push(json!({"type": "row", "id": format!("{id}-none"), "title": "no notifiers"}));
    }
    json!({"type": "section", "id": id, "header": header, "blocks": [{"type": "list", "id": format!("{id}-list"), "rows": rows}]})
}

pub(super) fn notify(cfg: &Config) -> Value {
    let mut blocks = vec![notify_section("nf-global", "[[notify]]", &cfg.notify)];
    for (f, fc) in cfg.fleet.iter().filter(|f| !f.1.notify.is_empty()) {
        let with = if fc.notify_global { "with" } else { "instead of" };
        blocks.push(notify_section(
            &format!("nf-{f}"),
            &format!("{f} ({with} the global ones)"),
            &fc.notify,
        ));
    }
    let tg = if cfg.telegram.is_some() {
        json!({"text": "removed (#557): drop the leftover [telegram] table", "tone": "warn"})
    } else {
        json!("removed (#557): a command notifier sends to Telegram")
    };
    let row = json!({"type": "row", "id": "telegram", "title": "Telegram bridge", "badge": {"text": "off", "tone": "mute"}, "subtitle": tg});
    blocks.push(json!({"type": "section", "id": "nf-telegram", "header": "Telegram",
        "blocks": [{"type": "list", "id": "nf-telegram-list", "rows": [row]}]}));
    json!({"type": "sheet", "id": "notify", "title": "Notifications", "cancel": "Done", "blocks": blocks})
}

impl Server<'_> {
    pub(super) fn save_fleets(&self, ctx: &Ctx, cfg: &Config, v: &Value) -> Result<String> {
        let (mut done, mut set) = (vec![], vec![]);
        for (f, fc) in &cfg.fleet {
            let val = |k: &str| v.get(format!("{f}.{k}")).filter(|x| !x.is_null());
            if let Some(on) = val("drive").and_then(Value::as_bool).filter(|on| *on == fc.paused) {
                done.push(self.form_op(ctx, if on { "resume" } else { "pause" }, &[("fleet", f.clone())])?);
            }
            for (role, r) in roles(fc) {
                let picked = |k: &str, cur: String| {
                    let new = val(k).and_then(Value::as_str).filter(|m| !m.is_empty() && **m != cur);
                    new.map(String::from)
                };
                let model = picked(role, r.model.clone().unwrap_or_default());
                let effort = picked(&format!("{role}.effort"), effort(cfg, r));
                let wanted: Vec<(&str, String)> = [("model", model), ("effort", effort)]
                    .into_iter()
                    .filter_map(|(k, x)| Some((k, x?)))
                    .collect();
                if !wanted.is_empty() {
                    let line: Vec<String> = wanted.iter().map(|(k, x)| format!("{k} {x}")).collect();
                    crate::set(ctx, &mut vec![], f, role, wanted, Some("cooee"))?;
                    done.push(format!("{f} {role}: {}", line.join(", ")));
                }
            }
            if let Some(b) = val("integration")
                .and_then(Value::as_str)
                .filter(|b| *b != fc.integration)
            {
                set.push(crate::propose::ConfigSet {
                    path: format!("fleet.{f}.integration"),
                    value: json!(b),
                });
            }
        }
        if !set.is_empty() {
            let c = crate::propose::ConfigChange {
                hash: crate::propose::view(ctx)?.hash,
                set,
                ..Default::default()
            };
            done.push(crate::propose::edit(
                ctx,
                &Config::kept(ctx)?,
                &self.r,
                &c,
                "owner (cooee)",
            )?);
        }
        Ok(if done.is_empty() {
            "nothing changed".into()
        } else {
            done.join("; ")
        })
    }
}
