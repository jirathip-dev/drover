use super::Session;
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

/// Every terminal shed on `r` lists, exited ones too, as shed sends them.
pub fn raw(r: &dyn Runner) -> Result<Vec<TerminalInfo>> {
    let v = crate::shed::call(r, &wire(r)?, Method::Ls, json!(LsParams::default()))?;
    Ok(serde_json::from_value(v)?)
}

/// Every session shed on `r` lists, exited ones too.
pub fn on(r: &dyn Runner) -> Result<Vec<Session>> {
    Ok(raw(r)?.into_iter().map(Session::from).collect())
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

/// `all` as the host at `machine` lists them, tagged with `host`.
pub fn tagged(all: Vec<Session>, host: &str, machine: &str) -> Vec<Session> {
    let tag = |s: Session| Session {
        host: Some(host.into()),
        machine: Some(machine.into()),
        ..s
    };
    all.into_iter().map(tag).collect()
}
