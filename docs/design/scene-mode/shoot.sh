#!/usr/bin/env bash
# Renders every image of this design: `docs/design/scene-mode/shoot.sh` from anywhere. CHROME=<chrome-headless-shell>
# overrides the browser (Playwright's build); clip.gif comes from gif.mjs (node, no dependency). Serves the repo root on 127.0.0.1:8395.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd) root=$(cd "$here/../../.." && pwd)
chrome=${CHROME:-$(ls -d "$HOME"/Library/Caches/ms-playwright/chromium_headless_shell-*/chrome-headless-shell-*/chrome-headless-shell | tail -1)}
port=8395 base="http://127.0.0.1:8395/docs/design/scene-mode/index.html"
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$root" >/dev/null 2>&1 &
srv=$!
trap 'kill $srv' EXIT
for _ in $(seq 50); do curl -fs "http://127.0.0.1:$port/" >/dev/null && break; sleep 0.1; done
shot() { # name, query, width, height, dpr
  local prof; prof=$(mktemp -d)
  "$chrome" --headless --no-sandbox --disable-gpu --hide-scrollbars --user-data-dir="$prof" --no-first-run \
    --window-size="$3,$4" --force-device-scale-factor="$5" --virtual-time-budget=4000 --screenshot="$here/$1.png" "$base?$2" 2>/dev/null
  rm -rf "$prof"
}
for w in 390 1440; do
  h=$([ $w = 390 ] && echo 844 || echo 900) d=$([ $w = 390 ] && echo 2 || echo 1)
  shot "scene-pixel-$w" "style=pixel&light=day&t=1.3" $w $h $d
  shot "scene-ascii-$w" "style=ascii&light=night&t=1.3" $w $h $d
  shot "look-sheet-$w" "mode=screens&sheet=look&style=pixel" $w $h $d
  shot "old-herd-pixel-$w" "old=1&style=pixel&light=day&t=1.3" $w $h $d
  shot "old-herd-ascii-$w" "old=1&style=pixel&light=night&t=1.3" $w $h $d
done
mkdir -p "$here/.frames" && rm -f "$here/.frames"/*.png
n=0
for q in "style=pixel&light=day&loading=1" "style=pixel&light=day" "style=ascii&light=night&loading=1" "style=ascii&light=night"; do
  k=$([[ $q == *loading* ]] && echo 8 || echo 30)
  for i in $(seq 0 $((k - 1))); do
    t=$(python3 -c "print(round($i/12,3))")
    base_q="$q&t=$t" ; f=$(printf '%03d' $n)
    shot ".frames/$f" "$base_q" 390 844 1
    n=$((n + 1))
  done
done
node "$here/gif.mjs" "$here/clip.gif" 12 "$here"/.frames/*.png
rm -rf "$here/.frames"
