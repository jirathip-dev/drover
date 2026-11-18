use super::{form, Q};
use crate::events::Json;
use serde_json::{json, Value};

const PAGE: usize = 100;

pub(super) fn page<T>(path: &str, q: &Q, rows: impl IntoIterator<Item = ((u64, u64), T)>) -> (Vec<T>, Option<String>) {
    let mut rows: Vec<_> = rows.into_iter().collect();
    let key = |k: &str| {
        let (a, b) = q.get(k)?.split_once('.')?;
        Some((a.parse().ok()?, b.parse().ok()?))
    };
    let (after, to) = (key("after"), key("to"));
    rows.sort_by_key(|r| std::cmp::Reverse(r.0));
    rows.retain(|r| after.is_none_or(|c| r.0 < c));
    let n = to.map_or(PAGE, |c| rows.iter().take_while(|r| r.0 >= c).count());
    let left = rows.len() > n;
    rows.truncate(n);
    let next = to.or_else(|| rows.last().map(|r| r.0)).filter(|_| left).map(|(a, b)| {
        let mut q = q.clone();
        q.remove("to");
        q.insert("after".into(), format!("{a}.{b}"));
        let q: Vec<String> = q.iter().map(|(k, v)| format!("{k}={}", super::super::enc(v))).collect();
        format!("{path}?{}", q.join("&"))
    });
    (rows.into_iter().map(|r| r.1).collect(), next)
}

pub(super) fn paged(mut block: Value, more: Option<String>) -> Value {
    if let Some(m) = more {
        block.put("more", json!(m));
    }
    block
}

pub(super) fn window(mut q: Q, first: &Value) -> Q {
    if let Some(c) = cursor(first) {
        q.insert("to".into(), c);
    }
    q
}

fn cursor(v: &Value) -> Option<String> {
    match v {
        Value::Object(m) => match m.get("more").and_then(Value::as_str) {
            Some(p) => form(p.split_once('?')?.1).remove("after"),
            None => m.values().find_map(cursor),
        },
        Value::Array(a) => a.iter().find_map(cursor),
        _ => None,
    }
}
