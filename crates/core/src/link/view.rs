use super::frame::{Line, Seen, Worktree};
use crate::config::Config;
use crate::events::Event;
use crate::session::{Session, Sessions};
use crate::Ctx;
use anyhow::Result;

/// What the main host knows of one host's link: its state, the last frame applied and when it arrived.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HostSeen {
    pub host: String,
    pub link: String,
    pub since: u64,
    pub why: Option<String>,
    pub boot: Option<String>,
    pub n: Option<u64>,
    pub heard: Option<u64>,
    pub build: Option<String>,
}

/// The view: every host's last word as `state.db` holds it; never a call to a host.
pub struct View {
    now: u64,
    fresh: u64,
    hosts: Vec<(String, Option<String>, Option<HostSeen>)>,
    lanes: Vec<(Option<String>, Seen)>,
}

impl View {
    /// The view as `state.db` holds it now.
    pub fn load(ctx: &Ctx, cfg: &Config) -> Result<View> {
        let (rows, lanes) = crate::db::view(&ctx.state)?;
        let hosts: Vec<_> = (super::hosts(cfg).into_iter())
            .map(|(name, m)| {
                let row = rows.iter().find(|h| h.host == name).cloned();
                (name, m, row)
            })
            .collect();
        let machine = |h: &str| hosts.iter().find(|x| x.0 == h).map(|x| x.1.clone());
        let lanes = (lanes.into_iter())
            .filter_map(|(h, json)| Some((machine(&h)?, serde_json::from_str(&json).ok()?)))
            .collect();
        Ok(View {
            now: ctx.now,
            fresh: 2 * cfg.limits.fast_secs,
            hosts,
            lanes,
        })
    }
    fn heard(&self, h: Option<&HostSeen>) -> Result<(), String> {
        let h = h.ok_or("no word from it yet")?;
        let why = || {
            h.why
                .clone()
                .filter(|w| w != super::NO_SERVICE)
                .unwrap_or_else(|| "no word from it yet".into())
        };
        let age = self.now.saturating_sub(h.heard.ok_or_else(why)?);
        if age > self.fresh {
            return Err(format!("no word from it for {}", crate::events::age(age)));
        }
        Ok(())
    }
    fn word(&self, h: Option<&HostSeen>) -> Result<(), String> {
        self.heard(h)?;
        match h.and_then(|h| h.why.clone()) {
            Some(why) if why != super::NO_SERVICE => Err(why),
            _ => Ok(()),
        }
    }
    /// The hosts to poll before reading: those with no fresh word.
    pub fn stale(&self) -> Vec<&str> {
        let due = |h: &&(String, Option<String>, Option<HostSeen>)| self.word(h.2.as_ref()).is_err();
        self.hosts.iter().filter(due).map(|h| h.0.as_str()).collect()
    }
    fn up(&self, m: Option<&str>) -> bool {
        self.down(m).is_none()
    }
    /// Each host's sessions, this one first; a host with no fresh word answers `Err`, as an unreachable one.
    pub fn sessions(&self) -> Sessions {
        let one = |(name, m, row): &(String, Option<String>, Option<HostSeen>)| {
            let got = self.word(row.as_ref()).map(|()| {
                let all = (self.lanes.iter())
                    .filter(|l| &l.0 == m)
                    .filter_map(|l| l.1.session.clone().map(Session::from));
                match m {
                    Some(m) => crate::session::list::tagged(all.collect(), name, m),
                    None => all.collect(),
                }
            });
            (m.clone(), got)
        };
        Sessions(self.hosts.iter().map(one).collect())
    }
    /// This host's sessions, as `sessions` lists them first.
    pub fn here(&self) -> Result<Vec<Session>, String> {
        let first = self.sessions().0.into_iter().next();
        first.map_or_else(|| Err("no word".into()), |h| h.1)
    }
    /// The worktree spawn `sp` made, as its host last reported it; None when that host has no fresh word.
    pub fn worktree(&self, sp: &Event) -> Option<&Worktree> {
        let (m, wt) = (sp.str("machine"), sp.str("worktree")?);
        let found = self.lanes.iter().filter(|l| l.0.as_deref() == m);
        let w = found.filter_map(|l| l.1.worktree.as_ref()).find(|w| w.path == wt);
        w.filter(|_| self.up(m))
    }
    /// Why the host at `m` has no fresh word (None: it has).
    pub fn down(&self, m: Option<&str>) -> Option<String> {
        let h = self.hosts.iter().find(|h| h.1.as_deref() == m);
        h.map_or(Some("not configured".into()), |h| self.heard(h.2.as_ref()).err())
    }
    /// Every outbox line a host with fresh word reported, with its machine.
    pub fn lines(&self) -> impl Iterator<Item = (Option<&str>, &Line)> {
        let up = self.lanes.iter().filter(|l| self.up(l.0.as_deref()));
        up.flat_map(|l| {
            l.1.worktree
                .iter()
                .flat_map(|w| w.lines.iter().map(|x| (l.0.as_deref(), x)))
        })
    }
}
