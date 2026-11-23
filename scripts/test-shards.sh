#!/usr/bin/env bash
# #1162: CI's test shards. `test-shards.sh <n>` prints shard n's cargo target flags (`unit` is every crate's lib and
# bin tests). #1548: a target the last file of scripts/test-shards.d names (`<shard> <test target>` lines) runs in that
# shard; any other runs in the shard its name hashes to. A name with no test file or a shard outside 1 to 4 is
# ignored, so every crates/*/tests/*.rs (or tests/<dir>/main.rs) target is in exactly one shard.
# #1586: `test-shards.sh balance <nextest output>...` writes a new list from the PASS lines of nextest runs (a shard
# job's log, `gh run view --job <id> --log`) and removes the one before: each target's summed test time, averaged
# over the logs that ran it, goes longest first to the shard with the least time so far.
set -euo pipefail
cd "$(dirname "$0")/.."
dir=scripts/test-shards.d
targets=$({ echo unit; ls crates/*/tests/*.rs crates/*/tests/*/main.rs; } | sed -E 's|.*/tests/||; s|/main\.rs$||; s|\.rs$||' | sort)
case ${1:-} in
  balance)
    shift
    test $# -gt 0 || { echo "usage: $0 balance <nextest output>..." >&2; exit 2; }
    new=$dir/$(date -u +%F).txt
    sed -E 's/(\^\[|\x1b)\[[0-9;]*m//g' "$@" | awk -v targets="$targets" '
      FNR == 1 { file++ }
      match($0, /PASS \[ *[0-9.]+s\] (\([ 0-9]+\/[0-9]+\) )?[^ ]+ /) {
        m = split(substr($0, RSTART, RLENGTH), f, /[][ ]+/)
        bin = f[m - 1]
        t = (bin ~ /::/ && bin !~ /::bin\//) ? substr(bin, index(bin, "::") + 2) : "unit"
        secs[t] += f[2]
        if (seen[t] != file) { seen[t] = file; runs[t]++ }
      }
      END {
        n = split(targets, all, "\n")
        for (i = 1; i <= n; i++) w[all[i]] = runs[all[i]] ? secs[all[i]] / runs[all[i]] : 0
        for (i = 1; i <= n; i++) for (j = i + 1; j <= n; j++) if (w[all[j]] > w[all[i]]) { x = all[i]; all[i] = all[j]; all[j] = x }
        for (i = 1; i <= n; i++) {
          s = 1
          for (k = 2; k <= 4; k++) if (load[k] < load[s]) s = k
          load[s] += w[all[i]]
          print s, all[i]
        }
      }' | sort -k2 >"$new.tmp"
    mv "$new.tmp" "$new"
    find "$dir" -name '*.txt' ! -path "$new" -delete
    ;;
  [1-4])
    list=$(find "$dir" -name '*.txt' 2>/dev/null | LC_ALL=C sort | tail -1)
    for t in $targets; do
      s=$(awk -v t="$t" '$2 == t && $1 ~ /^[1-4]$/ { print $1; exit }' "${list:-/dev/null}")
      test "${s:-$(($(cksum <<<"$t" | cut -d' ' -f1) % 4 + 1))}" = "$1" || continue
      if [ "$t" = unit ]; then printf -- '--lib --bins '; else printf -- '--test %s ' "$t"; fi
    done
    ;;
  *) echo "usage: $0 balance <nextest output>... | <1-4>" >&2; exit 2 ;;
esac
