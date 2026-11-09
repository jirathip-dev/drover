#!/usr/bin/env bash
# #720: each drover tab at 390×844 (DPR 3) in headless Chrome, as cooee's frame runs it: the built module (dist/) after
# a stand-in `cooee` that answers `/api/v1/*` from the recorded fixtures/ (every op READ_ONLY) and cooee's dark colours.
# Usage: web/plugin/scripts/shots.sh <shot-dir> [dist-dir]; CHROME=<binary> (chrome-headless-shell: 390 px wide).
set -euo pipefail
here=$(cd "$(dirname "$0")/.." && pwd)
out=$(mkdir -p "$1" && cd "$1" && pwd) dist=$(cd "${2:-$here/dist}" && pwd)
for tab in queue lanes asks fleets; do
  page="$out/plugin-$tab.html"
  node - "$here/fixtures" "$dist/$tab.js" >"$page" <<'EOF'
const fs = require("node:fs"), [dir, js] = process.argv.slice(2);
const fx = Object.fromEntries(fs.readdirSync(dir).map((f) => [f.replace(".json", ""), JSON.parse(fs.readFileSync(`${dir}/${f}`))]));
// fixture-herdsman's routes (cooee-drover's verify recipe): the open ask in `needs`, the closed ones after `events`.
fx.events = { ...fx.events, list: [...fx.events.list, ...fx.endings.list.map((e, i) => ({ ...e, seq: fx.events.seq + 1 + i }))] };
const api = `const fx=${JSON.stringify(fx)};
const file=(p)=>{const [n,...r]=p.split("?")[0].split("/");return n==="lessons"?(r.length?r[1]==="digest"&&"digest":"lessons"):["state","queue","needs","host","events","issue"].includes(n)&&n};
globalThis.cooee={async fetch(path,init){const f=path.startsWith("/api/v1/")&&!init?.method&&file(path.slice(8));
if(init?.method==="POST")return{status:405,ok:false,type:"application/json",text:'{"ok":false,"code":"READ_ONLY","message":"the fixture herdsman takes no ops"}'};
return f?{status:200,ok:true,type:"application/json",text:JSON.stringify(fx[f])}:{status:404,ok:false,type:"text/plain",text:"no such path"}},
events(){return()=>{}}};`;
const vars = "--bg:#0d1117;--fg:#e6edf3;--card:#161b22;--edge:#30363d;--mute:#8b949e;--link:#58a6ff;--g:#3fb950;--a:#d29922;--r:#f85149";
process.stdout.write(`<!doctype html><html style="${vars};background:var(--bg);color:var(--fg);font:15px -apple-system,sans-serif"><meta charset=utf-8><body>
<script>${api}</script><script type=module>${fs.readFileSync(js, "utf8").replaceAll("</script", "<\\/script")}</script></body></html>`);
EOF
  "${CHROME:?}" --headless --no-sandbox --disable-gpu --hide-scrollbars --user-data-dir="$(mktemp -d)" --no-first-run \
    --window-size=390,844 --force-device-scale-factor=3 --virtual-time-budget=4000 \
    --screenshot="$out/plugin-$tab.png" "file://$page" 2>/dev/null
  test -s "$out/plugin-$tab.png" && echo "plugin-$tab.png ok"
done
