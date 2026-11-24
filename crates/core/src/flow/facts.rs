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
