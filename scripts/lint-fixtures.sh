#!/usr/bin/env bash
# #809: each banned API's fixture must fail its gate, and clean fixtures must pass.
# rust: ci/lint-fixtures/rust/<lint>.rs under the workspace's [lints] and clippy.toml must fail clippy naming <lint>.
# ts: web/lint-fixtures/<name>.ts must fail web/lint.mjs (clean.ts must pass).
# Usage: scripts/lint-fixtures.sh rust|ts
set -euo pipefail
cd "$(dirname "$0")/.."
bad=0
expect() {
  local name=$1 code=$2 out=$3 want=$4
  if [ "$name" = clean ]; then
    [ "$code" -eq 0 ] && echo "ok: clean passes" || { echo "::error::clean fixture failed: $out"; bad=1; }
  elif [ "$code" -eq 0 ] || ! grep -q -e "$want" <<<"$out"; then
    echo "::error::fixture $name did not fail on $want: $out"
    bad=1
  else
    echo "ok: $name is refused"
  fi
}
case "${1:-}" in
rust)
  t="${DROVER_SCRATCH_TARGET:-${RUNNER_TEMP:-${TMPDIR:-/tmp}}}/lint-fixtures"
  mkdir -p "$t/src"
  cp rust-toolchain.toml clippy.toml "$t/"
  {
    printf '[package]\nname = "lint-fixture"\nversion = "0.0.0"\nedition = "2021"\n\n[workspace]\n\n'
    sed -n '/^\[workspace\.lints\./,/^\[workspace\.dependencies\]/p' Cargo.toml |
      grep -v '^\[workspace\.dependencies\]' | sed 's/^\[workspace\.lints\./[lints./'
  } >"$t/Cargo.toml"
  for f in ci/lint-fixtures/rust/*.rs; do
    name=$(basename "$f" .rs)
    cp "$f" "$t/src/lib.rs"
    code=0
    out=$(cd "$t" && cargo clippy -q --all-targets 2>&1) || code=$?
    expect "$name" "$code" "$out" "$name"
  done
  ;;
ts)
  for f in web/lint-fixtures/*.ts; do
    name=$(basename "$f" .ts)
    code=0
    out=$(node --no-warnings web/lint.mjs "$f" 2>&1) || code=$?
    case $name in
    as-cast) want='an `as` cast' ;;
    non-null) want='a non-null' ;;
    any) want='`any`' ;;
    ts-ignore | ts-expect-error) want='@ts-' ;;
    inner-html | outer-html | insert-adjacent-html) want='raw HTML' ;;
    eval | new-function) want='`eval` / `new Function`' ;;
    console-log) want='console.log' ;;
    default-export) want='a default export' ;;
    enum) want='no `enum`' ;;
    *) want=$name ;;
    esac
    expect "$name" "$code" "$out" "$f:.*$want"
  done
  ;;
*)
  echo "usage: $0 rust|ts" >&2
  exit 2
  ;;
esac
exit "$bad"
