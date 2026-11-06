#!/usr/bin/env bash
# #552: the old board takes bug fixes only. A PR's changed files (stdin, one per line) may touch the board's UI
# files only when its issue's spec (the file argument: body plus trusted amendments) has a line starting `board-fix`.
# /api/v1 (crates/server/src/api.rs, serve/v1.rs) stays open: cooee and the drover plugins read it.
# Usage: scripts/board-freeze.sh <spec-file> < changed-files
set -euo pipefail
spec=${1:?usage: board-freeze.sh <spec-file> < changed-files}
frozen=()
while IFS= read -r f; do
  case $f in
    crates/server/src/serve/v1.rs) ;;
    assets/serve.js | assets/serve.css | assets/herd.* | web/src/* | web/dist/* | assets/themes/* | \
      crates/server/src/serve.rs | crates/server/src/serve/*) frozen+=("$f") ;;
  esac
done
((${#frozen[@]})) || exit 0
grep -qE '^[[:space:]]*([-*#]+[[:space:]]+)?(\*\*Amendment[^*]*\*\*:?[[:space:]]*)?`?board-fix' "$spec" 2>/dev/null && exit 0
echo "::error::board-freeze: the old board takes bug fixes only (board-fix in the issue); new UI goes to cooee or the drover plugins"
printf '  %s\n' "${frozen[@]}"
exit 1
