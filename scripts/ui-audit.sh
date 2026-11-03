#!/usr/bin/env bash
# #63 gate 2 (docs/ui.md): headless Chrome opens the exported demo (`site/`) at 390×844, DPR 3, on each view,
# saves a screenshot and asserts the page's `?audit=<view>&check=1` JSON has "outliers":[].
# Usage: scripts/ui-audit.sh [site-dir] [shot-dir]; CHROME=<binary> overrides the browser. Use chrome-headless-shell:
# Chrome's new headless keeps windows ≥ 500 px wide, and the page fails the audit unless it is 390 (#68).
set -euo pipefail
site=${1:-site} shots=${2:-ui-390}
chrome=${CHROME:-$(command -v google-chrome || command -v chromium || command -v chromium-browser || echo "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")}
port=${UI_AUDIT_PORT:-${HERDSMAN_PORT:-8390}}
mkdir -p "$shots" && shots=$(cd "$shots" && pwd)
python3 -m http.server "$port" --bind 127.0.0.1 --directory "$site" >/dev/null 2>&1 &
srv=$!
trap 'kill $srv' EXIT
for _ in $(seq 50); do curl -fs "http://127.0.0.1:$port/" >/dev/null && break; sleep 0.1; done
# #71: a Chrome run that never returns (virtual time stuck) is killed after UI_AUDIT_TIMEOUT s and fails loudly,
# with its stderr tail, instead of hanging the job (`timeout` where the host has it: coreutils on the runner).
limit=${UI_AUDIT_TIMEOUT:-60} to=$(command -v timeout || command -v gtimeout || true)
run() { # url, extra args
  local prof rc=0; prof=$(mktemp -d)
  ${to:+"$to" -k 5 "$limit"} "$chrome" --headless --no-sandbox --disable-gpu --hide-scrollbars --user-data-dir="$prof" --no-first-run \
    --window-size=390,844 --force-device-scale-factor=3 --virtual-time-budget=8000 "${@:2}" "$1" 2>"$prof.err" || rc=$?
  if [ "$rc" != 0 ]; then
    echo "$v: chrome ${2%%=*} exited $rc$([ "$rc" = 124 ] || [ "$rc" = 137 ] && echo ", killed after ${limit} s: hung")" >&2
    tail -5 "$prof.err" >&2
  fi
  rm -rf "$prof" "$prof.err"
  return "$rc"
}
fail=0
# #141: `busy/host-ranked` is the `host-ranked` view of <site>/busy (scripts/host-fixtures.sh), shot as busy-host-ranked.png.
# #157: `hosts/host` and `hosts/sheet`: the host cards (one offline) and a lane card naming its host; #234 `hosts/host-chart`:
# mini's load/cpu chart open.
for v in queue queue-open queue-lanes queue-change queue-drive queue-result config lanes lanes-now sheet sheet-full sheet-parked sheet-other sheet-drag host busy/host-ranked healthy/host-ranked hosts/host hosts/host-chart hosts/sheet events pipeline pipeline-custom herd1 herd2 herd-kitchen herd-kitchen-busy herd-kitchen-held herd-kitchen-still herd-nodone herd-night herd-night-storm herd-scroll herd-nav queue-scroll toast needs-later needs-pending \
  queue-rows events-rows queue-rows~kitchen events-rows~kitchen queue~kitchen lanes~kitchen host~kitchen events~kitchen queue~terminal lanes~terminal lanes-now~terminal host~terminal events~terminal theme-switch moment-still loading loading~kitchen loading~terminal loading-still \
  $(for s in github-dark solarized-dark gruvbox nord dracula paper farm kitchen; do echo "queue^$s busy/host-ranked^$s"; done); do
  # #241: `<view>~<theme>` is the view in that theme (`?theme=`), shot as <view>-<theme>.png. #244: `<view>^<scheme>`
  # in that colour scheme (`?scheme=`), shot as <view>-scheme-<scheme>.png: the contact sheet, from the real CSS.
  sc=$([[ $v == *^* ]] && echo "${v#*^}" || true) v=${v%%^*}
  th=$([[ $v == *~* ]] && echo "${v#*~}" || true) v=${v%%~*} flags=()
  dir=$([[ $v == */* ]] && echo "${v%/*}/" || true) view=${v##*/} v=${v//\//-}${th:+-$th}${sc:+-scheme-$sc}
  url="http://127.0.0.1:$port/${dir}index.html?audit=$view${th:+&theme=$th}${sc:+&scheme=$sc}"
  case $v in herd-kitchen*) url+="&theme=kitchen" ;; herd-nodone) url+="&theme=kitchen&nopose=done" ;; esac # #172, #239
  case $v in moment-still|herd-kitchen-still|herd-nav|loading-still) flags=(--force-prefers-reduced-motion) ;; esac # #241, #239, #291, #305
  run "$url" --screenshot="$shots/$v.png" ${flags[@]+"${flags[@]}"} >/dev/null || fail=1
  run "$url&check=1" --dump-dom ${flags[@]+"${flags[@]}"} > "$shots/$v.html" || fail=1
  if ! python3 - "$shots/$v.html" "$v" "$shots/$v.png" <<'EOF'
import html, json, re, struct, sys, zlib
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
if r.get("sprites"):
    px, blend = png(sys.argv[3]), 0
    mid = lambda a, p, b: a != b and p not in (a, b) and all(min(x, z) <= y <= max(x, z) for x, y, z in zip(a, p, b))
    for x0, y0, w, h in r["sprites"]:
        for y in range(round(y0 * 3) + 1, round((y0 + h) * 3) - 1):
            for x in range(round(x0 * 3) + 1, round((x0 + w) * 3) - 1):
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
# #166: the chat sheet opened and swipe-closed under a CDP screencast: no near-black frame, the page never moves.
${to:+"$to" -k 5 90} node "$(dirname "$0")/ui-close.mjs" "http://127.0.0.1:$port/index.html" "$chrome" "$shots" || fail=1
exit $fail
