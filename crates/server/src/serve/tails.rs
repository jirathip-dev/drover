use super::{lanes, lock, working, Config, Ctx, Server};
use crate::api::Block;
use crate::session::transcript::{archived, of, Transcript};
use crate::shed::proto::Logged;
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;
use std::time::Duration;

const SHOWN: u64 = 200;
const CHUNK: u64 = 50;
const DEEP: usize = 5000;
const HEAD: usize = 3;
const WATCHED_BYTES: usize = 512 << 10;
const ALL_BYTES: usize = 16 << 20;
const WATCH_SECS: u64 = 600;
const COLD_SECS: u64 = 30;

pub(super) fn window<T: std::fmt::Debug>(v: &[T], n: usize, kb: usize) -> &[T] {
    let (mut size, mut k) = (0, 0);
    for x in v.iter().rev().take(n) {
        size += format!("{x:?}").len();
        k += usize::from(k == 0 || size <= kb << 10);
    }
    v.get(v.len() - k..).unwrap_or_default()
}

#[derive(Default)]
struct Tail {
    gen: u32,
    lines: VecDeque<(u64, String)>,
    bytes: usize,
    acp: bool,
    more: bool,
    deep: bool,
    watched_at: u64,
    read_at: u64,
    ver: u64,
}

impl Tail {
    fn watched(&self, now: u64) -> bool {
        self.watched_at > 0 && self.watched_at + WATCH_SECS >= now
    }

    fn pop(&mut self) -> usize {
        let n = self.lines.pop_front().map_or(0, |l| l.1.len());
        self.bytes = self.bytes.saturating_sub(n);
        self.more = true;
        n
    }

    fn replace(&mut self, lines: VecDeque<(u64, String)>, more: bool) {
        self.gen = self.gen.saturating_add(1);
        self.bytes = lines.iter().map(|l| l.1.len()).sum();
        (self.lines, self.more) = (lines, more);
    }

    fn screen(&mut self, new: Vec<String>, asked: usize) -> bool {
        let (h, len) = (new.len().min(HEAD), self.lines.len());
        let head = |p: &usize| self.lines.iter().skip(*p).take(h).map(|l| &l.1).eq(new.iter().take(h));
        let at = (len.saturating_sub(new.len())..=len.saturating_sub(h)).find(head);
        match at.filter(|_| !self.acp && h > 0) {
            Some(p) if self.lines.iter().skip(p).map(|l| &l.1).eq(new.iter()) => false,
            Some(p) => {
                let first = self.lines.get(p).map_or(1, |l| l.0);
                self.lines.truncate(p);
                self.lines.extend((first..).zip(new));
                self.bytes = self.lines.iter().map(|l| l.1.len()).sum();
                true
            }
            None if new.is_empty() => false,
            None => {
                self.acp = false;
                let more = new.len() >= asked;
                self.replace((1..).zip(new).collect(), more);
                true
            }
        }
    }

    fn events(&mut self, new: &[Logged]) -> bool {
        let line = |e: &Logged| (e.seq, json!(e).to_string());
        let (front, back) = (self.lines.front().map(|l| l.0), self.lines.back().map(|l| l.0));
        if !self.acp || back.is_none() {
            self.acp = true;
            self.replace(new.iter().map(line).collect(), new.first().is_some_and(|e| e.seq > 1));
            return !new.is_empty();
        }
        let older: Vec<(u64, String)> = new
            .iter()
            .filter(|e| front.is_some_and(|f| e.seq < f))
            .map(line)
            .collect();
        let newer: Vec<(u64, String)> = new
            .iter()
            .filter(|e| back.is_some_and(|b| e.seq > b))
            .map(line)
            .collect();
        let changed = !older.is_empty() || !newer.is_empty();
        self.more = new.first().is_some_and(|e| e.seq > 1) || (older.is_empty() && self.more);
        self.bytes += older.iter().chain(&newer).map(|l| l.1.len()).sum::<usize>();
        older.into_iter().rev().for_each(|l| self.lines.push_front(l));
        self.lines.extend(newer);
        changed
    }

    fn trim(&mut self, now: u64) {
        let (n, max) = if self.watched(now) {
            (DEEP, WATCHED_BYTES)
        } else {
            self.deep = false;
            (usize::try_from(SHOWN).unwrap_or(DEEP), usize::MAX)
        };
        while self.lines.len() > n || self.bytes > max && self.lines.len() > 1 {
            self.pop();
        }
    }

    fn blocks(&self, from: u64, to: u64) -> Vec<Value> {
        let lines = self.lines.iter().filter(|l| (from..to).contains(&l.0));
        let id = |n: u64| format!("t{}-{n}", self.gen);
        if self.acp {
            let ev: Vec<Logged> = lines.filter_map(|l| serde_json::from_str(&l.1).ok()).collect();
            let said = |(n, b): &(u64, Block)| super::screens::said(&id(*n), b, &[]);
            return super::v1::acp_numbered(&ev).iter().map(said).collect();
        }
        let mut out: Vec<(u64, Vec<String>)> = vec![];
        let chunk = |n: u64| n.saturating_sub(1) / CHUNK;
        for (n, s) in lines {
            match out.last_mut() {
                Some(c) if chunk(c.0) == chunk(*n) => c.1.push(s.clone()),
                _ => out.push((*n, vec![s.clone()])),
            }
        }
        let said = |(n, l): &(u64, Vec<String>)| super::screens::said(&id(*n), &Block::Screen, l);
        out.iter().map(said).collect()
    }
}

/// Each lane's last lines, numbered and masked, fed in the background; a lane sheet reads only these.
#[derive(Default)]
pub struct Tails {
    lanes: Mutex<BTreeMap<String, Tail>>,
    changes: AtomicU64,
}

impl Tails {
    /// A lane sheet is open on `agent` at `now`: its tail is read deep and often for 10 min.
    pub fn watch(&self, agent: &str, now: u64) {
        lock(&self.lanes).entry(agent.into()).or_default().watched_at = now;
    }

    /// Bumped when `agent`'s kept lines change.
    pub fn version(&self, agent: &str) -> u64 {
        lock(&self.lanes).get(agent).map_or(0, |t| t.ver)
    }

    /// Lane `l`'s transcript blocks: its last 200 lines from a 50-line boundary, each block `t<gen>-<first line>`.
    pub fn sheet(&self, l: &lanes::Lane) -> Vec<Value> {
        let m = lock(&self.lanes);
        let Some((t, last)) = m.get(&l.name).and_then(|t| Some((t, t.lines.back()?.0))) else {
            let fed = m.get(&l.name).is_some_and(|t| t.read_at > 0);
            return match l.session {
                Some(_) if !fed => vec![json!({"type": "row", "id": "loading", "title": "transcript loading…"})],
                _ => vec![],
            };
        };
        let lo = last.saturating_sub(SHOWN - 1).max(1);
        let from = (lo - 1) / CHUNK * CHUNK + 1;
        let shown: Vec<&(u64, String)> = t.lines.iter().filter(|l| l.0 >= from).collect();
        let fit = window(&shown, shown.len(), 32).first().map_or(from, |l| l.0);
        let up = fit.saturating_sub(1).div_ceil(CHUNK) * CHUNK + 1;
        t.blocks(
            if fit > from {
                up.min((last - 1) / CHUNK * CHUNK + 1)
            } else {
                from
            },
            last + 1,
        )
    }

    /// The 200 kept lines before block `before` (`t<gen>-<n>`) of `agent`'s tail; None once its gen moved on.
    pub fn page(&self, agent: &str, before: &str) -> Option<Value> {
        let (gen, n) = before.strip_prefix('t')?.split_once('-')?;
        let (gen, n) = (gen.parse::<u32>().ok()?, n.parse::<u64>().ok()?);
        let m = lock(&self.lanes);
        let t = m.get(agent).filter(|t| t.gen == gen)?;
        let from = n.saturating_sub(SHOWN).max(1);
        let more = t.lines.front().is_some_and(|f| f.0 < from) || t.more;
        Some(json!({"blocks": t.blocks(from, n), "more": more}))
    }

    fn put(&self, agent: &str, got: Option<Transcript>, asked: usize, now: u64) -> bool {
        let mut m = lock(&self.lanes);
        let t = m.entry(agent.into()).or_default();
        t.read_at = now;
        t.deep |= asked == DEEP && got.is_some();
        let changed = match got {
            Some(Transcript::Screen(text)) => {
                let budget = if t.watched(now) { WATCHED_BYTES } else { usize::MAX };
                let mut size = 0;
                let fits = |l: &&str| {
                    size += l.len();
                    size <= budget || size == l.len()
                };
                let mut lines: Vec<String> = text
                    .lines()
                    .rev()
                    .take(asked)
                    .take_while(fits)
                    .map(lanes::mask)
                    .collect();
                lines.reverse();
                t.screen(lines, if size > budget { 0 } else { asked })
            }
            Some(Transcript::Events(ev)) => t.events(ev.get(ev.len().saturating_sub(asked)..).unwrap_or_default()),
            None => false,
        };
        if changed {
            t.ver = self.changes.fetch_add(1, Relaxed) + 1;
        }
        Self::bound(&mut m, now);
        changed
    }

    fn bound(m: &mut BTreeMap<String, Tail>, now: u64) {
        for t in m.values_mut() {
            t.trim(now);
        }
        let mut total: usize = m.values().map(|t| t.bytes).sum();
        let mut order: Vec<&mut Tail> = m.values_mut().collect();
        order.sort_by_key(|t| t.watched_at);
        for t in order {
            while total > ALL_BYTES && t.lines.len() > 1 {
                total = total.saturating_sub(t.pop());
            }
        }
    }
}

impl Server<'_> {
    pub(super) fn feed_tails(&self) {
        loop {
            self.feed(crate::now());
            drover_core::clock::sleep(Duration::from_secs(1));
        }
    }

    pub(super) fn feed(&self, now: u64) {
        let (Ok(cfg), Ok(ev)) = (Config::kept(self.ctx), working(&self.ctx.state)) else {
            return;
        };
        let ctx = Ctx {
            now,
            ..self.ctx.clone()
        };
        let Some(list) = self.recent_sessions(now, &cfg) else {
            return;
        };
        let all = crate::snapshot::lanes(&ctx, &cfg, (&ev, &ev.states), &list);
        let mut due: Vec<(&lanes::Lane, usize)> = vec![];
        let mut m = lock(&self.tails.lanes);
        m.retain(|k, _| all.iter().any(|l| &l.name == k));
        for l in &all {
            let t = m.entry(l.name.clone()).or_default();
            let w = t.watched(now);
            let every = match (w, l.machine.is_some()) {
                (false, _) => COLD_SECS,
                (true, false) => 1,
                (true, true) => cfg.limits.serve_remote_secs,
            };
            if t.read_at == 0 || l.session.is_some() && t.read_at + every <= now {
                due.push((l, if w && !t.deep { DEEP } else { 200 }));
            }
        }
        drop(m);
        for (l, n) in due {
            let got = match &l.session {
                Some(s) => of(&l.on(&self.r), s, None, Some(n)).ok(),
                None => archived(&ctx, &l.name),
            };
            if self.tails.put(&l.name, got, n, now) {
                self.hub.kick();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tails_stay_under_their_bound() {
        let (tails, now) = (Tails::default(), 1_000_000);
        let line = "abc def ".repeat(1536);
        let screen: String = (0..5000).map(|i| format!("{i} {line}\n")).collect();
        for i in 0..10 {
            let agent = format!("lane-{i}");
            if i < 3 {
                tails.watch(&agent, now);
            }
            let asked = if i < 3 { DEEP } else { 200 };
            tails.put(&agent, Some(Transcript::Screen(screen.clone())), asked, now);
        }
        let m = lock(&tails.lanes);
        let total: usize = m.values().map(|t| t.bytes).sum();
        assert!(total <= ALL_BYTES, "{total}");
        assert_eq!(
            total,
            m.values()
                .map(|t| t.lines.iter().map(|l| l.1.len()).sum::<usize>())
                .sum::<usize>()
        );
        let lines = |i: usize| m[&format!("lane-{i}")].lines.len();
        for i in 0..3 {
            assert_eq!(lines(i), WATCHED_BYTES / (line.len() + 5), "watched lanes keep 512 KB");
        }
        assert!(
            (3..10).all(|i| lines(i) <= 200),
            "unwatched lanes keep 200 lines at most"
        );
        assert!((3..10).any(|i| lines(i) < 200), "unwatched lanes shrink first");
        assert!(m.values().all(|t| t.more), "the oldest lines went");
    }

    #[test]
    fn a_sheet_of_long_lines_keeps_32_kb_from_a_block_boundary() {
        let (tails, now) = (Tails::default(), 1_000_000);
        let screen: String = (1..=300).map(|i| format!("{i} {}\n", "ab ".repeat(100))).collect();
        tails.watch("lane", now);
        tails.put("lane", Some(Transcript::Screen(screen)), DEEP, now);
        let l = lanes::Lane {
            name: "lane".into(),
            ..lanes::Lane::default()
        };
        let sheet = tails.sheet(&l);
        let ids: Vec<&str> = sheet.iter().map(|b| b["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["t1-201", "t1-251"], "{sheet:?}");
        assert!(json!(sheet).to_string().len() <= 32 << 10);
    }
}
