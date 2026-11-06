#!/bin/sh
# signing-identity [KEYCHAIN] (#556): create the self-signed code-signing identity "herdsman local" once, in the
# login keychain. install-gate.sh signs every installed herdsman with it, so the designated requirement stays the
# same across builds and the owner's one "Always Allow" on the Keychain dialog lasts. Run it, then reinstall.
set -eu
name="herdsman local"
kc=${1:-$HOME/Library/Keychains/login.keychain-db}
if security find-certificate -c "$name" "$kc" >/dev/null 2>&1; then
  echo "signing-identity: \"$name\" is already in $kc"
  exit 0
fi
d=$(mktemp -d)
trap 'rm -f "$d"/key.pem "$d"/cert.pem "$d"/id.p12; rmdir "$d"' EXIT
# /usr/bin/openssl (LibreSSL) writes a PKCS#12 that `security import` reads; OpenSSL 3's default does not.
/usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj "/CN=$name" \
  -addext "keyUsage=critical,digitalSignature" -addext "extendedKeyUsage=critical,codeSigning" \
  -keyout "$d/key.pem" -out "$d/cert.pem" 2>/dev/null
/usr/bin/openssl pkcs12 -export -inkey "$d/key.pem" -in "$d/cert.pem" -out "$d/id.p12" -passout pass:herdsman
security import "$d/id.p12" -k "$kc" -P herdsman -T /usr/bin/codesign
echo "signing-identity: \"$name\" added to $kc; now reinstall herdsman (docs/setup.md)"
