#!/usr/bin/env bash
# #844: every core timing is a [limits] key: a literal `Duration::from_secs(` / `from_millis(` / `from_mins(` in
# crates/core/src/** fails, but in config.rs (its defaults) and in a `#[cfg(test)]` item.
# Usage: scripts/timing-check.sh [repo-root]
set -euo pipefail
cd "${1:-.}"
bad=$(find crates/core/src -name '*.rs' ! -path crates/core/src/config.rs | sort | xargs awk '
  FNR == 1 { skip = 0; depth = 0 }
  /^[[:space:]]*#\[cfg\(test\)\]/ { skip = 1; depth = 0; next }
  skip {
    o = gsub(/\{/, "{"); c = gsub(/\}/, "}"); depth += o - c
    if (o + c > 0 && depth <= 0 || o + c == 0 && depth == 0 && /;[[:space:]]*$/) skip = 0
    next
  }
  /^[[:space:]]*\/\// { next }
  /Duration::from_(secs|millis|mins)\([[:space:]]*[0-9]/ { print FILENAME ":" FNR ": " $0 }')
test -z "$bad" || {
  echo "::error::timing-check: a hard-coded Duration in core; make it a [limits] key (crates/core/src/config.rs, docs/config.md)"
  echo "$bad"
  exit 1
}
