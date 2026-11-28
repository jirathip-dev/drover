use super::{header, hung_up, sent, wire, working, Ctx, Event, Server};
use crate::api::{RowView, Rows};
use crate::thread::{self, Row, Target};
use std::collections::BTreeMap;
use std::io::Write;
use std::net::TcpStream;
use std::time::Duration;

const MOST: usize = 500;

pub(super) fn is(p: &str) -> bool {
    p == "rows" || p.strip_prefix("thread/").is_some_and(|t| t.ends_with("/rows"))
}

fn view(ctx: &Ctx, events: &[Event], (subject, row): (&Event, Row)) -> RowView {
    let id = match &row.to {
        Target::Thread { id } => id.clone(),
        _ => String::new(),
    };
    let handoff = (subject.kind == "thread-handoff").then(|| super::v1::handoff(events, subject, false));
    let epic = (subject.kind == "epic-landed")
        .then(|| serde_json::from_value(serde_json::Value::Object(subject.data.clone())));
    RowView {
        block: super::chat::drawn(ctx, events, subject, &row, &id),
        row,
        handoff,
        epic: epic.and_then(Result::ok),
    }
}

pub(super) fn get(ctx: &Ctx, p: &str, f: &BTreeMap<String, String>) -> Rows {
    let events = working(&ctx.state).unwrap_or_default();
    let n = |k: &str| f.get(k).and_then(|n| n.parse::<u64>().ok());
    let cap = n("n").map_or(MOST, |n| usize::try_from(n).unwrap_or(MOST).clamp(1, MOST));
    let seq = events.last().map_or(0, |e| e.seq);
    let (more, list) = match p.strip_prefix("thread/").and_then(|t| t.strip_suffix("/rows")) {
        Some(id) => {
            let to = Target::Thread { id: id.into() };
            let before = n("before").unwrap_or(u64::MAX);
            let mut all: Vec<_> = thread::rows(&events);
            all.retain(|r| r.1.to == to && r.1.seq < before);
            let skip = all.len().saturating_sub(cap);
            (skip > 0, all.into_iter().skip(skip).collect::<Vec<_>>())
        }
        None if f.get("open").is_some_and(|o| o == "1") => (false, thread::open(&events)),
        None => {
            let since = n("since").unwrap_or(0);
            let mut all = thread::rows(&events);
            all.retain(|r| r.1.seq > since);
            (all.len() > cap, all.into_iter().take(cap).collect())
        }
    };
    let rows = list.into_iter().map(|r| view(ctx, &events, r)).collect();
    Rows { seq, more, rows }
}

impl Server<'_> {
    pub(super) fn rows_stream(&self, s: &mut TcpStream, req: &str) -> std::io::Result<()> {
        let head = req.split("\r\n\r\n").next().unwrap_or_default();
        let dev = crate::devices::check(self.ctx, header(head, "cookie").unwrap_or_default());
        if !(Self::plugin(self.ctx, head) || dev.is_ok() && self.allowed(self.ctx, head)) {
            let body = r#"{"ok":false,"code":"UNPAIRED","message":"the plugin token or a paired device"}"#;
            let n = body.len();
            return write!(
                s,
                "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {n}\r\nConnection: close\r\n\r\n{body}"
            );
        }
        let ctx = || Ctx {
            now: crate::now(),
            ..self.ctx.clone()
        };
        let last = || crate::db::last_seq(&self.ctx.state);
        let mut at = header(head, "last-event-id")
            .and_then(|n| n.trim().parse::<u64>().ok())
            .unwrap_or_else(last);
        let mut msg = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\n\r\n: rows\n\n"
            .to_string();
        let mut z = wire::Stream::new(head);
        let mut ping = drover_core::clock::instant();
        loop {
            let end = drover_core::clock::instant() + Duration::from_secs(10);
            if !msg.is_empty() && !z.frame(&msg).is_ok_and(|b| sent(s, &b, end)) {
                return Ok(());
            }
            msg.clear();
            if hung_up(s) {
                return Ok(());
            }
            if last() > at {
                let (c, events) = (ctx(), working(&self.ctx.state).unwrap_or_default());
                let mut fresh = thread::rows(&events);
                fresh.retain(|r| r.1.seq > at);
                if fresh.len() <= MOST {
                    at = at.max(events.last().map_or(at, |e| e.seq));
                }
                for r in fresh.into_iter().take(MOST) {
                    let v = view(&c, &events, r);
                    let id = v.row.seq;
                    msg += &format!("id: {id}\ndata: {}\n\n", serde_json::to_string(&v).unwrap_or_default());
                    at = at.max(id);
                }
            }
            if msg.is_empty() && ping.elapsed() >= Duration::from_secs(5) {
                (msg, ping) = (": ping\n\n".into(), drover_core::clock::instant());
            }
            if msg.is_empty() {
                drover_core::clock::sleep(Duration::from_millis(500));
            }
        }
    }
}
