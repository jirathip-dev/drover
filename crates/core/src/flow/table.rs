use super::facts::{Ask, Cond, Fact, Host, Lane, Pr, Turn, Verdict};
use crate::config::LIMIT_DOCS;
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/flow_files.rs"));

macro_rules! names {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $v:ident = $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
        pub enum $name {
            $($(#[$vm])* #[serde(rename = $s)] $v),+
        }
        impl $name {
            pub fn name(self) -> &'static str {
                match self {
                    $($name::$v => $s),+
                }
            }
        }
    };
}

names!(Owner { Drive = "drive", Lane = "lane", Orchestrator = "orchestrator", Owner = "owner" });
names!(#[derive(Default)] Since { #[default] Entered = "entered", Heard = "heard", HostHeard = "host-heard", Reset = "reset" });
names!(#[derive(Default)] Escalate { #[default] Ladder = "ladder", Orchestrator = "orchestrator", Owner = "owner" });
names!(Action {
    Wait = "wait", Start = "start", SpawnBuild = "spawn-build", SpawnCheck = "spawn-check", Respawn = "respawn",
    RespawnCheck = "respawn-check", Move = "move", Update = "update", Nudge = "nudge",
    ContinueAtReset = "continue-at-reset", TellCheck = "tell-check", Deliver = "deliver", HandOff = "hand-off",
    Record = "record", NoChange = "no-change", CiFail = "ci-fail", UnstickCi = "unstick-ci", Merge = "merge",
    RecordMerge = "record-merge", ApplyDecision = "apply-decision", RaiseCard = "raise-card", Remind = "remind",
    TakeDefault = "take-default", ToOwner = "to-owner", Expire = "expire", CloseSuperseded = "close-superseded",
    RunCard = "run-card", KillRun = "kill-run", DeliverHandoff = "deliver-handoff",
    TellOrchestrator = "tell-orchestrator", Release = "release", EndPass = "end-pass", DropMarker = "drop-marker",
    Clean = "clean", Reconcile = "reconcile",
});

/// A row's clock: a `[limits]` key, the subject's own timeout, or the owner's stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timeout {
    Limit(&'static str),
    Subject,
    OwnerHeld,
}

impl TryFrom<String> for Timeout {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        match s.as_str() {
            "subject" => Ok(Timeout::Subject),
            "owner-held" => Ok(Timeout::OwnerHeld),
            k => LIMIT_DOCS
                .iter()
                .find(|d| d.0 == k)
                .map(|d| Timeout::Limit(d.0))
                .ok_or_else(|| format!("timeout `{k}` is no [limits] key, `subject` or `owner-held`")),
        }
    }
}

impl<'de> Deserialize<'de> for Timeout {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Timeout::try_from(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

impl Timeout {
    pub fn name(self) -> &'static str {
        match self {
            Timeout::Limit(k) => k,
            Timeout::Subject => "subject",
            Timeout::OwnerHeld => "owner-held",
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Pick<T> {
    Any(Star),
    Of(Vec<T>),
}

#[derive(Deserialize)]
enum Star {
    #[serde(rename = "*")]
    Any,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WhenFile {
    lane: Option<Pick<Lane>>,
    host: Option<Pick<Host>>,
    pr: Option<Pick<Pr>>,
    verdict: Option<Pick<Verdict>>,
    ask: Option<Pick<Ask>>,
    turn: Option<Pick<Turn>>,
}

fn view<T: Copy>(p: Option<Pick<T>>, name: fn(T) -> &'static str) -> Option<Vec<&'static str>> {
    p.map(|p| match p {
        Pick::Any(Star::Any) => vec!["*"],
        Pick::Of(v) => v.into_iter().map(name).collect(),
    })
}

/// Per fact a row names, the values it matches (`*`: any).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct When(pub BTreeMap<Fact, Vec<&'static str>>);

impl From<WhenFile> for When {
    fn from(w: WhenFile) -> Self {
        let all = [
            (Fact::Lane, view(w.lane, Lane::name)),
            (Fact::Host, view(w.host, Host::name)),
            (Fact::Pr, view(w.pr, Pr::name)),
            (Fact::Verdict, view(w.verdict, Verdict::name)),
            (Fact::Ask, view(w.ask, Ask::name)),
            (Fact::Turn, view(w.turn, Turn::name)),
        ];
        When(all.into_iter().filter_map(|(f, v)| Some((f, v?))).collect())
    }
}

impl When {
    pub fn matches(&self, c: &Cond) -> bool {
        self.0.iter().all(|(f, v)| v.contains(&"*") || v.contains(&c.value(*f)))
    }
    /// The row names `value` of `fact` itself, not through `*`.
    pub fn names(&self, fact: Fact, value: &str) -> bool {
        self.0.get(&fact).is_some_and(|v| v.contains(&value))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RowFile {
    id: String,
    when: WhenFile,
    #[serde(rename = "do")]
    act: Action,
    #[serde(default)]
    then: Vec<Action>,
    owner: Owner,
    timeout: Timeout,
    #[serde(default)]
    since: Since,
    #[serde(default)]
    escalate: Escalate,
    #[serde(default)]
    to: Vec<String>,
    say: String,
}

/// One row: when its condition holds, what happens, who owns it, by when, and how it escalates.
#[derive(Debug, Clone)]
pub struct Row {
    pub id: String,
    pub when: When,
    pub act: Action,
    pub then: Vec<Action>,
    pub owner: Owner,
    pub timeout: Timeout,
    pub since: Since,
    pub escalate: Escalate,
    pub to: Vec<String>,
    pub say: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StateFileSpec {
    flow: String,
    states: Vec<String>,
    reads: Vec<Fact>,
    ignores: Vec<Fact>,
    #[serde(default)]
    in_flight: bool,
    row: Vec<RowFile>,
}

/// One file of a flow: the states it covers, the facts they read and their rows, first match wins.
#[derive(Debug, Clone)]
pub struct StateFile {
    pub name: String,
    pub states: Vec<String>,
    pub reads: Vec<Fact>,
    pub in_flight: bool,
    pub rows: Vec<Row>,
}

impl StateFile {
    pub fn row(&self, c: &Cond) -> Option<&Row> {
        self.rows.iter().find(|r| r.when.matches(c))
    }
}

/// One flow (`issue`, `ask`, `card`, …) and its state files.
#[derive(Debug, Clone)]
pub struct Flow {
    pub name: String,
    pub files: Vec<StateFile>,
}

/// Every flow's rows, as `crates/core/flow/` holds them.
#[derive(Debug, Clone)]
pub struct Table {
    pub flows: Vec<Flow>,
}

impl Table {
    pub fn flow(&self, name: &str) -> Option<&Flow> {
        self.flows.iter().find(|f| f.name == name)
    }
}

fn file(path: &str, text: &str) -> Result<(String, StateFile)> {
    let (flow, name) = path
        .strip_suffix(".toml")
        .and_then(|p| p.split_once('/'))
        .ok_or_else(|| anyhow!("not <flow>/<state>.toml"))?;
    let spec: StateFileSpec = toml::from_str(text)?;
    if spec.flow != flow {
        bail!("flow `{}` in folder `{flow}`", spec.flow);
    }
    if Fact::ALL
        .iter()
        .any(|f| spec.reads.contains(f) == spec.ignores.contains(f))
    {
        bail!("reads and ignores must hold every fact once");
    }
    let mut rows: Vec<Row> = vec![];
    for r in spec.row {
        let id = format!("{flow}/{name}/{}", r.id);
        let when = When::from(r.when);
        if !when
            .0
            .keys()
            .eq(spec.reads.iter().collect::<std::collections::BTreeSet<_>>())
        {
            bail!("{id}: `when` must name exactly the facts in `reads`");
        }
        if when.0.values().any(Vec::is_empty) || r.then.len() > 2 || rows.iter().any(|x| x.id == id) {
            bail!("{id}: an empty value list, over two `then`, or a second row of this id");
        }
        rows.push(Row {
            id,
            when,
            act: r.act,
            then: r.then,
            owner: r.owner,
            timeout: r.timeout,
            since: r.since,
            escalate: r.escalate,
            to: r.to,
            say: r.say,
        });
    }
    let name = name.to_string();
    let (states, reads, in_flight) = (spec.states, spec.reads, spec.in_flight);
    Ok((
        flow.to_string(),
        StateFile {
            name,
            states,
            reads,
            in_flight,
            rows,
        },
    ))
}

/// Load a table from `(path, text)` pairs, `path` being `<flow>/<state>.toml`.
pub fn parse(files: &[(&str, &str)]) -> Result<Table> {
    let mut flows: Vec<Flow> = vec![];
    for (path, text) in files {
        let (flow, f) = file(path, text).with_context(|| format!("crates/core/flow/{path}"))?;
        match flows.iter_mut().find(|x| x.name == flow) {
            Some(x) => x.files.push(f),
            None => flows.push(Flow {
                name: flow,
                files: vec![f],
            }),
        }
    }
    Ok(Table { flows })
}

/// The embedded table, loaded once.
pub fn table() -> Result<&'static Table> {
    static T: OnceLock<Result<Table, String>> = OnceLock::new();
    let rows: Vec<(&str, &str)> = FILES.iter().copied().filter(|f| f.0.contains('/')).collect();
    T.get_or_init(|| parse(&rows).map_err(|e| format!("{e:#}")))
        .as_ref()
        .map_err(|e| anyhow!("{e}"))
}

/// `crates/core/flow/outcomes.toml`: the refusal codes that queue, hold or end a row; any other code fails a try.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcomes {
    pub queued: Vec<String>,
    pub held: Vec<String>,
    #[serde(rename = "final")]
    pub ends: Vec<String>,
}

impl Outcomes {
    /// `code`'s class: `queued`, `held`, `final` or `failed`.
    pub fn class(&self, code: &str) -> &'static str {
        let has = |v: &[String]| v.iter().any(|c| c == code);
        [("queued", &self.queued), ("held", &self.held), ("final", &self.ends)]
            .into_iter()
            .find(|x| has(x.1))
            .map_or("failed", |x| x.0)
    }
}

/// The embedded `outcomes.toml`, loaded once.
pub fn outcomes() -> Result<&'static Outcomes> {
    static O: OnceLock<Result<Outcomes, String>> = OnceLock::new();
    let text = FILES.iter().find(|f| f.0 == "outcomes.toml").map_or("", |f| f.1);
    O.get_or_init(|| toml::from_str(text).map_err(|e| format!("crates/core/flow/outcomes.toml: {e}")))
        .as_ref()
        .map_err(|e| anyhow!("{e}"))
}
