#!/usr/bin/env bash
# #1587: runs a ui gate's command with CHROME set to the pinned chrome-headless-shell (Google's Chrome-for-Testing;
# #68: Chrome's new headless never makes a window under 500 px wide, the headless shell honours 390). CI only.
set -euo pipefail
cd "$(dirname "$0")/.."
if [ -z "${CI:-}" ]; then echo "chrome: CI's ui leg runs it"; exit 0; fi
CHROME=$(npx -y @puppeteer/browsers@2.6.1 install chrome-headless-shell@156.0.8078.80 --path "${RUNNER_TEMP:-target}/chs" | tail -1 | cut -d' ' -f2-)
test -x "$CHROME" || { echo "chrome-headless-shell install gave no binary: '$CHROME'"; exit 1; }
export CHROME
exec "$@"
