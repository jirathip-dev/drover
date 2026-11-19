use drover_core::clock;
use drover_server::serve::{Done, Local};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

const TICK: Duration = Duration::from_millis(500);
const REBUILD: Duration = Duration::from_secs(30);

pub(crate) enum Ask {
    Watch(Vec<String>),
    Kick,
    More(String),
}

pub(crate) enum Note {
    Tree(String, Option<Value>),
    Page(String, Option<Value>),
    Done(Result<Done, String>, bool),
}

fn has_id(v: &Value, id: &str) -> bool {
    match v {
        Value::Object(m) => m.get("id").and_then(Value::as_str) == Some(id) || m.values().any(|x| has_id(x, id)),
        Value::Array(a) => a.iter().any(|x| has_id(x, id)),
        _ => false,
    }
}

pub(crate) fn run(door: &Local, now: &dyn Fn() -> u64, rx: &Receiver<Ask>, tx: &Sender<Note>) {
    let (mut watch, mut last) = (Vec::<String>::new(), BTreeMap::<String, Option<Value>>::new());
    let (mut seen, mut kicked, mut built) = (None, false, clock::instant());
    loop {
        door.refresh(now());
        let stamp = Some(door.stamp());
        if stamp != seen || kicked || built.elapsed() >= REBUILD {
            (seen, kicked, built) = (stamp, false, clock::instant());
            for p in &watch {
                let tree = door.screen(now(), p);
                kicked |= tree.as_ref().is_some_and(|t| has_id(t, "loading"));
                if last.get(p) != Some(&tree) {
                    last.insert(p.clone(), tree.clone());
                    if tx.send(Note::Tree(p.clone(), tree)).is_err() {
                        return;
                    }
                }
            }
        }
        match rx.recv_timeout(TICK) {
            Ok(Ask::Watch(w)) => {
                last.retain(|k, _| w.contains(k));
                (watch, kicked) = (w, true);
            }
            Ok(Ask::Kick) => kicked = true,
            Ok(Ask::More(p)) => {
                let page = door.screen(now(), &p);
                tx.send(Note::Page(p, page)).ok();
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}
