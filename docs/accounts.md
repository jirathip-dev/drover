# Accounts

## Accounts are shed's (#1576: #923 is reversed)

#923's rule, lanes on the host's own login and no account layer, is reversed on the main host: shed keeps the
accounts (their folders and setup tokens) and injects one into each run. Name the main host's logins in shed
first (`shed account add claude work`, then its sign-in); `drover doctor` shows one `✓ account <agent>: …` row per
agent a role or an orchestrator starts over ACP.

fleet.toml picks the account with `account`, a name or `"auto"`, on a fleet, a role or
`[fleet.<n>.role.orchestrator]` (its threads); the role's wins, then the fleet's, and no key at all is `"auto"`:

```toml
[fleet.web]
account = "work"
[fleet.web.role.rev]
account = "auto"
[fleet.web.role.orchestrator]
account = "personal"
```

`auto` lets shed pick a signed-in named account and move a run at a limit; `default` (the CLI's own login) is used
only when named. A lane's or a thread's ACP start on the main host carries it (`shed run --account`), and its
`spawn` or `orchestrator-start` event records it. A terminal (dsh) lane and a lane on another host carry none.

## A signed-out or limited account holds its starts

On the main host drover reads `account.ls`, never a lane's banner. A start whose account is signed out, limited or
missing (for `auto`: no named account ready) is refused `LOGIN_NEEDED`, and goes through again by itself once
shed lists the account ready. An account whose folder holds a setup is refused `ACP_UNGUARDED`: its run would not
load drover's profile. `drover check` raises one alert per account and episode (`signed-out` or `usage-limit`). An
orchestrator thread gets one row: for a limit, `<agent> account <name> hit its usage limit until <reset>; your
message is kept. New threads will fail too until then.` (and one `continue` at the reset); for a sign-out, a card
whose [sign in] starts shed's `account.login` in the sign-in sheet. When shed moves a live orchestrator to another
account, its thread shows `<was> hit its limit; now on <now>` (or `switched to <now>`).

## A lane on another host: a usage limit or a sign-out holds the host

A lane there (or a terminal lane) that shows its CLI's usage-limit banner or a sign-in prompt raises one alert
(`usage-limit` or `signed-out`) naming the lane, the host and the login command. Drive then starts no new lane on
that host (spawn refuses `LOGIN_NEEDED`) until the owner signs in there and turns the host back on.

## Retired keys

A fleet.toml that still has a role's `accounts` / `fallback`, `[chat] account(s)` or the `[accounts]` tables
loads, and `check` warns once (`config-warning`) naming the keys to delete. They do nothing; that covers
`[accounts.usage]`'s `cap_pattern` too. The old board's Accounts panel is gone with them, and with it the account
API fields `tone`, `meters`, `hint` and each meter's `label` and `pct`.

## The old store

drover ≤ #683 kept secrets in the login Keychain as `drover/<fleet>/<NAME>`. It reads none of them now and deletes
none: `drover migrate` prints the `security delete-generic-password` lines for the owner to run. `secret:` env and the
other secrets are files: see [providers](providers.md).

On macOS, Claude Code's own login lives in the login Keychain (the `Claude Code-credentials` item), so a headless Mac
keeps that keychain unlocked with no timeout and stays awake: see [troubleshooting](troubleshooting.md) §7.
