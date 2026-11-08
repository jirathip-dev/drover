# Accounts and the Keychain

## Accounts are folders (#683)

An `[accounts]` entry `A = "folder:<dir>"` is a Claude Code config folder with its own login: cooee's
`<cooee data>/accounts/claude/A` (Settings › Providers signs it in or out), or an `[accounts.usage]` folder signed in
with `CLAUDE_CONFIG_DIR=<dir> claude /login`. A lane on it gets `CLAUDE_CONFIG_DIR=<dir>` and no token; its profile
goes on top (`--settings`, `--append-system-prompt-file`; an ACP lane's worktree-local settings). Claude Code keeps the
login in its own signed Keychain item, so no herdsman rebuild prompts. `herdsman accounts` and `doctor` show each
folder's sign-in from `claude auth status` there. Rotation, caps and `accounts reset` work per folder as per token.
Remote hosts get no folder: their lanes sign in through the main host's token proxy (#637).

`herdsman accounts import --folders` (owner) moves each token entry whose `[accounts.usage]` folder is signed in to
`folder:<that folder>` and prints the `herdsman secret rm all <NAME>` lines for the old tokens, for a run card once
every lane runs on folders. The rest of this page is about the token entries until then.

`[accounts]` tokens and `secret:` env values live in the main host's OS store as `herdsman/<fleet>/<NAME>` (#441);
on macOS that is the login Keychain.

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
and it lasts. An account item that still times out on a read shows in `herdsman doctor`:

```
✗ account A — herdsman/all/CLAUDE_CODE_OAUTH_TOKEN_A prompts for Keychain access → approve herdsman in the Keychain dialog: Always Allow for herdsman/all/CLAUDE_CODE_OAUTH_TOKEN_A, once (it lasts while herdsman is signed by "herdsman-local")
```

`doctor`'s `signature` row is red while the installed binary is not signed by "herdsman-local", with the setup command.

A message to a thread whose delivery is stuck waits at most 30 s behind it, then shows `not delivered` instead of
hanging.

Keychain over ssh, a jammed SecurityAgent and a new build's cdhash: see [troubleshooting](troubleshooting.md).
