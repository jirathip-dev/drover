#!/usr/bin/env bash
# #1162: CI's test shards. `test-shards.sh <n>` prints shard n's cargo target flags from scripts/test-shards.txt
# (`<shard> <test target>` lines; `unit` is every crate's lib and bin tests). Every form first fails when a
# crates/*/tests/*.rs (or tests/<dir>/main.rs) target is in no shard, in two, or listed but gone.
# #1586: `test-shards.sh balance <nextest output>...` rewrites the list from the PASS lines of nextest runs (a shard
# job's log, `gh run view --job <id> --log`): each target's summed test time, averaged over the logs that ran it,
# goes longest first to the shard with the least time so far.
set -euo pipefail
cd "$(dirname "$0")/.."
list=scripts/test-shards.txt
targets=$({ echo unit; ls crates/*/tests/*.rs crates/*/tests/*/main.rs; } | sed -E 's|.*/tests/||; s|/main\.rs$||; s|\.rs$||' | sort)
mode=${1:-}
if [ "$mode" = balance ]; then
  shift
  test $# -gt 0 || { echo "usage: $0 balance <nextest output>..." >&2; exit 2; }
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
    }' | sort -k2 >"$list.new"
  mv "$list.new" "$list"
fi
listed=$(awk '{print $2}' "$list" | sort)
bad=$({
  uniq -d <<<"$listed" | sed 's/^/in two shards: /'
  comm -23 <(echo "$targets") <(uniq <<<"$listed") | sed 's/^/in no shard: /'
  comm -13 <(echo "$targets") <(uniq <<<"$listed") | sed 's/^/no such test target: /'
  awk '$1 !~ /^[1-4]$/ {print "bad shard: " $0}' "$list"
})
test -z "$bad" || { echo "::error::$list: $bad"; exit 1; }
case $mode in
  check | balance) ;;
  [1-4]) awk -v n="$1" '$1 == n { printf "%s ", ($2 == "unit" ? "--lib --bins" : "--test " $2) }' "$list" ;;
  *) echo "usage: $0 check | balance <nextest output>... | <1-4>" >&2; exit 2 ;;
esac
