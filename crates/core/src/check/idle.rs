use super::Live;
use crate::events::{checks, fold, last_spawn, working};
use crate::Ctx;
use anyhow::Result;
use std::collections::BTreeMap;

pub(super) type Idle = BTreeMap<String, &'static str>;

pub(super) fn idle(ctx: &Ctx, live: &Live) -> Result<Idle> {
    let events = working(&ctx.state)?;
    let states = fold(&events);
    let why = |a: &str| {
        let e = last_spawn(&events, a)?;
        let s = states.get(&(e.fleet.clone(), e.issue?))?;
        match s.state.as_str() {
            "needs-human" => Some("needs-human"),
            "parked" => Some("parked"),
            "queued" => Some("queued"),
            st if s.lane.as_deref() != Some(a) || checks(e) && !st.starts_with("review") => Some("stage moved on"),
            _ => None,
        }
    };
    Ok(live.iter().filter_map(|a| Some((a.clone(), why(a)?))).collect())
}

pub(super) fn busy(ctx: &Ctx, live: &Live) -> Result<Live> {
    let idle = idle(ctx, live)?;
    Ok(live.iter().filter(|a| !idle.contains_key(*a)).cloned().collect())
}
