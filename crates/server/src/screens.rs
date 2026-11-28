use super::kit::{
    act, button, confirm, fields, label, open, plain, sheet, text, thread, titled, toned, typed, url, wire, word, Kind,
};
use super::{
    devices, form, header, mtime, shown, working, Auth, BTreeMap, Config, Ctx, Event, Refusal, Result, Server,
};
use super::{LaneRow, LanesTab, NeedAsk, NeedIssue, QueueCard, QueueRow, JSON};
use crate::access::{civil_month_day, civil_year};
use crate::api::{Block, Herd, HerdFleet, HerdLane, Lane};
use crate::events::Json;
use anyhow::Context;
use blocks as k;
use k::{Act, Confirm, Figure, Label, Shown, Tap, Tile, Time, Tone, Toned};
use serde_json::Value;
use std::sync::atomic::Ordering::Relaxed;

#[path = "screens_epics.rs"]
mod epics;
#[path = "screens_lanes.rs"]
mod lanes;
#[path = "later.rs"]
mod later;
#[path = "screens_local.rs"]
pub(super) mod local;
#[path = "screens_pages.rs"]
pub(super) mod pages;
pub(super) use lanes::said;
use lanes::{lanes_screen, scene_screen};

pub(super) type Res = (&'static str, &'static str, Vec<u8>);
type Q = BTreeMap<String, String>;

#[derive(Default, serde::Serialize)]
pub(super) struct Reply {
    #[serde(skip_serializing_if = "Option::is_none")]
    ops: Option<Vec<k::Op>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    toast: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    close: Option<bool>,
}

impl Reply {
    fn screen(s: Value, toast: Option<String>) -> Value {
        let ops = Some(vec![k::Op::Screen { screen: typed(s) }]);
        wire(&Self {
            ops,
            toast,
            close: None,
        })
    }

    fn done(ops: Vec<k::Op>, toast: String, close: bool) -> Value {
        wire(&Self {
            ops: Some(ops),
            toast: Some(toast),
            close: Some(close),
        })
    }
}

fn out(status: &'static str, v: &Value) -> Res {
    (status, JSON, v.to_string().into_bytes())
}

pub(super) fn line(status: &'static str, l: &str) -> Res {
    out(status, &Value::Object(fields([("line", l.into())])))
}

pub use crate::manifest::permitted;

/// The manifest's places as JSON: the tabs, menu rows and cards the phone page draws.
pub fn places() -> Value {
    serde_json::to_value(crate::manifest::manifest("").screens).unwrap_or_default()
}

/// The standalone page's places: the manifest's, then the Threads tab and the Hosts row (never the plugin's).
pub fn page_places() -> Value {
    let threads = fields([
        ("type", "header-strip".into()),
        ("id", "threads".into()),
        ("label", "Threads".into()),
        ("screen", "/screens/threads".into()),
    ]);
    let hosts = fields([
        ("type", "menu-row".into()),
        ("label", "Hosts".into()),
        ("subtitle", "on/off and reap, per host".into()),
        ("screen", "/screens/hosts".into()),
    ]);
    let mut all = places().as_array().cloned().unwrap_or_default();
    all.extend([Value::Object(threads), Value::Object(hosts)]);
    Value::Array(all)
}

#[path = "screens_fleets.rs"]
mod fleets;
#[path = "screens_hosts.rs"]
mod hosts;

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

pub(super) fn ticking(t: u64, label: &str) -> Time {
    Time {
        since: Some(iso(t)),
        label: Some(label.into()),
        ..Time::default()
    }
}

pub(super) fn since(t: u64) -> Shown {
    Shown::Time(Time {
        since: Some(iso(t)),
        ..Time::default()
    })
}

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

fn figure(act: &str, state: &str) -> Figure {
    let state = match act {
        "impl" | "fix" | "rev" | "update" => Tone::Working,
        "done" => Tone::Done,
        "blocked" if state == "dead" || state == "blocked" => Tone::Fail,
        "blocked" => Tone::Needs,
        _ => Tone::Idle,
    };
    Figure {
        state: Some(state),
        act: Some(act.into()),
        ..Figure::default()
    }
}

pub(super) fn state(t: Tone) -> Figure {
    Figure {
        state: Some(t),
        ..Figure::default()
    }
}

fn badge_tone(fig: &Figure) -> Tone {
    fig.state.clone().filter(|s| *s != Tone::Idle).unwrap_or(Tone::Mute)
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

fn verdict_tone(word: &str) -> Tone {
    match word {
        "PASS" => Tone::Ok,
        "FAIL" => Tone::Bad,
        _ => Tone::Warn,
    }
}

fn verdict_badge(v: &Event) -> Label {
    let word = v.str("verdict").unwrap_or("?");
    label(word, verdict_tone(word))
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

fn row(ctx: &Ctx, ev: &[Event], paused: bool, f: &str, r: &QueueRow, k: Option<usize>, lane: Option<&Lane>) -> k::Row {
    let fig = Figure {
        grow: Some(r.grow.clone()),
        ..figure(&r.act, &r.state)
    };
    let title = format!(
        "#{} {}",
        r.issue,
        crate::gh::cached_title(ctx, f, r.issue).unwrap_or_default()
    );
    let state = r.state.replace('(', " ").replace(')', "").replace("⏸ waiting: ", "");
    let badge = match k {
        Some(k) => label(format!("#{}", k + 1), Tone::Mute),
        None if !r.stage.is_empty() && !matches!(fig.state, Some(Tone::Fail | Tone::Needs)) => {
            label(strip(&r.stage), badge_tone(&fig))
        }
        None => label(state, badge_tone(&fig)),
    };
    let v = titled(format!("q-{f}-{}", r.issue), cut(title.trim_end(), LINE)).badge(badge);
    let mut v = v.figure(Figure {
        issue: Some(r.issue),
        ..fig
    });
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
    let vals = fields([
        ("fleet", f.into()),
        ("issue", r.issue.into()),
        ("agent", r.lane.clone().into()),
    ]);
    if let Some(b) = r.blocked_by {
        v = v.subtitle(toned(format!("held until #{b}"), Tone::Bad));
    } else if !sub.is_empty() {
        v = v.subtitle(plain(cut(&sub, LINE)));
    }
    if k.is_none() {
        v = v.value(since(r.t));
    }
    v = v.tap(sheet(format!("/screens/issue?repo={f}&issue={}", r.issue)));
    if live {
        let stop = Act {
            tone: Some(Tone::Bad),
            confirm: Some(confirm(format!("Stop #{}'s lane {}?", r.issue, r.lane), "Stop lane")),
            ..act("Stop", "stop", vals.clone())
        };
        if limited {
            let wait = act(
                "Wait",
                "limit",
                fields([("fleet", f.into()), ("issue", r.issue.into())]),
            );
            v = v.swipe(vec![wait, stop]);
        } else {
            v = v.swipe(vec![stop]);
        }
    }
    if k.is_some() {
        let mut swipe = moves(f, r, |l, values| act(l, "move", values));
        swipe.extend([
            act("Top", "top", vals.clone()),
            Act {
                tone: Some(Tone::Bad),
                confirm: Some(confirm(format!("Drop #{} from the queue?", r.issue), "Drop")),
                ..act("Drop", "drop", vals)
            },
        ]);
        v = v.swipe(swipe);
    }
    v
}

fn moves<T>(f: &str, r: &QueueRow, each: impl Fn(&str, k::Fields) -> T) -> Vec<T> {
    let to = |key: &str, at: Option<u64>| {
        at.map(|at| fields([("fleet", f.into()), ("issue", r.issue.into()), (key, at.into())]))
    };
    let (up, down) = (to("before", r.up), to("after", r.down));
    [up.map(|x| each("▲", x)), down.map(|x| each("▼", x))]
        .into_iter()
        .flatten()
        .collect()
}

fn hot(r: &QueueRow) -> Vec<&str> {
    r.groups.iter().filter_map(|g| g.strip_prefix("hot ")).collect()
}

fn lock_row(f: &str, r: &QueueRow) -> Option<k::Row> {
    let (n, i) = (hot(r).len(), r.issue);
    let serial = r.groups.iter().filter(|g| !g.starts_with("hot "));
    let mut tags: Vec<String> = serial.map(|g| format!("🔒 {g}")).collect();
    tags.extend((n > 0).then(|| format!("🔒 hot ×{n}")));
    let mut v = titled(format!("hot-{f}-{i}"), tags.join(" · "))
        .indent(1)
        .style(k::RowStyle::Link);
    if n > 0 {
        v = v.tap(sheet(format!("/screens/hot?repo={f}&issue={i}")));
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

fn link(id: &str, title: &str, tap: Tap) -> k::Row {
    titled(id, title).style(k::RowStyle::Link).tap(tap)
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

fn fleet_card(ctx: &Ctx, ev: &[Event], c: &QueueCard, q: &Q, lanes: &[Lane], late: Option<&String>) -> k::Block {
    let f = &c.name;
    let (queued, busy): (Vec<&QueueRow>, Vec<&QueueRow>) = c.rows.iter().partition(|r| r.state == "queued");
    let paused = c.paused.is_some();
    let rows = |id: &str, ts: Vec<(&QueueRow, usize)>| -> (usize, Vec<k::Block>) {
        let mut groups: Vec<Vec<k::Row>> = vec![];
        let mut i = 0;
        while let Some(&(r, d)) = ts.get(i) {
            let k = queued.iter().position(|x| x.issue == r.issue);
            let mut v = row(ctx, ev, paused, f, r, k, lanes.iter().find(|l| l.name == r.lane));
            if d > 0 {
                v = v.indent(u8::try_from(d.min(2)).unwrap_or(2));
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
                g.push(link(&format!("chain-{key}"), &title, open(view(q, "fold", &key))));
            }
            groups.push(g);
            i += 1 + if folded { kids } else { 0 };
        }
        let n = groups.len();
        let all = picked(q, "more").contains(&f.as_str());
        let shown = groups.into_iter().take(if all { n } else { SHOWN });
        let mut out: Vec<k::Block> = shown.flatten().map(Kind::block).collect();
        if n > SHOWN {
            let title = if all {
                "show fewer".into()
            } else {
                format!("+{} more", n - SHOWN)
            };
            out.push(link(&format!("more-{id}-{f}"), &title, open(view(q, "more", f))).block());
        }
        (ts.len(), out)
    };
    let section = |id: &str, word: &str, (n, rs): (usize, Vec<k::Block>)| {
        let list = rows_list(&format!("{id}-list-{f}"), rs);
        k::Section::new(vec![list])
            .id(format!("{id}-{f}"))
            .header(format!("{word} ({n})"))
            .block()
    };
    let badge = if paused {
        label("paused", Tone::Mute)
    } else {
        label("driving", Tone::Ok)
    };
    let chevron = |id: String, title: String, tap: Tap| titled(id, title).accessory(k::Accessory::Chevron).tap(tap);
    let mut end = vec![chevron(
        format!("done-today-{f}"),
        "done today".into(),
        open(format!("/screens/queue/done?repo={f}")),
    )
    .value(c.done.len().to_string())
    .block()];
    if let Some(b) = &c.backlog {
        end.push(
            chevron(
                format!("backlog-{f}"),
                format!("backlog ({})", b.issues.len()),
                sheet(format!("/screens/backlog?repo={f}")),
            )
            .block(),
        );
    }
    let busy = rows("inprogress", busy.into_iter().map(|r| (r, 0)).collect());
    let tone = if c.need > 0 { Tone::Warn } else { Tone::Accent };
    let subtitle = match (&c.paused, late) {
        (Some(p), _) => plain(p),
        (None, Some(l)) => toned(l.as_str(), Tone::Warn),
        (None, None) => plain(&c.drive.line),
    };
    let fleet = fields([("fleet", f.as_str().into())]);
    let buttons = vec![
        button(format!("drive-{f}"), "Drive now", "drive", fleet.clone())
            .style(k::ButtonStyle::Primary)
            .block(),
        button(
            format!("pause-{f}"),
            if paused { "Resume" } else { "Pause" },
            "pause",
            fields([("fleet", f.as_str().into()), ("on", (!paused).into())]),
        )
        .block(),
    ];
    let mut blocks = epics::epics(ctx, ev, c, q, lanes);
    blocks.extend([
        section("inprogress", "in progress", busy),
        section("queued", "queued", rows("queued", tree(&queued))),
        rows_list(&format!("done-{f}"), end),
    ]);
    let mut v = k::Card::new()
        .id(format!("fleet-{f}"))
        .tone(tone)
        .title(f.as_str())
        .badge(badge)
        .subtitle(subtitle)
        .buttons(buttons)
        .blocks(blocks);
    if let Some(p) = &c.paused {
        let main = k::FoldMain {
            label: "Resume".into(),
            action: "pause".into(),
            values: Some(fields([("fleet", f.as_str().into()), ("on", false.into())])),
            ..k::FoldMain::default()
        };
        v = v.fold(k::Fold {
            line: format!("{f} · {p} · {} open", c.rows.len()),
            main: Some(main),
        });
    }
    v.block()
}

fn done_screen(c: &QueueCard, q: &Q) -> k::Block {
    let f = &c.name;
    let (done, more) = pages::page("/screens/queue/done", q, c.done.iter().map(|d| ((d.t, d.issue), d)));
    let rows: Vec<k::Block> = (done.into_iter())
        .map(|d| {
            let sha: String = d.sha.as_deref().unwrap_or("?").chars().take(7).collect();
            let at = format!("{:02}:{:02} UTC", d.t % 86_400 / 3600, d.t % 3600 / 60);
            let title = format!("#{} {}", d.issue, d.title.as_deref().unwrap_or_default());
            titled(format!("done-{}-{}", d.issue, d.t), title.trim_end())
                .subtitle(plain(format!("merged {at} · {sha}")))
                .value(d.age.as_str())
                .accessory(k::Accessory::Chevron)
                .tap(sheet(format!("/screens/issue?repo={f}&issue={}", d.issue)))
                .block()
        })
        .collect();
    let mut list = k::List::new(rows).id("done-list");
    list.next = more.map(k::More::Link);
    k::Screen::new()
        .id(format!("done-{f}"))
        .title(format!("{f} · done today"))
        .blocks(vec![list.block()])
        .block()
}

const FILTERS: [(&str, &str); 5] = [
    ("new", "new"),
    ("30", ">30 days"),
    ("90", ">90 days"),
    ("ac", "has AC"),
    ("pr", "in a closed PR"),
];

fn chip(value: &str, label: impl Into<String>) -> k::ChipOption {
    k::ChipOption {
        value: value.into(),
        label: label.into(),
    }
}

fn backlog(now: u64, c: &QueueCard, q: &Q) -> Option<k::Block> {
    let (f, b) = (&c.name, c.backlog.as_ref()?);
    let on = picked(q, "on");
    let words = q.get("q").map_or(String::new(), |w| w.to_lowercase());
    let search: k::Field = k::TextField::new(format!("{f}.q"))
        .label("Search")
        .value(k::FieldValue::Text(words.clone()))
        .placeholder("search #, title, label")
        .into();
    let mut is = vec![];
    for (n, i) in b.issues.iter().filter_map(|(n, i)| Some((n.parse::<u64>().ok()?, i))) {
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
            let pick = k::ToggleField::new(format!("{f}.pick.{n}"))
                .label(label.trim_end_matches(" · "))
                .value(k::FieldValue::Flag(false));
            is.push(((n, 0), k::Field::from(pick)));
        }
    }
    let (picks, more) = pages::page("/screens/backlog", q, is);
    let fields: Vec<k::Field> = std::iter::once(search).chain(picks).collect();
    let opts: Vec<k::ChipOption> = FILTERS.iter().map(|(v, l)| chip(v, *l)).collect();
    let read = format!(
        "read {} ago · Drive now reads it again; ticked ones queue as needs-check",
        b.ago
    );
    let mut form = k::Form::new(fields).id(format!("bl-{f}"));
    form.next = more;
    let chips = k::Chips::new(format!("{f}.on"), opts)
        .id("on")
        .multi(true)
        .selected(on.iter().map(|s| (*s).to_string()).collect::<Vec<_>>())
        .action("filter");
    let issues = k::Section::new(vec![form.block()])
        .id("issues")
        .header("open, not queued")
        .footer(read);
    let done = k::Submit {
        label: "Queue ticked".into(),
        action: "queue".into(),
        ..k::Submit::default()
    };
    let title = format!("{f} backlog · {} open", b.issues.len());
    Some(
        k::Modal::new(title, vec![chips.block(), issues.block()])
            .id(format!("backlog-{f}"))
            .cancel("Cancel")
            .done(done)
            .block(),
    )
}

fn section(id: &str, header: &str, blocks: Vec<k::Block>) -> k::Block {
    k::Section::new(blocks).id(id).header(header).block()
}

fn rows_list(id: &str, rows: Vec<k::Block>) -> k::Block {
    k::List::new(rows).id(id).block()
}

fn check_rows(hist: &[Round], r: &QueueRow) -> Vec<k::Row> {
    let toned_row = |id: &str, title: &str, text: &str, t: Tone| titled(id, title).subtitle(toned(text, t));
    let mut rows = vec![];
    if r.dead {
        rows.push(toned_row(
            "dead",
            "dead",
            "the log has this lane open, no such agent",
            Tone::Bad,
        ));
    }
    if r.check {
        rows.push(toned_row(
            "needs-check",
            "needs-check",
            "from the backlog, spec not checked",
            Tone::Mute,
        ));
    }
    if let Some(ci) = &r.pr.ci {
        let (g, t) = match ci.as_str() {
            "green" => ("✓", Tone::Ok),
            "failed" => ("✗", Tone::Bad),
            _ => ("…", Tone::Warn),
        };
        rows.push(titled("ci", "CI").badge(label(g, t)));
    }
    let mut seen = vec![];
    for (h, v) in hist.iter().rev().filter_map(|h| Some((h, h.verdict?))) {
        if seen.contains(&h.key) {
            continue;
        }
        seen.push(h.key);
        let round = v
            .data
            .get("round")
            .and_then(Value::as_u64)
            .map_or(String::new(), |k| k.to_string());
        let mut row = titled(format!("v-{}", h.key), h.key)
            .badge(verdict_badge(v))
            .value(round);
        if let Some(x) =
            r.pr.form
                .as_ref()
                .filter(|_| v.data.get("form_only") == Some(&Value::Bool(true)))
        {
            row = row.subtitle(toned(format!("not counted · form {x}"), Tone::Warn));
        }
        rows.push(row);
    }
    rows
}

fn history_rows(now: u64, f: &str, n: u64, hist: &[Round], live: bool) -> Vec<k::Block> {
    let last = hist.len().saturating_sub(1);
    let row = |(i, h): (usize, &Round)| {
        let mut row = titled(format!("h-{i}"), h.name.as_str());
        if let Some(v) = h.verdict {
            let word = v.str("verdict").unwrap_or("?");
            let sha: String = v.str("sha").unwrap_or_default().chars().take(7).collect();
            let ago = crate::events::age(now.saturating_sub(v.ts));
            row = row
                .subtitle(toned(format!("{word} · {sha} · {ago}"), verdict_tone(word)))
                .tap(sheet(format!("/screens/verdict?repo={f}&issue={n}&seq={}", v.seq)));
        } else if h.check && !(i == last && live) {
            row = row.subtitle(toned("no verdict", Tone::Warn));
        }
        row.block()
    };
    hist.iter().enumerate().rev().map(row).collect()
}

fn issue_sheet(ctx: &Ctx, ev: &crate::events::Log, c: &QueueCard, r: &QueueRow) -> k::Block {
    let (f, n) = (c.name.as_str(), r.issue);
    let hist = history(ev, f, n);
    let mut status = vec![];
    if !r.stage.is_empty() {
        let label = |s: &super::Stage| format!("{} {}{}", s.g, s.name, if s.ok { " ✓" } else { "" });
        let opts: Vec<k::ChipOption> = r.stage.iter().map(|s| chip(&s.name, label(s))).collect();
        let lit: Vec<String> = r.stage.iter().filter(|s| s.at == 0).map(|s| s.name.clone()).collect();
        status.push(k::Chips::new("stage", opts).id("stage").selected(lit).block());
    }
    let mut now = titled("now", r.state.as_str()).value(since(r.t));
    let held = r.blocked_by.map(|b| format!("held until #{b}"));
    if let Some(w) = crate::pending::waits(ev, f, n, c.paused.is_some()).or(held) {
        now = now.subtitle(plain(w));
    }
    let mut rows = vec![now];
    if !r.lane.is_empty() {
        let l = titled("at", format!("lane {}", r.lane));
        let at = [r.place.as_str(), r.host.as_str()].into_iter().find(|s| !s.is_empty());
        rows.push(l.subtitle(plain(at.unwrap_or(if r.live { "live" } else { "gone" }))));
    }
    rows.extend(check_rows(&hist, r));
    rows.extend(r.notes.iter().enumerate().map(|(i, x)| titled(format!("note-{i}"), x)));
    status.push(rows_list("status-list", rows.into_iter().map(Kind::block).collect()));
    let mut blocks = vec![section("status", "status", status)];
    if let Some(a) = &r.open {
        blocks.push(section(
            "asks",
            &format!("asks: {}", a.ask.question),
            vec![ask_choice(&a.ask).block()],
        ));
    }
    let vals = fields([("fleet", f.into()), ("issue", n.into())]);
    if let Some(d) = &r.decided {
        let mut change = r.ask.as_ref().map(ask_choice);
        for o in change.iter_mut().flat_map(|c| c.options.iter_mut()) {
            o.values = Some(vals.clone());
        }
        let change: Vec<k::Block> = change.into_iter().map(Kind::block).collect();
        blocks.push(section("decided", &format!("decided by default: {d}"), change));
    }
    let gh = format!("https://github.com/{}", c.config.repo);
    let to = |id: &str, t: String, u: String| link(id, &t, url(u));
    let mut links = vec![to("issue", format!("issue #{n}"), format!("{gh}/issues/{n}"))];
    let spec = crate::gh::cached_title(ctx, f, n);
    match (r.pr.pr, &r.pr.run) {
        (Some(p), _) => {
            let mut pr = to("pr", format!("PR #{p}"), format!("{gh}/pull/{p}"));
            let cached = crate::gh::cached_pr(ctx, f, n);
            if let Some(t) = cached.at("title").as_str().filter(|t| Some(*t) != spec.as_deref()) {
                pr = pr.subtitle(plain(t));
            }
            links.extend([pr, to("checks", "checks".into(), format!("{gh}/pull/{p}/checks"))]);
        }
        (None, Some(run)) => links.push(to("checks", "checks".into(), run.clone())),
        _ => {}
    }
    if r.live && !r.lane.is_empty() {
        links.push(link(
            "lane",
            &format!("lane {}", r.lane),
            sheet(format!("/screens/lanes/{}", r.lane)),
        ));
    }
    if let Some((a, t)) = r.open.as_ref().and_then(|a| Some((a, a.thread.as_ref()?))) {
        let title = format!("asked in {}", a.title.as_ref().filter(|s| !s.is_empty()).unwrap_or(t));
        links.push(link("asked", &title, thread(t)));
    }
    let links = links.into_iter().map(Kind::block).collect();
    blocks.push(section("links", "links", vec![rows_list("links-list", links)]));
    if !r.lanes.is_empty() {
        let lane = |l: &super::QueueLane| {
            let v = hist.iter().rev().find(|h| h.check && h.agent == Some(l.name.as_str()));
            let state = v
                .and_then(|h| h.verdict)
                .map_or(plain(&l.state), |v| Toned::Label(verdict_badge(v)));
            titled(format!("l-{}", l.name), l.name.as_str())
                .subtitle(state)
                .value(l.ago.as_str())
                .tap(sheet(format!("/screens/lanes/{}", l.name)))
                .block()
        };
        let rows: Vec<k::Block> = r.lanes.iter().map(lane).collect();
        blocks.push(section("lanes", "lanes", vec![rows_list("lanes-list", rows)]));
    }
    if !hist.is_empty() {
        let rows = history_rows(ctx.now, f, n, &hist, r.live);
        blocks.push(section("history", "history", vec![rows_list("history-list", rows)]));
    }
    let button = |l: &str, action: &str, values: k::Fields| button(l, l, action, values);
    blocks.extend(moves(f, r, |l, values| button(l, "move", values).block()));
    if r.state == "parked" {
        blocks.push(button("Unpark", "unpark", vals.clone()).block());
    }
    if r.queued {
        let drop = button("Drop", "drop", vals.clone())
            .style(k::ButtonStyle::Destructive)
            .confirm(confirm(format!("Drop #{n} from the queue?"), "Drop"));
        blocks.extend([button("Top", "top", vals).block(), drop.block()]);
    }
    let title = format!(
        "#{n} {}",
        spec.or_else(|| r.title.clone().filter(|_| r.state == "merged"))
            .unwrap_or_default()
    );
    k::Modal::new(title.trim_end(), blocks)
        .id(format!("issue-{f}-{n}"))
        .cancel("Done")
        .block()
}

fn verdict_sheet(ev: &[Event], q: &Q) -> Option<k::Block> {
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
        k::Modal::new(format!("#{n} {stage} {word}"), vec![text("verdict-body", body)])
            .id(format!("verdict-{f}-{n}-{seq}"))
            .cancel("Done")
            .block(),
    )
}

fn tile(label: &str, value: impl Into<Shown>) -> Tile {
    Tile {
        label: label.into(),
        value: value.into(),
        ..Tile::default()
    }
}

fn stats(ctx: &Ctx, ev: &[Event], states: &crate::events::States, only: Option<&str>, lanes: &[Lane]) -> k::Block {
    let day = |e: &Event| usize::try_from(ctx.now.saturating_sub(e.ts) / 86_400).unwrap_or(usize::MAX);
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
    let spark = |f: &dyn Fn(&(u32, u32)) -> serde_json::Number| per.iter().map(f).collect::<Vec<_>>();
    let rounds = |p: &(u32, u32)| serde_json::Number::from_f64(avg(p.0, p.1)).unwrap_or_else(|| 0.into());
    let mut items = vec![
        Tile {
            spark: Some(spark(&|p| p.0.into())),
            ..tile("merged · 7d", n.to_string())
        },
        tile("busy", c.busy.to_string()),
        tile("approval", c.waiting_approval.to_string()),
        tile("owner", c.waiting_owner.to_string()),
        tile("rate-limited", c.rate_limited.to_string()),
        Tile {
            spark: Some(spark(&rounds)),
            ..tile("rounds / issue", format!("{:.1}", avg(n, r)))
        },
    ];
    let fleets = Config::kept(ctx).into_iter().flat_map(|c| c.fleet);
    let flights = fleets
        .filter(|f| only.is_none_or(|o| o == f.0))
        .filter_map(|(f, fc)| crate::queue::flight(ctx, ev, states, &fc, &f))
        .map(|(v, s, cap)| (v.len() + s.len(), cap))
        .reduce(|a, b| (a.0 + b.0, a.1 + b.1));
    items.extend(flights.map(|(v, cap)| tile("in flight", format!("{v}/{cap}"))));
    k::Stat::new().id("stats").items(items).block()
}

fn counts(shown: &[&QueueCard]) -> k::Block {
    let sum = |f: &dyn Fn(&QueueCard) -> usize| shown.iter().map(|c| f(c)).sum::<usize>();
    let need = sum(&|c| c.need);
    let queued = sum(&|c| c.rows.iter().filter(|r| r.state == "queued").count());
    let busy = sum(&|c| c.rows.len()) - need - queued;
    let tone = if need > 0 {
        Tone::Warn
    } else {
        Tone::Other(String::new())
    };
    let items = vec![
        Tile {
            tone: Some(tone),
            ..tile("needs you", need.to_string())
        },
        tile("in progress", busy.to_string()),
        tile("queued", queued.to_string()),
        tile("done today", sum(&|c| c.done_n).to_string()),
    ];
    k::Stat::new().id("counts").items(items).block()
}

fn option(value: &str, label: &str) -> k::ChoiceOption {
    k::ChoiceOption {
        value: value.into(),
        label: label.into(),
        ..k::ChoiceOption::default()
    }
}

fn fold(n: Option<u64>, kind: &str, q: &str, age: &str, main: Option<k::FoldMain>) -> k::Fold {
    let cut = cut(q, LINE);
    let n = n.map_or(String::new(), |n| format!("#{n} · "));
    k::Fold {
        line: format!("{n}{kind} · {cut} · {age}"),
        main,
    }
}

fn recommended(q: &crate::api::Ask) -> Option<&String> {
    q.recommend
        .and_then(|n| q.options.get(usize::try_from(n).ok()?.checked_sub(1)?))
}

fn ask_choice(q: &crate::api::Ask) -> k::Choice {
    let rec = recommended(q);
    let (first, rest): (Vec<&String>, Vec<&String>) = q.options.iter().partition(|o| Some(*o) == rec);
    let opts: Vec<k::ChoiceOption> = (first.into_iter().chain(rest)).map(|o| option(o, o)).collect();
    let mut c = k::Choice::new("answer", opts).id(q.id.as_str());
    if let Some(r) = rec {
        c = c.recommended(r.as_str());
    }
    let other = match q.options.iter().find(|o| *o == "revise") {
        Some(r) => k::Other {
            placeholder: Some("a revise note…".into()),
            r#for: Some(r.clone()),
        },
        None => k::Other {
            placeholder: Some("other…".into()),
            r#for: None,
        },
    };
    c.other(other)
}

fn ask_card(now: u64, mins: u64, a: &NeedAsk) -> k::Card {
    let (q, rec) = (&a.ask, recommended(&a.ask));
    let routine = a.kind.starts_with("routine");
    let mut c = ask_choice(&a.ask);
    if let Some(r) = rec.filter(|_| routine) {
        c = c.default(k::ChoiceDefault {
            value: r.clone(),
            until: iso(q.ts + 60 * mins),
            label: Some("takes the recommendation in {rel}".into()),
        });
    }
    let issue = q.issue.map_or(String::new(), |n| format!(" #{n}"));
    let design = if a.pngs.is_empty() { "" } else { " · design" };
    let mut blocks = vec![k::Text::new().markdown(format!("**{}**", q.question)).block()];
    if !a.pngs.is_empty() {
        let n = q.issue.unwrap_or(0);
        let items = (0..).zip(&a.pngs).map(|(i, p)| k::Image {
            src: format!("/design/{}/{n}/{i}/{}", q.fleet, q.id),
            alt: Some(p.clone()),
            caption: None,
        });
        blocks.push(
            k::Images::new(items.collect::<Vec<_>>())
                .id(format!("pngs-{}", q.id))
                .block(),
        );
    }
    blocks.push(c.block());
    let kind = match (design, routine) {
        ("", false) => "owner",
        ("", true) => "routine",
        _ => "design",
    };
    let age = crate::events::age(now.saturating_sub(q.ts));
    let main = rec.or(q.options.first()).map(|o| k::FoldMain {
        label: o.clone(),
        action: "answer".into(),
        block: Some(q.id.clone()),
        value: Some(o.clone()),
        values: None,
    });
    k::Card::new()
        .id(format!("ask-{}", q.id))
        .kind(k::CardKind::Ask)
        .tone(Tone::Warn)
        .title(format!("{}{issue}{design}", q.fleet))
        .meta(format!("{kind} · {age}"))
        .fold(fold(q.issue, kind, &q.question, &age, main))
        .blocks(blocks)
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

fn ask_sheet(ctx: &Ctx, q: &Q) -> Option<k::Block> {
    let seq = q.get("seq")?.parse::<u64>().ok()?;
    let log = working(&ctx.state).ok()?;
    let a = log.get(log.binary_search_by_key(&seq, |e| e.seq).ok()?)?;
    let a = (a.kind == "needs-decision").then_some(a)?;
    let ask = super::v1::ask(a);
    let blocks = if let Some(o) = ask_outcome(&log, a) {
        vec![k::Text::new().markdown(format!("**{}**\n\n{o}", ask.question)).block()]
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
        ask_card(ctx.now, ask_minutes(ctx), &need).blocks.unwrap_or_default()
    };
    let id = a.str("id").unwrap_or_default();
    Some(
        k::Modal::new(format!("{} · {id}", a.fleet), blocks)
            .id(format!("asks-{seq}"))
            .cancel("Done")
            .block(),
    )
}

fn needs_card(i: &NeedIssue, ev: &crate::events::Log) -> k::Block {
    let (f, n) = (&i.fleet, i.issue);
    let st = ev.states.get(&(f.clone(), n));
    let action = st.map_or("one more round", |s| s.card());
    let vals = fields([("fleet", f.as_str().into()), ("issue", n.into())]);
    let with = |value: &str, label: &str| k::ChoiceOption {
        values: Some(vals.clone()),
        ..option(value, label)
    };
    let mut opts = vec![with("extend", action)];
    if i.merge {
        let confirm = Confirm {
            message: Some("Logged by: owner. CI and serial still gate it.".into()),
            destructive: Some(true),
            ..confirm(format!("Merge #{n} past its FAIL?"), "Merge")
        };
        opts.push(k::ChoiceOption {
            confirm: Some(confirm),
            ..with("merge", "merge anyway")
        });
    }
    opts.push(with("park", "park"));
    let fail = if i.fail.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", i.fail)
    };
    let mut blocks = vec![k::Text::new().markdown(format!("**{}**{fail}", i.why)).block()];
    let at = i.shots_at.as_deref().map_or(String::new(), |a| format!("/{a}"));
    let shots = i.shots.iter().enumerate();
    let items: Vec<k::Image> = shots
        .map(|(k, a)| k::Image {
            src: format!("/shot/{f}/{n}/{k}{at}"),
            alt: Some(a.clone()),
            caption: None,
        })
        .collect();
    if !items.is_empty() {
        blocks.push(k::Images::new(items).id(format!("shots-{f}-{n}")).block());
    }
    if !i.pending {
        blocks.push(k::Choice::new("needs-human", opts).id(format!("nh-{f}-{n}")).block());
    }
    let age = i.who.rsplit(" · ").next().unwrap_or_default();
    let main = (!i.pending).then(|| k::FoldMain {
        label: action.into(),
        action: "needs-human".into(),
        block: Some(format!("nh-{f}-{n}")),
        value: Some("extend".into()),
        values: Some(vals.clone()),
    });
    k::Card::new()
        .id(format!("nhc-{f}-{n}"))
        .kind(k::CardKind::Ask)
        .tone(Tone::Bad)
        .title(format!("{f} #{n} · {}", i.title))
        .meta(format!("needs-human · {age}"))
        .fold(fold(Some(n), "needs-human", &i.why, age, main))
        .blocks(blocks)
        .block()
}

fn health_line(now: u64, ev: &[Event]) -> Option<k::Block> {
    let h = crate::slo::drive_health(ev);
    let oldest = now.saturating_sub(h.first()?.since) / 60;
    let mut parts: Vec<String> = ["drive late", "idle", "pass error"]
        .iter()
        .filter_map(|w| {
            let n = (h.iter().filter(|x| x.what == *w).map(|x| x.fleet)).collect::<std::collections::BTreeSet<_>>();
            let s = if n.len() == 1 { "" } else { "s" };
            (!n.is_empty()).then(|| format!("{w}: {} fleet{s}", n.len()))
        })
        .collect();
    parts.push(format!("{oldest}m"));
    let row = titled("drive-health", parts.join(" · "))
        .figure(state(Tone::Other("blocked".into())))
        .accessory(k::Accessory::Chevron)
        .tap(sheet("/screens/drive-health"));
    Some(rows_list("drive-health-list", vec![row.block()]))
}

fn health_sheet(now: u64, ev: &[Event]) -> k::Block {
    let rows: Vec<k::Block> = (crate::slo::drive_health(ev).iter().enumerate())
        .map(|(i, h)| {
            titled(format!("health-{i}"), format!("{} · {}", h.fleet, h.what))
                .subtitle(plain(h.reason))
                .value(format!("{}m", now.saturating_sub(h.since) / 60))
                .block()
        })
        .collect();
    let drive = k::Button::new("Drive now")
        .id("health-drive")
        .style(k::ButtonStyle::Primary)
        .action("drive-all");
    k::Modal::new("Drive health", vec![rows_list("health-rows", rows), drive.block()])
        .id("drive-health")
        .cancel("Done")
        .block()
}

fn run_cards(now: u64, ev: &[Event]) -> Vec<k::Block> {
    let open = ev
        .iter()
        .filter(|e| e.kind == "run-card" && now <= e.ts + crate::card::TTL);
    let open = open.filter(|e| e.str("id").is_some_and(|id| crate::card::settled(ev, id).is_none()));
    open.map(|e| {
        let s = |k: &str| e.str(k).unwrap_or_default();
        let sha = s("sha");
        let run = k::Run::new("run-card", s("cmd"), sha.get(..sha.len().min(12)).unwrap_or_default())
            .id(s("id"))
            .dir(s("dir"))
            .touches(s("blast"))
            .until(iso(e.ts + crate::card::TTL));
        k::Card::new()
            .id(format!("run-{}", s("id")))
            .kind(k::CardKind::Run)
            .title(s("why"))
            .blocks(vec![run.block()])
            .block()
    })
    .collect()
}

fn workflow(ctx: &Ctx) -> k::Block {
    let limits = Config::kept(ctx).map(|c| c.limits).unwrap_or_default();
    let ev = working(&ctx.state).unwrap_or_default();
    let counts = crate::diagram::counts(ev.states.values());
    let blocks = crate::diagram::charts(&limits, Some(&counts)).map(|c| {
        let nodes: Vec<k::Node> = (c.nodes.iter())
            .map(|(id, l)| k::Node {
                id: Some(id.clone()),
                limit: Some(l.to_string()),
                ..k::Node::default()
            })
            .collect();
        let n = if c.id == "states" {
            counts.iter().map(|(k, v)| (k.to_string(), (*v).into())).collect()
        } else {
            BTreeMap::new()
        };
        k::Diagram::new(c.source)
            .id(c.id)
            .counts(n)
            .nodes(k::Nodes::List(nodes))
            .block()
    });
    k::Screen::new()
        .id("workflow")
        .title("Workflow")
        .blocks(blocks.to_vec())
        .block()
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

const GROUPS: [&str; 4] = ["hour", "today", "yesterday", "earlier"];

fn when(now: u64, t: u64) -> (&'static str, &'static str) {
    match now.saturating_sub(t) {
        a if a < 3_600 => ("hour", "last hour"),
        _ if t / 86_400 == now / 86_400 => ("today", "today"),
        _ if t / 86_400 + 1 == now / 86_400 => ("yesterday", "yesterday"),
        _ => ("earlier", "earlier"),
    }
}

fn groups(now: u64, q: &Q, rows: Vec<(u64, k::Row)>, more: Option<String>) -> Vec<k::Block> {
    let mut out: Vec<(&str, &str, Vec<k::Block>)> = vec![];
    for (t, r) in rows {
        let (id, header) = (q.get("in"))
            .and_then(|g| GROUPS.iter().find(|x| **x == g.as_str()))
            .map_or_else(|| when(now, t), |g| (*g, ""));
        match out.iter_mut().find(|g| g.0 == id) {
            Some(g) => g.2.push(r.block()),
            None => out.push((id, header, vec![r.block()])),
        }
    }
    out.sort_by_key(|g| GROUPS.iter().position(|x| *x == g.0));
    let end = out.len().saturating_sub(1);
    let more = more.map(|m| match out.last().filter(|_| !q.contains_key("in")) {
        Some(g) => format!("{m}&in={}", g.0),
        None => m,
    });
    (out.into_iter().enumerate())
        .map(|(i, (id, header, rows))| {
            let mut list = k::List::new(rows).id(format!("ev-{id}-list"));
            list.next = more.clone().filter(|_| i == end).map(k::More::Link);
            section(&format!("ev-{id}"), header, vec![list.block()])
        })
        .collect()
}

fn events_screen(ctx: &Ctx, q: &Q) -> k::Block {
    let before = q.get("before").and_then(|b| b.parse().ok()).unwrap_or(ctx.now);
    let picked: Vec<&str> = q.get("kind").map_or("all", String::as_str).split(',').collect();
    let all = picked.contains(&"all") || picked.iter().all(|k| !KINDS.iter().any(|(v, _)| v == k));
    let imp = picked.contains(&"imp");
    let cfg = Config::kept(ctx).ok();
    let serial = |e: &Event| (cfg.as_ref().and_then(|c| c.fleet.get(&e.fleet))).map_or("serial", |f| f.serial_label());
    let kinds = ["merged", "verdict", "needs-human", "spawn", "needs-decision"];
    let window = crate::events::since(&ctx.state, &kinds, before.saturating_sub(86_400)).unwrap_or_default();
    let keep = |e: &Event| {
        e.ts < before
            && kind_of(&e.kind).is_some_and(|k| all || picked.contains(&k))
            && (!imp || super::words(e, serial(e)).3)
    };
    let rows = window.into_iter().filter(keep).map(|e| ((e.seq, 0), e));
    let (shown, more) = pages::page("/screens/events", q, rows);
    let repos: BTreeMap<String, String> = (cfg.iter().flat_map(|c| &c.fleet))
        .map(|(f, fc)| (f.clone(), fc.repo.clone()))
        .collect();
    let list = super::v1::feed(ctx, &shown, usize::MAX).list;
    let (mut rows, mut asks) = (vec![], None);
    for e in &list {
        let k = kind_of(&e.kind).unwrap_or_default();
        let role = e.data.get("role").and_then(Value::as_str).unwrap_or_default();
        let mut fig = match (k, e.tone.as_str()) {
            ("lane", _) => figure(super::act(role, "working"), ""),
            (_, "g") => figure("done", ""),
            (_, "r") => figure("blocked", "dead"),
            (_, "a") => figure("blocked", ""),
            _ => figure("idle", ""),
        };
        if e.kind == "verdict" && e.tone != "r" {
            fig.act = Some("rev".into());
        }
        let moment = match e.kind.as_str() {
            "merged" => Some("merged"),
            "verdict" if e.tone == "r" => Some("fail"),
            "needs-human" | "needs-decision" => Some("needs-you"),
            _ => None,
        };
        fig.moment = moment.map(Into::into);
        let mut r = titled(format!("e-{}", e.seq), e.text.as_str())
            .value(since(e.ts))
            .figure(fig);
        if let Some(t) = e.issue.and_then(|n| crate::gh::cached_title(ctx, &e.fleet, n)) {
            r = r.subtitle(plain(t));
        }
        let s = |k: &str| e.data.get(k).and_then(Value::as_str).filter(|s| !s.is_empty());
        let repo = repos.get(&e.fleet);
        r.tap = match (s("agent").filter(|_| k == "lane").or(s("thread")), e.pr.zip(repo)) {
            (Some(t), _) => Some(thread(t)),
            (_, Some((pr, repo))) => Some(url(format!("https://github.com/{repo}/pull/{pr}"))),
            _ => None,
        };
        let log =
            (e.kind == "needs-decision").then(|| asks.get_or_insert_with(|| working(&ctx.state).unwrap_or_default()));
        if let Some((log, i)) = log.and_then(|l| Some((&**l, l.binary_search_by_key(&e.seq, |x| x.seq).ok()?))) {
            match log.get(i).and_then(|x| ask_outcome(log, x)) {
                Some(o) => r = r.subtitle(plain(o)),
                None => r = r.tap(sheet(format!("/screens/ask?seq={}", e.seq))),
            }
        }
        rows.push((e.ts, r));
    }
    let mut sel: Vec<&str> = if all { vec!["all"] } else { picked.clone() };
    if all && imp {
        sel.push("imp");
    }
    let mut opts = vec![chip("all", "all")];
    opts.extend(KINDS.iter().map(|(v, l)| chip(v, *l)));
    opts.push(chip("imp", "important"));
    let kq = if all && !imp {
        String::new()
    } else {
        format!("kind={}&", sel.join(","))
    };
    let stream = if before == ctx.now {
        format!("/screens/events/stream?{kq}")
    } else {
        String::new()
    };
    let chips = k::Chips::new("kind", opts)
        .id("kind")
        .multi(true)
        .selected(sel.iter().map(|s| (*s).to_string()).collect::<Vec<_>>())
        .action("filter");
    let mut blocks = vec![chips.block()];
    blocks.extend(groups(ctx.now, q, rows, more));
    let older = titled("older", "earlier days")
        .style(k::RowStyle::Link)
        .tap(open(format!(
            "/screens/events?{kq}before={}",
            before.saturating_sub(86_400).div_ceil(3_600) * 3_600
        )));
    blocks.push(rows_list("older-list", vec![older.block()]));
    let mut s = k::Screen::new().id("events").title("Events").blocks(blocks);
    if !stream.is_empty() {
        s = s.stream(stream.trim_end_matches(['?', '&']));
    }
    s.block()
}

const DRY: [&str; 2] = ["kill", "clean"];

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
            .chain(["/screens/cards".to_string(), "/screens/scene".to_string()])
            .collect();
        let mut out = vec![];
        let mut i = 0;
        while let Some(p) = paths.get(i).cloned() {
            i += 1;
            if let Some(v) = self.build_screen(ctx, &p, &Q::new(), paired) {
                let mut more = vec![];
                sheets(&v, &mut more);
                more.retain(|m| !paths.contains(m));
                paths.extend(more);
                out.push((format!("{}/index.html", &p[1..]), v.to_string().into_bytes()));
            }
        }
        out
    }

    pub(super) fn build_screen(&self, ctx: &Ctx, path: &str, q: &Q, paired: bool) -> Option<Value> {
        let mut v = match path {
            "/screens/queue" => self.queue_screen(ctx, q),
            "/screens/lanes" => lanes_screen(&self.lanes_tab(ctx, paired)),
            p if paired && p.starts_with("/screens/lanes/") => self.lane_sheet(ctx, &p[15..])?,
            "/screens/events" => events_screen(ctx, q),
            "/screens/fleets" => return Some(fleets::sheet(&Config::kept(ctx).ok()?)),
            "/screens/notify" => return Some(fleets::notify(&Config::kept(ctx).ok()?)),
            "/screens/cards" => return Some(self.chat_cards(ctx, paired)),
            "/screens/workflow" => workflow(ctx),
            "/screens/drive-health" => health_sheet(ctx.now, &working(&ctx.state).ok()?),
            "/screens/scene" => scene_screen(&self.herd(ctx, None).ok()?),
            "/screens/hot" => self.on_row(ctx, q, |c, r| {
                let f = &c.name;
                let rows: Vec<k::Block> = (hot(r).iter().enumerate())
                    .map(|(i, p)| titled(format!("hot-{i}"), *p).block())
                    .collect();
                let title = format!("#{} · 🔒 hot ×{}", r.issue, rows.len());
                k::Modal::new(title, vec![rows_list("hot-paths", rows)])
                    .id(format!("hot-{f}-{}", r.issue))
                    .cancel("Done")
                    .block()
            })?,
            "/screens/issue" => {
                let ev = working(&ctx.state).ok()?;
                self.on_row(ctx, q, |c, r| issue_sheet(ctx, &ev, c, r))?
            }
            "/screens/verdict" => verdict_sheet(&working(&ctx.state).ok()?, q)?,
            "/screens/backlog" => {
                let tab = self.queue_tab(ctx);
                backlog(ctx.now, tab.fleets.iter().find(|c| Some(&c.name) == q.get("repo"))?, q)?
            }
            "/screens/queue/done" => done_screen(
                self.queue_tab(ctx)
                    .fleets
                    .iter()
                    .find(|c| Some(&c.name) == q.get("repo"))?,
                q,
            ),
            "/screens/ask" => ask_sheet(ctx, q)?,
            "/screens/hosts" => return self.hosts_sheet(ctx),
            p if p == "/screens/threads" || p.starts_with("/screens/threads/") => {
                return self.threads_screen(ctx, &p[16..], q.get("before"), paired)
            }
            _ => return None,
        };
        let snap = Some(self.snapshot().1.ts).filter(|t| *t > 0);
        let age = snap.map(|t| ctx.now.saturating_sub(t));
        if let Some(b) = match &mut v {
            k::Block::Screen(s) => s.blocks.as_mut(),
            _ => None,
        } {
            let line = snap.map_or(Shown::from("as of: not yet known"), |t| {
                ticking(t, "as of {} ago").into()
            });
            let tone = if age.is_some_and(|a| a <= 600) {
                Tone::Mute
            } else {
                Tone::Warn
            };
            let stat = b.iter_mut().find_map(|b| match b {
                k::Block::Stat(st) => Some(st),
                _ => None,
            });
            if let Some(st) = stat {
                if let Some(i) = st.items.as_mut() {
                    i.push(Tile {
                        tone: Some(tone),
                        ..tile("snapshot", line)
                    });
                }
            } else {
                let row = titled("age", "Snapshot").subtitle(toned(line, tone));
                b.insert(
                    0,
                    section("age", "snapshot", vec![rows_list("age-list", vec![row.block()])]),
                );
            }
        }
        Some(wire(&v))
    }

    fn lane_sheet(&self, ctx: &Ctx, agent: &str) -> Option<k::Block> {
        let (cfg, ev) = (Config::kept(ctx).ok()?, working(&ctx.state).ok()?);
        let list = self.recent_sessions(ctx.now, &cfg)?;
        let all = crate::snapshot::lanes(ctx, &cfg, (&ev, &ev.states), &list);
        let l = all.into_iter().find(|l| l.name == agent)?;
        let spawn = crate::events::last_spawn(&ev, agent);
        let fc = l.fleet.as_ref().and_then(|f| cfg.fleet.get(f));
        let pr = l.issue.zip(l.fleet.as_ref()).and_then(|(n, f)| {
            let s = ev.states.get(&(f.clone(), n));
            super::pr_data(ctx, fc, f, n, s).pr.zip(fc)
        });
        let issue = l.issue.zip(l.fleet.as_ref()).map(|(n, f)| format!("{f} #{n}"));
        let fact = |k: &str, v: Option<String>| v.filter(|v| !v.is_empty()).map(|v| (k.to_string(), v));
        let mut facts: Vec<k::Block> = [
            fact("issue", issue),
            fact("role", Some(l.role.clone())),
            fact("state", Some(l.state.word())),
            fact("host", Some(l.at())),
            fact("branch", spawn.and_then(|e| e.str("branch")).map(String::from)),
            fact("port", l.port.clone()),
        ]
        .into_iter()
        .flatten()
        .map(|(key, v)| titled(format!("f-{key}"), key.as_str()).value(v).block())
        .collect();
        if let Some((n, fc)) = pr {
            let to = format!("https://github.com/{}/pull/{n}", fc.repo);
            facts.push(titled("f-pr", "PR").value(format!("#{n}")).tap(url(to)).block());
        }
        let mut said: Vec<k::Block> = self.tails.sheet(&l).into_iter().map(typed).collect();
        said.extend(later::undelivered(&ev, agent).map(typed));
        let values = fields([("agent", agent.into())]);
        let other = l.role == "other";
        let acts: &[(&str, &str)] = if other {
            &[("close", "Close pane")]
        } else {
            &[("stop", "Stop"), ("kill", "Kill"), ("clean", "Clean")]
        };
        let mut blocks = vec![section("facts", "lane", vec![rows_list("facts-list", facts)])];
        blocks.extend(acts.iter().map(|(a, label)| {
            let mut b = button(format!("act-{a}"), *label, a, values.clone()).style(k::ButtonStyle::Destructive);
            if !DRY.contains(a) {
                b = b.confirm(confirm(format!("{label} {agent}?"), *label));
            }
            b.block()
        }));
        let mut v = k::Modal::new(agent, vec![]).id(format!("lane-{agent}")).cancel("Close");
        if !other {
            let field = k::TextField::new("text")
                .label("Say")
                .value(k::FieldValue::Text(String::new()))
                .placeholder("a line to the lane");
            blocks.push(k::Form::new(vec![field.into()]).id("say").block());
            v = v.done(k::Submit {
                label: "Send".into(),
                action: "send".into(),
                values: Some(values),
                ..k::Submit::default()
            });
        }
        blocks.push(section("transcript", "transcript", said));
        v.blocks = blocks;
        Some(v.block())
    }

    fn dry_run(&self, ctx: &Ctx, op: &str, agent: &str) -> Reply {
        let act = crate::api::Act {
            agent: agent.into(),
            op: op.into(),
            confirm: String::new(),
        };
        let label = if op == "kill" { "Kill" } else { "Clean" };
        let (said, ok) = match self.act(ctx, &cooee(), &act) {
            Ok(l) => (
                l.split("dry run:")
                    .next()
                    .unwrap_or_default()
                    .trim_end_matches("; ")
                    .into(),
                true,
            ),
            Err(e) => (shown(&e), false),
        };
        let mut blocks = vec![text("dry-run", said.as_str())];
        if ok {
            let sure = Confirm {
                message: Some(said),
                ..confirm(format!("{label} {agent}?"), label)
            };
            let values = fields([("agent", agent.into()), ("confirm", "1".into())]);
            blocks.push(
                button(format!("act-{op}"), label, op, values)
                    .style(k::ButtonStyle::Destructive)
                    .confirm(sure)
                    .block(),
            );
        }
        let sheet = k::Modal::new(format!("{label} {agent}?"), blocks)
            .id(format!("{op}-{agent}"))
            .cancel("Back");
        Reply {
            ops: Some(vec![k::Op::Screen { screen: sheet.block() }]),
            toast: None,
            close: Some(false),
        }
    }

    fn on_row(&self, ctx: &Ctx, q: &Q, draw: impl Fn(&QueueCard, &QueueRow) -> k::Block) -> Option<k::Block> {
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

    fn queue_screen(&self, ctx: &Ctx, q: &Q) -> k::Block {
        let (tab, ev) = (self.queue_tab(ctx), working(&ctx.state).unwrap_or_default());
        let lanes = Config::kept(ctx).map_or(vec![], |cfg| self.lane_list(ctx, &cfg, &ev, &ev.states));
        let late = Config::kept(ctx).ok().and_then(|c| crate::timer::unfinished(ctx, &c));
        let sel = q
            .get("repo")
            .filter(|r| tab.fleets.iter().any(|f| &f.name == *r))
            .map(String::as_str);
        let all: usize = tab.fleets.iter().map(|c| c.rows.len()).sum();
        let mut opts = vec![chip("all", format!("all {all}"))];
        let dot = |c: &QueueCard| format!("{} {}{}", c.name, c.rows.len(), if c.need > 0 { " ●" } else { "" });
        opts.extend(tab.fleets.iter().map(|f| chip(&f.name, dot(f))));
        let mut shown: Vec<&QueueCard> = tab.fleets.iter().filter(|f| sel.is_none_or(|s| f.name == s)).collect();
        shown.sort_by_key(|c| c.need == 0);
        let chips = k::Chips::new("repo", opts)
            .id("repo")
            .selected(vec![sel.unwrap_or("all").to_string()])
            .action("filter");
        let mut blocks = vec![chips.block(), stats(ctx, &ev, &ev.states, sel, &lanes), counts(&shown)];
        if let Some(e) = &tab.error {
            blocks.push(text("error", e.as_str()));
        }
        blocks.extend(shown.iter().map(|c| fleet_card(ctx, &ev, c, q, &lanes, late.as_ref())));
        let mut kept = q.clone();
        kept.retain(|k, _| k != "repo" || sel.is_some());
        let stream = view(&kept, "", "").replacen("/screens/queue", "/screens/queue/stream", 1);
        let menu = vec![
            Act {
                label: "Fleets…".into(),
                tap: Some(sheet("/screens/fleets")),
                ..Act::default()
            },
            Act {
                label: "Drive all now".into(),
                action: Some("drive-all".into()),
                ..Act::default()
            },
        ];
        let more = Act {
            label: "More".into(),
            icon: Some("more".into()),
            menu: Some(menu),
            ..Act::default()
        };
        let badge = self.needs_count(ctx, &ev);
        k::Screen::new()
            .id("queue")
            .title("Queue")
            .stream(stream)
            .badge(u64::try_from(badge).unwrap_or(u64::MAX))
            .actions(vec![more])
            .blocks(blocks)
            .block()
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

    pub(super) fn screens(&self, ctx: &Ctx, head: &str, (method, p): (&str, &str), body: &str, auth: &Auth) -> Res {
        let plugin = Self::plugin(ctx, head);
        if !plugin && !self.allowed(ctx, head) {
            return line("403 Forbidden", "foreign Host or Origin");
        }
        match (method, p) {
            ("POST", "/action") if self.addr.is_none() => line("405 Method Not Allowed", "read-only: GET only"),
            ("POST", "/action") if !plugin && auth.is_err() => line("401 Unauthorized", "pair this device first"),
            ("POST", "/action") => {
                let Ok(a) = serde_json::from_str::<Value>(body) else {
                    return line("400 Bad Request", "bad request body");
                };
                let who = match auth {
                    Ok(d) if !plugin => local::Who::Device(&d.label),
                    _ => local::Who::Plugin,
                };
                match self.perform(ctx, &who, &a) {
                    Ok(v) => out("200 OK", &v),
                    Err(e) => match e.downcast_ref::<Refusal>().map(|r| r.code) {
                        Some("NOT_PERMITTED") => line("403 Forbidden", &shown(&e)),
                        Some(_) => line("409 Conflict", &shown(&e)),
                        None => line("400 Bad Request", &shown(&e)),
                    },
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

    fn paired_screen(&self, ctx: &Ctx, path: &str, q: &Q) -> Option<Value> {
        self.build_screen(ctx, path, q, true)
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
                let s = self.paired_screen(ctx, screen, &form(&q)).context("no such screen")?;
                return Ok(Reply::screen(s, None));
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
                    let s = self.paired_screen(ctx, "/screens/backlog", &q).context("no backlog")?;
                    return Ok(Reply::screen(s, Some("no issue ticked: searched".into())));
                };
                let mut d = crate::queue::op("add", &ns, false, None, None, None);
                d.put("check", Value::Bool(true));
                crate::queue_change(ctx, &cfg, f, d, Some("cooee"))? + " (needs-check)"
            }
            "limit" => crate::limit::wait(ctx, &self.r, (&fleet, n()?), "owner")?,
            n if DRY.contains(&n) && s("confirm") != "1" => return Ok(wire(&self.dry_run(ctx, n, &s("agent")))),
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
                let keys = [
                    "text", "repo", "name", "act", "data", "q", "harness", "model", "effort", "req",
                ];
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
                    return Ok(Reply::screen(s, Some(line)));
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
        let ops = later::gone_ops(name, block, &s("fleet"), &s("issue"));
        Ok(Reply::done(ops, toast, close))
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

#[cfg(test)]
#[path = "herd_tests.rs"]
mod tests;
