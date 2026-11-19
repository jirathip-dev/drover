use crate::ui::{seg, st, wrap, Lines, Seg, AMBER, BAR, BLUE, CODE, GREY, RED};
use drover_core::events::Json;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

pub(crate) const TEXTS: [&str; 17] = [
    "title",
    "badge",
    "value",
    "subtitle",
    "meta",
    "header",
    "label",
    "markdown",
    "text",
    "command",
    "dir",
    "touches",
    "sha",
    "source",
    "alt",
    "placeholder",
    "footer",
];
pub(crate) const KIDS: [&str; 6] = ["blocks", "rows", "fields", "buttons", "items", "options"];

#[derive(Default)]
pub(crate) struct Laid {
    pub(crate) lines: Lines,
    pub(crate) hots: Vec<Hot>,
}

pub(crate) struct Hot {
    pub(crate) id: String,
    pub(crate) at: Range<usize>,
    pub(crate) tap: Option<Tap>,
    pub(crate) acts: Vec<Act>,
}

#[derive(Clone)]
pub(crate) enum Tap {
    Sheet(String),
    Open { path: String, keep: bool },
    Thread(String),
    Show(String),
    Fold(String),
    Edit(String, String),
    Set(String, Value),
}

#[derive(Clone)]
pub(crate) struct Act {
    pub(crate) label: String,
    pub(crate) does: Does,
}

#[derive(Clone)]
pub(crate) enum Does {
    Send(Send),
    Set(String, Value),
}

#[derive(Clone)]
pub(crate) struct Send {
    pub(crate) screen: String,
    pub(crate) block: Option<String>,
    pub(crate) action: String,
    pub(crate) values: Map<String, Value>,
    pub(crate) confirm: Option<Value>,
    pub(crate) typed: Option<(String, String)>,
    pub(crate) about: Vec<String>,
}

impl Send {
    pub(crate) fn json(&self, typed: &str) -> Value {
        let mut values = self.values.clone();
        if let Some((k, _)) = &self.typed {
            values.insert(k.clone(), json!(typed));
        }
        json!({"screen": self.screen, "block": self.block, "action": self.action, "values": values})
    }
}

#[derive(Clone)]
pub(crate) struct Editing {
    pub(crate) name: String,
    pub(crate) was: Value,
    pub(crate) cur: usize,
}

#[derive(Default, Clone)]
pub(crate) struct Ui {
    pub(crate) sel: Option<String>,
    pub(crate) top: std::cell::Cell<usize>,
    pub(crate) pos: usize,
    pub(crate) open: BTreeSet<String>,
    pub(crate) edits: BTreeMap<String, Value>,
    pub(crate) editing: Option<Editing>,
}

pub(crate) fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.at(k).as_str().unwrap_or_default()
}

pub(crate) fn tone(t: &str) -> Style {
    match t {
        "ok" | "working" => st(Color::Green),
        "warn" | "needs" => st(AMBER),
        "bad" | "fail" => st(RED),
        "accent" | "done" => st(BLUE),
        "" => Style::default(),
        _ => st(GREY),
    }
}

pub(crate) fn figure(f: &Value) -> Seg {
    match s(f, "state") {
        "working" => seg("● ", tone("working")),
        "needs" => seg("! ", tone("needs")),
        "fail" => seg("✗ ", tone("fail")),
        "done" => seg("✓ ", tone("done")),
        "" => seg("", Style::default()),
        _ => seg("○ ", st(GREY)),
    }
}

fn ago(secs: u64, label: bool) -> String {
    match (secs, label) {
        (s, true) if s < 120 => format!("{s}s"),
        (s, true) if s < 7200 => format!("{}m", s / 60),
        (s, true) if s < 172_800 => format!("{}h", s / 3600),
        (_, false) if secs < 60 => "now".into(),
        (s, false) if s < 3600 => format!("{}m", s / 60),
        (s, false) if s < 86_400 => format!("{}h {}m", s / 3600, s / 60 % 60),
        (s, _) => format!("{}d", s / 86_400),
    }
}

pub(crate) fn txt(v: &Value, now: u64) -> (String, &str) {
    let at = |k: &str| drover_core::usage::epoch(s(v, k)).unwrap_or(now);
    let label = v.at("label").as_str();
    match v {
        Value::String(t) => (t.clone(), ""),
        Value::Object(m) if m.contains_key("until") => {
            let left = at("until").saturating_sub(now).saturating_add(30) / 60;
            (label.unwrap_or_default().replace("{rel}", &format!("{left}m")), "")
        }
        Value::Object(m) if m.contains_key("since") => {
            let age = ago(now.saturating_sub(at("since")), label.is_some());
            (label.map_or(age.clone(), |l| l.replace("{}", &age)), "")
        }
        Value::Object(m) if m.contains_key("text") => (txt(v.at("text"), now).0, s(v, "tone")),
        Value::Null | Value::Object(_) | Value::Array(_) => (String::new(), ""),
        x => (x.to_string(), ""),
    }
}

fn inline(l: &str, base: Style) -> Vec<Seg> {
    let mut out = vec![];
    for (i, part) in l.split("**").enumerate() {
        let bold = i % 2 == 1;
        for (j, bit) in part.split('`').enumerate() {
            let code = j % 2 == 1;
            let mut style = if code { st(CODE) } else { base };
            if bold {
                style = style.add_modifier(Modifier::BOLD);
            }
            out.push(seg(links(bit), style));
        }
    }
    out
}

fn links(t: &str) -> String {
    let mut out = String::new();
    let mut rest = t;
    while let Some(a) = rest.find('[') {
        let (head, tail) = rest.split_at(a);
        let found = tail.find("](").and_then(|m| Some((m, m + tail.get(m..)?.find(')')?)));
        let Some((m, e)) = found else { break };
        out.push_str(head);
        out.push_str(tail.get(1..m).unwrap_or_default());
        out.push_str(" (");
        out.push_str(tail.get(m + 2..e).unwrap_or_default());
        out.push(')');
        rest = tail.get(e + 1..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

pub(crate) struct L<'a> {
    pub(crate) w: usize,
    pub(crate) ui: &'a Ui,
    pub(crate) now: u64,
    pub(crate) screen: String,
    pub(crate) out: Laid,
}

impl L<'_> {
    pub(crate) fn on(&self, id: &str) -> bool {
        self.ui.sel.as_deref() == Some(id)
    }

    pub(crate) fn gut(&self, id: &str) -> Seg {
        if self.on(id) {
            seg("›", st(BLUE).add_modifier(Modifier::BOLD))
        } else {
            seg(" ", Style::default())
        }
    }

    pub(crate) fn add(&mut self, lines: Lines) {
        self.out.lines.extend(lines);
    }

    pub(crate) fn segs(&mut self, segs: &[Seg], indent: usize) {
        let lines = wrap(segs, self.w, indent);
        self.add(lines);
    }

    pub(crate) fn hot(&mut self, id: &str, start: usize, tap: Option<Tap>, acts: Vec<Act>) {
        if self.on(id) {
            let w = self.w;
            for l in self.out.lines.iter_mut().skip(start) {
                let mut spans = std::mem::take(&mut l.spans);
                let fill = " ".repeat(w.saturating_sub(Line::from(spans.clone()).width()));
                spans.push(Span::raw(fill));
                *l = Line::from(spans).style(Style::default().bg(BAR));
            }
        }
        let at = start..self.out.lines.len().max(start + 1);
        self.out.hots.push(Hot {
            id: id.into(),
            at,
            tap,
            acts,
        });
    }

    pub(crate) fn nest(&mut self, edge: &Seg, f: impl FnOnce(&mut L)) {
        let pad = edge.0.chars().count();
        let mut sub = L {
            w: self.w.saturating_sub(pad).max(1),
            ui: self.ui,
            now: self.now,
            screen: self.screen.clone(),
            out: Laid::default(),
        };
        f(&mut sub);
        let base = self.out.lines.len();
        for mut l in sub.out.lines {
            l.spans.insert(0, Span::styled(edge.0.clone(), edge.1));
            self.out.lines.push(l);
        }
        for mut h in sub.out.hots {
            h.at = h.at.start + base..h.at.end + base;
            self.out.hots.push(h);
        }
    }

    pub(crate) fn send(&self, label: &str, block: Option<&str>, action: &Value, values: Map<String, Value>) -> Act {
        Act {
            label: label.into(),
            does: Does::Send(Send {
                screen: self.screen.clone(),
                block: block.map(Into::into),
                action: action.as_str().unwrap_or_default().into(),
                values,
                confirm: None,
                typed: None,
                about: vec![],
            }),
        }
    }

    pub(crate) fn value(&self, f: &Value) -> Value {
        self.ui
            .edits
            .get(s(f, "name"))
            .cloned()
            .unwrap_or_else(|| f.at("value").clone())
    }

    pub(crate) fn fields(&self, v: &Value, out: &mut Map<String, Value>) {
        match v {
            Value::Object(m) => {
                for f in v.at("fields").as_array().into_iter().flatten() {
                    if !s(f, "name").is_empty() && s(f, "type") != "secret" {
                        let x = self.value(f);
                        let x = if s(f, "type") == "toggle" { json!(x == true) } else { x };
                        out.insert(s(f, "name").into(), x);
                    }
                }
                m.iter()
                    .filter(|(k, _)| *k != "fields")
                    .for_each(|(_, x)| self.fields(x, out));
            }
            Value::Array(a) => a.iter().for_each(|x| self.fields(x, out)),
            _ => {}
        }
    }

    pub(crate) fn tap(t: &Value) -> Option<Tap> {
        let p = |k: &str| t.at(k).as_str().map(String::from);
        if let Some(x) = p("sheet") {
            return Some(Tap::Sheet(x));
        }
        if let Some(path) = p("open") {
            let keep = t.at("keep") == true;
            return Some(Tap::Open { path, keep });
        }
        p("thread")
            .map(Tap::Thread)
            .or_else(|| p("url").or_else(|| p("copy")).map(Tap::Show))
    }

    pub(crate) fn block(&mut self, b: &Value, pos: &str) {
        let id = b
            .at("id")
            .as_str()
            .or_else(|| b.at("name").as_str())
            .map_or_else(|| pos.to_string(), String::from);
        match s(b, "type") {
            "screen" | "sheet" => self.screen(b, &id),
            "section" => self.section(b, &id),
            "list" => self.kids(b, &id),
            "row" => self.row(b, &id),
            "text" if b.get("markdown").is_some() || b.get("name").is_none() => self.md(s(b, "markdown")),
            "button" => self.buttons(&[b], &id),
            "form" => self.form(b, &id),
            "text" | "toggle" | "picker" | "secret" => self.field(b, &id),
            "chips" => self.chips(b, &id),
            "choice" => self.choice(b, &id),
            "stat" => self.stat(b),
            "run" => self.run(b, &id),
            "images" => self.images(b),
            "diagram" => self.diagram(b),
            "composer" => self.composer(b, &id),
            "card" => self.card(b, &id),
            "scene" => self.segs(&[seg(" Herd is drawn on the phone", st(GREY))], 1),
            _ => self.unknown(b, &id),
        }
    }

    pub(crate) fn kids(&mut self, b: &Value, id: &str) {
        for k in KIDS {
            for (i, c) in b.at(k).as_array().into_iter().flatten().enumerate() {
                if c.is_object() {
                    self.block(c, &format!("{id}.{k}{i}"));
                }
            }
        }
    }

    pub(crate) fn md(&mut self, t: &str) {
        let (mut fence, mut code) = (false, vec![]);
        for l in t.lines() {
            if l.trim_start().starts_with("```") {
                if fence {
                    let w = self.w.saturating_sub(1);
                    let boxed = crate::ui::boxed(&std::mem::take(&mut code), w, st(GREY), None);
                    let boxed = boxed.into_iter().map(|mut l| {
                        l.spans.insert(0, Span::raw(" "));
                        l
                    });
                    self.add(boxed.collect());
                }
                fence = !fence;
            } else if fence {
                code.push(vec![seg(l, st(CODE))]);
            } else {
                let mut segs = vec![seg(" ", Style::default())];
                segs.extend(inline(l, Style::default()));
                self.segs(&segs, 1);
            }
        }
        if !code.is_empty() {
            let boxed = crate::ui::boxed(&code, self.w.saturating_sub(1), st(GREY), None);
            self.add(boxed);
        }
    }

    fn unknown(&mut self, b: &Value, id: &str) {
        let start = self.out.lines.len();
        let mut segs = vec![self.gut(id)];
        for k in TEXTS {
            let (t, tn) = txt(b.at(k), self.now);
            if !t.is_empty() {
                segs.push(seg(format!("{t} "), tone(tn)));
            }
        }
        if segs.len() > 1 {
            self.segs(&segs, 1);
        }
        let act = b
            .get("action")
            .map(|a| self.send(s(b, "label"), Some(id), a, own(b.at("values"))));
        let tap = Self::tap(b.at("tap"));
        if tap.is_some() || act.is_some() {
            self.hot(id, start, tap, act.into_iter().collect());
        }
        self.kids(b, id);
    }
}

pub(crate) fn own(v: &Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

pub(crate) fn lay(tree: &Value, w: usize, ui: &Ui, now: u64) -> Laid {
    let mut l = L {
        w: w.max(1),
        ui,
        now,
        screen: s(tree, "id").into(),
        out: Laid::default(),
    };
    l.block(tree, "root");
    l.out
}
