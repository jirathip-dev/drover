use super::lay::{own, s, tone, txt, Act, Does, Tap, L};
use super::rows::confirmed;
use crate::ui::{boxed, seg, st, AMBER, BLUE, CODE, GREY};
use drover_core::events::Json;
use ratatui::style::{Modifier, Style};
use serde_json::{json, Value};

impl L<'_> {
    pub(crate) fn card(&mut self, b: &Value, id: &str) {
        let edge = tone(if s(b, "tone").is_empty() { "mute" } else { s(b, "tone") });
        let fold = b.at("fold");
        let shut = fold.is_object() && !self.ui.open.contains(id);
        if !self.out.lines.is_empty() {
            self.out.lines.push(ratatui::text::Line::default());
        }
        self.nest(&seg("┃", edge), |l| {
            if fold.is_object() {
                let start = l.out.lines.len();
                let mark = if shut { "▸ " } else { "▾ " };
                l.segs(
                    &[
                        l.gut(id),
                        seg(mark, edge),
                        seg(txt(fold.at("line"), l.now).0, Style::default()),
                    ],
                    3,
                );
                let main = fold.at("main");
                let acts: Vec<Act> = (shut && main.is_object())
                    .then(|| {
                        let mut values = own(main.at("values"));
                        values.insert("choice".into(), main.at("value").clone());
                        l.send(s(main, "label"), main.at("block").as_str(), main.at("action"), values)
                    })
                    .into_iter()
                    .collect();
                l.hot(id, start, Some(Tap::Fold(id.into())), acts);
                if shut {
                    return;
                }
            }
            let (badge, bt) = txt(b.at("badge"), l.now);
            let (meta, _) = txt(b.at("meta"), l.now);
            let title = txt(b.at("title"), l.now).0;
            if !title.is_empty() || !badge.is_empty() {
                l.segs(
                    &[
                        seg(format!(" {title}"), Style::default().add_modifier(Modifier::BOLD)),
                        seg(format!(" {badge}"), tone(bt)),
                        seg(format!(" {meta}"), st(GREY)),
                    ],
                    2,
                );
            }
            let (sub, stn) = txt(b.at("subtitle"), l.now);
            if !sub.is_empty() {
                l.segs(
                    &[seg(
                        format!(" {sub}"),
                        if stn.is_empty() { st(GREY) } else { tone(stn) },
                    )],
                    2,
                );
            }
            l.kids(&json!({ "blocks": b.at("blocks") }), id);
            let buttons: Vec<&Value> = b.at("buttons").as_array().into_iter().flatten().collect();
            l.buttons(&buttons, &format!("{id}.buttons"));
        });
    }

    pub(crate) fn choice(&mut self, b: &Value, id: &str) {
        let start = self.out.lines.len();
        let rec = b.at("recommended");
        let mut opts: Vec<&Value> = b.at("options").as_array().into_iter().flatten().collect();
        opts.sort_by_key(|o| o.at("value") != rec || rec.is_null());
        let mut acts = vec![];
        for o in opts {
            let mut values = own(b.at("values"));
            values.extend(own(o.at("values")));
            values.insert("choice".into(), o.at("value").clone());
            acts.push(confirmed(self.send(s(o, "label"), Some(id), b.at("action"), values), o));
            let mark = if o.at("value") == rec && !rec.is_null() {
                " ★ recommended"
            } else {
                ""
            };
            let n = acts.len();
            self.segs(
                &[
                    self.gut(if n == 1 { id } else { "" }),
                    seg(format!("[{n}] "), st(BLUE)),
                    seg(s(o, "label"), Style::default()),
                    seg(mark, st(AMBER)),
                ],
                5,
            );
        }
        let other = b.at("other");
        if other.is_object() {
            let mut values = own(b.at("values"));
            values.insert("choice".into(), json!(s(other, "for")));
            let mut a = self.send("other…", Some(id), b.at("action"), values);
            if let Does::Send(x) = &mut a.does {
                let ph = other.at("placeholder").as_str().unwrap_or("other…");
                x.typed = Some(("other".into(), ph.into()));
            }
            acts.push(a);
            let n = acts.len();
            self.segs(&[seg(format!(" [{n}] "), st(BLUE)), seg("other…", st(GREY))], 5);
        }
        let d = b.at("default");
        if d.is_object() {
            let t = txt(&json!({"until": d.at("until"), "label": d.at("label")}), self.now).0;
            self.segs(&[seg(format!(" ⏱ {t}"), st(GREY))], 3);
        }
        if !acts.is_empty() {
            self.hot(id, start, None, acts);
        }
    }

    pub(crate) fn run(&mut self, b: &Value, id: &str) {
        let start = self.out.lines.len();
        let left = txt(&json!({"until": b.at("until"), "label": "{rel} left"}), self.now).0;
        let mut about = vec![format!("$ {}", s(b, "command"))];
        for (k, v) in [("in", s(b, "dir")), ("touches", s(b, "touches")), ("sha", s(b, "sha"))] {
            if !v.is_empty() {
                about.push(format!("{k} {v}"));
            }
        }
        if b.get("until").is_some() {
            about.push(left);
        }
        let rows: Vec<Vec<_>> = (about.iter())
            .map(|a| match a.strip_prefix("$ ") {
                Some(c) => vec![seg("$ ", st(AMBER)), seg(c, st(CODE))],
                None => vec![seg(a.clone(), Style::default())],
            })
            .collect();
        self.add(boxed(&rows, self.w, st(GREY), None));
        let mut acts = vec![];
        for (choice, label) in [("approve", "approve"), ("deny", "deny")] {
            let values = own(&json!({"choice": choice, "sha": b.at("sha")}));
            let mut a = self.send(label, Some(id), b.at("action"), values);
            if let Does::Send(x) = &mut a.does {
                x.about.clone_from(&about);
            }
            acts.push(a);
        }
        self.segs(
            &[
                self.gut(id),
                seg("[1] approve", st(BLUE)),
                seg("  [2] deny", tone("bad")),
            ],
            1,
        );
        self.hot(id, start, None, acts);
    }

    pub(crate) fn images(&mut self, b: &Value) {
        for i in b.at("items").as_array().into_iter().flatten() {
            let alt = s(i, "alt");
            self.segs(&[seg(format!(" ▸ image {alt} · shown on the phone"), st(GREY))], 3);
        }
    }

    pub(crate) fn diagram(&mut self, b: &Value) {
        let rows: Vec<Vec<_>> = s(b, "source").lines().map(|l| vec![seg(l, st(CODE))]).collect();
        self.add(boxed(&rows, self.w, st(GREY), None));
    }
}
