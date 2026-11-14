# Providers and keys

drover supports two model providers: Anthropic (Claude Code lanes) and DeepSeek (dsh lanes, and
Hermes lanes that use DeepSeek).

**No secrets in fleet.toml, ever.** fleet.toml holds only the *names* of env variables. drover
checks every fleet.toml value on load. A value shaped like a token fails with `SECRET_IN_CONFIG`
(exit 3), and the refusal names the key, never the value. Token-shaped means a known key prefix
(`sk-`, GitHub and Slack token prefixes: the ones the page masks in pane text), a JWT, or a run of 32+ letters and
digits with no `.`/`/`/`-`/`_` in it. Env-var names (`CLAUDE_CODE_OAUTH_TOKEN_ACCOUNT_A1_PERSONAL`),
paths and model ids pass.

## Anthropic (Claude Code lanes)

**No store (#707).** drover keeps no secret store and makes no Keychain read. Each secret is a file on the main host
only its owner reads (mode 0600; a looser one is refused `SECRET_FILE_OPEN`): a `secret:<NAME>` env is
`<config dir>/secrets/<NAME>`; the webhook secret, a provider key's `env_file` and the proxy's `token-file` are the
files fleet.toml names by path. Place one by hand (`umask 077`), or answer `drover secret request`'s masked card from a
paired device, which writes `<config dir>/secrets/<NAME>`; `drover secret rm` deletes it. Remote hosts keep no
secrets.

**Lane sign-in (#923).** drover keeps no Claude token and has no accounts: lanes run on the host's own login
(`claude /login` once per host, [accounts](accounts.md)). Remote `shed-acp` lanes sign in through the main host's
token proxy, which signs with the one `[proxy] token-file` ([hosts](hosts.md)).

**Delivery.** At spawn drover reads the lane's `secret:<NAME>` env from its file and writes it to a 0600 file beside
the worktree on the lane's host: directly on the main host, over the ssh channel's stdin on a remote one. The pane's
shell sources the file; spawn deletes it once the harness has started. A secret never reaches argv, the pane's
scrollback, the event log or a file left behind.

**Usage limits.** `drover check` reads each lane's pane. A usage-limit banner or a sign-in prompt raises one
`usage-limit` or `signed-out` alert and holds new lanes on that host until the owner signs in and runs
`drover hosts on <host>` ([accounts](accounts.md)). Claude lanes run `drover statusline` as their status line; it prints
the lane's fleet and issue.

## DeepSeek (dsh lanes, Hermes lanes using DeepSeek)

The key lives in an env file (mode 0600, else refused `SECRET_FILE_OPEN`):

```toml
[providers.deepseek]
key_env = "DEEPSEEK_API_KEY"     # the variable name
# env_file = "~/.config/deepseek.env"   # default: drover's environment
# warn_below = 5                 # amber, and `provider-low` from `drover check`, under this balance
```

drover reads `GET https://api.deepseek.com/user/balance` with `curl`, at most every 10 min.
The key goes in a curl config on stdin, never in argv, so `ps` can't show it. The Host tab shows
`deepseek · USD 2.28 · ok`. The row is hidden when no key is set. If `env_file` is set but unreadable, or
lacks the variable, a dim `deepseek · key not found in <file>` row shows instead (or the refusal, for a loose file).
`drover check` logs `provider-low` once each time the balance drops below `warn_below`.

## Hermes lanes

Hermes profiles own their provider keys, in `~/.hermes/profiles/<profile>/.env`. drover never
reads them.

## Adding a provider (#806)

Each provider is one module in `crates/core/src/providers/` behind the `Provider` trait (login, usage-limit
banner, models, launch, prompt and screen parsing); core asks `providers::get`, `lanes()`,
`proxied()` and the like, and never names one. arch-check's `[[ban]]`s in `.drover/arch.toml` fail any provider
name (any case, comments too) in `crates/*/src/**` outside that folder. The ADR is
[docs/architecture/providers/adr.md](architecture/providers/adr.md).

To add one:

1. Write `crates/core/src/providers/<name>.rs` with `pub static PROVIDER` implementing `Provider`. Override only what
   differs from the defaults: `surface()` (`Terminal`, a `harness` choice; `Acp`, reached through its ACP agent;
   `Listed`, known only as an agent's vendor), `start_cmd`/`kick_cmds`, `login()`, `token_env()`, `cap_banner()`,
   `efforts()`, `hook_dialect()`, `brands()`.
2. Append its name to the `registry! { … }` line in `crates/core/src/providers/mod.rs`. Order matters only for the
   first-match readers (screen parsing, `proxied()`), so new ones go last.

Nothing else changes: `harness = "<name>"` validates, `profiles sync` writes its files, setup offers its login. `crates/core/src/providers/fake.rs`, a test-only provider, proves it with unit tests.
