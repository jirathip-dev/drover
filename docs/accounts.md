# Accounts

## Sign in once per host with the CLI's own login (#923)

drover has no account layer. Lanes run on the host's own login: sign in once per host with the CLI's own command
(`claude /login`, `codex login`), in its default folder. drover sets no config folder, token or account name in a
lane. Choosing between accounts is cooee's job (its per-thread account picker and sign-in sheet).

A login shell that exports `CLAUDE_CODE_OAUTH_TOKEN` overrides that login in every lane; `drover doctor` flags it.
Remove the export.

## A usage limit or a sign-out holds the host

A lane that shows its CLI's usage-limit banner or a sign-in prompt raises one alert (`usage-limit` or `signed-out`)
naming the lane, the host and the login command. Drive then starts no new lane on that host (spawn refuses
`LOGIN_NEEDED`) until the owner signs in there and runs `drover hosts on <host>` (`main` for this host). There are
no rest timers and no reset verb.

## Remote hosts: the token proxy

A `shed-acp` host's lanes sign in through the main host's token proxy (#637, [hosts](hosts.md)). The proxy signs
every call with one token: run `claude setup-token` on the main host, save the token in a file only you can read
(0600), and name it in fleet.toml:

```toml
[proxy]
token-file = "~/.config/drover/tokens/proxy"
```

The token lasts a year. A remote spawn without that file is refused `NO_PROXY_TOKEN` with the setup command.

## Retired keys

A fleet.toml that still has a role's `account` / `accounts` / `fallback`, `[chat] account(s)` or the `[accounts]`
tables loads, and `check` warns once (`config-warning`) naming the keys to delete. They do nothing.

## The old store

drover ≤ #683 kept secrets in the login Keychain as `drover/<fleet>/<NAME>`. It reads none of them now and deletes
none: `drover migrate` prints the `security delete-generic-password` lines for the owner to run. `secret:` env and the
other secrets are files: see [providers](providers.md).

On macOS, Claude Code's own login lives in the login Keychain (the `Claude Code-credentials` item), so a headless Mac
keeps that keychain unlocked with no timeout and stays awake: see [troubleshooting](troubleshooting.md) §7.
