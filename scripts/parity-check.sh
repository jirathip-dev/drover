#!/usr/bin/env bash
# #1094: docs/parity.md maps each #553 row (1-90, R1-R27) to its `parity_<row>` test and a status. Fails when a
# `kept` or `page-only` row's test is missing, a parity test is in no row, a status is outside the list, or (given a
# base ref) a parity test or row the base had is gone while its row does not read `dropped q<N>` (the owner's decision).
# Usage: scripts/parity-check.sh [base-ref]; run from the repo root.
set -euo pipefail
export LC_ALL=C
list=docs/parity.md base=${1:-}
names() { # the parity test names in a tree: `fn parity_<row>` in crates/cli/tests, any `parity_<row>` in the audit
  if [ -n "${1:-}" ]; then
    git grep -hoE 'fn parity_r?[0-9]+' "$1" -- 'crates/cli/tests/*.rs' ':!crates/cli/tests/parity_check.rs' || true
    git grep -hoE 'parity_r?[0-9]+' "$1" -- scripts/ui-audit.sh assets/ui-audit.js || true
  else
    grep -rhoE --include='*.rs' --exclude=parity_check.rs 'fn parity_r?[0-9]+' crates/cli/tests || true
    grep -hoE 'parity_r?[0-9]+' scripts/ui-audit.sh assets/ui-audit.js 2>/dev/null || true
  fi | sed 's/^fn //' | sort -u
}
rows() { # row|test|status for each table line whose first cell is a row number
  awk -F'|' '{ for (i = 2; i <= 5; i++) gsub(/^ +| +$/, "", $i) } $2 ~ /^R?[0-9]+$/ { print $2 "|" $4 "|" $5 }'
}
[ -f "$list" ] || { echo "parity: no $list"; exit 1; }
have=$(names) table=$(rows < "$list") bad=()
while IFS='|' read -r row test status; do
  [ -n "$row" ] || continue
  [[ $status =~ ^(kept|page-only|cooee|open( \#[0-9]+)?|dropped\ q[0-9]+)$ ]] ||
    bad+=("row $row: status \"$status\" is none of kept, page-only, cooee, open [#N], dropped q<N>")
  [[ $test == — || $test =~ ^parity_r?[0-9]+$ ]] || bad+=("row $row: test \"$test\" is neither — nor parity_<row>")
  if [[ $status =~ ^(kept|page-only)$ ]] && ! grep -qx -- "$test" <<<"$have"; then
    bad+=("row $row is $status but its test $test is gone: restore it, or mark the row \`dropped q<N>\` with the owner's decision")
  fi
done <<<"$table"
for t in $have; do
  cut -d'|' -f2 <<<"$table" | grep -qx -- "$t" || bad+=("$t is in no row of $list")
done
if [ -n "$base" ] && git rev-parse -q --verify "$base^{commit}" >/dev/null; then
  for t in $(comm -23 <(names "$base") <(echo "$have")); do
    left=$(awk -F'|' -v t="$t" '$2 == t' <<<"$table")
    if [ -z "$left" ] || grep -vqE '\|dropped q[0-9]+$' <<<"$left"; then
      bad+=("$t is deleted, but $list has no \`dropped q<N>\` row for it: restore it, or record the owner's drop")
    fi
  done
  if old=$(git show "$base:$list" 2>/dev/null); then
    for r in $(comm -23 <(rows <<<"$old" | cut -d'|' -f1 | sort) <(cut -d'|' -f1 <<<"$table" | sort)); do
      bad+=("row $r is gone from $list: a row stays, \`dropped q<N>\` at most")
    done
  fi
fi
[ ${#bad[@]} -eq 0 ] || { printf 'parity: %s\n' "${bad[@]}"; exit 1; }
echo "parity: $(grep -c . <<<"$table") rows, $(grep -c . <<<"$have") tests, all listed"
