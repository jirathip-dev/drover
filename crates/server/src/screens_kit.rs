use blocks as k;
use k::{Act, Confirm, Label, Shown, Tap, Tone, Toned};
use serde_json::{Map, Value};

pub(super) trait Kind {
    fn block(self) -> k::Block;
}

macro_rules! kinds {
    ($($t:ident),*) => {
        $(impl Kind for k::$t {
            fn block(self) -> k::Block {
                k::Block::$t(Box::new(self))
            }
        })*
    };
}

kinds!(Screen, Section, List, Row, Card, Choice, Run, Button, Chips, Stat, Text, Images, Diagram, Composer, Scene);

impl Kind for k::Modal {
    fn block(self) -> k::Block {
        k::Block::Sheet(Box::new(self))
    }
}

impl Kind for k::Form {
    fn block(self) -> k::Block {
        k::Block::Form(Box::new(self))
    }
}

pub(super) fn wire(v: &impl serde::Serialize) -> Value {
    serde_json::to_value(v).unwrap_or_default()
}

pub(super) fn typed(v: Value) -> k::Block {
    let kind = serde_json::from_value::<k::Block>(v.clone()).ok();
    kind.filter(|b| wire(b) == v).unwrap_or_else(|| {
        let mut more = match v {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        let kind = more
            .remove("type")
            .and_then(|t| t.as_str().map(String::from))
            .unwrap_or_default();
        let id = more.remove("id").and_then(|t| t.as_str().map(String::from));
        k::Block::App(Box::new(k::AppBlock {
            kind,
            id,
            fallback: None,
            more,
        }))
    })
}

pub(super) fn fields<const N: usize>(kv: [(&str, Value); N]) -> k::Fields {
    kv.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

pub(super) fn word<T: serde::de::DeserializeOwned + Default>(w: &str) -> T {
    serde_json::from_value(Value::from(w)).unwrap_or_default()
}

pub(super) fn label(text: impl Into<Shown>, t: Tone) -> Label {
    Label {
        text: text.into(),
        tone: Some(t),
    }
}

pub(super) fn toned(text: impl Into<Shown>, t: Tone) -> Toned {
    Toned::Label(label(text, t))
}

pub(super) fn plain(text: impl Into<String>) -> Toned {
    Toned::Text(text.into())
}

pub(super) fn open(path: impl Into<String>) -> Tap {
    Tap {
        open: Some(path.into()),
        ..Tap::default()
    }
}

pub(super) fn sheet(path: impl Into<String>) -> Tap {
    Tap {
        sheet: Some(path.into()),
        ..Tap::default()
    }
}

pub(super) fn url(u: impl Into<String>) -> Tap {
    Tap {
        url: Some(u.into()),
        ..Tap::default()
    }
}

pub(super) fn thread(t: impl Into<String>) -> Tap {
    Tap {
        thread: Some(t.into()),
        ..Tap::default()
    }
}

pub(super) fn act(label: impl Into<String>, action: impl Into<String>, values: k::Fields) -> Act {
    Act {
        label: label.into(),
        action: Some(action.into()),
        values: Some(values),
        ..Act::default()
    }
}

pub(super) fn confirm(title: impl Into<String>, ok: impl Into<String>) -> Confirm {
    Confirm {
        title: title.into(),
        ok: Some(ok.into()),
        ..Confirm::default()
    }
}

pub(super) fn button(id: impl Into<String>, label: impl Into<String>, action: &str, values: k::Fields) -> k::Button {
    k::Button::new(label).id(id).action(action).values(values)
}

pub(super) fn titled(id: impl Into<String>, title: impl Into<String>) -> k::Row {
    k::Row::new(title).id(id)
}

pub(super) fn text(id: impl Into<String>, markdown: impl Into<String>) -> k::Block {
    k::Text::new().id(id).markdown(markdown).block()
}

fn own(v: &Value) -> Map<String, Value> {
    let mut m = v.as_object().cloned().unwrap_or_default();
    m.remove("blocks");
    m.remove("rows");
    m
}

fn kids(v: &Value) -> &[Value] {
    v.as_array()
        .or_else(|| v.get("blocks").or_else(|| v.get("rows")).and_then(Value::as_array))
        .map_or(&[], Vec::as_slice)
}

fn replace(id: &str, b: &Value) -> k::Op {
    k::Op::Replace {
        id: id.into(),
        block: k::Child::Block(typed(b.clone())),
    }
}

fn walk(a: &Value, b: &Value, out: &mut Vec<k::Op>) -> bool {
    let (id, mark) = (b["id"].as_str(), out.len());
    let (oa, ob) = (own(a), own(b));
    if oa != ob {
        let Some(id) = id else { return false };
        if oa.keys().any(|k| !ob.contains_key(k)) {
            out.push(replace(id, b));
            return true;
        }
        let fields: Map<String, Value> = ob.into_iter().filter(|(k, v)| oa.get(k) != Some(v)).collect();
        out.push(k::Op::Merge { id: id.into(), fields });
    }
    if children(id, kids(a), kids(b), out) {
        return true;
    }
    out.truncate(mark);
    id.is_some_and(|id| (out.push(replace(id, b)), true).1)
}

fn children(into: Option<&str>, ka: &[Value], kb: &[Value], out: &mut Vec<k::Op>) -> bool {
    fn ids(k: &[Value]) -> Option<Vec<&str>> {
        k.iter().map(|c| c["id"].as_str()).collect()
    }
    fn both<'a>(x: &[&'a str], y: &[&str]) -> Vec<&'a str> {
        x.iter().filter(|i| y.contains(i)).copied().collect()
    }
    let (Some(into), Some(ia), Some(ib)) = (into, ids(ka), ids(kb)) else {
        return ka.len() == kb.len() && ka.iter().zip(kb).all(|(x, y)| x["id"] == y["id"] && walk(x, y, out));
    };
    if both(&ia, &ib) != both(&ib, &ia) {
        return false;
    }
    out.extend(
        ia.iter()
            .filter(|i| !ib.contains(i))
            .map(|i| k::Op::Remove { id: (*i).into() }),
    );
    for (k, (i, c)) in ib.iter().zip(kb).enumerate() {
        if let Some(p) = ia.iter().position(|x| x == i).and_then(|p| ka.get(p)) {
            walk(p, c, out);
            continue;
        }
        out.push(k::Op::Insert {
            into: into.into(),
            before: ib.iter().skip(k + 1).find(|x| ia.contains(x)).map(|x| (*x).into()),
            after: None,
            block: k::Child::Block(typed(c.clone())),
        });
    }
    true
}

/// The ops that turn screen `a` into `b`, by block id (removes first); a change no id can address: the whole screen.
pub fn ops(a: &Value, b: &Value) -> Vec<Value> {
    let mut out = vec![];
    let root = b["id"].as_str();
    let whole = |o: &k::Op| matches!(o, k::Op::Replace { id, .. } if Some(id.as_str()) == root);
    if a["id"] != b["id"] || !walk(a, b, &mut out) || out.iter().any(whole) {
        if !b.is_object() {
            return vec![Value::Object(fields([("op", "screen".into()), ("screen", b.clone())]))];
        }
        out = vec![k::Op::Screen {
            screen: typed(b.clone()),
        }];
    }
    out.sort_by_key(|o| !matches!(o, k::Op::Remove { .. }));
    out.iter().map(wire).collect()
}
