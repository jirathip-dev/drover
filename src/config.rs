//! fleet.toml: load, and in-place value edits that preserve every other byte.
use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// #162: the accent tokens a fleet card's bar takes (`--l` blue … `--a` amber); red means "needs you", so it is left out.
pub const COLORS: [&str; 5] = ["l", "p", "c", "g", "a"];

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub fleet: BTreeMap<String, Fleet>,
    pub limits: Limits,
    pub notify: Vec<Notify>,
    pub accounts: Accounts,
    /// MCP server names no lane is given (`*` wildcards, case-insensitive).
    #[serde(default = "mcp_deny")]
    pub mcp_deny: Vec<String>,
    /// The models `herdsman serve` offers per role (its model dropdown).
    pub models: Vec<String>,
    pub serve: Serve,
    pub providers: Providers,
    /// #157: `[hosts.<name>]` lanes may run on (docs/hosts.md); none = this host only, as before.
    pub hosts: BTreeMap<String, HostCfg>,
}

/// #157: one host: its herdr machine label (unset = this host), hand tags, worktree root, and whether new lanes go there.
#[derive(Debug, Deserialize)]
pub struct HostCfg {
    pub machine: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub root: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

/// `[providers]`: API providers whose balance the Host tab shows (docs/providers.md).
#[derive(Debug, Default, Deserialize)]
pub struct Providers {
    pub deepseek: Option<Provider>,
}

/// The key is read from `env_file` (else `[accounts].accounts_env`, else the environment), never from fleet.toml.
#[derive(Debug, Deserialize)]
pub struct Provider {
    pub key_env: String,
    pub env_file: Option<String>,
    #[serde(default = "warn_below")]
    pub warn_below: f64,
}

fn warn_below() -> f64 {
    5.0
}

fn mcp_deny() -> Vec<String> {
    vec!["*prod*".into()]
}

/// `[serve]`: extra `Host` names (e.g. the `*.ts.net` name of `tailscale serve`) a POST may come from,
/// and `approve_hint`, appended to the pairing panel's Mac line (`{code}` substituted).
#[derive(Debug, Default, Deserialize)]
pub struct Serve {
    #[serde(default)]
    pub allowed_hosts: Vec<String>,
    pub approve_hint: Option<String>,
    /// #98 `herdsman setup`: the board job's label (default `dev.herdsman.serve`), address, `tailscale serve` port.
    pub launchd_label: Option<String>,
    pub addr: Option<String>,
    pub tailscale_https_port: Option<u16>,
    /// #172: the Herd's theme, `<config dir>/themes/<name>/` or built-in (default `farm`).
    pub theme: Option<String>,
}

impl Serve {
    pub fn addr(&self) -> String {
        self.addr.clone().unwrap_or_else(|| "127.0.0.1:7777".into())
    }
}

/// `[accounts]`: account name -> env var holding its token, plus the settings below.
#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Accounts {
    /// Env file (`VAR=value` lines) the tokens are read from.
    pub accounts_env: Option<String>,
    /// Usage-limit banner, `|`-separated substrings (default `access::CAP_PATTERN`).
    pub cap_pattern: Option<String>,
    pub cap_reset_minutes: u64,
    /// `[accounts.usage]`: account name -> the `CLAUDE_CONFIG_DIR` of its own `claude` login, read only for the usage meter.
    pub usage: BTreeMap<String, String>,
    #[serde(flatten)]
    pub names: BTreeMap<String, String>,
}

impl Default for Accounts {
    fn default() -> Self {
        Accounts {
            accounts_env: None,
            cap_pattern: None,
            cap_reset_minutes: 300,
            usage: BTreeMap::new(),
            names: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Fleet {
    pub repo: String,
    pub local: String,
    pub integration: String,
    /// Globs only `serial_max` lanes at a time may touch: `--serial` issues (`SERIAL_BUSY`, `SERIAL_UNMARKED`).
    #[serde(default)]
    pub serial_paths: Vec<String>,
    /// How many `--serial` issues may be in flight at once (`SERIAL_BUSY`); unset = 1.
    pub serial_max: Option<usize>,
    /// #115: the Queue graph's lock mark for `--serial` issues, at most 8 characters; unset = `serial`.
    pub serial_label: Option<String>,
    /// The only checks merge, `check` and `wait` look at (latest run of each); empty = all.
    #[serde(default)]
    pub required_checks: Vec<String>,
    /// #150: a label the PR needs for its full CI (e.g. `merge-ready`), kept on it exactly while a PASS stands at its
    /// head; unset = none.
    pub merge_label: Option<String>,
    /// Optional caps on concurrent impl/fix and review lanes (CAP); unset = no cap.
    pub max_impl: Option<usize>,
    pub max_rev: Option<usize>,
    #[serde(default)]
    pub role: BTreeMap<String, Role>,
    /// Replace the global `[[notify]]` for this fleet's events; `notify_global` sends to both.
    #[serde(default)]
    pub notify: Vec<Notify>,
    #[serde(default)]
    pub notify_global: bool,
    /// `herdsman pause`: `next` offers nothing and impl/rev spawns refuse PAUSED.
    #[serde(default)]
    pub paused: bool,
    /// #103: exported into every lane (`~/` expands).
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// #185: build folders a new lane APFS-clones from `local` on a lockfile match; `clone_node_modules` is its alias for one release.
    #[serde(default)]
    pub clone_build: Vec<String>,
    #[serde(default)]
    pub clone_node_modules: bool,
    /// #187: lanes' `HERDSMAN_PORT` range, `lo-hi`; unset = `41000-41999`.
    pub port_range: Option<String>,
    /// #164: build folders freed besides `target`, `node_modules`, `.next` and `dist` (paths in the worktree).
    #[serde(default)]
    pub build_dirs: Vec<String>,
    /// #173: `[fleet.<n>.pipeline]`; `Config::parse` fills in the built-in one when absent.
    pub pipeline: Option<crate::pipeline::Pipeline>,
    /// #173: the pipeline's broken rules: every command but `serve`/`doctor`/`status` refuses `BAD_PIPELINE`.
    #[serde(skip)]
    pub bad: Vec<crate::pipeline::Problem>,
    /// #162: the Queue card's colour, one of `COLORS`; unset = a stable hash of the fleet name.
    pub color: Option<String>,
    /// #157: host tags every lane of this fleet needs (e.g. `xcode`).
    #[serde(default)]
    pub needs: Vec<String>,
    /// #228: this fleet's form categories and form rounds; `Config::parse` fills in `[limits]`'s when absent.
    pub form_categories: Option<Vec<String>>,
    pub form_rounds: Option<u32>,
    /// #216: replaces `[limits] deny_commands` (empty = off); `deny_commands_add` adds to whichever applies.
    pub deny_commands: Option<Vec<Deny>>,
    #[serde(default)]
    pub deny_commands_add: Vec<Deny>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Role {
    pub harness: String,
    pub profile: Option<String>,
    pub model: Option<String>,
    /// Reasoning effort, mapped per harness (`harness::effort_ok`); `[limits].effort` when unset.
    pub effort: Option<String>,
    /// Claude lanes: the account (`[accounts]` name) to run on, or an ordered fallback list.
    pub account: Option<String>,
    #[serde(default)]
    pub accounts: Vec<String>,
    /// MCP servers (by `.mcp.json` name) the lane gets; none by default.
    #[serde(default)]
    pub mcp: Vec<String>,
    /// #173: a file appended to the role's kind prompt (`roles/impl.md` for a build, `roles/rev.md` for a check).
    pub prompt: Option<String>,
    /// #157: host tags this role's lanes need besides the fleet's.
    #[serde(default)]
    pub needs: Vec<String>,
    /// #236: the host its lanes always run on (`main`: the one without `machine`); `design` defaults to `main`.
    pub host: Option<String>,
}

impl Role {
    pub fn account_list(&self) -> Vec<String> {
        if self.accounts.is_empty() {
            self.account.iter().cloned().collect()
        } else {
            self.accounts.clone()
        }
    }
    pub fn profile_or(&self, role: &str) -> String {
        self.profile.clone().unwrap_or_else(|| format!("fleet-{role}"))
    }
    pub fn effort_or(&self, limits: &Limits) -> Option<String> {
        self.effort.clone().or_else(|| limits.effort.clone())
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Limits {
    pub review_rounds: u32,
    pub update_rounds: u32,
    pub stall_minutes: u64,
    /// Host-load gate for `herdsman spawn` (`HOST_BUSY`).
    pub load_per_cpu: f64,
    /// Pages swapped out per 2 s sample.
    pub swapout_rate: f64,
    /// `herdsman spawn` refuses `DISK_LOW` under this many GB free on `/`.
    pub min_free_gb: f64,
    /// #164: over this many GB of worktrees drive frees the idlest lanes' build folders, then spawn refuses `DISK_BUDGET`.
    pub worktree_budget_gb: f64,
    /// #168: `herdsman spawn` refuses `MEM_LOW` under this many GB of free + inactive memory.
    pub min_free_mem_gb: f64,
    /// Default reasoning effort for roles that set none.
    pub effort: Option<String>,
    /// #99: a routine ask unanswered this long takes its recommended option.
    pub ask_default_minutes: u64,
    /// #223: a lane `done`/idle this long loses the processes its agent started (tsserver, vite, ...).
    pub idle_reap_minutes: u64,
    /// #248: a `done` lane whose handoff is logged this long closes its session (its worktree stays).
    pub done_close_minutes: u64,
    /// #248: the lane settings' `cleanupPeriodDays` (Claude's own transcript retention).
    pub transcript_days: u64,
    /// #216: commands the lane hook denies; `[fleet.<n>] deny_commands` replaces it, `deny_commands_add` extends it.
    pub deny_commands: Vec<Deny>,
    /// #212: the fast step (`drive --fast`) and the slow tick run every this many seconds.
    pub fast_secs: u64,
    pub slow_secs: u64,
    /// #228: the finding tags a `FAIL (form-only)` may carry, and how many per issue don't count as a review round.
    pub form_categories: Vec<String>,
    pub form_rounds: u32,
}

/// #216: a `deny_commands` entry: a command word or command + subcommand prefix, bare or `{ cmd, why }`.
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum Deny {
    Cmd(String),
    Why { cmd: String, why: String },
}

impl Deny {
    pub fn cmd(&self) -> &str {
        match self {
            Deny::Cmd(c) | Deny::Why { cmd: c, .. } => c,
        }
    }
    /// The hook's message: `why` verbatim, else `REFUSED NO_COMMAND`.
    pub fn reason(&self) -> String {
        match self {
            Deny::Cmd(c) => format!("REFUSED NO_COMMAND: lanes never run `{c}` (deny_commands); ask the owner"),
            Deny::Why { why, .. } => why.clone(),
        }
    }
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            review_rounds: 3,
            update_rounds: 3,
            stall_minutes: 30,
            load_per_cpu: 2.5,
            swapout_rate: 2000.0,
            min_free_gb: 10.0,
            worktree_budget_gb: 25.0,
            min_free_mem_gb: 2.0,
            effort: None,
            ask_default_minutes: 30,
            idle_reap_minutes: 10,
            done_close_minutes: 5,
            transcript_days: 14,
            // #216: lanes never start containers (owner rule); the list is assets/deny_commands.toml.
            deny_commands: toml::from_str::<toml::Table>(include_str!("../assets/deny_commands.toml"))
                .ok()
                .and_then(|mut t| t.remove("deny_commands")?.try_into().ok())
                .unwrap_or_default(),
            fast_secs: 60,
            slow_secs: 300,
            form_categories: [
                "trailer",
                "pr-body",
                "stale-baseline",
                "format",
                "docs-fresh",
                "migration-redate",
                "infra",
            ]
            .map(String::from)
            .into(),
            form_rounds: 3,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Notify {
    #[serde(default)]
    pub cmd: String,
    #[serde(default)]
    pub on: Vec<String>,
    /// "json" sends the raw event; anything else one plain line.
    #[serde(default)]
    pub format: Option<String>,
    /// "webpush": push to the page's subscribed devices instead of running `cmd`.
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    /// webpush: the VAPID `sub` claim (`mailto:` or `https:`).
    #[serde(default)]
    pub sub: Option<String>,
}

/// A webpush notifier's events when `on` is unset: the Telegram set plus the asks (#40) and needs-human (#130).
pub const WEBPUSH_ON: &[&str] = &[
    "blocked",
    "stall",
    "lane-dead",
    "ready-unmerged",
    "merged",
    "needs-decision",
    "needs-human",
    "ask-waiting",
    "decision-undelivered",
    "state-write-failed",
    "wip-blocked",
];

impl Notify {
    pub fn webpush(&self) -> bool {
        self.kind.as_deref() == Some("webpush")
    }
    pub fn wants(&self, kind: &str) -> bool {
        self.on.iter().any(|k| k == kind) || (self.on.is_empty() && self.webpush() && WEBPUSH_ON.contains(&kind))
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path).with_context(|| format!("cannot read config {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("invalid config {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Config> {
        let c: Config = toml::from_str(text)?;
        let raw: toml::Table = toml::from_str(text)?;
        if let Some((k, _)) = strings("", &toml::Value::Table(raw.clone()))
            .into_iter()
            .find(|(_, v)| secret_like(v))
        {
            return crate::gate::refused(
                "SECRET_IN_CONFIG",
                format!("{k} looks like a secret; keep keys in an env file (docs/providers.md)"),
            );
        }
        if raw.get("limits").and_then(|l| l.get("max_agents")).is_some() {
            eprintln!("herdsman: [limits].max_agents is no longer used; ignoring it");
        }
        let levels = crate::harness::EFFORTS;
        if let Some(e) = c.limits.effort.as_deref().filter(|e| !levels.contains(e)) {
            bail!("limits.effort: \"{e}\" must be one of {levels:?}");
        }
        if c.notify
            .iter()
            .chain(c.fleet.values().flat_map(|f| &f.notify))
            .any(|n| n.webpush() && n.sub.as_deref().is_none_or(str::is_empty))
        {
            bail!("notify: a type = \"webpush\" notifier needs sub = \"mailto:you@example.com\"");
        }
        let mut c = c;
        for (name, f) in &mut c.fleet {
            let mut p = f
                .pipeline
                .take()
                .unwrap_or_else(|| crate::pipeline::Pipeline::builtin(&c.limits));
            // Each stage's line is its `[[fleet.<n>.pipeline.stage]]` header's, in order.
            let head = format!("[[fleet.{name}.pipeline.stage]]");
            let lines = text
                .lines()
                .enumerate()
                .filter(|l| l.1.split_whitespace().collect::<String>() == head);
            for (s, (i, _)) in p.stage.iter_mut().zip(lines) {
                s.line = i + 1;
            }
            // A gate without its own rounds takes `[limits] update_rounds`.
            for g in p.stage.iter_mut().filter(|s| s.kind == "gate" && s.rounds.is_none()) {
                g.rounds = Some(c.limits.update_rounds);
            }
            f.form_categories
                .get_or_insert_with(|| c.limits.form_categories.clone());
            f.form_rounds.get_or_insert(c.limits.form_rounds);
            f.bad = p.validate(name, &f.role);
            f.pipeline = Some(p);
        }
        for (name, f) in &c.fleet {
            if !(1..=8).contains(&f.serial_label().chars().count()) {
                bail!("fleet.{name}.serial_label: must be 1 to 8 characters");
            }
            if f.deny_commands.as_ref().is_some_and(Vec::is_empty) {
                eprintln!("herdsman: fleet.{name}.deny_commands is empty: the lane hook denies no commands");
            }
            if let Some(k) = f.color.as_deref().filter(|k| !COLORS.contains(k)) {
                bail!("fleet.{name}.color: \"{k}\" must be one of {COLORS:?}");
            }
            if f.notify.iter().any(|n| !n.webpush() && n.cmd.trim().is_empty()) {
                bail!("fleet.{name}.notify: cmd must not be empty");
            }
            for (role, r) in &f.role {
                if role == "orch" || role.is_empty() || !role.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                    bail!("fleet.{name}.role.{role}: a role name is letters, digits and `-`, not orch");
                }
                if !crate::harness::NAMES.contains(&r.harness.as_str()) {
                    bail!(
                        "fleet.{name}.role.{role}: harness must be one of {:?}",
                        crate::harness::NAMES
                    );
                }
                if let Some(a) = r.account_list().iter().find(|a| !c.accounts.names.contains_key(*a)) {
                    bail!("fleet.{name}.role.{role}: account '{a}' is not in [accounts]");
                }
                if let Some(e) = r.effort_or(&c.limits) {
                    let ok = crate::harness::efforts(&r.harness);
                    if !levels.contains(&e.as_str()) {
                        bail!("fleet.{name}.role.{role}: effort \"{e}\" must be one of {levels:?}");
                    }
                    if !ok.contains(&e.as_str()) {
                        bail!(
                            "fleet.{name}.role.{role}: harness {} does not support effort \"{e}\" (supported: {ok:?})",
                            r.harness
                        );
                    }
                }
            }
        }
        Ok(c)
    }

    /// Notifiers for `fleet`'s events: its own if any (plus the global ones with `notify_global`), else the global.
    pub fn notifiers(&self, fleet: &str) -> Vec<&Notify> {
        match self.fleet.get(fleet).filter(|f| !f.notify.is_empty()) {
            Some(f) if f.notify_global => f.notify.iter().chain(&self.notify).collect(),
            Some(f) => f.notify.iter().collect(),
            None => self.notify.iter().collect(),
        }
    }

    /// The first webpush notifier's VAPID `sub`: `None` means the page's alerts are saved but never sent (#87).
    pub fn webpush_sub(&self) -> Option<&str> {
        let all = self.notify.iter().chain(self.fleet.values().flat_map(|f| &f.notify));
        all.filter(|n| n.webpush()).find_map(|n| n.sub.as_deref())
    }

    /// The fleet, refused `BAD_PIPELINE` while its pipeline breaks a rule (#173: that fleet stops, the others run).
    pub fn fleet(&self, name: &str) -> Result<&Fleet> {
        let f = self.fleet.get(name).ok_or_else(|| anyhow!("unknown fleet '{name}'"))?;
        match f.bad.as_slice() {
            [] => Ok(f),
            bad => {
                let rules: Vec<String> = bad.iter().map(|p| format!("\n  {:<12} {}", p.rule, p.msg)).collect();
                let n = bad.len();
                let why = format!(
                    "fleet {name}: {n} error(s) in [fleet.{name}.pipeline]{}",
                    rules.concat()
                );
                crate::gate::refused("BAD_PIPELINE", why)
            }
        }
    }
}

impl Fleet {
    /// #173: the fleet's pipeline (the built-in one unless fleet.toml has its own).
    pub fn flow(&self) -> &crate::pipeline::Pipeline {
        self.pipeline
            .as_ref()
            .expect("Config::parse resolves every fleet's pipeline")
    }
    /// The prompt family a role's lane gets: `rev` for a check role, `design`, else `impl` (a build).
    pub fn family(&self, role: &str) -> &'static str {
        match role {
            "design" => "design",
            r if self.flow().is_check(r) => "rev",
            _ => "impl",
        }
    }
    /// #162: the card colour: `color`, else `sha256(name)[0] % 5` over `COLORS` (the horse coats' hash).
    pub fn color(&self, name: &str) -> &str {
        self.color.as_deref().unwrap_or_else(|| {
            let h = crate::devices::sha256(name.as_bytes());
            COLORS[usize::from_str_radix(&h[..2], 16).unwrap_or(0) % COLORS.len()]
        })
    }
    /// #216: the lane hook's deny list: `deny_commands`, else `[limits]`'s, plus `deny_commands_add`.
    pub fn deny(&self, limits: &Limits) -> Vec<Deny> {
        let mut d = self
            .deny_commands
            .clone()
            .unwrap_or_else(|| limits.deny_commands.clone());
        d.extend(self.deny_commands_add.iter().cloned());
        d
    }
    pub fn serial_label(&self) -> &str {
        self.serial_label.as_deref().unwrap_or("serial")
    }
    pub fn role(&self, role: &str) -> Result<&Role> {
        self.role
            .get(role)
            .ok_or_else(|| anyhow!("role '{role}' not configured"))
    }
    pub fn local_path(&self) -> PathBuf {
        expand(&self.local)
    }
    pub fn repo_name(&self) -> &str {
        self.repo.rsplit('/').next().unwrap_or(&self.repo)
    }
    /// `path` matches a `serial_paths` glob.
    pub fn serial_path(&self, path: &str) -> bool {
        self.serial_paths.iter().any(|g| glob(g.as_bytes(), path.as_bytes()))
    }
}

/// A value shaped like a secret: a known token prefix, a JWT, or a 32+ char letter-and-digit run between `.`/`/`/`-`/`_`
/// (so `UPPER_SNAKE` env-var names, paths and model ids pass).
pub fn secret_like(v: &str) -> bool {
    let words = v
        .split(|c: char| !(c.is_ascii_alphanumeric() || "-_+/=.~".contains(c)))
        .filter(|w| !w.is_empty());
    let b64 = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || "-_=".contains(c));
    let long =
        |p: &str| p.len() >= 32 && p.chars().any(|c| c.is_ascii_digit()) && p.chars().any(|c| c.is_ascii_alphabetic());
    words.into_iter().any(|w| {
        let snake = w
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        crate::lanes::PREFIXES
            .iter()
            .any(|p| w.starts_with(p) && w.len() >= p.len() + 10)
            || (w.starts_with("eyJ") && w.split('.').count() == 3 && w.split('.').all(b64))
            || (!snake && w.split(['.', '/', '-', '_']).any(long))
    })
}

/// Every string value under `v`, with its dotted key.
fn strings(key: &str, v: &toml::Value) -> Vec<(String, String)> {
    let sub = |k: String, v| strings(&k, v);
    match v {
        toml::Value::String(s) => vec![(key.into(), s.clone())],
        toml::Value::Table(t) => t
            .iter()
            .flat_map(|(k, v)| sub(format!("{key}{}{k}", if key.is_empty() { "" } else { "." }), v))
            .collect(),
        toml::Value::Array(a) => a
            .iter()
            .enumerate()
            .flat_map(|(i, v)| sub(format!("{key}[{i}]"), v))
            .collect(),
        _ => vec![],
    }
}

/// `**` matches anything, `*` anything but `/`, `?` one char other than `/`.
pub fn glob(g: &[u8], s: &[u8]) -> bool {
    match g {
        [] => s.is_empty(),
        [b'*', b'*', rest @ ..] => (0..=s.len()).any(|i| glob(rest, &s[i..])),
        [b'*', rest @ ..] => (0..=s.len())
            .take_while(|&i| i == 0 || s[i - 1] != b'/')
            .any(|i| glob(rest, &s[i..])),
        [b'?', rest @ ..] => s.first().is_some_and(|&c| c != b'/') && glob(rest, &s[1..]),
        [c, rest @ ..] => s.first() == Some(c) && glob(rest, &s[1..]),
    }
}

pub fn expand(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None => PathBuf::from(p),
    }
}

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

/// Set `key = "val"` in `[fleet.<fleet>.role.<role>]`, touching only the value bytes (or adding one line).
pub fn set_value(text: &str, fleet: &str, role: &str, key: &str, val: &str) -> Result<String> {
    set_in(
        text,
        &format!("[fleet.{fleet}.role.{role}]"),
        key,
        &format!("\"{val}\""),
    )
}

/// Set `key = <lit>` (a TOML literal) in the table `header`, as `set_value` does.
pub fn set_in(text: &str, header: &str, key: &str, lit: &str) -> Result<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let start = lines
        .iter()
        .position(|l| l.split_whitespace().collect::<String>() == header);
    let start = start.ok_or_else(|| anyhow!("no {header} table in fleet.toml; add it first"))?;
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map_or(lines.len(), |i| start + 1 + i);
    let mut out: Vec<String> = lines.iter().map(std::string::ToString::to_string).collect();
    let hit = (start + 1..end).find(|&i| {
        let t = lines[i].trim_start();
        t.strip_prefix(key).is_some_and(|r| r.trim_start().starts_with('='))
    });
    if let Some(i) = hit {
        let l = lines[i];
        let eq = l.find('=').unwrap() + 1;
        let v = eq + l[eq..].len() - l[eq..].trim_start().len();
        let end = if l[v..].starts_with('"') {
            l[v + 1..].find('"').map(|c| v + c + 2)
        } else if lit.starts_with('"') {
            None
        } else {
            Some(
                v + l[v..]
                    .find(|c: char| c.is_whitespace() || c == '#')
                    .unwrap_or(l.len() - v),
            )
        };
        let Some(end) = end else {
            bail!("{header} {key}: value must be a \"string\"");
        };
        out[i] = format!("{}{lit}{}", &l[..v], &l[end..]);
    } else {
        let last = (start..end).rev().find(|&i| !lines[i].trim().is_empty()).unwrap();
        if !out[last].ends_with('\n') {
            out[last].push('\n');
        }
        out.insert(last + 1, format!("{key} = {lit}\n"));
    }
    Ok(out.concat())
}

pub fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
