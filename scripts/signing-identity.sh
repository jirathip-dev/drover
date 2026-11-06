#!/bin/sh
# signing-identity [KEYCHAIN] (#556): create the self-signed code-signing identity "herdsman-local" once, in the
# login keychain. install-gate.sh signs every installed herdsman with it, so the designated requirement stays the
# same across builds and the owner's one "Always Allow" on the Keychain dialog lasts. Run it, then reinstall.
# #568: it also sets the key's partition list (asks the login password here), so codesign never waits on a dialog.
# signing-identity sign FILE [KEYCHAIN]: sign FILE with it by SHA-1 hash (the self-signed cert is not trusted, so
# `--sign herdsman-local` finds no identity); exit 2 without the identity, 1 when codesign fails or hangs.
set -eu
name="herdsman-local"
if [ "${1:-}" = sign ]; then
  file=${2:?usage: signing-identity.sh sign FILE [KEYCHAIN]}
  kc=${3:-$HOME/Library/Keychains/login.keychain-db}
  hash=$(security find-certificate -c "$name" -Z "$kc" 2>/dev/null | awk '/^SHA-1/ {print $NF}')
  [ -n "$hash" ] || exit 2
  codesign --force --sign "$hash" --keychain "$kc" --identifier herdsman "$file" &
  pid=$!
  (sleep "${HERDSMAN_SIGN_TIMEOUT:-60}" && kill "$pid") >/dev/null 2>&1 &
  dog=$!
  rc=0
  wait "$pid" || rc=$?
  kill "$dog" 2>/dev/null || true
  [ "$rc" -eq 0 ] && exit 0
  echo "signing-identity: codesign failed or timed out (exit $rc), likely waiting on a keychain dialog"
  echo "fix: security set-key-partition-list -S apple-tool:,apple:,codesign: -s $kc (asks the login password)"
  exit 1
fi
kc=${1:-$HOME/Library/Keychains/login.keychain-db}
if security find-certificate -c "$name" "$kc" >/dev/null 2>&1; then
  echo "signing-identity: \"$name\" is already in $kc"
else
  d=$(mktemp -d)
  trap 'rm -f "$d"/key.pem "$d"/cert.pem "$d"/id.p12; rmdir "$d"' EXIT
  # /usr/bin/openssl (LibreSSL) writes a PKCS#12 that `security import` reads; OpenSSL 3's default does not.
  /usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj "/CN=$name" \
    -addext "keyUsage=critical,digitalSignature" -addext "extendedKeyUsage=critical,codeSigning" \
    -keyout "$d/key.pem" -out "$d/cert.pem" 2>/dev/null
  /usr/bin/openssl pkcs12 -export -inkey "$d/key.pem" -in "$d/cert.pem" -out "$d/id.p12" -passout pass:herdsman
  security import "$d/id.p12" -k "$kc" -P herdsman -T /usr/bin/codesign
  echo "signing-identity: \"$name\" added to $kc"
fi
echo "signing-identity: letting codesign use the key without a dialog; enter your login password"
security set-key-partition-list -S apple-tool:,apple:,codesign: -s "$kc" >/dev/null
echo "signing-identity: done; now reinstall herdsman (docs/setup.md)"
