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
