use super::lay::{figure, own, s, tone, txt, Act, Does, L};
use crate::ui::{ends, seg, st, wrap, BLUE, GREY, RED};
use drover_core::events::Json;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::Value;

const SPARK: &str = "▁▂▃▄▅▆▇█";

pub(crate) fn confirmed(mut a: Act, b: &Value) -> Act {
    if let (Does::Send(x), Some(c)) = (&mut a.does, b.get("confirm").filter(|c| c.is_object())) {
        x.confirm = Some(c.clone());
    }
    a
}

impl L<'_> {
    pub(crate) fn screen(&mut self, b: &Value, id: &str) {
        let (badge, bt) = txt(b.at("badge"), self.now);
        let back = if s(b, "type") == "sheet" { "‹ " } else { "" };
        if !s(b, "title").is_empty() {
            let bold = Style::default().add_modifier(Modifier::BOLD);
            self.segs(
                &[
                    seg(format!(" {back}{}", s(b, "title")), bold),
                    seg(format!(" {badge}"), tone(bt)),
                ],
                3,
            );
        }
        let acts: Vec<&Value> = (b.at("actions").as_array().into_iter().flatten())
            .flat_map(|a| a.at("menu").as_array().map_or_else(|| vec![a], |m| m.iter().collect()))
            .collect();
        self.bar(&acts, &format!("{id}.actions"), true);
        self.kids(b, id);
        if let Some(done) = b.get("done").filter(|d| d.is_object()) {
            let mut values = own(done.at("values"));
            self.fields(b, &mut values);
            let start = self.out.lines.len();
            let act = self.send(s(done, "label"), Some(id), done.at("action"), values);
            let on = self.on("done");
            let style = if on {
                st(BLUE).add_modifier(Modifier::REVERSED)
            } else {
                st(BLUE)
            };
            self.segs(&[self.gut("done"), seg(format!("[{}]", s(done, "label")), style)], 2);
            self.hot("done", start, None, vec![act]);
        }
    }

    pub(crate) fn buttons(&mut self, list: &[&Value], pos: &str) {
        self.bar(list, pos, false);
    }

    fn bar(&mut self, list: &[&Value], pos: &str, top: bool) {
        let mut line = vec![Span::raw(" ")];
        let mut hots = vec![];
        let gut = |line: &mut Vec<Span<'static>>, on: bool| {
            if let Some(g) = line.first_mut().filter(|_| on) {
                *g = Span::styled("›", st(BLUE));
            }
        };
        for (i, b) in list.iter().enumerate() {
            let id = b.at("id").as_str().map_or_else(|| format!("{pos}.{i}"), String::from);
            let label = format!("[{}]", s(b, "label"));
            let n = label.chars().count() + 1;
            let width = Line::from(line.clone()).width();
            if width + n > self.w && line.len() > 1 {
                self.out
                    .lines
                    .push(Line::from(std::mem::replace(&mut line, vec![Span::raw(" ")])));
            }
            gut(&mut line, self.on(&id));
            let style = match s(b, "style") {
                "bad" => st(RED),
                "" | "primary" => st(BLUE),
                x => tone(x),
            };
            let style = if self.on(&id) {
                style.add_modifier(Modifier::REVERSED)
            } else {
                style
            };
            let block = (!top).then_some(id.as_str());
            let act = b
                .get("action")
                .map(|a| confirmed(self.send(s(b, "label"), block, a, own(b.at("values"))), b));
            let tap = Self::tap(b.at("tap"));
            if n > self.w {
                self.out
                    .lines
                    .push(Line::from(std::mem::replace(&mut line, vec![Span::raw(" ")])));
                let start = self.out.lines.len();
                self.segs(&[seg(" ", Style::default()), seg(label, style)], 1);
                hots.push((id, start..self.out.lines.len(), tap, act));
                continue;
            }
            line.extend([Span::styled(label, style), Span::raw(" ")]);
            let at = self.out.lines.len();
            hots.push((id, at..at + 1, tap, act));
        }
        if line.len() > 1 {
            self.out.lines.push(Line::from(line));
        }
        for (id, at, tap, act) in hots {
            self.out.hots.push(super::lay::Hot {
                id,
                at,
                tap,
                acts: act.into_iter().collect(),
            });
        }
    }

    pub(crate) fn section(&mut self, b: &Value, id: &str) {
        if !self.out.lines.is_empty() {
            self.out.lines.push(Line::default());
        }
        let h = s(b, "header");
        if !h.is_empty() {
            self.segs(&[seg(format!(" {h}"), st(GREY).add_modifier(Modifier::BOLD))], 2);
        }
        self.kids(b, id);
        let f = s(b, "footer");
        if !f.is_empty() {
            self.segs(&[seg(format!(" {f}"), st(GREY))], 2);
        }
    }

    pub(crate) fn row(&mut self, b: &Value, id: &str) {
        let start = self.out.lines.len();
        let ind = " ".repeat(2 * usize::try_from(b.at("indent").as_u64().unwrap_or(0).min(3)).unwrap_or(0));
        let on = self.on(id);
        let bold = if on {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let head = [
            self.gut(id),
            seg(ind.clone(), Style::default()),
            figure(b.at("figure")),
            seg(txt(b.at("title"), self.now).0, bold),
        ];
        let (badge, bt) = txt(b.at("badge"), self.now);
        let (value, vt) = txt(b.at("value"), self.now);
        let mut right = vec![];
        if !badge.is_empty() {
            right.push(Span::styled(
                format!(" {badge}"),
                tone(if bt.is_empty() { "mute" } else { bt }),
            ));
        }
        if !value.is_empty() {
            right.push(Span::styled(
                format!(" {value}"),
                if vt.is_empty() { st(GREY) } else { tone(vt) },
            ));
        }
        if b.get("accessory").is_some() {
            right.push(Span::styled(" ▸", st(GREY)));
        }
        let pad = ind.chars().count() + 3;
        let lines = ends(wrap(&head, self.w, pad), right, self.w);
        self.add(lines);
        let (sub, stn) = txt(b.at("subtitle"), self.now);
        if !sub.is_empty() {
            let style = if stn.is_empty() { st(GREY) } else { tone(stn) };
            self.segs(&[seg(format!("{}{sub}", " ".repeat(pad)), style)], pad);
        }
        let swipe: Vec<Act> = (b.at("swipe").as_array().into_iter().flatten())
            .map(|x| {
                confirmed(
                    self.send(s(x, "label"), Some(id), x.at("action"), own(x.at("values"))),
                    x,
                )
            })
            .collect();
        let tap = Self::tap(b.at("tap"));
        if tap.is_some() || !swipe.is_empty() {
            self.hot(id, start, tap, swipe);
        }
    }

    pub(crate) fn stat(&mut self, b: &Value) {
        let mut segs = vec![seg(" ", Style::default())];
        for (i, it) in b.at("items").as_array().into_iter().flatten().enumerate() {
            if i > 0 {
                segs.push(seg(" · ", st(GREY)));
            }
            let (v, t) = txt(it.at("value"), self.now);
            segs.push(seg(format!("{} ", s(it, "label")), st(GREY)));
            segs.push(seg(v, tone(t).add_modifier(Modifier::BOLD)));
            if !s(it, "unit").is_empty() {
                segs.push(seg(format!(" {}", s(it, "unit")), st(GREY)));
            }
            let sp: Vec<u64> = (it.at("spark").as_array().into_iter().flatten())
                .map(|x| x.as_u64().unwrap_or(0))
                .collect();
            if sp.len() > 1 {
                let hi = sp.iter().copied().max().unwrap_or(1).max(1);
                let bars: String = (sp.iter())
                    .filter_map(|v| SPARK.chars().nth(usize::try_from(v * 7 / hi).unwrap_or(0)))
                    .collect();
                segs.push(seg(format!(" {bars}"), st(BLUE)));
            }
        }
        self.segs(&segs, 1);
    }
}
