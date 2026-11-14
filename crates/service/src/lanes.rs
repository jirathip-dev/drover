//! #529: lane state from the runtimes' event streams (docs/service.md): herdr's socket API, and shed's `events` with
//! `[runtime] kind = "shed"`. Each lane's change is logged as a passive `lane-state` (working, idle, done, blocked;
//! gone once its process exits) as it happens. While herdr's stream is up, its agent list (read again only when a
//! pane is created, updated, closed or exits, or its agent comes or goes) is the one every pass in this process reads.
//! A dropped stream logs `lane-stream` down and the passes poll `herdr agent list` again until it is back.
use crate::Service;
use drover_core::config::Config;
use drover_core::events::{append, working, Event};
use drover_core::{herdr, shed, Ctx};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// herdr's events after which its agent list is read again (and the per-pane status subscriptions renewed).
const PANES: [&str; 5] = [
    "pane.created",
    "pane.updated",
    "pane.closed",
    "pane.exited",
    "pane.agent_detected",
];
const RETRY_SECS: u64 = 5;

/// Each lane's last logged state, by agent name.
type Known = BTreeMap<String, String>;

impl Service {
    /// herdr's stream on `sock`, followed for good: down, polling; again in 5 s.
    pub(crate) async fn herdr(self: Arc<Self>, sock: PathBuf) {
        let mut known = Known::new();
        loop {
            let why = self.follow_herdr(&sock, &mut known).await;
            *herdr::STREAMED
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            self.stream_is("herdr", Some(why));
            tokio::time::sleep(Duration::from_secs(RETRY_SECS)).await;
        }
    }

    /// Subscribes (the pane events and each listed pane's status), then keeps the list from the events; renews on
    /// a pane event. Returns why the stream went down.
    async fn follow_herdr(&self, sock: &std::path::Path, known: &mut Known) -> String {
        let r = self.r;
        let list = || async move {
            match tokio::task::spawn_blocking(move || herdr::poll(r)).await {
                Ok(v) => v.map_err(|e| format!("{e:#}")),
                Err(e) => Err(e.to_string()),
            }
        };
        loop {
            let first = match list().await {
                Ok(v) => v,
                Err(e) => return e,
            };
            let c = match UnixStream::connect(sock).await {
                Ok(c) => c,
                Err(e) => return format!("herdr socket {}: {e}", sock.display()),
            };
            let mut subs: Vec<Value> = PANES.iter().map(|t| json!({"type": t})).collect();
            subs.extend(panes(&first).map(|p| json!({"type": "pane.agent_status_changed", "pane_id": p})));
            let req = json!({"id": "drover", "method": "events.subscribe", "params": {"subscriptions": subs}});
            let (rd, mut wr) = c.into_split();
            if let Err(e) = wr.write_all(format!("{req}\n").as_bytes()).await {
                return format!("herdr subscribe: {e}");
            }
            let mut lines = BufReader::new(rd).lines();
            let started = lines.next_line().await.ok().flatten().unwrap_or_default();
            let v: Value = serde_json::from_str(&started).unwrap_or_default();
            if v["result"]["type"] != "subscription_started" {
                return format!("herdr subscribe: {}", started.trim());
            }
            // Read again once subscribed: a change between the two reads is in this one; a new pane renews.
            let mut list = match list().await {
                Ok(v) => v,
                Err(e) => return e,
            };
            self.stream_is("herdr", None);
            self.publish(&list, known);
            if !panes(&list).eq(panes(&first)) {
                continue;
            }
            loop {
                let Ok(Some(l)) = lines.next_line().await else {
                    return "herdr: the event stream closed".into();
                };
                let v: Value = serde_json::from_str(&l).unwrap_or_default();
                let d = &v["data"];
                let pane = d["pane_id"].as_str().or(d["pane"]["pane_id"].as_str());
                let on = |a: &Value| pane.is_some_and(|p| a["pane_id"] == p);
                match v["event"].as_str().unwrap_or_default().replace('.', "_").as_str() {
                    "pane_agent_status_changed" => {
                        list.iter_mut()
                            .filter(|a| on(a))
                            .for_each(|a| a["agent_status"] = d["agent_status"].clone());
                        self.publish(&list, known);
                    }
                    "pane_closed" | "pane_exited" => {
                        list.retain(|a| !on(a));
                        self.publish(&list, known);
                        break;
                    }
                    "" => {}
                    _ => break,
                }
            }
        }
    }

    /// The list for this process's passes, and each lane's change since the last one as its `lane-state`.
    fn publish(&self, list: &[Value], known: &mut Known) {
        *herdr::STREAMED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(list.to_vec());
        let state = |a: &Value| {
            let s = a["agent_status"].as_str().unwrap_or("unknown");
            if a["agent"].is_null() { "gone" } else { s }.to_string()
        };
        let now: Known = list
            .iter()
            .filter_map(|a| Some((a["name"].as_str()?.to_string(), state(a))))
            .collect();
        let gone = known.keys().filter(|k| !now.contains_key(*k)).map(|k| (k, "gone"));
        let changed = now
            .iter()
            .filter(|(k, s)| known.get(*k) != Some(s))
            .map(|(k, s)| (k, s.as_str()));
        let diff: Vec<(String, &str)> = changed.chain(gone).map(|(k, s)| (k.clone(), s)).collect();
        if !diff.is_empty() {
            let events = working(&self.ctx.state).unwrap_or_default();
            for (agent, s) in diff {
                let spawn = events
                    .iter()
                    .rfind(|e| e.kind == "spawn" && e.str("agent") == Some(&agent));
                if let Some(sp) = spawn {
                    let d = json!({"agent": agent, "state": s, "runtime": "herdr"});
                    let _ = append(
                        &self.ctx.state,
                        &Event::new((self.clock)(), &sp.fleet, sp.issue, "lane-state", d),
                    );
                }
            }
        }
        *known = now;
    }

    /// shed's `events` on `w`, followed for good on this thread: each lane's state logged, `shed.seq` kept.
    pub(crate) fn shed(self: Arc<Self>, w: &shed::Wire) {
        loop {
            let ctx = Ctx {
                now: (self.clock)(),
                ..self.ctx.clone()
            };
            let why = Config::load(&ctx.config).and_then(|cfg| {
                shed::follow(w, shed::seen(&ctx), None, |e| match e {
                    None => self.stream_is("shed", None),
                    Some(e) => {
                        let c = Ctx {
                            now: (self.clock)(),
                            ..ctx.clone()
                        };
                        let _ = shed::log(&c, &cfg, &e);
                        let _ = std::fs::write(c.state.with_file_name("shed.seq"), e.seq.to_string());
                    }
                })
            });
            let why = why
                .err()
                .map_or("shed: the event stream closed".into(), |e| format!("{e:#}"));
            self.stream_is("shed", Some(why));
            std::thread::sleep(Duration::from_secs(RETRY_SECS));
        }
    }

    /// `lane-stream` once per change of a runtime's stream: live, or down (`why`) while the passes poll.
    fn stream_is(&self, runtime: &'static str, down: Option<String>) {
        let mut s = self.streams.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if s.insert(runtime, down.is_none()) == Some(down.is_none()) {
            return;
        }
        let d = match down {
            None => json!({"runtime": runtime, "state": "live"}),
            Some(why) => json!({"runtime": runtime, "state": "down", "why": why}),
        };
        let _ = append(&self.ctx.state, &self.event("lane-stream", d));
    }
}

fn panes(list: &[Value]) -> impl Iterator<Item = &str> {
    list.iter().filter_map(|a| a["pane_id"].as_str())
}
