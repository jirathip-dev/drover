//! #859: each request's route and time: per-route p50/p95 (`/api/v1/health`) and the `/api/v1` SLO.
use crate::api::RouteTime;
use std::collections::{BTreeMap, VecDeque};

/// #859: the requests of the last hour, as (time, route, ms).
#[derive(Default)]
pub struct Timing(VecDeque<(u64, String, u64)>);

const KEEP: u64 = 3600;
const SHOWN: u64 = 600;

/// #859: a request path as its route: `/api/v1/<name>`, `/screens/<name>`, else its first segment.
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
    ms[i - 1]
}

impl Timing {
    pub fn add(&mut self, now: u64, path: &str, ms: u64) {
        self.0.push_back((now, route(path), ms));
        while self.0.front().is_some_and(|s| s.0 + KEEP < now) || self.0.len() > 100_000 {
            self.0.pop_front();
        }
    }

    fn by_route(&self, from: u64, to: u64) -> BTreeMap<&str, Vec<u64>> {
        let mut m: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
        for (_, r, ms) in self.0.iter().filter(|s| s.0 >= from && s.0 < to) {
            m.entry(r).or_default().push(*ms);
        }
        m
    }

    /// The last 10 min's routes, slowest p95 first.
    pub fn routes(&self, now: u64) -> Vec<RouteTime> {
        let mut v: Vec<RouteTime> = (self.by_route(now.saturating_sub(SHOWN), now + 1).into_iter())
            .map(|(r, mut ms)| RouteTime {
                route: r.into(),
                count: ms.len() as u64,
                p50: p(&mut ms, 50),
                p95: p(&mut ms, 95),
            })
            .collect();
        v.sort_by(|a, b| b.p95.cmp(&a.p95).then(a.route.cmp(&b.route)));
        v
    }

    /// Each of the last `minutes` minutes' `/api/v1` p95 over `limit` ms: the slowest route and its p95.
    pub fn breach(&self, now: u64, limit: u64, minutes: u64) -> Option<(String, u64)> {
        let api = |from, to| -> Vec<(String, Vec<u64>)> {
            (self.by_route(from, to).into_iter())
                .filter(|(r, _)| r.starts_with("/api/v1/"))
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
