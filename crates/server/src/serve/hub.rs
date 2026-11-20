use super::screens::{line, ops, pages, Res};
use super::{form, hash, header, hung_up, lock, sent, wire, Auth, Ctx, Server, JSON, VER};
use crate::api::ScreenKey;
use crate::events::Json;
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::net::TcpStream;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

const POLL: Duration = Duration::from_millis(25);
const BUILDS: usize = 2;
const UNHELD: usize = 64;
const UNHELD_SECS: u64 = 60;
const FRAMES: usize = 64;
const FRAME_BYTES: usize = 1 << 20;
const STREAMS_PER_DEVICE: usize = 16;
const LOG: u8 = 1;
const READS: u8 = 1 << 2;
const CONFIG: u8 = 1 << 3;
const CLOCK: u8 = 1 << 5;
const ALL: u8 = 0b11_1111;

type Q = BTreeMap<String, String>;
type Frame = (u64, u64, Arc<str>);

/// One screen as every client of its kind sees it: its path, its query without empty values, paired or not.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key {
    path: String,
    q: Q,
    paired: bool,
}

impl Key {
    fn new(path: &str, q: &Q, paired: bool) -> Key {
        let q = q.iter().filter(|(_, v)| !v.is_empty());
        let q = q.map(|(k, v)| (k.clone(), v.clone())).collect();
        Key {
            path: path.into(),
            q,
            paired,
        }
    }

    fn wire(&self) -> String {
        let q: Vec<String> = self.q.iter().map(|(k, v)| format!("{k}={v}")).collect();
        [self.path.clone(), q.join("&")].join(if q.is_empty() { "" } else { "?" })
    }

    fn deps(&self) -> u8 {
        match self.path.as_str() {
            p if p == "/screens/threads" || p.starts_with("/screens/threads/") => LOG | READS | CONFIG | CLOCK,
            "/screens/proxy-token" => CONFIG,
            _ => ALL,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
struct Signals([u64; 6]);

impl Signals {
    fn moved(self, now: Signals, deps: u8) -> bool {
        self.0
            .iter()
            .zip(now.0)
            .enumerate()
            .any(|(i, (a, b))| deps >> i & 1 == 1 && *a != b)
    }
}

#[derive(Default)]
struct Entry {
    built: bool,
    rev: u64,
    json: Option<Arc<str>>,
    seen: Signals,
    building: bool,
    holders: Vec<Weak<Queue>>,
    read_at: u64,
    builds: u64,
    ms: VecDeque<u64>,
}

/// One stream's outbound frames (from, rev, ops): the hub pushes, the stream's thread writes.
#[derive(Default)]
pub struct Queue {
    out: Mutex<(VecDeque<Frame>, usize, bool)>,
    wake: Condvar,
}

impl Queue {
    fn push(&self, f: Frame) {
        let mut o = lock(&self.out);
        if o.0.len() >= FRAMES || o.1 + f.2.len() > FRAME_BYTES {
            *o = (VecDeque::new(), 0, true);
        } else {
            o.1 += f.2.len();
            o.0.push_back(f);
        }
        self.wake.notify_one();
    }

    fn take(&self, wait: Duration) -> (VecDeque<Frame>, bool) {
        let o = lock(&self.out);
        let mut o = (self.wake.wait_timeout_while(o, wait, |o| o.0.is_empty() && !o.2))
            .unwrap_or_else(PoisonError::into_inner)
            .0;
        let (frames, _, lag) = std::mem::take(&mut *o);
        (frames, lag)
    }
}

/// Every screen built once per change, for all its readers; one publisher rebuilds the held ones.
pub struct Hub {
    keys: Mutex<BTreeMap<Key, Entry>>,
    built: Condvar,
    slots: (Mutex<usize>, Condvar),
    kicked: (Mutex<bool>, Condvar),
    epoch: String,
}

struct Slot<'h>(&'h Hub);

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        *lock(&self.0.slots.0) -= 1;
        self.0.slots.1.notify_one();
    }
}

fn wait<'m, T>(c: &Condvar, g: MutexGuard<'m, T>) -> MutexGuard<'m, T> {
    c.wait(g).unwrap_or_else(PoisonError::into_inner)
}

impl Hub {
    pub fn new() -> Hub {
        Hub {
            keys: Mutex::default(),
            built: Condvar::new(),
            slots: (Mutex::new(0), Condvar::new()),
            kicked: (Mutex::new(false), Condvar::new()),
            epoch: format!(
                "{:08x}",
                hash(&(drover_core::clock::instant(), crate::now())) & 0xffff_ffff
            ),
        }
    }

    /// Wake the publisher now: something was written.
    pub fn kick(&self) {
        *lock(&self.kicked.0) = true;
        self.kicked.1.notify_one();
    }

    /// The screen `k` as of now, built only when stale and nobody else is building it; its rev and JSON.
    fn read(&self, s: &Server, ctx: &Ctx, k: &Key) -> Option<(u64, Arc<str>)> {
        let now = s.signals(ctx);
        let mut m = lock(&self.keys);
        let old = loop {
            let e = m.entry(k.clone()).or_default();
            e.read_at = ctx.now;
            if e.built && !e.seen.moved(now, k.deps()) {
                return e.json.clone().map(|j| (e.rev, j));
            }
            if !e.building {
                e.building = true;
                break e.json.clone();
            }
            m = wait(&self.built, m);
        };
        Self::trim(&mut m, ctx.now);
        drop(m);
        self.build(s, ctx, k, now, old.as_deref())
    }

    fn build(&self, s: &Server, ctx: &Ctx, k: &Key, now: Signals, old: Option<&str>) -> Option<(u64, Arc<str>)> {
        let t = drover_core::clock::instant();
        let new = {
            let mut n = lock(&self.slots.0);
            while *n >= BUILDS {
                n = wait(&self.slots.1, n);
            }
            *n += 1;
            drop(n);
            let _slot = Slot(self);
            crate::runner::request(|| super::build(|| s.build_screen(ctx, &k.path, &k.q, k.paired)))
        };
        let text: Option<Arc<str>> = new.as_ref().map(|v| v.to_string().into());
        let delta = old.zip(new.as_ref()).filter(|(o, _)| Some(*o) != text.as_deref());
        let delta = delta.and_then(|(o, new)| {
            let o: Value = serde_json::from_str(o).ok()?;
            let mut d = ops(&o, new);
            let badge = new.at("badge").as_u64().filter(|n| Some(*n) != o.at("badge").as_u64());
            d.extend(badge.map(|n| serde_json::json!({"op": "badge", "count": n})));
            Some(d).filter(|d| !d.is_empty())
        });
        let mut m = lock(&self.keys);
        let e = m.entry(k.clone()).or_default();
        (e.built, e.building, e.seen, e.builds) = (true, false, now, e.builds + 1);
        e.ms.push_back(t.elapsed().as_millis().try_into().unwrap_or(u64::MAX));
        if e.ms.len() > 32 {
            e.ms.pop_front();
        }
        if e.json.is_none() && text.is_some() || delta.is_some() {
            e.rev += 1;
        }
        if let Some(d) = delta {
            let f: Frame = (e.rev - 1, e.rev, Value::Array(d).to_string().into());
            e.holders.retain(|h| h.upgrade().map(|q| q.push(f.clone())).is_some());
        }
        e.json.clone_from(&text);
        let out = text.map(|j| (e.rev, j));
        drop(m);
        self.built.notify_all();
        s.mark_read(ctx, &k.path, k.paired);
        out
    }

    fn trim(m: &mut BTreeMap<Key, Entry>, now: u64) {
        let idle = |e: &Entry| e.holders.is_empty() && !e.building;
        m.retain(|_, e| !idle(e) || e.read_at + UNHELD_SECS >= now);
        let mut unheld: Vec<(u64, Key)> = m
            .iter()
            .filter(|e| idle(e.1))
            .map(|(k, e)| (e.read_at, k.clone()))
            .collect();
        unheld.sort();
        for (_, k) in unheld.iter().rev().skip(UNHELD) {
            m.remove(k);
        }
    }

    fn hold(&self, k: &Key, q: &Arc<Queue>) {
        lock(&self.keys)
            .entry(k.clone())
            .or_default()
            .holders
            .push(Arc::downgrade(q));
        self.kick();
    }

    fn pass(&self, s: &Server) -> bool {
        let ctx = Ctx {
            now: crate::now(),
            ..s.ctx.clone()
        };
        let mut held: Vec<(usize, Key)> = {
            let mut m = lock(&self.keys);
            m.values_mut().for_each(|e| e.holders.retain(|h| h.strong_count() > 0));
            Self::trim(&mut m, ctx.now);
            m.iter()
                .map(|(k, e)| (e.holders.len(), k.clone()))
                .filter(|h| h.0 > 0)
                .collect()
        };
        held.sort_by_key(|h| std::cmp::Reverse(h.0));
        for (_, k) in &held {
            drop(self.read(s, &ctx, k));
        }
        !held.is_empty()
    }

    /// The publisher: asleep while no stream holds a key, else a pass each 25 ms or at once on `kick`.
    pub fn publish(&self, s: &Server) {
        loop {
            let held = self.pass(s);
            let k = lock(&self.kicked.0);
            let mut k = if held {
                let w = self.kicked.1.wait_timeout_while(k, POLL, |k| !*k);
                w.unwrap_or_else(PoisonError::into_inner).0
            } else {
                (self.kicked.1.wait_while(k, |k| !*k)).unwrap_or_else(PoisonError::into_inner)
            };
            *k = false;
        }
    }

    /// Each key kept: its size, rev, holders, builds and build times.
    pub fn keys(&self) -> Vec<ScreenKey> {
        let pct = |ms: &VecDeque<u64>, p: usize| {
            let mut v: Vec<u64> = ms.iter().copied().collect();
            v.sort_unstable();
            v.get((v.len() * p / 100).min(v.len().saturating_sub(1)))
                .copied()
                .unwrap_or(0)
        };
        let m = lock(&self.keys);
        m.iter()
            .map(|(k, e)| ScreenKey {
                key: k.wire(),
                paired: k.paired,
                bytes: e.json.as_ref().map_or(0, |j| j.len().try_into().unwrap_or(u64::MAX)),
                rev: e.rev,
                holders: e
                    .holders
                    .iter()
                    .filter(|h| h.strong_count() > 0)
                    .count()
                    .try_into()
                    .unwrap_or(u64::MAX),
                builds: e.builds,
                p50: pct(&e.ms, 50),
                p95: pct(&e.ms, 95),
            })
            .collect()
    }
}

fn frame(seq: u64, ops: &str) -> String {
    format!("id: {seq}\ndata: {{\"build\":\"{VER:08x}\",\"ops\":{ops},\"seq\":{seq}}}\n\n")
}

fn whole(j: &str) -> String {
    let badge = serde_json::from_str::<Value>(j)
        .ok()
        .and_then(|v| v.at("badge").as_u64());
    let badge = badge.map(|n| format!(",{{\"count\":{n},\"op\":\"badge\"}}"));
    format!("[{{\"op\":\"screen\",\"screen\":{j}}}{}]", badge.unwrap_or_default())
}

struct StreamSlot<'a>(&'a Mutex<BTreeMap<String, usize>>, String);

impl<'a> StreamSlot<'a> {
    fn take(m: &'a Mutex<BTreeMap<String, usize>>, who: String) -> Option<Self> {
        let mut held = lock(m);
        let n = held.entry(who.clone()).or_default();
        (*n < STREAMS_PER_DEVICE).then(|| {
            *n = n.saturating_add(1);
            Self(m, who)
        })
    }
}

impl Drop for StreamSlot<'_> {
    fn drop(&mut self) {
        let mut held = lock(self.0);
        if let Some(n) = held.get_mut(&self.1) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                held.remove(&self.1);
            }
        }
    }
}

impl Server<'_> {
    fn signals(&self, ctx: &Ctx) -> Signals {
        let at = |p: &std::path::Path| hash(&std::fs::metadata(p).and_then(|m| m.modified()).ok());
        let reads = if self.r.defer {
            self.r.changes.load(Relaxed)
        } else {
            self.r.changes.fetch_add(1, Relaxed)
        };
        Signals([
            crate::db::last_seq(&ctx.state),
            crate::db::stamp(&ctx.state, "board").unwrap_or(0),
            reads,
            at(&ctx.config),
            at(&ctx.state.with_file_name("cache")),
            ctx.now / 30,
        ])
    }

    /// `GET /screens/<x>`: the hub's copy with its `ETag` (`extra`), or 304 when the client holds it.
    pub(super) fn screen_get(
        &self,
        ctx: &Ctx,
        head: &str,
        path: &str,
        query: &str,
        auth: &Auth,
        extra: &mut String,
    ) -> Res {
        let plugin = Self::plugin(ctx, head);
        if !plugin && !self.allowed(ctx, head) {
            return line("403 Forbidden", "foreign Host or Origin");
        }
        let paired = plugin || auth.is_ok();
        if path.starts_with("/screens/lanes/") && !paired {
            return line("401 Unauthorized", "pair this device first");
        }
        let Some((rev, json)) = self.hub.read(self, ctx, &Key::new(path, &form(query), paired)) else {
            return line("404 Not Found", "no such screen");
        };
        let tag = format!("\"{}.{rev}\"", self.hub.epoch);
        *extra += &format!("ETag: {tag}\r\n");
        if header(head, "if-none-match") == Some(tag.as_str()) {
            return ("304 Not Modified", JSON, vec![]);
        }
        ("200 OK", JSON, json.as_bytes().to_vec())
    }

    /// `GET /screens/<x>/stream`: the screen, then its deltas as the hub publishes them, in `{seq, build, ops}` frames.
    pub(super) fn stream(&self, s: &mut TcpStream, req: &str, path: &str, query: &str) -> std::io::Result<()> {
        let (head, base) = (
            req.split("\r\n\r\n").next().unwrap_or_default(),
            path.trim_end_matches("/stream"),
        );
        let ctx = || Ctx {
            now: crate::now(),
            ..self.ctx.clone()
        };
        let plugin = Self::plugin(self.ctx, head);
        let dev = crate::devices::check(self.ctx, header(head, "cookie").unwrap_or_default());
        let paired = plugin || dev.is_ok();
        let who = match dev {
            _ if plugin => "cooee".into(),
            Ok(d) => d.hash,
            Err(_) => s.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default(),
        };
        let slot = StreamSlot::take(&self.streams, who);
        let q = form(query);
        let mine = Arc::new(Queue::default());
        let first = (slot.is_some() && (plugin || self.allowed(self.ctx, head)))
            .then(|| self.hub.read(self, &ctx(), &Key::new(base, &q, paired)))
            .flatten()
            .and_then(|(_, j)| {
                let key = Key::new(base, &pages::window(q, &serde_json::from_str(&j).ok()?), paired);
                self.hub.hold(&key, &mine);
                Some((self.hub.read(self, &ctx(), &key)?, key))
            });
        let Some(((mut have, json), key)) = first else {
            let r = line(
                if slot.is_some() {
                    "404 Not Found"
                } else {
                    "503 Service Unavailable"
                },
                "no stream",
            );
            let h = format!(
                "HTTP/1.1 {}\r\nContent-Type: {JSON}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                r.0,
                r.2.len()
            );
            return std::io::Write::write_all(s, &[h.as_bytes(), &r.2].concat());
        };
        let mut seq = header(head, "last-event-id")
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(0)
            + 1;
        let mut msg = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\n\r\n"
            .to_string()
            + &frame(seq, &whole(&json));
        let mut ping = drover_core::clock::instant();
        let mut z = wire::Stream::new(head);
        let mut missed = 0;
        'out: loop {
            let end = drover_core::clock::instant() + Duration::from_secs(10);
            if !z.frame(&msg).is_ok_and(|b| sent(s, &b, end)) {
                break;
            }
            msg.clear();
            while msg.is_empty() {
                if hung_up(s) {
                    break 'out;
                }
                let (frames, mut lag) = mine.take(Duration::from_millis(500));
                for (from, rev, ops) in &frames {
                    if *rev <= have {
                        continue;
                    }
                    lag |= *from != have;
                    if !lag {
                        seq += 1;
                        msg += &frame(seq, ops);
                        have = *rev;
                    }
                }
                if let Some((rev, j)) = self.hub.read(self, &ctx(), &key).filter(|r| lag && r.0 != have) {
                    seq += 1;
                    msg += &frame(seq, &whole(&j));
                    have = rev;
                }
                if msg.is_empty() && ping.elapsed() >= Duration::from_secs(5) {
                    missed = if drover_sys::unsent(s) > 0 { missed + 1 } else { 0 };
                    if missed >= 2 {
                        break 'out;
                    }
                    (msg, ping) = (": ping\n\n".into(), drover_core::clock::instant());
                }
            }
        }
        Ok(())
    }
}
