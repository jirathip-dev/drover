use super::kit::{self, fields, plain, text, wire, word, Kind};
use super::screens::ticking;
use crate::api::{Answered, Msg, RunCard};
use crate::events::{ask_event, closed, Event};
use crate::Ctx;
use blocks as k;
use serde_json::Value;

fn or<'a>(c: bool, a: &'a str, b: &'a str) -> &'a str {
    if c {
        a
    } else {
        b
    }
}

/// The files a message carries.
pub fn files_of(e: &Event) -> impl Iterator<Item = &Value> {
    e.data.get("files").and_then(Value::as_array).into_iter().flatten()
}

fn change<'a>(ctx: &Ctx, events: &'a [Event], e: &Event) -> Option<&'a Event> {
    let id = e.str("change")?;
    let d = ask_event(events, "decision", &e.fleet, id)?;
    let by = d.str("by").unwrap_or_default();
    (by.starts_with("orchestrator-") && ctx.now < d.ts + crate::CHANGE).then_some(())?;
    ask_event(events, "needs-decision", &e.fleet, id)
}

fn ask(events: &[Event], a: &Event, m: Msg) -> Msg {
    let answer = ask_event(events, "decision", &a.fleet, a.str("id").unwrap_or_default()).map(|d| {
        let c = d.str("choice").unwrap_or_default();
        let opts = a.val("options").as_array().into_iter().flatten();
        Answered {
            n: (opts.filter_map(Value::as_str).zip(1..))
                .find(|o| o.0 == c)
                .map(|o| o.1),
            choice: c.into(),
            by: d.str("by").map(Into::into),
            ts: d.ts,
        }
    });
    Msg {
        open: answer.is_none() && closed(events, a).is_none(),
        routine: a.str("kind") == Some("routine"),
        ask: Some(super::v1::ask(a)),
        answer,
        ..m
    }
}

fn run_card(ctx: &Ctx, events: &[Event], e: &Event) -> RunCard {
    let s = |e: &Event, k: &str| e.str(k).unwrap_or_default().to_string();
    let id = s(e, "id");
    let set = crate::card::settled(events, &id);
    let done = events.iter().find(|d| d.kind == "run-done" && d.str("id") == Some(&id));
    let exp = e.ts + crate::card::TTL;
    let state = match set {
        Some(x) if x.val("approved") != true => "denied",
        Some(_) if done.is_none() => "running",
        Some(_) => "approved",
        None if ctx.now > exp => "expired",
        None => "open",
    };
    RunCard {
        id,
        cmd: s(e, "cmd"),
        why: s(e, "why"),
        blast: s(e, "blast"),
        host: s(e, "host"),
        dir: s(e, "dir"),
        sha: s(e, "sha"),
        exp,
        state: state.into(),
        by: set.and_then(|x| x.str("by")).map(Into::into),
        code: done.and_then(|d| d.val("code").as_i64()),
        tail: done.map(|d| s(d, "tail")).unwrap_or_default(),
    }
}

fn owner_card(ctx: &Ctx, events: &[Event], e: &Event) -> Option<Msg> {
    let s = |k| e.str(k).unwrap_or_default().to_string();
    let m = |kind: &str, from: String, text: String, open: bool| Msg {
        seq: e.seq,
        ts: e.ts,
        kind: kind.into(),
        from,
        text,
        open,
        ..Msg::default()
    };
    match e.kind.as_str() {
        "config-propose" => match crate::propose::settled(events, &s("id")) {
            None => Some(m("config", s("id"), s("diff"), !crate::propose::stale(ctx, e))),
            Some(d) if d.val("applied") == true => Some(Msg {
                answer: Some(Answered {
                    n: None,
                    choice: d.str("backup").unwrap_or_default().into(),
                    by: Some("owner".into()),
                    ts: d.ts,
                }),
                ..m("config", s("id"), s("diff"), false)
            }),
            Some(_) => None,
        },
        "secret-request" => {
            let open = crate::secret::open(events, e.str("thread")?, &s("name"));
            open.is_some_and(|o| std::ptr::eq(o, e))
                .then(|| m("secret", s("name"), s("why"), false))
        }
        _ => None,
    }
}

pub(super) fn drawn(ctx: &Ctx, events: &[Event], e: &Event, row: &crate::thread::Row, id: &str) -> Value {
    use crate::thread::RowKind as K;
    let m = Msg {
        seq: e.seq,
        ts: e.ts,
        kind: "note".into(),
        text: row.text.clone(),
        ..Msg::default()
    };
    let m = match row.kind {
        K::Ask | K::Decision => ask(
            events,
            e,
            Msg {
                kind: "ask".into(),
                ..m
            },
        ),
        K::RunCard => Msg {
            kind: "run".into(),
            run: Some(run_card(ctx, events, e)),
            ..m
        },
        K::ConfigCard | K::SecretCard => owner_card(ctx, events, e).unwrap_or(m),
        K::System if change(ctx, events, e).is_some() => Msg {
            ask: change(ctx, events, e).map(super::v1::ask),
            open: true,
            ..m
        },
        K::Said => Msg {
            kind: "msg".into(),
            from: "owner".into(),
            ..m
        },
        _ => m,
    };
    wire(&msg_body(ctx.now, id, &m, true).block())
}

pub(super) fn owner_cards(ctx: &Ctx, events: &[Event], paired: bool) -> Vec<Value> {
    let all = crate::events::of_kind(events, &["config-propose", "secret-request"]);
    let card = |e: &Event| {
        let m = owner_card(ctx, events, e).filter(|m| m.answer.is_none())?;
        Some(wire(&msg_body(ctx.now, e.str("thread")?, &m, paired).block()))
    };
    all.into_iter().filter_map(card).collect()
}

fn run_body(now: u64, key: &str, r: &RunCard) -> (Vec<k::Block>, Vec<k::Block>, k::Tone) {
    let text = |k: &str, t: &str| text(format!("{key}-{k}"), t);
    let host = or(r.host.is_empty(), "local", &r.host);
    if r.state == "open" {
        let sha = r.sha.get(..12).unwrap_or(&r.sha);
        let left = r.exp.saturating_sub(now).div_ceil(60);
        let run = k::Run::new("run-card", r.cmd.as_str(), sha)
            .id(r.id.as_str())
            .dir(r.dir.as_str())
            .touches(r.blast.as_str());
        return (
            vec![run.block(), text("exp", &format!("on {host} · expires in {left}m"))],
            vec![],
            k::Tone::Warn,
        );
    }
    let by = r.by.as_deref().map_or(String::new(), |b| format!(" by {b}"));
    let code = r.code.map_or(String::new(), |c| format!(" · exit {c}"));
    let mut blocks = vec![
        text("cmd", &format!("```\n{}\n```", r.cmd)),
        text(
            "s",
            &format!(
                "on {host} · {} · touches {}  \n**{}**{by}{code}",
                r.dir, r.blast, r.state
            ),
        ),
    ];
    let mut buttons = vec![];
    if !r.tail.is_empty() {
        blocks.push(text("tail", &format!("```\n{}\n```", r.tail)));
        let copy = k::Tap {
            copy: Some(r.tail.clone()),
            ..k::Tap::default()
        };
        buttons.push(k::Button::new("copy").id(format!("{key}-copy")).tap(copy).block());
    }
    let tone = if r.code.is_some_and(|c| c != 0) {
        k::Tone::Bad
    } else {
        k::Tone::Other(String::new())
    };
    (blocks, buttons, tone)
}

fn options(a: &crate::api::Ask) -> Vec<k::ChoiceOption> {
    let rec = recommended(a);
    let (first, rest): (Vec<&String>, Vec<&String>) = a.options.iter().partition(|o| Some(*o) == rec);
    (first.into_iter().chain(rest))
        .map(|o| k::ChoiceOption {
            label: o.clone(),
            value: o.clone(),
            ..k::ChoiceOption::default()
        })
        .collect()
}

fn recommended(a: &crate::api::Ask) -> Option<&String> {
    a.recommend
        .and_then(|n| a.options.get(usize::try_from(n).ok()?.checked_sub(1)?))
}

fn other() -> k::Other {
    k::Other {
        placeholder: Some("other: type an answer".into()),
        r#for: None,
    }
}

fn note(key: &str, k: &str, t: &str) -> k::Block {
    text(format!("{key}-{k}"), t)
}

fn card(key: &str, kind: &str) -> k::Card {
    k::Card::new().id(key).kind(word::<k::CardKind>(kind))
}

fn act_button(key: &str, label: &str, style: &str, action: &str, values: k::Fields) -> k::Button {
    kit::button(format!("{key}-{}", label.replace(' ', "-")), label, action, values)
        .style(word::<k::ButtonStyle>(style))
}

fn ask_body(now: u64, key: &str, id: &str, m: &Msg, age: k::Time) -> k::Card {
    let text = |k: &str, t: &str| note(key, k, t);
    let card = |kind: &str| card(key, kind);
    let Some(a) = &m.ask else {
        return card("sys").blocks(vec![text("t", &m.text)]);
    };
    let mut blocks = vec![text("t", &a.question)];
    if let Some(d) = &m.answer {
        let by = d.by.as_deref().unwrap_or("you");
        let when = crate::events::age(now.saturating_sub(d.ts));
        blocks.push(text("a", &format!("✓ **{}** · {by} · {when}", d.choice)));
    } else if m.open {
        let mut ch = k::Choice::new("answer", options(a))
            .id(a.id.as_str())
            .values(fields([("thread", id.into())]))
            .other(other());
        if let Some(r) = recommended(a) {
            ch = ch.recommended(r.as_str());
        }
        blocks.push(ch.block());
    }
    let tone = if m.open && m.answer.is_none() {
        k::Tone::Warn
    } else {
        k::Tone::Other(String::new())
    };
    card("ask")
        .tone(tone)
        .title(format!("{}{}", a.id, or(m.routine, " · routine", "")))
        .meta(age)
        .blocks(blocks)
}

fn config_body(now: u64, key: &str, id: &str, m: &Msg, age: k::Time) -> k::Card {
    let text = |k: &str, t: &str| note(key, k, t);
    let card = |kind: &str| card(key, kind);
    let button =
        |label: &str, style: &str, action: &str, values: k::Fields| act_button(key, label, style, action, values);
    if let Some(d) = m.answer.as_ref() {
        let by = d.by.as_deref().unwrap_or("owner");
        let when = crate::events::age(now.saturating_sub(d.ts));
        let sub = format!("applied by {by} · {when} · backup {}", d.choice);
        card("config")
            .title(format!("config proposal {}", m.from))
            .subtitle(plain(sub))
            .meta(age)
            .blocks(vec![text("d", &format!("```diff\n{}\n```", m.text.trim_end()))])
    } else {
        let v = |act: &str| {
            fields([
                ("thread", id.into()),
                ("id", m.from.as_str().into()),
                ("act", act.into()),
            ])
        };
        let mut buttons = vec![button("discard", "", "config-card", v("discard")).block()];
        if m.open {
            buttons.insert(0, button("apply", "primary", "config-card", v("apply")).block());
        }
        let sub = or(
            m.open,
            "a fleet.toml change for you to apply",
            "stale: fleet.toml changed since, it can no longer apply",
        );
        card("config")
            .tone(if m.open { k::Tone::Warn } else { k::Tone::Bad })
            .title(format!("config proposal {}", m.from))
            .subtitle(sub)
            .meta(age)
            .blocks(vec![text("d", &format!("```diff\n{}\n```", m.text.trim_end()))])
            .buttons(buttons)
    }
}

fn secret_body(key: &str, id: &str, m: &Msg, paired: bool, age: k::Time) -> k::Card {
    let text = |k: &str, t: &str| note(key, k, t);
    let card = |kind: &str| card(key, kind);
    let button =
        |label: &str, style: &str, action: &str, values: k::Fields| act_button(key, label, style, action, values);
    let mut blocks = vec![text("t", &m.text)];
    let mut buttons = vec![];
    if paired {
        let field = k::SecretField::new("text")
            .label("")
            .placeholder(format!("value of {}", m.from))
            .value("");
        let submit = k::Submit {
            label: "save".into(),
            action: "secret-card".into(),
            values: Some(fields([
                ("thread", id.into()),
                ("name", m.from.as_str().into()),
                ("act", "save".into()),
            ])),
            ..k::Submit::default()
        };
        let form = k::Form::new(vec![field.into()]).id(format!("{key}-f")).submit(submit);
        blocks.push(form.block());
        buttons.push(
            button(
                "dismiss",
                "",
                "secret-card",
                fields([
                    ("thread", id.into()),
                    ("name", m.from.as_str().into()),
                    ("act", "dismiss".into()),
                ]),
            )
            .block(),
        );
    } else {
        blocks.push(text("p", "pair this device to enter it"));
    }
    card("secret")
        .tone(k::Tone::Warn)
        .title(format!("secret {}", m.from))
        .meta(age)
        .blocks(blocks)
        .buttons(buttons)
}

fn msg_body(now: u64, id: &str, m: &Msg, paired: bool) -> k::Card {
    let (key, age) = (format!("m-{}", m.seq), ticking(m.ts, "{}"));
    let text = |k: &str, t: &str| note(&key, k, t);
    let card = |kind: &str| card(&key, kind);
    let sys = |t: &str| card("sys").blocks(vec![text("t", t)]);
    match m.kind.as_str() {
        "msg" => card("you")
            .title("you")
            .meta(ticking(m.ts, "✓ {}"))
            .blocks(vec![text("t", &m.text)]),
        "ask" => ask_body(now, &key, id, m, age),
        "note" if m.open && m.ask.is_some() => {
            let Some(a) = &m.ask else { return sys(&m.text) };
            let mins = crate::CHANGE / 60;
            let ch = k::Choice::new("answer", options(a))
                .id(a.id.as_str())
                .values(fields([("thread", id.into()), ("fleet", a.fleet.as_str().into())]))
                .other(other());
            card("ask")
                .title(format!("{} · answered by the orchestrator", a.id))
                .meta(age)
                .blocks(vec![
                    text("t", &m.text),
                    text("c", &format!("change it within {mins} min of the answer:")),
                    ch.block(),
                ])
        }
        "config" => config_body(now, &key, id, m, age),
        "secret" => secret_body(&key, id, m, paired, age),
        "run" => {
            let Some(r) = &m.run else { return sys(&m.text) };
            let (blocks, buttons, tone) = run_body(now, &key, r);
            card("run")
                .tone(tone)
                .title(r.why.as_str())
                .meta(age)
                .blocks(blocks)
                .buttons(buttons)
        }
        _ => sys(&m.text),
    }
}

/// A secret card's [save] or [dismiss]: `t` (the chat), `name`, `act`, `value`.
pub fn secret(
    ctx: &Ctx,
    r: &dyn crate::runner::Runner,
    f: &std::collections::BTreeMap<String, String>,
) -> anyhow::Result<String> {
    let g = |k: &str| f.get(k).map_or("", String::as_str);
    let cfg = crate::config::Config::kept(ctx)?;
    crate::secret::settle(
        ctx,
        &cfg,
        r,
        (g("t"), g("name")),
        (g("act") == "save").then(|| g("value")),
    )
}
