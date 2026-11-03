//! The clap CLI, `herdsman help --agent` and dispatch to the modules.
use crate::config::{home, Config};
use crate::events::{emit, fold, read, status_line, Event};
use crate::hosts::all_agents;
use crate::runner::{self, Runner};
use crate::{
    access, check, devices, gate, gh, harness, hook, migrate, pause, protection, queue, queue_change, serve, set,
    spawn, Ctx,
};
use anyhow::{bail, Result};
use clap::{CommandFactory, Parser, Subcommand};
use serde_json::json;
use std::io::{Read, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "herdsman",
    version = crate::VERSION,
    about = "State, gates and harness toggle over herdr",
    disable_help_subcommand = true
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Switch a role's harness/model/profile/effort in fleet.toml; prints old -> new
    Set {
        fleet: String,
        /// impl | rev | design
        role: String,
        /// claude | hermes | dsh
        #[arg(long, value_parser = harness::NAMES)]
        harness: Option<String>,
        /// Model name (claude harness; Hermes profiles own their model)
        #[arg(long)]
        model: Option<String>,
        /// Profile name (default fleet-<role>)
        #[arg(long)]
        profile: Option<String>,
        /// none | low | medium | high | xhigh
        #[arg(long)]
        effort: Option<String>,
    },
    /// Pause a fleet: `next` offers nothing and impl/rev spawns refuse PAUSED; live lanes are untouched
    Pause { fleet: String },
    /// Resume a paused fleet
    Resume { fleet: String },
    /// Create worktree + herdr pane, write .brief.md, launch the role's harness, verify the prompt landed
    Spawn {
        fleet: String,
        issue: String,
        role: String,
        #[command(flatten)]
        o: spawn::Opts,
    },
    /// Record a lane already running in a herdr pane (repair a spawn that was never logged)
    Adopt {
        fleet: String,
        issue: u64,
        role: String,
        /// The herdr pane the harness runs in
        #[arg(long)]
        pane: String,
    },
    /// Record a review verdict pinned to the branch's exact head sha
    Verdict {
        fleet: String,
        issue: u64,
        #[arg(value_parser = ["PASS", "FAIL"])]
        verdict: String,
        /// Full 40-hex head sha that was reviewed
        #[arg(long)]
        sha: String,
        /// The reviewer's verdict.md
        #[arg(long)]
        file: String,
    },
    /// Merge the issue's PR into the integration branch if every gate passes; writes the one closing comment, closes the issue
    Merge {
        fleet: String,
        issue: u64,
        /// Owner only: merge past ROUND_LIMIT (never past PASS, CI or serial); logged `by: owner`
        #[arg(long)]
        owner_override: bool,
    },
    /// Owner only: one more review round for a needs-human issue (its card's [one more round]); logs `round-extended`
    Extend {
        fleet: String,
        issue: u64,
        /// Who approved it, as the log shows it
        #[arg(long, default_value = "owner")]
        by: String,
        /// Record the extra round only; leave the live lane alone (no impl spawn)
        #[arg(long)]
        no_spawn: bool,
    },
    /// End a fix round with no new commit: drive reviews the same head again (a CI-only FAIL with green CI merges)
    Done {
        fleet: String,
        issue: u64,
        /// Why nothing changed (the finding adjudicated away, CI-infra red)
        #[arg(long, value_name = "WHY")]
        no_change: String,
    },
    /// Owner only: park a needs-human issue (its card's [park]); the card clears
    Park { fleet: String, issue: u64 },
    /// Hold for a decision: log it, notify the fleet with numbered options and the answer command, print its id
    Ask {
        fleet: String,
        issue: String,
        question: String,
        /// One choice (at most 4)
        #[arg(long = "option", value_name = "TEXT")]
        options: Vec<String>,
        /// The recommended option's number
        #[arg(long, value_name = "N")]
        recommend: Option<usize>,
        /// routine (defaults to --recommend after [limits].ask_default_minutes) | owner (always waits)
        #[arg(long, default_value = "owner", value_parser = ["routine", "owner"])]
        kind: String,
    },
    /// Answer an open ask (option number or free text); prompts the asking agent `DECISION <id>: <text>`
    Answer {
        fleet: String,
        id: String,
        choice: String,
        /// Who answered, as the board shows it (e.g. owner, alice)
        #[arg(long, default_value = "owner")]
        by: String,
    },
    /// The open asks, oldest first, with age
    Asks {
        #[command(flatten)]
        scope: Scope,
        /// Closed asks too, with why: answered, closed: merged, closed: lane gone
        #[arg(long)]
        all: bool,
    },
    /// Per-issue state, lane and last-event age
    Status(Scope),
    /// Every lane waiting on drive or the owner, with its next command (the owner's watchdog)
    Waiting(Scope),
    /// Print the issue spec cached at spawn; read this instead of `gh issue view`
    Brief { fleet: String, issue: u64 },
    /// The issue PR's head sha, base, mergeable and check rollup: one `gh pr view` per 60 s
    Pr {
        fleet: String,
        issue: u64,
        /// Machine-readable output
        #[arg(long)]
        json: bool,
    },
    /// Poll `herdsman pr` with backoff (30 s → 5 min); exit 0 reached, 1 failed check/closed PR, 124 timeout
    Wait {
        fleet: String,
        issue: u64,
        /// ci-done | ci-green | merged
        #[arg(long, value_enum)]
        until: gh::Until,
        /// Give up after this many seconds (exit 124)
        #[arg(long, default_value_t = 3600)]
        timeout: u64,
    },
    /// The fleet's ordered work queue (kept as queue events in the log)
    Queue {
        fleet: String,
        #[command(subcommand)]
        cmd: QueueCmd,
    },
    /// Print the first queued issue that has no lane and is not blocked; exit 0 found, 1 empty, 2 top item blocked
    Next { fleet: String },
    /// Compact board (≤ 60 cols): queue order, state, lane, age, last event
    Board { fleet: Option<String> },
    /// Each `[hosts]` host: on/off, reachable, tags, load, memory, disk and lanes (docs/hosts.md)
    #[command(alias = "host")]
    Hosts {
        #[command(subcommand)]
        cmd: Option<HostsCmd>,
    },
    /// Phone page (PWA): queue, lanes, host, events; buttons for queue order, pause/resume, role model/effort
    Serve {
        #[command(subcommand)]
        cmd: Option<ServeCmd>,
        /// Listen address
        #[arg(long, default_value = "127.0.0.1:7777")]
        addr: String,
        /// GET only: no buttons, every POST is 405
        #[arg(long)]
        read_only: bool,
        /// Write the page as static files into this dir instead of serving (docs/serve.md)
        #[arg(long)]
        export: Option<std::path::PathBuf>,
        /// With --export: the inert demo, its herdr/gh/host reads canned in runner.json next to fleet.toml
        #[arg(long, requires = "export")]
        demo: bool,
    },
    /// Web push to the page's subscribed devices
    Push {
        #[command(subcommand)]
        cmd: PushCmd,
    },
    /// One pass of the loop for every unpaused fleet: do every step that's due, then exit; safe from cron
    Drive {
        fleet: Option<String>,
        /// #212: only record verdicts and merge PASS + green (the every-minute job); no lane starts
        #[arg(long)]
        fast: bool,
    },
    /// Drive one issue's due steps now, detached (lane hooks and board taps run it); the timers stay the safety net
    Kick {
        fleet: String,
        issue: u64,
        /// What happened: push, verdict, ask, stop, board
        #[arg(long, default_value = "cli")]
        why: String,
        #[arg(long, hide = true)]
        now: bool,
    },
    /// CI: formatted lines of src/**/*.rs against `line_cap` in --config; exit 1 over it
    #[command(hide = true)]
    CapCheck {
        #[arg(long)]
        config: std::path::PathBuf,
    },
    /// Write and load the tick and board jobs, tailscale serve, profiles sync, then doctor (its exit code); safe to rerun.
    /// `setup <host> <name>` (#303): run that configured login in a herdr pane on the host instead
    Setup {
        #[arg(requires = "name")]
        host: Option<String>,
        name: Option<String>,
        /// Write the jobs without loading them
        #[arg(long)]
        no_load: bool,
    },
    /// The 5-minute `herdsman tick` job: a launchd agent on macOS, a systemd user timer on Linux
    Timer {
        #[arg(value_parser = ["install", "remove", "status"])]
        action: String,
        /// The job's label
        #[arg(long, default_value = crate::timer::TICK)]
        label: String,
        /// install: write the job without loading it
        #[arg(long)]
        no_load: bool,
    },
    /// One timer pass: check, then drive; refusals exit 0, a real error logs `tick-failed` (alerts at most hourly)
    Tick,
    /// Alert once per state on stall, lane-dead, ready-unmerged, ask-waiting, orphan-lane; idempotent, safe from cron
    Check,
    /// Each `[accounts]` account: ok / capped until hh:mm, and the lanes on it (names only, never tokens)
    Accounts {
        #[command(subcommand)]
        cmd: Option<AccountsCmd>,
    },
    /// Read-only host check: herdr, gh, config, accounts, harnesses, serve, phone, watchdog; ✓/✗/! with the fix per line, exit 1 on any ✗
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Read-only: is GitHub branch protection on for main, staging and the integration branch of each fleet repo? Exit 1 unless all are
    Protection,
    /// Remove worktree + pane of merged or closed-issue lanes (never live ones; unsaved work goes to `wip/` first, kept on a secret) and list orphan folders; dry run unless --yes
    Clean {
        fleet: Option<String>,
        /// Only this agent's worktree; refuses UNSAFE_CLEAN with the reason
        #[arg(long, value_name = "AGENT")]
        lane: Option<String>,
        /// Actually remove
        #[arg(long)]
        yes: bool,
        /// With --yes, remove the orphans too: folders under the worktree root no fleet owns
        #[arg(long)]
        orphans: bool,
    },
    /// Send Esc to an agent's pane (it pauses); logs `stop`
    Stop { agent: String },
    /// Close the pane of a herdr agent that is not a herdsman lane (IS_LANE for lanes); files and worktrees stay
    Close { agent: String },
    /// Close an agent's pane, pushing unpushed or dirty work to `wip/<agent>-<stamp>` first; the worktree stays. Dry run unless --yes
    Kill {
        agent: String,
        /// Actually kill
        #[arg(long)]
        yes: bool,
    },
    /// Manage per-role Claude Code config folders
    Profiles {
        #[command(subcommand)]
        cmd: ProfilesCmd,
    },
    /// Move pre-rename ~/.config/fleet and ~/.local/state/fleet to their herdsman paths (HERDSMAN_HOME selects the tree); all or nothing
    Migrate,
    /// Help; --agent prints the usage doc agents get
    Help {
        /// Print the agent usage doc
        #[arg(long)]
        agent: bool,
    },
    /// Claude Code PreToolUse / Hermes pre_tool_call hook (reads hook JSON on stdin)
    #[command(hide = true)]
    Hook {
        /// The lane's role; `rev` is read-only
        #[arg(long)]
        role: Option<String>,
        /// Act as git's pre-push hook (reads `<local ref> <sha> <remote ref> <sha>` lines on stdin)
        #[arg(long)]
        git_pre_push: bool,
    },
    /// Claude Code status line: records the lane account's 5h/7d limits (reads status JSON on stdin)
    #[command(hide = true)]
    Statusline,
    /// #275: log a lane's own event (`hook-denied`, `coauthor-trailer`), forwarded from a lane on another host
    #[command(hide = true)]
    LaneEvent { kind: String, data: String },
}

/// A read over one fleet or all: `[FLEET] [--json]`.
#[derive(clap::Args)]
struct Scope {
    fleet: Option<String>,
    /// Machine-readable output
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
enum QueueCmd {
    /// Append issues to the queue (or put them on top / after N)
    Add {
        #[arg(required = true)]
        issues: Vec<u64>,
        /// Put them at the top
        #[arg(long, conflicts_with = "after")]
        top: bool,
        /// Put them right after issue N
        #[arg(long, value_name = "N")]
        after: Option<u64>,
        /// They touch the fleet's serial_paths (DB migrations): serial_max (default 1) in flight at a time
        #[arg(long)]
        serial: bool,
        /// Host tags their lanes need besides the fleet's (`docker,gpu`)
        #[arg(long, value_delimiter = ',')]
        needs: Vec<String>,
    },
    /// Move a queued issue
    #[command(group(clap::ArgGroup::new("to").required(true).args(["top", "before", "after"])))]
    Mv {
        issue: u64,
        /// Move it to the top
        #[arg(long)]
        top: bool,
        /// Move it right before issue N
        #[arg(long, value_name = "N")]
        before: Option<u64>,
        /// Move it right after issue N
        #[arg(long, value_name = "N")]
        after: Option<u64>,
    },
    /// Drop an issue from the queue
    Rm { issue: u64 },
    /// Replace a queued issue's `needs` host tags (`queue set 12 needs docker,gpu`; empty clears)
    Set {
        issue: u64,
        #[arg(value_parser = ["needs"])]
        key: String,
        #[arg(value_delimiter = ',')]
        tags: Vec<String>,
    },
    /// Hold an issue until another one is merged
    Block {
        issue: u64,
        /// The blocking issue
        #[arg(long, value_name = "ISSUE")]
        on: u64,
    },
    /// List the queue in order
    Ls {
        /// Machine-readable output
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ServeCmd {
    /// Now an alias: prints how to pair (tap Pair on the page, then `serve approve <code>`)
    Pair,
    /// Pair the phone showing this 6-digit code (single use, 10 min; NO_SUCH_CODE / EXPIRED otherwise)
    Approve { code: String },
    /// The open pairing requests: code, device, minutes left
    Pending,
    /// The paired devices and the days until each expires (a year after its last use)
    Devices,
}

#[derive(Subcommand)]
enum PushCmd {
    /// Send a test alert to every subscribed device (needs a `type = "webpush"` notifier for its `sub`)
    Test,
}

#[derive(Subcommand)]
enum AccountsCmd {
    /// Let spawns use a capped account again
    Reset { name: String },
}

#[derive(Subcommand)]
enum HostsCmd {
    /// Probe every host's tags again (else hourly, by `check`)
    Refresh,
    /// Let new lanes go to this host again; logs `host-on`
    On { name: String },
    /// No new lanes on this host (its lanes finish there); still monitored; logs `host-off`
    Off { name: String },
    /// Owner only: a cloud host's lanes' work to `wip/` first, then its recipe's `destroy`; logs `cloud-stop`
    #[command(hide = true)]
    Destroy { name: String },
}

#[derive(Subcommand)]
enum ProfilesCmd {
    /// Write ~/.config/herdsman/claude/<profile>/ (settings.json + hooks, role.md, fleet.md) and Hermes profiles' `hooks:` + hook approval
    Sync {
        /// Write nothing; exit 1 unless every fleet Hermes profile lists its hook as `✓ allowed`
        #[arg(long)]
        check: bool,
    },
}

/// Line budget of `help --agent` (tests/spawn.rs): raise it here, next to the text it counts (+1 for #130's `--owner-override`, +8 for #151's `extend`/`park`, +1 for #164's `--orphans`, +14 for #157's `hosts`, `queue set`, `--needs`, `--host`, +4 for #319's `done`).
pub const HELP_AGENT_MAX: usize = 222;

/// The one usage doc both harnesses get: the commands generated from the CLI definition, the rest from `roles/usage-*.md`.
pub fn usage_doc() -> String {
    let mut cmd = Cli::command();
    cmd.build();
    let mut s = String::from(include_str!("../roles/usage-head.md"));
    // The owner's commands; the sections below name them.
    let owner = [
        "accounts", "doctor", "drive", "kick", "pause", "resume", "serve", "setup", "tick", "timer",
    ];
    let subs = cmd
        .get_subcommands()
        .filter(|c| !c.is_hide_set() && !owner.contains(&c.get_name()));
    let subs: Vec<_> = subs
        .flat_map(|c| {
            if c.has_subcommands() {
                c.get_subcommands().filter(|c| !c.is_hide_set()).collect()
            } else {
                vec![c]
            }
        })
        .collect();
    for sub in subs {
        let mut sub = sub.clone();
        let about = sub
            .get_about()
            .map(std::string::ToString::to_string)
            .unwrap_or_default();
        let usage = sub.render_usage().to_string().replace("Usage: ", "");
        s += &format!("\n`{}`\n  {about}\n", usage.trim());
        for a in sub
            .get_arguments()
            .filter(|a| a.get_long().is_some_and(|l| l != "help"))
        {
            let h = a.get_help().map(std::string::ToString::to_string).unwrap_or_default();
            s += &format!("  --{}  {h}\n", a.get_long().unwrap());
        }
    }
    s + include_str!("../roles/usage-tail.md")
}

/// Entry point used by main and tests. Returns the process exit code.
pub fn main_with(args: Vec<String>, ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write, stdin: &mut dyn Read) -> i32 {
    let cli = match Cli::try_parse_from(&args) {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            return if e.use_stderr() { 2 } else { 0 };
        }
    };
    if !matches!(
        cli.cmd,
        Cmd::Hook { .. } | Cmd::LaneEvent { .. } | Cmd::Statusline | Cmd::Migrate | Cmd::Help { .. }
    ) {
        for h in &ctx.hints {
            eprintln!("{h}");
        }
    }
    let lane_write = matches!(
        cli.cmd,
        Cmd::Ask { .. } | Cmd::Answer { .. } | Cmd::Verdict { .. } | Cmd::Done { .. }
    );
    let mut sent: Vec<String> = args.into_iter().skip(1).collect();
    for i in 1..sent.len() {
        if sent[i - 1] == "--file" {
            sent[i] = std::path::absolute(&sent[i]).map_or(sent[i].clone(), |p| p.display().to_string());
        }
    }
    let res = match lane_write.then(|| forward(ctx, r, &sent)).flatten() {
        Some(o) => o.map(|o| {
            let _ = write!(out, "{}", o.stdout);
            eprint!("{}", o.stderr);
            o.code
        }),
        None => dispatch(cli.cmd, ctx, r, out, stdin),
    };
    match res {
        Ok(code) => code,
        Err(e) => {
            if let Some(rf) = e.downcast_ref::<gate::Refusal>() {
                let _ = writeln!(out, "{rf}");
                3
            } else {
                eprintln!("herdsman: {e:#}");
                1
            }
        }
    }
}

/// #275: a lane on another host has `HERDSMAN_MAIN` (`<main host's ssh name>:<its herdsman>`): its `args` run on the
/// main host over ssh, so the main log is the only one written and only the main host pushes.
pub(crate) fn forward(ctx: &Ctx, r: &dyn Runner, args: &[String]) -> Option<Result<runner::Out>> {
    let lane = std::env::var("HERDSMAN_LANE").ok().filter(|l| !l.is_empty())?;
    let main = std::env::var("HERDSMAN_MAIN").ok()?;
    let (host, exe) = main.split_once(':')?;
    if ctx.state.exists() {
        let s = ctx.state.display();
        eprintln!("herdsman: warning: {s} exists on a lane's host; only the main host's log counts");
    }
    let a = [runner::argv(&[&format!("HERDSMAN_LANE={lane}"), exe]), args.to_vec()].concat();
    Some(crate::hosts::On::new(r, Some(host)).run("env", &a))
}

/// One command run in process (`herdsman drive`): its exit code, or its error or refusal.
pub(crate) fn run(args: &[&str], ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write) -> Result<i32> {
    let cli = Cli::try_parse_from(std::iter::once("herdsman").chain(args.iter().copied()))?;
    dispatch(cli.cmd, ctx, r, out, &mut std::io::empty())
}

fn dispatch(cmd: Cmd, ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write, stdin: &mut dyn Read) -> Result<i32> {
    match cmd {
        Cmd::Migrate if std::env::var_os("HERDSMAN_HOME").is_some_and(|v| v.is_empty()) => {
            bail!("HERDSMAN_HOME is set but empty; unset it or point it at a home tree")
        }
        Cmd::Migrate => migrate(&ctx.home, out)?,
        Cmd::CapCheck { config } => return crate::overlap::cap_check(&config, out),
        Cmd::Help { agent: true } => write!(out, "{}", usage_doc())?,
        Cmd::Help { agent: false } => write!(out, "{}", Cli::command().render_long_help())?,
        Cmd::Hook {
            role,
            git_pre_push: true,
        } => return pre_push(ctx, r, out, stdin, role.as_deref() == Some("rev")),
        Cmd::Hook {
            role,
            git_pre_push: false,
        } => run_hook(ctx, r, out, stdin, role.as_deref() == Some("rev"))?,
        Cmd::Statusline => {
            let mut buf = String::new();
            let _ = stdin.read_to_string(&mut buf);
            let env = |k: &str| std::env::var(k).unwrap_or_default();
            crate::usage::statusline(ctx, &env("HERDSMAN_ACCOUNT"), &env("HERDSMAN_LANE"), &buf, out);
        }
        Cmd::LaneEvent { kind, data } => lane_event(ctx, r, &kind, serde_json::from_str(&data)?),
        Cmd::Status(Scope { fleet, json }) => {
            let events = read(&ctx.state)?;
            let all = fold(&events);
            let rows: Vec<_> = all
                .values()
                .filter(|s| fleet.as_ref().is_none_or(|f| *f == s.fleet))
                .collect();
            if json {
                // #129: `conflicts`: [(other issue, 0 = integration, [files])] from the last merge-tree preview.
                let rows = rows.iter().map(|s| {
                    let mut v = json!(s);
                    if let Some(c) = crate::overlap::cached(ctx, &s.fleet).remove(&s.issue) {
                        v["conflicts"] = json!(c);
                    }
                    v
                });
                writeln!(out, "{}", serde_json::to_string_pretty(&rows.collect::<Vec<_>>())?)?;
            } else if rows.is_empty() {
                writeln!(out, "no tracked issues")?;
            } else {
                let cfg = Config::load(&ctx.config).ok();
                let serial = |f: &str| queue::serial_use(&events, &all, f, cfg.as_ref()).map(|s| format!("{f}: {s}\n"));
                let mut serial: Vec<_> = rows.iter().filter_map(|s| serial(&s.fleet)).collect();
                serial.dedup();
                let waits = cfg
                    .map(|c| crate::waiting::scan(ctx, &c, r, &events, &all_agents(&c, r).unwrap_or_default(), false));
                // #157: a HOST column once fleet.toml has `[hosts]`.
                let hosts = Config::load(&ctx.config).is_ok_and(|c| !c.hosts.is_empty());
                let pad = [5, 63][usize::from(hosts)];
                let c = |h: &str| format!(" {h:<10}").repeat(usize::from(hosts));
                let head = c("HOST");
                let host = |s: &crate::events::IssueState| {
                    let e = events.iter().rev().find(|e| e.is("spawn", &s.fleet, s.issue));
                    c(e.map_or("-", |e| e.str("host").unwrap_or("local")))
                };
                writeln!(
                    out,
                    "{:<12} {:<6} {:<12} {:<24} {:<5}{} WAITING",
                    "FLEET", "ISSUE", "STATE", "LANE", "AGE", head
                )?;
                for s in rows {
                    let w = waits
                        .iter()
                        .flatten()
                        .find(|(x, _)| x.fleet == s.fleet && x.issue == s.issue)
                        .map(|w| w.1.what.clone())
                        .or_else(|| {
                            let skip = queue::skipped(&events, &all, &s.fleet, s.issue, ctx.now);
                            skip.map(|(c, why)| format!("skipped this pass: {c} ({why})"))
                        })
                        .unwrap_or_else(|| "-".into());
                    writeln!(out, "{:<pad$}{} {w}", status_line(s, ctx.now), host(s))?;
                }
                write!(out, "{}", serial.concat())?;
            }
        }
        Cmd::Set {
            fleet,
            role,
            harness,
            model,
            profile,
            effort,
        } => {
            let wanted = [
                ("harness", harness),
                ("model", model),
                ("profile", profile),
                ("effort", effort),
            ];
            set(
                ctx,
                out,
                &fleet,
                &role,
                wanted.into_iter().filter_map(|(k, v)| Some((k, v?))).collect(),
                None,
            )?;
        }
        Cmd::Pause { fleet } => pause(ctx, out, &fleet, true, None)?,
        Cmd::Resume { fleet } => pause(ctx, out, &fleet, false, None)?,
        Cmd::Next { fleet } => {
            if Config::load(&ctx.config).is_ok_and(|c| c.fleet.get(&fleet).is_some_and(|f| f.paused)) {
                writeln!(out, "{fleet}: paused")?;
                return Ok(1);
            }
            let (code, line) = queue::next(&read(&ctx.state)?, &fleet);
            writeln!(out, "{line}")?;
            return Ok(code);
        }
        Cmd::Board { fleet } => {
            let events = read(&ctx.state)?;
            let all = fleet.map_or_else(|| queue::fleets(&events), |f| vec![f]);
            let cfg = Config::load(&ctx.config).ok();
            let boards: Vec<_> = all
                .iter()
                .map(|f| queue::board(&events, f, ctx.now, cfg.as_ref(), &crate::overlap::cached(ctx, f)))
                .collect();
            write!(out, "{}", boards.join("\n"))?;
        }
        Cmd::Asks {
            scope: Scope { fleet, json },
            all,
        } => crate::asks(ctx, out, fleet.as_deref(), json, all)?,
        Cmd::Serve {
            cmd: Some(ServeCmd::Pair),
            ..
        } => writeln!(out, "now: open the page, tap Pair, then run serve approve <code>")?,
        Cmd::Serve {
            cmd: Some(ServeCmd::Approve { code }),
            ..
        } => writeln!(out, "paired {}", devices::approve(ctx, &code)?)?,
        Cmd::Serve {
            cmd: Some(ServeCmd::Devices),
            ..
        } => {
            for d in devices::load(ctx).into_iter().filter(|d| d.code.is_none()) {
                let left = (d.used + devices::YEAR).saturating_sub(ctx.now);
                if left > 0 {
                    writeln!(out, "{} expires in {}d", d.label, left / 86_400)?;
                }
            }
        }
        Cmd::Serve {
            cmd: Some(ServeCmd::Pending),
            ..
        } => devices::pending(ctx).iter().try_for_each(|d| {
            writeln!(
                out,
                "{} {} {}m left",
                d.code.as_deref().unwrap_or_default(),
                d.label,
                (d.created + 600 - ctx.now).div_ceil(60)
            )
        })?,
        Cmd::Serve {
            export: Some(dir),
            demo,
            ..
        } => serve::export(ctx, r, &dir, demo)?,
        Cmd::Serve { addr, read_only, .. } => serve::serve(ctx, r, &addr, read_only)?,
        Cmd::Push { cmd: PushCmd::Test } => {
            let cfg = Config::load(&ctx.config)?;
            let Some(sub) = cfg.webpush_sub() else {
                bail!("no `type = \"webpush\"` notifier in fleet.toml (docs/serve.md)")
            };
            let ev = crate::events::Event::new(ctx.now, "herdsman", None, "push-test", json!({"reason": "test alert"}));
            writeln!(out, "{}", crate::webpush::send(&ctx.state, sub, &ev, None)?)?;
        }
        Cmd::Queue {
            fleet,
            cmd: QueueCmd::Ls { json },
        } => {
            let rows = queue::ls_json(&read(&ctx.state)?, &fleet);
            if json {
                writeln!(out, "{}", serde_json::to_string_pretty(&rows)?)?;
            }
            for r in rows.as_array().into_iter().flatten().filter(|_| !json) {
                let mut on = r["blocked_by"]
                    .as_u64()
                    .map_or(String::new(), |b| format!("  blocked by #{b}"));
                if r["serial"] == true {
                    on += "  serial";
                }
                if r["needs"].as_array().is_some_and(|n| !n.is_empty()) {
                    on += &format!("  needs {}", r["needs"].to_string().replace(['[', ']', '"'], ""));
                }
                let n = |k: &str| r[k].as_u64().unwrap_or_default();
                writeln!(
                    out,
                    "{:>2} #{:<5} {}{on}",
                    n("pos"),
                    n("issue"),
                    r["state"].as_str().unwrap_or_default()
                )?;
            }
        }
        Cmd::Doctor { json } => return crate::doctor::doctor(ctx, r, out, json),
        Cmd::Setup { host, name, no_load } => {
            let Some((h, n)) = host.zip(name) else {
                return crate::timer::setup(ctx, r, out, !no_load);
            };
            let pane = crate::setup::start(ctx, &Config::load(&ctx.config)?, r, &h, &n, "cli")?;
            writeln!(
                out,
                "{n} on {h}: herdr pane {pane}; it closes when the login exits or in 10 min"
            )?;
        }
        Cmd::Tick => return crate::timer::tick(ctx, r, out),
        cmd => {
            let cfg = Config::load(&ctx.config)?;
            let kick = |f: &str, n, why| crate::drive::kick(ctx, r, f, n, why);
            match cmd {
                Cmd::Profiles {
                    cmd: ProfilesCmd::Sync { check: true },
                } => {
                    if !spawn::profiles_check(ctx, &cfg, r, out)? {
                        return Ok(1);
                    }
                }
                Cmd::Profiles {
                    cmd: ProfilesCmd::Sync { check: false },
                } => spawn::profiles_sync(ctx, &cfg, &usage_doc(), out)?,
                Cmd::Spawn { fleet, issue, role, o } => {
                    // #155: the orch role is gone; `- orch` (and any orch spawn) is refused.
                    if role == "orch" {
                        let why = "herdsman drives fleets itself; see herdsman timer";
                        return gate::refused("NO_ORCH", why);
                    }
                    let issue = issue
                        .parse()
                        .map_err(|_| anyhow::anyhow!("issue must be a number, not `{issue}`"))?;
                    if !spawn::spawn(ctx, &cfg, r, out, (&fleet, issue, &role), &o)? {
                        return Ok(4);
                    }
                }
                Cmd::Adopt {
                    fleet,
                    issue,
                    role,
                    pane,
                } => {
                    spawn::adopt(ctx, &cfg, r, out, (&fleet, issue, &role), &pane)?;
                }
                Cmd::Ask {
                    fleet,
                    issue,
                    question,
                    options,
                    recommend: n,
                    kind,
                } => {
                    let at = lane(&issue)?;
                    crate::ask(ctx, &cfg, out, (&fleet, at), &question, &options, (n, &kind))?;
                    at.inspect(|n| kick(&fleet, *n, "ask"));
                }
                Cmd::Answer { fleet, id, choice, by } => {
                    crate::answer(ctx, &cfg, r, out, (&fleet, &id), &choice, (&by, None))?;
                }
                Cmd::Check => check::check(ctx, &cfg, r, out)?,
                Cmd::Hosts { cmd } => match cmd {
                    None => crate::hosts::list(ctx, &cfg, r, out, false)?,
                    Some(HostsCmd::Refresh) => crate::hosts::list(ctx, &cfg, r, out, true)?,
                    Some(HostsCmd::On { name }) => crate::hosts::toggle(ctx, out, &name, true, "cli")?,
                    Some(HostsCmd::Off { name }) => crate::hosts::toggle(ctx, out, &name, false, "cli")?,
                    Some(HostsCmd::Destroy { name }) => {
                        let wip = crate::cloud::stop(ctx, &cfg, r, &name, "destroy", "owner")?;
                        writeln!(out, "{name}: destroyed; wip {}", wip.join(" "))?;
                    }
                },
                Cmd::Timer { action, label, no_load } => {
                    return crate::timer::timer(ctx, &cfg, r, out, (&action, &label, !no_load));
                }
                Cmd::Drive { fleet, fast } => crate::drive::drive(ctx, &cfg, r, out, fleet.as_deref(), false, fast)?,
                Cmd::Kick { fleet, issue, why, now } => {
                    crate::drive::kicked(ctx, &cfg, r, out, (&fleet, issue, &why, now))?;
                }
                Cmd::Waiting(Scope { fleet, json }) => {
                    let all = crate::waiting::scan(
                        ctx,
                        &cfg,
                        r,
                        &read(&ctx.state)?,
                        &all_agents(&cfg, r).unwrap_or_default(),
                        true,
                    );
                    let rows: Vec<_> = all
                        .into_iter()
                        .filter(|(s, w)| s.lane.as_ref() == Some(&w.lane))
                        .map(|x| x.1)
                        .filter(|w| matches!(w.who, "drive" | "owner") && fleet.as_ref().is_none_or(|f| *f == w.fleet))
                        .collect();
                    if json {
                        writeln!(out, "{}", serde_json::to_string_pretty(&rows)?)?;
                    }
                    for w in rows.iter().filter(|_| !json) {
                        writeln!(
                            out,
                            "{} #{} {}: {} ({}){}",
                            w.fleet,
                            w.issue,
                            w.who,
                            w.what,
                            crate::events::age(ctx.now.saturating_sub(w.since)),
                            w.next_cmd
                                .as_deref()
                                .or((w.who == "drive").then_some("?"))
                                .map_or(String::new(), |c| format!(" → {c}"))
                        )?;
                    }
                }
                Cmd::Accounts { cmd: None } => access::list(ctx, &cfg, r, out)?,
                Cmd::Accounts {
                    cmd: Some(AccountsCmd::Reset { name }),
                } => access::reset(ctx, &cfg, out, &name)?,
                Cmd::Protection => return protection::protection(&cfg, r, out),
                Cmd::Clean {
                    fleet,
                    lane,
                    yes,
                    orphans,
                } => {
                    check::clean(ctx, &cfg, r, out, fleet.as_deref(), (None, lane.as_deref()), yes)?;
                    if lane.is_none() {
                        check::orphans(ctx, &cfg, out, orphans && yes)?;
                    }
                }
                Cmd::Stop { agent } => check::stop(ctx, &cfg, r, out, &agent)?,
                Cmd::Close { agent } => check::close(ctx, &cfg, r, out, &agent)?,
                Cmd::Kill { agent, yes } => check::kill(ctx, &cfg, r, out, &agent, yes)?,
                Cmd::Verdict {
                    fleet,
                    issue,
                    verdict,
                    sha,
                    file,
                } => {
                    let pass = verdict == "PASS";
                    gate::verdict(ctx, &cfg, r, out, (&fleet, issue), pass, &sha, &file)?;
                    kick(&fleet, issue, "verdict");
                }
                Cmd::Merge {
                    fleet,
                    issue,
                    owner_override: o,
                } => gate::merge(ctx, &cfg, r, out, &fleet, issue, o)?,
                Cmd::Extend {
                    fleet,
                    issue,
                    by,
                    no_spawn,
                } => gate::needs_human(ctx, &cfg, r, out, (&fleet, issue), "extend", (!no_spawn, &by))?,
                Cmd::Done {
                    fleet,
                    issue,
                    no_change,
                } => {
                    let e = Event::new(ctx.now, &fleet, Some(issue), "fix-done", json!({"why": no_change}));
                    emit(&ctx.state, &cfg, e)?;
                    writeln!(out, "recorded {fleet}#{issue} fix-done: {no_change}")?;
                }
                Cmd::Park { fleet, issue } => {
                    gate::needs_human(ctx, &cfg, r, out, (&fleet, issue), "park", (false, "owner"))?;
                }
                Cmd::Brief { fleet, issue } => {
                    let v = gh::spec(ctx, r, cfg.fleet(&fleet)?, &fleet, issue, false)?;
                    let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
                    writeln!(out, "# Issue #{issue}: {}\n\n{}", s("title"), s("body"))?;
                }
                Cmd::Pr { fleet, issue, json } => {
                    let branch = gate::branch_of(gate::issue_state(ctx, &fleet, issue)?.as_ref(), issue);
                    let pr = gh::pr(ctx, r, cfg.fleet(&fleet)?, (&fleet, issue), &branch, ctx.now)?;
                    if json {
                        writeln!(out, "{}", serde_json::to_string_pretty(&pr)?)?;
                    } else {
                        writeln!(
                            out,
                            "{}",
                            gh::summary(&pr, &branch, &cfg.fleet(&fleet)?.required_checks)
                        )?;
                    }
                    if pr.is_null() {
                        return Ok(1);
                    }
                }
                Cmd::Wait {
                    fleet,
                    issue,
                    until,
                    timeout,
                } => {
                    let branch = gate::branch_of(gate::issue_state(ctx, &fleet, issue)?.as_ref(), issue);
                    return gh::wait(
                        ctx,
                        r,
                        out,
                        cfg.fleet(&fleet)?,
                        (&fleet, issue),
                        &branch,
                        until,
                        timeout,
                    );
                }
                Cmd::Queue { fleet, cmd } => {
                    cfg.fleet(&fleet)?;
                    let d = match cmd {
                        QueueCmd::Add {
                            issues,
                            top,
                            after,
                            serial,
                            needs,
                        } => {
                            let mut d = queue::op("add", &issues, top, None, after, None);
                            if serial {
                                d["serial"] = json!(true);
                            }
                            if !needs.is_empty() {
                                d["needs"] = json!(needs);
                            }
                            d
                        }
                        QueueCmd::Set { issue, tags, .. } => {
                            let mut d = queue::op("set", &[issue], false, None, None, None);
                            d["needs"] = json!(tags);
                            d
                        }
                        QueueCmd::Mv {
                            issue,
                            top,
                            before,
                            after,
                        } => queue::op("mv", &[issue], top, before, after, None),
                        QueueCmd::Rm { issue } => queue::op("rm", &[issue], false, None, None, None),
                        QueueCmd::Block { issue, on } => queue::op("block", &[issue], false, None, None, Some(on)),
                        QueueCmd::Ls { .. } => unreachable!(),
                    };
                    writeln!(out, "{}", queue_change(ctx, &cfg, &fleet, d, None)?)?;
                }
                _ => unreachable!(),
            }
        }
    }
    Ok(0)
}

/// Parse an ask's `<ISSUE>` argument (`-`: the fleet, no issue).
fn lane(issue: &str) -> Result<Option<u64>> {
    match issue {
        "-" => Ok(None),
        n => {
            Ok(Some(n.parse().map_err(|_| {
                anyhow::anyhow!("issue must be a number or `-`, not `{n}`")
            })?))
        }
    }
}

fn run_hook(ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write, stdin: &mut dyn Read, reviewer: bool) -> Result<()> {
    let mut buf = String::new();
    stdin.read_to_string(&mut buf)?;
    // Inert outside fleet lanes (they share the Hermes profiles); `HERDSMAN_LANE` beats `FLEET_LANE`, even empty.
    let lane = std::env::var_os("HERDSMAN_LANE").or_else(|| std::env::var_os("FLEET_LANE"));
    if lane.is_none_or(|v| v.is_empty()) {
        return Ok(());
    }
    let Ok(input) = serde_json::from_str::<serde_json::Value>(&buf) else {
        eprintln!("herdsman hook: input is not JSON; allowing");
        return Ok(());
    };
    // #304: a turn's end or a push that went through kicks the lane's issue at once.
    if let Some(why) = hook::kick_why(&input) {
        let lane = std::env::var("HERDSMAN_LANE").unwrap_or_default();
        let mut it = lane.split(':');
        if let (Some(f), Some(n)) = (it.next(), it.next().and_then(|n| n.parse().ok())) {
            crate::drive::kick(ctx, r, f, n, why);
        }
    }
    if matches!(
        input["hook_event_name"].as_str(),
        Some("PostToolUse" | "post_tool_call" | "Stop" | "on_session_end")
    ) {
        return Ok(()); // after the fact: nothing left to guard
    }
    let branches = protected_branches(ctx);
    let dir = |p: &PathBuf| p.parent().unwrap_or(p).to_string_lossy().into_owned();
    let h = home();
    let paths = vec![
        dir(&ctx.config),
        dir(&ctx.state),
        h.join(".config/fleet-operations").to_string_lossy().into(),
        h.join(".hermes/profiles").to_string_lossy().into(),
    ];
    let policy = hook::Policy {
        branches,
        paths,
        reviewer,
        deny: deny_list(ctx),
    };
    let cur = |d: &str| {
        runner::ok(r, "git", &["-C", d, "rev-parse", "--abbrev-ref", "HEAD"])
            .ok()
            .map(|s| s.trim().to_string())
    };
    if let Some(reason) = hook::decide(&input, &policy, &cur) {
        if policy.deny.iter().any(|d| d.reason() == reason) {
            lane_event(ctx, r, "hook-denied", json!({"rule": reason}));
        }
        writeln!(out, "{}", hook::deny_json(&input, &reason))?;
    } else if let Some(dir) = hook::push_dir(&input) {
        coauthor_warn(ctx, r, &dir, "HEAD");
    }
    Ok(())
}

/// #198: warn (never block) when an unpushed commit carries an AI `Co-Authored-By:`; logged `coauthor-trailer`.
fn coauthor_warn(ctx: &Ctx, r: &dyn Runner, dir: &str, rev: &str) {
    let msgs = runner::ok(r, "git", &["-C", dir, "log", rev, "--not", "--remotes", "--format=%B"]);
    let Some(trailer) = msgs.as_deref().ok().and_then(hook::ai_coauthor) else {
        return;
    };
    eprintln!("herdsman: warning: a pushed commit carries `{trailer}`; lanes add no AI co-author trailers");
    lane_event(ctx, r, "coauthor-trailer", json!({"line": trailer}));
}

/// Logs `kind` for this lane (`HERDSMAN_LANE`'s fleet and issue), its data plus `lane`.
fn lane_event(ctx: &Ctx, r: &dyn Runner, kind: &str, mut d: serde_json::Value) {
    if forward(ctx, r, &runner::argv(&["lane-event", kind, &d.to_string()])).is_some() {
        return;
    }
    let lane = std::env::var("HERDSMAN_LANE").unwrap_or_default();
    let mut it = lane.split(':');
    let (fleet, issue) = (it.next().unwrap_or_default(), it.next().and_then(|n| n.parse().ok()));
    d["lane"] = json!(lane);
    if let Ok(cfg) = Config::load(&ctx.config) {
        let _ = emit(&ctx.state, &cfg, Event::new(ctx.now, fleet, issue, kind, d));
    }
}

/// #216: the lane's fleet's `deny_commands` (`[limits]`'s when the fleet is unknown).
fn deny_list(ctx: &Ctx) -> Vec<crate::config::Deny> {
    let cfg = Config::load(&ctx.config).unwrap_or_default();
    let lane = std::env::var("HERDSMAN_LANE").unwrap_or_default();
    let f = cfg.fleet.get(lane.split(':').next().unwrap_or_default());
    f.map_or_else(|| cfg.limits.deny_commands.clone(), |f| f.deny(&cfg.limits))
}

/// main, master, staging and every fleet's integration branch.
fn protected_branches(ctx: &Ctx) -> Vec<String> {
    let mut branches: Vec<String> = ["main", "master", "staging"].map(String::from).to_vec();
    if let Ok(cfg) = Config::load(&ctx.config) {
        branches.extend(cfg.fleet.values().map(|f| f.integration.clone()));
    }
    branches
}

/// git `pre-push` hook for dsh lanes: exit 1 refuses a protected-branch push, or any reviewer push.
fn pre_push(ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write, stdin: &mut dyn Read, reviewer: bool) -> Result<i32> {
    let mut buf = String::new();
    stdin.read_to_string(&mut buf)?;
    let branches = protected_branches(ctx);
    for dst in buf.lines().filter_map(|l| l.split_whitespace().nth(2)) {
        let b = dst.trim_start_matches("refs/heads/");
        if reviewer {
            writeln!(
                out,
                "herdsman: reviewers are read-only: no `git push` (to `{b}`); write verdict.md and report in your pane"
            )?;
            return Ok(1);
        }
        if branches.iter().any(|x| x == b) {
            writeln!(
                out,
                "herdsman: pushing to `{b}` is blocked; integration branches change only via `herdsman merge`"
            )?;
            return Ok(1);
        }
    }
    // #216: no added script line may run a `deny_commands` entry (CI workflows may use Docker).
    let deny = deny_list(ctx);
    for sha in buf.lines().filter_map(|l| l.split_whitespace().nth(1)) {
        let specs = ["*.sh", "*.bash", "*.zsh", "*justfile", "*Justfile", "*Makefile"];
        let mut args = vec!["log", "-p", "--format=", sha, "--not", "--remotes", "--"];
        args.extend(specs.iter().chain(&[":!.github/workflows"]));
        let diff = runner::ok(r, "git", &args).unwrap_or_default();
        let mut hit = diff.lines().filter(|l| l.starts_with('+') && !l.starts_with("+++"));
        if let Some(d) = hit.find_map(|l| hook::denied(&l[1..], &deny)) {
            lane_event(ctx, r, "hook-denied", json!({"rule": d.reason()}));
            writeln!(out, "herdsman: a pushed script runs `{}`: {}", d.cmd(), d.reason())?;
            return Ok(1);
        }
        coauthor_warn(ctx, r, ".", sha);
    }
    Ok(0)
}
