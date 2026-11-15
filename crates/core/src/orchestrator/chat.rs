use super::{attached, brief, defaults, hold, name, role, DEFAULT};
use crate::config::{Config, Role};
use crate::events::working;
use crate::gate::refused;
use crate::{providers, Ctx};
use anyhow::Result;
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// One orchestrator as a cooee chat: who it is, and the fleet its lane identity names.
pub struct Seat<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub fleet: &'a str,
    pub fleets: Vec<&'a str>,
    pub role: Role,
}

/// Every orchestrator id: `default`, then each `[orchestrator.<id>]`.
pub fn ids(cfg: &Config) -> impl Iterator<Item = &str> {
    std::iter::once(DEFAULT).chain(cfg.orchestrator.more.keys().map(String::as_str))
}

/// Orchestrator `id`'s seat: `NO_ORCHESTRATOR` when none of its fleets has an orchestrator role.
pub fn seat<'a>(cfg: &'a Config, id: &str) -> Result<Seat<'a>> {
    let Some(id) = ids(cfg).find(|i| *i == id) else {
        return refused("BAD_VALUE", format!("no orchestrator {id}"));
    };
    let own = cfg.orchestrator.more.get(id).map(|o| &o.fleets);
    let fleets: Vec<&str> = match own.filter(|f| !f.iter().any(|f| f == "all")) {
        Some(f) => f.iter().map(String::as_str).collect(),
        None => cfg.fleet.keys().map(String::as_str).collect(),
    };
    let Some((fleet, _)) = fleets.iter().find_map(|f| role(cfg, f).ok()) else {
        return refused(
            "NO_ORCHESTRATOR",
            format!("none of {id}'s fleets has an orchestrator role"),
        );
    };
    Ok(Seat {
        id,
        name: name(cfg, id),
        fleet,
        fleets,
        role: defaults(cfg, fleet)?.1,
    })
}

/// One entry of `GET /cooee/profiles`.
#[derive(Serialize)]
pub struct ChatProfile {
    name: String,
    label: String,
    agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    settings: Option<PathBuf>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    env: BTreeMap<&'static str, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    hidden: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    why: String,
}

impl ChatProfile {
    fn offered(seat: &Seat, agent: &str, instructions: PathBuf, settings: PathBuf, policy: Option<String>) -> Self {
        let env = BTreeMap::from([
            ("DROVER_LANE", format!("{}:-:orchestrator", seat.fleet)),
            ("DROVER_ORCHESTRATOR", seat.id.into()),
            ("DROVER_THREAD", "{thread}".into()),
        ]);
        Self {
            name: seat.id.into(),
            label: seat.name.into(),
            agent: agent.into(),
            instructions: Some(instructions),
            settings: Some(settings),
            env,
            policy,
            hidden: false,
            why: String::new(),
        }
    }

    fn hidden(id: &str, label: &str, agent: &str, why: String) -> Self {
        Self {
            name: id.into(),
            label: label.into(),
            agent: agent.into(),
            instructions: None,
            settings: None,
            env: BTreeMap::new(),
            policy: None,
            hidden: true,
            why,
        }
    }
}

/// Every orchestrator's chat profile; an offered one's instructions file is rewritten when its text changed.
pub fn profiles(ctx: &Ctx, cfg: &Config) -> Result<Vec<ChatProfile>> {
    ids(cfg).map(|id| profile(ctx, cfg, id)).collect()
}

fn profile(ctx: &Ctx, cfg: &Config, id: &str) -> Result<ChatProfile> {
    let seat = match seat(cfg, id) {
        Ok(s) => s,
        Err(e) => return Ok(ChatProfile::hidden(id, name(cfg, id), "", e.to_string())),
    };
    let h = providers::get(&seat.role.harness);
    let Some(agent) = h.acp() else {
        let why = format!("harness {} has no ACP agent", seat.role.harness);
        return Ok(ChatProfile::hidden(id, seat.name, "", why));
    };
    let p = seat.role.profile_or("orchestrator");
    let why = crate::spawn::unguarded(ctx, (&p, "orchestrator", &seat.role.harness), agent, false);
    if !why.is_empty() {
        return Ok(ChatProfile::hidden(id, seat.name, agent, why));
    }
    let file = ctx
        .state
        .parent()
        .unwrap_or(&ctx.state)
        .join("chat")
        .join(format!("{id}.md"));
    let text = brief::chat(cfg, &seat);
    if std::fs::read_to_string(&file).ok().as_deref() != Some(text.as_str()) {
        std::fs::create_dir_all(file.parent().unwrap_or(&file))?;
        let tmp = file.with_extension("md.tmp");
        std::fs::write(&tmp, &text)?;
        std::fs::rename(&tmp, &file)?;
    }
    let settings = h.chat_settings(&providers::profiles_root(ctx).join(&p));
    let rt = cfg
        .fleet
        .get(seat.fleet)
        .and_then(|f| f.runtime.as_ref())
        .or(cfg.runtime.as_ref());
    Ok(ChatProfile::offered(
        &seat,
        agent,
        file,
        settings,
        rt.and_then(|r| r.acp_policy.clone()),
    ))
}

/// `POST /cooee/thread-started`: cooee `thread` is orchestrator `id`'s chat, linked once (`via: profile`).
pub fn started(ctx: &Ctx, cfg: &Config, id: &str, thread: &str) -> Result<String> {
    let seat = seat(cfg, id)?;
    let line = format!("{thread} is {id}'s chat on {}", seat.fleet);
    if attached(&working(&ctx.state)?, thread).is_some_and(|e| e.fleet == seat.fleet) {
        return Ok(line);
    }
    hold(
        ctx,
        cfg,
        seat.fleet,
        thread,
        json!({"orchestrator": id, "via": "profile"}),
    )?;
    Ok(line)
}
