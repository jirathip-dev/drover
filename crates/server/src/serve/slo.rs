use crate::api::RouteTime;
use std::collections::{BTreeMap, VecDeque};

/// The requests of the last hour, as (time, route, write, ms); and the read 503s' times.
#[derive(Default)]
pub struct Timing(VecDeque<(u64, String, bool, u64)>, VecDeque<u64>);

const KEEP: u64 = 3600;
const SHOWN: u64 = 600;

/// A request path as its route: `/api/v1/<name>`, `/screens/<name>`, else its first segment.
pub fn route(path: &str) -> String {
    let n = match path {
        p if p.starts_with("/api/v1/") => 4,
        p if p.starts_with("/screens/") => 3,
        _ => 2,
    };
    path.split('/').take(n).collect::<Vec<_>>().join("/")
}

fn p(ms: &mut [u64], pct: usize) -> u64 {
    ms.sort_unstable();
    let i = (ms.len() * pct).div_ceil(100).clamp(1, ms.len());
    ms.get(i - 1).copied().unwrap_or_default()
}

fn time(route: &str, mut ms: Vec<u64>) -> RouteTime {
    RouteTime {
        route: route.into(),
        count: u64::try_from(ms.len()).unwrap_or(u64::MAX),
        p50: p(&mut ms, 50),
        p95: p(&mut ms, 95),
        max: ms.last().copied().unwrap_or_default(),
    }
}

impl Timing {
    pub fn add(&mut self, now: u64, path: &str, write: bool, ms: u64) {
        self.0.push_back((now, route(path), write, ms));
        while self.0.front().is_some_and(|s| s.0 + KEEP < now) || self.0.len() > 100_000 {
            self.0.pop_front();
        }
    }

    /// A read answered 503.
    pub fn busy(&mut self, now: u64) {
        self.1.push_back(now);
        while self.1.front().is_some_and(|t| t + 60 < now) {
            self.1.pop_front();
        }
    }

    /// The last minute's `/live` first bytes, `/api/v1` reads and writes, and its read 503s.
    pub fn minute(&self, now: u64) -> (Vec<RouteTime>, u64) {
        let from = (now + 1).saturating_sub(60);
        let mut m: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
        for (_, r, w, ms) in self.0.iter().filter(|s| s.0 >= from) {
            let g = match (w, r.as_str()) {
                (true, _) => "writes",
                (_, "/live") => "/live",
                (_, r) if r.starts_with("/api/v1/") => "/api/v1 reads",
                _ => continue,
            };
            m.entry(g).or_default().push(*ms);
        }
        let groups = m.into_iter().map(|(g, ms)| time(g, ms)).collect();
        (groups, self.1.iter().filter(|t| **t >= from).map(|_| 1).sum())
    }

    fn by_route(&self, from: u64, to: u64) -> BTreeMap<&str, Vec<u64>> {
        let mut m: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
        for (_, r, _, ms) in self.0.iter().filter(|s| s.0 >= from && s.0 < to) {
            m.entry(r).or_default().push(*ms);
        }
        m
    }

    /// The last 10 min's routes, slowest p95 first.
    pub fn routes(&self, now: u64) -> Vec<RouteTime> {
        let mut v: Vec<RouteTime> = (self.by_route(now.saturating_sub(SHOWN), now + 1).into_iter())
            .map(|(r, ms)| time(r, ms))
            .collect();
        v.sort_by(|a, b| b.p95.cmp(&a.p95).then(a.route.cmp(&b.route)));
        v
    }

    /// The last minute's routes, by name.
    pub fn last(&self, now: u64) -> Vec<RouteTime> {
        let m = self.by_route((now + 1).saturating_sub(60), now + 1);
        m.into_iter().map(|(r, ms)| time(r, ms)).collect()
    }

    /// Each of the last `minutes` minutes' p95 of the routes `pick` takes over `limit` ms: the slowest and its p95.
    pub fn breach(&self, now: u64, limit: u64, minutes: u64, pick: impl Fn(&str) -> bool) -> Option<(String, u64)> {
        let api = |from, to| -> Vec<(String, Vec<u64>)> {
            (self.by_route(from, to).into_iter())
                .filter(|(r, _)| pick(r))
                .map(|(r, ms)| (r.to_string(), ms))
                .collect()
        };
        for m in 0..minutes.max(1) {
            let to = (now + 1).saturating_sub(60 * m);
            let mut ms: Vec<u64> = api(to.saturating_sub(60), to).into_iter().flat_map(|x| x.1).collect();
            if ms.is_empty() || p(&mut ms, 95) <= limit {
                return None;
            }
        }
        let all = api((now + 1).saturating_sub(60 * minutes.max(1)), now + 1);
        all.into_iter()
            .map(|(r, mut ms)| (r, p(&mut ms, 95)))
            .max_by_key(|x| x.1)
    }
}
