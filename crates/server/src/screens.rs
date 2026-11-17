use super::{
    devices, form, header, mtime, shown, working, Auth, BTreeMap, Config, Ctx, Event, Refusal, Result, Server,
};
use super::{LaneRow, LanesTab, NeedAsk, NeedIssue, QueueCard, QueueRow, JSON};
use crate::access::{civil_month_day, civil_year};
use crate::api::{Block, Chat, Herd, HerdFleet, HerdLane, Lane};
use crate::events::Json;
use anyhow::Context;
use serde_json::{json, Map, Value};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::Duration;

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

/// `cooee-plugin.toml`'s `[[places]]` as JSON: the tabs, menu rows and cards the phone page draws.
pub fn places() -> Value {
    let m: toml::Value = toml::from_str(MANIFEST).unwrap_or(toml::Value::Integer(0));
    serde_json::to_value(m.get("places")).unwrap_or_default()
}

/// The standalone page's places: the manifest's, then the Threads tab (never the Cooee plugin's).
pub fn page_places() -> Value {
    let threads = json!({"type": "header-strip", "id": "threads", "label": "Threads", "screen": "/screens/threads"});
    let herd = json!({"type": "menu-row", "label": "Herd", "subtitle": "the ranch: lanes by state, weather from host load", "screen": "/screens/herd"});
    let mut all = places().as_array().cloned().unwrap_or_default();
    all.extend([threads, herd]);
    Value::Array(all)
}

const THREAD_ACTIONS: [&str; 11] = [
    "thread-new",
    "thread-say",
    "thread-retry",
    "thread-archive",
    "thread-restore",
    "thread-queue",
    "thread-steer",
    "thread-unqueue",
    "thread-stop",
    "thread-model",
    "thread-signin",
];

pub(super) fn iso(t: u64) -> String {
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

struct Round<'a> {
    key: &'a str,
    name: String,
    check: bool,
    agent: Option<&'a str>,
    verdict: Option<&'a Event>,
}

fn history<'a>(events: &'a [Event], f: &str, n: u64) -> Vec<Round<'a>> {
    let mut out: Vec<Round> = vec![];
    for e in crate::events::of_issue(events, f, n) {
        let (check, key) = match e.kind.as_str() {
            "spawn" if crate::events::checks(e) => (true, e.str("stage").or(e.str("role")).unwrap_or("rev")),
            "spawn" => (false, e.str("role").unwrap_or_default()),
            "verdict" => (true, e.str("stage").unwrap_or("rev")),
            _ => continue,
        };
        if !out.last().is_some_and(|r| r.key == key && r.verdict.is_none()) {
            let k = out.iter().filter(|r| r.key == key).count() + 1;
            let name = match key {
                "rev" => format!("review {k}"),
                "fix" => format!("fix {k}"),
                s if check => format!("{s} {k}"),
                s => s.to_string(),
            };
            out.push(Round {
                key,
                name,
                check,
                agent: None,
                verdict: None,
            });
        }
        if let Some(r) = out.last_mut() {
            if e.kind == "verdict" {
                r.verdict = Some(e);
            } else {
                r.agent = e.str("agent");
            }
        }
    }
    out
}

fn verdict_tone(word: &str) -> &'static str {
    match word {
        "PASS" => "ok",
        "FAIL" => "bad",
        _ => "warn",
    }
}

fn verdict_badge(v: &Event) -> Value {
    let word = v.str("verdict").unwrap_or("?");
    json!({"text": word, "tone": verdict_tone(word)})
}

fn rounds(events: &[Event], f: &str, n: u64) -> Vec<String> {
    let h = history(events, f, n);
    let shown = h.iter().enumerate().map(|(i, r)| {
        let mark = match r.verdict.map(|v| v.str("verdict")) {
            Some(Some("PASS")) => " ✓",
            Some(_) => " ✗",
            None if !r.check && i + 1 < h.len() => " ✓",
            None => "",
        };
        format!("{}{mark}", r.name)
    });
    shown.collect()
}

fn strip(stage: &[super::Stage]) -> String {
    let chip = |s: &super::Stage| {
        if s.at == 0 {
            format!("[{} {}]", s.g, s.name)
        } else {
            s.g.clone()
        }
    };
    stage.iter().map(chip).collect::<Vec<_>>().join(" ")
}

fn cut(s: &str, n: usize) -> String {
    let mut c: String = s.chars().take(n).collect();
    if c.len() < s.len() {
        c.push('…');
    }
    c
}

const LINE: usize = 48;

fn row(ctx: &Ctx, ev: &[Event], paused: bool, f: &str, r: &QueueRow, k: Option<usize>, lane: Option<&Lane>) -> Value {
    let mut fig = figure(&r.act, &r.state);
    fig.put("grow", json!(r.grow));
    let title = format!(
        "#{} {}",
        r.issue,
        crate::gh::cached_title(ctx, f, r.issue).unwrap_or_default()
    );
    let state = r.state.replace('(', " ").replace(')', "").replace("⏸ waiting: ", "");
    let badge = match k {
        Some(k) => json!({"text": format!("#{}", k + 1), "tone": "mute"}),
        None if !r.stage.is_empty()
            && !["fail", "needs"].contains(&fig.get("state").and_then(Value::as_str).unwrap_or_default()) =>
        {
            json!({"text": strip(&r.stage), "tone": badge_tone(&fig)})
        }
        None => json!({"text": state, "tone": badge_tone(&fig)}),
    };
    let mut v = json!({"type": "row", "id": format!("q-{f}-{}", r.issue), "figure": fig, "title": cut(title.trim_end(), LINE), "badge": badge});
    let mut sub = rounds(ev, f, r.issue).join(" · ");
    if let Some(pr) = r.pr.pr {
        sub = format!("{sub} · PR #{pr}");
    }
    if r.pr.ci.as_deref() == Some("green") {
        sub = format!("{sub} · CI ✓");
    }
    let live = r.live && !r.dead && !r.lane.is_empty();
    let limited = lane.is_some_and(|l| l.state == "rate-limited");
    let said = lane.filter(|_| live).map(|l| {
        let note = l
            .note
            .as_deref()
            .map(|n| if limited { format!("resets {n}") } else { n.into() });
        [Some(l.state.clone()), note]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ")
    });
    if live {
        let at = [said.as_deref().unwrap_or_default(), r.lane.as_str(), r.place.as_str()];
        sub = format!(
            "{sub} ▸ {}",
            at.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
        );
    }
    if let Some(a) = &r.open {
        let rec = |i: usize| a.ask.recommend == u64::try_from(i + 1).ok();
        let opts = a.ask.options.iter().enumerate();
        let opts: Vec<String> = opts
            .map(|(i, o)| {
                if rec(i) {
                    format!("{o} (recommended)")
                } else {
                    o.clone()
                }
            })
            .collect();
        sub = format!("asks: {} · {}", a.ask.question, opts.join(" / "));
    }
    let sub = sub.trim_start_matches(" ▸ ").trim_start_matches(" · ");
    let sub = match k.and(crate::pending::waits(ev, f, r.issue, paused)) {
        Some(w) if !sub.is_empty() => format!("{w} · {sub}"),
        Some(w) => w,
        None => sub.into(),
    };
    let vals = json!({"fleet": f, "issue": r.issue, "agent": r.lane});
    if let Some(b) = r.blocked_by {
        v.put("subtitle", json!({"text": format!("held until #{b}"), "tone": "bad"}));
    } else if !sub.is_empty() {
        v.put("subtitle", json!(cut(&sub, LINE)));
    }
    if k.is_none() {
        v.put("value", json!({ "since": iso(r.t) }));
    }
    v.put(
        "tap",
        json!({ "sheet": format!("/screens/issue?repo={f}&issue={}", r.issue) }),
    );
    if live {
        let confirm = json!({"title": format!("Stop #{}'s lane {}?", r.issue, r.lane), "ok": "Stop lane"});
        let stop = json!({"label": "Stop", "tone": "bad", "action": "stop", "values": vals, "confirm": confirm});
        if limited {
            let wait = json!({"label": "Wait", "action": "limit", "values": {"fleet": f, "issue": r.issue}});
            v.put("swipe", json!([wait, stop]));
        } else {
            v.put("swipe", json!([stop]));
        }
    }
    if k.is_some() {
        let confirm = json!({"title": format!("Drop #{} from the queue?", r.issue), "ok": "Drop"});
        let mut swipe = moves(
            f,
            r,
            |l, values| json!({"label": l, "action": "move", "values": values}),
        );
        swipe.extend([
            json!({"label": "Top", "action": "top", "values": vals}),
            json!({"label": "Drop", "tone": "bad", "action": "drop", "values": vals, "confirm": confirm}),
        ]);
        v.put("swipe", json!(swipe));
    }
    v
}

fn moves(f: &str, r: &QueueRow, each: impl Fn(&str, Value) -> Value) -> Vec<Value> {
    let to = |key: &str, at: Option<u64>| at.map(|at| json!({"fleet": f, "issue": r.issue, key: at}));
    let (up, down) = (to("before", r.up), to("after", r.down));
    [up.map(|x| each("▲", x)), down.map(|x| each("▼", x))]
        .into_iter()
        .flatten()
        .collect()
}

fn hot(r: &QueueRow) -> Vec<&str> {
    r.groups.iter().filter_map(|g| g.strip_prefix("hot ")).collect()
}

fn lock_row(f: &str, r: &QueueRow) -> Option<Value> {
    let (n, i) = (hot(r).len(), r.issue);
    let serial = r.groups.iter().filter(|g| !g.starts_with("hot "));
    let mut tags: Vec<String> = serial.map(|g| format!("🔒 {g}")).collect();
    tags.extend((n > 0).then(|| format!("🔒 hot ×{n}")));
    let mut v =
        json!({"type": "row", "id": format!("hot-{f}-{i}"), "indent": 1, "title": tags.join(" · "), "style": "link"});
    if n > 0 {
        v.put("tap", json!({"sheet": format!("/screens/hot?repo={f}&issue={i}")}));
    }
    (!tags.is_empty()).then_some(v)
}

const SHOWN: usize = 5;

fn picked<'a>(q: &'a Q, key: &str) -> Vec<&'a str> {
    q.get(key)
        .map_or(vec![], |v| v.split(',').filter(|s| !s.is_empty()).collect())
}

fn view(q: &Q, key: &str, item: &str) -> String {
    let mut out: Vec<String> = vec![];
    for k in ["repo", "more", "fold"] {
        let mut v = picked(q, k);
        if k == key {
            match v.iter().position(|x| *x == item) {
                Some(i) => drop(v.remove(i)),
                None => v.push(item),
            }
        }
        if !v.is_empty() {
            out.push(format!("{k}={}", v.join(",")));
        }
    }
    format!("/screens/queue?{}", out.join("&")).trim_end_matches('?').into()
}

fn link(id: &str, title: &str, tap: &Value) -> Value {
    json!({"type": "row", "id": id, "title": title, "style": "link", "tap": tap})
}

fn tree<'a>(rs: &[&'a QueueRow]) -> Vec<(&'a QueueRow, usize)> {
    fn walk<'a>(rs: &[&'a QueueRow], at: Option<u64>, d: usize, out: &mut Vec<(&'a QueueRow, usize)>) {
        for r in rs {
            let up = r.blocked_by.filter(|b| rs.iter().any(|x| x.issue == *b));
            if up == at && !out.iter().any(|o| o.0.issue == r.issue) {
                out.push((r, d));
                walk(rs, Some(r.issue), d + 1, out);
            }
        }
    }
    let mut out = vec![];
    walk(rs, None, 0, &mut out);
    for r in rs {
        if !out.iter().any(|o| o.0.issue == r.issue) {
            out.push((r, 0));
            walk(rs, Some(r.issue), 1, &mut out);
        }
    }
    out
}

fn fleet_card(ctx: &Ctx, ev: &[Event], c: &QueueCard, q: &Q, lanes: &[Lane]) -> Value {
    let f = &c.name;
    let (queued, busy): (Vec<&QueueRow>, Vec<&QueueRow>) = c.rows.iter().partition(|r| r.state == "queued");
    let paused = c.paused.is_some();
    let rows = |id: &str, ts: Vec<(&QueueRow, usize)>| -> (usize, Vec<Value>) {
        let mut groups: Vec<Vec<Value>> = vec![];
        let mut i = 0;
        while let Some(&(r, d)) = ts.get(i) {
            let k = queued.iter().position(|x| x.issue == r.issue);
            let mut v = row(ctx, ev, paused, f, r, k, lanes.iter().find(|l| l.name == r.lane));
            if d > 0 {
                v.put("indent", json!(d.min(2)));
            }
            let mut g = vec![v];
            g.extend(lock_row(f, r));
            let kids = ts.iter().skip(i + 1).take_while(|x| x.1 > d).count();
            let key = format!("{f}-{}", r.issue);
            let folded = d == 0 && picked(q, "fold").contains(&key.as_str());
            if d == 0 && kids > 0 {
                let title = if folded {
                    format!("▸ {kids} held behind #{}", r.issue)
                } else {
                    format!("▾ fold #{}'s chain", r.issue)
                };
                let tap = json!({"open": view(q, "fold", &key)});
                g.push(link(&format!("chain-{key}"), &title, &tap));
            }
            groups.push(g);
            i += 1 + if folded { kids } else { 0 };
        }
        let n = groups.len();
        let all = picked(q, "more").contains(&f.as_str());
        let shown = groups.iter().take(if all { n } else { SHOWN });
        let mut out: Vec<Value> = shown.flatten().cloned().collect();
        if n > SHOWN {
            let title = if all {
                "show fewer".into()
            } else {
                format!("+{} more", n - SHOWN)
            };
            let tap = json!({"open": view(q, "more", f)});
            out.push(link(&format!("more-{id}-{f}"), &title, &tap));
        }
        (ts.len(), out)
    };
    let section = |id: &str, word: &str, (n, rs): (usize, Vec<Value>)| {
        json!({"type": "section", "id": format!("{id}-{f}"), "header": format!("{word} ({n})"),
            "blocks": [{"type": "list", "id": format!("{id}-list-{f}"), "rows": rs}]})
    };
    let badge = [
        json!({"text": "driving", "tone": "ok"}),
        json!({"text": "paused", "tone": "mute"}),
    ];
    let mut end = vec![
        json!({"type": "row", "id": format!("done-today-{f}"), "title": "done today", "value": c.done.len().to_string(),
            "accessory": "chevron", "tap": {"open": format!("/screens/queue/done?repo={f}")}}),
    ];
    if let Some(b) = &c.backlog {
        end.push(
            json!({"type": "row", "id": format!("backlog-{f}"), "title": format!("backlog ({})", b.issues.len()),
            "accessory": "chevron", "tap": {"sheet": format!("/screens/backlog?repo={f}")}}),
        );
    }
    let busy = rows("inprogress", busy.into_iter().map(|r| (r, 0)).collect());
    let tone = if c.need > 0 { "warn" } else { "accent" };
    let mut v = json!({"type": "card", "id": format!("fleet-{f}"), "tone": tone, "title": f,
        "subtitle": c.paused.clone().unwrap_or_else(|| c.drive.line.clone()), "badge": badge.get(usize::from(paused)),
        "buttons": [
            {"type": "button", "id": format!("drive-{f}"), "label": "Drive now", "style": "primary", "action": "drive", "values": {"fleet": f}},
            {"type": "button", "id": format!("pause-{f}"), "label": if paused { "Resume" } else { "Pause" }, "action": "pause", "values": {"fleet": f, "on": !paused}}],
        "blocks": [section("inprogress", "in progress", busy), section("queued", "queued", rows("queued", tree(&queued))),
            {"type": "list", "id": format!("done-{f}"), "rows": end}]});
    if let Some(p) = &c.paused {
        let main = json!({"action": "pause", "label": "Resume", "values": {"fleet": f, "on": false}});
        v.put(
            "fold",
            json!({"line": format!("{f} · {p} · {} open", c.rows.len()), "main": main}),
        );
    }
    v
}

fn done_screen(c: &QueueCard) -> Value {
    let f = &c.name;
    let rows: Vec<Value> = (c.done.iter())
        .map(|d| {
            let sha: String = d.sha.as_deref().unwrap_or("?").chars().take(7).collect();
            let at = format!("{:02}:{:02} UTC", d.t % 86_400 / 3600, d.t % 3600 / 60);
            let title = format!("#{} {}", d.issue, d.title.as_deref().unwrap_or_default());
            json!({"type": "row", "id": format!("done-{}-{}", d.issue, d.t), "title": title.trim_end(),
                "subtitle": format!("merged {at} · {sha}"), "value": d.age, "accessory": "chevron",
                "tap": {"sheet": format!("/screens/issue?repo={f}&issue={}", d.issue)}})
        })
        .collect();
    json!({"type": "screen", "id": format!("done-{f}"), "title": format!("{f} · done today"),
        "blocks": [{"type": "list", "id": "done-list", "rows": rows}]})
}

const LEGEND: [(&str, &str); 17] = [
    ("queued", "waiting its turn"),
    ("design · architect", "a design or ADR waits for your approval"),
    ("impl", "being built"),
    ("review n", "being checked, round n"),
    ("fix n", "fixing review findings, round n"),
    ("verify n", "the real app is being checked, round n"),
    ("update", "merging the integration branch into its PR"),
    ("ready", "passed, waiting to merge"),
    ("merged", "merged into the integration branch"),
    ("waiting: decision", "a lane asked: tap the row to answer in its sheet"),
    ("needs-human", "out of rounds or stuck: your call"),
    ("blocked · held until #N", "waits for another issue to merge"),
    ("parked", "lane closed, its work saved on the PR"),
    ("dead", "lane gone with unsaved work"),
    ("▲ [◆ rev] ◫ ✓", "the issue's pipeline, its current stage in brackets"),
    ("🔒 group", "serial: one lane of the group at a time"),
    ("🔒 hot ×N", "holds N hot files, one lane at a time: tap to list them"),
];

fn legend() -> Value {
    let rows = LEGEND
        .iter()
        .enumerate()
        .map(|(i, (w, m))| json!({"type": "row", "id": format!("lg-{i}"), "title": w, "subtitle": m}));
    json!({"type": "sheet", "id": "legend", "title": "What the marks mean", "cancel": "Done",
        "blocks": [{"type": "list", "id": "legend-list", "rows": rows.collect::<Vec<_>>()}]})
}

const FILTERS: [(&str, &str); 5] = [
    ("new", "new"),
    ("30", ">30 days"),
    ("90", ">90 days"),
    ("ac", "has AC"),
    ("pr", "in a closed PR"),
];

fn backlog(now: u64, c: &QueueCard, q: &Q) -> Option<Value> {
    let (f, b) = (&c.name, c.backlog.as_ref()?);
    let on = picked(q, "on");
    let words = q.get("q").map_or(String::new(), |w| w.to_lowercase());
    let mut fields = vec![
        json!({"type": "text", "name": format!("{f}.q"), "label": "Search", "value": words, "placeholder": "search #, title, label"}),
    ];
    let mut is: Vec<(u64, &super::BacklogIssue)> = (b.issues.iter())
        .filter_map(|(n, i)| Some((n.parse().ok()?, i)))
        .collect();
    is.sort_by_key(|x| std::cmp::Reverse(x.0));
    for (n, i) in is {
        let age = now.saturating_sub(crate::usage::epoch(&i.created).unwrap_or(now)) / 86_400;
        let pass = |k: &&str| match *k {
            "new" => age < 7,
            "30" => age > 30,
            "90" => age > 90,
            "ac" => i.ac,
            _ => i.pr,
        };
        let text = format!("#{n} {} {}", i.title, i.labels.join(" ")).to_lowercase();
        if on.iter().all(pass) && words.split_whitespace().all(|w| text.contains(w)) {
            let label = [format!("#{n} {}", i.title), i.labels.join(", ")].join(" · ");
            fields.push(json!({"type": "toggle", "name": format!("{f}.pick.{n}"), "label": label.trim_end_matches(" · "), "value": false}));
        }
    }
    let opts: Vec<Value> = FILTERS.iter().map(|(v, l)| json!({"value": v, "label": l})).collect();
    let read = format!(
        "read {} ago · Drive now reads it again; ticked ones queue as needs-check",
        b.ago
    );
    Some(
        json!({"type": "sheet", "id": format!("backlog-{f}"), "title": format!("{f} backlog · {} open", b.issues.len()),
        "cancel": "Cancel", "done": {"label": "Queue ticked", "action": "queue"}, "blocks": [
        {"type": "chips", "id": "on", "name": format!("{f}.on"), "multi": true, "selected": on, "action": "filter", "options": opts},
        {"type": "section", "id": "issues", "header": "open, not queued", "footer": read,
            "blocks": [{"type": "form", "id": format!("bl-{f}"), "fields": fields}]}]}),
    )
}

fn section(id: &str, header: &str, blocks: &[Value]) -> Value {
    json!({"type": "section", "id": id, "header": header, "blocks": blocks})
}

fn rows_list(id: &str, rows: &[Value]) -> Value {
    json!({"type": "list", "id": id, "rows": rows})
}

fn check_rows(
    st: Option<&crate::events::IssueState>,
    hist: &[Round],
    fl: Option<&crate::config::Fleet>,
    r: &QueueRow,
) -> Vec<Value> {
    let toned = |id: &str, title: &str, text: &str, tone: &str| json!({"type": "row", "id": id, "title": title, "subtitle": {"text": text, "tone": tone}});
    let mut rows = vec![];
    if r.dead {
        rows.push(toned(
            "dead",
            "dead",
            "the log has this lane open, no such agent",
            "bad",
        ));
    }
    if r.check {
        rows.push(toned(
            "needs-check",
            "needs-check",
            "from the backlog, spec not checked",
            "mute",
        ));
    }
    if let Some(ci) = &r.pr.ci {
        let (g, tone) = match ci.as_str() {
            "green" => ("✓", "ok"),
            "failed" => ("✗", "bad"),
            _ => ("…", "warn"),
        };
        rows.push(json!({"type": "row", "id": "ci", "title": "CI", "badge": {"text": g, "tone": tone}}));
    }
    let mut seen = vec![];
    for (h, v) in hist.iter().rev().filter_map(|h| Some((h, h.verdict?))) {
        if seen.contains(&h.key) {
            continue;
        }
        seen.push(h.key);
        let lim = fl.map(|fl| crate::gate::limit(fl, v.str("loop").unwrap_or(h.key), st));
        let lim = lim.filter(|l| l.0 != u32::MAX).map(|l| l.1).unwrap_or_default();
        let round = v
            .data
            .get("round")
            .and_then(Value::as_u64)
            .map_or(String::new(), |k| format!("{k}{lim}"));
        let mut row = json!({"type": "row", "id": format!("v-{}", h.key), "title": h.key, "badge": verdict_badge(v), "value": round});
        if let Some(x) =
            r.pr.form
                .as_ref()
                .filter(|_| v.data.get("form_only") == Some(&Value::Bool(true)))
        {
            row.put(
                "subtitle",
                json!({"text": format!("not counted · form {x}"), "tone": "warn"}),
            );
        }
        rows.push(row);
    }
    rows
}

fn history_rows(now: u64, f: &str, n: u64, hist: &[Round], live: bool) -> Vec<Value> {
    let last = hist.len().saturating_sub(1);
    let row = |(i, h): (usize, &Round)| {
        let mut row = json!({"type": "row", "id": format!("h-{i}"), "title": h.name});
        if let Some(v) = h.verdict {
            let word = v.str("verdict").unwrap_or("?");
            let sha: String = v.str("sha").unwrap_or_default().chars().take(7).collect();
            let ago = crate::events::age(now.saturating_sub(v.ts));
            row.put(
                "subtitle",
                json!({"text": format!("{word} · {sha} · {ago}"), "tone": verdict_tone(word)}),
            );
            row.put(
                "tap",
                json!({ "sheet": format!("/screens/verdict?repo={f}&issue={n}&seq={}", v.seq) }),
            );
        } else if h.check && !(i == last && live) {
            row.put("subtitle", json!({"text": "no verdict", "tone": "warn"}));
        }
        row
    };
    hist.iter().enumerate().rev().map(row).collect()
}

fn issue_sheet(
    ctx: &Ctx,
    ev: &crate::events::Log,
    fl: Option<&crate::config::Fleet>,
    c: &QueueCard,
    r: &QueueRow,
) -> Value {
    let (f, n) = (c.name.as_str(), r.issue);
    let titled = |id: String, title: &str| json!({"type": "row", "id": id, "title": title});
    let sheet = |p: String| json!({ "sheet": p });
    let hist = history(ev, f, n);
    let mut status = vec![];
    if !r.stage.is_empty() {
        let label = |s: &super::Stage| format!("{} {}{}", s.g, s.name, if s.ok { " ✓" } else { "" });
        let opts: Vec<Value> = r
            .stage
            .iter()
            .map(|s| json!({"value": s.name, "label": label(s)}))
            .collect();
        let lit: Vec<&str> = r.stage.iter().filter(|s| s.at == 0).map(|s| s.name.as_str()).collect();
        status.push(json!({"type": "chips", "id": "stage", "name": "stage", "selected": lit, "options": opts}));
    }
    let mut now = json!({"type": "row", "id": "now", "title": r.state, "value": {"since": iso(r.t)}});
    let held = r.blocked_by.map(|b| format!("held until #{b}"));
    if let Some(w) = crate::pending::waits(ev, f, n, c.paused.is_some()).or(held) {
        now.put("subtitle", json!(w));
    }
    let mut rows = vec![now];
    if !r.lane.is_empty() {
        let mut l = titled("at".into(), &format!("lane {}", r.lane));
        let at = [r.place.as_str(), r.host.as_str()].into_iter().find(|s| !s.is_empty());
        l.put("subtitle", json!(at.unwrap_or(if r.live { "live" } else { "gone" })));
        rows.push(l);
    }
    rows.extend(check_rows(ev.states.get(&(f.to_string(), n)), &hist, fl, r));
    rows.extend(r.notes.iter().enumerate().map(|(i, x)| titled(format!("note-{i}"), x)));
    status.push(rows_list("status-list", &rows));
    let mut blocks = vec![section("status", "status", &status)];
    if let Some(a) = &r.open {
        blocks.push(section(
            "asks",
            &format!("asks: {}", a.ask.question),
            &[ask_choice(&a.ask)],
        ));
    }
    let vals = json!({"fleet": f, "issue": n});
    if let Some(d) = &r.decided {
        let mut change = r.ask.as_ref().map(ask_choice);
        if let Some(o) = change.as_mut().and_then(|c| c["options"].as_array_mut()) {
            for o in o.iter_mut() {
                o.put("values", vals.clone());
            }
        }
        let change: Vec<Value> = change.into_iter().collect();
        blocks.push(section("decided", &format!("decided by default: {d}"), &change));
    }
    let gh = format!("https://github.com/{}", c.config.repo);
    let url = |id: &str, t: String, u: String| link(id, &t, &json!({ "url": u }));
    let mut links = vec![url("issue", format!("issue #{n}"), format!("{gh}/issues/{n}"))];
    let spec = crate::gh::cached_title(ctx, f, n);
    match (r.pr.pr, &r.pr.run) {
        (Some(p), _) => {
            let mut pr = url("pr", format!("PR #{p}"), format!("{gh}/pull/{p}"));
            let cached = crate::gh::cached_pr(ctx, f, n);
            if let Some(t) = cached.at("title").as_str().filter(|t| Some(*t) != spec.as_deref()) {
                pr.put("subtitle", json!(t));
            }
            links.extend([pr, url("checks", "checks".into(), format!("{gh}/pull/{p}/checks"))]);
        }
        (None, Some(run)) => links.push(url("checks", "checks".into(), run.clone())),
        _ => {}
    }
    if r.live && !r.lane.is_empty() {
        let tap = sheet(format!("/screens/lanes/{}", r.lane));
        links.push(link("lane", &format!("lane {}", r.lane), &tap));
    }
    if let Some((a, t)) = r.open.as_ref().and_then(|a| Some((a, a.thread.as_ref()?))) {
        let title = format!("asked in {}", a.title.as_ref().filter(|s| !s.is_empty()).unwrap_or(t));
        links.push(link("asked", &title, &json!({ "thread": t })));
    }
    blocks.push(section("links", "links", &[rows_list("links-list", &links)]));
    if !r.lanes.is_empty() {
        let lane = |l: &super::QueueLane| {
            let v = hist.iter().rev().find(|h| h.check && h.agent == Some(l.name.as_str()));
            let state = v.and_then(|h| h.verdict).map_or(json!(l.state), verdict_badge);
            json!({"type": "row", "id": format!("l-{}", l.name), "title": l.name, "subtitle": state, "value": l.ago,
                "tap": sheet(format!("/screens/lanes/{}", l.name))})
        };
        let rows: Vec<Value> = r.lanes.iter().map(lane).collect();
        blocks.push(section("lanes", "lanes", &[rows_list("lanes-list", &rows)]));
    }
    if !hist.is_empty() {
        let rows = history_rows(ctx.now, f, n, &hist, r.live);
        blocks.push(section("history", "history", &[rows_list("history-list", &rows)]));
    }
    let button = |l: &str, action: &str, values: Value| json!({"type": "button", "id": l, "label": l, "action": action, "values": values});
    blocks.extend(moves(f, r, |l, values| button(l, "move", values)));
    if r.state == "parked" {
        blocks.push(button("Unpark", "unpark", vals.clone()));
    }
    if r.queued {
        let mut drop = button("Drop", "drop", vals.clone());
        drop.put("style", json!("destructive"));
        drop.put(
            "confirm",
            json!({"title": format!("Drop #{n} from the queue?"), "ok": "Drop"}),
        );
        blocks.extend([button("Top", "top", vals), drop]);
    }
    let title = format!(
        "#{n} {}",
        spec.or_else(|| r.title.clone().filter(|_| r.state == "merged"))
            .unwrap_or_default()
    );
    json!({"type": "sheet", "id": format!("issue-{f}-{n}"), "title": title.trim_end(), "cancel": "Done", "blocks": blocks})
}

fn verdict_sheet(ev: &[Event], q: &Q) -> Option<Value> {
    let (f, n, seq) = (
        q.get("repo")?,
        q.get("issue")?.parse::<u64>().ok()?,
        q.get("seq")?.parse::<u64>().ok()?,
    );
    let v = crate::events::of_issue(ev, f, n)
        .into_iter()
        .find(|e| e.kind == "verdict" && e.seq == seq)?;
    let (stage, word) = (v.str("stage").unwrap_or("rev"), v.str("verdict").unwrap_or("?"));
    let body = v.str("body").unwrap_or("no verdict.md text was logged");
    Some(
        json!({"type": "sheet", "id": format!("verdict-{f}-{n}-{seq}"), "title": format!("#{n} {stage} {word}"), "cancel": "Done",
        "blocks": [{"type": "text", "id": "verdict-body", "markdown": body}]}),
    )
}

fn stats(now: u64, ev: &[Event], states: &crate::events::States, only: Option<&str>, lanes: &[Lane]) -> Value {
    let day = |e: &Event| usize::try_from(now.saturating_sub(e.ts) / 86_400).unwrap_or(usize::MAX);
    let merged: Vec<&Event> = ev
        .iter()
        .filter(|e| e.kind == "merged" && day(e) < 7 && only.is_none_or(|f| e.fleet == f))
        .collect();
    let mut per = [(0u32, 0u32); 7];
    for e in &merged {
        let r = states
            .get(&(e.fleet.clone(), e.issue.unwrap_or(0)))
            .map_or(0, |s| s.rounds);
        let Some(p) = per.get_mut(6 - day(e)) else { continue };
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
    let c = super::v1::counts(lanes, only);
    json!({"type": "stat", "id": "stats", "items": [
        {"label": "merged · 7d", "value": n.to_string(), "spark": per.map(|p| p.0)},
        {"label": "busy", "value": c.busy.to_string()},
        {"label": "approval", "value": c.waiting_approval.to_string()},
        {"label": "owner", "value": c.waiting_owner.to_string()},
        {"label": "rate-limited", "value": c.rate_limited.to_string()},
        {"label": "rounds / issue", "value": format!("{:.1}", avg(n, r)), "spark": per.map(|p| avg(p.0, p.1))}]})
}

fn counts(shown: &[&QueueCard]) -> Value {
    let sum = |f: &dyn Fn(&QueueCard) -> usize| shown.iter().map(|c| f(c)).sum::<usize>();
    let need = sum(&|c| c.need);
    let queued = sum(&|c| c.rows.iter().filter(|r| r.state == "queued").count());
    let busy = sum(&|c| c.rows.len()) - need - queued;
    let tone = if need > 0 { "warn" } else { "" };
    json!({"type": "stat", "id": "counts", "items": [
        {"label": "needs you", "value": need.to_string(), "tone": tone},
        {"label": "in progress", "value": busy.to_string()},
        {"label": "queued", "value": queued.to_string()},
        {"label": "done today", "value": sum(&|c| c.done_n).to_string()}]})
}

fn choice(id: &str, action: &str, options: &[Value]) -> Value {
    json!({"type": "choice", "id": id, "action": action, "options": options})
}

fn fold(n: Option<u64>, kind: &str, q: &str, age: &str, main: &Value) -> Value {
    let cut = cut(q, LINE);
    let n = n.map_or(String::new(), |n| format!("#{n} · "));
    json!({"line": format!("{n}{kind} · {cut} · {age}"), "main": main})
}

fn recommended(q: &crate::api::Ask) -> Option<&String> {
    q.recommend
        .and_then(|n| q.options.get(usize::try_from(n).ok()?.checked_sub(1)?))
}

fn ask_choice(q: &crate::api::Ask) -> Value {
    let rec = recommended(q);
    let (first, rest): (Vec<&String>, Vec<&String>) = q.options.iter().partition(|o| Some(*o) == rec);
    let opts: Vec<Value> = (first.into_iter().chain(rest))
        .map(|o| json!({"value": o, "label": o}))
        .collect();
    let mut c = choice(&q.id, "answer", &opts);
    if let Some(r) = rec {
        c.put("recommended", json!(r));
    }
    let other = match q.options.iter().find(|o| *o == "revise") {
        Some(r) => json!({"placeholder": "a revise note…", "for": r}),
        None => json!({"placeholder": "other…"}),
    };
    c.put("other", other);
    c
}

fn ask_card(now: u64, mins: u64, a: &NeedAsk) -> Value {
    let (q, rec) = (&a.ask, recommended(&a.ask));
    let routine = a.kind.starts_with("routine");
    let mut c = ask_choice(&a.ask);
    if let Some(r) = rec.filter(|_| routine) {
        let until = iso(q.ts + 60 * mins);
        let d = json!({"value": r, "until": until, "label": "takes the recommendation in {rel}"});
        c.put("default", d);
    }
    let issue = q.issue.map_or(String::new(), |n| format!(" #{n}"));
    let design = if a.pngs.is_empty() { "" } else { " · design" };
    let mut blocks = vec![json!({"type": "text", "markdown": format!("**{}**", q.question)})];
    if !a.pngs.is_empty() {
        let n = q.issue.unwrap_or(0);
        let items = (0..)
            .zip(&a.pngs)
            .map(|(i, p)| json!({"src": format!("/design/{}/{n}/{i}/{}", q.fleet, q.id), "alt": p}));
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

fn ask_outcome(log: &[Event], a: &Event) -> Option<String> {
    let why = crate::events::closed_row(log, a)?;
    let pick = (log.iter().rev())
        .find(|d| d.kind == "decision" && crate::events::names(d, a))
        .and_then(|d| d.str("choice"));
    Some(match pick.filter(|_| why == "answered") {
        Some(c) => format!("answered · {c}"),
        None => why,
    })
}

fn ask_minutes(ctx: &Ctx) -> u64 {
    Config::kept(ctx).map_or(crate::config::Limits::default().ask_default_minutes, |c| {
        c.limits.ask_default_minutes
    })
}

fn ask_sheet(ctx: &Ctx, q: &Q) -> Option<Value> {
    let seq = q.get("seq")?.parse::<u64>().ok()?;
    let log = working(&ctx.state).ok()?;
    let a = log.get(log.binary_search_by_key(&seq, |e| e.seq).ok()?)?;
    let a = (a.kind == "needs-decision").then_some(a)?;
    let ask = super::v1::ask(a);
    let blocks = if let Some(o) = ask_outcome(&log, a) {
        json!([{"type": "text", "markdown": format!("**{}**\n\n{o}", ask.question)}])
    } else {
        let routine = a.str("kind") == Some("routine") && ask.recommend.is_some();
        let need = NeedAsk {
            ask,
            who: String::new(),
            kind: (if routine { "routine" } else { "owner" }).into(),
            from: None,
            pngs: super::chat::files_of(a)
                .map(|f| f["name"].as_str().unwrap_or_default().into())
                .collect(),
            thread: None,
            title: None,
        };
        ask_card(ctx.now, ask_minutes(ctx), &need).at("blocks").clone()
    };
    let id = a.str("id").unwrap_or_default();
    Some(
        json!({"type": "sheet", "id": format!("asks-{seq}"), "title": format!("{} · {id}", a.fleet), "cancel": "Done", "blocks": blocks}),
    )
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
    let at = i.shots_at.as_deref().map_or(String::new(), |a| format!("/{a}"));
    let shots = i.shots.iter().enumerate();
    let items: Vec<Value> = shots
        .map(|(k, a)| json!({"src": format!("/shot/{f}/{n}/{k}{at}"), "alt": a}))
        .collect();
    if !items.is_empty() {
        blocks.push(json!({"type": "images", "id": format!("shots-{f}-{n}"), "items": items}));
    }
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

fn workflow(ctx: &Ctx) -> Value {
    let limits = Config::kept(ctx).map(|c| c.limits).unwrap_or_default();
    let ev = working(&ctx.state).unwrap_or_default();
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
    let kinds = ["merged", "verdict", "needs-human", "spawn", "needs-decision"];
    let window = crate::events::since(&ctx.state, &kinds, before.saturating_sub(86_400)).unwrap_or_default();
    let repos: BTreeMap<String, String> = Config::kept(ctx)
        .map(|c| c.fleet.into_iter().map(|(f, fc)| (f, fc.repo)).collect())
        .unwrap_or_default();
    let list = super::v1::feed(ctx, &window, usize::MAX).list;
    let (mut rows, mut asks) = (vec![], None);
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
            r.put("subtitle", json!(t));
        }
        let s = |k: &str| e.data.get(k).and_then(Value::as_str).filter(|s| !s.is_empty());
        let repo = repos.get(&e.fleet);
        let tap = match (s("agent").filter(|_| k == "lane").or(s("thread")), e.pr.zip(repo)) {
            (Some(t), _) => json!({ "thread": t }),
            (_, Some((pr, repo))) => json!({ "url": format!("https://github.com/{repo}/pull/{pr}") }),
            _ => Value::Null,
        };
        r.put("tap", tap);
        let log =
            (e.kind == "needs-decision").then(|| asks.get_or_insert_with(|| working(&ctx.state).unwrap_or_default()));
        if let Some((log, i)) = log.and_then(|l| Some((&**l, l.binary_search_by_key(&e.seq, |x| x.seq).ok()?))) {
            let (k, x) = match log.get(i).and_then(|x| ask_outcome(log, x)) {
                Some(o) => ("subtitle", json!(o)),
                None => ("tap", json!({ "sheet": format!("/screens/ask?seq={}", e.seq) })),
            };
            r.put(k, x);
        }
        if r.at("tap").is_null() {
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
        s.put("stream", json!(stream.trim_end_matches(['?', '&'])));
    }
    s
}

const COATS: [&str; 5] = ["bay", "blk", "ch", "gry", "wht"];

fn lane_figure(name: &str, state: &str) -> Value {
    let i = usize::from_str_radix(&devices::sha256(name.as_bytes())[..2], 16).unwrap_or(0) % COATS.len();
    let state = match state {
        "working" => "working",
        "blocked" => "needs",
        "done" => "done",
        "dead" => "fail",
        _ => "idle",
    };
    json!({"coat": COATS.get(i), "state": state})
}

fn lane_row(fleet: &str, l: &LaneRow) -> Value {
    let title = l.issue.map_or(l.name.clone(), |n| format!("{fleet} #{n}"));
    let now = l.now.as_ref().map(|n| n.title.clone());
    let now = now.or_else(|| l.why.clone()).unwrap_or_else(|| l.activity.clone());
    let sub: Vec<&str> = [l.role.as_str(), l.host.as_str(), now.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    json!({"type": "row", "id": format!("l-{}", l.name), "figure": lane_figure(&l.name, &l.state), "title": title,
        "subtitle": sub.join(" · "), "value": l.ago, "tap": {"sheet": format!("/screens/lanes/{}", l.name)}})
}

const BANDS: [(&str, &[&str]); 4] = [
    ("working", &["working"]),
    ("idle", &["idle", "unknown", "parked"]),
    ("done", &["done"]),
    ("blocked", &["blocked", "dead"]),
];

fn band(id: &str, name: &str, rows: &[Value]) -> Option<Value> {
    (!rows.is_empty()).then(|| {
        json!({"type": "section", "id": format!("h-{id}-{name}"), "header": format!("{name} ({})", rows.len()),
            "blocks": [{"type": "list", "id": format!("h-{id}-{name}-list"), "rows": rows}]})
    })
}

fn herd_repo(name: &str, color: &str, lanes: &[HerdLane], f: Option<&HerdFleet>) -> Option<Value> {
    let needs = f.map(|f| f.needs.as_slice()).unwrap_or_default();
    let lanes: Vec<&HerdLane> = lanes.iter().filter(|l| !l.gone).collect();
    let mut sum = vec![];
    let gate: Vec<Value> = (f.map(|f| f.queue.as_slice()).unwrap_or_default().iter())
        .filter(|q| !lanes.iter().any(|l| l.issue == Some(q.issue)))
        .map(|q| {
            let need = needs.contains(&q.issue);
            let blocked = q.blocked_by.map(|b| format!("blocked by #{b}"));
            let sub = need.then(|| "needs you".to_string()).or(blocked).or_else(|| q.title.clone());
            json!({"type": "row", "id": format!("h-{name}-q{}", q.issue), "figure": {"state": if need { "needs" } else { "idle" }},
                "title": format!("#{}", q.issue), "subtitle": sub.unwrap_or_default()})
        })
        .collect();
    sum.push((gate.len(), "queued"));
    let mut blocks: Vec<Value> = band(name, "queued", &gate).into_iter().collect();
    for (b, states) in BANDS {
        let rows: Vec<Value> = (lanes.iter())
            .filter(|l| {
                let need = l.issue.is_some_and(|i| needs.contains(&i));
                if need { b == "blocked" } else { states.contains(&l.state.as_str()) }
            })
            .map(|l| {
                let need = l.issue.is_some_and(|i| needs.contains(&i));
                let shown = if need { "needs you" } else { l.shown.as_str() };
                let sub: Vec<&str> = [shown, l.host.as_deref().unwrap_or_default()].into_iter().filter(|s| !s.is_empty()).collect();
                json!({"type": "row", "id": format!("h-{}", l.name), "figure": lane_figure(&l.name, if need { "blocked" } else { &l.state }),
                    "title": l.name, "subtitle": sub.join(" · "), "value": l.age, "tap": {"sheet": format!("/screens/lanes/{}", l.name)}})
            })
            .collect();
        sum.push((rows.len(), b));
        blocks.extend(band(name, b, &rows));
    }
    let sum: Vec<String> = sum
        .iter()
        .filter(|s| s.0 > 0)
        .map(|(n, w)| format!("{n} {w}"))
        .collect();
    (f.is_some() || !lanes.is_empty())
        .then(|| json!({"id": name, "title": name, "color": color, "subtitle": sum.join(" · "), "blocks": blocks}))
}

fn herd_screen(h: &Herd) -> Value {
    let mut repos: Vec<Value> = (h.fleets.iter())
        .filter_map(|f| herd_repo(&f.name, &f.color, &f.lanes, Some(f)))
        .collect();
    repos.extend(herd_repo("others", "", &h.others, None));
    let host = &h.host;
    let wind = (host.load.unwrap_or_default() / host.limit.max(0.01)).clamp(0.0, 1.0);
    let sky = match host.pressure.as_deref() {
        _ if host.gate.is_some() => "storm",
        Some("warn" | "critical") => "fog",
        _ if wind >= 0.5 => "windy",
        _ => "clear",
    };
    let load = host.load.map_or("?".into(), |l| format!("{l:.2}"));
    let swap = host.swapouts.map_or("?".into(), |s| format!("{s:.0}"));
    let line = format!(
        "load {load} · mem {} · swap {swap}",
        host.pressure.as_deref().unwrap_or("?")
    );
    json!({"type": "screen", "id": "herd", "title": "Herd", "blocks": [{"type": "scene", "id": "herd-scene",
        "weather": {"sky": sky, "wind": (wind * 100.0).round() / 100.0, "line": line}, "repos": repos}]})
}

fn section_word(state: &str) -> &str {
    match state {
        "blocked" => "waiting",
        "idle" => "asleep",
        s => s,
    }
}

fn lanes_screen(t: &LanesTab) -> Value {
    let mut blocks: Vec<Value> = t
        .sections
        .iter()
        .map(|sec| {
            let rows: Vec<Value> = sec
                .repos
                .iter()
                .flat_map(|r| r.lanes.iter().map(move |l| lane_row(&r.name, l)))
                .collect();
            json!({"type": "section", "id": format!("lanes-{}", sec.name), "header": format!("{} ({})", section_word(&sec.name), rows.len()),
                "blocks": [{"type": "list", "id": format!("lanes-list-{}", sec.name), "rows": rows}]})
        })
        .collect();
    if let Some(e) = t.error.as_deref().or(t.failed.then_some("the lane list failed")) {
        blocks.insert(0, json!({"type": "text", "id": "error", "markdown": e}));
    }
    let others: Vec<Value> = t
        .shed
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let name = o.name.clone().unwrap_or_else(|| o.host.clone());
            let what = [Some(&o.host), o.state.as_ref(), o.last.as_ref(), o.error.as_ref()];
            let sub: Vec<&str> = what.into_iter().flatten().map(String::as_str).collect();
            json!({"type": "row", "id": format!("o-{i}"), "figure": {"state": "idle"}, "title": name,
                "subtitle": sub.join(" · "), "value": o.age.clone().unwrap_or_default()})
        })
        .collect();
    if !others.is_empty() {
        blocks.push(
            json!({"type": "section", "id": "others", "header": format!("other in shed ({})", others.len()),
            "blocks": [{"type": "list", "id": "others-list", "rows": others}]}),
        );
    }
    json!({"type": "screen", "id": "lanes", "title": "Lanes", "stream": "/screens/lanes/stream", "blocks": blocks})
}

fn said(i: usize, b: &Block, lines: &[String]) -> Value {
    let id = format!("t-{i}");
    let text = |m: String| json!({"type": "text", "id": id, "markdown": m});
    match b {
        Block::Prompt { text: t } => text(format!("**›** {t}")),
        Block::Reply { markdown } => text(markdown.clone()),
        Block::Tool { title_line, status, .. } => {
            let state = match status.as_str() {
                "completed" => "done",
                "failed" => "fail",
                "pending" => "idle",
                _ => "working",
            };
            json!({"type": "row", "id": id, "figure": {"state": state}, "title": title_line, "subtitle": status})
        }
        Block::Plan { entries } => text(
            entries
                .iter()
                .map(|e| {
                    let mark = if e["status"] == "completed" { "x" } else { " " };
                    format!("- [{mark}] {}", e["content"].as_str().unwrap_or_default())
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        Block::Approval { tool, answered, .. } => text(format!(
            "**asks:** {tool} · {}",
            answered.as_deref().unwrap_or("waiting")
        )),
        Block::Screen => text(format!("```\n{}\n```", lines.join("\n"))),
    }
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
        if let Some(p) = ia.iter().position(|x| x == i).and_then(|p| ka.get(p)) {
            walk(p, c, out);
            continue;
        }
        let mut op = json!({"op": "insert", "into": into, "block": c});
        if let Some(next) = ib.iter().skip(k + 1).find(|x| ia.contains(x)) {
            op.put("before", json!(next));
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

fn count(v: &Value) -> Option<u64> {
    v["badge"].as_u64()
}

fn sheets(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            out.extend(
                m.get("sheet")
                    .or_else(|| m.get("open"))
                    .and_then(Value::as_str)
                    .filter(|p| p.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b)))
                    .map(String::from),
            );
            m.values().for_each(|x| sheets(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| sheets(x, out)),
        _ => {}
    }
}

impl Server<'_> {
    pub(super) fn screen_files(&self, ctx: &Ctx, paired: bool) -> Vec<(String, Vec<u8>)> {
        let mut paths: Vec<String> = page_places()
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p["screen"].as_str().map(String::from))
            .chain(["/screens/cards".to_string()])
            .collect();
        let mut out = vec![];
        let mut i = 0;
        while let Some(p) = paths.get(i).cloned() {
            i += 1;
            if let Some(v) = self.screen(ctx, &p, &Q::new(), paired) {
                let mut more = vec![];
                sheets(&v, &mut more);
                more.retain(|m| !paths.contains(m));
                paths.extend(more);
                out.push((format!("{}/index.html", &p[1..]), v.to_string().into_bytes()));
            }
        }
        out
    }

    fn screen(&self, ctx: &Ctx, path: &str, q: &Q, paired: bool) -> Option<Value> {
        let mut v = match path {
            "/screens/queue" => self.queue_screen(ctx, q),
            "/screens/lanes" => lanes_screen(&self.lanes_tab(ctx, paired)),
            p if paired && p.starts_with("/screens/lanes/") => self.lane_sheet(ctx, &p[15..])?,
            "/screens/events" => events_screen(ctx, q),
            "/screens/fleets" => fleets(&Config::kept(ctx).ok()?),
            "/screens/cards" => self.chat_cards(ctx, paired),
            "/screens/workflow" => workflow(ctx),
            "/screens/herd" => herd_screen(&self.herd(ctx, None).ok()?),
            "/screens/hot" => self.on_row(ctx, q, |c, r| {
                let f = &c.name;
                let rows: Vec<Value> = (hot(r).iter().enumerate())
                    .map(|(i, p)| json!({"type": "row", "id": format!("hot-{i}"), "title": p}))
                    .collect();
                json!({"type": "sheet", "id": format!("hot-{f}-{}", r.issue), "title": format!("#{} · 🔒 hot ×{}", r.issue, rows.len()),
                    "cancel": "Done", "blocks": [{"type": "list", "id": "hot-paths", "rows": rows}]})
            })?,
            "/screens/issue" => {
                let ev = working(&ctx.state).ok()?;
                let cfg = Config::kept(ctx).ok();
                let fl = |c: &QueueCard| cfg.as_ref().and_then(|x| x.fleet.get(&c.name));
                self.on_row(ctx, q, |c, r| issue_sheet(ctx, &ev, fl(c), c, r))?
            }
            "/screens/verdict" => verdict_sheet(&working(&ctx.state).ok()?, q)?,
            "/screens/legend" => legend(),
            "/screens/backlog" => {
                let tab = self.queue_tab(ctx);
                backlog(ctx.now, tab.fleets.iter().find(|c| Some(&c.name) == q.get("repo"))?, q)?
            }
            "/screens/queue/done" => done_screen(self.queue_tab(ctx).fleets.iter().find(|c| Some(&c.name) == q.get("repo"))?),
            "/screens/ask" => ask_sheet(ctx, q)?,
            p if p == "/screens/threads" || p.starts_with("/screens/threads/") => {
                return self.threads_screen(ctx, &p[16..], q.get("before"), paired)
            }
            _ => return None,
        };
        let snap = Some(self.snapshot().1.ts).filter(|t| *t > 0);
        let age = snap.map(|t| ctx.now.saturating_sub(t));
        let page = v.at("type") == "screen";
        if let Some(b) = v.get_mut("blocks").and_then(Value::as_array_mut).filter(|_| page) {
            let line = age.map_or("as of: not yet known".into(), |a| {
                format!("as of {} ago", crate::events::age(a))
            });
            let tone = if age.is_some_and(|a| a <= 600) { "mute" } else { "warn" };
            let row =
                json!({"type": "row", "id": "age", "title": "Snapshot", "subtitle": {"text": line, "tone": tone}});
            match b.iter_mut().find(|b| b["type"] == "stat") {
                Some(st) => st["items"].as_array_mut().into_iter().for_each(|i| {
                    i.push(json!({"label": "snapshot", "value": line.clone(), "tone": tone}));
                }),
                None => b.insert(
                    0,
                    json!({"type": "section", "id": "age", "header": "snapshot", "blocks": [{"type": "list", "id": "age-list", "rows": [row]}]}),
                ),
            }
        }
        Some(v)
    }

    fn lane_sheet(&self, ctx: &Ctx, agent: &str) -> Option<Value> {
        let (cfg, ev) = (Config::kept(ctx).ok()?, working(&ctx.state).ok()?);
        let all = self.lanes(ctx.now, &cfg, &ev, &ev.states)?;
        let l = all.into_iter().find(|l| l.name == agent)?;
        let spawn = crate::events::last_spawn(&ev, agent);
        let fc = l.fleet.as_ref().and_then(|f| cfg.fleet.get(f));
        let pr = l.issue.zip(l.fleet.as_ref()).and_then(|(n, f)| {
            let s = ev.states.get(&(f.clone(), n));
            super::pr_data(ctx, fc, f, n, s).pr.zip(fc)
        });
        let issue = l.issue.zip(l.fleet.as_ref()).map(|(n, f)| format!("{f} #{n}"));
        let fact = |k: &str, v: Option<String>| v.filter(|v| !v.is_empty()).map(|v| (k.to_string(), v));
        let mut facts: Vec<Value> = [
            fact("issue", issue),
            fact("role", Some(l.role.clone())),
            fact("state", Some(l.state.word())),
            fact("host", Some(l.at())),
            fact("branch", spawn.and_then(|e| e.str("branch")).map(String::from)),
            fact("port", l.port.clone()),
        ]
        .into_iter()
        .flatten()
        .map(|(k, v)| json!({"type": "row", "id": format!("f-{k}"), "title": k, "value": v}))
        .collect();
        if let Some((n, fc)) = pr {
            let url = format!("https://github.com/{}/pull/{n}", fc.repo);
            facts.push(
                json!({"type": "row", "id": "f-pr", "title": "PR", "value": format!("#{n}"), "tap": {"url": url}}),
            );
        }
        let q: Q = [("agent".to_string(), agent.to_string())].into();
        let chat = self
            .transcript(ctx, &q)
            .and_then(|t| serde_json::from_str::<Chat>(&t).ok());
        let said: Vec<Value> = chat.map_or_else(Vec::new, |c| {
            let lc = self.blocks(ctx, &q, c);
            lc.blocks
                .iter()
                .enumerate()
                .map(|(i, b)| said(i, b, &lc.chat.lines))
                .collect()
        });
        let values = json!({ "agent": agent });
        let other = l.role == "other";
        let acts: &[(&str, &str)] = if other {
            &[("close", "Close pane")]
        } else {
            &[("stop", "Stop"), ("kill", "Kill"), ("clean", "Clean")]
        };
        let mut blocks = vec![
            json!({"type": "section", "id": "facts", "header": "lane", "blocks": [{"type": "list", "id": "facts-list", "rows": facts}]}),
        ];
        blocks.extend(acts.iter().map(|(a, label)| {
            let confirm = json!({"title": format!("{label} {agent}?"), "ok": label});
            json!({"type": "button", "id": format!("act-{a}"), "label": label, "style": "destructive", "action": a, "values": values, "confirm": confirm})
        }));
        let mut v = json!({"type": "sheet", "id": format!("lane-{agent}"), "title": agent, "cancel": "Close"});
        if !other {
            let field = json!({"type": "text", "name": "text", "label": "Say", "value": "", "placeholder": "a line to the lane"});
            blocks.push(json!({"type": "form", "id": "say", "fields": [field]}));
            v.put("done", json!({"label": "Send", "action": "send", "values": values}));
        }
        blocks.push(json!({"type": "section", "id": "transcript", "header": "transcript", "blocks": said}));
        v.put("blocks", json!(blocks));
        Some(v)
    }

    fn on_row(&self, ctx: &Ctx, q: &Q, draw: impl Fn(&QueueCard, &QueueRow) -> Value) -> Option<Value> {
        let (f, n) = (q.get("repo")?, q.get("issue")?.parse::<u64>().ok()?);
        let tab = self.queue_tab(ctx);
        let c = tab.fleets.iter().find(|c| &c.name == f)?;
        let merged = |d: &super::DoneRow| QueueRow {
            issue: n,
            state: "merged".into(),
            title: d.title.clone(),
            pr: super::PrLine {
                pr: d.pr.as_deref().and_then(|p| p.parse().ok()),
                ..super::PrLine::default()
            },
            notes: vec![d.note.clone()],
            ..QueueRow::default()
        };
        let done = c.done.iter().find(|d| d.issue == n).map(merged);
        Some(draw(c, c.rows.iter().find(|r| r.issue == n).or(done.as_ref())?))
    }

    fn queue_screen(&self, ctx: &Ctx, q: &Q) -> Value {
        let (tab, ev) = (self.queue_tab(ctx), working(&ctx.state).unwrap_or_default());
        let lanes = Config::kept(ctx).map_or(vec![], |cfg| self.lane_list(ctx, &cfg, &ev, &ev.states));
        let sel = q
            .get("repo")
            .filter(|r| tab.fleets.iter().any(|f| &f.name == *r))
            .map(String::as_str);
        let all: usize = tab.fleets.iter().map(|c| c.rows.len()).sum();
        let mut opts = vec![json!({"value": "all", "label": format!("all {all}")})];
        let dot = |c: &QueueCard| format!("{} {}{}", c.name, c.rows.len(), if c.need > 0 { " ●" } else { "" });
        opts.extend(tab.fleets.iter().map(|f| json!({"value": f.name, "label": dot(f)})));
        let mut shown: Vec<&QueueCard> = tab.fleets.iter().filter(|f| sel.is_none_or(|s| f.name == s)).collect();
        shown.sort_by_key(|c| c.need == 0);
        let mut blocks = vec![
            json!({"type": "chips", "id": "repo", "name": "repo", "selected": [sel.unwrap_or("all")], "action": "filter", "options": opts}),
            stats(ctx.now, &ev, &ev.states, sel, &lanes),
            counts(&shown),
        ];
        if let Some(e) = &tab.error {
            blocks.push(json!({"type": "text", "id": "error", "markdown": e}));
        }
        blocks.extend(shown.into_iter().map(|c| fleet_card(ctx, &ev, c, q, &lanes)));
        let mut kept = q.clone();
        kept.retain(|k, _| k != "repo" || sel.is_some());
        let stream = view(&kept, "", "").replacen("/screens/queue", "/screens/queue/stream", 1);
        let menu = [
            json!({"label": "Fleets…", "tap": {"sheet": "/screens/fleets"}}),
            json!({"label": "Drive all now", "action": "drive-all"}),
            json!({"label": "Legend [?]", "tap": {"sheet": "/screens/legend"}}),
        ];
        let badge = self.needs_count(ctx, &ev);
        json!({"type": "screen", "id": "queue", "title": "Queue", "stream": stream, "badge": badge,
            "actions": [{"icon": "more", "label": "More", "menu": menu}], "blocks": blocks})
    }

    fn chat_cards(&self, ctx: &Ctx, paired: bool) -> Value {
        let tab = self.needs_tab(ctx, true);
        let ev = working(&ctx.state).unwrap_or_default();
        let mut cards = run_cards(ctx.now, &ev);
        cards.extend(super::chat::owner_cards(ctx, &ev, paired));
        cards.extend(tab.config.as_ref().map(|e| {
            json!({"type": "card", "id": "config-invalid", "kind": "alert", "tone": "bad",
                "title": "fleet.toml is invalid", "blocks": [{"type": "text", "id": "config-why", "markdown": e}]})
        }));
        cards.extend(super::v1::slo_open(&ev).map(|e| {
            json!({"type": "card", "id": format!("slo-{}", e.seq), "kind": "alert", "tone": "warn",
                "title": "The board is slow", "blocks": [{"type": "text", "id": "slo-why", "markdown": e.str("reason")}]})
        }));
        cards.extend(crate::slo::open_stuck(&ev).into_iter().map(|(f, es)| {
            let mut md: Vec<String> = es.iter().map(|e| format!("- {}", crate::events::plain(e))).collect();
            md.push(format!("`drover waiting {f}`"));
            let mut buttons = vec![json!({"type": "button", "id": format!("stuck-drive-{f}"), "label": "Drive now", "action": "drive-all"})];
            buttons.extend(es.iter().filter_map(|e| e.issue).map(|n| {
                json!({"type": "button", "id": format!("stuck-force-{f}-{n}"), "label": format!("Force #{n}"), "action": "force", "values": {"fleet": f, "issue": n}})
            }));
            json!({"type": "card", "id": format!("stuck-{f}"), "kind": "alert", "tone": "warn",
                "title": "Drive is stuck", "blocks": [{"type": "text", "id": "stuck-why", "markdown": md.join("\n")}],
                "buttons": buttons})
        }));
        cards.extend(tab.asks.iter().map(|a| ask_card(ctx.now, ask_minutes(ctx), a)));
        cards.extend(tab.issues.iter().map(|i| needs_card(i, &ev)));
        Value::Array(cards)
    }

    pub(super) fn plugin(ctx: &Ctx, head: &str) -> bool {
        let file = std::fs::read_to_string(ctx.config.with_file_name("cooee-token")).unwrap_or_default();
        let bearer = header(head, "authorization")
            .and_then(|h| h.strip_prefix("Bearer "))
            .unwrap_or_default();
        [
            drover_core::config::env("COOEE_PLUGIN_TOKEN").unwrap_or_default(),
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
            ("GET", p) if p.starts_with("/screens/lanes/") && !plugin && auth.is_err() => {
                line("401 Unauthorized", "pair this device first")
            }
            ("GET", p) => match self.screen(ctx, p, &form(query), plugin || auth.is_ok()) {
                Some(v) => out("200 OK", &v),
                None => line("404 Not Found", "no such screen"),
            },
            ("POST", "/action") if self.addr.is_none() => line("405 Method Not Allowed", "read-only: GET only"),
            ("POST", "/action") if !plugin && auth.is_err() => line("401 Unauthorized", "pair this device first"),
            ("POST", "/action") => {
                let Ok(a) = serde_json::from_str::<Value>(body) else {
                    return line("400 Bad Request", "bad request body");
                };
                let name = a.at("action").as_str().unwrap_or_default();
                if !permitted(name) && (plugin || !THREAD_ACTIONS.contains(&name)) {
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
        self.op(ctx, &f)
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
                    _ => match v
                        .as_object()
                        .and_then(|m| m.iter().find_map(|(k, x)| Some((k.strip_suffix(".on")?, x))))
                    {
                        Some((f, on)) => ("/screens/backlog", format!("repo={f}&on={}", list(on))),
                        None => (
                            "/screens/queue",
                            format!(
                                "repo={}&more={}&fold={}",
                                list(&v["repo"]),
                                list(&v["more"]),
                                list(&v["fold"])
                            ),
                        ),
                    },
                };
                let s = self.screen(ctx, screen, &form(&q), true).context("no such screen")?;
                return Ok(json!({"ops": [{"op": "screen", "screen": s}]}));
            }
            "drive" => self.form_op(ctx, "drive", &[("fleet", fleet)])?,
            "force" => crate::slo::force(ctx, &cfg, &self.r, &mut vec![], (&fleet, n()?))?,
            "drive-all" => {
                let live = cfg.fleet.iter().filter(|f| !f.1.paused);
                let lines: Vec<String> = live
                    .map(|(f, _)| self.form_op(ctx, "drive", &[("fleet", f.clone())]))
                    .collect::<Result<_>>()?;
                lines.join("; ")
            }
            "pause" => self.form_op(
                ctx,
                if v["on"] == false { "resume" } else { "pause" },
                &[("fleet", fleet)],
            )?,
            "top" | "drop" | "move" => {
                let at = |k: &str| v.at(k).as_u64();
                let op = if name == "drop" { "rm" } else { "mv" };
                let d = crate::queue::op(op, &[n()?], name == "top", at("before"), at("after"), None);
                crate::queue_change(ctx, &cfg, &fleet, d, Some("cooee"))?
            }
            "queue" => {
                let m = v.as_object().cloned().unwrap_or_default();
                let picks = m
                    .iter()
                    .filter(|(_, x)| **x == true)
                    .filter_map(|(k, _)| k.split_once(".pick."));
                let (fs, ns): (Vec<&str>, Vec<u64>) =
                    picks.filter_map(|(f, n)| Some((f, n.parse::<u64>().ok()?))).unzip();
                let Some(f) = fs.first() else {
                    let found = m.iter().find_map(|(k, x)| Some((k.strip_suffix(".q")?, x)));
                    let (f, words) = found.context("no issue ticked")?;
                    let mut q = form(&format!("repo={f}"));
                    q.insert("q".into(), list(words));
                    let s = self.screen(ctx, "/screens/backlog", &q, true).context("no backlog")?;
                    return Ok(json!({"ops": [{"op": "screen", "screen": s}], "toast": "no issue ticked: searched"}));
                };
                let mut d = crate::queue::op("add", &ns, false, None, None, None);
                d.put("check", json!(true));
                crate::queue_change(ctx, &cfg, f, d, Some("cooee"))? + " (needs-check)"
            }
            "limit" => crate::limit::wait(ctx, &self.r, (&fleet, n()?), "owner")?,
            "stop" | "kill" | "clean" | "close" => {
                let act = crate::api::Act {
                    agent: s("agent"),
                    op: name.into(),
                    confirm: "1".into(),
                };
                self.act(ctx, &cooee(), &act)?
            }
            "send" => {
                let send = crate::api::Send {
                    agent: s("agent"),
                    setup: None,
                    text: s("text"),
                };
                self.prompt(ctx, &cooee(), &send)?
            }
            "answer" => {
                let open = crate::events::open_asks(&working(&ctx.state)?)
                    .into_iter()
                    .find(|e| e.str("id") == Some(block))
                    .cloned();
                let at = open
                    .map(|a| a.fleet)
                    .or_else(|| (!fleet.is_empty()).then(|| fleet.clone()));
                let at = at.with_context(|| format!("no open ask {block}"))?;
                let pick = if s("other").trim().is_empty() {
                    s("choice")
                } else {
                    s("other")
                };
                self.form_op(ctx, "answer", &[("fleet", at), ("id", block.into()), ("choice", pick)])?
            }
            "unpark" => self.form_op(ctx, "unpark", &[("fleet", fleet), ("issue", n()?.to_string())])?,
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
                let act = if s("choice") == "approve" { "approve" } else { "deny" };
                let kv = [
                    ("id", block.into()),
                    ("act", act.into()),
                    ("sha", s("sha")),
                    ("via", "cooee".into()),
                ];
                self.form_op(ctx, "card", &kv)?
            }
            "config-card" => self.form_op(ctx, "config", &[("id", s("id")), ("act", s("act"))])?,
            "thread-signin" => crate::setup::start(ctx, &cfg, &self.r, &s("host"), &s("name"), "web")?,
            n if THREAD_ACTIONS.contains(&n) || n == "secret-card" => {
                let keys = ["text", "repo", "name", "act", "data", "q", "harness", "model", "effort"];
                let mut f: Q = keys.into_iter().map(|k| (k.into(), s(k))).collect();
                f.extend([
                    ("t".into(), s("thread")),
                    ("n".into(), s("msg")),
                    ("value".into(), s("text")),
                ]);
                f.insert("op".into(), n.strip_prefix("thread-").unwrap_or("secret").into());
                let mut later = vec![];
                let (res, more) = crate::conductor::deferred(|| super::chat::post(ctx, &self.r, &f, &mut later));
                self.later(ctx, later.into_iter().chain(more));
                self.kick.store(true, Relaxed);
                let line = res?;
                if n == "thread-new" {
                    let id = line.split(' ').next().unwrap_or_default();
                    let s = self
                        .threads_screen(ctx, &format!("/{id}"), None, true)
                        .context("no such thread")?;
                    return Ok(json!({"ops": [{"op": "screen", "screen": s}], "toast": line}));
                }
                line
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
        let close = ["save-fleets", "remove-fleet", "kill", "clean", "close", "queue"].contains(&name)
            || name == "answer" && a["screen"] != "cards";
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
                done.push(self.form_op(ctx, if on { "resume" } else { "pause" }, &[("fleet", f.clone())])?);
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
        let paired = Self::plugin(self.ctx, head)
            || devices::check(self.ctx, header(head, "cookie").unwrap_or_default()).is_ok();
        let screen = |c: &Ctx| crate::runner::request(|| self.screen(c, base, &q, paired));
        let first = screen(&ctx()).filter(|_| open && (Self::plugin(self.ctx, head) || self.allowed(self.ctx, head)));
        let Some(mut cur) = first else {
            STREAMS.fetch_sub(1, Relaxed);
            let r = line(
                if open {
                    "503 Service Unavailable"
                } else {
                    "404 Not Found"
                },
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
        let (mut log, mut ping) = (crate::db::last_seq(&self.ctx.state), drover_core::clock::instant());
        let mut built = drover_core::clock::instant();
        let res = loop {
            if let Err(e) = s.write_all(msg.as_bytes()) {
                break Err(e);
            }
            msg.clear();
            while msg.is_empty() {
                drover_core::clock::sleep(Duration::from_millis(500));
                let now = crate::db::last_seq(&self.ctx.state);
                if now != log || built.elapsed() >= Duration::from_secs(30) {
                    (log, built) = (now, drover_core::clock::instant());
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
                    (msg, ping) = (": ping\n\n".into(), drover_core::clock::instant());
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

fn cooee() -> devices::Device {
    devices::Device {
        label: "cooee".into(),
        ..Default::default()
    }
}
