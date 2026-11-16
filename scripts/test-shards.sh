#!/usr/bin/env bash
# #1162: CI's test shards. `test-shards.sh <n>` prints shard n's cargo target flags from scripts/test-shards.txt
# (`<shard> <test target>` lines; `unit` is every crate's lib and bin tests). Either form first fails when a
# crates/*/tests/*.rs (or tests/<dir>/main.rs) target is in no shard, in two, or listed but gone.
set -euo pipefail
cd "$(dirname "$0")/.."
list=scripts/test-shards.txt
targets=$({ echo unit; ls crates/*/tests/*.rs crates/*/tests/*/main.rs; } | sed -E 's|.*/tests/||; s|/main\.rs$||; s|\.rs$||' | sort)
listed=$(awk '{print $2}' "$list" | sort)
bad=$({
  uniq -d <<<"$listed" | sed 's/^/in two shards: /'
  comm -23 <(echo "$targets") <(uniq <<<"$listed") | sed 's/^/in no shard: /'
  comm -13 <(echo "$targets") <(uniq <<<"$listed") | sed 's/^/no such test target: /'
  awk '$1 !~ /^[1-4]$/ {print "bad shard: " $0}' "$list"
})
test -z "$bad" || { echo "::error::$list: $bad"; exit 1; }
case ${1:-} in
  check) ;;
  [1-4]) awk -v n="$1" '$1 == n { printf "%s ", ($2 == "unit" ? "--lib --bins" : "--test " $2) }' "$list" ;;
  *) echo "usage: $0 check | <1-4>" >&2; exit 2 ;;
esac
