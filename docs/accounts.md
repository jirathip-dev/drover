# Accounts and the Keychain

## Lanes sign in with the account's own login (#683)

herdsman keeps no Claude token. A lane on account `A` gets the env that account signs in with, and no
`CLAUDE_CODE_OAUTH_TOKEN`:

- `[accounts] account_env = "cooee accounts env {provider} {name}"` (q846): herdsman runs that command (its words,
  `{provider}` and `{name}` filled in, 30 s at most) and exports exactly the JSON object of env vars it prints, e.g.
  `{"CLAUDE_CONFIG_DIR": "…"}`. A failing or non-JSON answer refuses the spawn `ACCOUNT_ENV` with its stderr.
  herdsman never reads cooee's folder layout.
- With no `account_env`, the account's folder: its `[accounts.usage]` folder, else `~/.config/herdsman/accounts/<name>`,
  as the provider's config folder (claude: `CLAUDE_CONFIG_DIR`, codex: `CODEX_HOME`; one table, `access::CONFIG_ENV`).

Sign in with cooee (Settings › Providers) or `CLAUDE_CONFIG_DIR=<folder> claude /login`. Claude Code keeps the login
in its own signed Keychain item, so no herdsman rebuild prompts. The lane's profile goes on top of that login
(`--settings`, `--append-system-prompt-file`; an ACP lane's worktree-local settings), so its hook still guards it.
`herdsman accounts` and `doctor` (and the install gate, `doctor --tokens`) show each account's sign-in from its folder's
`.claude.json`, never a Keychain read or a `claude` call. Rotation, caps and `accounts reset` work per account as
before. The `[accounts]` values (once each token's name) are unused. Remote hosts get no account env: their lanes sign
in through the main host's token proxy (#637).

`accounts import` is retired (q854). The old `herdsman/all/CLAUDE_CODE_OAUTH_TOKEN_*` items go only with the owner's OK, on
a run card (`herdsman secret rm all CLAUDE_CODE_OAUTH_TOKEN_A`, …), once every lane runs on its account env. The rest of
this page is about the store's other secrets: `secret:` env values live in the main host's OS store as
`herdsman/<fleet>/<NAME>` (#441); on macOS that is the login Keychain.

## Why a rebuild prompts (#548)

`cargo install` signs the binary ad hoc: its code signature is a hash of the binary itself, so every rebuild is a new
app to the Keychain. An item herdsman saved (or was allowed to read) by an older build is not open to the new one, and
macOS shows a "herdsman wants to use your confidential information" dialog on the host's screen. Until someone clicks
it, the read waits.

herdsman never waits on it: a keychain read or write gets 5 s, then fails with
`REFUSED KEYCHAIN_LOCKED: approve herdsman in the Keychain dialog on <host>: Always Allow for <item>, once (…)`. A thread shows it as a
`not delivered` row, a lane's spawn as its refusal, and `herdsman check` alerts `keychain-locked` once.

## The one-time fix

An item admits the app that wrote it and every app the owner allowed; the Keychain knows an app by its designated
requirement. An ad-hoc signed herdsman has a new one each build, so every reinstall asked again. Since #556 the install
signs herdsman with one local identity, "herdsman-local" (`scripts/signing-identity.sh` creates it once; see
[setup](setup.md)), so its designated requirement stays the same across builds. Click **Always Allow** once per item
and it lasts.

`doctor`'s `signature` row is red while the installed binary is not signed by "herdsman-local", with the setup command.

A message to a thread whose delivery is stuck waits at most 30 s behind it, then shows `not delivered` instead of
hanging.

Keychain over ssh, a jammed SecurityAgent and a new build's cdhash: see [troubleshooting](troubleshooting.md).
