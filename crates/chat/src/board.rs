mod cards;
mod feed;
mod forms;
pub(crate) mod lay;
mod rows;

use crate::ui::{boxed, put, seg, st, wrap, Lines, Seg, AMBER, BLUE, GREY, RED};
use drover_core::events::Json;
use drover_core::{clock, Ctx};
use drover_server::serve::{Local, Place};
use feed::{Ask, Note};
use lay::{lay, s, txt, Act, Does, Editing, Hot, Laid, Tap, Ui};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::Frame;
use serde_json::{json, Value};
use std::cell::Cell;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::Scope;
use std::time::Duration;

const WIDE: u16 = 100;

const KEYS: [(&str, &str); 19] = [
    ("Ctrl-C", "quit, anywhere; a running action finishes first"),
    ("Tab", "switch Threads / Board; each keeps its place"),
    ("↑ ↓  k j", "move the cursor from hot to hot"),
    ("Ctrl-B Ctrl-F", "a page up / down"),
    ("← →  h l", "root: the previous or next place"),
    ("m", "root: the menu (Hosts, Fleets, Workflow)"),
    (
        "↵",
        "follow the hot: open, unfold, tick, type, show; one action: its panel",
    ),
    ("1 … 9", "the panel for that action of the hot"),
    ("Esc or ⌫", "close the sheet or screen on top"),
    ("q", "root: quit"),
    ("?", "this sheet"),
    ("a field: text", "type; ← → move, ⌫ deletes, Ctrl-U clears"),
    ("a field: ↵", "keep it; then its form's send, if any"),
    ("a field: Esc", "undo it"),
    ("panel: ↵", "send it (run, for a run card)"),
    ("panel: Esc or ⌫", "back out; nothing is sent"),
    ("panel: text", "the line an other… sends"),
    ("refused: ↵ Esc ⌫", "close the refusal"),
    ("in a field", "q m j k h l ? and digits only type"),
];

pub(crate) enum Out {
    Stay,
    Quit,
    Thread(String),
}

enum Source {
    Live(String),
    Held,
}

enum Tree {
    Loading,
    Shown(Value),
    Gone,
}

struct View {
    from: Source,
    tree: Tree,
    ui: Ui,
}

impl View {
    fn live(path: String) -> View {
        View {
            from: Source::Live(path),
            tree: Tree::Loading,
            ui: Ui::default(),
        }
    }

    fn held(tree: Value) -> View {
        View {
            from: Source::Held,
            tree: Tree::Shown(tree),
            ui: Ui::default(),
        }
    }

    fn path(&self) -> Option<&str> {
        match &self.from {
            Source::Live(p) => Some(p),
            Source::Held => None,
        }
    }
}

enum Picked {
    Shown,
    Sending,
    Refused(String),
}

struct Pick {
    act: Act,
    typed: String,
    state: Picked,
    hidden: bool,
}

pub(crate) struct Board<'s, 'e> {
    s: &'s Scope<'s, 'e>,
    door: &'e Local<'e>,
    base: u64,
    born: u64,
    places: Vec<Place>,
    root: usize,
    cards: View,
    stack: Vec<View>,
    pick: Option<Pick>,
    said: Option<(String, Style)>,
    feed: Sender<Ask>,
    tx: Sender<Note>,
    rx: Receiver<Note>,
    size: Cell<(u16, u16)>,
    paging: Option<String>,
}

fn ctrl(k: &KeyEvent) -> bool {
    k.modifiers.contains(KeyModifiers::CONTROL)
}

fn plain(l: &Line) -> String {
    l.spans.iter().map(|s| s.content.as_ref()).collect()
}

impl<'s, 'e> Board<'s, 'e> {
    pub(crate) fn new(s: &'s Scope<'s, 'e>, ctx: &Ctx, door: &'e Local<'e>, fleet: Option<&str>) -> Self {
        let (feed, asks) = channel();
        let (tx, rx) = channel();
        let (base, born) = (ctx.now, clock::now());
        let notes = tx.clone();
        s.spawn(move || feed::run(door, &|| base + clock::now().saturating_sub(born), &asks, &notes));
        let places = Local::places();
        let home = Local::home(fleet);
        let root = places
            .iter()
            .position(|p| p.root && home.starts_with(&p.path))
            .unwrap_or(0);
        let b = Board {
            s,
            door,
            base,
            born,
            places,
            root,
            cards: View::live(Local::cards()),
            stack: vec![View::live(home)],
            pick: None,
            said: None,
            feed,
            tx,
            rx,
            size: Cell::new((40, 18)),
            paging: None,
        };
        b.watch();
        b
    }

    fn now(&self) -> u64 {
        self.base + clock::now().saturating_sub(self.born)
    }

    fn watch(&self) {
        let paths = std::iter::once(&self.cards).chain(&self.stack).filter_map(View::path);
        self.feed.send(Ask::Watch(paths.map(String::from).collect())).ok();
    }

    fn roots(&self) -> Vec<&Place> {
        self.places.iter().filter(|p| p.root).collect()
    }

    pub(crate) fn badge(&self) -> Option<u64> {
        match &self.stack.first()?.tree {
            Tree::Shown(t) => t.at("badge").as_u64(),
            _ => None,
        }
    }

    fn top(&self) -> &View {
        self.stack.last().unwrap_or(&self.cards)
    }

    fn top_mut(&mut self) -> &mut View {
        match self.stack.last_mut() {
            Some(v) => v,
            None => &mut self.cards,
        }
    }

    fn panes(&self, w: usize) -> Vec<(Laid, bool)> {
        let now = self.now();
        let draw = |v: &View, ui: &Ui, w: usize| match &v.tree {
            Tree::Shown(t) => lay(t, w, ui, now),
            Tree::Loading => lay(&json!({"type": "text", "markdown": "loading…"}), w, ui, now),
            Tree::Gone => lay(
                &json!({"type": "text", "markdown": "this screen is gone · esc closes"}),
                w,
                ui,
                now,
            ),
        };
        let ui = &self.top().ui;
        if self.stack.len() > 1 {
            return vec![(draw(self.top(), ui, w), true)];
        }
        vec![(draw(&self.cards, ui, w), true), (draw(self.top(), ui, w), true)]
    }

    fn hots(&self) -> Vec<Hot> {
        let w = usize::from(self.size.get().0).min(usize::from(WIDE) / 2);
        self.panes(w).into_iter().flat_map(|p| p.0.hots).collect()
    }

    pub(crate) fn tick(&mut self, wait: Duration) {
        let first = self.rx.recv_timeout(wait).ok();
        let notes: Vec<Note> = first.into_iter().chain(self.rx.try_iter()).collect();
        for n in notes {
            self.apply(n);
        }
        let hots = self.hots();
        let ui = &mut self.top_mut().ui;
        match hots.iter().position(|h| Some(&h.id) == ui.sel.as_ref()) {
            Some(i) => ui.pos = i,
            None => ui.sel = hots.get(ui.pos.min(hots.len().saturating_sub(1))).map(|h| h.id.clone()),
        }
    }

    fn apply(&mut self, n: Note) {
        match n {
            Note::Tree(path, tree) => {
                for v in std::iter::once(&mut self.cards).chain(self.stack.iter_mut()) {
                    if v.path() == Some(path.as_str()) {
                        v.tree = tree.clone().map_or(Tree::Gone, Tree::Shown);
                    }
                }
            }
            Note::Page(path, tree) => self.paged(&path, tree.as_ref()),
            Note::Done(Err(line), _) => {
                if let Some(p) = self.pick.as_mut() {
                    (p.state, p.hidden) = (Picked::Refused(line.clone()), false);
                }
                self.said = Some((format!("✗ {line}"), st(RED)));
            }
            Note::Done(Ok(done), live) => {
                self.pick = None;
                if !done.toast.is_empty() {
                    self.said = Some((format!("✓ {}", done.toast), st(Color::Green)));
                }
                if done.close && self.stack.len() > 1 {
                    self.stack.pop();
                } else if let Some(tree) = done.screen {
                    let stream = s(&tree, "stream").replace("/stream", "");
                    let v = self.top_mut();
                    if !stream.is_empty() {
                        v.from = Source::Live(stream);
                    } else if !live {
                        v.from = Source::Held;
                    }
                    v.tree = Tree::Shown(tree);
                }
                self.watch();
                self.feed.send(Ask::Kick).ok();
            }
        }
    }

    fn paged(&mut self, more: &str, page: Option<&Value>) {
        if self.paging.as_deref() != Some(more) {
            return;
        }
        self.paging = None;
        let to = page
            .and_then(more_of)
            .and_then(|m| after(&m))
            .unwrap_or_else(|| "0.0".into());
        let v = self.top_mut();
        if let Some(p) = v.path() {
            let (base, q) = p.split_once('?').unwrap_or((p, ""));
            let mut q: Vec<&str> = q
                .split('&')
                .filter(|k| !k.is_empty() && !k.starts_with("to="))
                .collect();
            let to = format!("to={to}");
            q.push(&to);
            v.from = Source::Live(format!("{base}?{}", q.join("&")));
            self.watch();
        }
    }

    fn push(&mut self, v: View) {
        self.stack.push(v);
        self.watch();
    }

    fn follow(&mut self, tap: Tap) -> Out {
        match tap {
            Tap::Sheet(p) => self.push(View::live(p)),
            Tap::Open { path, keep } => {
                let same = self.top().path().and_then(|p| p.split('?').next()) == path.split('?').next();
                if (keep || same) && self.stack.len() > 1 {
                    self.stack.pop();
                } else if keep || same {
                    self.top_mut().from = Source::Live(path);
                    return self.watched();
                }
                self.push(View::live(path));
            }
            Tap::Thread(t) => return Out::Thread(t),
            Tap::Show(t) => self.push(View::held(json!({"type": "sheet", "id": "show", "title": "link",
                "blocks": [{"type": "text", "id": "show-text", "markdown": t}]}))),
            Tap::Fold(id) => {
                let open = &mut self.top_mut().ui.open;
                if !open.remove(&id) {
                    open.insert(id);
                }
            }
            Tap::Edit(name, cur) => {
                let was = json!(cur);
                let ui = &mut self.top_mut().ui;
                ui.editing = Some(Editing {
                    name: name.clone(),
                    was,
                    cur: cur.chars().count(),
                });
                ui.edits.insert(name, json!(cur));
            }
            Tap::Set(name, v) => {
                self.top_mut().ui.edits.insert(name, v);
            }
        }
        Out::Stay
    }

    fn watched(&self) -> Out {
        self.watch();
        Out::Stay
    }

    fn take(&mut self, a: Act) {
        match a.does {
            Does::Set(name, v) => {
                self.top_mut().ui.edits.insert(name, v);
            }
            Does::Send(_) => {
                self.pick = Some(Pick {
                    act: a,
                    typed: String::new(),
                    state: Picked::Shown,
                    hidden: false,
                });
            }
        }
    }

    fn send(&mut self) {
        let Some(p) = self.pick.as_mut() else { return };
        let Does::Send(x) = &p.act.does else { return };
        if !matches!(p.state, Picked::Shown) || x.typed.is_some() && p.typed.trim().is_empty() {
            return;
        }
        p.state = Picked::Sending;
        let body = x.json(&p.typed);
        let (door, tx, now) = (self.door, self.tx.clone(), self.now());
        let path = self.top().path().map(String::from);
        self.s.spawn(move || {
            let res = door.act(now, &body);
            let live = match (&res, path) {
                (Ok(d), Some(p)) => d.screen.is_some() && door.screen(now, &p) == d.screen,
                _ => false,
            };
            tx.send(Note::Done(res, live)).ok();
        });
    }

    fn pick_key(&mut self, k: &KeyEvent) {
        let Some(p) = self.pick.as_mut() else { return };
        let typing = matches!(&p.act.does, Does::Send(x) if x.typed.is_some());
        match (&p.state, k.code) {
            (Picked::Shown, KeyCode::Backspace) if typing && !p.typed.is_empty() => {
                p.typed.pop();
            }
            (Picked::Refused(_), KeyCode::Enter | KeyCode::Esc | KeyCode::Backspace)
            | (Picked::Shown, KeyCode::Esc | KeyCode::Backspace) => self.pick = None,
            (Picked::Sending, KeyCode::Esc | KeyCode::Backspace) => p.hidden = true,
            (Picked::Shown, KeyCode::Enter) => self.send(),
            (Picked::Shown, KeyCode::Char('u')) if typing && ctrl(k) => p.typed.clear(),
            (Picked::Shown, KeyCode::Char(c)) if typing && !ctrl(k) => p.typed.push(c),
            _ => {}
        }
    }

    fn edit_key(&mut self, k: &KeyEvent) {
        let ui = &mut self.top_mut().ui;
        let Some(e) = ui.editing.as_mut() else { return };
        let mut v = ui
            .edits
            .get(&e.name)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let byte = |s: &str, at: usize| s.char_indices().nth(at).map_or(s.len(), |(i, _)| i);
        match k.code {
            KeyCode::Esc => {
                ui.edits.insert(e.name.clone(), e.was.clone());
                ui.editing = None;
                return;
            }
            KeyCode::Enter => {
                let name = e.name.clone();
                ui.editing = None;
                return self.submit(&name);
            }
            KeyCode::Char('u') if ctrl(k) => (v, e.cur) = (String::new(), 0),
            KeyCode::Char(c) if !ctrl(k) => {
                v.insert(byte(&v, e.cur), c);
                e.cur += 1;
            }
            KeyCode::Backspace if e.cur > 0 => {
                e.cur -= 1;
                v.remove(byte(&v, e.cur));
            }
            KeyCode::Left => e.cur = e.cur.saturating_sub(1),
            KeyCode::Right => e.cur = (e.cur + 1).min(v.chars().count()),
            _ => {}
        }
        ui.edits.insert(e.name.clone(), json!(v));
    }

    fn submit(&mut self, field: &str) {
        let hots = self.hots();
        let after = hots.iter().skip_while(|h| h.id != field).skip(1);
        let mut send = after.filter(|h| h.id == "done" || h.id.ends_with(".submit"));
        if let Some(a) = send.next().and_then(|h| h.acts.first().cloned()) {
            self.take(a);
        }
    }

    fn step(&mut self, by: isize) {
        let hots = self.hots();
        let ui = &self.top().ui;
        let at = hots.iter().position(|h| Some(&h.id) == ui.sel.as_ref()).unwrap_or(0);
        let to = at.saturating_add_signed(by).min(hots.len().saturating_sub(1));
        let id = hots.get(to).map(|h| h.id.clone());
        let ui = &mut self.top_mut().ui;
        (ui.sel, ui.pos) = (id, to);
        if to + 1 >= hots.len() && self.paging.is_none() {
            let more = match &self.top().tree {
                Tree::Shown(t) => more_of(t),
                _ => None,
            };
            if let Some(m) = more {
                self.paging = Some(m.clone());
                self.feed.send(Ask::More(m)).ok();
            }
        }
    }

    fn page(&self) -> isize {
        isize::try_from(self.size.get().1 / 3).unwrap_or(1).max(1)
    }

    fn place(&mut self, by: isize) {
        let n = self.roots().len();
        if n == 0 {
            return;
        }
        self.root = (self.root.saturating_add_signed(by)).min(n - 1);
        if let Some(p) = self.roots().get(self.root).map(|p| p.path.clone()) {
            self.stack = vec![View::live(p)];
            self.watch();
        }
    }

    fn menu(&mut self) {
        let rows: Vec<Value> = (self.places.iter().filter(|p| !p.root))
            .map(|p| json!({"type": "row", "id": p.path, "title": p.label, "subtitle": p.subtitle, "accessory": "chevron", "tap": {"sheet": p.path}}))
            .collect();
        self.push(View::held(json!({"type": "sheet", "id": "menu", "title": "Menu", "blocks": [{"type": "list", "id": "menu-list", "rows": rows}]})));
    }

    fn keys(&mut self) {
        let rows: Vec<Value> = (KEYS.iter().enumerate())
            .map(|(i, (k, d))| json!({"type": "row", "id": format!("k{i}"), "title": k, "subtitle": d}))
            .collect();
        self.push(View::held(json!({"type": "sheet", "id": "keys", "title": "Keys", "blocks": [{"type": "list", "id": "keys-list", "rows": rows}]})));
    }

    pub(crate) fn key(&mut self, k: &KeyEvent) -> Out {
        self.said = None;
        if self.pick.as_ref().is_some_and(|p| !p.hidden) {
            self.pick_key(k);
            return Out::Stay;
        }
        if self.top().ui.editing.is_some() {
            self.edit_key(k);
            return Out::Stay;
        }
        let root = self.stack.len() == 1;
        let hot = || {
            let ui = &self.top().ui;
            self.hots().into_iter().find(|h| Some(&h.id) == ui.sel.as_ref())
        };
        match (k.code, ctrl(k)) {
            (KeyCode::Up | KeyCode::Char('k'), false) => self.step(-1),
            (KeyCode::Down | KeyCode::Char('j'), false) => self.step(1),
            (KeyCode::Char('b'), true) | (KeyCode::PageUp, _) => self.step(-self.page()),
            (KeyCode::Char('f'), true) | (KeyCode::PageDown, _) => self.step(self.page()),
            (KeyCode::Left | KeyCode::Char('h'), false) if root => self.place(-1),
            (KeyCode::Right | KeyCode::Char('l'), false) if root => self.place(1),
            (KeyCode::Char('m'), false) if root => self.menu(),
            (KeyCode::Char('q'), false) if root => return Out::Quit,
            (KeyCode::Char('?'), false) => self.keys(),
            (KeyCode::Esc | KeyCode::Backspace, _) if !root => {
                self.stack.pop();
                self.watch();
            }
            (KeyCode::Enter, _) => match hot() {
                Some(Hot { tap: Some(t), .. }) => return self.follow(t),
                Some(Hot { acts, .. }) if acts.len() == 1 => {
                    if let Some(a) = acts.into_iter().next() {
                        self.take(a);
                    }
                }
                _ => {}
            },
            (KeyCode::Char(c @ '1'..='9'), false) => {
                let n = c.to_digit(10).and_then(|n| usize::try_from(n).ok()).unwrap_or(1);
                if let Some(a) = hot().and_then(|h| h.acts.into_iter().nth(n - 1)) {
                    self.take(a);
                }
            }
            _ => {}
        }
        Out::Stay
    }

    fn strip(&self, w: usize) -> Lines {
        let mut spans = vec![Span::raw(" ")];
        for (i, p) in self.roots().iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" · ", st(GREY)));
            }
            let style = if i == self.root {
                st(BLUE).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                st(GREY)
            };
            spans.push(Span::styled(p.label.clone(), style));
        }
        crate::ui::ends(vec![Line::from(spans)], vec![Span::styled("m menu ", st(GREY))], w)
    }

    fn panel(&self, w: usize) -> Lines {
        let Some(p) = self.pick.as_ref().filter(|p| !p.hidden) else {
            return vec![];
        };
        let Does::Send(x) = &p.act.does else { return vec![] };
        let mut rows: Vec<Vec<Seg>> = vec![];
        if let Some(c) = &x.confirm {
            rows.push(vec![seg(s(c, "title"), Style::default().add_modifier(Modifier::BOLD))]);
            if !s(c, "message").is_empty() {
                rows.push(vec![seg(s(c, "message"), Style::default())]);
            }
        }
        rows.extend(x.about.iter().map(|a| vec![seg(a.clone(), Style::default())]));
        let block = x.block.as_deref().map_or(String::new(), |b| format!(" · {b}"));
        rows.push(vec![
            seg("sends ", st(GREY)),
            seg(x.action.clone(), Style::default().add_modifier(Modifier::BOLD)),
            seg(format!(" to {}{block}", x.screen), st(GREY)),
        ]);
        let mut vals: Vec<Seg> = (x
            .values
            .iter()
            .filter(|(k, _)| Some(*k) != x.typed.as_ref().map(|t| &t.0)))
        .flat_map(|(k, v)| {
            [
                seg(" · ", st(GREY)),
                seg(format!("{k} "), st(GREY)),
                seg(show(v), Style::default()),
            ]
        })
        .collect();
        if !vals.is_empty() {
            vals.remove(0);
        }
        if !vals.is_empty() {
            rows.push(vals);
        }
        if let Some((_, ph)) = &x.typed {
            rows.push(if p.typed.is_empty() {
                vec![seg("› ", st(BLUE)), seg("▌", st(BLUE)), seg(ph.clone(), st(GREY))]
            } else {
                vec![
                    seg("› ", st(BLUE)),
                    seg(p.typed.clone(), Style::default()),
                    seg("▌", st(BLUE)),
                ]
            });
        }
        let edge = match &p.state {
            Picked::Refused(line) => {
                rows.push(vec![seg(format!("✗ {line}"), st(RED))]);
                rows.push(vec![seg("nothing ran", st(GREY))]);
                RED
            }
            Picked::Sending => {
                rows.push(vec![seg("sending…", st(AMBER))]);
                BLUE
            }
            Picked::Shown => BLUE,
        };
        let title = format!("{} · {}", p.act.label, x.action);
        boxed(
            &rows,
            w,
            st(edge),
            Some(seg(title, st(edge).add_modifier(Modifier::BOLD))),
        )
    }

    fn hint(&self, w: usize, hot: Option<&Hot>) -> Lines {
        let wide = w >= 80;
        let run = |p: &Pick| matches!(&p.act.does, Does::Send(x) if !x.about.is_empty() && p.act.label == "approve");
        let h = match &self.pick {
            Some(p) if p.hidden => "sending… · its result shows here",
            Some(Pick {
                state: Picked::Refused(_),
                ..
            }) => "↵ esc or ⌫ close",
            Some(p) if run(p) => "↵ run · esc or ⌫ back",
            Some(_) => "↵ send · esc or ⌫ back",
            None if self.top().ui.editing.is_some() => "type · ↵ keep · ←→ move · ^U clear · esc undo",
            None if self.stack.len() > 1 && wide => "↑↓ move · ↵ open · 1-9 act · esc or ⌫ close · ? keys",
            None if self.stack.len() > 1 => "↑↓ ↵ 1-9 · esc ⌫ close · ?",
            None if wide => "↑↓ move · ↵ open · 1-9 act · ←→ place · m menu · tab threads · ? keys · q quit",
            None => "↑↓ ↵ 1-9 · ←→ place · m · tab · ? · q",
        };
        let mut out = vec![];
        if let Some((t, style)) = &self.said {
            out.extend(wrap(&[seg(format!(" {t}"), *style)], w, 3));
        }
        let acts: Vec<Seg> = (hot.map(|h| h.acts.as_slice()).unwrap_or_default().iter().enumerate())
            .flat_map(|(i, a)| {
                [
                    seg(format!(" {} ", i + 1), st(BLUE)),
                    seg(format!("{} ", a.label), Style::default()),
                ]
            })
            .collect();
        if self.pick.is_none() && !acts.is_empty() {
            out.extend(wrap(&acts, w, 1));
        }
        out.extend(wrap(&[seg(format!(" {h}"), st(GREY))], w, 1));
        out
    }

    fn pane(&self, f: &mut Frame, area: Rect, laid: Laid, cursor: bool, grey: bool) {
        let h = usize::from(area.height);
        let ui = &self.top().ui;
        let mut top = if cursor { ui.top.get() } else { 0 };
        let n = laid.lines.len();
        let room = if n > h { h.saturating_sub(1) } else { h };
        if let Some(hot) = laid.hots.iter().find(|x| cursor && Some(&x.id) == ui.sel.as_ref()) {
            if hot.at.start < top {
                top = hot.at.start;
            } else if hot.at.end > top + room {
                top = hot.at.end.saturating_sub(room).min(hot.at.start);
            }
        }
        top = top.min(n.saturating_sub(room));
        if cursor {
            ui.top.set(top);
        }
        let mut shown: Lines = laid.lines.into_iter().skip(top).take(room).collect();
        if n > h {
            let below = n.saturating_sub(top + room);
            let mut m = vec![];
            if top > 0 {
                m.push(format!("▲ {top} lines above"));
            }
            if below > 0 {
                m.push(format!("▼ {below} more below"));
            }
            shown.push(Line::styled(format!(" {}", m.join(" · ")), st(GREY)));
        }
        put(f, area, shown);
        if grey {
            f.buffer_mut().set_style(area, st(GREY));
        }
    }

    pub(crate) fn draw(&self, f: &mut Frame, a: Rect) {
        self.size.set((a.width, a.height));
        let w = usize::from(a.width);
        let wide = a.width >= WIDE;
        let sheet = self.stack.len() > 1;
        let hots = self.hots();
        let hot = hots.iter().find(|h| Some(&h.id) == self.top().ui.sel.as_ref());
        let strip = if sheet && !wide {
            vec![]
        } else {
            self.strip(if wide { w * 4 / 7 } else { w })
        };
        let foot = [self.panel(w), self.hint(w, hot)].concat();
        let sh = u16::try_from(strip.len()).unwrap_or(0).min(a.height);
        let fh = u16::try_from(foot.len()).unwrap_or(0).min(a.height.saturating_sub(sh));
        put(f, Rect { height: sh, ..a }, strip);
        let body = Rect {
            y: a.y + sh,
            height: a.height.saturating_sub(sh + fh),
            ..a
        };
        put(
            f,
            Rect {
                y: body.y + body.height,
                height: fh,
                ..a
            },
            foot,
        );
        let now = self.now();
        if !wide {
            let laid = self.panes(w).into_iter().fold(Laid::default(), |mut acc, (l, _)| {
                let base = acc.lines.len();
                acc.lines.extend(l.lines);
                acc.hots.extend(l.hots.into_iter().map(|mut h| {
                    h.at = h.at.start + base..h.at.end + base;
                    h
                }));
                acc
            });
            return self.pane(f, body, laid, true, false);
        }
        let lw = a.width * 4 / 7;
        let (left, right) = (
            Rect { width: lw, ..body },
            Rect {
                x: body.x + lw + 1,
                width: a.width.saturating_sub(lw + 1),
                ..body
            },
        );
        put(
            f,
            Rect {
                x: body.x + lw,
                width: 1,
                y: a.y,
                height: a.height.saturating_sub(fh),
            },
            vec![Line::styled("│", st(GREY)); usize::from(a.height)],
        );
        let rw = usize::from(right.width);
        let ui = &self.top().ui;
        let root = self.stack.first().map_or(&self.cards, |v| v);
        let tree = |v: &View| match &v.tree {
            Tree::Shown(t) => t.clone(),
            _ => json!({"type": "text", "markdown": "loading…"}),
        };
        if sheet {
            let base = Ui::default();
            self.pane(f, left, lay(&tree(root), usize::from(lw), &base, now), false, true);
            self.pane(f, right, lay(&tree(self.top()), rw, ui, now), true, false);
        } else {
            let in_cards = lay(&tree(&self.cards), rw, ui, now);
            let on_cards = in_cards.hots.iter().any(|h| Some(&h.id) == ui.sel.as_ref());
            self.pane(f, left, lay(&tree(root), usize::from(lw), ui, now), !on_cards, false);
            self.pane(f, right, in_cards, on_cards, false);
        }
    }
}

fn show(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(show).collect::<Vec<_>>().join(","),
        x if x.is_object() => x.to_string(),
        x => txt(x, 0).0,
    }
}

fn more_of(v: &Value) -> Option<String> {
    match v {
        Value::Object(m) => m
            .get("more")
            .and_then(Value::as_str)
            .map(String::from)
            .or_else(|| m.values().find_map(more_of)),
        Value::Array(a) => a.iter().find_map(more_of),
        _ => None,
    }
}

fn after(more: &str) -> Option<String> {
    let q = more.split_once('?')?.1;
    q.split('&').find_map(|kv| kv.strip_prefix("after=")).map(String::from)
}

/// `tree` as plain text lines `w` columns wide at `now`, every card unfolded: what the Board draws of it.
pub fn text(tree: &Value, w: usize, now: u64) -> Vec<String> {
    fn folds(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                if m.contains_key("fold") {
                    out.extend(m.get("id").and_then(Value::as_str).map(String::from));
                }
                m.values().for_each(|x| folds(x, out));
            }
            Value::Array(a) => a.iter().for_each(|x| folds(x, out)),
            _ => {}
        }
    }
    let mut open = vec![];
    folds(tree, &mut open);
    let ui = Ui {
        open: open.into_iter().collect(),
        ..Ui::default()
    };
    lay(tree, w, &ui, now).lines.iter().map(plain).collect()
}
