use super::lay::{own, s, txt, Act, Does, Tap, L};
use crate::ui::{seg, st, Seg, BLUE, GREY};
use drover_core::events::Json;
use ratatui::style::{Modifier, Style};
use serde_json::{json, Value};

impl L<'_> {
    fn typed(&self, name: &str, v: &str, placeholder: &str) -> Vec<Seg> {
        match self.ui.editing.as_ref().filter(|e| e.name == name) {
            Some(e) => {
                let at = v.char_indices().nth(e.cur).map_or(v.len(), |(i, _)| i);
                let (a, b) = v.split_at(at);
                vec![seg(a, Style::default()), seg("▌", st(BLUE)), seg(b, Style::default())]
            }
            None if v.is_empty() => vec![seg(placeholder, st(GREY))],
            None => vec![seg(v, Style::default())],
        }
    }

    pub(crate) fn field(&mut self, f: &Value, id: &str) {
        let start = self.out.lines.len();
        let name = s(f, "name");
        let v = self.value(f);
        let label = if s(f, "label").is_empty() { name } else { s(f, "label") };
        let mut segs = vec![self.gut(id)];
        let (tap, acts) = match s(f, "type") {
            "toggle" => {
                let on = v == true;
                segs.extend([
                    seg(if on { "[x] " } else { "[ ] " }, st(BLUE)),
                    seg(label, Style::default()),
                ]);
                (Some(Tap::Set(name.into(), json!(!on))), vec![])
            }
            "picker" => {
                let cur = txt(&v, self.now).0;
                segs.extend([
                    seg(format!("{label}: "), st(GREY)),
                    seg(format!("{cur} ▾"), Style::default()),
                ]);
                let mut opts = vec![v.clone()];
                opts.extend(f.at("options").as_array().into_iter().flatten().cloned());
                opts.dedup();
                let acts = (opts.into_iter().filter(|o| !o.is_null() && *o != ""))
                    .map(|o| Act {
                        label: txt(&o, self.now).0,
                        does: Does::Set(name.into(), o),
                    })
                    .collect();
                (None, acts)
            }
            "secret" => {
                segs.push(seg(format!("{label}: enter it on a paired device"), st(GREY)));
                self.segs(&segs, 3);
                return;
            }
            _ => {
                segs.push(seg(format!("{label}: "), st(GREY)));
                let cur = txt(&v, self.now).0;
                segs.extend(self.typed(name, &cur, s(f, "placeholder")));
                (Some(Tap::Edit(name.into(), cur)), vec![])
            }
        };
        self.segs(&segs, 3);
        self.hot(id, start, tap, acts);
    }

    pub(crate) fn form(&mut self, b: &Value, id: &str) {
        let fields: Vec<&Value> = b.at("fields").as_array().into_iter().flatten().collect();
        for (i, f) in fields.iter().enumerate() {
            let fid = f.at("name").as_str().map_or_else(|| format!("{id}.f{i}"), String::from);
            self.field(f, &fid);
        }
        let submit = b.at("submit");
        if submit.is_object() && !fields.iter().any(|f| s(f, "type") == "secret") {
            let mut values = own(submit.at("values"));
            self.fields(b, &mut values);
            let sid = format!("{id}.submit");
            let start = self.out.lines.len();
            let act = self.send(s(submit, "label"), Some(id), submit.at("action"), values);
            let style = if self.on(&sid) {
                st(BLUE).add_modifier(Modifier::REVERSED)
            } else {
                st(BLUE)
            };
            self.segs(&[self.gut(&sid), seg(format!("[{}]", s(submit, "label")), style)], 2);
            self.hot(&sid, start, None, vec![act]);
        }
    }

    pub(crate) fn chips(&mut self, b: &Value, id: &str) {
        let start = self.out.lines.len();
        let sel: Vec<&Value> = b.at("selected").as_array().into_iter().flatten().collect();
        let multi = b.at("multi") == true;
        let mut segs = vec![self.gut(id)];
        let mut acts = vec![];
        for (i, o) in b.at("options").as_array().into_iter().flatten().enumerate() {
            let v = o.at("value");
            let on = sel.contains(&v);
            let style = if on {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                st(GREY)
            };
            let num = if b.get("action").is_some() {
                format!("{} ", i + 1)
            } else {
                String::new()
            };
            segs.extend([
                seg(num, st(BLUE)),
                seg(format!(" {} ", s(o, "label")), style),
                seg(" ", Style::default()),
            ]);
            let mut next: Vec<Value> = if !multi || v == "all" {
                vec![v.clone()]
            } else if on {
                sel.iter().filter(|x| **x != v).map(|x| (*x).clone()).collect()
            } else {
                let mut n: Vec<Value> = sel.iter().filter(|x| **x != "all").map(|x| (*x).clone()).collect();
                n.push(v.clone());
                n
            };
            if next.is_empty() {
                next = vec![json!("all")];
            }
            if let Some(a) = b.get("action") {
                let values = own(&json!({ s(b, "name"): next }));
                acts.push(self.send(s(o, "label"), Some(id), a, values));
            }
        }
        self.segs(&segs, 1);
        if !acts.is_empty() {
            self.hot(id, start, None, acts);
        }
    }

    pub(crate) fn composer(&mut self, b: &Value, id: &str) {
        for q in b.at("queued").as_array().into_iter().flatten() {
            self.segs(&[seg(format!("   queued: {}", s(q, "text")), st(GREY))], 3);
        }
        for c in b.at("chips").as_array().into_iter().flatten() {
            self.segs(&[seg(format!("   {}: {}", s(c, "name"), s(c, "cur")), st(GREY))], 3);
        }
        let start = self.out.lines.len();
        let text = self.ui.edits.get(id).map_or(String::new(), |v| txt(v, self.now).0);
        let mut segs = vec![self.gut(id), seg("› ", st(BLUE))];
        segs.extend(self.typed(id, &text, s(b, "placeholder")));
        self.segs(&segs, 3);
        let queue = s(b.at("acts"), "queue");
        let action = if b.at("working").is_object() && !queue.is_empty() {
            json!(queue)
        } else {
            b.at("action").clone()
        };
        let mut values = own(b.at("values"));
        values.insert("text".into(), json!(text));
        let label = if s(b, "label").is_empty() {
            "send"
        } else {
            s(b, "label")
        };
        let act = self.send(label, Some(id), &action, values);
        self.hot(id, start, Some(Tap::Edit(id.into(), text)), vec![act]);
    }
}
