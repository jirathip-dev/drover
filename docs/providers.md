# Providers and keys

drover runs Claude Code lanes (Anthropic). It can also show a DeepSeek key's balance.

**No secrets in fleet.toml, ever.** fleet.toml holds only the *names* of env variables. drover
checks every fleet.toml value on load. A value shaped like a token fails with `SECRET_IN_CONFIG`
(exit 3), and the refusal names the key, never the value. Token-shaped means a known key prefix
(`sk-`, GitHub and Slack token prefixes: the ones the page masks in pane text), a JWT, or a run of 32+ letters and
digits with no `.`/`/`/`-`/`_` in it. Env-var names (`CLAUDE_CODE_OAUTH_TOKEN_ACCOUNT_A1_PERSONAL`),
paths and model ids pass.

## Anthropic (Claude Code lanes)

**No store (#707).** drover keeps no secret store and makes no Keychain read. Each secret is a file on the main host
only its owner reads (mode 0600; a looser one is refused `SECRET_FILE_OPEN`): a `secret:<NAME>` env is
`<config dir>/secrets/<NAME>`; the webhook secret and a provider key's `env_file` are the
files fleet.toml names by path. Place one by hand (`umask 077`), or answer `drover secret request`'s masked card from a
paired device, which writes `<config dir>/secrets/<NAME>`; `drover secret rm` deletes it. Remote hosts keep no
secrets.

**Lane sign-in (#923).** drover keeps no Claude token and has no accounts: lanes run on the host's own login
(`claude /login` once per host, [accounts](accounts.md)).

**Delivery.** At spawn drover reads the lane's `secret:<NAME>` env from its file and writes it to a 0600 file beside
the worktree on the lane's host: directly on the main host, over the ssh channel's stdin on a remote one. The pane's
shell sources the file and deletes it; spawn deletes it too once the harness has started (#1532: a failed remove logs
`secrets-left` with the path and the next drive pass retries it; `drover clean --yes` removes any `*.env` left under a
worktree root, on every host), so an ACP lane that sources it starts
`--no-hibernate` (#1311: shed's wake would run the start again and die on the missing file). A secret never reaches argv, the pane's
scrollback, the event log or a file left behind.

**Usage limits.** `drover check` reads each lane's pane. A usage-limit banner or a sign-in prompt raises one
`usage-limit` or `signed-out` alert and holds new lanes on that host until the owner signs in and runs
`drover hosts on <host>` ([accounts](accounts.md)). Claude lanes run `drover statusline` as their status line; it prints
the lane's fleet and issue.

## DeepSeek balance

The key lives in an env file (mode 0600, else refused `SECRET_FILE_OPEN`):

```toml
[providers.deepseek]
key_env = "DEEPSEEK_API_KEY"     # the variable name
# env_file = "~/.config/deepseek.env"   # default: drover's environment
# warn_below = 5                 # amber, and `provider-low` from `drover check`, under this balance
```

drover reads `GET https://api.deepseek.com/user/balance` with `curl`, at most every 10 min.
The key goes in a curl config on stdin, never in argv, so `ps` can't show it. `GET /api/v1/host` carries
`deepseek · USD 2.28 · ok`. The row is hidden when no key is set. If `env_file` is set but unreadable, or
lacks the variable, a dim `deepseek · key not found in <file>` row shows instead (or the refusal, for a loose file).
`drover check` logs `provider-low` once each time the balance drops below `warn_below`.

## Agents and the one provider module (#806, #1694)

A lane's agent is any row of shed's agent list ([lanes/agents](lanes/agents.md)); no code per agent. One module stays
in `crates/core/src/providers/`, behind the `Provider` trait, for what shed's record cannot carry: Claude's profile
files, session options and sandbox needs, and until the cut-over its hook format. arch-check's `[[ban]]`s in
`.drover/arch.toml` fail any provider or agent name (any case, comments too) in `crates/*/src/**` outside that folder.
The ADRs are [providers](architecture/providers/adr.md) and [judged-lanes](architecture/judged-lanes/adr.md).
`crates/core/src/providers/fake.rs`, a test-only provider, proves the registry with unit tests.
