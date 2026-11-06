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

An item's partition list says which code may read it without a dialog. `security` creates items with only
`apple-tool:`, and an ad-hoc signed herdsman matches none of Apple's partitions, only its own `cdhash:<hash>` (the
`CDHash=` of `codesign -dvvv $(which herdsman)`). `herdsman doctor` reads every `herdsman/*` item's list from
`security dump-keychain -a` (no secret is read, so no dialog) and prints, for each one that lacks this build's cdhash:

```
✗ keychain herdsman/all/CLAUDE_CODE_OAUTH_TOKEN_A — prompts for Keychain access (partition list lacks cdhash:<hash>) → security set-generic-password-partition-list -S apple-tool:,apple:,unsigned:,cdhash:<hash> -s herdsman/all/CLAUDE_CODE_OAUTH_TOKEN_A -a herdsman
```

An account item that still times out on a read gets the same fix on an `account … prompts for Keychain access` row.
Run the command after the arrow in a terminal (it asks for the login keychain password): the item then admits this
build without a dialog. Each rebuild has a new cdhash, so `scripts/install-gate.sh` runs the installed binary's doctor
after every install, runs these fixes, and prints the ones it could not run (no terminal) for you to run once by hand.

A message to a thread whose delivery is stuck waits at most 30 s behind it, then shows `not delivered` instead of
hanging.
