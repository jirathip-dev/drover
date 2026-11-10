#!/bin/sh
# install-gate NEW DEST (#539): put the freshly built herdsman binary NEW at DEST only if NEW's
# `doctor --config-only` loads the live fleet.toml; else keep DEST, print the fix and exit 1 (the auto-install job's
# failure goes to #fleet-ops). #781: where codesign exists, NEW is signed with "herdsman-local" first
# (scripts/signing-identity.sh); no identity, or a codesign that fails or hangs, keeps DEST and exits 1.
set -u
new=${1:?usage: install-gate.sh NEW DEST}
dest=${2:?usage: install-gate.sh NEW DEST}
if ! out=$("$new" doctor --config-only 2>&1); then
  echo "herdsman install-gate: the new binary can't load the live config; kept $dest"
  echo "$out"
  echo "fix: edit the config as the ✗ line says, then rerun the install"
  exit 1
fi
mkdir -p "$(dirname "$dest")" && cp "$new" "$dest.new" || exit 1
if command -v codesign >/dev/null; then
  rc=0
  out=$(sh "$(dirname "$0")/signing-identity.sh" sign "$dest.new") || rc=$?
  if [ "$rc" -ne 0 ]; then
    rm -f "$dest.new"
    echo "herdsman install-gate: not signed; kept $dest"
    [ "$rc" -eq 2 ] && out="fix: sh scripts/signing-identity.sh once, then rerun the install"
    echo "$out"
    exit 1
  fi
fi
mv "$dest.new" "$dest"
