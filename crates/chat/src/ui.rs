use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

pub(crate) type Seg = (String, Style);
pub(crate) type Lines = Vec<Line<'static>>;

pub(crate) fn st(c: Color) -> Style {
    Style::default().fg(c)
}

pub(crate) const AMBER: Color = Color::Yellow;
pub(crate) const GREY: Color = Color::DarkGray;
pub(crate) const RED: Color = Color::Red;
pub(crate) const BLUE: Color = Color::LightBlue;
pub(crate) const CODE: Color = Color::Cyan;
pub(crate) const BAR: Color = Color::Indexed(236);

pub(crate) fn seg(t: impl Into<String>, s: Style) -> Seg {
    (t.into(), s)
}

pub(crate) fn line(pad: usize, cells: &[(char, Style)]) -> Line<'static> {
    let mut spans = vec![Span::raw(" ".repeat(pad))];
    for c in cells {
        match spans.last_mut() {
            Some(last) if last.style == c.1 => last.content.to_mut().push(c.0),
            _ => spans.push(Span::styled(c.0.to_string(), c.1)),
        }
    }
    Line::from(spans)
}

pub(crate) fn wrap(segs: &[Seg], w: usize, indent: usize) -> Lines {
    let cells: Vec<(char, Style)> = segs.iter().flat_map(|(t, s)| t.chars().map(move |c| (c, *s))).collect();
    let w = w.max(1);
    let indent = if indent + 8 > w { 0 } else { indent };
    let mut out = vec![];
    for para in cells.split(|c| c.0 == '\n') {
        let (mut rest, mut pad) = (para, 0);
        loop {
            let room = w - pad;
            if rest.len() <= room {
                out.push(line(pad, rest));
                break;
            }
            let space = rest.iter().take(room + 1).rposition(|c| c.0 == ' ').filter(|i| *i > 0);
            let (head, tail) = rest.split_at(space.unwrap_or(room));
            out.push(line(pad, head));
            rest = if space.is_some() {
                tail.get(1..).unwrap_or_default()
            } else {
                tail
            };
            pad = indent;
        }
    }
    out
}

pub(crate) fn boxed(rows: &[Vec<Seg>], w: usize, edge: Style, title: Option<Seg>) -> Lines {
    if w < 8 {
        return rows.iter().flat_map(|r| wrap(r, w, 2)).collect();
    }
    let t = title.as_ref().map_or(0, |t| t.0.chars().count() + 2);
    let mut top = vec![Span::styled("┌", edge)];
    top.extend(title.map(|(t, s)| Span::styled(format!(" {t} "), s)));
    top.push(Span::styled(format!("{}┐", "─".repeat(w.saturating_sub(t + 2))), edge));
    let mut out = vec![Line::from(top)];
    for r in rows.iter().flat_map(|r| wrap(r, w - 4, 2)) {
        let fill = " ".repeat((w - 4).saturating_sub(r.width()));
        let mut spans = vec![Span::styled("│ ", edge)];
        spans.extend(r.spans);
        spans.push(Span::styled(format!("{fill} │"), edge));
        out.push(Line::from(spans));
    }
    out.push(Line::styled(format!("└{}┘", "─".repeat(w - 2)), edge));
    out
}

pub(crate) fn ends(mut left: Lines, right: Vec<Span<'static>>, w: usize) -> Lines {
    let rw = Line::from(right.clone()).width();
    let gap = match left.as_slice() {
        [l] => w.checked_sub(l.width() + rw),
        _ => None,
    };
    let fill = Span::raw(" ".repeat(gap.unwrap_or_else(|| w.saturating_sub(rw))));
    match (gap, left.last_mut()) {
        (Some(_), Some(l)) => l.spans.extend([vec![fill], right].concat()),
        _ => left.push(Line::from([vec![fill], right].concat())),
    }
    left
}

pub(crate) fn put(f: &mut Frame, area: Rect, lines: Lines) {
    f.render_widget(Paragraph::new(lines), area);
}
