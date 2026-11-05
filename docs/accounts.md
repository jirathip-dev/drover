# Accounts and the Keychain

`[accounts]` tokens and `secret:` env values live in the main host's OS store as `herdsman/<fleet>/<NAME>` (#441);
on macOS that is the login Keychain.

## Why a rebuild prompts (#548)

`cargo install` signs the binary ad hoc: its code signature is a hash of the binary itself, so every rebuild is a new
app to the Keychain. An item herdsman saved (or was allowed to read) by an older build is not open to the new one, and
macOS shows a "herdsman wants to use your confidential information" dialog on the host's screen. Until someone clicks
it, the read waits.

herdsman never waits on it: a keychain read or write gets 5 s, then fails with
`REFUSED KEYCHAIN_LOCKED: approve herdsman in the Keychain dialog on <host>, or run <fix>`. A thread shows it as a
`not delivered` row, a lane's spawn as its refusal, and `herdsman check` alerts `keychain-locked` once.

## The one-time fix

`herdsman doctor` reads every account item and prints, for each one that prompts:

```
✗ account A — herdsman/all/CLAUDE_CODE_OAUTH_TOKEN_A prompts for Keychain access → security set-generic-password-partition-list -S apple-tool:,apple:,unsigned: -s herdsman/all/CLAUDE_CODE_OAUTH_TOKEN_A -a herdsman
```

Run the command after the arrow in a terminal (it asks for the login keychain password): the item's partition list then
admits unsigned and ad-hoc signed builds, so the next rebuild reads it without a dialog. `scripts/install-gate.sh` runs
these fixes after each install and prints the ones it could not run (no terminal) for you to run once by hand.

A message to a thread whose delivery is stuck waits at most 30 s behind it, then shows `not delivered` instead of
hanging.
