#!/bin/sh
# install-gate NEW DEST (#539): put the freshly built herdsman binary NEW at DEST only if NEW's
# `doctor --config-only` loads the live fleet.toml; else keep DEST, print the fix and exit 1 (the
# auto-install job's failure goes to #fleet-ops).
set -u
new=${1:?usage: install-gate.sh NEW DEST}
dest=${2:?usage: install-gate.sh NEW DEST}
if ! out=$("$new" doctor --config-only 2>&1); then
  echo "herdsman install-gate: the new binary can't load the live config; kept $dest"
  echo "$out"
  echo "fix: edit the config as the ✗ line says, then rerun the install"
  exit 1
fi
mkdir -p "$(dirname "$dest")" && cp "$new" "$dest.new" && mv "$dest.new" "$dest"
# #548: an ad-hoc signed rebuild is a new app to the Keychain, so each account item asks again; the installed binary's
# doctor names every item that prompts with its one-time fix (security asks for the login keychain password on a
# terminal and fails fast without one; then the fix is printed to run by hand).
if command -v security >/dev/null 2>&1; then
  "$dest" doctor 2>/dev/null | sed -n 's/.* prompts for Keychain access → //p' | while read -r fix; do
    sh -c "$fix" </dev/null || echo "keychain: run once in a terminal: $fix"
  done
fi
