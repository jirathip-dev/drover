//! `herdsman spawn`, `adopt` and `profiles sync` (docs/lanes.md); per-harness parts live in `harness`.
use crate::access;
use crate::config::{expand, Config, Fleet, Role};
use crate::events::{emit, fold, last_spawn, plain, read, Event};
use crate::gate::refused;
use crate::harness::{self, hook_command};
use crate::herdr;
use crate::host;
use crate::runner::{argv, ok, Runner};
use crate::Ctx;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn role_prompt(role: &str) -> &'static str {
    match role {
        "impl" => include_str!("../roles/impl.md"),
        "design" => include_str!("../roles/design.md"),
        _ => include_str!("../roles/rev.md"),
    }
}

/// #173: a role's brief prompt: its kind's, then its `prompt = "<file>"` appended (unreadable: noted, not fatal).
fn lane_prompt(f: &Fleet, role: &str) -> String {
    let extra =
        f.role.get(role).and_then(|r| r.prompt.as_deref()).map(|p| {
            std::fs::read_to_string(expand(p)).unwrap_or_else(|e| format!("(prompt file {p} unreadable: {e})"))
        });
    let base = role_prompt(f.family(role));
    extra.map_or_else(|| base.into(), |x| format!("{}\n\n{}", base.trim_end(), x.trim_end()))
}

/// Each profile once, at its first (fleet, role) in fleet.toml order.
fn profiles(cfg: &Config) -> Vec<(&str, &Fleet, &str, &Role, String)> {
    let mut done = std::collections::BTreeSet::new();
    let all = cfg.fleet.iter().flat_map(|(name, f)| {
        f.role
            .iter()
            .map(move |(role, r)| (name.as_str(), f, role.as_str(), r, r.profile_or(role)))
    });
    all.filter(|p| done.insert(p.4.clone())).collect()
}

/// Write every harness's files for every role's profile, so `herdsman set` alone can switch a role's harness;
/// a generated profile no role names any more (#155: a retired role's) is removed.
pub fn profiles_sync(ctx: &Ctx, cfg: &Config, usage: &str, out: &mut dyn Write) -> Result<()> {
    let all = profiles(cfg);
    for (_, f, role, r, profile) in &all {
        // #173: a role's profile is its kind's: a check role's is read-only, as rev's.
        for h in harness::NAMES {
            harness::get(h).sync(ctx, f.family(role), profile, usage, out)?;
        }
        harness::set_mcp(ctx, profile, &access::mcp_servers(cfg, f, r)?)?;
    }
    let root = harness::profiles_root(ctx);
    for d in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let (name, dir) = (d.file_name().to_string_lossy().into_owned(), d.path());
        if dir.join("role.md").exists() && !all.iter().any(|p| p.4 == name) {
            std::fs::remove_dir_all(&dir)?;
            writeln!(out, "removed stale {}", dir.display())?;
        }
    }
    Ok(())
}

/// `profiles sync --check`: each Hermes profile's `hooks list` (run as in a lane) shows its hook `✓ allowed`.
pub fn profiles_check(ctx: &Ctx, cfg: &Config, r: &dyn Runner, out: &mut dyn Write) -> Result<bool> {
    let mut good = true;
    for (name, f, role, _, profile) in profiles(cfg) {
        if !ctx.hermes.join(&profile).join("config.yaml").exists() {
            continue;
        }
        let lane = format!("HERDSMAN_LANE={name}:-:{role}");
        let o = r.run("env", &argv(&[&lane, "hermes", "-p", &profile, "hooks", "list"]))?;
        let cmd = hook_command(ctx, f.family(role));
        let allowed = o.code == 0 && o.stdout.lines().any(|l| l.contains(&cmd) && l.contains("✓ allowed"));
        writeln!(
            out,
            "{} {profile} ({role})",
            if allowed { "✓ allowed" } else { "✗ not allowed" }
        )?;
        good &= allowed;
    }
    Ok(good)
}

pub struct Plan {
    pub agent: String,
    pub role: String,
    /// #173: the pipeline stage it runs and the role's kind (`impl`, `rev` for a check, `design`).
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
    /// `HERDSMAN_LANE` value (`<fleet>:<issue>:<role>`); arms `herdsman hook` in the lane.
    pub lane: String,
    /// `[accounts]` lanes: the account name and the shell assignment exporting its token.
    pub account: Option<(String, String)>,
    pub env: std::collections::BTreeMap<String, String>,
    pub port: u16,
    /// The only MCP servers the lane gets, and the lane-local file a harness reads them from.
    pub mcp: serde_json::Map<String, Value>,
    pub mcp_file: PathBuf,
    /// Display title head (`<fleet> · <role> #<issue>`) and the full lane title built from it.
    pub head: String,
    pub title: String,
    /// #302: the card's `on <entry> (<entries> resting until` and that time, when a `fallback` entry carries on.
    pub resting: Option<(String, u64)>,
}

impl Plan {
    /// Exports `HERDSMAN_LANE`, `HERDSMAN_PORT`, the fleet's `env` and an account's token in the pane's shell.
    pub fn lane_cmd(&self, pane: &str) -> Vec<String> {
        let (lane, port) = (format!("HERDSMAN_LANE='{}'", self.lane), self.port.to_string());
        let mut c = herdr::run_argv(pane, &["export", &lane, &format!("HERDSMAN_PORT={port}")]);
        let v = |v: &str| expand(v).display().to_string().replace("$HERDSMAN_PORT", &port);
        c.extend(self.env.iter().map(|(k, x)| format!("{k}='{}'", v(x))));
        c.extend(
            self.account
                .iter()
                .flat_map(|a| [format!("HERDSMAN_ACCOUNT='{}'", a.0), a.1.clone()]),
        );
        c.extend(harness::get(&self.harness).env().iter().map(|v| String::from(*v)));
        c
    }

    pub fn start_cmd(&self, ctx: &Ctx, pane: &str) -> Vec<String> {
        harness::get(&self.harness).start_cmd(ctx, self, pane)
    }

    pub fn kick_cmds(&self, pane: &str) -> Vec<Vec<String>> {
        harness::get(&self.harness).kick_cmds(self, pane)
    }

    /// #157: the lane's files under `root` on its host.
    fn at(&mut self, root: &Path, repo: &str) {
        self.worktree = root.join(repo).join(&self.branch);
        self.brief = self.worktree.join(".brief.md");
        self.mcp_file = self.worktree.join(".mcp.lane.json");
        self.kick = format!("Read {} and execute it fully.", self.brief.display());
    }
}

/// Longest lane title, in chars.
const TITLE_MAX: usize = 60;

/// `<head> · <short issue title>`, ≤ 60 chars (not bytes): `feat:`/`P0:` prefixes dropped, cut at a word with `…`.
pub fn lane_title(head: &str, issue_title: &str) -> String {
    let mut t = issue_title.trim();
    // `feat(x)!:`, `spawn:`, `P0:` — one ASCII token, an optional scope, then a colon.
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

/// #187: `<fleet>/<branch>` hashed (FNV-1a) into `range` (default 41000-41999); a `taken` port takes the next free one.
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
    let (r, resting) = access::fallback(cfg, &read(&ctx.state)?, f.role(role)?, ctx.now)?;
    let r = &r;
    let (agent, branch, base, worktree, brief, head) = {
        let st = fold(&read(&ctx.state)?).remove(&(fleet.to_string(), issue));
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
        } else if role == "design" {
            let head = format!("{fleet} · design #{issue}");
            (
                format!("{fleet}-design-{issue}"),
                format!("design-{issue}"),
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
    let account = access::pick(ctx, cfg, &read(&ctx.state)?, r)?;
    let mcp_file = worktree.join(".mcp.lane.json");
    // Ports of the logged lanes of open issues, this branch's own aside.
    let events = read(&ctx.state)?;
    let st = fold(&events);
    let open = st.values().filter(|s| !s.done()).flat_map(|s| s.lanes.values());
    let taken: Vec<u16> = open
        .filter_map(|a| last_spawn(&events, a))
        .filter(|e| e.fleet != fleet || e.str("branch") != Some(&branch))
        .filter_map(|e| e.data.get("port")?.as_u64()?.try_into().ok())
        .collect();
    let port = lane_port(f.port_range.as_deref(), &format!("{fleet}/{branch}"), &taken);
    let title = lane_title(&head, &crate::gh::cached_title(ctx, fleet, issue).unwrap_or_default());
    Ok(Plan {
        agent,
        role: role.into(),
        stage: f.flow().stage_of(role).into(),
        family: f.family(role),
        harness: r.harness.clone(),
        profile: r.profile_or(role),
        model: r.model.clone(),
        effort: r.effort_or(&cfg.limits),
        branch,
        base,
        worktree,
        brief,
        kick,
        lane,
        account,
        env: f.env.clone(),
        port,
        mcp: access::mcp_servers(cfg, f, r)?,
        mcp_file,
        head,
        title,
        resting,
    })
}

/// #92: a fix round may dispute a finding; the next, fresh reviewer decides.
const PUSHBACK: &str = "If a finding is wrong or out of scope, say so in your report with file:line proof; \
that is not a fix. The next reviewer decides.";

/// The issue lane's `.brief.md` (`roles/brief*.md` templates).
fn brief(f: &Fleet, issue: u64, p: &Plan, title: &str, body: &str, fix: Option<&str>) -> String {
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
            integration = integration
        ),
        _ => format!(
            include_str!("../roles/brief-impl.md"),
            branch = branch,
            integration = integration,
            issue = issue
        ),
    };
    let head = "## Review findings to fix (latest FAIL verdict)";
    let fix = fix.map_or(String::new(), |v| format!("{head}\n\n{PUSHBACK}\n\n{v}\n\n"));
    format!(
        include_str!("../roles/brief.md"),
        agent = p.agent,
        repo = f.repo,
        issue = issue,
        role = lane_prompt(f, &p.role),
        title = title,
        body = body,
        fix = fix,
        contract = contract.trim_end()
    )
}

/// The herdr argv that opens (`reopen`) or creates the lane's worktree.
fn create_argv(local: &str, p: &Plan, reopen: bool) -> Vec<String> {
    let wt = p.worktree.to_string_lossy();
    if reopen {
        herdr::worktree_open_argv(local, &wt, &p.agent)
    } else {
        herdr::worktree_create_argv(local, &p.branch, &p.base, &wt, &p.agent)
    }
}

/// #236: the commands as sent to the lane's host (`on`, `home`): spawn runs these same argvs.
fn plan_json(ctx: &Ctx, p: &Plan, on: &crate::hosts::On, (local, reopen): (&str, bool), home: Option<&str>) -> Value {
    let sent = |prog: &str, a: Vec<String>| {
        let (prog, a) = on.wrap(prog, &a);
        [vec![prog], a].concat()
    };
    let home = |a| crate::hosts::rehome(a, home);
    let mut cmds = vec![sent(
        "git",
        argv(&["-C", local, "fetch", "origin", p.base.trim_start_matches("origin/")]),
    )];
    cmds.push(sent(herdr::BIN, create_argv(local, p, reopen)));
    cmds.push(sent(herdr::BIN, home(p.lane_cmd("<pane>"))));
    cmds.push(sent(herdr::BIN, home(p.start_cmd(ctx, "<pane>"))));
    cmds.extend(p.kick_cmds("<pane>").into_iter().map(|k| sent(herdr::BIN, k)));
    json!({
        "agent": p.agent, "role": p.role, "harness": p.harness, "profile": p.profile, "machine": on.machine,
        "model": p.model, "effort": p.effort, "branch": p.branch, "base": p.base, "account": p.account.as_ref().map(|a| &a.0),
        "worktree": p.worktree, "brief": p.brief, "kick": p.kick, "title": p.title, "port": p.port, "commands": cmds,
    })
}

/// Lane scratch: untracked files fleet itself writes into a worktree (some gitignored). Never dirty work.
pub const OWN_FILES: &[&str] = &[
    ".brief.md",
    ".addendum.md",
    ".role-prompt.md",
    ".mcp.lane.json",
    "verdict.md",
    ".review-brief.md",
];

/// `git status --porcelain` lines that are not lane scratch or generated (#248: screenshots, packages).
pub fn changes(st: &str) -> usize {
    let made = ["ui-390/", "node_modules/"];
    let own = |l: &&str| OWN_FILES.iter().chain(&made).any(|f| l.trim_end() == format!("?? {f}"));
    st.lines().filter(|l| !own(l)).count()
}

/// #103: some lockfile exists, and each one reads the same in the local checkout and the new lane.
pub fn lockfiles_match(locks: &[&str], local: &Path, wt: &Path) -> bool {
    let read = |dir: &Path, l: &str| std::fs::read(dir.join(l)).ok();
    locks.iter().any(|l| read(local, l).is_some()) && locks.iter().all(|l| read(local, l) == read(wt, l))
}

/// #185: clone each `clone_build` folder whose lockfile matches (`Cargo.lock` for `target`, the package ones for
/// `node_modules`); the `build-cloned` data, if any. Never a full copy: macOS `cp -c` copies across volumes, so it
/// clones within one; elsewhere `--reflink=always` fails instead. Never over a folder the lane's agent made: the
/// clone lands beside it and renames in only while the name is free.
pub fn clone_build(r: &dyn Runner, dirs: &[String], local: &Path, wt: &Path) -> Option<Value> {
    use std::os::unix::fs::MetadataExt;
    let dev = |p: &Path| std::fs::metadata(p).map(|m| m.dev()).ok();
    let (t, mut done, mut errs) = (std::time::Instant::now(), vec![], vec![]);
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
                let _ = std::fs::remove_dir_all(&tmp);
                errs.push(format!("{d}: {e:#}"));
            }
        }
    }
    let secs = t.elapsed().as_secs();
    (!done.is_empty() || !errs.is_empty()).then(|| json!({"dirs": done, "secs": secs, "errors": errs}))
}

/// Why a worktree must not be reused or removed: uncommitted changes (`OWN_FILES` aside) or unpushed commits.
pub fn dirty(r: &dyn Runner, wt: &Path) -> Option<String> {
    let wt = wt.to_string_lossy();
    let st = match ok(r, "git", &["-C", &wt, "status", "--porcelain"]) {
        Ok(s) => s,
        Err(e) => return Some(format!("unreadable git status ({e:#})")),
    };
    let changed = changes(&st);
    if changed > 0 {
        return Some(format!("{changed} uncommitted change(s)"));
    }
    match ok(
        r,
        "git",
        &["-C", &wt, "rev-list", "--count", "HEAD", "--not", "--remotes"],
    ) {
        Ok(n) if n.trim() == "0" => None,
        Ok(n) => Some(format!("{} unpushed commit(s)", n.trim())),
        Err(e) => Some(format!("unreadable unpushed count ({e:#})")),
    }
}

/// `spawn`'s flags.
#[allow(clippy::struct_excessive_bools)] // independent CLI switches
#[derive(Default, clap::Args)]
pub struct Opts {
    /// Print the plan; change nothing
    #[arg(long)]
    pub dry_run: bool,
    /// Machine-readable output
    #[arg(long)]
    pub json: bool,
    /// Spawn although the host is busy (logs an `override` event)
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
}

pub fn spawn(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, issue, role): (&str, u64, &str),
    o: &Opts,
) -> Result<bool> {
    let f = cfg.fleet(fleet)?;
    if f.paused && role != "design" {
        return refused("PAUSED", format!("{fleet} is paused; `herdsman resume {fleet}` first"));
    }
    if o.update && f.family(role) != "impl" {
        return refused("BAD_VALUE", "--update is an impl fix round");
    }
    let mut p = plan(ctx, cfg, fleet, issue, role)?;
    // #157: from here every call and file of the lane is on its host; #236: the dry run shows it placed.
    let placed = crate::hosts::place(ctx, cfg, r, &read(&ctx.state)?, (fleet, issue, role), o.host.as_deref())?;
    let placed = crate::hosts::wake(ctx, cfg, r, (f, o.dry_run), placed)?;
    let on = crate::hosts::On::new(r, placed.as_ref().and_then(|pl| pl.machine.as_deref()));
    let (base, r): (&dyn Runner, &dyn Runner) = (r, &on);
    if let Some(pl) = &placed {
        p.at(&pl.root, f.repo_name());
        p.env.insert("HERDSMAN_HOST".into(), pl.name.clone()); // #256: the lane hook's per-host rules
        if pl.machine.is_some() {
            p.account = None; // Q3: never ship a token to another host; the lane uses that host's own login

            // #275: its asks, answers, verdicts and events go to the main host (by its `[hosts]` name over ssh).
            if let Some(main) = cfg.hosts.iter().find(|h| h.1.machine.is_none()) {
                p.env.insert("HERDSMAN_MAIN".into(), format!("{}:{}", main.0, ctx.exe));
            }
        }
    }
    let hm = placed.as_ref().and_then(|pl| pl.home.as_deref());
    let home = |a: Vec<String>| crate::hosts::rehome(a, hm);
    let wt = p.worktree.to_string_lossy().into_owned();
    let local = placed.as_ref().map_or_else(|| f.local_path(), |pl| pl.local.clone());
    let local = local.to_string_lossy().into_owned();
    let reopen = on.exists(&p.worktree);
    let mut pj = plan_json(ctx, &p, &on, (&local, reopen), hm);
    if o.dry_run {
        pj["dry_run"] = json!(true);
        print_plan(out, &pj, o.json)?;
        return Ok(true);
    }
    let h = harness::get(&p.harness);
    h.ready(ctx, r, &p)?;
    let event = |kind: &str, d: Value| emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(issue), kind, d));

    // Duplicate guard.
    let events = read(&ctx.state)?;
    let agents = host::agents(r).context("cannot list herdr agents for the duplicate guard")?;
    let live = host::live_in(&agents, &p.agent, &p.worktree);
    // An agent whose start timed out has no name; name it by its pane.
    let names: Vec<&str> = live
        .iter()
        .filter_map(|a| a["name"].as_str().filter(|n| !n.is_empty()).or(a["pane_id"].as_str()))
        .collect();
    let mut dup = vec![];
    // #182: an idle pane no spawn logged names its last activity (the issue's last event) and the way out.
    let stale = live.iter().all(|a| a["agent_status"] != "working") && orphan(&agents, &events, &p, true).is_some();
    if stale {
        let last = events
            .iter()
            .rfind(|e| e.fleet == fleet && e.issue == Some(issue) && !crate::events::PASSIVE.contains(&e.kind.as_str()))
            .map_or(0, |e| e.ts);
        let idle = crate::events::age(ctx.now.saturating_sub(last));
        dup.push((
            "LANE_LIVE",
            format!(
                "stale pane {} (idle {idle}): herdsman adopt or --supersede",
                names.join(", ")
            ),
        ));
    } else if !live.is_empty() {
        dup.push(("LANE_LIVE", format!("live agent(s) {names:?} in {wt}")));
    }
    if let Some(s) = fold(&events)
        .remove(&(fleet.to_string(), issue))
        .filter(|s| s.unfinished(role, f.family(role) == "rev"))
    {
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
    // #302: the lane before went unavailable (capped, signed out): its successor carries on, and says from what.
    let unavailable =
        |e: &Event, sp: &Event| e.issue == Some(issue) && e.ts >= sp.ts && e.str("agent") == sp.str("agent");
    let rest = |e: &&Event| matches!(e.kind.as_str(), "account-capped" | "agent-unavailable");
    let chain = !f.role(role)?.fallback.is_empty();
    let gone = prior.filter(|sp| chain && events.iter().filter(rest).any(|e| unavailable(e, sp)));
    let entry = |h: Option<&str>, a: Option<&str>| [h, a].into_iter().flatten().collect::<Vec<_>>().join(":");
    if on.exists(&p.worktree) && prior.is_none() {
        if let Some(why) = dirty(r, &p.worktree) {
            dup.push(("WORKTREE_DIRTY", format!("{wt} is no fleet lane and has {why}")));
        }
    }
    if let Some(pane) = orphan(&agents, &events, &p, false).filter(|_| live.len() == 1 && !o.supersede) {
        return adopt(ctx, cfg, r, out, (fleet, issue, role), &pane).map(|()| true);
    }
    if let Some((code, msg)) = dup.first().filter(|_| !o.supersede) {
        return refused(
            code,
            format!("{msg}{}", if stale { "" } else { "; `--supersede` replaces it" }),
        );
    }

    lane_admit(ctx, f, &events, fleet, issue, role)?;
    let updates = fold(&events).remove(&(fleet.into(), issue)).map_or(0, |s| s.updates);
    if o.update && updates >= f.flow().update_limit() {
        let why = format!(
            "PR still conflicts after {updates}/{} update rounds",
            f.flow().update_limit()
        );
        event("needs-human", json!({"code": "UPDATE_LIMIT", "reason": why}))?;
        return refused("UPDATE_LIMIT", why);
    }

    // Host-load gate, before anything is created (#157: placement already looked at each host).
    let refusal = match &placed {
        Some(pl) => pl.refusal.clone(),
        None => crate::hosts::gate(ctx, cfg, &on).1,
    };
    // #164: the last drive pass's walk of this host's worktrees (drive frees the idlest lanes' build folders first).
    let budget = cfg.limits.worktree_budget_gb;
    if let Some(gb) = crate::check::worktree_gb(ctx).filter(|g| on.machine.is_none() && *g > budget) {
        return refused("DISK_BUDGET", format!("worktrees {gb:.1} GB > {budget} GB"));
    }
    match refusal {
        Some(("HOST_BUSY", why)) if o.force => event(
            "override",
            json!({"code": "HOST_BUSY", "reason": why, "role": role, "agent": p.agent}),
        )?,
        Some(("HOST_BUSY", why)) => return refused("HOST_BUSY", format!("{why}; retry later or pass --force")),
        Some((code, why)) => return refused(code, why),
        None => {}
    }
    if o.supersede && !dup.is_empty() {
        let was = last_spawn(&events, &p.agent); // #327: the superseded lane's kill steps run on its own host
        let old = crate::hosts::On::of(base, was);
        let owt = was.and_then(|e| e.str("worktree")).unwrap_or(&wt);
        for a in host::live_in(&host::agents(&old).unwrap_or_default(), &p.agent, Path::new(owt)) {
            if let Some(pane) = a["pane_id"].as_str() {
                herdr::close(&old, pane)?;
            }
        }
        crate::check::reap(ctx, cfg, &old, (fleet, Some(issue)), owt)?; // #186: before the new lane starts
        let why: Vec<&str> = dup.iter().map(|d| d.1.as_str()).collect();
        event(
            "supersede",
            json!({"role": role, "agent": p.agent, "killed": names, "reason": why.join("; ")}),
        )?;
    }

    let mut sha = None;
    let note; // #353: a superseded or fallback lane's summary for its successor, logged on its spawn
    let (created, brief_text) = {
        let iss = crate::gh::spec(ctx, base, f, fleet, issue, true)?;
        // #157: first use on a host: it clones the repo itself; #307: plain git, so its insteadOf/SSH and helpers apply.
        if on.machine.is_some() && !on.exists(&Path::new(&local).join(".git")) {
            let url = format!("https://github.com/{}.git", f.repo);
            ok(r, "git", &["clone", &url, &local])?;
        }
        ok(
            r,
            "git",
            &["-C", &local, "fetch", "origin", p.base.trim_start_matches("origin/")],
        )?;
        let created = herdr::json(r, &create_argv(&local, &p, reopen))?;
        if p.family == "rev" {
            // #92: a re-review of the same round moves its worktree to the fetched head; the spawn logs that head.
            if reopen {
                ok(r, "git", &["-C", &wt, "reset", "--hard", &p.base])?;
                let _ = on.remove(&p.worktree.join("verdict.md"));
            }
            sha = ok(r, "git", &["-C", &wt, "rev-parse", "HEAD"])
                .ok()
                .map(|h| h.trim().to_string());
        }
        let (title, body) = (iss["title"].as_str().unwrap_or(""), iss["body"].as_str().unwrap_or(""));
        p.title = lane_title(&p.head, title);
        note = prior
            .filter(|_| gone.is_some() || o.supersede && !dup.is_empty())
            .map(|prev| handover(ctx, r, &events, prev, &p, (fleet, issue, title)));
        // A fix round's brief carries the findings of the latest verdict if it was FAIL.
        let last = events.iter().rev().find(|e| e.is("verdict", fleet, issue));
        let fix = last
            .filter(|e| p.family == "impl" && e.str("verdict") == Some("FAIL"))
            .and_then(|e| e.str("body"));
        // #102: an update round's instructions follow the spec (and any decisions).
        let update = o
            .update
            .then(|| format!(include_str!("../roles/brief-update.md"), f.integration));
        let decided = events
            .iter()
            .filter(|e| e.is("decision", fleet, issue))
            .map(plain)
            .chain(update)
            .chain(note.clone())
            .map(|t| format!("\n\n{t}"));
        (
            created,
            brief(f, issue, &p, title, &decided.fold(body.to_string(), |b, d| b + &d), fix),
        )
    };
    let wsid = |v: Value| v["result"]["workspace"]["workspace_id"].as_str().map(String::from);
    let mut ws = wsid(created).context("herdr: no workspace_id")?;
    herdr::workspace_rename(r, &ws, &p.agent)?; // #194: an open workspace keeps its branch label otherwise
    on.write(&p.brief, &brief_text).context("writing the brief")?;
    h.prepare(ctx, &on, &p)?;
    // #327: a workspace gone under us (its last pane closed) is already gone: open the worktree again and go on.
    let pick = |ws: &mut String, skip: Option<&str>| match herdr::free_pane(r, ws, &p.worktree, skip) {
        Err(e) if format!("{e:#}").contains("workspace_not_found") => {
            *ws = wsid(herdr::json(r, &create_argv(&local, &p, true))?).context("herdr: no workspace_id")?;
            herdr::free_pane(r, ws, &p.worktree, None)
        }
        x => x,
    };
    let mut pane = pick(&mut ws, None)?;
    pj = plan_json(ctx, &p, &on, (&local, reopen), hm);
    herdr::ok(r, &home(p.lane_cmd(&pane)))?;
    let mut start = herdr::exec(r, &home(p.start_cmd(ctx, &pane)))?;
    // #168: `agent_pane_busy` is transient: close the pane if a finished lane left it, then retry once on a fresh one.
    if herdr::failed_with(&start, "agent_pane_busy") {
        // #327: pane ids are per host: only a lane spawned on this one left this pane.
        let lane = events
            .iter()
            .any(|e| e.kind == "spawn" && e.str("pane") == Some(&pane) && e.str("machine") == on.machine);
        let closed = lane && !agents.iter().any(|a| a["pane_id"] == pane.as_str()) && herdr::close(r, &pane).is_ok();
        event("pane-busy", json!({"agent": p.agent, "pane": pane, "closed": closed}))?;
        pane = pick(&mut ws, Some(&pane))?;
        herdr::ok(r, &home(p.lane_cmd(&pane)))?;
        start = herdr::exec(r, &home(p.start_cmd(ctx, &pane)))?;
    }
    // #275: on any host (under load, or over `--machine`) the name can be lost while the harness came up: retry only
    // when none did; one that did is named below.
    let lost = |s: &crate::runner::Out| herdr::failed_with(s, "agent_name_not_found");
    if lost(&start) && herdr::harness_in(r, &pane).is_none() {
        start = herdr::exec(r, &home(p.start_cmd(ctx, &pane)))?;
    }
    if start.code != 0 {
        let taken = herdr::failed_with(&start, "agent_name_taken");
        if let Some(pane) = orphan(&if taken { host::agents(r)? } else { vec![] }, &events, &p, true) {
            return adopt(ctx, cfg, r, out, (fleet, issue, role), &pane).map(|()| true);
        }
        // Under load herdr can time out while the harness still comes up: keep it as the lane.
        // #182: any other failure closes the pane, so no unlogged pane is left to refuse the next pass LANE_LIVE.
        let why = if !herdr::failed_with(&start, "timeout") && !lost(&start) {
            Some(format!("herdr agent start failed: {}", start.stderr.trim()))
        } else if herdr::harness_in(r, &pane).is_none() {
            Some(format!(
                "herdr agent start failed and no harness came up: {}",
                start.stderr.trim()
            ))
        } else {
            herdr::ok(r, &herdr::rename_argv(&pane, &p.agent))
                .err()
                .map(|e| format!("{e:#}"))
        };
        if let Some(why) = why {
            // #302: a harness[:account] that failed to start twice in a row rests (`[limits] fallback_rest_minutes`).
            let mine = |k: &str, e: &Event| e.is(k, fleet, issue) && e.str("agent") == Some(&p.agent);
            let acct = p.account.as_ref().map(|a| a.0.as_str());
            let same = |e: &Event| e.str("harness") == Some(&p.harness) && e.str("account") == acct;
            let twice = events
                .iter()
                .rev()
                .take_while(|e| !mine("spawn", e))
                .any(|e| mine("spawn-failed", e) && same(e));
            let until = ctx.now + cfg.limits.fallback_rest_minutes * 60;
            let kind = ["spawn-failed", "agent-unavailable"][usize::from(twice)];
            let d = json!({"agent": p.agent, "harness": p.harness, "account": acct, "until": until, "reason": "won't start"});
            if chain {
                event(kind, d)?;
            }
            // #236: on another host the half-made workspace goes too (nobody there would close it).
            if on.machine.is_some() {
                let _ = herdr::workspace_close(r, &ws);
                bail!("{why}; closed workspace {ws}");
            }
            let _ = herdr::close(r, &pane);
            bail!("{why}; closed pane {pane}");
        }
    }
    // #185: a new lane's clone runs beside the kick and pane clean-up, once the lane has started; a failure is logged.
    let mut dirs: Vec<String> = f.clone_build.clone();
    dirs.extend(f.clone_node_modules.then(|| "node_modules".into()));
    dirs.sort();
    dirs.dedup();
    let dirs = if reopen || on.machine.is_some() {
        &[][..]
    } else {
        &dirs[..]
    };
    let (res, cloned) = std::thread::scope(|s| {
        let c = s.spawn(|| clone_build(r, dirs, &f.local_path(), &p.worktree));
        let res = (|| -> Result<bool> {
            let kicked = h.kick(r, &p, &pane);
            let mut d = lane_data(&p, &pane, &ws);
            d["confirmed"] = json!(kicked.as_ref().is_ok_and(|c| *c));
            d["error"] = json!(kicked.as_ref().err().map(|e| format!("{e:#}")));
            d["update"] = json!(o.update);
            d["handover"] = json!(note);
            if let Some(pl) = &placed {
                d["host"] = json!(pl.name);
                d["machine"] = json!(pl.machine);
                if let Some(w) = &pl.why {
                    d["why"] = json!(w);
                }
            }
            if let Some(h) = sha.filter(|h| !h.is_empty()) {
                d["sha"] = json!(h);
            }
            let until = p.resting.as_ref().map(|r| crate::usage::date(base, r.1, "+%H:%M"));
            d["resting"] = json!(p.resting.as_ref().zip(until).map(|(r, t)| format!("{} {t})", r.0)));
            let to = entry(d["harness"].as_str(), d["account"].as_str());
            event("spawn", d)?; // #107: logged even when the kick failed, so the running lane is tracked
            if let Some(sp) = gone {
                let from = entry(sp.str("harness"), sp.str("account"));
                event("fallback", json!({"agent": p.agent, "from": from, "to": to}))?;
            }
            let confirmed = kicked?;
            // #128: once confirmed, the superseded lane's pane goes (its folder is reused); live agents were handled above.
            if let Some((agent, old)) = prior.and_then(|e| Some((e.str("agent")?, e.str("pane")?))) {
                if o.supersede && confirmed && old != pane && !agents.iter().any(|a| a["pane_id"] == old) {
                    let _ = herdr::close(r, old);
                    event("lane-closed", json!({"agent": agent, "reason": "superseded"}))?;
                }
            }
            pj["pane"] = json!(pane);
            pj["confirmed"] = json!(confirmed);
            print_plan(out, &pj, o.json)?;
            Ok(confirmed)
        })();
        (res, c.join().ok().flatten())
    });
    if let Some(d) = cloned {
        event("build-cloned", d)?;
    }
    res
}

/// #107: the pane of the lane's agent (its planned name, or `unnamed`) in its worktree that no `spawn` logged: adopt it.
fn orphan(agents: &[Value], events: &[Event], p: &Plan, unnamed: bool) -> Option<String> {
    let mine = |a: &&Value| a["name"] == p.agent.as_str() || unnamed && a["name"].as_str().is_none_or(str::is_empty);
    let pane = host::live_in(agents, "\0", &p.worktree).into_iter().find(mine)?["pane_id"].as_str()?; // by cwd only
    let logged = events.iter().any(|e| e.kind == "spawn" && e.str("pane") == Some(pane));
    (!logged).then(|| pane.into())
}

/// `SERIAL_BUSY` (a group of the issue's is full, unless it holds a slot there) and CAP (`max_impl` / `max_rev`).
pub(crate) fn lane_admit(ctx: &Ctx, f: &Fleet, events: &[Event], fleet: &str, issue: u64, role: &str) -> Result<()> {
    let events = &[events, &crate::drive::pending(ctx)].concat(); // #318: spawns still starting count as lanes
    let st = fold(events);
    let others: Vec<_> = st.values().filter(|s| s.fleet == fleet && s.issue != issue).collect();
    let building = |s: &str| s == "impl" || s.starts_with("fix");
    // #294: each of its groups (name order) needs a free slot, unless it holds one there already (#232).
    let gs = crate::queue::groups(&crate::queue::replay(events, fleet), events, f, fleet, issue);
    for g in gs.into_iter().filter(|_| role == "impl") {
        let h = crate::queue::holders(events, &st, f, fleet, &g.name);
        if !h.iter().any(|s| s.issue == issue) && h.len() >= g.max {
            let why = format!("{}; retry after they merge", crate::queue::holds(g, &h));
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
    Ok(())
}

/// #353: a summary of lane `prev` for its successor `p`, one whitespace-collapsed line of at most 240 chars per item:
/// from its harness's transcript, else its pane log. If `p`'s harness[:account] ran this role before, only what came
/// after that lane was replaced.
fn handover(
    ctx: &Ctx,
    r: &dyn Runner,
    events: &[Event],
    prev: &Event,
    p: &Plan,
    (fleet, issue, title): (&str, u64, &str),
) -> String {
    let acct = p.account.as_ref().map(|a| a.0.as_str());
    let mine = |e: &&Event| e.fleet == fleet && e.issue == Some(issue);
    let spawns: Vec<&Event> = events
        .iter()
        .filter(mine)
        .filter(|e| e.kind == "spawn" && e.str("role") == Some(&p.role))
        .collect();
    let ran = spawns
        .iter()
        .rposition(|e| e.str("harness") == Some(&p.harness) && e.str("account") == acct);
    let since = ran.and_then(|i| spawns.get(i + 1)).map(|e| e.ts);
    let at = since.map(iso);
    let after = |t: &Value| at.as_ref().is_none_or(|s| t.as_str().is_none_or(|t| t > s.as_str()));
    let (wt, h) = (
        p.worktree.to_string_lossy(),
        harness::get(prev.str("harness").unwrap_or_default()),
    );
    let (mut prompts, mut replies, mut cmds) = (vec![], vec![], vec![]);
    let jsonl = h
        .transcript(&ctx.home, &wt)
        .and_then(|f| crate::lanes::read_tail(&f, 1 << 20));
    let log = crate::lanes::log(&ctx.state, prev.str("agent").unwrap_or_default());
    if let Some(t) = &jsonl {
        for m in crate::lanes::said(t).0.into_iter().filter(|m| after(&m["at"])) {
            let text = m["text"].as_str().unwrap_or_default().to_string();
            [&mut replies, &mut prompts][usize::from(m["k"] == "me")].push(text);
        }
        let rows = t.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok());
        for v in rows.filter(|v| after(&v["timestamp"])) {
            let parts = v["message"]["content"].as_array().cloned().unwrap_or_default();
            cmds.extend(
                parts
                    .iter()
                    .filter_map(|c| c["input"]["command"].as_str())
                    .map(crate::lanes::mask),
            );
        }
    } else if let Some(text) = log.and_then(|f| std::fs::read_to_string(f).ok()) {
        let mut open = false; // a reply's wrapped lines follow it
        for l in text.lines().map(crate::lanes::strip) {
            let l = l.trim();
            let glyph = |g: &[char]| {
                l.strip_prefix(g)
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
            };
            open = match (h.tool_line(l), glyph(&['❯', '›', '>']), glyph(&['⏺', '•'])) {
                (Some((t, a)), ..) => {
                    cmds.extend((t == "Bash" || t == "$").then_some(a));
                    false
                }
                (_, Some(t), _) => {
                    prompts.push(t);
                    false
                }
                (.., Some(t)) => {
                    replies.push(t);
                    true
                }
                _ if open && !l.is_empty() => {
                    replies.last_mut().into_iter().for_each(|r| *r += &format!(" {l}"));
                    true
                }
                _ => false,
            };
        }
    }
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
        let who = [Some(p.harness.as_str()), acct]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(":");
        out += &format!(
            "\nOnly what happened since {who}'s last lane on this issue was replaced ({} UTC).",
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
        for i in &items[items.len().saturating_sub(20)..] {
            out += &format!("\n- {}", handover_item(i));
        }
    }
    out + "\nContinue; don't redo."
}

/// #353: one handover item: whitespace collapsed, at most 240 chars (237 and `...`).
fn handover_item(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match s.char_indices().nth(237) {
        Some((i, _)) if s.chars().count() > 240 => format!("{}...", &s[..i]),
        _ => s,
    }
}

/// `YYYY-MM-DDTHH:MM:SS` (UTC) of unix time `t`, as a session transcript's `timestamp` starts.
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

/// The `spawn` event's data for a lane of plan `p` running in `pane`.
fn lane_data(p: &Plan, pane: &str, ws: &str) -> Value {
    json!({"role": p.role, "stage": p.stage, "check": p.family == "rev", "harness": p.harness, "profile": p.profile, "model": p.model, "effort": p.effort, "agent": p.agent,
        "pane": pane, "workspace": ws, "worktree": p.worktree, "branch": p.branch, "account": p.account.as_ref().map(|a| &a.0), "port": p.port})
}

/// `herdsman adopt`: log the lane running in `pane` and name its agent as planned; nothing is started.
pub fn adopt(
    ctx: &Ctx,
    cfg: &Config,
    r: &dyn Runner,
    out: &mut dyn Write,
    (fleet, issue, role): (&str, u64, &str),
    pane: &str,
) -> Result<()> {
    let p = plan(ctx, cfg, fleet, issue, role)?;
    let info = herdr::harness_in(r, pane).with_context(|| format!("no harness running in pane {pane}"))?;
    herdr::ok(r, &herdr::rename_argv(pane, &p.agent))?;
    let ws = info["workspace_id"]
        .as_str()
        .or_else(|| pane.split(':').next())
        .unwrap_or_default();
    let mut d = lane_data(&p, pane, ws);
    d["adopted"] = json!(true);
    emit(&ctx.state, cfg, Event::new(ctx.now, fleet, Some(issue), "spawn", d))?;
    writeln!(
        out,
        "adopted {pane} as {} ({} @ {})",
        p.agent,
        p.branch,
        p.worktree.display()
    )?;
    Ok(())
}

fn print_plan(out: &mut dyn Write, pj: &Value, as_json: bool) -> Result<()> {
    if as_json {
        writeln!(out, "{}", serde_json::to_string_pretty(pj)?)?;
    } else {
        writeln!(
            out,
            "{} [{} {}] {} @ {}",
            pj["agent"], pj["harness"], pj["profile"], pj["branch"], pj["worktree"]
        )?;
        for c in pj["commands"].as_array().into_iter().flatten() {
            let words: Vec<&str> = c.as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            writeln!(out, "  $ {}", words.join(" "))?;
        }
        if pj.get("confirmed") == Some(&json!(false)) {
            writeln!(out, "WARNING: prompt not confirmed landed; check the pane")?;
        }
    }
    Ok(())
}
