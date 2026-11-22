#!/usr/bin/env bash
# #1586: the path-gated ci jobs a pull request runs. Reads the changed file names on stdin; prints one
# `<output>=true|false` line per gated job for $GITHUB_OUTPUT. Pushes run every job; ci.yml decides that, not this.
# webkit (#862, #927, #1312): the page and its own tests. release (#1586): what release-tests builds and runs.
set -euo pipefail
changed=$(cat)
on() { if grep -qE "$1" <<<"$changed"; then echo true; else echo false; fi; }
echo "webkit=$(on '^(web/|assets/|crates/server/|crates/cli/tests/(support/webview\.mjs|perf\.rs|url_links\.rs|parity_webkit\.rs|page_drafts\.rs|keyboard_webkit\.rs|ui/(build|drafts|repaint|compose|timings|keyboard|answered|optimistic|settled|pages|revive)\.mjs|threads_tab\.rs|herd\.rs|issue_sheet_webkit\.rs|phone_timings\.rs|screen_pages\.rs|common/large\.rs))')"
echo "release=$(on '^(crates/|Cargo\.toml$|Cargo\.lock$|rust-toolchain\.toml$|assets/|demo/|web/dist/)')"
