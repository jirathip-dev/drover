# Providers: data shapes (#806)

All of it lives in `crates/core/src/providers/mod.rs`. Existing method names stay where tests call them
(`login`, `credential`, `usage_meter`→`usage`, see the rename table); defaults are today's Generic behaviour.

## Registry

```rust
/// One line per provider: declares `mod <name>;` and lists `<name>::PROVIDER` under that name, in this order.
/// Order is behaviour (first-match readers: `tool_line`, `pending_menu`, `transcript`, `accounts()`): new ones append.
macro_rules! registry { ($($(#[$m:meta])* $n:ident),* $(,)?) => {
    $($(#[$m])* mod $n;)*
    static ALL: &[Entry] = &[$($(#[$m])* Entry { name: stringify!($n), provider: &$n::PROVIDER }),*];
} }
registry! { claude, hermes, dsh, codex, gemini, #[cfg(test)] fake }

/// A registry entry: its name (the registry's ident, written once) and the provider; derefs to the provider.
#[derive(Clone, Copy)]
pub struct Entry { pub name: &'static str, pub provider: &'static dyn Provider }
impl Deref for Entry { type Target = dyn Provider; /* … */ }

pub fn all() -> impl Iterator<Item = Entry>;                          // registry order
pub fn lanes() -> impl Iterator<Item = Entry>;                        // Surface::Terminal (was NAMES / all())
pub fn acp_only() -> impl Iterator<Item = Entry>;                     // Surface::Acp (was ACP_ONLY)
pub fn lane_names() -> Vec<&'static str>;                             // was `NAMES` const
pub fn known() -> impl Iterator<Item = &'static str>;                 // lanes then acp_only, as today
pub fn get(name: &str) -> &'static dyn Provider;                      // Terminal|Acp by name, else Generic
pub fn accounts() -> Option<&'static dyn Provider>;                   // first lane on Credential::Account
pub fn proxied() -> Option<&'static Proxy>;                           // first provider with a proxy
pub fn vendor_of_agent(acp_id: &str) -> Option<&'static str>;         // was PROVIDERS + provider()
pub fn of_agent(acp_id: &str) -> Option<&'static str>;                // guarding provider: Terminal|Acp only
pub fn brands() -> impl Iterator<Item = &'static str>;                // #198 co-author names
```

## The trait

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    Terminal, // a lane harness: offered as `harness`, runs in a herdr pane (claude, hermes, dsh)
    Acp,      // reached only through its ACP agent (codex)
    Listed,   // known only by its ACP agent's vendor, for the impl≠rev vendor rule (gemini); `get` never returns it
}

pub trait Provider: Sync {
    fn surface(&self) -> Surface { Surface::Terminal }
    fn vendor(&self) -> Option<&'static str> { None }        // "anthropic", "openai", "google"
    fn brands(&self) -> &'static [&'static str] { &[] }      // names in an AI co-author trailer

    // login
    fn login(&self) -> Option<&'static str> { None }
    fn install(&self) -> Option<&'static str> { None }
    fn health_check(&self) -> Option<(&'static str, &'static str, &'static str)> { None }
    fn signed_in(&self, r: &dyn Runner, dir: &Path) -> Option<bool> { None }
    fn signed_out(&self, screen: &str) -> bool { false }

    // accounts
    fn credential(&self) -> Credential { Credential::None }
    fn proxy(&self) -> Option<&'static Proxy> { None }

    // usage / limits
    fn usage(&self) -> Option<&'static dyn Usage> { None }
    fn usage_limited(&self, cfg: &Config, r: &dyn Runner, screen: &str, now: u64) -> Option<u64> { None }

    // models
    fn efforts(&self) -> &'static [&'static str] { EFFORTS }
    fn takes_model(&self) -> bool { false }
    fn model_default(&self) -> Option<&'static str> { None }   // None: the harness name; Hermes: "profile"

    // profile + launch + prompt (unchanged signatures)
    fn sync(..) -> Result<()>; fn ready(..); fn prepare(..); fn start_cmd(..); fn resumes(..);
    fn kick_cmds(..); fn kick(..); fn env(..); fn acp(..); fn acp_env(..); fn acp_profile(..);
    fn hook_dialect(..); fn startup_title(..); fn startup_prompt(..); fn process_names(..);
    fn home(&self) -> Option<Home> { None }                       // its own dir under ~, env-overridable
    fn profile_files(&self, ctx: &Ctx, profile: &str) -> Vec<PathBuf> { vec![] }   // timer fingerprint
    fn hooks_allowed(&self, ctx, cfg, r, out) -> Result<bool> { Ok(true) }          // profiles sync --check
    fn doctor(&self, ctx: &Ctx, cfg: &Config, r: &dyn Runner) -> Vec<Row> { vec![] }
    fn scrubs_env(&self, var: &str) -> bool { false }             // notifier env

    // screen parsing (unchanged signatures)
    fn status_word(..); fn now_line(..); fn tool_line(..); fn pending_menu(..); fn answer_keys(..); fn transcript(..);
}
```

## Accounts, proxy, usage

```rust
pub enum Credential {
    None,
    Own,                    // the harness's own login on the host
    Account(Accounts),      // signs in with an `[accounts]` account
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Accounts {
    pub provider: &'static str,          // `{provider}` in account_env ("claude", "codex": unchanged values)
    pub config_env: &'static str,        // its config-folder env (was CONFIG_ENV)
    pub token_env: Option<&'static str>, // `token-file:` delivery and the shell override (was token_env())
}

/// #637: what the token proxy needs from the provider it fronts.
pub struct Proxy {
    pub upstream: &'static str,        // `[proxy] upstream` default
    pub base_url_env: &'static str,    // where an ACP adapter reads the proxy URL
    pub auth_token_env: &'static str,  // where it reads the lane key
    pub headers: fn(Vec<(String, String)>) -> Vec<(String, String)>, // forwarded headers, reshaped (beta merge)
}

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Window { pub used: Option<f64>, pub resets: Option<u64> }   // was Part = (Option<f64>, Option<u64>)

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Limits { pub session: Window, pub week: Window }           // was (Part, Part) = (5 h, 7 d)

pub trait Usage: Sync {                    // was UsageMeter
    fn reading(&self, status_line: &Value) -> Limits;   // was status()
    fn cap_banner(&self) -> &'static str;               // was CAP_PATTERN
}

pub struct Home { pub env: &'static str, pub default: &'static str }  // ("FLEET_HERMES_PROFILES", ".hermes/profiles")
```

`Ctx.hermes: PathBuf` becomes `Ctx.homes: BTreeMap<&'static str, PathBuf>` (filled from each `home()`); a provider
reads its own with `ctx.homes["hermes"]` (inside its own module, the one place that may say it).

## Rename table (tests follow these renames only)

| today | after |
|---|---|
| `crate::harness` | `crate::providers` |
| `trait Harness` | `trait Provider` |
| `NAMES`, `ACP_ONLY`, `PROVIDERS`, `provider()` | `lane_names()`, `acp_only()`, `vendor_of_agent()` |
| `CONFIG_ENV`, `Credential::Account(&str)`, `token_env()` | `Credential::Account(Accounts)` |
| `PROXY_UPSTREAM`, `BETA_HEADER`, `OAUTH_BETA`, `BASE_URL_ENV`, `AUTH_TOKEN_ENV` | `Proxy` (claude.rs private consts) |
| `CAP_PATTERN`, `UsageMeter::status`, `Part`, `usage_meter()` | `Usage::cap_banner`, `Usage::reading`, `Window`/`Limits`, `usage()` |
| `efforts(harness)` free fn | `get(h).efforts()` |
| `harness/claude_meter.rs` | `providers/claude/meter.rs` (a submodule of claude) |
| `Ctx.hermes` | `Ctx.homes` |
