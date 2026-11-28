use crate::config::{Account, Config};
use crate::runner::Runner;
use crate::shed::proto::{self, AccountState};
use crate::{gate::refused, Event};
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::BTreeSet;

/// Why an ACP start on the main host cannot run on its account.
#[derive(Clone, Debug, PartialEq)]
pub enum Why {
    Missing,
    SignedOut,
    Drifted,
    Limited(Option<u64>),
    Setup(String),
}

/// An agent's account that holds its starts: `name` is the one to sign in, none when shed has no account to name; `skipped`, each account `auto` passed over and why.
#[derive(Clone, Debug, PartialEq)]
pub struct Hold {
    pub agent: String,
    pub name: Option<String>,
    pub why: Why,
    pub skipped: Vec<(String, Why)>,
}

impl Hold {
    /// The hold in words, each account's fix last.
    pub fn line(&self, r: &dyn Runner) -> String {
        let a = &self.agent;
        match (self.skipped.as_slice(), self.name.as_deref()) {
            ([], None) => format!("no named {a} account in shed → shed account add {a} work"),
            ([], Some(n)) => one(r, a, n, &self.why),
            ([(n, why)], _) => one(r, a, n, why),
            (all, _) => {
                let each: Vec<_> = all.iter().map(|(n, why)| one(r, a, n, why)).collect();
                format!("no {a} account is ready: {}", each.join("; "))
            }
        }
    }
    /// The `agent/name` a [sign in] passes to `account.login`.
    pub fn signin(&self) -> Option<String> {
        let n = (self.name.as_ref()).filter(|_| matches!(self.why, Why::SignedOut | Why::Drifted))?;
        Some(format!("{}/{n}", self.agent))
    }
}

fn one(r: &dyn Runner, a: &str, n: &str, why: &Why) -> String {
    match why {
        Why::Missing => format!("no signed-in {a} account {n} in shed → shed account add {a} {n}"),
        Why::SignedOut => format!("{a} account {n} is signed out → sign in (shed account login {a} {n})"),
        Why::Drifted => format!("{a} account {n} drifted → sign in again (shed account login {a} {n})"),
        Why::Limited(at) => {
            let until = at.map_or("its reset".into(), |at| crate::usage::date(r, at, "+%a %H:%M"));
            format!("{a} account {n} hit its usage limit until {until}")
        }
        Why::Setup(f) => {
            format!("{a} account {n} holds a setup ({f}): its run would not load drover's profile → remove those from its folder")
        }
    }
}

/// What shed's `account.ls` lists on the main host; none when it does not answer, so nothing is held on it.
pub fn ls(cfg: &Config, r: &dyn Runner) -> Option<Vec<proto::Account>> {
    let w = r.shed().or_else(|| cfg.shed())?.wire;
    crate::shed::accounts(r, &w).ok()
}

/// The hold on agent `agent` starting on `a`, from `all`: a named account missing, out, limited or holding a setup; for `auto`, no named one ready.
pub fn of(all: &[proto::Account], agent: &str, a: &Account) -> Option<Hold> {
    let mine = all.iter().filter(|x| x.provider == agent);
    let hold = |name: Option<&str>, why, skipped| {
        Some(Hold {
            agent: agent.into(),
            name: name.map(String::from),
            why,
            skipped,
        })
    };
    let state = |x: &proto::Account| match x.state {
        _ if !x.setup.is_empty() => Some(Why::Setup(x.setup.join(", "))),
        AccountState::Ready => None,
        AccountState::Limited => Some(Why::Limited(x.limited_until)),
        AccountState::SignedOut => Some(Why::SignedOut),
        AccountState::Drifted => Some(Why::Drifted),
    };
    if let Account::Named(n) = a {
        return match mine.clone().find(|x| x.name == *n) {
            None => hold(Some(n), Why::Missing, vec![]),
            Some(x) => hold(Some(n), state(x)?, vec![]),
        };
    }
    let named = mine.filter(|x| x.name != "default");
    let skipped = named.map(|x| Some((x, state(x)?))).collect::<Option<Vec<_>>>()?;
    let open: Vec<_> = skipped.iter().filter(|x| !matches!(x.1, Why::Setup(_))).collect();
    let limited = open.iter().filter(|x| matches!(x.1, Why::Limited(_)));
    let soonest = limited.min_by_key(|x| x.0.limited_until.unwrap_or(u64::MAX));
    let all = skipped.iter().map(|(x, why)| (x.name.clone(), why.clone())).collect();
    match soonest.or_else(|| open.first()) {
        Some((x, why)) => hold(Some(&x.name), why.clone(), all),
        None => hold(None, Why::Missing, all),
    }
}

/// `spawn` on the main host: `ACP_UNGUARDED` for an account holding a setup, `LOGIN_NEEDED` while it cannot run.
pub fn admit(cfg: &Config, r: &dyn Runner, agent: &str, a: &Account) -> Result<()> {
    match ls(cfg, r).and_then(|all| of(&all, agent, a)) {
        None => Ok(()),
        Some(h) if matches!(h.why, Why::Setup(_)) => {
            refused("ACP_UNGUARDED", format!("{}; the lane does not start", h.line(r)))
        }
        Some(h) => refused("LOGIN_NEEDED", format!("{}; new lanes on it wait", h.line(r))),
    }
}

fn used(cfg: &Config) -> BTreeSet<(String, Account)> {
    let acp = cfg.fleet.iter().filter(|(n, _)| cfg.shed_for(n).is_some());
    let roles = acp.flat_map(|(n, f)| {
        let chat = cfg
            .chat
            .harness
            .iter()
            .map(|h| (h.clone(), cfg.account(n, "orchestrator")));
        let roles = f.role.iter().map(|(r, x)| (x.harness.clone(), cfg.account(n, r)));
        roles.chain(chat).collect::<Vec<_>>()
    });
    roles
        .filter(|(h, _)| crate::providers::get(h).acp().is_some())
        .collect()
}

/// `check`'s alerts: one `signed-out` or `usage-limit` per account and episode (until a sign-in, or a limit's end), with the [sign in] it offers.
pub fn alerts(cfg: &Config, r: &dyn Runner, events: &[Event]) -> Vec<(&'static str, Value)> {
    let Some(all) = ls(cfg, r) else { return vec![] };
    let holds = used(cfg).into_iter().filter_map(|(h, a)| of(&all, &h, &a));
    let holds: std::collections::BTreeMap<_, _> = holds
        .filter(|h| !matches!(h.why, Why::Setup(_)))
        .map(|h| (format!("{}/{}", h.agent, h.name.as_deref().unwrap_or_default()), h))
        .collect();
    let alert = |(at, h): (String, Hold)| {
        let tried = events.iter().rfind(|e| e.kind == "setup" && e.str("name") == Some(&at));
        let kind = if matches!(h.why, Why::Limited(_)) {
            "usage-limit"
        } else {
            "signed-out"
        };
        let key = format!("account:{at}:{:?}:{}", h.why, tried.map_or(0, |e| e.ts));
        (
            kind,
            json!({"key": key, "account": at, "reason": h.line(r), "signin": h.signin()}),
        )
    };
    holds.into_iter().map(alert).collect()
}
