#!/usr/bin/env bash
# #63 gate 2 (docs/ui.md): headless Chrome opens the exported demo (`site/`) at 390×844, DPR 3, on each view,
# saves a screenshot and asserts the page's `?audit=<view>&check=1` JSON has "outliers":[].
# Usage: scripts/ui-audit.sh [site-dir] [shot-dir]; CHROME=<binary> overrides the browser. Use chrome-headless-shell:
# Chrome's new headless keeps windows ≥ 500 px wide, and the page fails the audit unless it is 390 (#68).
set -euo pipefail
# #560: one UI audit at a time on this host (scripts/ui-lock.sh).
[ -n "${UI_AUDIT_LOCKED:-}" ] || UI_AUDIT_LOCKED=1 exec "$(dirname "$0")/ui-lock.sh" "$0" "$@"
site=${1:-site} shots=${2:-ui-390}
chrome=${CHROME:-$(command -v google-chrome || command -v chromium || command -v chromium-browser || echo "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")}
port=${UI_AUDIT_PORT:-${DROVER_PORT:-8390}}
mkdir -p "$shots" && shots=$(cd "$shots" && pwd)
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$site" >/dev/null 2>&1 &
srv=$!
trap 'kill $srv' EXIT
for _ in $(seq 50); do curl -fs "http://127.0.0.1:$port/" >/dev/null && break; sleep 0.1; done
# #71: a Chrome run that never returns (virtual time stuck) is killed after UI_AUDIT_TIMEOUT s and fails loudly,
# with its stderr tail, instead of hanging the job (`timeout` where the host has it: coreutils on the runner; else perl's alarm).
# #566: a hung run is retried twice, each on a fresh profile. Every hang goes to <shots>/hangs.txt; a run still hung
# after that is no failure: it goes to <shots>/flaky.txt (CI's soft `ui-flaky` check), and a hung check run skips the view's check.
limit=${UI_AUDIT_TIMEOUT:-60} to=$(command -v timeout || command -v gtimeout || true)
: > "$shots/hangs.txt" && : > "$shots/flaky.txt"
hung() { [ "$1" = 124 ] || [ "$1" = 137 ] || [ "$1" = 142 ]; }
run() { # url, extra args
  local rc=0 n
  for n in 1 2 3; do
    rc=0; once "$@" || rc=$?
    hung "$rc" || return "$rc"
    echo "$v ${2%%=*}: try $n hung" >> "$shots/hangs.txt"
  done
  echo "$v ${2%%=*}" >> "$shots/flaky.txt"
}
once() {
  local prof rc=0 hold=(perl -e 'alarm shift; exec @ARGV' "$limit"); prof=$(mktemp -d)
  [ -n "$to" ] && hold=("$to" -k 5 "$limit")
  "${hold[@]}" "$chrome" --headless --no-sandbox --disable-gpu --hide-scrollbars --user-data-dir="$prof" --no-first-run \
    --window-size="${size:-390,844}" --force-device-scale-factor="${dpr:-3}" --virtual-time-budget=8000 "${@:2}" "$1" 2>"$prof.err" || rc=$?
  if [ "$rc" != 0 ]; then
    echo "$v: chrome ${2%%=*} exited $rc$(hung "$rc" && echo ", killed after ${limit} s: hung")" >&2
    tail -5 "$prof.err" >&2
  fi
  rm -rf "$prof" "$prof.err"
  return "$rc"
}
fail=0
# #927: the page draws the JSON screens: each tab (its place id), `fleets` (the [⚙] screen's sheet), `lane` (a lane's
# sheet) and `card` (the first card opened); #1072: `threads` (the standalone tab) and `thread` (its first thread opened), in each built-in theme (`~<theme>`), at 1440 (`@1440`) and in every colour
# scheme (`^<scheme>`, the contact sheet). #725: `lifeboat/page`: the lifeboat's paired page (site/lifeboat/, written by
# `cargo test --test lifeboat`), shot as lifeboat-page.png.
for v in lifeboat/page queue lanes events threads fleets lane card thread queue@1440 lanes@1440 events@1440 threads@1440 \
  $(for t in kitchen garden terminal; do echo "queue~$t lanes~$t events~$t threads~$t"; done) lanes~garden^garden-dark \
  $(for s in github-dark solarized-dark gruvbox nord dracula paper farm kitchen garden-light garden-dark; do echo "queue^$s"; done); do
  [ -n "${UI_AUDIT_VIEWS:-}" ] && [[ " $UI_AUDIT_VIEWS " != *" $v "* ]] && continue # #266: only these views (space-separated)
  # #241: `<view>~<theme>` is the view in that theme (`?theme=`), shot as <view>-<theme>.png. #244: `<view>^<scheme>`
  # in that colour scheme (`?scheme=`), shot as <view>-scheme-<scheme>.png: the contact sheet, from the real CSS.
  # #337: `<view>@1440` at the desktop width (1440×900, DPR 2), shot as <view>-1440.png.
  wd=$([[ $v == *@* ]] && echo "${v#*@}" || true) v=${v%%@*} size=390,844 dpr=3
  [ -n "$wd" ] && size=$wd,900 dpr=2
  sc=$([[ $v == *^* ]] && echo "${v#*^}" || true) v=${v%%^*}
  th=$([[ $v == *~* ]] && echo "${v#*~}" || true) v=${v%%~*} flags=()
  dir=$([[ $v == */* ]] && echo "${v%/*}/" || true) view=${v##*/} v=${v//\//-}${th:+-$th}${sc:+-scheme-$sc}
  v=$v${wd:+-$wd}
  url="http://127.0.0.1:$port/${dir}index.html?audit=$view${th:+&theme=$th}${sc:+&scheme=$sc}${wd:+&w=$wd}"
  run "$url&shot=1" --screenshot="$shots/$v.png" ${flags[@]+"${flags[@]}"} >/dev/null || fail=1
  run "$url&check=1" --dump-dom ${flags[@]+"${flags[@]}"} > "$shots/$v.html" || fail=1
  if grep -qx "$v --dump-dom" "$shots/flaky.txt"; then echo "$v: hung 3 times, not checked (ui-flaky)"
  elif ! python3 - "$shots/$v.html" "$v" "$shots/$v.png" "$dpr" <<'EOF'
import html, json, os, re, struct, sys, zlib
m = re.search(r'<pre id="audit">(.*?)</pre>', open(sys.argv[1]).read(), re.S)
r = json.loads(html.unescape(m.group(1))) if m else {"outliers": ["no audit output"]}
def png(path):  # 8-bit RGB(A), non-interlaced: Chrome's screenshots
    b = open(path, "rb").read()
    i, idat, w = 8, b"", 0
    while i < len(b):
        n, t = struct.unpack(">I4s", b[i:i + 8])
        if t == b"IHDR": w, h, _, ct = struct.unpack(">IIBB", b[i + 8:i + 18])
        if t == b"IDAT": idat += b[i + 8:i + 8 + n]
        i += n + 12
    bpp, raw, rows, prev = (4 if ct == 6 else 3), zlib.decompress(idat), [], bytearray(w * (4 if ct == 6 else 3))
    for y in range(h):
        f, line = raw[y * (len(prev) + 1)], bytearray(raw[y * (len(prev) + 1) + 1:(y + 1) * (len(prev) + 1)])
        for x in range(len(line)):
            a, c, u = line[x - bpp] if x >= bpp else 0, prev[x - bpp] if x >= bpp else 0, prev[x]
            p = a + u - c
            line[x] = (line[x] + [0, a, u, (a + u) // 2, a if abs(p - a) <= abs(p - u) and abs(p - a) <= abs(p - c) else u if abs(p - u) <= abs(p - c) else c][f]) & 255
        rows.append([tuple(line[x:x + 3]) for x in range(0, len(line), bpp)])
        prev = line
    return rows
# #281: a row sprite's pixels are crisp: an art pixel is 4.5 device px (1.5 css px at DPR 3), so no device pixel inside its
# box is a blend of its two neighbours (smoothing paints those along every edge).
if r.get("sprites") and os.path.exists(sys.argv[3]):
    px, blend, d = png(sys.argv[3]), 0, int(sys.argv[4])  # d: device px per css px (#266: 2 at 1440)
    mid = lambda a, p, b: a != b and p not in (a, b) and all(min(x, z) <= y <= max(x, z) for x, y, z in zip(a, p, b))
    for x0, y0, w, h in r["sprites"]:
        for y in range(round(y0 * d) + 1, round((y0 + h) * d) - 1):
            for x in range(round(x0 * d) + 1, round((x0 + w) * d) - 1):
                blend += mid(px[y][x - 1], px[y][x], px[y][x + 1]) or mid(px[y - 1][x], px[y][x], px[y + 1][x])
    print(f"{sys.argv[2]}: {len(r['sprites'])} row sprites, {blend} smoothed pixels")
    if blend: r["outliers"].append(f"{blend} smoothed pixels in the row sprites")
print(f"{r.get('view', sys.argv[2])}: {r.get('controls', 0)} controls, {len(r['outliers'])} outliers")
for o in r["outliers"]: print("  " + o)
sys.exit(1 if r["outliers"] else 0)
EOF
  then fail=1; fi
  rm "$shots/$v.html"
done
exit $fail
