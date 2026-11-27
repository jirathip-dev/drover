use crate::access;
use crate::clone::refspec;
use crate::config::{expand, Config, Fleet, Role};
use crate::events::{emit, fold, last_spawn, plain, working, Event, Json};
use crate::gate::refused;
use crate::orchestrator::actor;
use crate::providers;
use crate::runner::{argv, ok, Runner};
use crate::session::{Session, Status};
use crate::term;
use crate::Ctx;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn role_prompt(role: &str) -> &'static str {
    match role {
        "impl" => include_str!("../roles/impl.md"),
        "design" => include_str!("../roles/design.md"),
        "architect" => include_str!("../roles/architect.md"),
        "orchestrator" => concat!(
            include_str!("../roles/orchestrator/role.md"),
            "\n",
            include_str!("../roles/orchestrator/thread.md")
        ),
        "lesson" => include_str!("../roles/lesson.md"),
        "triage" => include_str!("../roles/triage.md"),
        "deps" => concat!(include_str!("../roles/impl.md"), "\n", include_str!("../roles/deps.md")),
        "verify" => include_str!("../roles/verify.md"),
        _ => include_str!("../roles/rev.md"),
    }
}

fn lane_prompt(f: &Fleet, role: &str) -> String {
    let extra =
        f.role.get(role).and_then(|r| r.prompt.as_deref()).map(|p| {
            std::fs::read_to_string(expand(p)).unwrap_or_else(|e| format!("(prompt file {p} unreadable: {e})"))
        });
    let base = role_prompt(prompt_kind(f, role));
    extra.map_or_else(|| base.into(), |x| format!("{}\n\n{}", base.trim_end(), x.trim_end()))
}

fn prompt_kind<'a>(f: &'a Fleet, role: &'a str) -> &'a str {
    let own = matches!(role, "lesson" | "triage" | "deps") || role == "verify" && f.family(role) == "rev";
    if own {
        role
    } else {
        f.family(role)
    }
}

pub(crate) fn profiles(cfg: &Config) -> Vec<(&str, &Fleet, &str, &Role, String)> {
    let mut done = std::collections::BTreeSet::new();
    let all = cfg.fleet.iter().flat_map(|(name, f)| {
        f.role
            .iter()
            .map(move |(role, r)| (name.as_str(), f, role.as_str(), r, r.profile_or(role)))
    });
    all.filter(|p| done.insert(p.4.clone())).collect()
}

/// Write every harness's files for every role's profile, so `drover set` alone can switch a role's harness; a generated profile no role names any more (a retired role's) is removed.
pub fn profiles_sync(ctx: &Ctx, cfg: &Config, usage: &str, out: &mut dyn Write) -> Result<()> {
    let all = profiles(cfg);
    for (_, f, role, r, profile) in &all {
        access::mcp_servers(cfg, f, r)?;
        for h in providers::known() {
            providers::get(h).sync(ctx, f.family(role), prompt_kind(f, role), profile, usage, out)?;
        }
    }
    let root = providers::profiles_root(ctx);
    for d in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let (name, dir) = (d.file_name().to_string_lossy().into_owned(), d.path());
        if dir.join("role.md").exists() && !all.iter().any(|p| p.4 == name) {
            std::fs::remove_dir_all(&dir)?;
            writeln!(out, "removed stale {}", dir.display())?;
        }
    }
    Ok(())
}

/// `profiles sync --check`: no stray hook, and each provider's profiles allow their hook (`Provider::hooks_allowed`).
pub fn profiles_check(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write) -> Result<bool> {
    let stray = stray_hooks(ctx, cfg);
    let mut good = stray.is_empty();
    for s in stray {
        writeln!(out, "✗ {s} → drover profiles sync")?;
    }
    for p in providers::lanes() {
        good &= p.hooks_allowed(ctx, cfg, r, out)?;
    }
    Ok(good)
}

/// Each synced profile's drover hook whose program is not the running binary: `<profile> hook runs <exe>`.
pub fn stray_hooks(ctx: &Ctx, cfg: &Config) -> Vec<String> {
    let mut stray = vec![];
    for (_, _, _, _, profile) in profiles(cfg) {
        let path = providers::profiles_root(ctx).join(&profile).join("settings.json");
        let settings: Value = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let pre = settings.at("hooks").at("PreToolUse").as_array().into_iter().flatten();
        for h in pre.flat_map(|m| m.at("hooks").as_array().into_iter().flatten()) {
            let cmd = h.at("command").as_str().unwrap_or_default();
            if let Some(exe) = providers::hook_program(cmd).filter(|_| !providers::runs_this(ctx, cmd)) {
                stray.push(format!("{profile} hook runs {exe}, not {}", ctx.exe));
            }
        }
    }
    stray
}

pub struct Plan {
    pub agent: String,
    pub role: String,
    /// The pipeline stage it runs and the role's kind (`impl`, `rev` for a check, `design`).
    pub stage: String,
    pub family: &'static str,
    pub harness: String,
    pub profile: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub branch: String,
    pub base: String,
    pub worktree: PathBuf,
    pub brief: PathBuf,
    pub kick: String,
    /// `DROVER_LANE` value (`<fleet>:<issue>:<role>`); arms `drover hook` in the lane.
    pub lane: String,
    pub env: std::collections::BTreeMap<String, String>,
    pub port: u16,
    /// The only MCP servers the lane gets, and the lane-local file a harness reads them from.
    pub mcp: serde_json::Map<String, Value>,
    pub mcp_file: PathBuf,
    /// Display title head (`<fleet> · <role> #<issue>`) and the full lane title built from it.
    pub head: String,
    pub title: String,
    /// The fleet's `sandbox`.
    pub sandbox: Option<crate::config::Sandbox>,
    /// The shed account an ACP start on the main host asks for; shed sets its login, so the start keeps the token variable.
    pub account: Option<crate::config::Account>,
}

/// A build or check lane's shell and all it starts under macOS background `QoS`, else `nice 10` and idle I/O.
pub const BACKGROUND: &str = "&& if [ -x /usr/sbin/taskpolicy ]; then /usr/sbin/taskpolicy -b -p $$; else renice -n 10 -p $$ >/dev/null; ionice -c3 -p $$ 2>/dev/null; true; fi";

/// Execs the lane's adapter under background `QoS` from launch, which a program's own process title cannot clear.
pub const BACKGROUND_EXEC: &str =
    "if [ -x /usr/sbin/taskpolicy ]; then exec /usr/sbin/taskpolicy -b \"$@\"; else exec \"$@\"; fi";

impl Plan {
    /// Build and check lanes run in the background; design, architect and orchestrator lanes do not.
    pub fn qos(&self) -> Option<&'static str> {
        matches!(self.family, "impl" | "rev").then_some("background")
    }

    /// Exports `DROVER_LANE`, `DROVER_PORT` and the fleet's `env`; sources the secrets `deliver` wrote.
    pub fn lane_cmd(&self, pane: &str) -> Vec<String> {
        let (lane, port) = (format!("DROVER_LANE='{}'", self.lane), self.port.to_string());
        let mut c = term::run_argv(pane, &["export", &lane, &format!("DROVER_PORT={port}")]);
        let v = |v: &str| {
            expand(v)
                .display()
                .to_string()
                .replace("$DROVER_PORT", &port)
                .replace("$HERDSMAN_PORT", &port)
        };
        let plain = self.env.iter().filter(|(_, x)| !x.starts_with("secret:"));
        c.extend(plain.map(|(k, x)| format!("{k}='{}'", v(x))));
        c.extend(providers::get(&self.harness).env().iter().map(|v| String::from(*v)));
        if let Some(v) = providers::get(&self.harness)
            .token_env()
            .filter(|_| self.account.is_none())
        {
            c.push(format!("&& unset {v}"));
        }
        if self.sources_secrets() {
            let f = self.secrets_file().display().to_string().replace('\'', r"'\''");
            c.push(format!("&& . '{f}' && rm -f '{f}'"));
        }
        c.extend(self.qos().map(|_| BACKGROUND.to_string()));
        c
    }

    /// Its start sources the secrets file `deliver` writes and removes once the lane runs.
    pub fn sources_secrets(&self) -> bool {
        !self.secrets().is_empty()
    }

    fn secrets(&self) -> Vec<(String, &str, String)> {
        let fleet = self.lane.split(':').next().unwrap_or_default();
        let env = self
            .env
            .iter()
            .filter_map(|(k, x)| Some((k.clone(), fleet, x.strip_prefix("secret:")?.into())));
        env.collect()
    }

    /// The file `deliver` writes beside the worktree, on the lane's host.
    pub fn secrets_file(&self) -> PathBuf {
        PathBuf::from(format!("{}.env", self.worktree.display()))
    }

    /// A remote lane's hook rules, beside its worktree on its host.
    pub fn rules_file(&self) -> PathBuf {
        PathBuf::from(format!("{}.rules.json", self.worktree.display()))
    }

    fn at(&mut self, root: &Path, f: &Fleet) {
        self.env.extend(shared_target(root, f, &self.agent));
        self.worktree = root.join(f.repo_name()).join(&self.branch);
        self.brief = self.worktree.join(".brief.md");
        self.mcp_file = self.worktree.join(".mcp.lane.json");
        self.kick = format!("Read {} and execute it fully.", self.brief.display());
    }
}

/// The lane's secrets read from 0600 files on the main host (`<config>/secrets/<NAME>`) and
pub fn deliver<'a>(ctx: &'a Ctx, on: &'a crate::hosts::On<'a>, p: &Plan) -> Result<crate::secret::Delivered<'a>> {
    let mut text = String::new();
    for (var, _, name) in p.secrets() {
        let f = crate::secret::path(ctx, &name);
        let why = || {
            format!(
                "{var}: secret {name} is not in {} (`drover secret request`)",
                f.display()
            )
        };
        let v = crate::secret::file(&f)?.with_context(why)?;
        text += &format!("export {var}='{}'\n", v.replace('\'', r"'\''"));
    }
    let file = (!text.is_empty()).then(|| p.secrets_file());
    if let Some(f) = &file {
        on.write_private(f, &text)?;
    }
    Ok(crate::secret::Delivered {
        ctx,
        on,
        lane: p.lane.clone(),
        file,
    })
}

const TITLE_MAX: usize = 60;

/// `<head> · <short issue title>`, ≤ 60 chars (not bytes): `feat:`/`P0:` prefixes dropped, cut at a word with `…`.
pub fn lane_title(head: &str, issue_title: &str) -> String {
    let mut t = issue_title.trim();
    while let Some((pre, rest)) = t.split_once(':') {
        let pre = pre.strip_suffix('!').unwrap_or(pre);
        let tok = pre
            .split_once('(')
            .map_or(pre, |(a, b)| if b.ends_with(')') { a } else { "" });
        if tok.is_empty() || !tok.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            break;
        }
        t = rest.trim_start();
    }
    if t.is_empty() {
        return head.to_string();
    }
    let room = TITLE_MAX.saturating_sub(head.chars().count() + 3);
    if t.chars().count() <= room {
        return format!("{head} · {t}");
    }
    let cut: String = t.chars().take(room.saturating_sub(1)).collect();
    let cut = match cut.rfind(char::is_whitespace) {
        Some(i) if i > 0 => &cut[..i],
        _ => &cut,
    };
    match cut.trim_end() {
        "" => head.to_string(),
        c => format!("{head} · {c}…"),
    }
}

/// `<fleet>/<branch>` hashed (FNV-1a) into `range` (default 41000-41999); a `taken` port takes the next free one.
pub fn lane_port(range: Option<&str>, key: &str, taken: &[u16]) -> u16 {
    let r = range.and_then(|r| r.split_once('-'));
    let r = r.and_then(|(a, b)| Some((a.trim().parse::<u16>().ok()?, b.trim().parse::<u16>().ok()?)));
    let (lo, hi) = r.filter(|(a, b)| a <= b).unwrap_or((41000, 41999));
    let n = u32::from(hi - lo) + 1;
    let fnv = |h: u32, b: u8| (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    let h = key.bytes().fold(0x811c_9dc5, fnv) % n;
    let at = |i: u32| lo + u16::try_from((h + i) % n).unwrap_or_default();
    (0..n).map(at).find(|p| !taken.contains(p)).unwrap_or(at(0))
}

pub fn plan(ctx: &Ctx, cfg: &Config, fleet: &str, issue: u64, role: &str) -> Result<Plan> {
    let f = cfg.fleet(fleet)?;
    let r = f.role(role)?;
    let (agent, branch, base, worktree, brief, head) = {
        let ev = working(&ctx.state)?;
        let st = fold(&ev).remove(&(fleet.to_string(), issue));
        if st.as_ref().is_some_and(|s| s.state == "merged") {
            return refused(
                "MERGED",
                format!("{fleet}#{issue} is merged; a merged issue never reopens"),
            );
        }
        let impl_branch = crate::gate::branch_of(st.as_ref(), issue);
        let (agent, branch, base, head) = if f.family(role) == "rev" {
            let rounds = st.as_ref().and_then(|s| s.stage_rounds.get(f.flow().stage_of(role)));
            let k = rounds.copied().unwrap_or_default() + 1;
            let head = format!("{fleet} · {role} #{issue} r{k}");
            (
                format!("{fleet}-{role}-{issue}-r{k}"),
                format!("{role}-{issue}-r{k}"),
                format!("origin/{impl_branch}"),
                head,
            )
        } else if crate::events::STAGES.contains(&role) {
            let head = format!("{fleet} · {role} #{issue}");
            let pre = st
                .as_ref()
                .is_none_or(|s| crate::events::DESIGN.contains(&s.state.as_str()));
            let pre = pre
                && crate::queue::replay(&ev, fleet)
                    .iter()
                    .any(|i| i.issue == issue && (i.design || i.architect));
            (
                format!("{fleet}-{role}-{issue}"),
                if pre { impl_branch } else { format!("{role}-{issue}") },
                format!("origin/{}", f.integration),
                head,
            )
        } else {
            (
                format!("{fleet}-{role}-{issue}"),
                impl_branch,
                format!("origin/{}", f.integration),
                format!("{fleet} · {role} #{issue}"),
            )
        };
        let wt = ctx.worktrees.join(f.repo_name()).join(&branch);
        (agent, branch, base, wt.clone(), wt.join(".brief.md"), head)
    };
    let kick = format!("Read {} and execute it fully.", brief.display());
    let lane = format!("{fleet}:{issue}:{role}");
    let mcp_file = worktree.join(".mcp.lane.json");
    let events = working(&ctx.state)?;
    let st = fold(&events);
    let open = st.values().filter(|s| !s.done()).flat_map(|s| s.lanes.values());
    let taken: Vec<u16> = open
        .filter_map(|a| last_spawn(&events, a))
        .filter(|e| e.fleet != fleet || e.str("branch") != Some(&branch))
        .filter_map(|e| e.data.get("port")?.as_u64()?.try_into().ok())
        .collect();
    let port = lane_port(f.port_range.as_deref(), &format!("{fleet}/{branch}"), &taken);
    let title = lane_title(&head, &crate::gh::cached_title(ctx, fleet, issue).unwrap_or_default());
    let env = shared_target(&ctx.worktrees, f, &agent)
        .chain(f.env.clone())
        .chain(f.shared_cache.clone())
        .collect();
    Ok(Plan {
        agent,
        role: role.into(),
        stage: f.flow().stage_of(role).into(),
        family: f.family(role),
        harness: r.harness.clone(),
        profile: r.profile_or(f.role_key(role)),
        model: r.model.clone(),
        effort: r.effort_or(&cfg.limits),
        branch,
        base,
        worktree,
        brief,
        kick,
        lane,
        env,
        port,
        mcp: access::mcp_servers(cfg, f, r)?,
        mcp_file,
        head,
        title,
        sandbox: f.sandbox,
        account: None,
    })
}

fn shared_target<'a>(root: &Path, f: &'a Fleet, agent: &str) -> impl Iterator<Item = (String, String)> + 'a {
    let b = root.join(crate::roots::BUILD);
    let d = b.join(agent);
    let s = b.join(SCRATCH).join(agent).display().to_string();
    let mut vars = vec![
        ("CARGO_TARGET_DIR".into(), d.display().to_string()),
        ("DROVER_SCRATCH_TARGET".into(), s),
    ];
    let swift = SWIFT.iter().filter(|_| f.swift_build);
    vars.extend(swift.map(|(k, n)| ((*k).into(), d.join(n).display().to_string())));
    vars.into_iter()
        .filter(|v| !f.env.contains_key(&v.0) && !f.shared_cache.contains_key(&v.0))
}

/// #1478: a `swift_build` fleet's Swift build folders, each an env var naming a folder in the lane's `.build/<agent>`.
pub const SWIFT: [(&str, &str); 2] = [("DROVER_DERIVED_DATA", "xcode"), ("DROVER_SWIFT_SCRATCH", "swiftpm")];

/// The folder under a root's `.build` holding each lane's scratch.
pub const SCRATCH: &str = ".scratch";

const PUSHBACK: &str = "If a finding is wrong or out of scope, say so in your report with file:line proof; \
that is not a fix. The next reviewer decides.";

fn brief(
    f: &Fleet,
    issue: u64,
    p: &Plan,
    title: &str,
    body: &str,
    (fix, rules, docs, repro, play): (Option<&str>, Option<&str>, Option<&str>, bool, Option<String>),
    recipe: &crate::design::Recipe,
) -> String {
    let (branch, integration) = (&p.branch, &f.integration);
    let contract = match p.family {
        "rev" => format!(
            include_str!("../roles/brief-rev.md"),
            integration = integration,
            worktree = p.worktree.display(),
            form = f.form_categories.as_deref().unwrap_or_default().join(", ")
        ),
        "design" => format!(
            include_str!("../roles/brief-design.md"),
            branch = branch,
            integration = integration,
            medium = recipe.medium,
            paths = (recipe.folders.iter())
                .map(|p| format!("`{p}/`"))
                .collect::<Vec<_>>()
                .join(", "),
            render = recipe
                .render
                .as_ref()
                .map_or("render a PNG of each page or view".into(), |c| format!("run `{c}`")),
            sizes = recipe.sizes,
            clip = if recipe.clip {
                " The design moves: also a short clip of it (`clip.gif` or `clip.mp4`) beside the images."
            } else {
                ""
            }
        ),
        "architect" => {
            let hot = include_str!("../roles/brief-hotspots-architect.md");
            format!(
                include_str!("../roles/brief-architect.md"),
                branch = branch,
                integration = integration
            )
            .trim_end()
            .to_string()
                + "\n"
                + if title.starts_with(crate::hotspots::TITLE) {
                    hot
                } else {
                    ""
                }
        }
        _ if p.role == "triage" => format!(
            include_str!("../roles/brief-triage.md"),
            fleet = p.lane.split(':').next().unwrap_or_default(),
            integration = integration,
            issue = issue
        ),
        _ => format!(
            include_str!("../roles/brief-impl.md"),
            branch = branch,
            integration = integration,
            issue = issue
        ),
    };
    let steps = match (repro, p.family, p.stage.as_str()) {
        (false, ..) | (_, "design" | "architect", _) => "",
        (_, "rev", "verify") => include_str!("../roles/brief-repro-verify.md"),
        (_, "rev", _) => include_str!("../roles/brief-repro-rev.md"),
        _ => include_str!("../roles/brief-repro-impl.md"),
    };
    let play = match (play.as_deref(), p.family, p.stage.as_str()) {
        (None, ..) | (_, "design", _) | (_, "rev", "verify") => "",
        (Some("perf"), "rev", _) => include_str!("../roles/brief-perf-rev.md"),
        (Some("perf"), ..) => include_str!("../roles/brief-perf-impl.md"),
        (_, "rev", _) => include_str!("../roles/brief-refactor-rev.md"),
        _ => include_str!("../roles/brief-refactor-impl.md"),
    };
    let swift = match p.family {
        "design" | "architect" => "",
        _ if f.swift_build => include_str!("../roles/brief-swift.md"),
        _ => "",
    };
    let contract = contract.trim_end().to_string() + "\n" + steps + play + swift;
    let head = "## Review findings to fix (latest FAIL verdict)";
    let fix = fix.map_or(String::new(), |v| format!("{head}\n\n{PUSHBACK}\n\n{v}\n\n"));
    let rules = rules.map_or(String::new(), |r| {
        format!("## Repo rules (.drover/rules.md)\n\n{}\n\n", r.trim_end())
    });
    let docs = docs.map_or(String::new(), |d| {
        format!("## Docs you may trust (.drover/docs.md)\n\nAny other doc is not current: don't read it, unless this brief names it.\n\n{}\n\n", d.trim_end())
    });
    let shared = include_str!("../rules/shared.md").trim_end();
    let rules = format!("## Shared rules (rules/shared.md)\n\n{shared}\n\n{rules}");
    format!(
        include_str!("../roles/brief.md"),
        agent = p.agent,
        repo = f.repo,
        issue = issue,
        role = lane_prompt(f, &p.role),
        title = title,
        body = body,
        fix = fix,
        rules = rules,
        docs = docs,
        contract = contract.trim_end()
    )
}

fn labels(p: &Plan, f: &Fleet, round: u32) -> Vec<(&'static str, String)> {
    let issue = p.lane.split(':').nth(1).unwrap_or_default().to_string();
    let fleet = p.lane.split(':').next().unwrap_or_default().to_string();
    let role = if p.family == "impl" && round > 0 {
        "fix"
    } else {
        p.family
    }
    .to_string();
    let url = format!("https://github.com/{}/issues/{issue}", f.repo);
    vec![
        ("fleet", fleet.clone()),
        ("issue", issue.clone()),
        ("role", role.clone()),
        ("harness", p.harness.clone()),
        ("source", "drover".into()),
        ("group", f.repo_name().into()),
        ("item", format!("#{issue}")),
        ("round", round.to_string()),
        ("url", url),
        ("drover.fleet", fleet),
        ("drover.issue", issue),
        ("drover.role", role),
        ("drover.round", round.to_string()),
    ]
}

fn round(events: &[Event], fleet: &str, issue: u64, p: &Plan) -> u32 {
    let k = p.agent.rsplit_once("-r").and_then(|(_, k)| k.parse().ok());
    let n = fold(events).remove(&(fleet.into(), issue)).map_or(0, |s| s.rounds);
    k.filter(|_| p.family == "rev").unwrap_or(n)
}

/// Why drover's hook would not guard ACP agent `id` running `harness` on `profile` for `family`; empty when it would. an attached cooee conductor's `guarded` too.
pub fn unguarded(ctx: &Ctx, (profile, family, h): (&str, &str, &str), id: &str) -> String {
    let settings: Value = std::fs::read_to_string(providers::profiles_root(ctx).join(profile).join("settings.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let pre = settings.at("hooks").at("PreToolUse").as_array().into_iter().flatten();
    let mut hooks = pre.flat_map(|m| m.at("hooks").as_array().into_iter().flatten());
    if providers::get(h).acp() != Some(id) {
        format!("ACP agent {id} runs no {h} hook")
    } else if !hooks.any(|h| h["command"].as_str().is_some_and(|c| providers::guards(ctx, c, family))) {
        format!("profile {profile} has no drover PreToolUse hook; run `drover profiles sync`")
    } else {
        String::new()
    }
}

/// file, signed in with the host's own login, its profile's settings and role as the
pub fn acp_cmd(ctx: &Ctx, r: &dyn Runner, p: &Plan, id: &str, remote: Option<&str>) -> Result<term::Acp> {
    let dir = providers::profiles_root(ctx).join(&p.profile);
    let home = |a: Vec<String>| crate::hosts::rehome(a, remote);
    let why = unguarded(ctx, (&p.profile, p.family, &p.harness), id);
    if !why.is_empty() {
        return refused("ACP_UNGUARDED", format!("{why}; the lane does not start"));
    }
    if !shed_meta(r) {
        return refused("SHED_OLD", format!("{SHED_OLD}; the lane does not start"));
    }
    let cmd = crate::shed::adapter(r, id)?
        .with_context(|| format!("ACP agent {id} is not installed; `shed agents add {id}`"))?;
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
    Ok(term::Acp {
        cmd: [argv(&["sh", "-c", &line, "sh"]), cmd].concat(),
        mcp: (meta.is_some() || !p.mcp.is_empty()).then(|| p.mcp_file.clone()),
        meta,
        awake: p.sources_secrets(),
        account: p.account.clone(),
    })
}

/// What `shed_meta` lacking means, for doctor's row and spawn's refusal.
pub const SHED_OLD: &str =
    "this host's shed predates shed#94 (no `shed run --meta`): an ACP lane would load the owner's own setup";

/// The host's shed takes `run --meta`.
pub fn shed_meta(r: &dyn Runner) -> bool {
    crate::shed::cli(r, &["help", "run"]).is_ok_and(|o| o.stdout.contains("--meta"))
}

/// An ACP lane's `--mcp` file: the role's servers as ACP's list (stdio, `http`, `sse`).
pub fn write_acp_mcp(on: &crate::hosts::On, p: &Plan, acp: &term::Acp) -> Result<()> {
    let Some(file) = &acp.mcp else { return Ok(()) };
    let pairs = |v: &Value| -> Vec<Value> {
        let kv = v.as_object().into_iter().flatten();
        kv.map(|(k, v)| json!({"name": k, "value": v})).collect()
    };
    let list: Vec<Value> = p
        .mcp
        .iter()
        .map(|(name, s)| match s["type"].as_str() {
            Some(t @ ("http" | "sse")) => json!({"type": t, "name": name, "url": s["url"], "headers": pairs(&s["headers"])}),
            _ => json!({"name": name, "command": s["command"], "args": s.get("args").unwrap_or(&json!([])), "env": pairs(&s["env"])}),
        })
        .collect();
    on.write(file, &serde_json::to_string_pretty(&list)?)
}

fn no_webkit(ctx: &Ctx, cfg: &Config, ev: &[Event], (fleet, issue): (&str, u64), e: &anyhow::Error) {
    let Some(x) = e.downcast_ref::<crate::gate::Refusal>() else {
        return;
    };
    let carded = fold(ev)
        .get(&(fleet.to_string(), issue))
        .is_some_and(|s| s.state == "needs-human");
    if x.code == "NO_HOST" && x.msg.contains(crate::hosts::WEBKIT) && !carded {
        let d = json!({"code": x.code, "reason": format!("its repro test needs Playwright WebKit: {}", x.msg)});
        emit(
            &ctx.state,
            cfg,
            Event::new(ctx.now, fleet, Some(issue), "needs-human", d),
        )
        .ok();
    }
}

fn checked_out(r: &dyn Runner, local: &str, branch: &str) -> Option<String> {
    let list = ok(r, "git", &["-C", local, "worktree", "list", "--porcelain"]).ok()?;
    let want = format!("branch refs/heads/{branch}");
    let held = list.split("\n\n").skip(1).find(|b| b.lines().any(|l| l == want))?;
    held.lines().next()?.strip_prefix("worktree ").map(String::from)
}

fn worktree_add(local: &str, p: &Plan, have: bool) -> Vec<String> {
    let wt = p.worktree.to_string_lossy();
    if have {
        argv(&["-C", local, "worktree", "add", &wt, &p.branch])
    } else {
        argv(&[
            "-C",
            local,
            "worktree",
            "add",
            "-b",
            &p.branch,
            "--no-track",
            &wt,
            &p.base,
        ])
    }
}

fn track(local: &str, branch: &str) -> [Vec<String>; 2] {
    let (remote, merge) = (format!("branch.{branch}.remote"), format!("branch.{branch}.merge"));
    let head = format!("refs/heads/{branch}");
    [
        argv(&["-C", local, "config", &remote, "origin"]),
        argv(&["-C", local, "config", &merge, &head]),
    ]
}

fn plan_json(p: &Plan, on: &crate::hosts::On, (local, reopen): (&str, bool), rt: Runtime) -> Value {
    let (bt, labels, acp) = rt;
    let sent = |prog: &str, a: Vec<String>| {
        let (prog, a) = on.wrap(prog, &a);
        [vec![prog], a].concat()
    };
    let mut cmds = vec![sent(
        "git",
        argv(&[
            "-C",
            local,
            "fetch",
            "origin",
            &refspec(p.base.trim_start_matches("origin/")),
        ]),
    )];
    if !reopen {
        cmds.push(sent("git", worktree_add(local, p, false)));
        cmds.extend(track(local, &p.branch).map(|a| sent("git", a)));
    }
    let run = bt.acp_argv(&p.agent, labels, &p.worktree.to_string_lossy(), acp);
    cmds.push(sent("shed", run));
    cmds.push(sent("shed", argv(&["send", &p.agent, "<the brief>"])));
    json!({
        "agent": p.agent, "role": p.role, "harness": p.harness, "profile": p.profile, "machine": on.machine,
        "model": p.model, "effort": p.effort, "branch": p.branch, "base": p.base,
        "worktree": p.worktree, "brief": p.brief, "kick": p.kick, "title": p.title, "port": p.port, "commands": cmds,
    })
}

type Runtime<'a> = (&'a term::Shed, &'a [(&'static str, String)], &'a term::Acp);

/// Lane scratch: untracked files fleet itself writes into a worktree (some gitignored). Never dirty work.
pub const OWN_FILES: &[&str] = &[
    ".brief.md",
    ".addendum.md",
    ".role-prompt.md",
    ".mcp.lane.json",
    "verdict.md",
    ".review-brief.md",
    "body.md",
    ".verify-browser.mjs",
    ".verify.md",
    crate::outbox::FILE,
    crate::outbox::ACKED,
];

const OWN_REPO: [&str; 8] = [
    ".drover/rules.md",
    ".drover/verify.md",
    ".drover/design.md",
    ".drover/arch.toml",
    ".herdsman/rules.md",
    ".herdsman/verify.md",
    ".herdsman/design.md",
    ".herdsman/arch.toml",
];

/// `OWN_FILES` and the harness files spawn copies in (`providers::LOCAL_FILES`).
pub fn own_files() -> impl Iterator<Item = &'static str> {
    OWN_FILES.iter().chain(&providers::LOCAL_FILES).copied()
}

/// Screenshot folders a lane leaves behind; `clean` removes them with `OWN_FILES`.
pub const SHOT_DIRS: &[&str] = &["ui-390/", "ui-1440/", "shots/"];

/// A drover-made clone of `repo` in `dir`: `branch` only, then every branch but `wip/*` on each later fetch.
pub fn clone_sh(repo: &str, branch: &str, dir: &str) -> String {
    let all = "'+refs/heads/*:refs/remotes/origin/*'";
    format!(
        "git clone --single-branch --branch '{branch}' https://github.com/{repo}.git '{dir}' && git -C '{dir}' config \
         remote.origin.fetch {all} && git -C '{dir}' config --add remote.origin.fetch '^refs/heads/wip/*'"
    )
}

/// `git status --porcelain` lines that are not lane scratch or generated (screenshots, packages).
pub fn changes(st: &str) -> usize {
    let own = |l: &&str| {
        (own_files()
            .chain(SHOT_DIRS.iter().copied())
            .chain(["node_modules/", providers::LOCAL_DIR]))
        .any(|f| l.trim_end() == format!("?? {f}"))
    };
    st.lines().filter(|l| !own(l) && scratch_log(l).is_none()).count()
}

/// An untracked `*.log` in a `git status --porcelain` line: lane scratch, never wip.
pub fn scratch_log(l: &str) -> Option<&str> {
    let log = |p: &&str| Path::new(p).extension().is_some_and(|e| e.eq_ignore_ascii_case("log"));
    l.trim_end().strip_prefix("?? ").filter(log)
}

/// Some lockfile exists, and each one reads the same in the local checkout and the new lane.
pub fn lockfiles_match(locks: &[&str], local: &Path, wt: &Path) -> bool {
    let read = |dir: &Path, l: &str| std::fs::read(dir.join(l)).ok();
    locks.iter().any(|l| read(local, l).is_some()) && locks.iter().all(|l| read(local, l) == read(wt, l))
}

/// Clone each `clone_build` folder whose lockfile matches (`Cargo.lock` for `target`, the package ones for `node_modules`); the `build-cloned` data, if any. Never a full copy: macOS `cp -c` copies across volumes, so it clones within one; elsewhere `--reflink=always` fails instead. Never over a folder the lane's agent made: the clone lands beside it and renames in only while the name is free.
pub fn clone_build(r: &dyn Runner, dirs: &[String], local: &Path, wt: &Path) -> Option<Value> {
    use std::os::unix::fs::MetadataExt;
    let dev = |p: &Path| std::fs::metadata(p).map(|m| m.dev()).ok();
    let (t, mut done, mut errs) = (crate::clock::instant(), vec![], vec![]);
    for d in dirs {
        let locks: &[&str] = match d.as_str() {
            "target" => &["Cargo.lock"],
            "node_modules" => &["package-lock.json", "pnpm-lock.yaml"],
            _ => {
                errs.push(format!("{d}: not target or node_modules"));
                continue;
            }
        };
        let (src, dst, tmp) = (local.join(d), wt.join(d), wt.join(format!(".{d}.clone")));
        if !src.is_dir() || dst.exists() || !lockfiles_match(locks, local, wt) {
            continue;
        }
        let (s, t) = (src.to_string_lossy(), tmp.to_string_lossy());
        let res = match cfg!(target_os = "macos") {
            true if dev(local) != dev(wt) => Err(anyhow::anyhow!("another volume, no clone")),
            true => ok(r, "cp", &["-cR", &s, &t]),
            false => ok(r, "cp", &["-R", "--reflink=always", &s, &t]),
        };
        match res.and_then(|_| Ok(std::fs::rename(&tmp, &dst)?)) {
            Ok(()) => done.push(d),
            Err(e) => {
                std::fs::remove_dir_all(&tmp).ok();
                errs.push(format!("{d}: {}", first_lines(&e)));
            }
        }
    }
    let secs = t.elapsed().as_secs();
    (!done.is_empty() || !errs.is_empty()).then(|| json!({"dirs": done, "secs": secs, "errors": errs}))
}

fn first_lines(e: &anyhow::Error) -> String {
    format!("{e:#}").lines().take(5).collect::<Vec<_>>().join("\n")
}

fn reuse(p: &Plan, dirs: &[String], target: Option<&String>, agents: &[Session]) -> Option<Value> {
    let (stem, k) = p.agent.rsplit_once("-r")?;
    let k = k.parse::<u64>().ok().filter(|k| *k > 1)?;
    let prev = format!("{stem}-r{}", k - 1);
    if agents.iter().any(|a| a.name == prev) {
        return None;
    }
    let wt = p.worktree.to_string_lossy();
    let old = wt
        .strip_suffix(&format!("-r{k}"))
        .map(|w| PathBuf::from(format!("{w}-r{}", k - 1)));
    let mut pairs: Vec<(PathBuf, PathBuf)> = (dirs.iter())
        .filter_map(|d| Some((old.as_ref()?.join(d), p.worktree.join(d))))
        .collect();
    pairs.extend(target.map(|t| (Path::new(t).with_file_name(&prev), PathBuf::from(t))));
    let moved: Vec<String> = (pairs.into_iter())
        .filter(|(from, to)| from.is_dir() && !to.exists() && std::fs::rename(from, to).is_ok())
        .map(|(_, to)| to.display().to_string())
        .collect();
    (!moved.is_empty()).then(|| json!({"dirs": moved, "from": prev}))
}

/// A new lane's build folder `t` (under its root's `.build`) cloned from the newest of its fleet's lanes' there: one volume, so copy-on-write, never a full copy. The `build-cloned` data, if any, its `swift` folders listed.
pub fn clone_warm(r: &dyn Runner, fleet: &str, t: &Path, swift: bool) -> Option<Value> {
    let b = t.parent().filter(|b| b.ends_with(crate::roots::BUILD) && !t.exists())?;
    let mine = |e: &std::fs::DirEntry| {
        let n = e.file_name().to_string_lossy().into_owned();
        n.strip_prefix(fleet).is_some_and(|x| x.starts_with('-')) && e.path().is_dir()
    };
    let ls = std::fs::read_dir(b).ok()?.flatten().filter(mine);
    let src = ls.map(|e| e.path()).max_by_key(|p| crate::check::mtime(p))?;
    let tmp = b.join(format!(".{}.clone", t.file_name()?.to_string_lossy()));
    let at = crate::clock::instant();
    let (s, d) = (src.to_string_lossy(), tmp.to_string_lossy());
    let args: &[&str] = if cfg!(target_os = "macos") {
        &["-cR"]
    } else {
        &["-R", "--reflink=always"]
    };
    let res = ok(r, "cp", &[args, &[&s, &d]].concat());
    let res = res.and_then(|_| Ok(std::fs::rename(&tmp, t)?));
    let errs: Vec<String> = res.err().map(|e| first_lines(&e)).into_iter().collect();
    std::fs::remove_dir_all(&tmp).ok();
    let swift = SWIFT
        .iter()
        .filter(|_| swift)
        .map(|(_, n)| t.join(n))
        .filter(|d| d.is_dir());
    let dirs: Vec<String> = (errs.is_empty().then(|| t.to_path_buf()).into_iter().chain(swift))
        .map(|d| d.display().to_string())
        .collect();
    Some(json!({"dirs": dirs, "from": src, "secs": at.elapsed().as_secs(), "errors": errs}))
}

/// The ignored files `.drover/worktree-include` lists (one path per line, `#` comments), copied from the main checkout into a new lane's worktree when missing there. Never secrets: those stay in `secret:` env.
pub fn copy_included(local: &Path, wt: &Path) -> Result<()> {
    let list = std::fs::read_to_string(crate::repo_path(&local.join(".drover/worktree-include"))).unwrap_or_default();
    for l in list
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let (src, dst) = (local.join(l), wt.join(l));
        if l.split('/').any(|c| c == "..") || !src.is_file() || dst.exists() {
            continue;
        }
        std::fs::create_dir_all(dst.parent().unwrap_or(wt))?;
        std::fs::copy(&src, &dst).with_context(|| format!("worktree-include: {l}"))?;
    }
    Ok(())
}

/// `git status --porcelain` in `wt`, drover's own files excluded: a folder holding only them (`.drover/`) never shows.
pub fn status(r: &dyn Runner, wt: &str) -> Result<String> {
    let own: Vec<String> = own_files().map(|f| format!(":(exclude,literal){f}")).collect();
    let args = ["-C", wt, "status", "--porcelain", "--", "."]
        .into_iter()
        .chain(own.iter().map(String::as_str));
    ok(r, "git", &args.collect::<Vec<_>>())
}

/// Worktree `wt`'s unsaved work, readable: its uncommitted files (`OWN_FILES` aside) and unpushed commits.
pub fn unsaved(r: &dyn Runner, wt: &Path) -> Option<(u64, u64)> {
    let wt = wt.to_string_lossy();
    let st = status(r, &wt).ok()?;
    let n = ok(
        r,
        "git",
        &["-C", &wt, "rev-list", "--count", "HEAD", "--not", "--remotes"],
    )
    .ok()?;
    Some((u64::try_from(changes(&st)).unwrap_or(u64::MAX), n.trim().parse().ok()?))
}

/// `+<files> ⇡<commits>` of unsaved work `u`, none when there is none.
pub fn unsaved_text(u: Option<(u64, u64)>) -> Option<String> {
    let (f, c) = u.filter(|u| *u != (0, 0))?;
    Some(format!("+{f} ⇡{c}"))
}

/// Why a worktree must not be reused or removed: uncommitted changes (`OWN_FILES` aside) or unpushed commits.
pub fn dirty(r: &dyn Runner, wt: &Path) -> Option<String> {
    dirty_past(r, wt, None)
}

/// `dirty`, with the commits reachable from `kept` (a merged PR's head, which its PR ref keeps) counted as pushed.
pub fn dirty_past(r: &dyn Runner, wt: &Path, kept: Option<&str>) -> Option<String> {
    let wt = wt.to_string_lossy();
    let st = match status(r, &wt) {
        Ok(s) => s,
        Err(e) => return Some(format!("unreadable git status ({e:#})")),
    };
    let changed = changes(&st);
    if changed > 0 {
        return Some(format!("{changed} uncommitted change(s)"));
    }
    let args = ["-C", &wt, "rev-list", "--count", "HEAD", "--not", "--remotes"];
    let past = kept.map(|k| ["--ignore-missing", k]);
    match ok(
        r,
        "git",
        &[&args[..], past.as_ref().map_or(&[][..], |p| &p[..])].concat(),
    ) {
        Ok(n) if n.trim() == "0" => None,
        Ok(n) => Some(format!("{} unpushed commit(s)", n.trim())),
        Err(e) => Some(format!("unreadable unpushed count ({e:#})")),
    }
}

/// `spawn`'s flags.
#[derive(Default, clap::Args)]
pub struct Opts {
    /// Print the plan; change nothing
    #[arg(long)]
    pub dry_run: bool,
    /// Machine-readable output
    #[arg(long)]
    pub json: bool,
    /// Spawn although the host is busy: the owner or an orchestrator (logs an `override` event with `by`)
    #[arg(long)]
    pub force: bool,
    /// Kill the live agent / unfinished lane in the way, then spawn (logged)
    #[arg(long)]
    pub supersede: bool,
    /// Run the lane on this `[hosts.<name>]` (default: the least-busy enabled host whose tags cover the needs)
    #[arg(long)]
    pub host: Option<String>,
    /// Fix round that merges the integration branch into a conflicting or behind PR
    #[arg(long)]
    pub update: bool,
    #[arg(skip = Some(actor()))]
    pub by: Option<String>,
}

fn integration_view(
    ctx: &Ctx,
    (r, on): (&dyn Runner, &crate::hosts::On),
    f: &crate::config::Fleet,
    p: &Plan,
    (fleet, issue, update): (&str, u64, bool),
    local: &str,
) -> Result<(Option<String>, Option<String>)> {
    let own = || crate::repo_read("rules.md", |x| on.read(&p.worktree.join(x)));
    let int = &f.integration;
    if p.family != "rev" {
        return Ok((
            own(),
            update.then(|| format!(include_str!("../roles/brief-update.md"), int)),
        ));
    }
    crate::clone::fetch(ctx, r, Path::new(local), &["origin", &refspec(int)])?;
    let at_int = |name: &str| {
        crate::repo_read(name, |x| {
            ok(r, "git", &["-C", local, "show", &format!("origin/{int}:{x}")]).ok()
        })
    };
    if let Some(v) = (p.role == "verify").then(|| at_int("verify.md")).flatten() {
        on.write(&p.worktree.join(".verify.md"), &v)?;
    }
    if crate::lesson::is_lesson(ctx, None, (fleet, issue)) {
        return Ok((own(), None));
    }
    let wt = p.worktree.to_string_lossy();
    let diff = ok(
        r,
        "git",
        &[
            &["-C", &wt, "diff", &format!("origin/{int}...HEAD"), "--"][..],
            &OWN_REPO,
        ]
        .concat(),
    );
    let d = diff.unwrap_or_default();
    let names: Vec<String> = (d.lines())
        .filter_map(|l| Some(format!("`{}`", l.strip_prefix("diff --git a/")?.split(" b/").next()?)))
        .collect();
    let changed = (!names.is_empty()).then(|| {
        format!(
            "This PR changes {}: you are briefed with the integration branch's copy; the change itself is under review.\n\n~~~diff\n{d}~~~",
            names.join(", ")
        )
    });
    Ok((at_int("rules.md").or_else(own), changed))
}

fn add_worktree(r: &dyn Runner, local: &str, p: &Plan, h: &crate::config::Hygiene) -> Result<()> {
    let verify = argv(&["-C", local, "rev-parse", "--verify", "--quiet", &p.branch]);
    let have = r.run("git", &verify)?.code == 0;
    ok(r, "git", &worktree_add(local, p, have))?;
    for a in track(local, &p.branch) {
        ok(r, "git", &a)?;
    }
    let deny = h.docs_trust.clone().unwrap_or_default();
    if !deny.is_empty() {
        let pats = deny.iter().map(|d| format!("!/{}", d.trim_start_matches('/')));
        let wt = p.worktree.to_string_lossy();
        let set = ["-C", &wt, "sparse-checkout", "set", "--no-cone", "/*"].map(String::from);
        ok(r, "git", &[set.to_vec(), pats.collect()].concat())?;
    }
    Ok(())
}

pub fn spawn(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    who: (&str, u64, &str),
    o: &Opts,
) -> Result<bool> {
    let mut intent = None;
    let res = start(ctx, cfg, r, out, who, o, &mut intent);
    if let (Err(e), Some(i)) = (&res, intent.take()) {
        i.fail(e)?;
    }
    res
}

fn start(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, issue, role): (&str, u64, &str),
    o: &Opts,
    intent: &mut Option<crate::action::Intent>,
) -> Result<bool> {
    let f = cfg.fleet(fleet)?;
    if f.paused && !crate::events::STAGES.contains(&role) {
        return refused("PAUSED", format!("{fleet} is paused; `drover resume {fleet}` first"));
    }
    if o.update && f.family(role) != "impl" {
        return refused("BAD_VALUE", "--update is an impl fix round");
    }
    let mut p = plan(ctx, cfg, fleet, issue, role)?;
    let ev = crate::reroot::on_root(r, working(&ctx.state)?, (fleet, f))?;
    let host = o.host.clone().or_else(|| crate::starts::admitted(ctx, fleet, issue));
    let placed = crate::hosts::place(ctx, cfg, r, &ev, (fleet, issue, role), (host.as_deref(), &|_| None))
        .inspect_err(|e| no_webkit(ctx, cfg, &ev, (fleet, issue), e))?;
    if let Some(why) = crate::breaker::says(&ev, placed.as_ref().map_or("local", |pl| pl.name.as_str())) {
        return refused("RUNTIME_DOWN", why);
    }
    let on = crate::hosts::On::new(r, placed.as_ref().and_then(|pl| pl.machine.as_deref()));
    let (base, r): (&dyn Runner, &dyn Runner) = (r, &on);
    if let Some(pl) = &placed {
        p.at(&pl.root, f);
        p.env.insert("DROVER_HOST".into(), pl.name.clone());
        if pl.machine.is_some() {
            p.env.insert("DROVER_OUTBOX".into(), "1".into());
            p.env
                .insert("DROVER_RULES".into(), p.rules_file().display().to_string());
        }
    }
    let hm = placed.as_ref().and_then(|pl| pl.home.as_deref());
    let home = |a: Vec<String>| crate::hosts::rehome(a, hm);
    let wt = p.worktree.to_string_lossy().into_owned();
    let local = placed.as_ref().map_or_else(|| f.local_path(), |pl| pl.local.clone());
    let local = local.to_string_lossy().into_owned();
    let reopen = on.exists(&p.worktree);
    let bt = {
        let b = cfg.shed_for(fleet).context(crate::config::NO_RUNTIME)?;
        let wire = on.shed().map_or(b.wire.clone(), |w| w.wire);
        let acp = b
            .acp
            .clone()
            .map(|a| f.role(role).ok().and_then(|r| r.agent.clone()).unwrap_or(a));
        term::Shed(crate::shed::Shed { wire, acp, ..b })
    };
    if on.machine.is_none() {
        crate::shed::ready(&on, &bt.0.wire, o.dry_run)?;
    }
    let acp_id = bt.0.acp.clone().context(crate::config::NO_RUNTIME)?;
    let n = round(&working(&ctx.state)?, fleet, issue, &p);
    let labels = labels(&p, f, n);
    let offline = placed
        .as_ref()
        .and_then(|pl| pl.refusal.clone())
        .filter(|x| x.0 == "HOST_OFFLINE");
    if let Some((code, why)) = offline.clone().filter(|_| !o.dry_run) {
        return refused(code, why);
    }
    if on.machine.is_some() && offline.is_none() && !o.dry_run {
        let exe = home(vec![ctx.exe.clone()]).remove(0);
        ok(
            r,
            &exe,
            &["profiles", "sync", "--profile", &p.profile, "--role", p.family],
        )?;
    }
    p.account = on.machine.is_none().then(|| cfg.account(fleet, role));
    let acp = acp_cmd(ctx, r, &p, &acp_id, hm.filter(|_| on.machine.is_some()))?;
    if let Some(a) = p.account.as_ref().filter(|_| !o.dry_run) {
        crate::accounts::admit(cfg, &on, &p.harness, a)?;
    }
    let mut pj = plan_json(&p, &on, (&local, reopen), (&bt, &labels, &acp));
    if o.dry_run {
        pj.put("dry_run", json!(true));
        print_plan(out, &pj, o.json, placed.as_ref())?;
        return Ok(true);
    }
    let h = providers::get(&p.harness);
    h.ready(ctx, r, &p)?;
    let event = |kind: &str, d: Value| emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(issue), kind, d));

    let events = working(&ctx.state)?;
    let agents = crate::session::live_on(r).context("cannot list the lanes for the duplicate guard")?;
    let live: Vec<&Session> = agents.iter().filter(|a| a.in_dir(&p.agent, &p.worktree)).collect();
    let names: Vec<&str> = live.iter().map(|a| a.name.as_str()).collect();
    let mut dup = vec![];
    let stale = live.iter().all(|a| !a.busy()) && orphan(&agents, &events, &p).is_some();
    if stale {
        let last = events
            .iter()
            .rfind(|e| e.fleet == fleet && e.issue == Some(issue) && !e.is_passive())
            .map_or(0, |e| e.ts);
        let idle = crate::events::age(ctx.now.saturating_sub(last));
        dup.push((
            "LANE_LIVE",
            format!(
                "stale pane {} (idle {idle}): drover adopt or --supersede",
                names.join(", ")
            ),
        ));
    }
    let unfinished = fold(&events)
        .remove(&(fleet.to_string(), issue))
        .filter(|s| s.unfinished(role, f.family(role) == "rev"));
    let over = |a: &&Session| {
        let sp = (events.iter()).rposition(|e| e.is("spawn", fleet, issue) && e.str("agent") == Some(a.name.as_str()));
        let after = |i: usize| {
            (events.iter().skip(i + 1)).any(|e| e.is("verdict", fleet, issue) || e.is("spawn", fleet, issue))
        };
        let judged = |i: usize| {
            let at = events.get(i).and_then(|e| e.str("stage"));
            (events.iter().skip(i + 1)).any(|e| e.is("verdict", fleet, issue) && e.str("stage") == at)
        };
        let ended = matches!(a.status(), Some(Status::Done | Status::Idle));
        sp.is_some_and(|i| ended && after(i) || f.family(role) == "rev" && judged(i))
    };
    if !stale && !live.is_empty() {
        let st = live.first().and_then(|l| l.status()).map_or("unknown", Status::word);
        let finished = if unfinished.is_none() && live.iter().all(over) {
            format!("finished ({st}) lane(s) ")
        } else {
            "live agent(s) ".into()
        };
        dup.push(("LANE_LIVE", format!("{finished}{names:?} in {wt}")));
    }
    if let Some(s) = unfinished {
        dup.push((
            "LANE_LIVE",
            format!(
                "unfinished {role} lane {:?} for #{} ({})",
                s.lanes.get(role),
                s.issue,
                s.state
            ),
        ));
    }
    let prior = events
        .iter()
        .rfind(|e| e.kind == "spawn" && e.str("worktree") == Some(&wt));
    if on.exists(&p.worktree) && prior.is_none() {
        if let Some(why) = dirty(r, &p.worktree) {
            dup.push(("WORKTREE_DIRTY", format!("{wt} is no fleet lane and has {why}")));
        }
    }
    if let Some(pane) = orphan(&agents, &events, &p).filter(|_| live.len() == 1 && !o.supersede) {
        return adopt(ctx, cfg, r, out, (fleet, issue, role), &pane, &json!({})).map(|()| true);
    }
    if let Some((code, msg)) = dup.first().filter(|_| !o.supersede) {
        return refused(
            code,
            format!("{msg}{}", if stale { "" } else { "; `--supersede` replaces it" }),
        );
    }

    lane_admit(ctx, (cfg, r), &events, (fleet, issue, role), o.force)?;
    crate::respawn::guard(ctx, cfg, &events, (fleet, issue, role))?;
    if o.update {
        update_guard(ctx, cfg, &events, (fleet, issue))?;
    }

    let refusal = match &placed {
        Some(pl) => pl.refusal.clone(),
        None => crate::hosts::gate(ctx, cfg, &on).1,
    };
    let budget = cfg.limits.worktree_budget_gb;
    if let Some(gb) = crate::check::worktree_gb(ctx).filter(|g| on.machine.is_none() && *g > budget) {
        let why = crate::check::worktree_why(ctx);
        return refused("DISK_BUDGET", format!("worktrees {gb:.1} GB > {budget} GB; {why}"));
    }
    match refusal {
        Some(("HOST_BUSY", why)) if o.force => {
            let d = json!({"code": "HOST_BUSY", "reason": why, "role": role, "agent": p.agent, "by": actor()});
            event("override", d)?;
        }
        Some(("HOST_BUSY", why)) => return refused("HOST_BUSY", format!("{why}; retry later or pass --force")),
        Some((code, why)) => return refused(code, why),
        None => {}
    }
    let step = if o.supersede && !dup.is_empty() {
        "supersede"
    } else {
        "spawn"
    };
    let round = events
        .iter()
        .rfind(|e| e.is("spawn", fleet, issue))
        .map_or(0, |e| e.seq);
    let probe = |i: &Event| -> Result<Option<Value>> {
        let logged = working(&ctx.state)?
            .iter()
            .any(|e| e.seq > i.seq && e.is("spawn", fleet, issue));
        let up = (step == "spawn").then(|| crate::session::live_on(r).ok()).flatten();
        let up = up.is_some_and(|a| a.iter().any(|a| a.in_dir(&p.agent, &p.worktree)));
        Ok((logged || up).then(|| json!({"agent": p.agent})))
    };
    let (key, secs) = (format!("{role}@{round}"), SPAWN_SECS + 300);
    let inputs = json!({"role": role, "agent": p.agent, "worktree": p.worktree});
    *intent =
        Some(crate::action::begin(r, &ctx.state, ctx.now, (fleet, issue, step, &key), inputs, secs, &probe)?.act()?);
    let (mut wip, saved) = (None, (cfg, fleet));
    if o.supersede && !dup.is_empty() {
        let was = last_spawn(&events, &p.agent);
        let old = crate::hosts::On::of(base, was);
        let owt = was.and_then(|e| e.str("worktree")).unwrap_or(&wt);
        let listed = crate::session::live_on(&old);
        let stale = was.filter(|_| listed.is_err() && old.machine.is_some());
        let hung =
            was.is_some_and(|e| matches!(crate::roots::skip(ctx, cfg, e), Some("root unreachable" | "root-hung")));
        let cause = crate::session::stamp::exit(&events, &p.agent).filter(|x| x.cause.resumable());
        let here = cause.is_some() && owt == wt && old.machine == on.machine;
        if stale.is_none() && !here && !hung {
            wip = crate::check::save_lane(ctx, saved, &old, &p.agent, owt)?;
        }
        let mut killed = names.clone();
        for a in listed
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|a| a.in_dir(&p.agent, Path::new(owt)))
        {
            term::close(ctx, &old, &a.name)?;
            if !killed.contains(&a.name.as_str()) {
                killed.push(&a.name);
            }
        }
        if stale.is_none() && !hung {
            crate::check::reap(ctx, cfg, &old, was, (fleet, Some(issue)), owt)?;
        }
        let why: Vec<&str> = dup.iter().map(|d| d.1.as_str()).collect();
        let mut d = json!({"role": role, "agent": p.agent, "killed": killed, "reason": why.join("; "), "wip": wip});
        if let Some(e) = stale {
            d.put("stale_host", json!(e.str("host")))
                .put("stale_pane", json!(e.str("pane")));
        }
        event("supersede", d)?;
    }
    crate::check::stop_rounds(ctx, cfg, base, (fleet, issue, role), &p.agent)?;
    let moved = (placed.as_ref())
        .and_then(|pl| last_spawn(&events, &p.agent).filter(|e| e.str("machine") != pl.machine.as_deref()));
    if let Some((e, owt)) = moved
        .and_then(|e| Some((e, e.str("worktree")?)))
        .filter(|_| wip.is_none())
    {
        wip = crate::check::save_lane(ctx, saved, &crate::hosts::On::of(base, Some(e)), &p.agent, owt)?;
    }

    let mut sha = None;
    let (note, history);
    let brief_text = {
        let iss = crate::gh::spec(ctx, base, f, fleet, issue, true)?;
        if on.machine.is_some() && !on.exists(&Path::new(&local).join(".git")) {
            ok(r, "sh", &["-c", &clone_sh(&f.repo, &f.integration, &local)])?;
        }
        let from = p.base.trim_start_matches("origin/");
        crate::clone::fetch(ctx, r, Path::new(&local), &["origin", &refspec(from)])?;
        let body = iss.at("body").as_str().unwrap_or("");
        let recipe = match p.family {
            "design" => crate::design::Recipe::at(r, &local, &p.base, body),
            _ => crate::design::Recipe::of("", body),
        };
        crate::design::tools(ctx, cfg, r, (fleet, issue, &p.agent), &recipe)?;
        let spec = refspec(&p.branch);
        let pushed = || crate::clone::fetch(ctx, r, Path::new(&local), &["origin", &spec]).is_ok();
        if moved.is_some() && p.family != "rev" && pushed() {
            p.base = format!("origin/{}", p.branch);
        }
        let make = || -> Result<()> {
            if !reopen {
                add_worktree(r, &local, &p, &cfg.hygiene(fleet))?;
            }
            Ok(())
        };
        match make() {
            Err(e) if !reopen => {
                let Some(held) = checked_out(r, &local, &p.branch) else {
                    return Err(e);
                };
                let live = crate::session::live_on(r)?;
                let names: Vec<&str> = (live.iter().filter(|a| a.in_dir("\0", Path::new(&held))))
                    .map(|a| a.name.as_str())
                    .collect();
                if !names.is_empty() {
                    let why = format!("live agent(s) {names:?} in {held}, where {} is checked out", p.branch);
                    return refused("LANE_LIVE", why);
                }
                let git = |a: &[&str]| ok(r, "git", &[&["-C", &local, "worktree"][..], a].concat());
                if on.exists(Path::new(&held)) {
                    wip = wip.or(crate::check::save_lane(ctx, saved, &on, &p.agent, &held)?);
                    if git(&["remove", &held]).is_err() {
                        git(&["remove", "--force", &held])?;
                    }
                } else {
                    git(&["prune"])?;
                }
                let d = json!({"role": role, "agent": p.agent, "old": held, "worktree": wt, "wip": wip});
                event("worktree-moved", d)?;
                make()?;
            }
            c => c?,
        }
        if p.family == "rev" && reopen {
            ok(r, "git", &["-C", &wt, "reset", "--hard", &p.base])?;
            on.remove(&p.worktree.join("verdict.md")).ok();
        }
        history = crate::baseline::lane(ctx, cfg, r, (fleet, issue, f), (&p.agent, &p.worktree), wip.as_deref())?;
        if p.family == "rev" || crate::events::STAGES.contains(&p.family) {
            sha = ok(r, "git", &["-C", &wt, "rev-parse", "HEAD"])
                .ok()
                .map(|h| h.trim().to_string());
        }
        let title = iss.at("title").as_str().unwrap_or("");
        p.title = lane_title(&p.head, title);
        note = prior
            .filter(|_| o.supersede && !dup.is_empty())
            .map(|prev| handover(ctx, r, &events, prev, &p, (fleet, issue, title)));
        let cause = prior
            .filter(|_| o.supersede)
            .and_then(|sp| crate::session::stamp::exit(&events, sp.str("agent")?));
        let resume = cause.filter(|x| x.cause.resumable()).map(|x| {
            let c = x.cause.word();
            format!(
                "Resume: your previous session in this worktree stopped mid-turn ({c}). Its local commits and staged \
                 changes are kept here, and drover pushed the commits to `{}`: continue from them and push.",
                p.branch
            )
        });
        let last = events.iter().rev().find(|e| e.is("verdict", fleet, issue));
        let fix = last
            .filter(|e| p.family == "impl" && e.str("verdict") == Some("FAIL"))
            .and_then(|e| e.str("body"));
        let adr = events
            .iter()
            .rfind(|e| e.is("design-approved", fleet, issue) && e.str("role") == Some("architect"))
            .and_then(|e| e.str("adr"))
            .filter(|_| p.family != "architect")
            .map(|a| match p.family {
                "rev" => format!("Architecture: the owner approved the ADR in `{a}/`; check the diff against it."),
                _ => format!("Architecture: the owner approved the ADR in `{a}/` on this branch; build to it, and add any `.drover/arch.toml` lines it ends with in this PR."),
            });
        let (rules, int_note) = integration_view(ctx, (r, &on), f, &p, (fleet, issue, o.update), &local)?;
        let decided = events
            .iter()
            .filter(|e| e.is("decision", fleet, issue))
            .map(plain)
            .chain(int_note)
            .chain(adr)
            .chain((p.family == "rev").then(|| crate::guard::brief(&events, (fleet, issue))).flatten())
            .chain(note.clone())
            .chain(history.brief())
            .chain(resume)
            .chain(wip.as_ref().map(|w| {
                format!("Saved work: the last lane's uncommitted and unpushed work is on `origin/{w}`; fetch and merge it first.")
            }))
            .map(|t| format!("\n\n{t}"));
        brief(
            f,
            issue,
            &p,
            title,
            &decided.fold(body.to_string(), |b, d| b + &d),
            (
                fix,
                rules.as_deref(),
                crate::repo_read("docs.md", |f| on.read(&p.worktree.join(f))).as_deref(),
                crate::queue::repro(&events, fleet, issue),
                crate::queue::playbook(&events, fleet, issue),
            ),
            &recipe,
        )
    };
    on.write(&p.brief, &brief_text).context("writing the brief")?;
    if p.role == "verify" {
        let vb = include_str!("../../../scripts/verify-browser.mjs");
        on.write(&p.worktree.join(".verify-browser.mjs"), vb)?;
    }
    if on.machine.is_none() {
        copy_included(&f.local_path(), &p.worktree)?;
    }
    h.prepare(ctx, &on, &p)?;
    write_acp_mcp(&on, &p, &acp)?;
    let _secrets = deliver(ctx, &on, &p)?;
    if p.sandbox.is_some() {
        let rules = serde_json::to_string_pretty(&crate::sandbox::settings(ctx, f, &p, on.machine.is_none()))?;
        on.write(&crate::sandbox::file(&p), &home(vec![rules]).remove(0))?;
        on.write(&crate::sandbox::tmp(&p).join(".keep"), "")?;
    }
    let rules = placed.as_ref().filter(|pl| pl.machine.is_some());
    let cap_raise = crate::gh::cached_title(ctx, fleet, issue).is_some_and(|t| t.starts_with("cap:"));
    let hygiene = crate::hygiene::of(ctx, Some(r), cfg, fleet);
    let rules = rules.map(|pl| crate::hook::Rules {
        cap_raise,
        hygiene,
        ..crate::hook::Rules::resolve(cfg, fleet, role, &pl.name)
    });
    if let Some(rl) = &rules {
        on.write(&p.rules_file(), &serde_json::to_string(rl)?)?;
    }
    let (pane, host) = (p.agent.clone(), placed.as_ref().map(|pl| (&pl.name, &pl.machine)));
    let d = json!({"agent": p.agent, "role": role, "host": host.map(|h| h.0), "machine": host.and_then(|h| h.1.as_ref()),
        "worktree": p.worktree, "pane": pane});
    event("spawn-started", d)?;
    pj = plan_json(&p, &on, (&local, reopen), (&bt, &labels, &acp));
    pj.put("history", json!(history.line(&p.branch)));
    crate::slo::start_healed(ctx, (fleet, issue), || bt.start(ctx, r, &pane, &labels, &wt, &acp))?;
    crate::starts::started(ctx, fleet, issue);
    let mut dirs: Vec<String> = f.clone_build.clone();
    dirs.extend(f.clone_node_modules.then(|| "node_modules".into()));
    dirs.sort();
    dirs.dedup();
    dirs.retain(|_| !reopen && on.machine.is_none());
    let target = p.env.get("CARGO_TARGET_DIR").filter(|_| on.machine.is_none());
    if let Some(d) = (p.family == "rev").then(|| reuse(&p, &dirs, target, &agents)).flatten() {
        event("build-reused", d)?;
    }
    let (res, cloned) = std::thread::scope(|s| {
        let c = s.spawn(|| {
            let warm = target.and_then(|t| clone_warm(r, fleet, Path::new(t), f.swift_build));
            let warm = warm.or_else(|| crate::roots::clone_warm_on(&on, fleet, p.env.get("CARGO_TARGET_DIR")?));
            [clone_build(r, &dirs, &f.local_path(), &p.worktree), warm]
        });
        let res = (|| -> Result<bool> {
            let kicked = bt.kick_acp(r, &pane, &brief_text);
            let mut d = lane_data(&p, &pane);
            d.put("runtime", json!("shed-acp"));
            let confirmed = kicked.as_ref().is_ok_and(|c| *c);
            d.put("confirmed", json!(confirmed))
                .put("error", json!(kicked.as_ref().err().map(|e| format!("{e:#}"))))
                .put("by", json!(o.by));
            d.put("update", json!(o.update)).put("handover", json!(note));
            if let Some(pl) = &placed {
                d.put("host", json!(pl.name))
                    .put("machine", json!(pl.machine))
                    .put("root", json!(pl.root));
                if let Some(w) = &pl.why {
                    d.put("why", json!(w));
                }
            }
            if let Some(h) = sha.filter(|h| !h.is_empty()) {
                d.put("sha", json!(h));
            }
            if let Some(rl) = &rules {
                d.put("rules", json!(rl));
            }
            if let Some((agent, old)) = prior.and_then(|e| Some((e.str("agent")?, e.str("pane")?))) {
                if o.supersede && confirmed && old != pane && !agents.iter().any(|a| a.name == old) {
                    term::close(ctx, r, old).ok();
                    let c = json!({"agent": agent, "reason": "superseded", "pane": old});
                    event("lane-closed", c)?;
                }
            }
            if let Some(i) = intent.take() {
                i.done(json!({"agent": p.agent, "pane": pane}))?;
            }
            event("spawn", d)?;
            crate::link::look_at(ctx, cfg, r, placed.as_ref().and_then(|pl| pl.machine.as_deref())).ok();
            kicked?;
            pj.put("pane", json!(pane)).put("confirmed", json!(confirmed));
            print_plan(out, &pj, o.json, placed.as_ref())?;
            Ok(confirmed)
        })();
        (res, c.join().ok())
    });
    for d in cloned.into_iter().flatten().flatten() {
        let d = crate::events::offload(&ctx.state, serde_json::from_value(d)?)?;
        event("build-cloned", Value::Object(d))?;
    }
    res
}

pub const SPAWN_SECS: u64 = 3 * term::CONFIRM_SECS;

fn orphan(agents: &[Session], events: &[Event], p: &Plan) -> Option<String> {
    let a = agents
        .iter()
        .find(|a| a.name == p.agent && a.cwd.starts_with(&p.worktree))?;
    let logged = events
        .iter()
        .any(|e| e.kind == "spawn" && e.str("pane") == Some(a.name.as_str()));
    (!logged).then(|| a.name.clone())
}

pub(crate) fn lane_admit(
    ctx: &Ctx,
    (cfg, r): (&Config, &dyn Runner),
    events: &[Event],
    (fleet, issue, role): (&str, u64, &str),
    force: bool,
) -> Result<()> {
    let f = cfg.fleet(fleet)?;
    let lanes = fold(events);
    let pending = crate::drive::pending(ctx, cfg, r);
    let st = crate::events::fold_into(lanes.clone(), &pending);
    let full = || crate::queue::flight_full(ctx, events, &lanes, f, (fleet, issue));
    let events = &[events, &pending].concat();
    let others: Vec<_> = st.values().filter(|s| s.fleet == fleet && s.issue != issue).collect();
    let building = |s: &str| s == "impl" || s.starts_with("fix");
    let gs = crate::queue::groups(&crate::queue::replay(events, fleet), events, f, fleet, issue);
    let pushed = crate::gh::cached_pr(ctx, fleet, issue).at("state") == "OPEN";
    for g in gs.into_iter().filter(|_| role == "impl" && !pushed) {
        let h = crate::queue::holders(events, &st, f, fleet, &g.name);
        if !h.iter().any(|s| s.issue == issue) && h.len() >= g.max {
            let why = format!("{}; retry after they merge", crate::queue::holds(&g, &h));
            return refused("SERIAL_BUSY", why);
        }
    }
    let fam = f.family(role);
    let (cap, n) = match fam {
        "impl" => (f.max_impl, others.iter().filter(|s| building(&s.state)).count()),
        "rev" => (
            f.max_rev,
            others.iter().filter(|s| s.state.starts_with("review")).count(),
        ),
        _ => (None, 0),
    };
    if let Some(cap) = cap.filter(|&c| n >= c) {
        return refused("CAP", format!("{n} {role} lane(s) open in {fleet}, max_{fam} = {cap}"));
    }
    let admitted = crate::drive::own_marker(ctx, fleet, issue).is_some();
    if fam != "impl" || admitted {
        return Ok(());
    }
    let named = |v: String, starting: usize| {
        let starting = (starting > 0).then(|| format!("{starting} starting"));
        let v = (!v.is_empty()).then_some(v);
        v.into_iter().chain(starting).collect::<Vec<_>>().join(", ")
    };
    let why = if let Some((v, starting, cap)) = full() {
        let count = v.len() + starting;
        format!(
            "{count}/{cap} PRs in flight in {fleet} ({})",
            named(crate::queue::hashes(&v), starting)
        )
    } else if let Some(t) = crate::queue::total_full(ctx, (events, &lanes), cfg, (fleet, issue, &pending)) {
        let (count, cap) = (t.prs.len() + t.starts, t.cap);
        let ahead = (t.ahead > 0).then(|| format!("; {} start(s) in other fleets go first", t.ahead));
        let all = named(crate::queue::by_fleet(&t.prs), t.starts);
        format!(
            "{count}/{cap} PRs in flight across fleets ({all}){}",
            ahead.unwrap_or_default()
        )
    } else {
        return Ok(());
    };
    if !force {
        return refused("IN_FLIGHT_CAP", format!("{why}; they land first, or pass --force"));
    }
    let by = crate::orchestrator::acting().map_or_else(|| "owner".into(), |id| format!("orchestrator-{id}"));
    let d = json!({"code": "IN_FLIGHT_CAP", "reason": why, "role": role, "by": by});
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(issue), "override", d))
}

/// Stuck (#1809): the PR conflicts again after 2 update rounds in a row, each with its PASS, and no update extend since the last.
fn update_guard(ctx: &Ctx, cfg: &Config, events: &[Event], (fleet, issue): (&str, u64)) -> Result<()> {
    let (mut run, mut updating, mut extended) = (0, false, false);
    for e in crate::events::of_issue(events, fleet, issue) {
        match e.kind.as_str() {
            "spawn" if !crate::events::checks(e) => updating = e.data.get("update") == Some(&json!(true)),
            "verdict" => {
                run = if updating && e.str("verdict") == Some("PASS") {
                    run + 1
                } else {
                    0
                };
                (updating, extended) = (false, false);
            }
            "round-extended" => extended |= e.data.get("update") == Some(&json!(true)),
            _ => {}
        }
    }
    if run < 2 || extended {
        return Ok(());
    }
    let why = "PR still conflicts after 2 update rounds in a row".to_string();
    let turn = ["spawn", "needs-human", "round-extended"];
    let held = (events.iter().rev())
        .filter(|e| e.fleet == fleet && e.issue == Some(issue))
        .find(|e| turn.contains(&e.kind.as_str()));
    if held.is_none_or(|e| e.kind == "spawn") {
        let d = json!({"code": "STUCK", "update": true, "reason": format!("stuck: {why}")});
        emit(
            &ctx.state,
            cfg,
            Event::new(ctx.now, fleet, Some(issue), "needs-human", d),
        )?;
    }
    refused("STUCK", why)
}

fn handover(
    ctx: &Ctx,
    r: &dyn Runner,
    events: &[Event],
    prev: &Event,
    p: &Plan,
    (fleet, issue, title): (&str, u64, &str),
) -> String {
    let mine = |e: &&Event| e.fleet == fleet && e.issue == Some(issue);
    let spawns: Vec<&Event> = events
        .iter()
        .filter(mine)
        .filter(|e| e.kind == "spawn" && e.str("role") == Some(&p.role))
        .collect();
    let ran = spawns.iter().rposition(|e| e.str("harness") == Some(&p.harness));
    let since = ran.and_then(|i| spawns.get(i + 1)).map(|e| e.ts);
    let wt = p.worktree.to_string_lossy();
    let (was, prev) = (prev, prev.str("agent").unwrap_or_default());
    let said = crate::session::transcript::page(&crate::hosts::On::of(r, Some(was)), ctx, prev, None, usize::MAX);
    let [prompts, replies, cmds] = said.as_ref().map(crate::session::transcript::said).unwrap_or_default();
    let git = |a: &[&str]| ok(r, "git", &[&["-C", &*wt][..], a].concat()).unwrap_or_default();
    let files = match since {
        Some(s) => {
            git(&["log", &format!("--since=@{s}"), "--name-only", "--format="]) + &git(&["diff", "--name-only", "HEAD"])
        }
        None => git(&["diff", "--name-only", &p.base]),
    };
    let mut files: Vec<String> = files
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect();
    files.sort();
    files.dedup();
    let later = |e: &&Event| since.is_none_or(|s| e.ts > s);
    let asks = events
        .iter()
        .filter(mine)
        .filter(later)
        .filter(|e| matches!(e.kind.as_str(), "needs-decision" | "decision"));
    let asks: Vec<String> = asks.map(plain).collect();
    let notes = spawns
        .iter()
        .copied()
        .filter(later)
        .filter_map(|e| e.str("handover"))
        .map(String::from);
    let mut out =
        "Handover: Summary of the previous lane; its tool state and approvals are not carried over.".to_string();
    if let Some(s) = since {
        out += &format!(
            "\nOnly what happened since {}'s last lane on this issue was replaced ({} UTC).",
            p.harness,
            iso(s).replace('T', " ")
        );
    }
    out += &format!("\nIssue: {}", handover_item(title));
    let sections = [
        ("Prompts", prompts),
        ("Final replies", replies),
        ("Commands run", cmds),
        ("Files changed", files),
        ("Asks and answers", asks),
        ("Prior handover notes", notes.collect()),
    ];
    for (name, items) in sections.into_iter().filter(|s| !s.1.is_empty()) {
        out += &format!("\n{name}:");
        for i in items.iter().skip(items.len().saturating_sub(20)) {
            out += &format!("\n- {}", handover_item(i));
        }
    }
    out + "\nContinue; don't redo."
}

fn handover_item(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match s.char_indices().nth(237) {
        Some((i, _)) if s.chars().count() > 240 => format!("{}...", &s[..i]),
        _ => s,
    }
}

fn iso(t: u64) -> String {
    let (d, s) = (t / 86_400, t % 86_400);
    let (m, day) = access::civil_month_day(d);
    format!(
        "{}-{m:02}-{day:02}T{:02}:{:02}:{:02}",
        access::civil_year(d),
        s / 3600,
        s / 60 % 60,
        s % 60
    )
}

fn lane_data(p: &Plan, pane: &str) -> Value {
    json!({"role": p.role, "stage": p.stage, "check": p.family == "rev", "harness": p.harness, "profile": p.profile, "model": p.model, "effort": p.effort, "agent": p.agent,
        "pane": pane, "worktree": p.worktree, "branch": p.branch, "port": p.port, "qos": p.qos(),
        "account": p.account.as_ref().map(crate::config::Account::word)})
}

pub fn adopt(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, issue, role): (&str, u64, &str),
    pane: &str,
    extra: &Value,
) -> Result<()> {
    let p = plan(ctx, cfg, fleet, issue, role)?;
    crate::session::get(r, pane)?
        .filter(|s| s.running() && s.labels.contains_key("harness"))
        .with_context(|| format!("no harness running in pane {pane}"))?;
    let mut d = lane_data(&p, pane);
    d.put("adopted", json!(true));
    extra.as_object().into_iter().flatten().for_each(|(k, v)| {
        d.put(k, v.clone());
    });
    writeln!(
        out,
        "adopted {pane} as {} ({} @ {})",
        p.agent,
        p.branch,
        d.at("worktree")
    )?;
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(issue), "spawn", d))
}

pub fn orphans(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write) -> Result<bool> {
    let sessions = crate::link::view::View::load(ctx, cfg)?.sessions().checked()?;
    let events = working(&ctx.state)?;
    let (st, mut any) = (fold(&events), false);
    for a in crate::lanes::orphans(ctx, cfg, &events, &sessions) {
        let (pane, m, name) = (a.name.as_str(), a.machine.as_deref(), Some(a.name.as_str()));
        let at = |e: &&Event| e.str("agent") == name;
        let Some((fleet, Some(n), role)) = crate::lanes::whose(cfg, a) else {
            continue;
        };
        let (role, wt) = (role.map(String::from), Some(a.cwd.to_string_lossy().into_owned()));
        let Some(s) = st.get(&(fleet.to_string(), n)) else {
            continue;
        };
        let f = cfg.fleet(fleet)?;
        let started = (crate::events::lanes(&events).into_iter())
            .filter(|e| e.kind == "spawn-started")
            .rfind(at);
        let role = role.or(s.build.clone()).unwrap_or(f.flow().start().into());
        let wt = wt.or(started.and_then(|e| e.str("worktree")).map(String::from));
        let on = crate::hosts::On::new(r, m);
        let rev = wt.as_deref().filter(|_| f.family(&role) == "rev");
        let verdict = rev.and_then(|w| on.read(&Path::new(w).join("verdict.md")));
        let git = |w: &str| ok(&on, "git", &["-C", w, "rev-parse", "HEAD"]).ok();
        let named = verdict.map(|v| {
            crate::waiting::reviewed_head(&v)
                .map(String::from)
                .or_else(|| git(rev?))
        });
        let branch = crate::gate::branch_of(Some(s), n);
        let head = |x: String| {
            Some((
                x,
                crate::gh::pr(ctx, r, f, (fleet, n), &branch, ctx.now)
                    .ok()?
                    .at("headRefOid")
                    .as_str()?
                    .into(),
            ))
        };
        let old = named
            .flatten()
            .and_then(head)
            .filter(|(x, h): &(String, String)| !x.eq_ignore_ascii_case(h));
        let due = s
            .stage_rounds
            .get(f.flow().stage_of(&role))
            .copied()
            .unwrap_or_default()
            + 1;
        let lane = name.filter(|n| !n.is_empty()).or(wt.as_deref());
        let round = lane.and_then(|l| l.rsplit_once("-r")?.1.parse::<u32>().ok());
        let round = round.filter(|k| f.family(&role) == "rev" && *k != due);
        let stale = match old {
            _ if s.done() => Some(format!("{fleet}#{n} is {}", s.state)),
            _ if let Some(k) = round => Some(format!("round r{k}, r{due} is due")),
            Some((x, h)) => Some(format!("verdict for {x:.7}, head is {h:.7}")),
            None => None,
        };
        let agent = name.filter(|n| !n.is_empty()).unwrap_or(pane);
        let res = if let Some(why) = stale {
            (|| {
                if let Some(w) = wt.as_deref().filter(|w| dirty(&on, Path::new(w)).is_some()) {
                    let b = crate::check::wip_branch(ctx, &on, &format!("{fleet}-{role}-{n}"));
                    crate::check::save_wip(&on, w, &b, "wip: stale orphan lane, saved by drover")?;
                }
                term::close(ctx, &on, pane)?;
                writeln!(out, "closed stale orphan {agent} ({why})")?;
                let d = json!({"agent": agent, "pane": pane, "host": a.host, "why": "stale-orphan", "reason": why});
                emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), "lane-closed", d))
            })()
        } else {
            let since = started.map_or(0, |e| e.ts);
            let mut d = json!({"why": "orphan", "since": since, "worktree": wt, "host": a.host, "machine": m});
            d.as_object_mut()
                .into_iter()
                .for_each(|o| o.retain(|_, v| !v.is_null()));
            adopt(ctx, cfg, &on, out, (fleet, n, &role), pane, &d).and_then(|()| {
                let d = json!({"agent": agent, "pane": pane, "why": "orphan"});
                emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(n), "spawn-adopted", d))
            })
        };
        match res {
            Ok(()) => any = true,
            Err(e) => writeln!(out, "{fleet} #{n} orphan {agent}: {e:#}")?,
        }
    }
    if any {
        crate::link::look(ctx, cfg, r, out, true)?;
    }
    Ok(any)
}

fn print_plan(out: &mut dyn Write, pj: &Value, as_json: bool, placed: Option<&crate::hosts::Placed>) -> Result<()> {
    let mut v = pj.clone();
    if let Some(pl) = placed {
        v.put("host", json!(pl.name)).put("why", json!(pl.why));
    }
    let pj = &v;
    if as_json {
        writeln!(out, "{}", serde_json::to_string_pretty(pj)?)?;
    } else {
        writeln!(
            out,
            "{} [{} {}] {} @ {}",
            pj["agent"], pj["harness"], pj["profile"], pj["branch"], pj["worktree"]
        )?;
        if let Some(h) = pj["host"].as_str() {
            let why = pj["why"].as_str().map_or(String::new(), |w| format!(": {w}"));
            writeln!(out, "  on {h}{why}")?;
        }
        for c in pj["commands"].as_array().into_iter().flatten() {
            let words: Vec<&str> = c.as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            writeln!(out, "  $ {}", words.join(" "))?;
        }
        if let Some(h) = pj["history"].as_str() {
            writeln!(out, "{h}")?;
        }
        if pj.get("confirmed") == Some(&json!(false)) {
            writeln!(out, "WARNING: prompt not confirmed landed; check the pane")?;
        }
    }
    Ok(())
}
