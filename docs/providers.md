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

**The store (#441).** Every secret lives only in the main host's OS store, through the `keyring` crate: the macOS
Keychain, Linux Secret Service or the Windows Credential Manager, as `herdsman/<fleet>/<NAME>` (`all` for every fleet).
There is no plain-file fallback: when fleet.toml names a secret (an `[accounts]` entry or a `secret:` env) and the
main host has no usable store, `herdsman serve` refuses `NO_STORE`. Remote hosts
keep no secrets.

**Lane tokens.** Each `[accounts]` entry names the secret holding that account's token, `herdsman/all/<VAR>`:

```toml
[accounts]
A = "CLAUDE_CODE_OAUTH_TOKEN_A"   # the secret's name, never the token
B = "CLAUDE_CODE_OAUTH_TOKEN_B"
```

Make a token with `claude setup-token` while logged in to that account. It is long-lived.
A role picks with `account = "A"` or an ordered `accounts = ["A", "B"]`.

**Delivery.** At spawn herdsman reads the lane's secrets (the first uncapped account's token as
`CLAUDE_CODE_OAUTH_TOKEN`, and each `secret:<NAME>` env) from the store and writes them to a 0600 file beside the
worktree on the lane's host: directly on the main host, over the ssh channel's stdin on a remote one. The pane's shell
sources the file; spawn deletes it once the harness has started. A token never reaches argv, the pane's scrollback,
the event log or a file left behind.

**Import from `tokens.env`.** `accounts_env` is retired; a fleet.toml that still sets it fails to load. Run
`herdsman accounts import [FILE]` (owner only; default `~/.config/herdsman/accounts/tokens.env`) once: it copies each
`[accounts]` variable's value into the store, reads it back and checks its sha256, and prints a `✓` row per account.
It never deletes the file; do that yourself once lanes run on the store.

**Fallback and caps.** `herdsman check` reads each Claude lane's pane. When it shows the usage-limit
banner, it logs `account-capped` with `until`, the banner's reset time (`resets 9am`, `resets Nov 4, 8am`,
host local time; unparseable: `cap_reset_minutes`, default 300, or a day for a weekly limit). Spawns skip
that account until then, or until `herdsman accounts reset <name>`; with every account capped, spawn
refuses `ALL_CAPPED` (drive stops starting for the pass). The capped lane waits on drive, which
respawns it with `--supersede` on a free account (the kill pushes its work to `wip/` first).

**Usage meter from the lanes.** Claude lanes run `herdsman statusline` as their status line. Claude Code hands
it the account's 5 h / 7 d limits on every refresh; it appends at most one row per account per minute to
`<state>/usage.jsonl` (the account from `HERDSMAN_ACCOUNT`, set at spawn) and prints `myapp #12 · 5h 41% · 7d 63%`
in the pane. The meter shows an account's newest row (with its age), preferring a row under 10 min old over the
`/login` reading below, which is now optional. `[accounts] pick = "headroom"` makes spawns take the uncapped
account with the lowest live 5 h / 7 d use; the default, `order`, keeps today's pick.

**Usage meter.** Setup tokens can't read the usage API: they lack the `user:profile` scope, and
`/api/oauth/usage` answers 403. To get the real meter, log each account in once with a full `claude`
login in its own config folder:

```sh
CLAUDE_CONFIG_DIR=~/.config/herdsman/accounts/A claude   # then /login as account A; repeat for B, C
```

```toml
[accounts.usage]
A = "~/.config/herdsman/accounts/A"   # no trailing slash: it must match CLAUDE_CONFIG_DIR exactly
```

On macOS, Claude Code keeps that login in the Keychain as a generic password. **The service name
is derived from Claude Code 2.1.308 and is unverified:** `Claude Code-credentials-<h>`, where `<h>`
is the first 8 hex digits of the SHA-256 of the `CLAUDE_CONFIG_DIR` string exactly as given. A
trailing slash, `~` or another spelling of the same folder gives a different hash, so write the path
in `[accounts.usage]` exactly as you set `CLAUDE_CONFIG_DIR`. Without `CLAUDE_CONFIG_DIR` the item is
plain `Claude Code-credentials`, your default login. herdsman never reads or writes that one, only
the hashed items of the `[accounts.usage]` folders.

herdsman reads the item's account attribute, then its value, with `security find-generic-password` (these logins are
Claude Code's own items, not herdsman's store).
When the access token has expired, herdsman refreshes it and writes the rotated login back to the
same item (same service and account) the way Claude Code does: `security -i`, with the value
hex-encoded on stdin. It then re-reads the item to check that the new token was stored. A refresh
rotates the refresh token, so if the write-back fails, the login is gone. herdsman then logs
`usage-login-lost` (sent to notifiers) naming the account and the fix, `claude /login in <dir>`.
An item whose service or account holds a quote or another unusual character is not refreshed at
all. These logins are used only for the usage read and are never exported to lanes.

The meter is read at most every 10 min per account, one read at a time. A 429 backs off for at
least its `retry-after`. The cache, `<state>/usage/<name>.json` (mode 0600), holds numbers and
reset times only. `herdsman accounts` shows (the Host tab shows the same):

```
A        ok                           1 lanes app-impl-12
         session 9% (resets 10m) · week 22% (resets Fri 11:59)
         5h 1.5 · 7d 12.0 herdsman lane-hours
```

The first line is the cap state (`ok`, or `capped until Nov 4 08:00`) and the lanes on the account now.

Each value has a thin bar on the page: amber at 75 % or more, red at 90 % or more. When the weekly rate
would run out before the reset, the line adds `at this pace: out Mon evening`. If a login is
missing or expired, the line reads `usage: login needed (claude /login in <dir>)`, and the page
still loads.

At spawn, when every uncapped account of the role has a reading under 30 min old, the one with the
lowest weekly % goes first. Otherwise the configured order holds.

**Lane-hours** are what herdsman can measure itself. For each account, they are the hours its lanes
ran in the last 5 h and 7 d, taken from the event log: from the spawn to the issue's merge or clean,
the lane's `lane-dead`, a supersede, or now. They are not Anthropic's quota.

## DeepSeek (dsh lanes, Hermes lanes using DeepSeek)

The key lives in an env file, like the Claude tokens:

```toml
[providers.deepseek]
key_env = "DEEPSEEK_API_KEY"     # the variable name
# env_file = "~/.config/deepseek.env"   # default: the store's `herdsman/all/<key_env>`, else herdsman's environment
# warn_below = 5                 # amber, and `provider-low` from `herdsman check`, under this balance
```

herdsman reads `GET https://api.deepseek.com/user/balance` with `curl`, at most every 10 min.
The key goes in a curl config on stdin, never in argv, so `ps` can't show it. The Host tab and
`herdsman accounts` show `deepseek · USD 2.28 · ok`. The row is hidden when no key is set. If `env_file` is set but unreadable, or
lacks the variable, a dim `deepseek · key not found in <file>` row shows instead.
`herdsman check` logs `provider-low` once each time the balance drops below `warn_below`.

## Hermes lanes

Hermes profiles own their provider keys, in `~/.hermes/profiles/<profile>/.env`. herdsman never
reads them.
