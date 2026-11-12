use super::{devices, form, header, mtime, shared, shown, Auth, BTreeMap, Config, Ctx, Event, Refusal, Result, Server};
use super::{NeedAsk, NeedIssue, QueueCard, QueueRow, JSON};
use crate::access::{civil_month_day, civil_year};
use anyhow::Context;
use serde_json::{json, Map, Value};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

const MANIFEST: &str = include_str!("../../../cooee-plugin.toml");
static STREAMS: AtomicUsize = AtomicUsize::new(0);
type Res = (&'static str, &'static str, Vec<u8>);
type Q = BTreeMap<String, String>;

fn out(status: &'static str, v: &Value) -> Res {
    (status, JSON, v.to_string().into_bytes())
}

fn line(status: &'static str, l: &str) -> Res {
    out(status, &json!({ "line": l }))
}

/// Whether `cooee-plugin.toml`'s `[permissions]` lists `action`.
pub fn permitted(action: &str) -> bool {
    let m: toml::Value = toml::from_str(MANIFEST).unwrap_or(toml::Value::Integer(0));
    let list = m.get("permissions").and_then(|p| p.get("actions")?.as_array().cloned());
    list.unwrap_or_default().iter().any(|a| a.as_str() == Some(action))
}

fn iso(t: u64) -> String {
    let (d, s) = (t / 86_400, t % 86_400);
    let (m, day) = civil_month_day(d);
    format!(
        "{}-{m:02}-{day:02}T{:02}:{:02}:{:02}Z",
        civil_year(d),
        s / 3600,
        s / 60 % 60,
        s % 60
    )
}

fn tool(role: &str) -> &'static str {
    match role {
        "rev" | "review" => "read",
        "fix" | "update" => "edit",
        _ => "run",
    }
}

fn figure(act: &str, state: &str) -> Value {
    match act {
        "impl" | "fix" | "rev" | "update" => json!({"state": "working", "tool": tool(act)}),
        "done" => json!({"state": "done"}),
        "blocked" if state == "dead" || state == "blocked" => json!({"state": "fail"}),
        "blocked" => json!({"state": "needs"}),
        _ => json!({"state": "idle"}),
    }
}

fn badge_tone(fig: &Value) -> &str {
    fig["state"].as_str().filter(|s| *s != "idle").unwrap_or("mute")
}

fn rounds(events: &[Event], f: &str, n: u64) -> String {
    let mut parts: Vec<(String, &str)> = vec![];
    let (mut revs, mut fixes) = (0, 0);
    for e in events.iter().filter(|e| e.fleet == f && e.issue == Some(n)) {
        let role = e.str("role").unwrap_or_default();
        match e.kind.as_str() {
            "spawn" => {
                if let Some(p) = parts.last_mut().filter(|p| p.1.is_empty()) {
                    p.1 = "✓";
                }
                let name = match role {
                    "rev" => (revs += 1, format!("review {revs}")).1,
                    "fix" => (fixes += 1, format!("fix {fixes}")).1,
                    r => r.to_string(),
                };
                parts.push((name, ""));
            }
            "verdict" => {
                if let Some(p) = parts.last_mut() {
                    p.1 = if e.str("verdict") == Some("PASS") { "✓" } else { "✗" };
                }
            }
            _ => {}
        }
    }
    let shown = parts
        .iter()
        .map(|(n, m)| if m.is_empty() { n.clone() } else { format!("{n} {m}") });
    shown.collect::<Vec<_>>().join(" · ")
}

fn row(ev: &[Event], repo: &str, f: &str, r: &QueueRow, k: Option<usize>) -> Value {
    let fig = figure(&r.act, &r.state);
    let title = format!("#{} {}", r.issue, r.title.clone().unwrap_or_default());
    let state = r.state.replace('(', " ").replace(')', "").replace("⏸ waiting: ", "");
    let badge = match k {
        Some(k) => json!({"text": format!("#{}", k + 1), "tone": "mute"}),
        None => json!({"text": state, "tone": badge_tone(&fig)}),
    };
    let mut v = json!({"type": "row", "id": format!("q-{f}-{}", r.issue), "figure": fig, "title": title.trim_end(), "badge": badge});
    let mut sub = rounds(ev, f, r.issue);
    let live = r.live && !r.dead && !r.lane.is_empty();
    if live {
        let at = [r.lane.as_str(), r.place.as_str()];
        sub = format!(
            "{sub} ▸ {}",
            at.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
        );
    }
    if r.pr.ci.as_deref() == Some("green") {
        sub = format!("{sub} · CI ✓");
    }
    let vals = json!({"fleet": f, "issue": r.issue, "agent": r.lane});
    if let Some(b) = r.blocked_by {
        v["indent"] = json!(1);
        v["subtitle"] = json!({"text": format!("held until #{b}"), "tone": "bad"});
    } else if !sub.is_empty() {
        v["subtitle"] = json!(sub.trim_start_matches(" ▸ ").trim_start_matches(" · "));
    }
    if k.is_none() {
        v["value"] = json!({ "since": iso(r.t) });
    }
    if live {
        v["tap"] = json!({ "thread": r.lane });
        let confirm = json!({"title": format!("Stop #{}'s lane {}?", r.issue, r.lane), "ok": "Stop lane"});
        v["swipe"] = json!([{"label": "Stop", "tone": "bad", "action": "stop", "values": vals, "confirm": confirm}]);
    } else if let Some(pr) = r.pr.pr {
        v["tap"] = json!({ "url": format!("https://github.com/{repo}/pull/{pr}") });
    }
    if k.is_some() {
        let confirm = json!({"title": format!("Drop #{} from the queue?", r.issue), "ok": "Drop"});
        v["swipe"] = json!([{"label": "Top", "action": "top", "values": vals},
            {"label": "Drop", "tone": "bad", "action": "drop", "values": vals, "confirm": confirm}]);
    }
    v
}

fn fleet_card(ev: &[Event], c: &QueueCard) -> Value {
    let f = &c.name;
    let (queued, busy): (Vec<&QueueRow>, Vec<&QueueRow>) = c.rows.iter().partition(|r| r.state == "queued");
    let rows = |rs: &[&QueueRow], q: bool| -> Vec<Value> {
        let k = |i| q.then_some(i);
        rs.iter()
            .enumerate()
            .map(|(i, r)| row(ev, &c.config.repo, f, r, k(i)))
            .collect()
    };
    let section = |id: &str, word: &str, rs: Vec<Value>| {
        json!({"type": "section", "id": format!("{id}-{f}"), "header": format!("{word} ({})", rs.len()),
            "blocks": [{"type": "list", "id": format!("{id}-list-{f}"), "rows": rs}]})
    };
    let paused = c.paused.is_some();
    let badge = [
        json!({"text": "driving", "tone": "ok"}),
        json!({"text": "paused", "tone": "mute"}),
    ];
    let done =
        json!({"type": "row", "id": format!("done-today-{f}"), "title": "done today", "value": c.done_n.to_string()});
    json!({"type": "card", "id": format!("fleet-{f}"), "tone": "accent", "title": f,
        "subtitle": c.paused.clone().unwrap_or_else(|| c.drive.line.clone()), "badge": badge[usize::from(paused)],
        "buttons": [
            {"type": "button", "id": format!("drive-{f}"), "label": "Drive now", "style": "primary", "action": "drive", "values": {"fleet": f}},
            {"type": "button", "id": format!("pause-{f}"), "label": if paused { "Resume" } else { "Pause" }, "action": "pause", "values": {"fleet": f, "on": !paused}}],
        "blocks": [section("inprogress", "in progress", rows(&busy, false)), section("queued", "queued", rows(&queued, true)),
            {"type": "list", "id": format!("done-{f}"), "rows": [done]}]})
}

fn stats(now: u64, ev: &[Event], states: &crate::events::States, only: Option<&str>) -> Value {
    let day = |e: &Event| (now.saturating_sub(e.ts) / 86_400) as usize;
    let merged: Vec<&Event> = ev
        .iter()
        .filter(|e| e.kind == "merged" && day(e) < 7 && only.is_none_or(|f| e.fleet == f))
        .collect();
    let mut per = [(0u32, 0u32); 7];
    for e in &merged {
        let r = states
            .get(&(e.fleet.clone(), e.issue.unwrap_or(0)))
            .map_or(0, |s| s.rounds);
        let p = &mut per[6 - day(e)];
        (p.0, p.1) = (p.0 + 1, p.1 + r);
    }
    let avg = |n: u32, r: u32| {
        if n == 0 {
            0.0
        } else {
            (f64::from(r) / f64::from(n) * 10.0).round() / 10.0
        }
    };
    let (n, r) = per.iter().fold((0, 0), |a, p| (a.0 + p.0, a.1 + p.1));
    let lanes = states.values().filter(|s| only.is_none_or(|f| s.fleet == f));
    let lanes = lanes
        .filter(|s| s.lane.is_some() && !s.done() && s.state != "queued")
        .count();
    json!({"type": "stat", "id": "stats", "items": [
        {"label": "merged · 7d", "value": n.to_string(), "spark": per.map(|p| p.0)},
        {"label": "lanes", "value": lanes.to_string()},
        {"label": "rounds / issue", "value": format!("{:.1}", avg(n, r)), "spark": per.map(|p| avg(p.0, p.1))}]})
}

fn choice(id: &str, action: &str, options: &[Value]) -> Value {
    json!({"type": "choice", "id": id, "action": action, "options": options})
}

fn fold(n: Option<u64>, kind: &str, q: &str, age: &str, main: &Value) -> Value {
    let mut cut: String = q.chars().take(48).collect();
    if cut.len() < q.len() {
        cut.push('…');
    }
    let n = n.map_or(String::new(), |n| format!("#{n} · "));
    json!({"line": format!("{n}{kind} · {cut} · {age}"), "main": main})
}

fn ask_card(now: u64, mins: u64, a: &NeedAsk) -> Value {
    let q = &a.ask;
    let opts: Vec<Value> = q.options.iter().map(|o| json!({"value": o, "label": o})).collect();
    let mut c = choice(&q.id, "answer", &opts);
    let rec = q
        .recommend
        .and_then(|n| q.options.get(usize::try_from(n).ok()?.checked_sub(1)?));
    if let Some(r) = rec {
        c["recommended"] = json!(r);
    }
    let routine = a.kind.starts_with("routine");
    c["other"] = match q.options.iter().find(|o| *o == "revise") {
        Some(r) => json!({"placeholder": "a revise note…", "for": r}),
        None => json!({"placeholder": "other…"}),
    };
    if let Some(r) = rec.filter(|_| routine) {
        let until = iso(q.ts + 60 * mins);
        c["default"] = json!({"value": r, "until": until, "label": "takes the recommendation in {rel}"});
    }
    let issue = q.issue.map_or(String::new(), |n| format!(" #{n}"));
    let design = if a.pngs.is_empty() { "" } else { " · design" };
    let mut blocks = vec![json!({"type": "text", "markdown": format!("**{}**", q.question)})];
    if !a.pngs.is_empty() {
        let n = q.issue.unwrap_or(0);
        let items = (0..a.pngs.len()).map(|i| json!({"src": format!("/design/{}/{n}/{i}", q.fleet), "alt": a.pngs[i]}));
        blocks.push(json!({"type": "images", "id": format!("pngs-{}", q.id), "items": items.collect::<Vec<_>>()}));
    }
    blocks.push(c);
    let kind = match (design, routine) {
        ("", false) => "owner",
        ("", true) => "routine",
        _ => "design",
    };
    let age = crate::events::age(now.saturating_sub(q.ts));
    let main = rec
        .or(q.options.first())
        .map(|o| json!({"block": q.id, "action": "answer", "value": o, "label": o}));
    json!({"type": "card", "id": format!("ask-{}", q.id), "kind": "ask", "tone": "warn",
        "title": format!("{}{issue}{design}", q.fleet), "meta": format!("{kind} · {age}"),
        "fold": fold(q.issue, kind, &q.question, &age, &main.unwrap_or_default()), "blocks": blocks})
}

fn needs_card(i: &NeedIssue, ev: &crate::events::Log) -> Value {
    let (f, n) = (&i.fleet, i.issue);
    let st = ev.states.get(&(f.clone(), n));
    let action = st.map_or("one more round", |s| s.card());
    let vals = json!({"fleet": f, "issue": n});
    let mut opts = vec![json!({"value": "extend", "label": action, "values": vals})];
    if i.merge {
        let confirm = json!({"title": format!("Merge #{n} past its FAIL?"), "message": "Logged by: owner. CI and serial still gate it.", "ok": "Merge", "destructive": true});
        opts.push(json!({"value": "merge", "label": "merge anyway", "values": vals, "confirm": confirm}));
    }
    opts.push(json!({"value": "park", "label": "park", "values": vals}));
    let fail = if i.fail.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", i.fail)
    };
    let mut blocks = vec![json!({"type": "text", "markdown": format!("**{}**{fail}", i.why)})];
    if !i.pending {
        blocks.push(choice(&format!("nh-{f}-{n}"), "needs-human", &opts));
    }
    let age = i.who.rsplit(" · ").next().unwrap_or_default();
    let main = (!i.pending).then(|| json!({"block": format!("nh-{f}-{n}"), "action": "needs-human", "value": "extend", "label": action, "values": vals}));
    json!({"type": "card", "id": format!("nhc-{f}-{n}"), "kind": "ask", "tone": "bad",
        "title": format!("{f} #{n} · {}", i.title), "meta": format!("needs-human · {age}"),
        "fold": fold(Some(n), "needs-human", &i.why, age, &main.unwrap_or_default()), "blocks": blocks})
}

fn run_cards(now: u64, ev: &[Event]) -> Vec<Value> {
    let open = ev
        .iter()
        .filter(|e| e.kind == "run-card" && now <= e.ts + crate::card::TTL);
    let open = open.filter(|e| e.str("id").is_some_and(|id| crate::card::settled(ev, id).is_none()));
    open.map(|e| {
        let s = |k: &str| e.str(k).unwrap_or_default();
        json!({"type": "card", "id": format!("run-{}", s("id")), "kind": "run", "title": s("why"), "blocks": [
            {"type": "run", "id": s("id"), "action": "run-card", "command": s("cmd"), "sha": &s("sha")[..s("sha").len().min(12)],
                "dir": s("dir"), "touches": s("blast"), "until": iso(e.ts + crate::card::TTL)}]})
    })
    .collect()
}

fn fleets(cfg: &Config) -> Value {
    let mut blocks = vec![];
    for (f, fc) in &cfg.fleet {
        let mut fields = vec![
            json!({"type": "toggle", "name": format!("{f}.drive"), "label": "Drive", "value": !fc.paused}),
            json!({"type": "text", "name": format!("{f}.integration"), "label": "Branch", "value": fc.integration, "placeholder": "integration branch"}),
        ];
        for (role, label) in [("impl", "Impl"), ("rev", "Review")]
            .into_iter()
            .filter(|_| !cfg.models.is_empty())
        {
            let Some(r) = fc.role.get(role) else { continue };
            let value = r.model.clone().unwrap_or_default();
            fields.push(json!({"type": "picker", "name": format!("{f}.{role}"), "label": label, "value": value, "options": cfg.models}));
        }
        blocks.push(json!({"type": "section", "id": format!("fleet-{f}"), "header": f, "footer": "Pause stops new lanes; live ones finish.",
            "blocks": [{"type": "form", "id": format!("f-{f}"), "fields": fields}]}));
    }
    for f in cfg.fleet.keys() {
        let confirm = json!({"title": format!("Remove {f}?"), "message": "Only its config section; its queue, lanes and log stay on disk.", "ok": "Remove"});
        blocks.push(
            json!({"type": "button", "id": format!("rm-{f}"), "label": format!("Remove {f}…"), "style": "destructive",
            "action": "remove-fleet", "values": {"fleet": f}, "confirm": confirm}),
        );
    }
    json!({"type": "sheet", "id": "fleets", "title": "Fleets", "cancel": "Cancel", "done": {"label": "Save", "action": "save-fleets"}, "blocks": blocks})
}

/// #845: the statechart and the pass chart as `diagram` blocks: live counts per state, each limit node's key.
fn workflow(ctx: &Ctx) -> Value {
    let limits = Config::kept(ctx).map(|c| c.limits).unwrap_or_default();
    let ev = shared(&ctx.state).unwrap_or_default();
    let counts = crate::diagram::counts(ev.states.values());
    let blocks = crate::diagram::charts(&limits, Some(&counts)).map(|c| {
        let nodes: Vec<Value> = c.nodes.iter().map(|(id, k)| json!({"id": id, "limit": k})).collect();
        let n = if c.id == "states" { json!(counts) } else { json!({}) };
        json!({"type": "diagram", "id": c.id, "source": c.source, "counts": n, "nodes": nodes})
    });
    json!({"type": "screen", "id": "workflow", "title": "Workflow", "blocks": blocks})
}

const KINDS: [(&str, &str); 4] = [
    ("merge", "merges"),
    ("verdict", "verdicts"),
    ("lane", "lanes"),
    ("ask", "asks"),
];

fn kind_of(kind: &str) -> Option<&'static str> {
    match kind {
        "merged" => Some("merge"),
        "verdict" | "needs-human" => Some("verdict"),
        "spawn" => Some("lane"),
        "needs-decision" => Some("ask"),
        _ => None,
    }
}

fn events_screen(ctx: &Ctx, q: &Q) -> Value {
    let before = q.get("before").and_then(|b| b.parse().ok()).unwrap_or(ctx.now);
    let picked: Vec<&str> = q.get("kind").map_or("all", String::as_str).split(',').collect();
    let all = picked.contains(&"all") || picked.iter().all(|k| !KINDS.iter().any(|(v, _)| v == k));
    let log = shared(&ctx.state).unwrap_or_default();
    let since = log
        .iter()
        .find(|e| e.ts + 86_400 >= before)
        .map_or(u64::MAX, |e| e.seq.saturating_sub(1));
    let repos: BTreeMap<String, String> = Config::kept(ctx)
        .map(|c| c.fleet.into_iter().map(|(f, fc)| (f, fc.repo)).collect())
        .unwrap_or_default();
    let list = super::v1::events(ctx, since, u64::MAX, usize::MAX).list;
    let mut rows = vec![];
    for e in list.iter().rev().filter(|e| e.ts < before) {
        let Some(k) = kind_of(&e.kind).filter(|k| all || picked.contains(k)) else {
            continue;
        };
        let fig = match (k, e.tone.as_str()) {
            ("lane", _) => {
                json!({"state": "working", "tool": tool(e.data.get("role").and_then(Value::as_str).unwrap_or_default())})
            }
            (_, "g") => json!({"state": "done"}),
            (_, "r") => json!({"state": "fail"}),
            (_, "a") => json!({"state": "needs"}),
            _ => json!({"state": "idle"}),
        };
        let mut r = json!({"type": "row", "id": format!("e-{}", e.seq), "figure": fig, "title": e.text, "value": {"since": iso(e.ts)}});
        if let Some(t) = e.issue.and_then(|n| crate::gh::cached_title(ctx, &e.fleet, n)) {
            r["subtitle"] = json!(t);
        }
        let s = |k: &str| e.data.get(k).and_then(Value::as_str).filter(|s| !s.is_empty());
        let repo = repos.get(&e.fleet);
        r["tap"] = match (s("agent").filter(|_| k == "lane").or(s("thread")), e.pr.zip(repo)) {
            (Some(t), _) => json!({ "thread": t }),
            (_, Some((pr, repo))) => json!({ "url": format!("https://github.com/{repo}/pull/{pr}") }),
            _ => Value::Null,
        };
        if r["tap"].is_null() {
            r.as_object_mut().map(|o| o.remove("tap"));
        }
        rows.push(r);
    }
    let sel: Vec<&str> = if all { vec!["all"] } else { picked.clone() };
    let mut opts = vec![json!({"value": "all", "label": "all"})];
    opts.extend(KINDS.iter().map(|(v, l)| json!({"value": v, "label": l})));
    let kq = if all {
        String::new()
    } else {
        format!("kind={}&", sel.join(","))
    };
    let header = if before == ctx.now {
        "last 24 h".into()
    } else {
        format!("24 h before {}", iso(before))
    };
    let stream = if before == ctx.now {
        format!("/screens/events/stream?{kq}")
    } else {
        String::new()
    };
    let mut s = json!({"type": "screen", "id": "events", "title": "Events", "blocks": [
        {"type": "chips", "id": "kind", "name": "kind", "multi": true, "selected": sel, "action": "filter", "options": opts},
        {"type": "section", "id": "today", "header": header, "blocks": [{"type": "list", "id": "ev", "rows": rows}]},
        {"type": "list", "id": "older-list", "rows": [{"type": "row", "id": "older", "title": "earlier days", "style": "link",
            "tap": {"open": format!("/screens/events?{kq}before={}", before.saturating_sub(86_400))}}]}]});
    if !stream.is_empty() {
        s["stream"] = json!(stream.trim_end_matches(['?', '&']));
    }
    s
}

fn own(v: &Value) -> Map<String, Value> {
    let mut m = v.as_object().cloned().unwrap_or_default();
    m.remove("blocks");
    m.remove("rows");
    m
}

fn kids(v: &Value) -> &[Value] {
    v.get("blocks")
        .or_else(|| v.get("rows"))
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn walk(a: &Value, b: &Value, out: &mut Vec<Value>) -> bool {
    let (id, mark) = (b["id"].as_str(), out.len());
    let (oa, ob) = (own(a), own(b));
    if oa != ob {
        let Some(id) = id else { return false };
        if oa.keys().any(|k| !ob.contains_key(k)) {
            out.push(json!({"op": "replace", "id": id, "block": b}));
            return true;
        }
        let fields: Map<String, Value> = ob.into_iter().filter(|(k, v)| oa.get(k) != Some(v)).collect();
        out.push(json!({"op": "merge", "id": id, "fields": fields}));
    }
    if children(id, kids(a), kids(b), out) {
        return true;
    }
    out.truncate(mark);
    id.is_some_and(|id| (out.push(json!({"op": "replace", "id": id, "block": b})), true).1)
}

fn children(into: Option<&str>, ka: &[Value], kb: &[Value], out: &mut Vec<Value>) -> bool {
    fn ids(k: &[Value]) -> Option<Vec<&str>> {
        k.iter().map(|c| c["id"].as_str()).collect()
    }
    fn both<'a>(x: &[&'a str], y: &[&str]) -> Vec<&'a str> {
        x.iter().filter(|i| y.contains(i)).copied().collect()
    }
    let (Some(into), Some(ia), Some(ib)) = (into, ids(ka), ids(kb)) else {
        return ka.len() == kb.len() && ka.iter().zip(kb).all(|(x, y)| x["id"] == y["id"] && walk(x, y, out));
    };
    if both(&ia, &ib) != both(&ib, &ia) {
        return false;
    }
    out.extend(
        ia.iter()
            .filter(|i| !ib.contains(i))
            .map(|i| json!({"op": "remove", "id": i})),
    );
    for (k, (i, c)) in ib.iter().zip(kb).enumerate() {
        if let Some(p) = ia.iter().position(|x| x == i) {
            walk(&ka[p], c, out);
            continue;
        }
        let mut op = json!({"op": "insert", "into": into, "block": c});
        if let Some(next) = ib[k + 1..].iter().find(|x| ia.contains(x)) {
            op["before"] = json!(next);
        }
        out.push(op);
    }
    true
}

/// The ops that turn screen `a` into `b`, by block id (removes first); a change no id can address: the whole screen.
pub fn ops(a: &Value, b: &Value) -> Vec<Value> {
    let mut out = vec![];
    if a["id"] != b["id"] || !walk(a, b, &mut out) || out.iter().any(|o| o["id"] == b["id"] && o["op"] == "replace") {
        return vec![json!({"op": "screen", "screen": b})];
    }
    out.sort_by_key(|o| o["op"] != "remove");
    out
}

fn count(v: &Value) -> Option<usize> {
    let busy = |b: &&Value| b["id"].as_str().is_some_and(|i| i.starts_with("inprogress-"));
    let cards = kids(v).iter().filter(|b| b["type"] == "card");
    (v["id"] == "queue").then(|| {
        cards
            .flat_map(|c| kids(c).iter().filter(busy))
            .map(|s| kids(&kids(s)[0]).len())
            .sum()
    })
}

impl Server<'_> {
    fn screen(&self, ctx: &Ctx, path: &str, q: &Q) -> Option<Value> {
        let mut v = match path {
            "/screens/queue" => self.queue_screen(ctx, q),
            "/screens/events" => events_screen(ctx, q),
            "/screens/fleets" => fleets(&Config::kept(ctx).ok()?),
            "/screens/cards" => self.chat_cards(ctx),
            "/screens/workflow" => workflow(ctx),
            _ => return None,
        };
        let oldest = self.r.refreshed().into_values().min();
        let old = oldest.map(|t| ctx.now.saturating_sub(t)).filter(|a| *a > 10);
        if let (Some(a), Some(b)) = (old, v.get_mut("blocks").and_then(Value::as_array_mut)) {
            let line = format!("data {} old", crate::events::age(a));
            b.insert(0, json!({"type": "text", "id": "age", "markdown": line}));
        }
        Some(v)
    }

    fn queue_screen(&self, ctx: &Ctx, q: &Q) -> Value {
        let (tab, ev) = (self.queue_tab(ctx), shared(&ctx.state).unwrap_or_default());
        let sel = q
            .get("repo")
            .filter(|r| tab.fleets.iter().any(|f| &f.name == *r))
            .map(String::as_str);
        let mut opts = vec![json!({"value": "all", "label": "all"})];
        opts.extend(tab.fleets.iter().map(|f| json!({"value": f.name, "label": f.name})));
        let mut blocks = vec![
            json!({"type": "chips", "id": "repo", "name": "repo", "selected": [sel.unwrap_or("all")], "action": "filter", "options": opts}),
            stats(ctx.now, &ev, &ev.states, sel),
        ];
        if let Some(e) = &tab.error {
            blocks.push(json!({"type": "text", "id": "error", "markdown": e}));
        }
        blocks.extend(
            tab.fleets
                .iter()
                .filter(|f| sel.is_none_or(|s| f.name == s))
                .map(|c| fleet_card(&ev, c)),
        );
        let stream = sel.map_or("/screens/queue/stream".into(), |s| {
            format!("/screens/queue/stream?repo={s}")
        });
        let menu = [
            json!({"label": "Fleets…", "tap": {"sheet": "/screens/fleets"}}),
            json!({"label": "Drive all now", "action": "drive-all"}),
        ];
        json!({"type": "screen", "id": "queue", "title": "Queue", "stream": stream,
            "actions": [{"icon": "more", "label": "More", "menu": menu}], "blocks": blocks})
    }

    fn chat_cards(&self, ctx: &Ctx) -> Value {
        let tab = self.needs_tab(ctx, true);
        let mins = Config::kept(ctx).map_or(crate::config::Limits::default().ask_default_minutes, |c| {
            c.limits.ask_default_minutes
        });
        let ev = shared(&ctx.state).unwrap_or_default();
        let mut cards = run_cards(ctx.now, &ev);
        cards.extend(super::v1::slo_open(&ev).map(|e| {
            json!({"type": "card", "id": format!("slo-{}", e.seq), "kind": "alert", "tone": "warn",
                "title": "The board is slow", "blocks": [{"type": "text", "id": "slo-why", "markdown": e.str("reason")}]})
        }));
        cards.extend(crate::slo::open_stuck(&ev).into_iter().map(|e| {
            json!({"type": "card", "id": format!("stuck-{}", e.seq), "kind": "alert", "tone": "warn",
                "title": "Drive is stuck", "blocks": [{"type": "text", "id": "stuck-why", "markdown": crate::events::plain(e)}]})
        }));
        cards.extend(tab.asks.iter().map(|a| ask_card(ctx.now, mins, a)));
        cards.extend(tab.issues.iter().map(|i| needs_card(i, &ev)));
        Value::Array(cards)
    }

    fn plugin(ctx: &Ctx, head: &str) -> bool {
        let file = std::fs::read_to_string(ctx.config.with_file_name("cooee-token")).unwrap_or_default();
        let bearer = header(head, "authorization")
            .and_then(|h| h.strip_prefix("Bearer "))
            .unwrap_or_default();
        [
            std::env::var("COOEE_PLUGIN_TOKEN").unwrap_or_default(),
            file.trim().into(),
        ]
        .iter()
        .any(|t| !t.is_empty() && devices::sha256(bearer.as_bytes()) == devices::sha256(t.as_bytes()))
    }

    pub(super) fn screens(
        &self,
        ctx: &Ctx,
        head: &str,
        (method, p): (&str, &str),
        query: &str,
        body: &str,
        auth: &Auth,
    ) -> Res {
        let plugin = Self::plugin(ctx, head);
        if !plugin && !self.allowed(ctx, head) {
            return line("403 Forbidden", "foreign Host or Origin");
        }
        match (method, p) {
            ("GET", p) => match self.screen(ctx, p, &form(query)) {
                Some(v) => out("200 OK", &v),
                None => line("404 Not Found", "no such screen"),
            },
            ("POST", "/action") if self.addr.is_none() => line("405 Method Not Allowed", "read-only: GET only"),
            ("POST", "/action") if !plugin && auth.is_err() => line("401 Unauthorized", "pair this device first"),
            ("POST", "/action") => {
                let a: Value = serde_json::from_str(body).unwrap_or_default();
                let name = a["action"].as_str().unwrap_or_default();
                if !permitted(name) {
                    let why =
                        format!("REFUSED NOT_PERMITTED: `{name}` is not in cooee-plugin.toml's [permissions] actions");
                    return line("403 Forbidden", &why);
                }
                match self.action(ctx, name, &a) {
                    Ok(v) => out("200 OK", &v),
                    Err(e) if e.downcast_ref::<Refusal>().is_some() => line("409 Conflict", &shown(&e)),
                    Err(e) => line("400 Bad Request", &shown(&e)),
                }
            }
            _ => line("404 Not Found", "no such screen route"),
        }
    }

    fn form_op(&self, ctx: &Ctx, op: &str, kv: &[(&str, String)]) -> Result<String> {
        let mut f: Q = kv.iter().map(|(k, v)| ((*k).into(), v.clone())).collect();
        f.extend([("op".into(), op.into()), ("mtime".into(), mtime(ctx))]);
        self.op(ctx, f)
    }

    fn action(&self, ctx: &Ctx, name: &str, a: &Value) -> Result<Value> {
        let v = &a["values"];
        let s = |k: &str| match &v[k] {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            x => x.to_string(),
        };
        let (fleet, block, cfg) = (s("fleet"), a["block"].as_str().unwrap_or_default(), Config::kept(ctx)?);
        let n = || s("issue").parse::<u64>().context("no issue");
        let toast = match name {
            "filter" => {
                let (screen, q) = match a["screen"].as_str() {
                    Some("events") => ("/screens/events", format!("kind={}", list(&v["kind"]))),
                    _ => ("/screens/queue", format!("repo={}", list(&v["repo"]))),
                };
                let s = self.screen(ctx, screen, &form(&q)).context("no such screen")?;
                return Ok(json!({"ops": [{"op": "screen", "screen": s}]}));
            }
            "drive" => self.form_op(ctx, "drive", &[("fleet", fleet)])?,
            "drive-all" => {
                let live = cfg.fleet.iter().filter(|f| !f.1.paused);
                let lines: Vec<String> = live
                    .map(|(f, _)| self.form_op(ctx, "drive", &[("fleet", f.clone())]))
                    .collect::<Result<_>>()?;
                lines.join("; ")
            }
            "pause" => self.form_op(
                ctx,
                ["resume", "pause"][usize::from(v["on"] != false)],
                &[("fleet", fleet)],
            )?,
            "top" | "drop" => {
                let d = crate::queue::op(
                    ["mv", "rm"][usize::from(name == "drop")],
                    &[n()?],
                    name == "top",
                    None,
                    None,
                    None,
                );
                crate::queue_change(ctx, &cfg, &fleet, d, Some("cooee"))?
            }
            "stop" => {
                crate::check::stop(ctx, &cfg, &self.r, &mut vec![], &s("agent"))?;
                format!("stopped {}", s("agent"))
            }
            "answer" => {
                let open = crate::events::open_asks(&shared(&ctx.state)?)
                    .into_iter()
                    .find(|e| e.str("id") == Some(block))
                    .cloned();
                let ask = open.with_context(|| format!("no open ask {block}"))?;
                let pick = if s("other").trim().is_empty() {
                    s("choice")
                } else {
                    s("other")
                };
                self.form_op(
                    ctx,
                    "answer",
                    &[("fleet", ask.fleet), ("id", block.into()), ("choice", pick)],
                )?
            }
            "needs-human" => {
                let pick = s("choice");
                let op = match pick.as_str() {
                    "merge" => "override",
                    c @ ("extend" | "park") => c,
                    c => anyhow::bail!("no needs-human choice `{c}`"),
                };
                self.form_op(ctx, op, &[("fleet", fleet), ("issue", n()?.to_string())])?
            }
            "run-card" => {
                let sha = Some(s("sha")).filter(|s| !s.is_empty());
                crate::card::settle_via(
                    ctx,
                    &cfg,
                    &self.r,
                    block,
                    s("choice") == "approve",
                    sha.as_deref(),
                    Some("cooee"),
                )?
            }
            "save-fleets" => self.save_fleets(ctx, &cfg, v)?,
            "remove-fleet" => {
                let hash = crate::propose::view(ctx)?.hash;
                let c = crate::propose::ConfigChange {
                    hash,
                    remove: Some(fleet),
                    ..Default::default()
                };
                crate::propose::edit(ctx, &cfg, &self.r, &c, "owner (cooee)")?
            }
            other => anyhow::bail!("no handler for `{other}`"),
        };
        let close = ["save-fleets", "remove-fleet"].contains(&name);
        let ops: Vec<Value> = (name == "answer")
            .then(|| json!({"op": "remove", "id": format!("ask-{block}")}))
            .into_iter()
            .collect();
        Ok(json!({"ops": ops, "toast": toast, "close": close}))
    }

    fn save_fleets(&self, ctx: &Ctx, cfg: &Config, v: &Value) -> Result<String> {
        let (mut done, mut set) = (vec![], vec![]);
        for (f, fc) in &cfg.fleet {
            let val = |k: &str| v.get(format!("{f}.{k}")).filter(|x| !x.is_null());
            if let Some(on) = val("drive").and_then(Value::as_bool).filter(|on| *on == fc.paused) {
                done.push(self.form_op(ctx, ["pause", "resume"][usize::from(on)], &[("fleet", f.clone())])?);
            }
            for role in ["impl", "rev"] {
                let cur = fc.role.get(role).and_then(|r| r.model.clone()).unwrap_or_default();
                if let Some(m) = val(role).and_then(Value::as_str).filter(|m| !m.is_empty() && *m != cur) {
                    let wanted = vec![("model", m.to_string())];
                    crate::set(ctx, &mut vec![], f, role, wanted, Some("cooee"))?;
                    done.push(format!("{f} {role}: model {m}"));
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

    pub(super) fn stream(
        &self,
        s: &mut std::net::TcpStream,
        req: &str,
        path: &str,
        query: &str,
    ) -> std::io::Result<()> {
        let (head, base) = (
            req.split("\r\n\r\n").next().unwrap_or_default(),
            path.trim_end_matches("/stream"),
        );
        let ctx = || Ctx {
            now: crate::now(),
            ..self.ctx.clone()
        };
        let (q, open) = (form(query), STREAMS.fetch_add(1, Relaxed) < 4);
        let screen = |c: &Ctx| crate::runner::request(|| self.screen(c, base, &q));
        let first = screen(&ctx()).filter(|_| open && (Self::plugin(self.ctx, head) || self.allowed(self.ctx, head)));
        let Some(mut cur) = first else {
            STREAMS.fetch_sub(1, Relaxed);
            let r = line(
                ["404 Not Found", "503 Service Unavailable"][usize::from(!open)],
                "no stream",
            );
            let h = format!(
                "HTTP/1.1 {}\r\nContent-Type: {JSON}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                r.0,
                r.2.len()
            );
            return s.write_all(&[h.as_bytes(), &r.2].concat());
        };
        let mut seq = header(head, "last-event-id")
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(0)
            + 1;
        let mut start = vec![json!({"op": "screen", "screen": cur})];
        start.extend(count(&cur).map(|n| json!({"op": "badge", "count": n})));
        let mut msg = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\n\r\nid: {seq}\ndata: {}\n\n", json!({"seq": seq, "ops": start}));
        let (mut log, mut ping) = (crate::db::last_seq(&self.ctx.state), Instant::now());
        let mut built = Instant::now();
        let res = loop {
            if let Err(e) = s.write_all(msg.as_bytes()) {
                break Err(e);
            }
            msg.clear();
            while msg.is_empty() {
                std::thread::sleep(Duration::from_millis(500));
                let now = crate::db::last_seq(&self.ctx.state);
                if now != log || built.elapsed() >= Duration::from_secs(30) {
                    (log, built) = (now, Instant::now());
                    let new = screen(&ctx()).unwrap_or_else(|| cur.clone());
                    let mut change = ops(&cur, &new);
                    change.extend(
                        count(&new)
                            .filter(|n| Some(*n) != count(&cur))
                            .map(|n| json!({"op": "badge", "count": n})),
                    );
                    if !change.is_empty() {
                        seq += 1;
                        msg = format!("id: {seq}\ndata: {}\n\n", json!({"seq": seq, "ops": change}));
                    }
                    cur = new;
                }
                if msg.is_empty() && ping.elapsed() >= Duration::from_secs(15) {
                    (msg, ping) = (": ping\n\n".into(), Instant::now());
                }
            }
        };
        STREAMS.fetch_sub(1, Relaxed);
        res
    }
}

fn list(v: &Value) -> String {
    match v {
        Value::Array(a) => a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(","),
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}
