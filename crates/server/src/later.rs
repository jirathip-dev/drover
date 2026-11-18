use super::{ask_card, ask_minutes, devices, needs_card, run_cards, working, BTreeMap, Ctx, Event, Result, Server};
use crate::events::Json;
use anyhow::Context;
use serde_json::{json, Value};

struct Line {
    key: String,
    fp: String,
    md: String,
    issue: Option<u64>,
}

struct Owned {
    id: String,
    fp: String,
    card: Value,
    group: Option<String>,
    rows: Vec<Line>,
}

impl Owned {
    fn of(ctx: &Ctx, ev: &crate::events::Log, card: Value, at: Option<(&str, u64)>) -> Owned {
        let id = card.at("id").as_str().unwrap_or_default().to_string();
        let fp = fingerprint(ctx, ev, &card.at("blocks").to_string(), at);
        Owned {
            id,
            fp,
            card,
            group: None,
            rows: vec![],
        }
    }
}

fn fingerprint(ctx: &Ctx, ev: &crate::events::Log, reason: &str, at: Option<(&str, u64)>) -> String {
    let (state, head) = at.map_or_else(Default::default, |(f, n)| {
        let st = ev.states.get(&(f.to_string(), n)).map(|s| s.state.clone());
        let pr = crate::gh::cached_pr(ctx, f, n);
        (
            st.unwrap_or_default(),
            pr.at("headRefOid").as_str().unwrap_or_default().to_string(),
        )
    });
    let h = devices::sha256(format!("{reason}\n{state}\n{head}").as_bytes());
    h.chars().take(12).collect()
}

fn laters(ev: &[Event]) -> BTreeMap<(String, String), String> {
    let s = |e: &Event, k: &str| e.str(k).unwrap_or_default().to_string();
    (ev.iter().filter(|e| e.kind == "card-later"))
        .map(|e| ((s(e, "card"), s(e, "row")), s(e, "fp")))
        .collect()
}

fn stuck_card(at: &str, f: &str, rows: &[&Line]) -> Value {
    let mut md: Vec<String> = rows.iter().map(|l| format!("- {}", l.md)).collect();
    md.push(format!("`drover waiting {f}`"));
    let mut buttons = vec![
        json!({"type": "button", "id": format!("{at}stuck-drive-{f}"), "label": "Drive now", "action": "drive-all"}),
    ];
    buttons.extend(rows.iter().filter_map(|l| l.issue).map(|n| {
        json!({"type": "button", "id": format!("{at}stuck-force-{f}-{n}"), "label": format!("Force #{n}"), "action": "force", "values": {"fleet": f, "issue": n}})
    }));
    json!({"type": "card", "id": format!("{at}stuck-{f}"), "kind": "alert", "tone": "warn",
        "title": "Drive is stuck", "blocks": [{"type": "text", "id": format!("{at}stuck-why"), "markdown": md.join("\n")}],
        "buttons": buttons})
}

fn with_later(mut card: Value, group: bool) -> Value {
    let id = card.at("id").as_str().unwrap_or_default().to_string();
    let mut add = vec![
        json!({"type": "button", "id": format!("{id}-later"), "label": "later", "action": "later", "values": {"card": id}}),
    ];
    if group {
        add.push(json!({"type": "button", "id": format!("{id}-later-all"), "label": "later all", "action": "later", "values": {"card": id, "all": true}}));
    }
    if let Some(b) = card.get_mut("buttons").and_then(Value::as_array_mut) {
        b.extend(add);
    } else {
        card.put("buttons", Value::Array(add));
    }
    card
}

impl Server<'_> {
    pub(super) fn chat_cards(&self, ctx: &Ctx, paired: bool) -> Value {
        let ev = working(&ctx.state).unwrap_or_default();
        let hid = laters(&ev);
        let is_hid =
            |id: &str, row: &str, fp: &str| hid.get(&(id.to_string(), row.to_string())).is_some_and(|h| h == fp);
        let (mut shown, mut later) = (vec![], vec![]);
        for o in self.owner_cards(ctx, &ev, paired) {
            let whole = is_hid(&o.id, "", &o.fp);
            let (gone, kept): (Vec<&Line>, Vec<&Line>) =
                o.rows.iter().partition(|l| whole || is_hid(&o.id, &l.key, &l.fp));
            match o.group.as_deref() {
                _ if whole || !gone.is_empty() && kept.is_empty() => later.push(o.card),
                Some(f) => {
                    if !gone.is_empty() {
                        later.push(stuck_card("later-", f, &gone));
                    }
                    shown.push(with_later(stuck_card("", f, &kept), true));
                }
                None => shown.push(with_later(o.card, false)),
            }
        }
        if !later.is_empty() {
            shown.push(json!({"type": "card", "id": "later", "kind": "later", "tone": "mute",
                "fold": {"line": format!("{} later", later.len())}, "blocks": later}));
        }
        Value::Array(shown)
    }

    fn owner_cards(&self, ctx: &Ctx, ev: &crate::events::Log, paired: bool) -> Vec<Owned> {
        let tab = self.needs_tab(ctx, true);
        let mut cards = run_cards(ctx.now, ev);
        cards.retain(|c| {
            !self.flying(&[
                ("op", "card"),
                ("id", c["id"].as_str().unwrap_or_default().trim_start_matches("run-")),
            ])
        });
        cards.extend(super::super::chat::owner_cards(ctx, ev, paired));
        cards.extend(tab.config.as_ref().map(|e| {
            json!({"type": "card", "id": "config-invalid", "kind": "alert", "tone": "bad",
                "title": "fleet.toml is invalid", "blocks": [{"type": "text", "id": "config-why", "markdown": e}]})
        }));
        cards.extend(super::super::v1::slo_open(ev).map(|e| {
            json!({"type": "card", "id": format!("slo-{}", e.seq), "kind": "alert", "tone": "warn",
                "title": "The board is slow", "blocks": [{"type": "text", "id": "slo-why", "markdown": e.str("reason")}]})
        }));
        let mut out: Vec<Owned> = cards.into_iter().map(|c| Owned::of(ctx, ev, c, None)).collect();
        out.extend(crate::slo::open_stuck(ev).into_iter().map(|(f, es)| {
            let rows: Vec<Line> = (es.iter())
                .map(|e| {
                    let md = crate::events::plain(e);
                    let key = e
                        .issue
                        .map_or_else(|| e.str("key").unwrap_or_default().into(), |n| format!("#{n}"));
                    let fp = fingerprint(ctx, ev, &md, e.issue.map(|n| (f, n)));
                    Line {
                        key,
                        fp,
                        md,
                        issue: e.issue,
                    }
                })
                .collect();
            let fp = fingerprint(ctx, ev, &rows.iter().map(|l| l.fp.as_str()).collect::<String>(), None);
            let card = stuck_card("", f, &rows.iter().collect::<Vec<_>>());
            Owned {
                id: format!("stuck-{f}"),
                fp,
                card,
                group: Some(f.into()),
                rows,
            }
        }));
        let asks = (tab.asks.iter())
            .filter(|a| !self.flying(&[("op", "answer"), ("fleet", &a.ask.fleet), ("id", &a.ask.id)]))
            .map(|a| {
                let at = a.ask.issue.map(|n| (a.ask.fleet.as_str(), n));
                let mut card = ask_card(ctx.now, ask_minutes(ctx), a);
                let tap = [("op", "answer"), ("fleet", &a.ask.fleet), ("id", &a.ask.id)];
                refusal(&mut card, self.refusal(&tap));
                Owned::of(ctx, ev, card, at)
            });
        out.extend(asks.collect::<Vec<_>>());
        let needs = (tab.issues.iter())
            .filter(|i| {
                let n = i.issue.to_string();
                !["extend", "override", "park"]
                    .iter()
                    .any(|op| self.flying(&[("op", op), ("fleet", &i.fleet), ("issue", &n)]))
            })
            .map(|i| {
                let (mut card, n) = (needs_card(i, ev), i.issue.to_string());
                let taps = ["extend", "override", "park"].map(|op| [("op", op), ("fleet", &i.fleet), ("issue", &n)]);
                refusal(&mut card, taps.iter().find_map(|t| self.refusal(t)));
                Owned::of(ctx, ev, card, Some((&i.fleet, i.issue)))
            });
        out.extend(needs.collect::<Vec<_>>());
        out
    }

    pub(super) fn hide_card(&self, ctx: &Ctx, by: &str, v: &Value) -> Result<Value> {
        let ev = working(&ctx.state)?;
        let id = v.at("card").as_str().unwrap_or_default();
        let o = self.owner_cards(ctx, &ev, true).into_iter().find(|o| o.id == id);
        let o = o.with_context(|| format!("no card {id}"))?;
        let marks: Vec<(&str, &str)> = if v.at("all") == &Value::Bool(true) && !o.rows.is_empty() {
            o.rows.iter().map(|l| (l.key.as_str(), l.fp.as_str())).collect()
        } else {
            vec![("", o.fp.as_str())]
        };
        for (row, fp) in marks {
            let d = json!({"card": id, "row": row, "fp": fp, "by": by});
            crate::events::append(
                &ctx.state,
                &Event::new(ctx.now, "drover", None, "card-later", d).passive(),
            )?;
        }
        Ok(json!({"ops": [{"op": "remove", "id": id}], "toast": format!("{id}: later, back when it changes")}))
    }
}

fn refusal(card: &mut Value, why: Option<String>) {
    let id = format!("refused-{}", card.at("id").as_str().unwrap_or_default());
    if let (Some(w), Some(b)) = (why, card.get_mut("blocks").and_then(Value::as_array_mut)) {
        b.push(json!({"type": "text", "id": id, "markdown": w}));
    }
}

pub(super) fn gone_ops(name: &str, block: &str, fleet: &str, issue: &str) -> Vec<Value> {
    let gone = match name {
        "answer" => format!("ask-{block}"),
        "needs-human" => format!("nhc-{fleet}-{issue}"),
        "run-card" => format!("run-{block}"),
        _ => return vec![],
    };
    vec![json!({"op": "remove", "id": gone})]
}
