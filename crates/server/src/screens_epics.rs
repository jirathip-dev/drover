use super::{
    label, link, open, picked, plain, row, rows_list, section, sheet, since, state, titled, view, Ctx, Event, Figure,
    Kind, Lane, QueueCard, Tone, Q,
};
use crate::epic::{Epic, Member};
use blocks as k;

fn head(f: &str, e: &Epic) -> k::Row {
    let bar = "▰".repeat(e.merged()) + &"▱".repeat(e.total().saturating_sub(e.merged()));
    let mut sub = vec![bar];
    sub.extend(e.counts());
    let title = format!("#{} {}", e.parent, e.title);
    titled(format!("epic-{f}-{}", e.parent), title.trim_end())
        .badge(label(format!("{}/{} merged", e.merged(), e.total()), Tone::Done))
        .subtitle(plain(sub.join(" · ")))
        .value(since(e.since))
        .tap(sheet(format!("/screens/issue?repo={f}&issue={}", e.parent)))
}

fn kid(ctx: &Ctx, ev: &[Event], c: &QueueCard, lanes: &[Lane], x: &Member) -> k::Row {
    let (f, n) = (c.name.as_str(), x.issue());
    let id = format!("ep-{f}-{n}");
    let title = format!("#{n} {}", crate::gh::cached_title(ctx, f, n).unwrap_or_default());
    let plain_row = || titled(id.clone(), title.trim_end());
    let v = match x {
        Member::Merged { pr, t, .. } => plain_row()
            .figure(Figure {
                issue: Some(n),
                ..state(Tone::Done)
            })
            .subtitle(plain(pr.map_or("merged".into(), |p| format!("merged · PR #{p}"))))
            .value(since(*t)),
        Member::Dropped { .. } => plain_row()
            .badge(label("dropped", Tone::Mute))
            .subtitle(plain("closed unmerged · not counted")),
        Member::Open { state, .. } => match c.rows.iter().find(|r| r.issue == n) {
            Some(r) => row(
                ctx,
                ev,
                c.paused.is_some(),
                f,
                r,
                None,
                lanes.iter().find(|l| l.name == r.lane),
            )
            .id(id),
            None => plain_row().subtitle(plain(state)),
        },
    };
    v.indent(1).tap(sheet(format!("/screens/issue?repo={f}&issue={n}")))
}

pub(super) fn epics(ctx: &Ctx, ev: &[Event], c: &QueueCard, q: &Q, lanes: &[Lane]) -> Vec<k::Block> {
    let f = &c.name;
    let mut rows = vec![];
    for e in &c.epics {
        rows.push(head(f, e).block());
        let key = format!("{f}-{}", e.parent);
        let folded = picked(q, "fold").contains(&key.as_str());
        if !folded {
            rows.extend(e.children.iter().map(|x| kid(ctx, ev, c, lanes, x).block()));
        }
        let title = if folded {
            format!("▸ {} children of #{}", e.children.len(), e.parent)
        } else {
            format!("▾ fold #{}'s children", e.parent)
        };
        rows.push(link(&format!("epic-fold-{key}"), &title, open(view(q, "fold", &key))).block());
    }
    let list = rows_list(&format!("epics-list-{f}"), rows);
    let header = format!("epics ({})", c.epics.len());
    (!c.epics.is_empty())
        .then(|| section(&format!("epics-{f}"), &header, vec![list]))
        .into_iter()
        .collect()
}
