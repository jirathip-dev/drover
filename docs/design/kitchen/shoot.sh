#!/usr/bin/env bash
# #239: regenerate the kitchen mockup's files: `node build.mjs`, then the PNGs with chrome-headless-shell
# (CHROME=<binary>; Chrome's new headless keeps windows ≥ 500 px wide). Run from the repo root.
set -euo pipefail
d=docs/design/kitchen port=${PORT:-8392}
chrome=${CHROME:-$(command -v chrome-headless-shell || command -v google-chrome || command -v chromium)}
(cd "$d" && node build.mjs)
python3 -m http.server "$port" --bind 127.0.0.1 >/dev/null 2>&1 &
srv=$!
trap 'kill $srv' EXIT
for _ in $(seq 50); do curl -fs "http://127.0.0.1:$port/" >/dev/null && break; sleep 0.1; done
shot() { # path out width height dpr
  local prof; prof=$(mktemp -d)
  "$chrome" --headless --no-sandbox --hide-scrollbars --user-data-dir="$prof" --window-size="$3,$4" \
    --force-device-scale-factor="$5" --virtual-time-budget=3000 --screenshot="$2" "http://127.0.0.1:$port/$1" >/dev/null 2>&1
  rm -rf "$prof"
}
for n in 180 192 512; do shot "$d/icon/icon-$n.svg" "$d/icon/icon-$n.png" "$n" "$n" 1; done
shot "$d/icon/favicon.svg" "$d/icon/favicon-32.png" 32 32 1
sed 's/width="32" height="32"/width="16" height="16"/' "$d/icon/favicon.svg" > "$d/icon/f16.svg"
shot "$d/icon/f16.svg" "$d/icon/favicon-16.png" 16 16 1 && rm "$d/icon/f16.svg"
# The page heights as rendered on 2026-11-03; a taller window only adds background below.
shot "$d/index.html" "$d/kitchen-390.png" 390 13340 2
shot "$d/index.html" "$d/kitchen-1440.png" 1440 8350 1
shot "$d/sheet.html" "$d/sprite-sheet.png" 960 1486 2
