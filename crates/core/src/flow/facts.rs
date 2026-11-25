use crate::config::{Config, Limits};
use crate::events::{Event, IssueState, Json};
use crate::link::frame::Sample;
use crate::link::view::{HostSeen, View};
use crate::session::{Life, Seen as Listed, Status};
use crate::Ctx;
use serde::Deserialize;
use std::fmt;

macro_rules! values {
    ($name:ident { $($v:ident = $s:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
        pub enum $name {
            $(#[serde(rename = $s)] $v),+
        }
        impl $name {
            pub const ALL: &[$name] = &[$($name::$v),+];
            pub fn name(self) -> &'static str {
                match self {
                    $($name::$v => $s),+
                }
            }
        }
    };
}

values!(Fact { Lane = "lane", Host = "host", Pr = "pr", Verdict = "verdict", Ask = "ask", Turn = "turn" });
values!(Lane { None = "none", Running = "running", Limited = "limited", Asleep = "asleep", Done = "done", Exited = "exited", Gone = "gone" });
values!(Host { Up = "up", Loaded = "loaded", Draining = "draining", Offline = "offline" });
values!(Pr { None = "none", Merged = "merged", Closed = "closed", Conflicting = "conflicting", Red = "red", Pending = "pending", Green = "green", Open = "open" });
values!(Verdict { None = "none", Written = "written", Pass = "PASS", Fail = "FAIL", Refused = "refused" });
values!(Ask { None = "none", Open = "open", Answered = "answered" });
values!(Turn { None = "none", Working = "working", Done = "done", Blocked = "blocked", NeedInput = "need-input" });

/// One observed value of each fact: the condition a state's rows are matched against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cond {
    pub lane: Lane,
    pub host: Host,
    pub pr: Pr,
    pub verdict: Verdict,
    pub ask: Ask,
    pub turn: Turn,
}

impl Cond {
    /// Every condition: each fact's every value, crossed.
    pub fn every() -> impl Iterator<Item = Cond> {
        Lane::ALL.iter().flat_map(|&lane| {
            Host::ALL.iter().flat_map(move |&host| {
                Pr::ALL.iter().flat_map(move |&pr| {
                    Verdict::ALL.iter().flat_map(move |&verdict| {
                        Ask::ALL.iter().flat_map(move |&ask| {
                            Turn::ALL.iter().map(move |&turn| Cond {
                                lane,
                                host,
                                pr,
                                verdict,
                                ask,
                                turn,
                            })
                        })
                    })
                })
            })
        })
    }

    /// The value of `fact` in this condition.
    pub fn value(&self, fact: Fact) -> &'static str {
        match fact {
            Fact::Lane => self.lane.name(),
            Fact::Host => self.host.name(),
            Fact::Pr => self.pr.name(),
            Fact::Verdict => self.verdict.name(),
            Fact::Ask => self.ask.name(),
            Fact::Turn => self.turn.name(),
        }
    }
}

impl Fact {
    /// The names of this fact's values.
    pub fn values(self) -> Vec<&'static str> {
        match self {
            Fact::Lane => Lane::ALL.iter().map(|v| v.name()).collect(),
            Fact::Host => Host::ALL.iter().map(|v| v.name()).collect(),
            Fact::Pr => Pr::ALL.iter().map(|v| v.name()).collect(),
            Fact::Verdict => Verdict::ALL.iter().map(|v| v.name()).collect(),
            Fact::Ask => Ask::ALL.iter().map(|v| v.name()).collect(),
            Fact::Turn => Turn::ALL.iter().map(|v| v.name()).collect(),
        }
    }
}

impl fmt::Display for Cond {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = Fact::ALL
            .iter()
            .map(|&x| format!("{} {}", x.name(), self.value(x)))
            .collect();
        write!(f, "{}", parts.join(" · "))
    }
}

/// One pass's sample: the view and the log, read once, and the PR cache; never a call to a host, `gh` or `git`.
pub struct Seen<'a> {
    ctx: &'a Ctx,
    cfg: &'a Config,
    view: View,
    events: std::sync::Arc<crate::events::Log>,
    asks: Vec<Event>,
}

impl<'a> Seen<'a> {
    /// The sample as `state.db`, the log and the PR cache hold it now.
    pub fn load(ctx: &'a Ctx, cfg: &'a Config) -> anyhow::Result<Seen<'a>> {
        let (view, events) = (View::load(ctx, cfg)?, crate::events::working(&ctx.state)?);
        let asks = crate::events::open_asks(&events).into_iter().cloned().collect();
        Ok(Seen {
            ctx,
            cfg,
            view,
            events,
            asks,
        })
    }

    /// Issue `s`'s episode, read from this sample's log.
    pub fn episode(&self, s: &IssueState) -> super::Episode {
        super::Episode::of(&self.events, s)
    }

    /// Issue `s`'s condition in its episode `ep`, and each value's evidence.
    pub fn cond(&self, s: &IssueState, ep: &super::Episode) -> (Cond, Vec<String>) {
        let (ev, now, sp) = (&self.events[..], self.ctx.now, ep.spawn.as_ref());
        let m = sp.and_then(|e| e.str("machine"));
        let json = crate::gh::cached_pr(self.ctx, &s.fleet, s.issue);
        let req = self.cfg.fleet.get(&s.fleet).map_or(&[][..], |f| f.checks(&json));
        let (lane, l) = lane(&self.view, sp);
        let (host, h) = match sp {
            Some(_) => host(self.view.at(m), self.view.at(None), &self.cfg.limits, now),
            None => (Host::Up, "no lane".into()),
        };
        let (pr, p) = pr(&json, req);
        let (verdict, v) = verdict(ev, s, json.at("headRefOid").as_str());
        let (ask, a) = ask(ev, &self.asks, s);
        let (turn, t) = turn(ev, s);
        let cond = Cond {
            lane,
            host,
            pr,
            verdict,
            ask,
            turn,
        };
        (cond, vec![l, h, p, v, a, t])
    }

    /// Issue episode `ep`'s clocks: when shed last spoke of its lane, its host's last frame, and the lane's limit reset.
    pub fn clock(&self, ep: &super::Episode) -> super::Clock {
        let (ev, sp) = (&self.events[..], ep.spawn.as_ref());
        let agent = sp.and_then(|e| e.str("agent"));
        let frame = sp.and_then(|e| self.view.at(e.str("machine")).map(|h| h.heard.unwrap_or(h.since)));
        let picked =
            |a: &str| (crate::events::of_kind(ev, &["limit-picked"]).into_iter()).rfind(|e| e.str("agent") == Some(a));
        super::Clock {
            now: self.ctx.now,
            heard: agent
                .and_then(|a| crate::session::stamp::since(ev, a))
                .or(sp.map(|e| e.ts)),
            frame,
            reset: agent.and_then(picked).and_then(|e| e.val("at").as_u64()),
        }
    }
}

/// This round's lane, as its host's last frame listed the session of `sp`.
pub fn lane(view: &View, sp: Option<&Event>) -> (Lane, String) {
    let Some(sp) = sp else {
        return (Lane::None, "no spawn this round".into());
    };
    let name = sp.str("agent").unwrap_or_default();
    let all = view.last_word();
    let s = match all.seen(name, sp.str("machine")) {
        Listed::Here(s) => s,
        Listed::Absent => return (Lane::Gone, format!("{name} not in its host's last frame")),
        Listed::Unreachable => return (Lane::Running, format!("{name}: no frame from its host yet")),
    };
    let v = match s.life {
        Life::Exited { .. } => Lane::Exited,
        Life::Running(Status::Limited) => Lane::Limited,
        Life::Running(Status::Asleep) => Lane::Asleep,
        Life::Running(Status::Done | Status::Idle | Status::Failed) => Lane::Done,
        Life::Running(_) => Lane::Running,
    };
    (v, format!("{name} {:?}", s.life))
}

fn over(s: &Sample, l: &Limits) -> Option<String> {
    let pressed = s.pressure.as_deref().filter(|p| *p != "normal");
    let load = (s.load_per_cpu > l.load_per_cpu).then(|| format!("load {:.1} per CPU", s.load_per_cpu));
    load.or_else(|| pressed.map(|p| format!("memory pressure {p}")))
}

/// No frame from the host of row `at` for `host_offline_minutes` (a host never heard counts from its row's start).
pub fn silent(at: Option<&HostSeen>, l: &Limits, now: u64) -> bool {
    at.is_some_and(|h| now.saturating_sub(h.heard.unwrap_or(h.since)) >= l.host_offline_minutes * 60)
}

/// A lane's host from its row `at` and the main host's `main`: only its own sample makes it loaded, a late frame keeps its last value while either end is loaded, and no frame for `host_offline_minutes` is offline at any load.
pub fn host(at: Option<&HostSeen>, main: Option<&HostSeen>, l: &Limits, now: u64) -> (Host, String) {
    let Some(h) = at else {
        return (Host::Offline, "no word from it yet".into());
    };
    let loaded = |r: Option<&HostSeen>| over(r?.sample.as_ref()?, l);
    let ago = crate::events::age(now.saturating_sub(h.heard.unwrap_or(h.since)));
    let late = now.saturating_sub(h.heard.unwrap_or(h.since)) > 2 * l.fast_secs;
    let (own, busy) = (loaded(Some(h)), loaded(main));
    if silent(at, l, now) {
        return (Host::Offline, format!("no frame for {ago}"));
    }
    if h.link != "live" && late && own.is_none() && busy.is_none() {
        return (Host::Offline, format!("link {}, last frame {ago} ago", h.link));
    }
    match (own, &h.drain) {
        (Some(w), _) => (Host::Loaded, format!("{w}, last frame {ago} ago")),
        (None, Some(d)) => (Host::Draining, d.clone()),
        (None, None) => (Host::Up, format!("last frame {ago} ago")),
    }
}

/// The issue branch's PR, as the PR cache holds `pr`, its checks judged by `required`.
pub fn pr(pr: &serde_json::Value, required: &[String]) -> (Pr, String) {
    let checks = crate::gh::required(pr.at("statusCheckRollup"), required);
    let v = match (pr.at("state").as_str(), pr.at("mergeable").as_str()) {
        (None, _) => Pr::None,
        (Some("MERGED"), _) => Pr::Merged,
        (Some("CLOSED"), _) => Pr::Closed,
        (_, Some("CONFLICTING")) => Pr::Conflicting,
        _ if checks.as_array().is_none_or(Vec::is_empty) => Pr::Open,
        _ => match crate::gh::ci(&checks) {
            "failed" => Pr::Red,
            "green" => Pr::Green,
            _ => Pr::Pending,
        },
    };
    let head: String = pr
        .at("headRefOid")
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(7)
        .collect();
    (
        v,
        format!("PR {} at {head}", pr.at("number")).replace("PR null at ", "no PR"),
    )
}

/// The check stage's word on `head`, from the log: `verdict-seen`, then `verdict` or `verdict-refused`.
pub fn verdict(events: &[Event], s: &IssueState, head: Option<&str>) -> (Verdict, String) {
    let Some(head) = head else {
        return (Verdict::None, "no PR head".into());
    };
    let at = |e: &Event| {
        e.str("sha")
            .or_else(|| e.str("head"))
            .is_some_and(|h| h.eq_ignore_ascii_case(head))
    };
    let stage = |e: &Event| e.str("stage").is_none_or(|x| x == s.stage);
    let mine = crate::events::of_issue(events, &s.fleet, s.issue);
    let last = mine
        .into_iter()
        .rev()
        .filter(|e| at(e))
        .find_map(|e| match e.kind.as_str() {
            "verdict" if stage(e) && e.str("verdict") == Some("PASS") => Some((Verdict::Pass, e)),
            "verdict" if stage(e) => Some((Verdict::Fail, e)),
            "verdict-refused" => Some((Verdict::Refused, e)),
            "verdict-seen" if matches!(e.str("first"), Some("PASS" | "FAIL")) => Some((Verdict::Written, e)),
            _ => None,
        });
    let short: String = head.chars().take(7).collect();
    last.map_or((Verdict::None, format!("nothing at {short}")), |(v, e)| {
        (
            v,
            format!("{} at {short} {}", e.kind, e.str("code").unwrap_or_default())
                .trim_end()
                .into(),
        )
    })
}

/// The issue's ask: one of the `open` asks on it or by one of its lanes, else an answer not yet delivered.
pub fn ask(events: &[Event], open: &[Event], s: &IssueState) -> (Ask, String) {
    let by_lane = |e: &Event| e.str("agent").is_some_and(|a| s.lanes.values().any(|l| l == a));
    let mine = |e: &&Event| e.fleet == s.fleet && (e.issue == Some(s.issue) || by_lane(e));
    if let Some(e) = open.iter().find(mine) {
        return (Ask::Open, format!("ask {} open", e.str("id").unwrap_or("?")));
    }
    match crate::undelivered(events, &s.fleet)
        .iter()
        .find(|e| e.issue == Some(s.issue))
    {
        Some(e) => (
            Ask::Answered,
            format!("answer {} not delivered", e.str("id").unwrap_or("?")),
        ),
        None => (Ask::None, "no ask".into()),
    }
}

/// What the build lane last said with `drover turn` this round.
pub fn turn(events: &[Event], s: &IssueState) -> (Turn, String) {
    let t = crate::waiting::turn(events, s);
    let v = match t.and_then(|e| e.str("state")) {
        Some("working") => Turn::Working,
        Some("done") => Turn::Done,
        Some("blocked") => Turn::Blocked,
        Some("need-input") => Turn::NeedInput,
        _ => Turn::None,
    };
    (v, format!("turn {}", v.name()))
}
