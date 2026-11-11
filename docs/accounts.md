# Accounts

## Lanes sign in with the account's own login (#683)

herdsman keeps no Claude token and reads no Keychain item (#707). A lane on account `A` gets the env that account signs
in with:

- `[accounts] account_env = "cooee accounts env {provider} {name}"` (q846): herdsman runs that command (its words,
  `{provider}` and `{name}` filled in, 30 s at most) and exports exactly the JSON object of env vars it prints, e.g.
  `{"CLAUDE_CONFIG_DIR": "…"}`. A failing or non-JSON answer refuses the spawn `ACCOUNT_ENV` with its stderr.
  herdsman never reads cooee's folder layout. When it is set, cooee's account list wins.
- With no `account_env`, the account's folder: its `[accounts.usage]` folder, else `~/.config/herdsman/accounts/<name>`,
  as the provider's config folder (claude: `CLAUDE_CONFIG_DIR`, codex: `CODEX_HOME`; each provider's `Accounts::config_env`).

Standalone, sign each account in once with `CLAUDE_CONFIG_DIR=<folder> claude /login` (or with cooee, Settings ›
Providers). Claude Code keeps that login itself; herdsman only points the lane at the folder. The lane's profile goes on
top of that login (`--settings`, `--append-system-prompt-file`; an ACP lane's worktree-local settings), so its hook
still guards it. `herdsman accounts` shows each account's sign-in as its CLI's own status command says
(`claude auth status` / `codex login status` in the folder, the shell's token unset), never a Keychain read.
`herdsman doctor` shows one row for where accounts sign in from (the folders or the `account_env` helper), no row per
account (q855). Rotation, caps and `accounts reset` work per account as before. Remote hosts get no account env: their
lanes sign in through the main host's token proxy (#637).

## A signed-out account is skipped (#785)

Before a lane starts on an account, herdsman checks its sign-in: with cooee, `account_env = "cooee accounts env
{provider} {name} --require-signed-in"` exits 3 for a signed-out account; standalone, the folder's status command says
`loggedIn: false`. Either way the account rests like a capped one (`account-signed-out`, one owner alert naming the
sign-in), the lane goes on the role's next account, and the account stays out until it is signed in and `herdsman
accounts reset <name>` is run. A dry run asks nothing. An ACP lane with no event back `[limits] acp_start_minutes`
(default 3) after its first turn alerts `lane-failed-start` naming its account, and `waiting` calls it a failed start,
not idle.

The meter (`usage.jsonl`) is written only by an interactive Claude status line; ACP lanes write none, so a reading can be
days old. `herdsman accounts` shows each reading's age, a window past its reset reads `0% (reset)`, `doctor` warns on a
reading over 5 h old, and the pick never counts a window past its reset. `herdsman accounts` lists only lanes still
running.

## A token file, for a host with no login Keychain (#707)

A headless Mac that must work with its login Keychain locked can give an account a hand-placed token instead:

```toml
[accounts]
A = "token-file:~/.config/herdsman/tokens/A"   # a `claude setup-token` token, mode 0600
```

With no `account_env`, a lane on `A` gets the file's token as `CLAUDE_CODE_OAUTH_TOKEN`, in its own process only (the
0600 secrets file its pane sources, never argv or the log); the token proxy signs that account's remote ACP lanes with
it. A file group or others can read is refused `SECRET_FILE_OPEN`, and `doctor` warns when it is missing, loose or more
than 300 days old (these tokens last a year). Folder logins stay the default: they refresh and can be revoked.

## The old store

herdsman ≤ #683 kept secrets in the login Keychain as `herdsman/<fleet>/<NAME>`. It reads none of them now and deletes
none: `herdsman migrate` prints the `security delete-generic-password` lines for the owner to run. `secret:` env and the
other secrets are files: see [providers](providers.md).

On macOS, Claude Code's own login lives in the login Keychain (the `Claude Code-credentials` item), so a headless Mac
on folder logins keeps that keychain unlocked with no timeout and stays awake: see [troubleshooting](troubleshooting.md)
§7 (or give its accounts a `token-file:`). On Linux it is said to be a plain file in the config folder
(`.credentials.json`): unverified.
