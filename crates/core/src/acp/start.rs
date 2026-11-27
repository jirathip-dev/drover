use super::Guard;
use crate::config::Config;
use crate::gate::refused;
use crate::providers;
use crate::runner::{argv, Runner};
use crate::spawn::{shed_meta, unguarded, Plan, BACKGROUND_EXEC, SHED_OLD};
use crate::term;
use crate::Ctx;
use anyhow::Result;
use std::path::PathBuf;

/// Plan `p`'s ACP start for role `role` of `fleet` on agent `id` under runtime policy `policy`: its guard (a judged lane's `DROVER_GUARD`), the words and the agent's record, after the checks (`BAD_VALUE`, `REVIEW_SAME_PROVIDER`, and unless `dry_run` `LOGIN_NEEDED` and the test run).
pub fn of(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    p: &mut Plan,
    (fleet, role, id, policy): (&str, &str, &str, &str),
    (remote, dry_run): (Option<&str>, bool),
) -> Result<(term::Acp, crate::agents::Agent, Guard)> {
    let guard = super::guard(policy, id);
    if guard != Guard::Hooked {
        p.env.insert("DROVER_GUARD".into(), guard.word().into());
    }
    let catalog = crate::agents::Catalog::read(r)?;
    let (acp, agent) = acp_cmd(ctx, r, p, (id, guard, &catalog), remote)?;
    let f = cfg.fleet(fleet)?;
    if guard == Guard::Judge {
        let own = f.role(role)?;
        crate::agents::fits(&agent, own.model.as_deref(), own.effort.as_deref())?;
    }
    if f.family(role) == "rev" {
        same_vendor(ctx, cfg, fleet, &agent, &catalog)?;
    }
    if dry_run {
        return Ok((acp, agent, guard));
    }
    if let Some(a) = &p.account {
        crate::accounts::admit(cfg, r, &agent.vendor, a)?;
    }
    if guard != Guard::Hooked {
        super::probe::guarded(ctx, cfg, r, &agent, p.account.as_ref())?;
    }
    Ok((acp, agent, guard))
}

/// An ACP lane's start words for plan `p` on agent `id`, refused `ACP_UNGUARDED` when its guard cannot run here.
pub fn acp_cmd(
    ctx: &Ctx,
    r: &dyn Runner,
    p: &Plan,
    (id, guard, catalog): (&str, Guard, &crate::agents::Catalog),
    remote: Option<&str>,
) -> Result<(term::Acp, crate::agents::Agent)> {
    let dir = providers::profiles_root(ctx).join(&p.profile);
    let home = |a: Vec<String>| crate::hosts::rehome(a, remote);
    let why = match guard {
        Guard::Hooked => unguarded(ctx, (&p.profile, p.family, &p.harness), id),
        _ if remote.is_some() => format!("agent {id} is judged, and a judged lane runs on the main host only"),
        _ => crate::acp::ready(ctx, r).map_or_else(|w| format!("agent {id}: {w}"), |()| String::new()),
    };
    if !why.is_empty() {
        return refused("ACP_UNGUARDED", format!("{why}; the lane does not start"));
    }
    if !shed_meta(r) {
        return refused("SHED_OLD", format!("{SHED_OLD}; the lane does not start"));
    }
    let agent = crate::agents::installed(catalog, id)?;
    let cmd = agent.installed.clone().map(|i| i.cmd).unwrap_or_default();
    let mut words = home(p.lane_cmd("").get(3..).unwrap_or_default().to_vec());
    let at = words.iter().position(|w| w.starts_with("&&")).unwrap_or(words.len());
    let q = |v: &str| format!("'{}'", v.replace('\'', r"'\''"));
    let dir = PathBuf::from(home(vec![dir.display().to_string()]).remove(0));
    let env = providers::get(&p.harness).acp_env(p.model.as_deref(), p.effort.as_deref());
    let own: Vec<String> = env.iter().map(|(k, v)| format!("{k}={}", q(v))).collect();
    words.splice(at..at, own);
    let (set, role) = (dir.join("settings.json"), dir.join("role.md"));
    let (set, role) = (q(&set.to_string_lossy()), q(&role.to_string_lossy()));
    let profile = providers::get(&p.harness).acp_profile(&set, &role);
    let run = p.qos().map_or("exec \"$@\"", |_| BACKGROUND_EXEC);
    let line = words.join(" ") + &profile + " && unset SHED BERTH && " + run;
    let meta = providers::get(&p.harness).acp_meta();
    let meta = meta.map(|m| serde_json::from_str(&home(vec![m.to_string()]).remove(0)).unwrap_or(m));
    let own = guard == Guard::Judge;
    let judge = (guard != Guard::Hooked).then(|| term::Judged {
        cmd: crate::acp::command(ctx),
        secs: crate::config::limits().acp_judge_secs,
        env: p.judge_env(),
        model: p.model.clone().filter(|_| own && agent.takes_model),
        effort: p.effort.clone().filter(|e| own && agent.efforts.contains(e)),
    });
    let acp = term::Acp {
        cmd: [argv(&["sh", "-c", &line, "sh"]), cmd].concat(),
        mcp: (meta.is_some() || !p.mcp.is_empty()).then(|| p.mcp_file.clone()),
        meta,
        awake: p.sources_secrets(),
        account: p.account.clone(),
        judge,
    };
    Ok((acp, agent))
}

fn same_vendor(
    ctx: &Ctx,
    cfg: &Config,
    fleet: &str,
    agent: &crate::agents::Agent,
    c: &crate::agents::Catalog,
) -> Result<()> {
    let f = cfg.fleet(fleet)?;
    let (Some(i), Some(v)) = (f.role.get("impl"), f.role.get("rev")) else {
        return Ok(());
    };
    let run = cfg.shed_for(fleet).and_then(|b| b.acp);
    let id = i
        .agent
        .clone()
        .or_else(|| providers::get(&i.harness).acp().map(Into::into))
        .or(run);
    let theirs = id.and_then(|id| Some(c.get(&id)?.vendor.clone()));
    if i.agent.is_none() && v.agent.is_none() || theirs.as_deref() != Some(agent.vendor.as_str()) {
        return Ok(());
    }
    let text = std::fs::read_to_string(&ctx.config).unwrap_or_default();
    let mut head = "";
    let why = text.lines().any(|l| {
        let t = l.trim();
        if t.starts_with('[') {
            head = t;
        }
        head == format!("[fleet.{fleet}]") && t.starts_with("rev_same_provider") && t.contains('#')
    });
    if f.rev_same_provider == Some(true) && why {
        return Ok(());
    }
    let msg = format!(
        "fleet.{fleet}: rev runs on impl's program {}; give rev another agent, or set rev_same_provider = true # <why>",
        agent.vendor
    );
    refused("REVIEW_SAME_PROVIDER", msg)
}
