#!/bin/sh
# #1783: shoot each state at 390 (DPR 2) and 1440. Usage: sh docs/design/new-issue/shoot.sh [chrome-headless-shell]
# Serves the repo root (the page loads web/dist/board.js) on 127.0.0.1:8783 for the run.
set -eu
here=$(cd "$(dirname "$0")" && pwd) root=$(cd "$here/../../.." && pwd)
b=${1:-$(ls -d "$HOME"/Library/Caches/ms-playwright/chromium_headless_shell-*/chrome-headless-shell-*/chrome-headless-shell | tail -1)}
python3 -m http.server 8783 --bind 127.0.0.1 --directory "$root" >/dev/null 2>&1 &
srv=$!
trap 'kill $srv' EXIT
for _ in 1 2 3 4 5 6 7 8 9 10; do curl -fs http://127.0.0.1:8783/ >/dev/null && break; sleep 0.2; done
u=http://127.0.0.1:8783/docs/design/new-issue/index.html
for v in new scoping draft; do
  h=844; [ "$v" = scoping ] && h=1300; [ "$v" = draft ] && h=1600
  "$b" --headless --hide-scrollbars --force-device-scale-factor=2 --window-size=390,$h --virtual-time-budget=6000 --screenshot="$here/$v-390.png" "$u?view=$v" 2>/dev/null
  "$b" --headless --hide-scrollbars --force-device-scale-factor=1 --window-size=1440,1000 --virtual-time-budget=6000 --screenshot="$here/$v-1440.png" "$u?view=$v" 2>/dev/null
done
