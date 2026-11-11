# Providers and keys

herdsman supports two model providers: Anthropic (Claude Code lanes) and DeepSeek (dsh lanes, and
Hermes lanes that use DeepSeek).

**No secrets in fleet.toml, ever.** fleet.toml holds only the *names* of env variables. herdsman
checks every fleet.toml value on load. A value shaped like a token fails with `SECRET_IN_CONFIG`
(exit 3), and the refusal names the key, never the value. Token-shaped means a known key prefix
(`sk-`, GitHub and Slack token prefixes: the ones the page masks in pane text), a JWT, or a run of 32+ letters and
digits with no `.`/`/`/`-`/`_` in it. Env-var names (`CLAUDE_CODE_OAUTH_TOKEN_ACCOUNT_A1_PERSONAL`),
paths and model ids pass.

## Anthropic (Claude Code lanes)

**No store (#707).** herdsman keeps no secret store and makes no Keychain read. Each secret is a file on the main host
only its owner reads (mode 0600; a looser one is refused `SECRET_FILE_OPEN`): a `secret:<NAME>` env is
`<config dir>/secrets/<NAME>`; the webhook secret, a provider key's `env_file` and an account's `token-file:` are the
files fleet.toml names by path. Place one by hand (`umask 077`), or answer `herdsman secret request`'s masked card from a
paired device, which writes `<config dir>/secrets/<NAME>`; `herdsman secret rm` deletes it. Remote hosts keep no
secrets.

**Lane accounts (#683).** herdsman keeps no Claude token. Each `[accounts]` key names an account; a lane on it signs in
with `account_env`'s printed env, else the account's own login folder as `CLAUDE_CONFIG_DIR`
([accounts](accounts.md)):

```toml
[accounts]
# account_env = "cooee accounts env {provider} {name}"
A = "A"   # the value is unused, unless `token-file:<path>` (accounts.md)
B = "B"
```

A role picks with `account = "A"` or an ordered `accounts = ["A", "B"]`.

**Delivery.** At spawn herdsman reads the lane's `secret:<NAME>` env from its file and writes it to a 0600 file beside
the worktree on the lane's host: directly on the main host, over the ssh channel's stdin on a remote one. The pane's
shell sources the file; spawn deletes it once the harness has started. A secret never reaches argv, the pane's
scrollback, the event log or a file left behind. `accounts_env` is retired (#441; a `!` row while set) and
`accounts import` with it (#683).

**Fallback and caps.** `herdsman check` reads each Claude lane's pane. When it shows the usage-limit
banner, it logs `account-capped` with `until`, the banner's reset time (`resets 9am`, `resets Nov 4, 8am`,
host local time; unparseable: `cap_reset_minutes`, default 300, or a day for a weekly limit). Spawns skip
that account until then, or until `herdsman accounts reset <name>`; with every account capped, spawn
refuses `ALL_CAPPED` (drive stops starting for the pass). The capped lane waits on drive, which
respawns it with `--supersede` on a free account (the kill pushes its work to `wip/` first).

**Usage meter from the lanes.** Claude lanes run `herdsman statusline` as their status line. Claude Code hands
it the account's 5 h / 7 d limits on every refresh; it appends at most one row per account per minute to
`<state>/usage.jsonl` (the account from `HERDSMAN_ACCOUNT`, set at spawn) and prints `myapp #12 · 5h 41% · 7d 63%`
in the pane. The meter shows an account's newest row, with its age. It is the only source (#707: herdsman no longer
polls `/api/oauth/usage`, which read Claude Code's Keychain item): an account with no live lane keeps its last row, and
`[accounts] pick = "headroom"` (spawns take the uncapped account with the lowest live 5 h / 7 d use; the default,
`order`, keeps today's pick) works from those older rows. Whether the status line carries `rate_limits` under a
`token-file:` login is untested.

`herdsman accounts` shows (the Host tab shows the same):

```
A        ok                           1 lanes app-impl-12
         session 9% (resets 10m) · week 22% (resets Fri 11:59)
         5h 1.5 · 7d 12.0 herdsman lane-hours
```

The first line is the cap state (`ok`, or `capped until Nov 4 08:00`) and the lanes on the account now.

Each value has a thin bar on the page: amber at 75 % or more, red at 90 % or more. When the weekly rate
would run out before the reset, the line adds `at this pace: out Mon evening`.

At spawn, when every uncapped account of the role has a reading under 30 min old, the one with the
lowest weekly % goes first. Otherwise the configured order holds.

**Lane-hours** are what herdsman can measure itself. For each account, they are the hours its lanes
ran in the last 5 h and 7 d, taken from the event log: from the spawn to the issue's merge or clean,
the lane's `lane-dead`, a supersede, or now. They are not Anthropic's quota.

## DeepSeek (dsh lanes, Hermes lanes using DeepSeek)

The key lives in an env file (mode 0600, else refused `SECRET_FILE_OPEN`):

```toml
[providers.deepseek]
key_env = "DEEPSEEK_API_KEY"     # the variable name
# env_file = "~/.config/deepseek.env"   # default: herdsman's environment
# warn_below = 5                 # amber, and `provider-low` from `herdsman check`, under this balance
```

herdsman reads `GET https://api.deepseek.com/user/balance` with `curl`, at most every 10 min.
The key goes in a curl config on stdin, never in argv, so `ps` can't show it. The Host tab and
`herdsman accounts` show `deepseek · USD 2.28 · ok`. The row is hidden when no key is set. If `env_file` is set but unreadable, or
lacks the variable, a dim `deepseek · key not found in <file>` row shows instead (or the refusal, for a loose file).
`herdsman check` logs `provider-low` once each time the balance drops below `warn_below`.

## Hermes lanes

Hermes profiles own their provider keys, in `~/.hermes/profiles/<profile>/.env`. herdsman never
reads them.

## Adding a provider (#806)

Each provider is one module in `crates/core/src/providers/` behind the `Provider` trait (login, accounts,
usage/limits, models, launch, prompt and screen parsing); core asks `providers::get`, `lanes()`, `accounts()`,
`proxied()` and the like, and never names one. arch-check's `[[ban]]`s in `.herdsman/arch.toml` fail any provider
name (any case, comments too) in `crates/*/src/**` outside that folder. The ADR is
[docs/architecture/providers/adr.md](architecture/providers/adr.md).

To add one:

1. Write `crates/core/src/providers/<name>.rs` with `pub static PROVIDER` implementing `Provider`. Override only what
   differs from the defaults: `surface()` (`Terminal`, a `harness` choice; `Acp`, reached through its ACP agent;
   `Listed`, known only as an agent's vendor), `start_cmd`/`kick_cmds`, `login()`, `credential()` (an `Accounts` with
   its config-folder and token env), `usage()`, `efforts()`, `hook_dialect()`, `brands()`.
2. Append its name to the `registry! { … }` line in `crates/core/src/providers/mod.rs`. Order matters only for the
   first-match readers (screen parsing, `accounts()`), so new ones go last.

Nothing else changes: `harness = "<name>"` validates, `profiles sync` writes its files, setup offers its login and an
account signs in through its env. `crates/core/src/providers/fake.rs`, a test-only provider, proves it with unit tests.
