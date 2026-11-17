pub mod list;
pub mod pulse;
pub mod stamp;
pub mod transcript;

pub use list::{all, get, live_on, on};
pub use pulse::Pulse;

use shed_proto::{Reported, TerminalInfo};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One shed session (shed's `TerminalInfo`), typed: nothing here is derived by drover.
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub name: String,
    pub labels: BTreeMap<String, String>,
    pub cwd: PathBuf,
    pub acp: bool,
    pub pid: Option<i32>,
    pub cpu_pct: Option<u64>,
    pub life: Life,
    pub note: Option<String>,
    pub host: Option<String>,
    pub machine: Option<String>,
}

/// Running with its reported status, or exited with its code and the last status it reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Life {
    Running(Status),
    Exited { code: Option<i32>, last: Status },
}

/// shed's reported state, every word it has; built only by `Status::of`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Unreported,
    Working,
    Running,
    Blocked,
    Approval,
    Limited,
    Done,
    Idle,
    Asleep,
    Failed,
    Other,
}

impl Status {
    /// shed's `reported.state` word as a status: the one mapping.
    pub fn of(r: Option<&Reported>) -> Status {
        match r.map(|r| r.state.as_str()) {
            None => Status::Unreported,
            Some("working") => Status::Working,
            Some("running") => Status::Running,
            Some("blocked") => Status::Blocked,
            Some("waiting-approval") => Status::Approval,
            Some("rate-limited") => Status::Limited,
            Some("done") => Status::Done,
            Some("idle") => Status::Idle,
            Some("hibernated") => Status::Asleep,
            Some("error") => Status::Failed,
            Some(_) => Status::Other,
        }
    }
    /// The status a shed word (a `lane-state` row's `state`) names.
    pub fn word_of(state: &str) -> Status {
        Status::of(Some(&Reported {
            state: state.into(),
            ..Reported::default()
        }))
    }
    /// Working or running: the lane is mid-turn.
    pub fn busy(self) -> bool {
        matches!(self, Status::Working | Status::Running)
    }
    /// The stall clock runs: neither busy, blocked, waiting on approval or a limit, nor unreported.
    pub fn quiet(self) -> bool {
        !self.busy()
            && !matches!(
                self,
                Status::Blocked | Status::Approval | Status::Limited | Status::Unreported
            )
    }
    /// shed's own word, every surface shows it as-is; `unknown` for none or one drover does not know.
    pub fn word(self) -> &'static str {
        match self {
            Status::Unreported | Status::Other => "unknown",
            Status::Working => "working",
            Status::Running => "running",
            Status::Blocked => "blocked",
            Status::Approval => "waiting-approval",
            Status::Limited => "rate-limited",
            Status::Done => "done",
            Status::Idle => "idle",
            Status::Asleep => "hibernated",
            Status::Failed => "error",
        }
    }
}

impl From<TerminalInfo> for Session {
    fn from(t: TerminalInfo) -> Session {
        let status = Status::of(t.reported.as_ref());
        let life = match t.state {
            shed_proto::State::Running => Life::Running(status),
            shed_proto::State::Exited => Life::Exited {
                code: t.exit,
                last: status,
            },
        };
        Session {
            note: t.reported.and_then(|r| r.note),
            name: t.name,
            labels: t.labels,
            cwd: t.cwd,
            acp: t.acp.is_some(),
            pid: t.pid,
            cpu_pct: t.resources.map(|u| u.cpu_pct),
            life,
            host: None,
            machine: None,
        }
    }
}

impl Session {
    /// Its status while it runs; None once exited.
    pub fn status(&self) -> Option<Status> {
        match self.life {
            Life::Running(s) => Some(s),
            Life::Exited { .. } => None,
        }
    }
    /// It runs.
    pub fn running(&self) -> bool {
        self.status().is_some()
    }
    /// It runs and shed says it works.
    pub fn busy(&self) -> bool {
        self.status().is_some_and(Status::busy)
    }
    /// A lane's or conductor's session: drover labelled it with a fleet.
    pub fn lane(&self) -> bool {
        self.labels.contains_key("fleet")
    }
    /// shed says its agent is signed out: `error` with an `auth:` note that is no [`Session::key_refused`].
    pub fn signed_out(&self) -> bool {
        self.auth().is_some_and(|n| !n.contains("lane key"))
    }
    /// #1223: the token proxy refused this lane's own key (`401 lane key <id> revoked`): the lane's fault, not the host's login.
    pub fn key_refused(&self) -> bool {
        self.auth().is_some_and(|n| n.contains("lane key"))
    }
    fn auth(&self) -> Option<&str> {
        let n = self.note.as_deref().filter(|n| n.starts_with("auth:"));
        n.filter(|_| self.status() == Some(Status::Failed))
    }
    /// Named `name`, or working inside `dir`.
    pub fn in_dir(&self, name: &str, dir: &Path) -> bool {
        self.name == name || self.cwd.starts_with(dir)
    }
}

/// Why a lane's session ended, from shed's exit code and last status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    Limit,
    Crash,
    Killed,
    Ended,
}

impl Cause {
    /// An exit with `code` after shed last reported `last`.
    pub fn of(code: Option<i32>, last: Status) -> Cause {
        match last {
            Status::Limited => Cause::Limit,
            Status::Working | Status::Running | Status::Failed => Cause::Crash,
            _ if code != Some(0) => Cause::Crash,
            _ => Cause::Ended,
        }
    }
    /// The word the log and the alerts carry.
    pub fn word(self) -> &'static str {
        match self {
            Cause::Limit => "usage limit",
            Cause::Crash => "adapter crash",
            Cause::Killed => "killed",
            Cause::Ended => "ended normally",
        }
    }
    /// The cause a logged word names (old rows too).
    pub fn parse(w: &str) -> Option<Cause> {
        [Cause::Limit, Cause::Crash, Cause::Killed, Cause::Ended]
            .into_iter()
            .find(|c| c.word() == w)
    }
    /// #662: drover resumes the lane in its worktree.
    pub fn resumable(self) -> bool {
        matches!(self, Cause::Limit | Cause::Crash)
    }
}

/// A lane's end as shed told it.
#[derive(Clone, Debug, PartialEq)]
pub struct Exit {
    pub code: Option<i32>,
    pub cause: Cause,
    pub note: Option<String>,
}

/// Where a session stands on the host its spawn named.
#[derive(Debug)]
pub enum Seen<'a> {
    Here(&'a Session),
    Absent,
    Unreachable,
}

/// One host's `ls`: its machine (`None`: this one) and its sessions, or why its shed did not answer.
pub type Listed = (Option<String>, Result<Vec<Session>, String>);

/// Every host's sessions; a host whose shed did not answer is not "no sessions".
#[derive(Clone, Debug, Default)]
pub struct Sessions(pub Vec<Listed>);

impl Sessions {
    /// The answer of the host at `machine` (`None`: this one).
    pub fn one(machine: Option<String>, got: Result<Vec<Session>, String>) -> Sessions {
        Sessions(vec![(machine, got)])
    }
    /// The sessions given, as one answering host's.
    pub fn of(all: impl IntoIterator<Item = Session>) -> Sessions {
        Sessions::one(None, Ok(all.into_iter().collect()))
    }
    /// Every session a host answered with, exited ones too.
    pub fn every(&self) -> impl Iterator<Item = &Session> {
        self.0.iter().filter_map(|h| h.1.as_ref().ok()).flatten()
    }
    /// The running sessions drover labelled (lanes and conductors).
    pub fn live(&self) -> impl Iterator<Item = &Session> {
        self.every().filter(|s| s.running() && s.lane())
    }
    /// `live`, owned: what a caller that takes one host's sessions gets.
    pub fn running(&self) -> Vec<Session> {
        self.live().cloned().collect()
    }
    /// The answer of this host, failing when its shed did not answer.
    pub fn checked(self) -> anyhow::Result<Sessions> {
        match self.0.iter().find(|h| h.0.is_none()).map(|h| &h.1) {
            Some(Err(e)) => anyhow::bail!("{e}"),
            _ => Ok(self),
        }
    }
    /// Session `name`, running or exited, on any host that answered.
    pub fn get(&self, name: &str) -> Option<&Session> {
        self.every().find(|s| s.name == name)
    }
    /// Session `name` runs.
    pub fn up(&self, name: &str) -> bool {
        self.live().any(|s| s.name == name)
    }
    /// Session `name` runs and shed says it works.
    pub fn busy(&self, name: &str) -> bool {
        self.live()
            .any(|s| s.name == name && s.status().is_some_and(Status::busy))
    }
    /// The running sessions named `name` or inside `dir`.
    pub fn in_dir<'a>(&'a self, name: &'a str, dir: &'a Path) -> impl Iterator<Item = &'a Session> {
        self.live().filter(move |s| s.in_dir(name, dir))
    }
    /// The hosts whose shed failed: (machine or `local`, why).
    pub fn failed(&self) -> impl Iterator<Item = (&str, &str)> {
        (self.0.iter()).filter_map(|(m, got)| Some((m.as_deref().unwrap_or("local"), got.as_ref().err()?.as_str())))
    }
    /// Whether this host's shed answered.
    pub fn listed(&self) -> bool {
        self.0.iter().any(|h| h.0.is_none() && h.1.is_ok())
    }
    /// Session `name` on the host at `machine` (`None`: this one).
    pub fn seen(&self, name: &str, machine: Option<&str>) -> Seen<'_> {
        let at = self.0.iter().find(|h| h.0.as_deref() == machine);
        match at.map(|h| &h.1) {
            Some(Ok(all)) => all.iter().find(|s| s.name == name).map_or(Seen::Absent, Seen::Here),
            _ => Seen::Unreachable,
        }
    }
}
