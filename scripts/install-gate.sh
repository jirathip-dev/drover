#!/bin/sh
# install-gate NEW DEST (#539): put the freshly built herdsman binary NEW at DEST only if NEW's
# `doctor --config-only` loads the live fleet.toml; else keep DEST, print the fix and exit 1 (the auto-install job's
# failure goes to #fleet-ops). #707: no signing step and no secret read (herdsman keeps no Keychain items).
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
