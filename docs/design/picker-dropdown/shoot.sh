#!/bin/sh
# #1297: shoot both mockups at 390 (DPR 2, iPhone height) and 1440. Usage: sh docs/design/picker-dropdown/shoot.sh [chrome-headless-shell]
set -eu
here=$(cd "$(dirname "$0")" && pwd)
b=${1:-$(ls -d "$HOME"/Library/Caches/ms-playwright/chromium_headless_shell-*/chrome-headless-shell-*/chrome-headless-shell | tail -1)}
for p in composer form; do
  "$b" --headless --hide-scrollbars --force-device-scale-factor=2 --window-size=390,844 --screenshot="$here/$p-390.png" "file://$here/$p.html"
  "$b" --headless --hide-scrollbars --force-device-scale-factor=1 --window-size=1440,900 --screenshot="$here/$p-1440.png" "file://$here/$p.html"
done
