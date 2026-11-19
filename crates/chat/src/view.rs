use super::{s, Act, App, Shed};
use drover_core::card::TTL;
use drover_core::events::age;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use serde_json::Value;

type Seg = (String, Style);
type Lines = Vec<Line<'static>>;

const WIDE: u16 = 100;
const LIST: u16 = 40;

const KEYS: [(&str, &str); 16] = [
    ("Ctrl-C", "quit, anywhere"),
    ("Tab", "switch Threads / Board"),
    ("↑ ↓  k j", "list: move"),
    ("↵", "list: open the thread; chat: send the line"),
    ("q", "list: quit"),
    ("?", "this sheet (list, or chat with the box empty)"),
    ("← →", "chat: move the cursor in the box"),
    ("⌫", "delete a character; box empty: back to the list"),
    ("Esc", "box empty: back to the list"),
    ("Ctrl-U", "clear the box"),
    ("↑ ↓", "chat: scroll a line"),
    ("Ctrl-B Ctrl-F", "chat: scroll a page"),
    ("1 … 9", "box empty: select that option of the open ask"),
    ("a / d", "box empty: select approve / deny of the open card"),
    ("↵", "a selection open: confirm it"),
    ("Esc or ⌫", "a selection open: back out; nothing is sent"),
];

fn st(c: Color) -> Style {
    Style::default().fg(c)
}

const AMBER: Color = Color::Yellow;
const PURPLE: Color = Color::Magenta;
const GREY: Color = Color::DarkGray;
const RED: Color = Color::Red;
const BLUE: Color = Color::LightBlue;
const CODE: Color = Color::Cyan;
const BAR: Color = Color::Indexed(236);

fn seg(t: impl Into<String>, s: Style) -> Seg {
    (t.into(), s)
}

fn line(pad: usize, cells: &[(char, Style)]) -> Line<'static> {
    let mut spans = vec![Span::raw(" ".repeat(pad))];
    for c in cells {
        match spans.last_mut() {
            Some(last) if last.style == c.1 => last.content.to_mut().push(c.0),
            _ => spans.push(Span::styled(c.0.to_string(), c.1)),
        }
    }
    Line::from(spans)
}

fn wrap(segs: &[Seg], w: usize, indent: usize) -> Lines {
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

fn boxed(rows: &[Vec<Seg>], w: usize, edge: Style, title: Option<Seg>) -> Lines {
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

fn text(t: &str, w: usize, style: Style) -> Lines {
    let (mut out, mut fence, mut code) = (vec![], false, vec![]);
    for l in t.lines() {
        if l.trim_start().starts_with("```") {
            if fence {
                out.extend(boxed(&std::mem::take(&mut code), w, st(GREY), None));
            }
            fence = !fence;
        } else if fence {
            code.push(vec![seg(l, st(CODE))]);
        } else {
            out.extend(wrap(&[seg(l, style)], w, 0));
        }
    }
    out.extend(
        (!code.is_empty())
            .then(|| boxed(&code, w, st(GREY), None))
            .into_iter()
            .flatten(),
    );
    out
}

fn edged(lines: Lines, c: Color) -> Lines {
    let mark = |mut l: Line<'static>| {
        l.spans.insert(0, Span::styled("┃ ", st(c)));
        l
    };
    lines.into_iter().map(mark).collect()
}

fn clamp(t: &str, n: usize) -> String {
    if t.chars().count() <= n {
        return t.into();
    }
    t.chars().take(n.saturating_sub(1)).chain(['…']).collect()
}

fn ends(mut left: Lines, right: Vec<Span<'static>>, w: usize) -> Lines {
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

fn bar(mut l: Line<'static>, w: usize) -> Line<'static> {
    let fill = " ".repeat(w.saturating_sub(l.width()));
    l.spans.push(Span::raw(fill));
    l.style(Style::default().bg(BAR))
}

fn put(f: &mut Frame, area: Rect, lines: Lines) {
    f.render_widget(Paragraph::new(lines), area);
}

impl App<'_, '_> {
    /// Draw the whole screen: the top line, then the list, the chat, or both side by side from 100 columns.
    pub fn draw(&self, f: &mut Frame) {
        let a = f.area();
        let top = self.top(usize::from(a.width));
        let h = u16::try_from(top.len()).unwrap_or(a.height).min(a.height);
        put(f, a, top);
        let a = Rect {
            y: a.y + h,
            height: a.height - h,
            ..a
        };
        if self.board {
            let t = "The Board tab comes with #1354. Until then: drover board. Tab goes back.";
            return put(f, a, wrap(&[seg(t, st(GREY))], usize::from(a.width), 0));
        }
        if let Some(n) = self.sheet {
            let (w, h) = (usize::from(a.width), usize::from(a.height));
            let hint = self.hint(w);
            let mut keys: Lines = sheet(w)
                .into_iter()
                .skip(n)
                .take(h.saturating_sub(hint.len()))
                .collect();
            keys.resize(h.saturating_sub(hint.len()), Line::default());
            return put(f, a, [keys, hint].concat());
        }
        if a.width < WIDE {
            let lines = match self.open {
                Some(_) => self.chat(usize::from(a.width), usize::from(a.height), true),
                None => self.list(usize::from(a.width), usize::from(a.height), true),
            };
            return put(f, a, lines);
        }
        let chat = Rect {
            x: a.x + LIST + 1,
            width: a.width - LIST - 1,
            ..a
        };
        put(
            f,
            Rect { width: LIST, ..a },
            self.list(usize::from(LIST), usize::from(a.height), self.open.is_none()),
        );
        put(
            f,
            Rect {
                x: a.x + LIST,
                width: 1,
                ..a
            },
            vec![Line::styled("│", st(GREY)); usize::from(a.height)],
        );
        put(
            f,
            chat,
            self.chat(usize::from(chat.width), usize::from(a.height), self.open.is_some()),
        );
        let dim = if self.open.is_some() {
            Rect { width: LIST, ..a }
        } else {
            chat
        };
        f.buffer_mut().set_style(dim, st(GREY));
    }

    fn top(&self, w: usize) -> Lines {
        let tab = |t: &str, on: bool| {
            let s = if on {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                st(GREY)
            };
            Span::styled(format!(" {t} "), s)
        };
        let wide = w >= 80;
        let mut down = vec![];
        if !self.serve.is_empty() {
            down.push(seg(
                if wide {
                    "✗ serve down: the page is off, chat works "
                } else {
                    "✗ serve "
                },
                st(RED),
            ));
        }
        match self.shed {
            Shed::Starting(_) => down.push(seg(
                if wide {
                    "↻ shed down: starting it "
                } else {
                    "↻ shed "
                },
                st(AMBER),
            )),
            Shed::Failed(_) => down.push(seg(
                if wide {
                    "✗ shed down: would not start "
                } else {
                    "✗ shed "
                },
                st(RED),
            )),
            Shed::Up if down.is_empty() && self.known => down.push(seg("✓ ", st(Color::Green))),
            Shed::Up => {}
        }
        let right: Vec<Span> = down.into_iter().map(|(t, s)| Span::styled(t, s)).collect();
        let left = vec![tab("Threads", !self.board), Span::raw(" "), tab("Board", self.board)];
        let mut out = ends(vec![Line::from(left)], right, w);
        match self.shed {
            Shed::Starting(_) => out.extend(wrap(
                &[seg(
                    "↻ shed is down. Starting it with the shed job: messages wait, then go.",
                    st(AMBER),
                )],
                w,
                2,
            )),
            Shed::Failed(cmd) => {
                out.extend(wrap(
                    &[seg(
                        "✗ shed did not start. Run it in a shell, then this clears:",
                        st(RED),
                    )],
                    w,
                    2,
                ));
                out.extend(wrap(&[seg("  $ ", st(AMBER)), seg(cmd, st(CODE))], w, 4));
            }
            Shed::Up => {}
        }
        out
    }

    fn list(&self, w: usize, h: usize, focus: bool) -> Lines {
        let mut out: Lines = vec![];
        let (mut repo, mut at) = ("", 0);
        let now = self.now().now;
        for (i, r) in self.rows.iter().enumerate() {
            if s(r, "repo") != repo {
                repo = s(r, "repo");
                let name = if repo == "all" {
                    "ALL REPOS".into()
                } else {
                    repo.to_uppercase()
                };
                out.extend((!out.is_empty()).then(Line::default));
                out.push(Line::styled(format!(" {name}"), st(GREY).add_modifier(Modifier::BOLD)));
            }
            let on = i == self.sel;
            at = if on { out.len() } else { at };
            let mark = match s(r, "state") {
                "awake" => Span::styled("✻", st(PURPLE)),
                "asleep" => Span::styled("☾", st(GREY)),
                _ => Span::raw(" "),
            };
            let ago = age(now.saturating_sub(r["at"].as_u64().unwrap_or(0)));
            let tail = format!(" {ago} ");
            let head = format!(
                "{}{} {} ",
                if on { "›" } else { " " },
                if r["unread"] == true { "●" } else { " " },
                s(r, "thread")
            );
            let room = w.saturating_sub(head.chars().count() + tail.chars().count() + 2);
            let title = clamp(s(r, "title"), room);
            let fill = " ".repeat(room.saturating_sub(title.chars().count()));
            let bold = if on {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let l1 = Line::from(vec![
                Span::styled(head, st(BLUE)),
                Span::styled(title, bold),
                Span::raw(fill),
                Span::styled(tail, st(GREY)),
                mark,
            ]);
            let l2 = Line::styled(format!("   {}", clamp(s(r, "last"), w.saturating_sub(4))), st(GREY));
            out.extend([l1, l2].map(|l| if on { bar(l, w) } else { l }));
        }
        if self.rows.is_empty() {
            out.push(Line::styled(" no threads: drover thread new <repo> <title>", st(GREY)));
        }
        let hint = if focus { self.hint(w) } else { vec![] };
        let room = h.saturating_sub(hint.len());
        let skip = (at + 3).saturating_sub(room);
        let mut shown: Lines = out.into_iter().skip(skip).take(room).collect();
        shown.resize(room, Line::default());
        shown.extend(hint);
        shown
    }

    fn row(&self) -> Option<&Value> {
        let id = self
            .open
            .as_deref()
            .or_else(|| self.rows.get(self.sel).map(|r| s(r, "thread")))?;
        self.rows.iter().find(|r| s(r, "thread") == id)
    }

    fn chat(&self, w: usize, h: usize, focus: bool) -> Lines {
        let Some(r) = self.row() else { return vec![] };
        let state = if self.open_ask().or(self.open_card()).is_some() {
            Span::styled("! choose ", st(AMBER))
        } else if s(r, "state") == "awake" {
            Span::styled("· awake ", st(GREY))
        } else {
            Span::styled("☾ asleep ", st(GREY))
        };
        let left = format!("‹ {} {}", s(r, "thread"), s(r, "title"));
        let repo = format!("{} · ", s(r, "repo"));
        let right = if left.chars().count() + repo.chars().count() + state.width() < w {
            vec![Span::styled(repo, st(GREY)), state]
        } else {
            vec![state]
        };
        let left = wrap(&[seg(left, Style::default().add_modifier(Modifier::BOLD))], w, 2);
        let head: Lines = ends(left, right, w).into_iter().map(|l| bar(l, w)).collect();
        let mut body: Lines = vec![];
        for i in &self.items {
            body.extend((!body.is_empty()).then(Line::default));
            body.extend(self.item(i, w));
        }
        let mut foot = self.panel(w);
        if self.pick.is_none() && focus {
            foot.extend(self.input_box(w));
        }
        if let Some(e) = &self.error {
            foot.extend(wrap(&[seg(format!("✗ {e}"), st(RED))], w, 2));
        }
        foot.extend(if focus { self.hint(w) } else { vec![] });
        let room = h.saturating_sub(head.len() + foot.len());
        let scroll = self.scroll.get().min(body.len().saturating_sub(room));
        if focus {
            self.scroll.set(scroll);
        }
        let end = body.len() - scroll;
        let mut shown: Lines = body.into_iter().take(end).collect();
        if shown.len() > room {
            let above = shown.len() - room.saturating_sub(1);
            shown = shown.split_off(above);
            shown.extend((room > 0).then(|| Line::styled(format!("  ▲ {above} lines above · ↑ scroll"), st(GREY))));
            shown.rotate_right(usize::from(room > 0));
        }
        while shown.len() < room {
            shown.insert(0, Line::default());
        }
        [head, shown, foot].concat()
    }

    fn item(&self, i: &Value, w: usize) -> Lines {
        let now = self.now().now;
        let ago = format!(" {}", age(now.saturating_sub(i["ts"].as_u64().unwrap_or(0))));
        let iw = w.saturating_sub(2);
        let bold = |c: Color| st(c).add_modifier(Modifier::BOLD);
        if let Some(q) = i["ask"].as_str() {
            let open = i["open"] == true;
            let mut out = wrap(&[seg("orchestrator asks", bold(PURPLE)), seg(ago, st(GREY))], iw, 0);
            out.extend(wrap(
                &[
                    seg(format!("? {q} "), bold(AMBER)),
                    seg(s(i, "question"), Style::default()),
                ],
                iw,
                2,
            ));
            for o in i["options"].as_array().into_iter().flatten() {
                let n = o["n"].as_u64().unwrap_or(0);
                let on = matches!(self.pick.as_ref().map(|p| &p.act), Some(Act::Answer { ask, n: m, .. }) if ask == q && *m == n);
                let rec = if o["n"] == i["recommend"] { " (recommended)" } else { "" };
                let k = match (on, open) {
                    (true, _) => Style::default().add_modifier(Modifier::REVERSED),
                    (_, false) => st(GREY),
                    _ => Style::default(),
                };
                let num = if on || !open { k } else { st(BLUE) };
                out.extend(wrap(
                    &[seg(format!("[{n}] "), num), seg(format!("{}{rec}", s(o, "text")), k)],
                    iw,
                    4,
                ));
            }
            return edged(out, PURPLE);
        }
        if let Some(c) = i["card"].as_object().map(|_| &i["card"]) {
            let open = i["open"] == true;
            let mut out = wrap(
                &[seg("orchestrator posted a run card", bold(PURPLE)), seg(ago, st(GREY))],
                iw,
                0,
            );
            let tag = if open { left(i, now) } else { "settled".into() };
            out.extend(wrap(
                &[
                    seg(format!("$ {} ", s(c, "id")), bold(AMBER)),
                    seg(format!("· {tag}"), st(GREY)),
                ],
                iw,
                0,
            ));
            let rows = [
                vec![seg("$ ", st(AMBER)), seg(s(c, "cmd"), st(CODE))],
                vec![seg("in ", st(GREY)), seg(s(c, "dir"), Style::default())],
                vec![seg("why ", st(GREY)), seg(s(c, "why"), Style::default())],
                vec![seg("touches ", st(GREY)), seg(s(c, "blast"), Style::default())],
            ];
            out.extend(boxed(&rows, iw, st(GREY), None));
            if open {
                let on = |a: bool| matches!(self.pick.as_ref().map(|p| &p.act), Some(Act::Card { approve, .. }) if *approve == a);
                let k = |a: bool, c: Color| {
                    if on(a) {
                        Style::default().add_modifier(Modifier::REVERSED)
                    } else {
                        st(c)
                    }
                };
                out.push(Line::from(vec![
                    Span::styled("[a] approve", k(true, BLUE)),
                    Span::raw("  "),
                    Span::styled("[d] deny", k(false, RED)),
                ]));
            }
            return edged(out, PURPLE);
        }
        let mut out = match s(i, "from") {
            "owner" => {
                let wait = self.waiting.iter().any(|x| x.text == s(i, "text"));
                let mark = if wait {
                    seg("", st(AMBER))
                } else {
                    seg(" ✓", st(Color::Green))
                };
                let mut out = wrap(&[seg("you", bold(AMBER)), seg(ago, st(GREY)), mark], iw, 0);
                out.extend(text(s(i, "text"), iw, Style::default()));
                out.extend(
                    wait.then(|| wrap(&[seg("… logged · waits for shed", st(AMBER))], iw, 2))
                        .into_iter()
                        .flatten(),
                );
                out
            }
            "orchestrator" => {
                let mut out = wrap(&[seg("orchestrator", bold(PURPLE)), seg(ago, st(GREY))], iw, 0);
                out.extend(text(s(i, "text"), iw, Style::default()));
                out
            }
            _ => {
                let link = i["link"].as_str().map_or(String::new(), |l| format!(" ({l})"));
                return text(&format!("· {}{link}", s(i, "text")), w, st(GREY));
            }
        };
        for f in i["files"].as_array().into_iter().flatten() {
            out.extend(wrap(&[seg("▸ file ", st(GREY)), seg(s(f, "path"), st(BLUE))], iw, 2));
        }
        edged(out, if s(i, "from") == "owner" { AMBER } else { PURPLE })
    }

    fn panel(&self, w: usize) -> Lines {
        let Some(p) = &self.pick else { return vec![] };
        let (title, rows) = match &p.act {
            Act::Answer { ask, n, text, .. } => (
                format!("answer {ask}"),
                vec![
                    vec![seg("sends ", st(GREY)), seg(format!("[{n}] {text}"), Style::default())],
                    vec![
                        seg("to ", st(GREY)),
                        seg(
                            format!("{ask} in {}, as owner", self.open.as_deref().unwrap_or_default()),
                            Style::default(),
                        ),
                    ],
                ],
            ),
            Act::Card { card: c, approve } => (
                format!("{} {}", if *approve { "approve" } else { "deny" }, s(c, "id")),
                vec![
                    vec![
                        seg(if *approve { "runs " } else { "denies " }, st(GREY)),
                        seg("$ ", st(AMBER)),
                        seg(s(c, "cmd"), st(CODE)),
                    ],
                    vec![seg("in ", st(GREY)), seg(s(c, "dir"), Style::default())],
                    if *approve {
                        let sha: String = s(c, "sha").chars().take(7).collect();
                        vec![
                            seg(format!("once, on {} · sha ", s(c, "host")), st(GREY)),
                            seg(sha, Style::default()),
                            seg(format!(" · {}", left(c, self.now().now)), st(GREY)),
                        ]
                    } else {
                        vec![seg("nothing runs", st(GREY))]
                    },
                ],
            ),
        };
        let (edge, rows) = match (&p.refused, p.busy) {
            (Some(e), _) => (
                RED,
                vec![vec![seg(format!("✗ {e}"), st(RED))], vec![seg("nothing ran", st(GREY))]],
            ),
            (None, true) => (BLUE, [rows, vec![vec![seg("sending…", st(AMBER))]]].concat()),
            (None, false) => (BLUE, rows),
        };
        boxed(
            &rows,
            w,
            st(edge),
            Some(seg(title, st(edge).add_modifier(Modifier::BOLD))),
        )
    }

    fn input_box(&self, w: usize) -> Lines {
        let at = self
            .input
            .char_indices()
            .nth(self.cursor)
            .map_or(self.input.len(), |(i, _)| i);
        let (a, b) = self.input.split_at(at);
        let mut segs = vec![
            seg("› ", st(BLUE)),
            seg(a, Style::default()),
            seg("▌", st(BLUE)),
            seg(b, Style::default()),
        ];
        if self.input.is_empty() {
            segs.push(seg(
                format!("message {}", self.open.as_deref().unwrap_or_default()),
                st(GREY),
            ));
        }
        wrap(&segs, w, 2).into_iter().map(|l| bar(l, w)).collect()
    }

    fn hint(&self, w: usize) -> Lines {
        let wide = w >= 80;
        let h = match (&self.pick, self.open.is_some()) {
            _ if self.sheet.is_some() => "↑↓ scroll · esc or ⌫ or ? close",
            (Some(p), _) if p.refused.is_some() => "esc or ⌫ back",
            (Some(p), _) => match p.act {
                Act::Card { approve: true, .. } => "↵ run · esc or ⌫ back",
                Act::Card { .. } => "↵ deny · esc or ⌫ back",
                Act::Answer { .. } => "↵ send · esc or ⌫ back",
            },
            (None, false) if wide => "↑↓ j k move · ↵ open · tab board · ? keys · q quit",
            (None, false) => "↑↓ jk move · ↵ open · tab board · q",
            (None, true) if !self.input.is_empty() => "↵ send · ←→ move · ^U clear",
            (None, true) if self.open_ask().or(self.open_card()).is_some() && wide => {
                "↵ send · 1-9 answer · a d card · ↑↓ scroll · esc ⌫ list · tab board · ? keys"
            }
            (None, true) if self.open_ask().or(self.open_card()).is_some() => "1-9 answer · a d card · esc list · ?",
            (None, true) if wide => "↵ send · ↑↓ scroll · esc ⌫ list · tab board · ? keys",
            (None, true) => "↵ send · ↑↓ scroll · esc list · ?",
        };
        wrap(&[seg(format!(" {h}"), st(GREY))], w, 1)
    }
}

fn sheet(w: usize) -> Lines {
    let row = |(k, d): &(&str, &str)| wrap(&[seg(format!("{k:<14}"), st(AMBER)), seg(*d, Style::default())], w, 14);
    KEYS.iter().flat_map(row).collect()
}

fn left(c: &Value, now: u64) -> String {
    let posted = c["ts"].as_u64().unwrap_or(0);
    match (posted + TTL).checked_sub(now) {
        Some(l) if l > 0 => format!("{} min left", l.div_ceil(60)),
        _ => "expired".into(),
    }
}
