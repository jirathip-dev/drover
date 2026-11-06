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
mkdir -p "$(dirname "$dest")" && cp "$new" "$dest.new" || exit 1
# #556: sign with the one local identity, so every build keeps the keychain's "Always Allow"; without it the
# install goes on ad-hoc signed and `herdsman doctor` shows the signature row red with the setup command.
# #568: a codesign that fails or hangs on a keychain dialog (timed out) keeps DEST and exits 1 with the fix.
if command -v codesign >/dev/null; then
  sh "$(dirname "$0")/signing-identity.sh" sign "$dest.new"
  case $? in
  0) ;;
  2) echo "herdsman install-gate: not signed; run scripts/signing-identity.sh once, then reinstall" ;;
  *)
    rm -f "$dest.new"
    echo "herdsman install-gate: signing failed; kept $dest"
    exit 1
    ;;
  esac
fi
mv "$dest.new" "$dest"
