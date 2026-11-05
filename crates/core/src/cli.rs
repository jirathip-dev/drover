//! The clap CLI, `herdsman help --agent` and dispatch to the modules.
use crate::config::{home, Config};
use crate::events::{emit, read, status_line, Event};
use crate::hosts::all_agents;
use crate::runner::{self, Runner};
use crate::{
    access, check, devices, gate, gh, harness, hook, migrate, pause, protection, queue, queue_change, set, spawn, Ctx,
};
use anyhow::{bail, Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use serde_json::json;
use std::io::{Read, Write};
use std::path::PathBuf;

/// #264: (`serve`, `serve --export`), set by the binary: the server crate depends on this one, never the reverse.
pub static SERVE: std::sync::OnceLock<(Serve, Export)> = std::sync::OnceLock::new();
type Serve = fn(&Ctx, &dyn Runner, &str, bool) -> Result<()>;
type Export = fn(&Ctx, &dyn Runner, &std::path::Path, bool) -> Result<()>;
/// #345: `herdsman service` (api addr, board addr), set by the binary from the service crate.
pub static SERVICE: std::sync::OnceLock<Service> = std::sync::OnceLock::new();
type Service = fn(&Ctx, &str, Option<&str>) -> Result<()>;

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
        /// Ask in this conductor thread: its options become choice rows, the answer goes back into it
        #[arg(long, value_name = "ID")]
        thread: Option<String>,
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
    /// The board's bug reports (#363), newest first: id, note, bundle path; with an id, that bundle
    Report { id: Option<String> },
    /// Conductor chat threads per repo (or `all`), each talked through by one conductor lane (docs/conductor.md)
    Thread {
        #[command(subcommand)]
        cmd: ThreadCmd,
    },
    /// #387: fleet.toml changes as thread cards only the owner applies (docs/conductor.md)
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// #387: secrets in the host's keychain as `herdsman/<fleet>/<NAME>`; fleet `env` reads them as `secret:<NAME>`
    Secret {
        #[command(subcommand)]
        cmd: SecretCmd,
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
    /// Every open issue's latest lane: what it waits on, with its next command (the owner's watchdog)
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
    /// File this week's lessons issue, queue it on top and spawn its `lesson` lane (conductors and the owner only)
    #[command(args_conflicts_with_subcommands = true)]
    Lesson {
        #[command(subcommand)]
        cmd: Option<LessonCmd>,
        fleet: Option<String>,
    },
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
    /// CI: formatted lines of src/**/*.rs against `line_cap` in --config; exit 1 over it or a --rules file over 30 lines
    #[command(hide = true)]
    CapCheck {
        #[arg(long)]
        config: std::path::PathBuf,
        #[arg(long, default_value = ".herdsman/rules.md")]
        rules: std::path::PathBuf,
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
    /// One supervised process: the board, kicks driven at once, the GitHub poll, API v1 for remote lanes (docs/service.md)
    Service {
        /// API v1 listen address (remote lanes reach it over the tailnet)
        #[arg(long, default_value = "127.0.0.1:7778")]
        api: String,
        /// Also serve the board here (the `serve` job's address)
        #[arg(long)]
        board: Option<String>,
        /// Write its launchd/systemd unit and print the commands that load it; loads nothing
        #[arg(long)]
        install: bool,
    },
    /// Alert once per state on stall, lane-dead, ready-unmerged, ci-missing, ask-waiting, orphan-lane, outbox-stale; idempotent, safe from cron
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
    /// Build folders (target, node_modules, .cache, `build_dirs`) of worktrees idle 7 days: preview, then apply on `y`
    Reclaim {
        #[arg(long, help = "Remove without asking")]
        apply: bool,
    },
    /// Send Esc to an agent's pane (it pauses); logs `stop`
    Stop { agent: String },
    /// Close the pane of a herdr agent that is not a herdsman lane (IS_LANE for lanes); files and worktrees stay
    Close {
        agent: String,
        /// The agent on this `[hosts.<name>]` (when another host has one of the same name)
        #[arg(long)]
        host: Option<String>,
    },
    /// Close an agent's pane, pushing unpushed or dirty work to `wip/<agent>-<stamp>` first; the worktree stays. Dry run unless --yes
    Kill {
        agent: String,
        /// Actually kill
        #[arg(long)]
        yes: bool,
        /// Its lane on this `[hosts.<name>]` (a pane a supersede left there under the same name)
        #[arg(long)]
        host: Option<String>,
    },
    /// Manage per-role Claude Code config folders
    Profiles {
        #[command(subcommand)]
        cmd: ProfilesCmd,
    },
    /// Move pre-rename ~/.config/fleet and ~/.local/state/fleet to their herdsman paths (HERDSMAN_HOME selects the tree); all or nothing
    Migrate {
        /// Allow it on the live state dir (~/.local/state/herdsman); without it migrate refuses there
        #[arg(long)]
        live: bool,
    },
    /// The event log in its JSONL format (one event a line, with its seq), for external readers
    #[command(hide = true)]
    Events {
        /// Print it as JSONL (the only format)
        #[arg(long, required = true)]
        jsonl: bool,
    },
    /// Landing numbers: `stats land <fleet>`
    Stats {
        #[command(subcommand)]
        cmd: StatsCmd,
    },
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
enum ThreadCmd {
    /// Start a thread for a fleet's repo (or `all`); prints its id. A conductor uses `thread handoff`
    New { repo: String, title: String },
    /// A conductor hands off: a new thread in <to> (a repo or `all`) or thread <to>, linked both ways; <text> wakes its conductor as from <from>
    Handoff {
        from: String,
        to: String,
        title: String,
        text: String,
    },
    /// The owner's message: logged, then typed into the thread's conductor (started or resumed first)
    Say {
        id: String,
        text: String,
        /// Who wrote it, as the thread shows it
        #[arg(long, default_value = "owner")]
        by: String,
        /// A file or image to send with it (repeatable, at most 10)
        #[arg(long)]
        file: Vec<PathBuf>,
    },
    /// Retitle a thread: the conductor's short summary, once after its first reply
    Title { id: String, title: String },
    /// Fold a thread into its repo's archived list; its conductor stops, nothing is deleted
    Archive { id: String },
    /// Bring an archived thread back; its next message resumes the same session
    Restore { id: String },
    /// The conductor's reply to the owner
    Reply {
        id: String,
        text: String,
        /// A file or image to show with it (repeatable)
        #[arg(long)]
        file: Vec<PathBuf>,
    },
    /// The conversation, asks with their options; marks it read
    Show {
        id: String,
        /// Machine-readable output
        #[arg(long)]
        json: bool,
    },
    /// Per repo, its threads: title, last message, unread dot, conductor awake/asleep
    Ls(Scope),
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Propose <file> as the whole new fleet.toml in a thread: validated, its diff a card with [apply] [discard]
    Propose { thread: String, file: PathBuf },
    /// Owner only: apply a proposal (backup kept, doctor run)
    Apply { id: String },
    /// Owner only: discard a proposal
    Discard { id: String },
}

#[derive(Subcommand)]
enum SecretCmd {
    /// Ask the owner for a secret in a thread: a masked card on paired owner devices
    Request {
        thread: String,
        name: String,
        /// What it is for, as the card shows it
        #[arg(long)]
        why: String,
    },
    /// Owner only: the secrets set, by fleet
    Ls,
    /// Owner only: delete a secret
    Rm { fleet: String, name: String },
}

#[derive(Subcommand)]
enum QueueCmd {
    /// Append issues to the queue (or put them on top / after N)
    Add {
        issues: Vec<u64>,
        /// Put them at the top
        #[arg(long, conflicts_with = "after")]
        top: bool,
        /// Put them right after issue N
        #[arg(long, value_name = "N")]
        after: Option<u64>,
        /// They touch serial group G's paths (plain: the first group): its `max` (default 1) in flight at a time
        #[arg(long, value_name = "G", num_args = 0..=1, default_missing_value = "")]
        serial: Option<String>,
        /// Host tags their lanes need besides the fleet's (`docker,gpu`)
        #[arg(long, value_delimiter = ',')]
        needs: Vec<String>,
        /// A design lane first; impl once the owner approves the design
        #[arg(long)]
        design: bool,
        /// Force the verify stage on (plain) or off, whatever the PR touches
        #[arg(long, num_args = 0..=1, default_missing_value = "on", value_parser = ["on", "off"])]
        verify: Option<String>,
        /// A bug fix: impl's first commit is a failing test, rev proves it fails on base and passes at head
        #[arg(long)]
        repro: bool,
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
    /// Replace a queued issue's `needs` host tags, serial groups, design, verify or repro mark (`queue set 12 needs docker,gpu`; `design on|off`)
    Set {
        issue: u64,
        #[arg(value_parser = ["needs", "serial", "design", "verify", "repro"])]
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
enum StatsCmd {
    /// #350: per landing mode, median PASS → landed, landings per day, reverts, check time; red minutes on integration
    Land { fleet: String },
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
    /// Owner only: install herdr, herdsman, git, gh, Claude Code there over SSH, open its logins, clone its fleets
    #[command(hide = true)]
    Bootstrap { name: String },
    /// Owner only: a healthy cloud host saved as its recipe's snapshot; logs `cloud-snapshot`
    #[command(hide = true)]
    Snapshot { name: String },
}

#[derive(Subcommand)]
enum LessonCmd {
    /// The fleet's recent mistakes as plain text: rev FAIL findings, the owner's thread lines and `by: owner` events, reverts
    Digest {
        fleet: String,
        /// The window in days
        #[arg(long, default_value_t = 7)]
        days: u64,
    },
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

/// Line budget of `help --agent` (tests/spawn.rs): raise it here, next to the text it counts (+1 for #130's `--owner-override`, +8 for #151's `extend`/`park`, +1 for #164's `--orphans`, +14 for #157's `hosts`, `queue set`, `--needs`, `--host`, +4 for #319's `done`, +19 for #355's `thread` and `ask --thread`, +1 for #386's `thread reply --file`, +19 for #387's `config` and `secret`, +3 for #413's `thread handoff`, +10 for #405's `thread title`/`archive`/`restore` and `thread say --file`, +9 for #419's `lesson`, +2 for #412's `queue add --design` and the design states, +2 for #417's `queue add --verify` and the verify stage, +1 for #461's conductor `timer install`, +1 for #420's `queue add --repro`, +1 for #344's `migrate --live`, +2 for #478's `kill`/`close --host`).
pub const HELP_AGENT_MAX: usize = 292;

/// The one usage doc both harnesses get: the commands generated from the CLI definition, the rest from `roles/usage-*.md`.
pub fn usage_doc() -> String {
    let mut cmd = Cli::command();
    cmd.build();
    let mut s = String::from(include_str!("../roles/usage-head.md"));
    // The owner's commands; the sections below name them.
    let owner = [
        "accounts", "doctor", "drive", "kick", "pause", "reclaim", "report", "resume", "serve", "service", "setup",
        "stats", "tick", "timer",
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
        Cmd::Hook { .. } | Cmd::LaneEvent { .. } | Cmd::Statusline | Cmd::Migrate { .. } | Cmd::Help { .. }
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
    // #298: `[runtime] kind = "shed"`: lanes start, and are reached, on shed.
    let with = Config::load(&ctx.config)
        .ok()
        .and_then(|c| c.runtime.shed())
        .map(|b| crate::shed::With(r, b));
    let r: &dyn Runner = with.as_ref().map_or(r, |w| w);
    let res = match lane_write.then(|| forward(r, &sent)).flatten() {
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

/// #389: a lane on another host (`HERDSMAN_OUTBOX`) never connects to the main host: its `args` wait in its
/// worktree's outbox, which drive pulls over main → host ssh and runs as the lane.
/// #345: with `HERDSMAN_SERVICE` it goes over HTTP to the main host's service with the lane's own token instead.
pub(crate) fn forward(r: &dyn Runner, args: &[String]) -> Option<Result<runner::Out>> {
    if let (Ok(url), Ok(token)) = (std::env::var("HERDSMAN_SERVICE"), std::env::var("HERDSMAN_TOKEN")) {
        return Some(crate::service::post(r, &url, &token, args));
    }
    let lane = crate::outbox::lane()?;
    Some(crate::outbox::queue(&lane, args).map(|p| runner::Out {
        code: 0,
        stdout: crate::outbox::QUEUED.into(),
        stderr: format!("herdsman: queued in {}\n", p.display()),
    }))
}

/// #389: why a lane's outbox line is not its own to run (`None`: it is): `ask`, `done`, `verdict`, `answer` for its
/// issue, events.
pub(crate) fn foreign(args: &[String], lane: &str, events: &[Event]) -> Option<String> {
    let cli = Cli::try_parse_from(std::iter::once("herdsman").chain(args.iter().map(String::as_str)));
    let is = |e: &&Event, f: &str, id: &str| e.kind == "needs-decision" && e.fleet == f && e.str("id") == Some(id);
    let asked = |f: &str, id: &str| events.iter().rfind(|e| is(e, f, id))?.issue;
    let (f, n) = match cli.map(|c| c.cmd) {
        Err(e) => return Some(format!("unparsable: {}", e.kind())),
        Ok(Cmd::LaneEvent { .. }) => return None,
        Ok(Cmd::Ask { fleet, issue, .. }) => (fleet, issue),
        Ok(Cmd::Done { fleet, issue, .. } | Cmd::Verdict { fleet, issue, .. } | Cmd::Kick { fleet, issue, .. }) => {
            (fleet, issue.to_string())
        }
        Ok(Cmd::Answer { fleet, id, .. }) => (fleet.clone(), asked(&fleet, &id).map_or("-".into(), |n| n.to_string())),
        _ => return Some(format!("`{}` is no lane's own verb", args[0])),
    };
    (!lane.starts_with(&format!("{f}:{n}:"))).then(|| format!("names {f}#{n}, not the lane's {lane}"))
}

/// One command run in process (`herdsman drive`): its exit code, or its error or refusal.
pub(crate) fn run(args: &[&str], ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write) -> Result<i32> {
    let cli = Cli::try_parse_from(std::iter::once("herdsman").chain(args.iter().copied()))?;
    dispatch(cli.cmd, ctx, r, out, &mut std::io::empty())
}

fn dispatch(cmd: Cmd, ctx: &Ctx, r: &dyn Runner, out: &mut dyn Write, stdin: &mut dyn Read) -> Result<i32> {
    match cmd {
        Cmd::Migrate { .. } if std::env::var_os("HERDSMAN_HOME").is_some_and(|v| v.is_empty()) => {
            bail!("HERDSMAN_HOME is set but empty; unset it or point it at a home tree")
        }
        Cmd::Migrate { live: false } if crate::db::live(&ctx.state) => bail!(
            "{} is the live state dir: `herdsman migrate --live` to migrate it",
            ctx.state.parent().unwrap_or(std::path::Path::new(".")).display()
        ),
        Cmd::Migrate { live } => {
            crate::db::LIVE.store(live, std::sync::atomic::Ordering::Relaxed);
            migrate(&ctx.home, out)?;
            // The log where the move above put it: this command's own state, never the environment's.
            let old = ctx.home.join(".local/state/fleet");
            let state = match ctx.state.strip_prefix(&old) {
                Ok(rest) if !old.exists() => ctx.home.join(".local/state/herdsman").join(rest),
                _ => ctx.state.clone(),
            };
            crate::db::migrate(&state, out)?;
        }
        Cmd::Events { .. } => write!(out, "{}", crate::events::jsonl(&ctx.state)?)?,
        Cmd::CapCheck { config, rules } => return crate::overlap::cap_check(&config, &rules, out),
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
        Cmd::LaneEvent { kind, data } => lane_event(ctx, &kind, serde_json::from_str(&data)?),
        Cmd::Status(Scope { fleet, json }) => {
            let (events, all) = (read(&ctx.state)?, crate::events::states(&ctx.state)?);
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
                // #337: the fast step's snapshot (a remote read it lacks fails), and only the fleet asked for.
                let look = crate::snapshot::Snap::read(ctx, r);
                let waits = cfg.map(|mut c| {
                    c.fleet.retain(|k, _| fleet.as_ref().is_none_or(|f| f == k));
                    crate::waiting::scan(
                        ctx,
                        &c,
                        &look,
                        &events,
                        &all_agents(&c, &look).unwrap_or_default(),
                        false,
                    )
                });
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
                        .or_else(|| {
                            let e = events.iter().rev().find(|e| e.is("spawn", &s.fleet, s.issue))?;
                            Some(format!("on {}: {}", e.str("host")?, e.str("why")?))
                        })
                        .unwrap_or_else(|| "-".into());
                    writeln!(out, "{:<pad$}{} {w}", status_line(s, ctx.now), host(s))?;
                }
                write!(out, "{}", serial.concat())?;
                if let Some(t) = look.ts {
                    writeln!(
                        out,
                        "as of the step's look {} ago",
                        crate::events::age(ctx.now.saturating_sub(t))
                    )?;
                }
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
                .map(|f| queue::board(ctx, &events, f, cfg.as_ref(), &crate::overlap::cached(ctx, f)))
                .collect();
            write!(out, "{}", boards.join("\n"))?;
        }
        Cmd::Report { id } => crate::report::show(ctx, out, id.as_deref())?,
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
        } => (SERVE.get().context("no serve")?.1)(ctx, r, &dir, demo)?,
        Cmd::Serve { addr, read_only, .. } => (SERVE.get().context("no serve")?.0)(ctx, r, &addr, read_only)?,
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
            let rows = queue::ls_json(&read(&ctx.state)?, &crate::events::states(&ctx.state)?, &fleet);
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
                if r["design"] == true {
                    on += "  design";
                }
                if r["repro"] == true {
                    on += "  repro";
                }
                if let Some(v) = r["verify"].as_bool() {
                    on += if v { "  verify on" } else { "  verify off" };
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
        Cmd::Service {
            api,
            board,
            install: true,
        } => crate::timer::service_unit(ctx, r, out, &api, board.as_deref())?,
        Cmd::Service { api, board, .. } => (SERVICE.get().context("no service")?)(ctx, &api, board.as_deref())?,
        cmd => {
            let cfg = Config::load(&ctx.config)?;
            let kick = |f: &str, n, why| {
                let _ = crate::drive::kick(ctx, r, f, n, why);
            };
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
                    if role == "conductor" {
                        return gate::refused("BAD_VALUE", "a conductor starts from `herdsman thread say`");
                    }
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
                    spawn::adopt(ctx, &cfg, r, out, (&fleet, issue, &role), &pane, &serde_json::json!({}))?;
                }
                Cmd::Ask {
                    fleet,
                    issue,
                    question,
                    options,
                    recommend: n,
                    kind,
                    thread,
                } => {
                    let at = lane(&issue)?;
                    let how = (n, kind.as_str(), thread.as_deref());
                    crate::ask(ctx, &cfg, out, (&fleet, at), &question, &options, how)?;
                    at.inspect(|n| kick(&fleet, *n, "ask"));
                }
                Cmd::Answer { fleet, id, choice, by } => {
                    crate::answer(ctx, &cfg, r, out, (&fleet, &id), &choice, (&by, None))?;
                }
                Cmd::Check => check::check(ctx, &cfg, r, out)?,
                Cmd::Thread { cmd } => match cmd {
                    // #413: every thread a conductor starts is logged with its source.
                    ThreadCmd::New { .. } if std::env::var("HERDSMAN_THREAD").is_ok_and(|t| !t.is_empty()) => {
                        gate::refused("USE_HANDOFF", "a conductor starts threads with `thread handoff`")?;
                    }
                    ThreadCmd::New { repo, title } => {
                        writeln!(out, "{}", crate::conductor::new(ctx, &cfg, &repo, &title)?)?;
                    }
                    ThreadCmd::Handoff { from, to, title, text } => {
                        let to = crate::conductor::handoff(ctx, &cfg, r, [&from, &to, &title, &text])?;
                        writeln!(out, "{to}")?;
                    }
                    ThreadCmd::Say { id, text, by, file } => {
                        let name = |p: &PathBuf| p.file_name().map_or(String::new(), |n| n.to_string_lossy().into());
                        let load =
                            |p: &PathBuf| Ok((name(p), std::fs::read(p).with_context(|| p.display().to_string())?));
                        let files = file.iter().map(load).collect::<Result<Vec<_>>>()?;
                        crate::conductor::find(&read(&ctx.state)?, &id)?;
                        let files = crate::conductor::upload(ctx, &cfg, &id, &files)?;
                        let agent = crate::conductor::say(ctx, &cfg, r, (&id, &text, &files), (&by, false))?;
                        writeln!(out, "{id}: -> {agent}")?;
                    }
                    ThreadCmd::Title { id, title } => crate::conductor::retitle(ctx, &cfg, &id, &title)?,
                    ThreadCmd::Archive { id } => crate::conductor::archive(ctx, &cfg, r, &id, true)?,
                    ThreadCmd::Restore { id } => crate::conductor::archive(ctx, &cfg, r, &id, false)?,
                    ThreadCmd::Reply { id, text, file } => crate::conductor::reply(ctx, &cfg, &id, &text, &file)?,
                    ThreadCmd::Show { id, json } => crate::conductor::show(ctx, &cfg, out, &id, json)?,
                    ThreadCmd::Ls(Scope { fleet, json }) => crate::conductor::ls(ctx, out, fleet.as_deref(), json)?,
                },
                Cmd::Config { cmd } => {
                    let (id, apply) = match cmd {
                        ConfigCmd::Propose { thread, file } => {
                            return crate::propose::propose(ctx, &cfg, out, &thread, &file).map(|()| 0)
                        }
                        ConfigCmd::Apply { id } => (id, true),
                        ConfigCmd::Discard { id } => (id, false),
                    };
                    writeln!(out, "{}", crate::propose::settle(ctx, &cfg, r, &id, apply)?)?;
                }
                Cmd::Secret { cmd } => match cmd {
                    SecretCmd::Request { thread, name, why } => {
                        crate::secret::request(ctx, &cfg, &thread, &name, &why)?;
                    }
                    SecretCmd::Ls => crate::secret::ls(ctx, out)?,
                    SecretCmd::Rm { fleet, name } => crate::secret::rm(ctx, &cfg, r, &fleet, &name)?,
                },
                Cmd::Hosts { cmd } => match cmd {
                    None => crate::hosts::list(ctx, &cfg, r, out, false)?,
                    Some(HostsCmd::Refresh) => crate::hosts::list(ctx, &cfg, r, out, true)?,
                    Some(HostsCmd::On { name }) => crate::hosts::toggle(ctx, out, &name, true, "cli")?,
                    Some(HostsCmd::Off { name }) => crate::hosts::toggle(ctx, out, &name, false, "cli")?,
                    Some(HostsCmd::Destroy { name }) => {
                        let wip = crate::cloud::stop(ctx, &cfg, r, &name, "destroy", "owner")?;
                        writeln!(out, "{name}: destroyed; wip {}", wip.join(" "))?;
                    }
                    Some(HostsCmd::Bootstrap { name }) => crate::cloud::bootstrap(ctx, &cfg, r, &name, out)?,
                    Some(HostsCmd::Snapshot { name }) => crate::cloud::snapshot(ctx, &cfg, r, &name)?,
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
                        // #445: every open issue, with the reason, nothing-due ones too.
                        .filter(|(s, w)| s.lane.as_ref() == Some(&w.lane) && !s.done() && s.state != "parked")
                        .map(|x| x.1)
                        .filter(|w| fleet.as_ref().is_none_or(|f| *f == w.fleet))
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
                Cmd::Stats {
                    cmd: StatsCmd::Land { fleet },
                } => crate::land::stats(ctx, &cfg, out, &fleet)?,
                Cmd::Clean {
                    fleet,
                    lane,
                    yes,
                    orphans,
                } => {
                    check::clean(ctx, &cfg, r, out, fleet.as_deref(), (None, lane.as_deref()), yes)?;
                    if lane.is_none() {
                        check::orphans(ctx, &cfg, out, orphans && yes)?;
                        check::remote_orphans(&cfg, r, out, orphans && yes)?;
                    }
                }
                Cmd::Reclaim { apply } => check::reclaim(ctx, &cfg, r, out, stdin, apply)?,
                Cmd::Stop { agent } => check::stop(ctx, &cfg, r, out, &agent)?,
                Cmd::Close { agent, host } => check::close(ctx, &cfg, r, out, &agent, host.as_deref())?,
                Cmd::Kill { agent, yes, host } => check::kill(ctx, &cfg, r, out, (&agent, host.as_deref()), yes)?,
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
                    crate::lesson::done(ctx, &cfg, r, out, (&fleet, issue), &no_change)?;
                }
                Cmd::Lesson {
                    cmd: Some(LessonCmd::Digest { fleet, days }),
                    ..
                } => write!(out, "{}", crate::lesson::digest(ctx, &cfg, r, &fleet, days)?)?,
                Cmd::Lesson { fleet, .. } => {
                    let fleet = fleet.context("usage: herdsman lesson <FLEET> | herdsman lesson digest <FLEET>")?;
                    if !crate::lesson::run(ctx, &cfg, r, out, &fleet)? {
                        return Ok(4);
                    }
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
                    let groups = &cfg.fleet(&fleet)?.serial;
                    let known = |g: &String| g.is_empty() || groups.iter().any(|x| x.name == *g);
                    let d = match cmd {
                        QueueCmd::Add {
                            mut issues,
                            top,
                            after,
                            serial,
                            needs,
                            design,
                            verify,
                            repro,
                        } => {
                            // `--serial 12 13`: an issue right after a plain `--serial` (the old flag) is not a group.
                            let g =
                                serial.map(|g| g.parse().map(|n| issues.insert(0, n)).map_or(g, |()| String::new()));
                            anyhow::ensure!(
                                g.iter().all(known),
                                "no serial group `{}` in fleet.{fleet}",
                                g.unwrap_or_default()
                            );
                            let mut d = queue::op("add", &issues, top, None, after, None);
                            for &i in &issues {
                                let _ = gh::spec(ctx, r, cfg.fleet(&fleet)?, &fleet, i, false);
                            }
                            if let Some(g) = g {
                                d["serial"] = if g.is_empty() { json!(true) } else { json!(g) };
                            }
                            if !needs.is_empty() {
                                d["needs"] = json!(needs);
                            }
                            if design {
                                d["design"] = json!(true);
                            }
                            if let Some(v) = verify {
                                d["verify"] = json!(v);
                            }
                            if repro {
                                d["repro"] = json!(true);
                            }
                            d
                        }
                        QueueCmd::Set { issue, key, tags } => {
                            anyhow::ensure!(
                                key != "serial" || tags.iter().all(known),
                                "no such serial group: {tags:?}"
                            );
                            let on = ["on", "off"].iter().any(|o| tags == [*o]);
                            anyhow::ensure!(
                                !["design", "verify", "repro"].contains(&&*key) || on,
                                "{key} is `on` or `off`"
                            );
                            let mut d = queue::op("set", &[issue], false, None, None, None);
                            d[key.as_str()] = json!(tags);
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
                    // #412: a conductor's design mark: the design's approval asks in its thread too.
                    let mut d = d;
                    if let Some(t) = std::env::var("HERDSMAN_THREAD")
                        .ok()
                        .filter(|_| d.get("design").is_some())
                    {
                        d["design_thread"] = json!(t);
                    }
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
    // #298: in a shed terminal (`$SHED`; #455: or berth's `$BERTH`) the hook reports the turn's state, so nothing
    // reads it off the screen.
    let term = std::env::var("SHED").or_else(|_| std::env::var("BERTH"));
    if let (Ok(name), Some(state)) = (term, hook::reported(&input)) {
        let w = r
            .shed()
            .map_or_else(|| crate::shed::Wire::Socket(crate::shed::default_socket()), |b| b.wire);
        let report = shed_proto::Reported {
            state: state.into(),
            note: None,
        };
        if let Err(e) = crate::shed::call(
            r,
            &w,
            shed_proto::Method::State,
            json!(shed_proto::StateParams { name, report }),
        ) {
            eprintln!("herdsman hook: shed state: {e:#}");
        }
    }
    // #304: a turn's end or a push that went through kicks the lane's issue at once.
    if let Some(why) = hook::kick_why(&input) {
        let lane = std::env::var("HERDSMAN_LANE").unwrap_or_default();
        let mut it = lane.split(':');
        if let (Some(f), Some(n)) = (it.next(), it.next().and_then(|n| n.parse().ok())) {
            crate::drive::kick(ctx, r, f, n, why);
        }
    }
    // #413: an issue a handed-off thread's conductor filed goes back to the thread it came from.
    let thread = std::env::var("HERDSMAN_THREAD").unwrap_or_default();
    if let Some(text) = hook::issue_filed(&input).filter(|_| !thread.is_empty()) {
        if let Err(e) = Config::load(&ctx.config).and_then(|cfg| crate::conductor::flow(ctx, &cfg, &thread, &text)) {
            eprintln!("herdsman hook: {e:#}");
        }
    }
    if hook::after(&input) {
        return Ok(());
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
    // #410: a conductor (`HERDSMAN_LANE`'s role) spawns design lanes and writes its own thread folder.
    let conductor = std::env::var("HERDSMAN_LANE").is_ok_and(|l| l.split(':').nth(2) == Some("conductor"));
    let thread = std::env::var("HERDSMAN_THREAD").unwrap_or_default();
    let own = (conductor && !thread.is_empty() && !thread.contains(['/', '.'])).then(|| {
        let cfg = Config::load(&ctx.config).unwrap_or_default();
        crate::conductor::dir(ctx, &cfg, &thread).to_string_lossy().into_owned()
    });
    let policy = hook::Policy {
        branches,
        paths,
        reviewer,
        deny: deny_list(ctx),
        conductor,
        own,
    };
    let cur = |d: &str| {
        runner::ok(r, "git", &["-C", d, "rev-parse", "--abbrev-ref", "HEAD"])
            .ok()
            .map(|s| s.trim().to_string())
    };
    if let Some(reason) = hook::decide(&input, &policy, &cur) {
        if policy.deny.iter().any(|d| d.reason() == reason) {
            lane_event(ctx, "hook-denied", json!({"rule": reason}));
        }
        writeln!(out, "{}", hook::deny_json(&input, &reason))?;
        return Ok(());
    }
    if let Some(dir) = hook::push_dir(&input) {
        coauthor_warn(ctx, r, &dir, "HEAD");
    }
    // #386: a conductor's `herdsman`/`gh` command shows in its thread as a closed tool row.
    if let Some(cmd) = hook::fleet_cmd(&input).filter(|_| !thread.is_empty()) {
        lane_event(ctx, "thread-cmd", json!({"thread": thread, "cmd": cmd}));
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
    lane_event(ctx, "coauthor-trailer", json!({"line": trailer}));
}

/// Logs `kind` for this lane (`HERDSMAN_LANE`'s fleet and issue), its data plus `lane`.
fn lane_event(ctx: &Ctx, kind: &str, mut d: serde_json::Value) {
    if forward(
        &crate::runner::Real,
        &runner::argv(&["lane-event", kind, &d.to_string()]),
    )
    .is_some()
    {
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

/// #216: the lane's fleet's `deny_commands` (`[limits]`'s when the fleet is unknown); #256: its host's rules on top.
fn deny_list(ctx: &Ctx) -> Vec<crate::config::Deny> {
    let cfg = Config::load(&ctx.config).unwrap_or_default();
    let lane = std::env::var("HERDSMAN_LANE").unwrap_or_default();
    let f = cfg.fleet.get(lane.split(':').next().unwrap_or_default());
    let d = f.map_or_else(|| cfg.limits.deny_commands.clone(), |f| f.deny(&cfg.limits));
    let host = std::env::var("HERDSMAN_HOST").unwrap_or_default();
    match cfg.hosts.get(&host) {
        Some(h) => h.deny(d),
        None => d,
    }
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
            lane_event(ctx, "hook-denied", json!({"rule": d.reason()}));
            writeln!(out, "herdsman: a pushed script runs `{}`: {}", d.cmd(), d.reason())?;
            return Ok(1);
        }
        coauthor_warn(ctx, r, ".", sha);
    }
    Ok(0)
}
