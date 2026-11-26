#!/usr/bin/env bash
# #1587: runs a ui-webkit gate's command with Playwright's WebKit on NODE_PATH (pinned, so the engine never moves
# without a commit). CI only: a lane's host has no WebKit, and the tests skip there in the test gate anyway.
set -euo pipefail
cd "$(dirname "$0")/.."
if [ -z "${CI:-}" ]; then echo "webkit: CI's ui-webkit leg runs it"; exit 0; fi
pw="${RUNNER_TEMP:-target}/pw"
export NODE_PATH="$pw/node_modules"
if ! node -e "require('playwright')" 2>/dev/null; then
  npm install --no-save --no-audit --no-fund --prefix "$pw" playwright@1.63.0
  npx --prefix "$pw" playwright install --with-deps webkit
  node -e "require('playwright')"
fi
exec "$@"
