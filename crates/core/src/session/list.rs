use super::{Session, Sessions};
use crate::config::Config;
use crate::hosts::On;
use crate::runner::Runner;
use anyhow::{Context, Result};
use serde_json::json;
use shed_proto::{LsParams, Method, Target, TerminalInfo};

fn wire(r: &dyn Runner) -> Result<crate::shed::Wire> {
    let s = r
        .shed()
        .context("no shed runtime: set `[runtime] kind = \"shed-acp\"` (docs/runtimes.md)")?;
    Ok(s.wire)
}

/// Every session shed on `r` lists, exited ones too.
pub fn on(r: &dyn Runner) -> Result<Vec<Session>> {
    let v = crate::shed::call(r, &wire(r)?, Method::Ls, json!(LsParams::default()))?;
    let all: Vec<TerminalInfo> = serde_json::from_value(v)?;
    Ok(all.into_iter().map(Session::from).collect())
}

/// The running sessions drover labelled (lanes and conductors) on `r`'s shed.
pub fn live_on(r: &dyn Runner) -> Result<Vec<Session>> {
    Ok(on(r)?.into_iter().filter(|s| s.running() && s.lane()).collect())
}

/// Session `name` on `r`'s shed, if it has one.
pub fn get(r: &dyn Runner, name: &str) -> Result<Option<Session>> {
    let got = crate::shed::call(r, &wire(r)?, Method::Get, json!(Target { name: name.into() }));
    match got {
        Ok(v) => Ok(Some(serde_json::from_value::<TerminalInfo>(v)?.into())),
        Err(e) if format!("{e:#}").starts_with("NO_SUCH_TERMINAL") => Ok(None),
        Err(e) => Err(e),
    }
}

/// This host's sessions, then each `[hosts]` machine's, each tagged with its host.
pub fn all(cfg: &Config, r: &dyn Runner) -> Sessions {
    let mut out = vec![(None, on(r).map_err(|e| format!("{e:#}")))];
    for (name, m) in cfg.hosts.iter().filter_map(|(n, h)| Some((n, h.machine.as_deref()?))) {
        let got = on(&On::new(r, Some(m))).map(|all| tagged(all, name, m));
        out.push((Some(m.to_string()), got.map_err(|e| format!("{e:#}"))));
    }
    Sessions(out)
}

/// `all` as the host at `machine` lists them, tagged with `host`.
pub fn tagged(all: Vec<Session>, host: &str, machine: &str) -> Vec<Session> {
    let tag = |s: Session| Session {
        host: Some(host.into()),
        machine: Some(machine.into()),
        ..s
    };
    all.into_iter().map(tag).collect()
}
