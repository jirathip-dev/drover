use super::{
    devices, hosts, k, plain, rows_list, section, sheet, state, text, titled, wire, word, Block, Figure, Herd,
    HerdFleet, HerdLane, Kind, LaneRow, LanesTab, Tone, Value,
};

const COATS: [&str; 5] = ["bay", "blk", "ch", "gry", "wht"];

fn lane_figure(name: &str, state: &str, act: &str) -> Figure {
    let i = usize::from_str_radix(&devices::sha256(name.as_bytes())[..2], 16).unwrap_or(0) % COATS.len();
    let state = match state {
        "working" => Tone::Working,
        "blocked" => Tone::Needs,
        "done" => Tone::Done,
        "dead" => Tone::Fail,
        _ => Tone::Idle,
    };
    Figure {
        coat: COATS.get(i).map(|c| (*c).into()),
        state: Some(state),
        act: Some(act.into()),
        ..Figure::default()
    }
}

fn lane_row(fleet: &str, l: &LaneRow) -> k::Block {
    let title = l.issue.map_or(l.name.clone(), |n| format!("{fleet} #{n}"));
    let now = l.now.as_ref().map(|n| n.title.clone());
    let now = now.or_else(|| l.why.clone()).unwrap_or_else(|| l.activity.clone());
    let sub: Vec<&str> = [l.role.as_str(), l.host.as_str(), now.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    let mut fig = lane_figure(&l.name, &l.state, &l.act);
    if l.issue.is_some() {
        fig.grow = Some(l.grow.clone());
        fig.issue = l.issue;
    }
    let row = titled(format!("l-{}", l.name), title)
        .subtitle(plain(sub.join(" · ")))
        .value(l.ago.as_str())
        .tap(sheet(format!("/screens/lanes/{}", l.name)));
    row.figure(fig).block()
}

const BANDS: [(&str, &[&str]); 4] = [
    ("working", &["working"]),
    ("idle", &["idle", "unknown", "parked"]),
    ("done", &["done"]),
    ("blocked", &["blocked", "dead"]),
];

fn band(id: &str, name: &str, rows: Vec<k::SceneRow>) -> Option<k::Band> {
    let header = if name == "blocked" { "blocked/dead" } else { name };
    (!rows.is_empty()).then(|| k::Band {
        kind: "section".into(),
        id: format!("h-{id}-{name}"),
        header: Some(header.into()),
        rows: Some(rows),
        blocks: None,
    })
}

fn scene_repo(name: &str, color: &str, lanes: &[HerdLane], f: Option<&HerdFleet>) -> Option<k::SceneRepo> {
    let needs = f.map(|f| f.needs.as_slice()).unwrap_or_default();
    let lanes: Vec<&HerdLane> = lanes.iter().filter(|l| !l.gone).collect();
    let mut sum = vec![];
    let gate: Vec<k::SceneRow> = (f.map(|f| f.queue.as_slice()).unwrap_or_default().iter())
        .filter(|q| !lanes.iter().any(|l| l.issue == Some(q.issue)))
        .map(|q| {
            let need = needs.contains(&q.issue);
            let blocked = q.blocked_by.map(|b| format!("blocked by #{b}"));
            let sub = need
                .then(|| "needs you".to_string())
                .or(blocked)
                .or_else(|| q.title.clone());
            k::SceneRow {
                id: format!("h-{name}-q{}", q.issue),
                figure: Some(state(if need {
                    Tone::Needs
                } else {
                    Tone::Other("queued".into())
                })),
                title: format!("#{}", q.issue),
                subtitle: Some(sub.unwrap_or_default()),
                blocked_by: Some(q.blocked_by),
                ..k::SceneRow::default()
            }
        })
        .collect();
    sum.push((gate.len(), "queued"));
    let mut blocks: Vec<k::Band> = band(name, "queued", gate).into_iter().collect();
    for (b, states) in BANDS {
        let rows: Vec<k::SceneRow> = (lanes.iter())
            .filter(|l| {
                let need = l.issue.is_some_and(|i| needs.contains(&i));
                if need {
                    b == "blocked"
                } else {
                    states.contains(&l.state.as_str())
                }
            })
            .map(|l| {
                let need = l.issue.filter(|i| needs.contains(i));
                let (state, act, tone, shown) = match need {
                    Some(_) => ("blocked", "blocked", "r", "needs you"),
                    None => (l.state.as_str(), l.act.as_str(), l.tone.as_str(), l.shown.as_str()),
                };
                k::SceneRow {
                    id: format!("h-{}", l.name),
                    figure: Some(Figure {
                        state: Some(word(state)),
                        act: Some(act.into()),
                        coat: Some(l.coat.clone()),
                        ..Figure::default()
                    }),
                    title: l.name.clone(),
                    subtitle: Some(shown.into()),
                    tone: Some(word(tone)),
                    value: Some(l.age.clone()),
                    host: Some(l.host.clone()),
                    needs: Some(need),
                    tap: Some(sheet(format!("/screens/lanes/{}", l.name))),
                    ..k::SceneRow::default()
                }
            })
            .collect();
        sum.push((rows.len(), b));
        blocks.extend(band(name, b, rows));
    }
    if let Some(t) = sum.get_mut(1..) {
        t.rotate_right(1);
    }
    let sum: Vec<String> = sum
        .iter()
        .filter(|s| s.0 > 0)
        .map(|(n, w)| format!("{n} {w}"))
        .collect();
    (f.is_some() || !lanes.is_empty()).then(|| k::SceneRepo {
        id: name.into(),
        title: name.into(),
        color: Some(color.into()),
        subtitle: Some(sum.join("·")),
        blocks,
    })
}

pub(super) fn scene_screen(h: &Herd) -> k::Block {
    let mut repos: Vec<k::SceneRepo> = (h.fleets.iter())
        .filter_map(|f| scene_repo(&f.name, &f.color, &f.lanes, Some(f)))
        .collect();
    repos.extend(scene_repo("others", "", &h.others, None));
    let host = &h.host;
    let ratio = host.load.unwrap_or_default() / host.limit.max(0.01);
    let fog = match host.pressure.as_deref() {
        Some("warn") => 1,
        Some("critical") => 2,
        _ => 0,
    };
    let (sky, level) = hosts::weather(host);
    let load = host.load.map_or("?".into(), |l| format!("{l:.0}%"));
    let swap = host.swapouts.map_or("?".into(), |s| format!("{s:.0}"));
    let line = format!(
        "cpu {load} busy · mem {} · swap {swap}",
        host.pressure.as_deref().unwrap_or("?")
    );
    let weather = k::Weather {
        sky: word(sky),
        wind: serde_json::Number::from_f64((ratio.clamp(0.0, 1.0) * 100.0).round() / 100.0),
        fog: Some(fog),
        storm: Some(host.gate.is_some()),
        line: Some(line),
    };
    let scene = k::Scene::new()
        .id("ranch")
        .load(word::<k::Load>(level))
        .weather(weather)
        .repos(repos);
    k::Screen::new()
        .id("scene")
        .title("Scene")
        .stream("/screens/scene/stream")
        .blocks(vec![scene.block()])
        .block()
}

fn section_word(state: &str) -> &str {
    match state {
        "blocked" => "waiting",
        "idle" => "asleep",
        s => s,
    }
}

pub(super) fn lanes_screen(t: &LanesTab) -> k::Block {
    let mut blocks: Vec<k::Block> = t
        .sections
        .iter()
        .map(|sec| {
            let rows: Vec<k::Block> = sec
                .repos
                .iter()
                .flat_map(|r| r.lanes.iter().map(move |l| lane_row(&r.name, l)))
                .collect();
            let header = format!("{} ({})", section_word(&sec.name), rows.len());
            section(
                &format!("lanes-{}", sec.name),
                &header,
                vec![rows_list(&format!("lanes-list-{}", sec.name), rows)],
            )
        })
        .collect();
    if let Some(e) = t.error.as_deref().or(t.failed.then_some("the lane list failed")) {
        blocks.insert(0, text("error", e));
    }
    let others: Vec<k::Block> = t
        .shed
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let name = o.name.clone().unwrap_or_else(|| o.host.clone());
            let what = [Some(&o.host), o.state.as_ref(), o.last.as_ref(), o.error.as_ref()];
            let sub: Vec<&str> = what.into_iter().flatten().map(String::as_str).collect();
            titled(format!("o-{i}"), name)
                .figure(state(Tone::Idle))
                .subtitle(plain(sub.join(" · ")))
                .value(o.age.clone().unwrap_or_default())
                .block()
        })
        .collect();
    if !others.is_empty() {
        let header = format!("other in shed ({})", others.len());
        blocks.push(section("others", &header, vec![rows_list("others-list", others)]));
    }
    k::Screen::new()
        .id("lanes")
        .title("Lanes")
        .stream("/screens/lanes/stream")
        .blocks(blocks)
        .block()
}

pub(in crate::serve) fn said(id: &str, b: &Block, lines: &[String]) -> Value {
    let text = |m: String| text(id, m);
    let said = match b {
        Block::Prompt { text: t } => text(format!("**›** {t}")),
        Block::Reply { markdown } => text(markdown.clone()),
        Block::Tool { title_line, status, .. } => {
            let t = match status.as_str() {
                "completed" => Tone::Done,
                "failed" => Tone::Fail,
                "pending" => Tone::Idle,
                _ => Tone::Working,
            };
            titled(id, title_line.as_str())
                .figure(state(t))
                .subtitle(plain(status.as_str()))
                .block()
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
    };
    wire(&said)
}
