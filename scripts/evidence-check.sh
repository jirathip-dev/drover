#!/usr/bin/env bash
# #453: a PR body's evidence names the head it was produced at (stale evidence was the top review FAIL class).
# The body (stdin) must name the head sha (its first 7 hex), or a commit under a run of `Merge … <base>` commits on
# top: drive's auto-update catch-up merges the base without touching the body, and the premerge checks cover it.
# Usage: scripts/evidence-check.sh <head-sha> [base-branch] < body
set -euo pipefail
head=$1 base=${2:-staging} body=$(cat) c=$1
merge="^Merge ((remote-tracking )?branch )?'?(origin/)?$base'?( |\$)"
while :; do
  grep -qiE "(^|[^0-9a-f])${c:0:7}" <<<"$body" && exit 0
  [[ $(git log -1 --format=%s "$c") =~ $merge ]] || break
  c=$(git rev-parse "$c^1")
done
echo "::error::the PR body's evidence names no sha of head ${head:0:7}: re-run the gates at the head and \`gh pr edit --body\` with it"
exit 1
